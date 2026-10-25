//! Independent final-diff reviewer process.
//!
//! Fetches an exact candidate inspection from Core under a dedicated reviewer
//! credential, re-verifies Git identity and scope, applies deterministic review
//! rules by default, or a bounded model semantic review when
//! `FACTORY_REVIEW_MECHANISM=MODEL`. It submits a candidate-bound receipt with
//! validated findings and model provenance. It never edits the candidate,
//! executes candidate code, holds provider credentials, or uses the
//! implementer/verifier credential.

use std::{env, path::Path, time::Duration};

use factory_native::{
    db::{CandidateInspection, QualityReceiptRecord, QualityRole, ReviewMechanism},
    quality::{EvidenceKind, ReceiptResult},
    review::{
        FindingCategory, FindingSeverity, ReviewEvidence, ReviewFinding, ReviewRequest,
        ReviewVerdict,
    },
    semantic_review::{
        ReviewBundleRequest, SemanticReviewBundle, SemanticReviewInput, SemanticReviewMechanism,
        SemanticVerdict, VerifierEvidence, build_review_bundle, mechanism_from_env,
        render_semantic_review_prompt,
    },
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelCallResponse {
    reservation_id: String,
    provider: String,
    model: String,
    request_sha256: String,
    content: String,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    cost_micros: u64,
    usage_known: bool,
}

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
    let mechanism = mechanism_from_env(env::var("FACTORY_REVIEW_MECHANISM").ok().as_deref())
        .map_err(|error| format!("invalid FACTORY_REVIEW_MECHANISM: {error}"))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?;
    let inspection: CandidateInspection = client
        .get(core.join(&format!("quality/candidates/{candidate_id}/inspection"))?)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let action_spec_sha256 = inspection.action_spec_sha256.clone();
    let outcome = match mechanism {
        SemanticReviewMechanism::Deterministic => deterministic_outcome(inspection).await?,
        SemanticReviewMechanism::Model => {
            let deterministic = deterministic_evidence(&inspection).await?;
            if deterministic.verdict == ReviewVerdict::RequestChanges {
                CompletedOutcome {
                    commit_sha: deterministic.commit_sha,
                    tree_sha: deterministic.tree_sha,
                    result: ReceiptResult::ChangesRequested,
                    evidence: EvidenceKind::Executed,
                    findings: Vec::new(),
                    mechanism: ReviewMechanism::Deterministic,
                    provider: String::new(),
                    model: String::new(),
                    reservation_id: String::new(),
                    request_sha256: String::new(),
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    cost_micros: 0,
                    usage_known: false,
                }
            } else {
                model_outcome(
                    &client,
                    &core,
                    &token,
                    &principal,
                    &candidate_id,
                    inspection,
                    deterministic,
                )
                .await?
            }
        }
    };
    let receipt = client
        .post(core.join("quality/receipts")?)
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": Uuid::new_v4().to_string(),
            "principal_id": principal,
            "actionSpecSha256": action_spec_sha256,
            "commitSha": outcome.commit_sha,
            "treeSha": outcome.tree_sha,
            "result": outcome.result,
            "evidence": outcome.evidence,
            "findings": outcome.findings,
            "mechanism": outcome.mechanism,
            "modelProvider": outcome.provider,
            "modelName": outcome.model,
            "modelReservationId": outcome.reservation_id,
            "modelRequestSha256": outcome.request_sha256,
            "inputTokens": outcome.input_tokens,
            "outputTokens": outcome.output_tokens,
            "totalTokens": outcome.total_tokens,
            "costMicros": outcome.cost_micros,
            "usageKnown": outcome.usage_known,
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
            "commitSha": outcome.commit_sha,
            "treeSha": outcome.tree_sha,
            "verdict": outcome.result,
            "mechanism": outcome.mechanism,
            "modelProvider": outcome.provider,
            "modelName": outcome.model,
            "modelReservationId": outcome.reservation_id,
            "modelRequestSha256": outcome.request_sha256,
            "findings": outcome.findings,
            "inputTokens": outcome.input_tokens,
            "outputTokens": outcome.output_tokens,
            "totalTokens": outcome.total_tokens,
            "costMicros": outcome.cost_micros,
            "usageKnown": outcome.usage_known
        }))?
    );
    Ok(())
}

