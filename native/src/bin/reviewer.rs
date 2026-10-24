//! Independent final-diff reviewer process.
//!
//! Fetches an exact candidate inspection from Core under a dedicated reviewer
//! credential, re-verifies Git identity and scope, applies deterministic review
//! rules, and submits a candidate-bound receipt. It never edits the candidate,
//! executes candidate code, or uses the implementer/verifier credential.

use std::{env, path::Path, time::Duration};

use factory_native::{
    db::CandidateInspection,
    review::{ReviewRequest, ReviewVerdict, review_candidate},
};
use serde_json::json;
use uuid::Uuid;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let core = env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let core = validated_core_url(&core)?;
    let token =
        env::var("FACTORY_REVIEWER_TOKEN").map_err(|_| "FACTORY_REVIEWER_TOKEN required")?;
    if token.len() < 32 {
        return Err("FACTORY_REVIEWER_TOKEN must be at least 32 bytes".into());
    }
    let candidate_id =
        env::var("FACTORY_CANDIDATE_ID").map_err(|_| "FACTORY_CANDIDATE_ID required")?;
    let principal =
        env::var("FACTORY_REVIEWER_PRINCIPAL").unwrap_or_else(|_| "reviewer-process".into());
    if principal.trim().is_empty() || principal.len() > 128 {
        return Err("FACTORY_REVIEWER_PRINCIPAL is invalid".into());
    }
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
    let evidence = tokio::task::spawn_blocking(move || {
        let repository = Path::new(&inspection.repository);
        let lineage = inspection
            .integration
            .then_some(inspection.lineage.as_slice());
        review_candidate(&ReviewRequest {
            repository,
            base_commit: &inspection.base_commit,
            commit_sha: &inspection.candidate.commit_sha,
            tree_sha: &inspection.candidate.tree_sha,
            allowed_paths: &inspection.action_spec.allowed_paths,
            integration_lineage: lineage,
        })
        .map(|evidence| (evidence, inspection.action_spec_sha256))
    })
    .await
    .map_err(|error| format!("review task failed: {error}"))??;
    let (evidence, action_spec_sha256) = evidence;
    let result = match evidence.verdict {
        ReviewVerdict::Approve => "PASS",
        ReviewVerdict::RequestChanges => "CHANGES_REQUESTED",
    };
    let receipt = client
        .post(core.join("quality/receipts")?)
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": Uuid::new_v4().to_string(),
            "principal_id": principal,
            "actionSpecSha256": action_spec_sha256,
            "commitSha": evidence.commit_sha,
            "treeSha": evidence.tree_sha,
            "result": result,
            "evidence": "EXECUTED"
        }))
        .send()
        .await?;
    if !receipt.status().is_success() {
        return Err(format!(
            "Core rejected reviewer receipt: {} {}",
            receipt.status(),
            receipt.text().await.unwrap_or_default()
        )
        .into());
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "candidateId": candidate_id,
            "commitSha": evidence.commit_sha,
            "verdict": result,
            "findings": evidence.findings,
            "diffStat": evidence.diff_stat,
            "diffBytes": evidence.diff_bytes
        }))?
    );
    Ok(())
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

    #[test]
    fn requires_loopback_http_origin_with_explicit_port() {
        assert!(validated_core_url("http://127.0.0.1:8765").is_ok());
        for invalid in [
            "https://127.0.0.1:8765",
            "http://127.0.0.1",
            "http://localhost:8765/path",
            "http://user@127.0.0.1:8765",
            "http://example.com:8765",
        ] {
            assert!(validated_core_url(invalid).is_err(), "{invalid}");
        }
    }
}
