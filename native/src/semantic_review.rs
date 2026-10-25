//! Strict model-backed semantic review contract and bounded review bundle.
//!
//! A semantic verdict is advisory. Core validates every identity and finding
//! before persistence, the deterministic Quality Gate remains authoritative,
//! and model output can never edit the candidate or grant authority. Raw prompts
//! and source bodies are never persisted in telemetry or receipts.

use std::collections::BTreeSet;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::lead::SpecCheck;
use crate::quality::{EvidenceKind, ReceiptResult};
use crate::review::{FindingSeverity, ReviewFinding};
use crate::workspace::{self, WorkspaceError};

pub const SEMANTIC_SCHEMA_REVISION: u32 = 1;
pub const MAX_SEMANTIC_FINDINGS: usize = 20;
pub const MAX_SUMMARY_CHARS: usize = 4000;
pub const MAX_FINDING_MESSAGE_CHARS: usize = 2000;
pub const MAX_EVIDENCE_REFS: usize = 16;
pub const MAX_EVIDENCE_REF_CHARS: usize = 512;
pub const MAX_SOURCE_FILE_BYTES: usize = 64 * 1024;
pub const MAX_SOURCE_TOTAL_BYTES: usize = 128 * 1024;
pub const MAX_BUNDLE_DIFF_BYTES: usize = 192 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SemanticVerdict {
    Approve,
    RequestChanges,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SemanticReview {
    pub schema_revision: u32,
    pub candidate_commit_sha: String,
    pub candidate_tree_sha: String,
    pub action_spec_sha256: String,
    pub verdict: SemanticVerdict,
    #[schemars(length(min = 1, max = 4000))]
    pub summary: String,
    #[schemars(range(min = 0, max = 100))]
    pub confidence: u32,
    #[schemars(length(min = 0, max = 20))]
    pub findings: Vec<ReviewFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReviewInput {
    pub candidate_commit_sha: String,
    pub candidate_tree_sha: String,
    pub action_spec_sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SemanticReviewError {
    #[error("semantic review is invalid: {0}")]
    Invalid(String),
}

impl SemanticReview {
    pub fn from_json(raw: &[u8], input: &SemanticReviewInput) -> Result<Self, SemanticReviewError> {
        let review: Self = serde_json::from_slice(raw).map_err(|error| {
            SemanticReviewError::Invalid(format!("not strict semantic review JSON: {error}"))
        })?;
        review.validate(input)?;
        Ok(review)
    }

    pub fn validate(&self, input: &SemanticReviewInput) -> Result<(), SemanticReviewError> {
        if self.schema_revision != SEMANTIC_SCHEMA_REVISION {
            return invalid("unsupported schemaRevision");
        }
        if !self
            .candidate_commit_sha
            .eq_ignore_ascii_case(&input.candidate_commit_sha)
            || !self
                .candidate_tree_sha
                .eq_ignore_ascii_case(&input.candidate_tree_sha)
            || !self
                .action_spec_sha256
                .eq_ignore_ascii_case(&input.action_spec_sha256)
        {
            return invalid("semantic review identities do not match the exact candidate");
        }
        validate_text("summary", &self.summary, MAX_SUMMARY_CHARS)?;
        if self.confidence > 100 {
            return invalid("confidence must be within 0..=100");
        }
        if self.findings.len() > MAX_SEMANTIC_FINDINGS {
            return invalid("semantic review has too many findings");
        }
        let mut ids = BTreeSet::new();
        for finding in &self.findings {
            validate_finding(finding)?;
            if !ids.insert(finding.id.as_str()) {
                return invalid("semantic review contains duplicate finding ids");
            }
        }
        if self.verdict == SemanticVerdict::Approve
            && self
                .findings
                .iter()
                .any(|finding| finding.severity == FindingSeverity::Blocking)
        {
            return invalid("an approval cannot contain blocking findings");
        }
        if self.verdict != SemanticVerdict::Approve && self.findings.is_empty() {
            return invalid("a non-approval verdict requires at least one finding");
        }
        Ok(())
    }
}

pub fn validate_finding(finding: &ReviewFinding) -> Result<(), SemanticReviewError> {
    validate_text("finding id", &finding.id, 64)?;
    validate_text(
        "finding message",
        &finding.message,
        MAX_FINDING_MESSAGE_CHARS,
    )?;
    if finding.evidence_refs.is_empty() || finding.evidence_refs.len() > MAX_EVIDENCE_REFS {
        return invalid("finding must cite 1..=16 evidence references");
    }
    let mut refs = BTreeSet::new();
    for reference in &finding.evidence_refs {
        validate_text("evidence reference", reference, MAX_EVIDENCE_REF_CHARS)?;
        if !refs.insert(reference.as_str()) {
            return invalid("finding cites a duplicate evidence reference");
        }
    }
    Ok(())
}

pub fn validate_findings(findings: &[ReviewFinding]) -> Result<(), SemanticReviewError> {
    if findings.len() > MAX_SEMANTIC_FINDINGS {
        return invalid("too many review findings");
    }
    let mut ids = BTreeSet::new();
    for finding in findings {
        validate_finding(finding)?;
        if !ids.insert(finding.id.as_str()) {
            return invalid("duplicate review finding id");
        }
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max_chars: usize) -> Result<(), SemanticReviewError> {
    if value.trim().is_empty()
        || value.chars().count() > max_chars
        || value.chars().any(|character| {
            character.is_control() && character != '\n' && character != '\t' && character != '\r'
        })
    {
        return invalid(name);
    }
    Ok(())
}

fn invalid<T>(message: &str) -> Result<T, SemanticReviewError> {
    Err(SemanticReviewError::Invalid(message.into()))
}

#[derive(Debug, Clone)]
pub struct VerifierEvidence {
    pub principal_id: String,
    pub result: ReceiptResult,
    pub evidence: EvidenceKind,
}

#[derive(Debug, Clone)]
pub struct SourceExcerpt {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SemanticReviewBundle {
    pub candidate_id: String,
    pub action_spec_sha256: String,
    pub candidate_commit_sha: String,
    pub candidate_tree_sha: String,
    pub objective: String,
    pub allowed_paths: Vec<String>,
    pub changed_paths: Vec<String>,
    pub diff_stat: String,
    pub diff: String,
    pub diff_truncated: bool,
    pub source_files: Vec<SourceExcerpt>,
    pub source_bytes: usize,
    pub verifier: Option<VerifierEvidence>,
    pub acceptance_checks: Vec<SpecCheck>,
    pub negative_checks: Vec<SpecCheck>,
}

pub struct ReviewBundleRequest<'a> {
    pub candidate_id: &'a str,
    pub repository: &'a Path,
    pub base_commit: &'a str,
    pub commit_sha: &'a str,
    pub tree_sha: &'a str,
    pub action_spec_sha256: &'a str,
    pub objective: &'a str,
    pub allowed_paths: &'a [String],
    pub checks: &'a [SpecCheck],
    pub integration_lineage: Option<&'a [String]>,
    pub verifier: Option<VerifierEvidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReviewBundleError {
    #[error("candidate facts could not be read: {0}")]
    Workspace(#[from] WorkspaceError),
    #[error("candidate identity changed during bundle construction")]
    Identity,
}

pub fn build_review_bundle(
    request: &ReviewBundleRequest<'_>,
) -> Result<SemanticReviewBundle, ReviewBundleError> {
    let (commit_sha, tree_sha) = match request.integration_lineage {
        Some(lineage) => {
            let expected: Vec<String> = lineage
                .iter()
                .filter(|parent| !parent.eq_ignore_ascii_case(request.base_commit))
                .cloned()
                .collect();
            let verified = workspace::verify_integrated_candidate(
                request.repository,
                request.base_commit,
                request.commit_sha,
                request.tree_sha,
                &expected,
            )?;
            (verified.commit_sha, verified.tree_sha)
        }
        None => {
            let verified = workspace::verify_candidate_commit(
                request.repository,
                request.base_commit,
                request.commit_sha,
                request.tree_sha,
            )?;
            (verified.commit_sha, verified.tree_sha)
        }
    };
    if !commit_sha.eq_ignore_ascii_case(request.commit_sha)
        || !tree_sha.eq_ignore_ascii_case(request.tree_sha)
    {
        return Err(ReviewBundleError::Identity);
    }
    let changed_paths =
        workspace::changed_paths(request.repository, request.base_commit, &commit_sha)?;
    let (full_diff, _) = workspace::diff(request.repository, request.base_commit, &commit_sha)?;
    let diff_stat = workspace::diff_stat(request.repository, request.base_commit, &commit_sha)
        .unwrap_or_default();
    let (diff, diff_truncated) = bound_text(&full_diff, MAX_BUNDLE_DIFF_BYTES);
    let mut source_files = Vec::new();
    let mut source_bytes = 0usize;
    for path in &changed_paths {
        if source_bytes >= MAX_SOURCE_TOTAL_BYTES {
            break;
        }
        if is_secret_shaped_path(path) {
            continue;
        }
        let remaining = MAX_SOURCE_TOTAL_BYTES - source_bytes;
        let limit = remaining.min(MAX_SOURCE_FILE_BYTES);
        let Some(content) =
            workspace::candidate_file(request.repository, &commit_sha, path, limit)?
        else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        source_bytes = source_bytes.saturating_add(content.len());
        source_files.push(SourceExcerpt {
            path: path.clone(),
            content,
        });
    }
    let mut acceptance_checks = Vec::new();
    let mut negative_checks = Vec::new();
    for check in request.checks {
        if check.kind == crate::lead::CheckKind::Negative {
            negative_checks.push(check.clone());
        } else {
            acceptance_checks.push(check.clone());
        }
    }
    Ok(SemanticReviewBundle {
        candidate_id: request.candidate_id.to_owned(),
        action_spec_sha256: request.action_spec_sha256.to_owned(),
        candidate_commit_sha: commit_sha,
        candidate_tree_sha: tree_sha,
        objective: request.objective.to_owned(),
        allowed_paths: request.allowed_paths.to_vec(),
        changed_paths,
        diff_stat,
        diff,
        diff_truncated,
        source_files,
        source_bytes,
        verifier: request.verifier.clone(),
        acceptance_checks,
        negative_checks,
    })
}

pub fn render_semantic_review_prompt(bundle: &SemanticReviewBundle) -> (String, String) {
    let schema = crate::model_output::semantic_review_schema();
    let schema = serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into());
    let system = format!(
        "You are an independent semantic code reviewer. Return ONLY one JSON object that \
exactly matches the supplied JSON Schema. Do not emit prose, markdown, comments, duplicate \
fields or unknown fields. Never edit candidate code. A verdict is advisory: Core revalidates \
every identity and finding, a deterministic pre-review must also approve, and the deterministic \
Quality Gate makes the final decision. Your approval is corroborating advice, not acceptance \
authority. Approve only when the exact diff plausibly satisfies the frozen objective without \
blocking functional, architectural, security, testing or scope defects. Use request_changes with \
blocking findings for defects, and uncertain with at least one concrete finding when evidence \
is insufficient. Never approve from the candidate description alone.\n\nJSON Schema:\n{schema}"
    );
    let mut prompt = String::new();
    prompt.push_str("Review the following exact frozen candidate.\n\n");
    prompt.push_str(&format!(
        "candidateId: {}\nactionSpecSha256: {}\ncandidateCommitSha: {}\ncandidateTreeSha: {}\n",
        bundle.candidate_id,
        bundle.action_spec_sha256,
        bundle.candidate_commit_sha,
        bundle.candidate_tree_sha
    ));
    prompt.push_str(&format!("objective: {}\n", bundle.objective));
    prompt.push_str(&format!("allowedPaths: {:?}\n", bundle.allowed_paths));
    prompt.push_str(&format!("changedPaths: {:?}\n", bundle.changed_paths));
    prompt.push_str(&format!("diffStat: {}\n", bundle.diff_stat));
    prompt.push_str(&format!(
        "sourceBytesIncluded: {}\nsourceFileBudget: {}\n",
        bundle.source_bytes, MAX_SOURCE_TOTAL_BYTES
    ));
    match &bundle.verifier {
        Some(verifier) => prompt.push_str(&format!(
            "verifierPrincipal: {}\nverifierResult: {:?}\nverifierEvidence: {:?}\n",
            verifier.principal_id, verifier.result, verifier.evidence
        )),
        None => prompt.push_str("verifierResult: unavailable\n"),
    }
    prompt.push_str("\nAcceptance checks:\n");
    for check in &bundle.acceptance_checks {
        prompt.push_str(&format!(
            "- id={} kind={:?} expectExit={} description={}\n",
            check.id, check.kind, check.expect_exit, check.description
        ));
    }
    prompt.push_str("\nNegative checks:\n");
    for check in &bundle.negative_checks {
        prompt.push_str(&format!(
            "- id={} kind={:?} expectExit={} description={}\n",
            check.id, check.kind, check.expect_exit, check.description
        ));
    }
    prompt.push_str(
        "\nGeneric architecture constraints:\n\
- Keep domain policy in Core; do not move authority into provider, UI or model output.\n\
- No provider credentials, secrets or raw prompts in state, logs, receipts or telemetry.\n\
- Preserve narrow module boundaries, explicit errors and deterministic state transitions.\n\
- Reject scope creep, unsafe paths, symlinks, destructive migration or hidden global state.\n\
- Model output never grants budgets, tools, delivery authority or Quality Gate acceptance.\n",
    );
    prompt.push_str(&format!(
        "\nUnified diff{}:\n{}\n",
        if bundle.diff_truncated {
            " (bounded; truncated)"
        } else {
            ""
        },
        bundle.diff
    ));
    prompt.push_str("\nBounded source files from the exact candidate:\n");
    for file in &bundle.source_files {
        prompt.push_str(&format!("\n--- {} ---\n{}\n", file.path, file.content));
    }
    (system, prompt)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticReviewMechanism {
    Deterministic,
    Model,
}

pub fn mechanism_from_env(
    value: Option<&str>,
) -> Result<SemanticReviewMechanism, SemanticReviewError> {
    match value {
        None => Ok(SemanticReviewMechanism::Deterministic),
        Some(value) if value.eq_ignore_ascii_case("MODEL") => Ok(SemanticReviewMechanism::Model),
        Some(value) if value.eq_ignore_ascii_case("DETERMINISTIC") => {
            Ok(SemanticReviewMechanism::Deterministic)
        }
        Some(_) => invalid("unsupported review mechanism"),
    }
}

pub fn is_secret_shaped_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.split('/').any(|component| {
        [
            ".env",
            "secret",
            "credential",
            "password",
            "passwd",
            "id_rsa",
            "id_ed25519",
            "token",
        ]
        .iter()
        .any(|marker| component.contains(marker))
    }) || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
}

fn bound_text(value: &str, limit: usize) -> (String, bool) {
    if value.len() <= limit {
        return (value.to_owned(), false);
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::FindingCategory;

    fn input() -> SemanticReviewInput {
        SemanticReviewInput {
            candidate_commit_sha: "a".repeat(40),
            candidate_tree_sha: "b".repeat(40),
            action_spec_sha256: "c".repeat(64),
        }
    }

    fn finding(id: &str, severity: FindingSeverity) -> ReviewFinding {
        ReviewFinding {
            id: id.into(),
            severity,
            category: FindingCategory::Functional,
            message: "concrete defect".into(),
            evidence_refs: vec!["src/lib.rs".into()],
        }
    }

    fn review(verdict: SemanticVerdict, findings: Vec<ReviewFinding>) -> SemanticReview {
        SemanticReview {
            schema_revision: 1,
            candidate_commit_sha: "a".repeat(40),
            candidate_tree_sha: "b".repeat(40),
            action_spec_sha256: "c".repeat(64),
            verdict,
            summary: "bounded review".into(),
            confidence: 88,
            findings,
        }
    }

    #[test]
    fn valid_approval_and_rejection_are_accepted() {
        review(SemanticVerdict::Approve, Vec::new())
            .validate(&input())
            .unwrap();
        review(
            SemanticVerdict::RequestChanges,
            vec![finding("f1", FindingSeverity::Blocking)],
        )
        .validate(&input())
        .unwrap();
        review(
            SemanticVerdict::Uncertain,
            vec![finding("f1", FindingSeverity::Warning)],
        )
        .validate(&input())
        .unwrap();
        let mut architecture = finding("f-arch", FindingSeverity::Blocking);
        architecture.category = FindingCategory::Architecture;
        architecture.message = "domain policy leaked into the provider adapter".into();
        review(SemanticVerdict::RequestChanges, vec![architecture])
            .validate(&input())
            .unwrap();
    }

    #[test]
    fn rejects_wrong_identity_blocking_approval_duplicates_and_bounds() {
        let mut wrong = review(SemanticVerdict::Approve, Vec::new());
        wrong.candidate_commit_sha = "d".repeat(40);
        assert!(wrong.validate(&input()).is_err());

        let blocking = review(
            SemanticVerdict::Approve,
            vec![finding("f1", FindingSeverity::Blocking)],
        );
        assert!(blocking.validate(&input()).is_err());

        let duplicate = review(
            SemanticVerdict::RequestChanges,
            vec![
                finding("f1", FindingSeverity::Blocking),
                finding("f1", FindingSeverity::Warning),
            ],
        );
        assert!(duplicate.validate(&input()).is_err());

        let mut oversized = review(SemanticVerdict::Approve, Vec::new());
        oversized.summary = "x".repeat(MAX_SUMMARY_CHARS + 1);
        assert!(oversized.validate(&input()).is_err());

        let mut confidence = review(SemanticVerdict::Approve, Vec::new());
        confidence.confidence = 101;
        assert!(confidence.validate(&input()).is_err());
    }

    #[test]
    fn rejects_empty_evidence_and_unknown_fields() {
        let mut empty_ref = review(
            SemanticVerdict::RequestChanges,
            vec![finding("f1", FindingSeverity::Blocking)],
        );
        empty_ref.findings[0].evidence_refs.clear();
        assert!(empty_ref.validate(&input()).is_err());

        let raw = serde_json::to_vec(&review(SemanticVerdict::Approve, Vec::new())).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        value["extra"] = serde_json::json!(true);
        let raw = serde_json::to_vec(&value).unwrap();
        assert!(SemanticReview::from_json(&raw, &input()).is_err());
    }

    #[test]
    fn secret_shaped_and_environment_mechanism_helpers_are_conservative() {
        assert!(is_secret_shaped_path("config/.env"));
        assert!(is_secret_shaped_path("deploy/server.PEM"));
        assert!(is_secret_shaped_path("src/token_store.rs"));
        assert!(!is_secret_shaped_path("src/review.rs"));
        assert_eq!(
            mechanism_from_env(Some("model")).unwrap(),
            SemanticReviewMechanism::Model
        );
        assert_eq!(
            mechanism_from_env(None).unwrap(),
            SemanticReviewMechanism::Deterministic
        );
        assert!(mechanism_from_env(Some("anything-else")).is_err());
    }
}