struct CompletedOutcome {
    commit_sha: String,
    tree_sha: String,
    result: ReceiptResult,
    evidence: EvidenceKind,
    findings: Vec<ReviewFinding>,
    mechanism: ReviewMechanism,
    provider: String,
    model: String,
    reservation_id: String,
    request_sha256: String,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    cost_micros: u64,
    usage_known: bool,
}

async fn deterministic_evidence(
    inspection: &CandidateInspection,
) -> Result<ReviewEvidence, Box<dyn std::error::Error>> {
    let repository = inspection.repository.clone();
    let base_commit = inspection.base_commit.clone();
    let commit_sha = inspection.candidate.commit_sha.clone();
    let tree_sha = inspection.candidate.tree_sha.clone();
    let allowed_paths = inspection.action_spec.allowed_paths.clone();
    let lineage = inspection.integration.then_some(inspection.lineage.clone());
    tokio::task::spawn_blocking(move || {
        factory_native::review::review_candidate(&ReviewRequest {
            repository: Path::new(&repository),
            base_commit: &base_commit,
            commit_sha: &commit_sha,
            tree_sha: &tree_sha,
            allowed_paths: &allowed_paths,
            integration_lineage: lineage.as_deref(),
        })
    })
    .await
    .map_err(|error| format!("review task failed: {error}"))?
    .map_err(Into::into)
}

async fn deterministic_outcome(
    inspection: CandidateInspection,
) -> Result<CompletedOutcome, Box<dyn std::error::Error>> {
    let evidence = deterministic_evidence(&inspection).await?;
    let result = match evidence.verdict {
        ReviewVerdict::Approve => ReceiptResult::Pass,
        ReviewVerdict::RequestChanges => ReceiptResult::ChangesRequested,
    };
    Ok(CompletedOutcome {
        commit_sha: evidence.commit_sha,
        tree_sha: evidence.tree_sha,
        result,
        evidence: EvidenceKind::Executed,
        findings: Vec::new(),
        mechanism: ReviewMechanism::Deterministic,
        provider: String::new(),
        model: String::new(),
        reservation_id: String::new(),
        request_sha256: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_micros: 0,
        usage_known: false,
    })
}

fn combine_review_verdicts(deterministic: ReviewVerdict, semantic: ReceiptResult) -> ReceiptResult {
    match (deterministic, semantic) {
        (ReviewVerdict::RequestChanges, _) => ReceiptResult::ChangesRequested,
        (ReviewVerdict::Approve, ReceiptResult::Pass) => ReceiptResult::Pass,
        (ReviewVerdict::Approve, ReceiptResult::ChangesRequested) => {
            ReceiptResult::ChangesRequested
        }
        (ReviewVerdict::Approve, ReceiptResult::NotTested) => ReceiptResult::NotTested,
        (ReviewVerdict::Approve, ReceiptResult::Fail) => ReceiptResult::Fail,
    }
}

