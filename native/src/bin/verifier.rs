//! Independent verifier process.
//!
//! Fetches a candidate inspection from Core, re-verifies Git identity in a
//! clean worktree, executes the frozen ActionSpec checks and submits a
//! candidate-bound verifier receipt. It does not trust the Worker's claim and
//! does not start a provider session.

use std::{env, path::Path, process::Stdio, time::Duration};

use factory_native::db::CandidateInspection;
use factory_native::lead::SpecCheck;
use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let core = env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let core = validated_core_url(&core)?;
    let token =
        env::var("FACTORY_VERIFIER_TOKEN").map_err(|_| "FACTORY_VERIFIER_TOKEN required")?;
    let candidate_id =
        env::var("FACTORY_CANDIDATE_ID").map_err(|_| "FACTORY_CANDIDATE_ID required")?;
    let principal =
        env::var("FACTORY_VERIFIER_PRINCIPAL").unwrap_or_else(|_| "verifier-process".into());
    let workspace_root = env::var("FACTORY_VERIFIER_WORKSPACE")
        .map_err(|_| "FACTORY_VERIFIER_WORKSPACE required")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let inspection: CandidateInspection = client
        .get(core.join(&format!("quality/candidates/{candidate_id}/inspection"))?)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let result = verify_candidate(&inspection, Path::new(&workspace_root)).await;
    let outcome = match &result {
        Ok(()) => "PASS",
        Err(_) => "FAIL",
    };
    if inspection.action_spec.checks.is_empty() {
        return Err("candidate ActionSpec has no executable checks; refusing PASS".into());
    }
    let receipt = client
        .post(core.join("quality/receipts")?)
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": Uuid::new_v4().to_string(),
            "principal_id": principal,
            "actionSpecSha256": inspection.action_spec_sha256,
            "commitSha": inspection.candidate.commit_sha,
            "treeSha": inspection.candidate.tree_sha,
            "result": outcome,
            "evidence": "EXECUTED"
        }))
        .send()
        .await?;
    if !receipt.status().is_success() {
        return Err(format!(
            "Core rejected verifier receipt: {} {}",
            receipt.status(),
            receipt.text().await.unwrap_or_default()
        )
        .into());
    }
    match result {
        Ok(()) => {
            println!("verifier PASS for {}", inspection.candidate.commit_sha);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn verify_candidate(
    inspection: &CandidateInspection,
    workspace_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = Path::new(&inspection.repository);
    verify_git_identity(
        repository,
        &inspection.base_commit,
        &inspection.candidate.commit_sha,
        &inspection.candidate.tree_sha,
    )?;
    std::fs::create_dir_all(workspace_root)?;
    let worktree = workspace_root.join(format!("verify-{}", Uuid::new_v4()));
    if worktree.exists() {
        return Err("verifier worktree path already exists".into());
    }
    git(
        repository,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.to_string_lossy().as_ref(),
            &inspection.candidate.commit_sha,
        ],
    )?;
    let checks = inspection.action_spec.checks.clone();
    let result = run_checks(&worktree, &checks).await;
    let cleanup = git(
        repository,
        &[
            "worktree",
            "remove",
            "--force",
            worktree.to_string_lossy().as_ref(),
        ],
    );
    if worktree.exists() {
        let _ = std::fs::remove_dir_all(&worktree);
    }
    match (result, cleanup) {
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(error)) => Err(format!("verifier cleanup failed: {error}").into()),
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; verifier cleanup failed: {cleanup}").into())
        }
    }
}

async fn run_checks(
    worktree: &Path,
    checks: &[SpecCheck],
) -> Result<(), Box<dyn std::error::Error>> {
    for check in checks {
        let Some(executable) = check.command.first() else {
            return Err(format!("check {} has no executable", check.id).into());
        };
        let mut command = tokio::process::Command::new(executable);
        command
            .args(&check.command[1..])
            .current_dir(worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = tokio::time::timeout(
            Duration::from_secs(u64::from(check.timeout_secs)),
            command.output(),
        )
        .await
        .map_err(|_| format!("check {} timed out", check.id))?
        .map_err(|error| format!("check {} failed to start: {error}", check.id))?;
        let code = output.status.code().unwrap_or(-1);
        if code != check.expect_exit {
            return Err(format!(
                "check {} exited {code}, expected {}: {}",
                check.id,
                check.expect_exit,
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_git_identity(
    repository: &Path,
    base: &str,
    commit: &str,
    tree: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let actual_tree = git(repository, &["show", "-s", "--format=%T", commit])?;
    if actual_tree != tree {
        return Err("candidate tree does not match the claimed commit".into());
    }
    let parents = git(repository, &["show", "-s", "--format=%P", commit])?;
    let mut parents = parents.split_whitespace();
    if parents.next() != Some(base) || parents.next().is_some() {
        return Err("candidate is not a single-parent child of the frozen base".into());
    }
    Ok(())
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
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
    use super::*;
    use factory_native::lead::CheckKind;

    fn check(id: &str, command: Vec<&str>, expect_exit: i32) -> SpecCheck {
        SpecCheck {
            id: id.into(),
            kind: CheckKind::Functional,
            description: id.into(),
            command: command.into_iter().map(str::to_owned).collect(),
            expect_exit,
            timeout_secs: 30,
        }
    }

    #[tokio::test]
    async fn checks_execute_argv_and_honor_expected_exit() {
        let root = std::env::temp_dir().join(format!("factory-verifier-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("candidate.txt"), "expected content\n").unwrap();
        run_checks(
            &root,
            &[check(
                "present",
                vec!["grep", "-q", "expected content", "candidate.txt"],
                0,
            )],
        )
        .await
        .unwrap();
        assert!(
            run_checks(
                &root,
                &[check(
                    "wrong-exit",
                    vec!["grep", "-q", "expected content", "candidate.txt"],
                    1,
                )],
            )
            .await
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn git_identity_requires_single_parent_on_frozen_base() {
        let root = std::env::temp_dir().join(format!("factory-verifier-git-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Verifier Test"],
            vec!["config", "user.email", "verifier@example.invalid"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("seed"), "base\n").unwrap();
        for arguments in [vec!["add", "seed"], vec!["commit", "-qm", "base"]] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let base = git(&root, &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(root.join("seed"), "candidate\n").unwrap();
        for arguments in [vec!["add", "seed"], vec!["commit", "-qm", "candidate"]] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let commit = git(&root, &["rev-parse", "HEAD"]).unwrap();
        let tree = git(&root, &["show", "-s", "--format=%T", &commit]).unwrap();
        verify_git_identity(&root, &base, &commit, &tree).unwrap();
        assert!(verify_git_identity(&root, &commit, &commit, &tree).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
