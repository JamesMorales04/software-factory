//! Trusted Worker: Core lease coordination and Supervisor-mediated execution.
//!
//! The Worker never mounts or executes a project directly. It asks the trusted
//! Supervisor for a durable per-Attempt workspace, sends a bounded command into
//! an allowlisted rootless Podman sandbox, and reports the exact candidate. No
//! provider credentials, Podman socket or host workspace path reach the
//! sandbox.

use std::{env, time::Duration};

use factory_native::db::ActionSpec;
use factory_native::execution::{
    ExecuteWorkspace, ExecutionResult, WorkspaceAllocation, WorkspaceHandle, WorkspaceStatus,
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CORE_TIMEOUT: Duration = Duration::from_secs(30);
const SUPERVISOR_TIMEOUT: Duration = Duration::from_secs(300);

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

struct Supervisor {
    client: reqwest::Client,
    base: reqwest::Url,
    token: String,
    image: String,
    timeout_secs: u64,
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
    let core = validated_loopback_url(
        &env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into()),
        "FACTORY_CORE_URL",
    )?;
    let supervisor = Supervisor {
        client: reqwest::Client::builder()
            .timeout(SUPERVISOR_TIMEOUT)
            .build()?,
        base: validated_loopback_url(
            &env::var("FACTORY_SUPERVISOR_URL").map_err(|_| "FACTORY_SUPERVISOR_URL required")?,
            "FACTORY_SUPERVISOR_URL",
        )?,
        token: env::var("FACTORY_SUPERVISOR_TOKEN")
            .map_err(|_| "FACTORY_SUPERVISOR_TOKEN required")?,
        image: env::var("FACTORY_WORKER_FIXTURE_IMAGE").unwrap_or_else(|_| "alpine:3.20".into()),
        timeout_secs: env::var("FACTORY_WORKER_FIXTURE_TIMEOUT_SECS")
            .unwrap_or_else(|_| "120".into())
            .parse()?,
    };
    if supervisor.token.len() < 32 {
        return Err("FACTORY_SUPERVISOR_TOKEN must be at least 32 bytes".into());
    }
    if supervisor.timeout_secs == 0 || supervisor.timeout_secs > 600 {
        return Err("FACTORY_WORKER_FIXTURE_TIMEOUT_SECS must be between 1 and 600".into());
    }
    let once = env::var("FACTORY_WORKER_ONCE").ok().as_deref() == Some("1");
    let client = reqwest::Client::builder().timeout(CORE_TIMEOUT).build()?;
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
        let heartbeat_client = client.clone();
        let heartbeat_url = core.join("worker/heartbeat")?;
        let heartbeat_token = session_token.clone();
        let heartbeat_worker = worker_id.clone();
        let heartbeat_attempt = assignment.lease.attempt_id.clone();
        let heartbeat_generation = assignment.lease.generation;
        let heartbeat = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(20));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let response = heartbeat_client
                    .post(heartbeat_url.clone())
                    .bearer_auth(&heartbeat_token)
                    .json(&json!({
                        "attempt_id": heartbeat_attempt,
                        "worker_id": heartbeat_worker,
                        "generation": heartbeat_generation
                    }))
                    .send()
                    .await;
                if response
                    .map(|value| !value.status().is_success())
                    .unwrap_or(true)
                {
                    return;
                }
            }
        });
        let execution = execute_assignment(&assignment, &supervisor).await;
        heartbeat.abort();
        match execution {
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
                    "sandbox candidate accepted for action {}",
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
                eprintln!("sandbox execution failed: {message}");
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

async fn execute_assignment(
    assignment: &Assignment,
    supervisor: &Supervisor,
) -> Result<factory_native::workspace::CandidateRevision, Box<dyn std::error::Error>> {
    let file =
        env::var("FACTORY_FIXTURE_FILE").unwrap_or_else(|_| "factory-fixture-change.txt".into());
    if !factory_native::lead::is_safe_relative_path(&file) {
        return Err("fixture file path is unsafe".into());
    }
    let content = env::var("FACTORY_FIXTURE_CONTENT").unwrap_or_else(|_| {
        format!(
            "fixture candidate\nobjective: {}\n",
            assignment.action_spec.objective
        )
    });
    let allocation = WorkspaceAllocation {
        attempt_id: assignment.lease.attempt_id.clone(),
        repository: assignment.repository.clone(),
        base_commit: assignment.action_spec.base_commit.clone(),
    };
    let response = supervisor
        .client
        .post(supervisor.base.join("v1/workspaces")?)
        .bearer_auth(&supervisor.token)
        .json(&allocation)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Supervisor refused workspace allocation: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    let handle: WorkspaceHandle = response.json().await?;
    if handle.status == WorkspaceStatus::Completed {
        return handle
            .candidate
            .ok_or_else(|| "completed workspace has no candidate".into());
    }
    let request = ExecuteWorkspace {
        image: supervisor.image.clone(),
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "mkdir -p \"$(dirname \"$2\")\" && printf '%s' \"$1\" > \"$2\"".into(),
            "factory-fixture".into(),
            content,
            file,
        ],
        timeout_secs: supervisor.timeout_secs,
    };
    let response = supervisor
        .client
        .post(
            supervisor
                .base
                .join(&format!("v1/workspaces/{}/execute", handle.workspace_id))?,
        )
        .bearer_auth(&supervisor.token)
        .json(&request)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let _ = cleanup(supervisor, &handle.workspace_id).await;
        return Err(format!("Supervisor execution failed: {status} {body}").into());
    }
    let result: ExecutionResult = response.json().await?;
    let cleanup_result = cleanup(supervisor, &handle.workspace_id).await;
    let Some(candidate) = result.candidate else {
        let stderr = result
            .stderr
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!("sandbox exited with {:?}: {stderr}", result.exit_code).into());
    };
    if let Err(error) = cleanup_result {
        eprintln!("Supervisor workspace cleanup failed: {error}");
    }
    Ok(candidate)
}

async fn cleanup(
    supervisor: &Supervisor,
    workspace_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = supervisor
        .client
        .delete(
            supervisor
                .base
                .join(&format!("v1/workspaces/{workspace_id}"))?,
        )
        .bearer_auth(&supervisor.token)
        .send()
        .await?;
    if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    Err(format!("Supervisor cleanup returned {}", response.status()).into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validated_loopback_url(
    raw: &str,
    variable: &str,
) -> Result<reqwest::Url, Box<dyn std::error::Error>> {
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
        return Err(format!("{variable} must be a loopback HTTP origin with explicit port").into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::validated_loopback_url;

    #[test]
    fn rejects_userinfo_host_bypass() {
        assert!(validated_loopback_url("http://127.0.0.1:x@attacker.example", "TEST").is_err());
        assert!(validated_loopback_url("http://localhost:8765", "TEST").is_ok());
        assert!(validated_loopback_url("http://127.0.0.1:8765/path", "TEST").is_err());
    }
}