async fn model_outcome(
    client: &reqwest::Client,
    core: &reqwest::Url,
    token: &str,
    principal: &str,
    candidate_id: &str,
    inspection: CandidateInspection,
    deterministic: ReviewEvidence,
) -> Result<CompletedOutcome, Box<dyn std::error::Error>> {
    let project_id = inspection.action_spec.project_id.clone();
    let commit_sha = deterministic.commit_sha.clone();
    let tree_sha = deterministic.tree_sha.clone();
    let receipts: Vec<QualityReceiptRecord> = match client
        .get(core.join(&format!("quality/candidates/{candidate_id}/receipts"))?)
        .bearer_auth(token)
        .send()
        .await
    {
        Ok(response) => response.json().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    let verifier = receipts
        .iter()
        .find(|receipt| receipt.role == QualityRole::Verifier)
        .map(|receipt| VerifierEvidence {
            principal_id: receipt.principal_id.clone(),
            result: receipt.result,
            evidence: receipt.evidence,
        });
    let bundle = match build_bundle(inspection, candidate_id, verifier).await {
        Ok(bundle) => bundle,
        Err(error) => {
            return Ok(failed_model_outcome(
                &commit_sha,
                &tree_sha,
                candidate_id,
                ReceiptResult::NotTested,
                EvidenceKind::Unknown,
                &format!("semantic review bundle could not be built: {error}"),
                None,
            ));
        }
    };
    let (system_prompt, user_prompt) = render_semantic_review_prompt(&bundle);
    let request = client
        .post(core.join("model/calls")?)
        .bearer_auth(token)
        .json(&json!({
            "role": "semantic_reviewer",
            "principalId": principal,
            "projectId": project_id,
            "objectiveId": candidate_id,
            "scopeId": candidate_id,
            "purpose": "semantic_review",
            "systemPrompt": system_prompt,
            "userPrompt": user_prompt,
            "maxOutputTokens": 4096
        }))
        .send()
        .await;
    let response = match request {
        Ok(response) => response,
        Err(_) => {
            return Ok(failed_model_outcome(
                &commit_sha,
                &tree_sha,
                candidate_id,
                ReceiptResult::NotTested,
                EvidenceKind::Unknown,
                "semantic model call could not reach Core",
                None,
            ));
        }
    };
    let status = response.status();
    if !status.is_success() {
        return Ok(failed_model_outcome(
            &commit_sha,
            &tree_sha,
            candidate_id,
            ReceiptResult::NotTested,
            EvidenceKind::Unknown,
            &format!("semantic model call failed with HTTP {status}"),
            None,
        ));
    }
    let call: ModelCallResponse = match response.json().await {
        Ok(call) => call,
        Err(_) => {
            return Ok(failed_model_outcome(
                &commit_sha,
                &tree_sha,
                candidate_id,
                ReceiptResult::NotTested,
                EvidenceKind::Unknown,
                "semantic model call returned an unreadable response envelope",
                None,
            ));
        }
    };
    if !call.usage_known {
        return Ok(failed_model_outcome(
            &commit_sha,
            &tree_sha,
            candidate_id,
            ReceiptResult::NotTested,
            EvidenceKind::Unknown,
            "semantic model usage is unknown",
            Some(&call),
        ));
    }
    let input = SemanticReviewInput {
        candidate_commit_sha: bundle.candidate_commit_sha.clone(),
        candidate_tree_sha: bundle.candidate_tree_sha.clone(),
        action_spec_sha256: bundle.action_spec_sha256.clone(),
    };
    let review =
        match factory_native::model_output::decode_semantic_review(call.content.as_bytes(), &input)
        {
            Ok(review) => review,
            Err(error) => {
                let message = bounded(
                    &format!("semantic review output was rejected: {error}"),
                    500,
                );
                return Ok(CompletedOutcome {
                    commit_sha,
                    tree_sha,
                    result: ReceiptResult::Fail,
                    evidence: EvidenceKind::Executed,
                    findings: vec![failure_finding(candidate_id, &message)],
                    mechanism: ReviewMechanism::Model,
                    provider: call.provider,
                    model: call.model,
                    reservation_id: call.reservation_id,
                    request_sha256: call.request_sha256,
                    input_tokens: call.input_tokens,
                    output_tokens: call.output_tokens,
                    total_tokens: call.total_tokens,
                    cost_micros: call.cost_micros,
                    usage_known: call.usage_known,
                });
            }
        };
    let semantic_result = match review.verdict {
        SemanticVerdict::Approve => ReceiptResult::Pass,
        SemanticVerdict::RequestChanges => ReceiptResult::ChangesRequested,
        SemanticVerdict::Uncertain => ReceiptResult::NotTested,
    };
    let result = combine_review_verdicts(deterministic.verdict, semantic_result);
    let mut findings: Vec<ReviewFinding> = deterministic
        .findings
        .into_iter()
        .filter(|finding| !finding.evidence_refs.is_empty())
        .collect();
    findings.extend(review.findings);
    Ok(CompletedOutcome {
        commit_sha,
        tree_sha,
        result,
        evidence: EvidenceKind::Executed,
        findings,
        mechanism: ReviewMechanism::Model,
        provider: call.provider,
        model: call.model,
        reservation_id: call.reservation_id,
        request_sha256: call.request_sha256,
        input_tokens: call.input_tokens,
        output_tokens: call.output_tokens,
        total_tokens: call.total_tokens,
        cost_micros: call.cost_micros,
        usage_known: call.usage_known,
    })
}

async fn build_bundle(
    inspection: CandidateInspection,
    candidate_id: &str,
    verifier: Option<VerifierEvidence>,
) -> Result<SemanticReviewBundle, Box<dyn std::error::Error>> {
    let candidate_id = candidate_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let lineage = inspection
            .integration
            .then_some(inspection.lineage.as_slice());
        build_review_bundle(&ReviewBundleRequest {
            candidate_id: &candidate_id,
            repository: Path::new(&inspection.repository),
            base_commit: &inspection.base_commit,
            commit_sha: &inspection.candidate.commit_sha,
            tree_sha: &inspection.candidate.tree_sha,
            action_spec_sha256: &inspection.action_spec_sha256,
            objective: &inspection.action_spec.objective,
            allowed_paths: &inspection.action_spec.allowed_paths,
            checks: &inspection.action_spec.checks,
            integration_lineage: lineage,
            verifier,
        })
    })
    .await
    .map_err(|error| format!("review bundle task failed: {error}"))?
    .map_err(Into::into)
}

