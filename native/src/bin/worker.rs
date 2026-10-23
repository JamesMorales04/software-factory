use std::{env, path::Path, process::Command, time::Duration};

use factory_native::db::ActionSpec;
use factory_native::workspace::WorkspaceAllocator;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ClaimResponse {
    dispatch_supported: bool,
    assignment: Option<Assignment>,
}

#[derive(Debug, Deserialize)]
struct Assignment {
    lease: Lease,
    action_id: String,
    action_spec: ActionSpec,
    action_spec_sha256: String,
    repository: String,
    execution_mode: String,
}

#[derive(Debug, Deserialize)]
struct Lease {
    attempt_id: String,
    generation: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap_token = env::var("FACTORY_WORKER_BOOTSTRAP_TOKEN")
        .map_err(|_| "FACTORY_WORKER_BOOTSTRAP_TOKEN required")?;
    let worker_id = env::var("FACTORY_WORKER_ID").map_err(|_| "FACTORY_WORKER_ID required")?;
    let registration_key = env::var("FACTORY_WORKER_REGISTRATION_KEY")
        .map_err(|_| "FACTORY_WORKER_REGISTRATION_KEY required")?;
    if bootstrap_token.is_empty() || worker_id.trim().is_empty() || registration_key.is_empty() {
        return Err(
            "Worker bootstrap token, identity and registration key must not be empty".into(),
        );
    }
    let core = env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let core = validated_core_url(&core)?;
    let workspace_root = env::var("FACTORY_WORKSPACE_ROOT")
        .map_err(|_| "FACTORY_WORKSPACE_ROOT required for fixture execution")?;
    let workspace_root = Path::new(&workspace_root).to_path_buf();
    let once = env::var("FACTORY_WORKER_ONCE").ok().as_deref() == Some("1");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client
        .post(core.join("worker/register")?)
        .bearer_auth(bootstrap_token)
        .json(&json!({"worker_id":worker_id,"registration_key":registration_key}))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Core refused Worker registration: {}", response.status()).into());
    }
    let body: serde_json::Value = response.json().await?;
    let session_token = body
        .get("session_token")
        .and_then(serde_json::Value::as_str)
        .ok_or("Core did not return a Worker session credential")?
        .to_owned();
    if body.get("worker_id").and_then(serde_json::Value::as_str) != Some(worker_id.as_str()) {
        return Err("Core returned a different Worker identity".into());
    }

    loop {
        let response = client
            .post(core.join("worker/claim")?)
            .bearer_auth(&session_token)
            .json(&json!({"worker_id":worker_id}))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(format!("Core refused Worker claim: {}", response.status()).into());
        }
        let claim: ClaimResponse = response.json().await?;
        if !claim.dispatch_supported {
            return Err("Core reports dispatch unsupported for this Worker".into());
        }
        let Some(assignment) = claim.assignment else {
            if once {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        if assignment.execution_mode != "FIXTURE" {
            return Err(format!(
                "Worker refuses unverified execution mode {}",
                assignment.execution_mode
            )
            .into());
        }
        match execute_fixture(&assignment, &workspace_root).await {
            Ok(candidate) => {
                let response = client
                    .post(core.join("worker/candidate")?)
                    .bearer_auth(&session_token)
                    .json(&json!({
                        "attempt_id": assignment.lease.attempt_id,
                        "worker_id": worker_id,
                        "generation": assignment.lease.generation,
                        "action_spec_sha256": assignment.action_spec_sha256,
                        "commit_sha": candidate.commit_sha,
                        "tree_sha": candidate.tree_sha
                    }))
                    .send()
                    .await?;
                if !response.status().is_success() {
                    return Err(format!(
                        "Core rejected candidate: {} {}",
                        response.status(),
                        response.text().await.unwrap_or_default()
                    )
                    .into());
                }
                println!(
                    "fixture candidate accepted for action {}",
                    assignment.action_id
                );
            }
            Err(error) => {
                let message = error.to_string();
                let _ = client
                    .post(core.join("worker/report")?)
                    .bearer_auth(&session_token)
                    .json(&json!({
                        "attempt_id": assignment.lease.attempt_id,
                        "worker_id": worker_id,
                        "generation": assignment.lease.generation,
                        "receipt_id": Uuid::new_v4().to_string(),
                        "result_sha256": sha256_hex(message.as_bytes()),
                        "terminal_status": "FAILED"
                    }))
                    .send()
                    .await;
                eprintln!("fixture execution failed: {message}");
                if once {
                    return Err(error);
                }
            }
        }
        if once {
            return Ok(());
        }
    }
}

async fn execute_fixture(
    assignment: &Assignment,
    workspace_root: &Path,
) -> Result<factory_native::workspace::CandidateRevision, Box<dyn std::error::Error>> {
    let allocator = WorkspaceAllocator::new(&assignment.repository, workspace_root)?;
    let mut workspace = allocator.allocate(
        &assignment.lease.attempt_id,
        &assignment.action_spec.base_commit,
    )?;
    let result = (|| -> Result<_, Box<dyn std::error::Error>> {
        let file = env::var("FACTORY_FIXTURE_FILE")
            .unwrap_or_else(|_| "factory-fixture-change.txt".into());
        if !factory_native::lead::is_safe_relative_path(&file) {
            return Err("fixture file path is unsafe".into());
        }
        let content = env::var("FACTORY_FIXTURE_CONTENT").unwrap_or_else(|_| {
            format!(
                "fixture candidate\nobjective: {}\n",
                assignment.action_spec.objective
            )
        });
        let target = workspace.path().join(&file);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, content)?;
        run_git(workspace.path(), &["add", &file])?;
        let message = format!("fixture candidate for {}", assignment.action_id);
        run_git(
            workspace.path(),
            &[
                "-c",
                "user.name=Factory Fixture Worker",
                "-c",
                "user.email=fixture-worker@example.invalid",
                "commit",
                "-qm",
                &message,
            ],
        )?;
        Ok(workspace.candidate_revision()?)
    })();
    let cleanup = workspace.cleanup();
    match (result, cleanup) {
        (Ok(candidate), Ok(())) => Ok(candidate),
        (Ok(_), Err(error)) => Err(format!("workspace cleanup failed: {error}").into()),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; workspace cleanup failed: {cleanup}").into())
        }
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validated_core_url(raw: &str) -> Result<reqwest::Url, Box<dyn std::error::Error>> {
    let url = reqwest::Url::parse(raw)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("FACTORY_CORE_URL must be a loopback HTTP origin with explicit port".into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::validated_core_url;

    #[test]
    fn rejects_userinfo_host_bypass() {
        assert!(validated_core_url("http://127.0.0.1:x@attacker.example").is_err());
        assert!(validated_core_url("http://localhost:8765").is_ok());
    }
}