fn failed_model_outcome(
    commit_sha: &str,
    tree_sha: &str,
    candidate_id: &str,
    result: ReceiptResult,
    evidence: EvidenceKind,
    message: &str,
    call: Option<&ModelCallResponse>,
) -> CompletedOutcome {
    CompletedOutcome {
        commit_sha: commit_sha.into(),
        tree_sha: tree_sha.into(),
        result,
        evidence,
        findings: vec![failure_finding(candidate_id, message)],
        mechanism: ReviewMechanism::Model,
        provider: call.map(|call| call.provider.clone()).unwrap_or_default(),
        model: call.map(|call| call.model.clone()).unwrap_or_default(),
        reservation_id: call
            .map(|call| call.reservation_id.clone())
            .unwrap_or_default(),
        request_sha256: call
            .map(|call| call.request_sha256.clone())
            .unwrap_or_default(),
        input_tokens: call.map(|call| call.input_tokens).unwrap_or_default(),
        output_tokens: call.map(|call| call.output_tokens).unwrap_or_default(),
        total_tokens: call.map(|call| call.total_tokens).unwrap_or_default(),
        cost_micros: call.map(|call| call.cost_micros).unwrap_or_default(),
        usage_known: call.map(|call| call.usage_known).unwrap_or(false),
    }
}

fn failure_finding(candidate_id: &str, message: &str) -> ReviewFinding {
    ReviewFinding {
        id: "semantic-review-unavailable".into(),
        severity: FindingSeverity::Blocking,
        category: FindingCategory::Other,
        message: bounded(message, 500),
        evidence_refs: vec![candidate_id.to_owned()],
    }
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
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

    #[test]
    fn deterministic_approval_is_required_for_a_model_review_pass() {
        assert_eq!(
            combine_review_verdicts(ReviewVerdict::Approve, ReceiptResult::Pass),
            ReceiptResult::Pass
        );
        assert_eq!(
            combine_review_verdicts(ReviewVerdict::RequestChanges, ReceiptResult::Pass),
            ReceiptResult::ChangesRequested
        );
        assert_eq!(
            combine_review_verdicts(ReviewVerdict::Approve, ReceiptResult::ChangesRequested),
            ReceiptResult::ChangesRequested
        );
        assert_eq!(
            combine_review_verdicts(ReviewVerdict::Approve, ReceiptResult::NotTested),
            ReceiptResult::NotTested
        );
        assert_eq!(
            combine_review_verdicts(ReviewVerdict::Approve, ReceiptResult::Fail),
            ReceiptResult::Fail
        );
    }

    #[test]
    fn failure_findings_are_bounded_and_concrete() {
        let long = "x".repeat(2000);
        let finding = failure_finding("candidate", &long);
        assert_eq!(finding.evidence_refs, vec!["candidate"]);
        assert_eq!(finding.message.len(), 500);
        assert_eq!(finding.severity, FindingSeverity::Blocking);
    }
}
