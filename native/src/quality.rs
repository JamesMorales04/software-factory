//! Deterministic, evidence-bound quality gate domain evaluator.
//!
//! This module evaluates recorded evidence only. It does not execute verification,
//! review, or policy checks, and an `ACCEPTED` result is not itself proof that the
//! receipts were cryptographically authenticated.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenActionSpec {
    /// SHA-256 of the canonical, frozen ActionSpec bytes (64 hexadecimal characters).
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CandidateRevision {
    /// Exact Git commit object ID (40 hexadecimal characters).
    pub commit_sha: String,
    /// Exact Git tree object ID (40 hexadecimal characters).
    pub tree_sha: String,
    /// Frozen ActionSpec hash this candidate claims to implement.
    pub action_spec_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReceiptResult {
    Pass,
    Fail,
    ChangesRequested,
    NotTested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceKind {
    Executed,
    Mocked,
    Unknown,
}

/// A verifier receipt bound to a single exact ActionSpec and candidate revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VerifierReceipt {
    pub receipt_id: String,
    pub principal_id: String,
    pub action_spec_hash: String,
    pub candidate_commit_sha: String,
    pub candidate_tree_sha: String,
    pub result: ReceiptResult,
    pub evidence: EvidenceKind,
}

/// An independent reviewer receipt bound to the same frozen candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewerReceipt {
    pub receipt_id: String,
    pub principal_id: String,
    pub action_spec_hash: String,
    pub candidate_commit_sha: String,
    pub candidate_tree_sha: String,
    pub result: ReceiptResult,
    pub evidence: EvidenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorizationStatus {
    Authorized,
    HumanApprovalRequired,
    Denied,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskStatus {
    WithinPolicy,
    HumanApprovalRequired,
    ExceedsPolicy,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GatePolicy {
    pub authorization: AuthorizationStatus,
    pub risk: RiskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateOutcome {
    Accepted,
    RepairRequired,
    Blocked,
    HumanRequired,
    NotTested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GateDecision {
    pub outcome: GateOutcome,
    pub reasons: Vec<String>,
}

/// Evaluate all supplied evidence deterministically.
///
/// Ordering is intentional: authorization, candidate identity, and evidence
/// integrity failures cannot be overridden by a passing receipt. Missing or
/// unknown evidence yields `NOT_TESTED`; it never yields `ACCEPTED`.
pub fn evaluate_quality_gate(
    action: &FrozenActionSpec,
    candidate: &CandidateRevision,
    implementer_id: &str,
    verifier: Option<&VerifierReceipt>,
    reviewer: Option<&ReviewerReceipt>,
    policy: GatePolicy,
) -> GateDecision {
    let mut reasons = Vec::new();
    let mut human_required = false;

    match policy.authorization {
        AuthorizationStatus::Denied => reasons.push("authorization denied".into()),
        AuthorizationStatus::Unknown => reasons.push("authorization status unknown".into()),
        AuthorizationStatus::HumanApprovalRequired => human_required = true,
        AuthorizationStatus::Authorized => {}
    }
    match policy.risk {
        RiskStatus::ExceedsPolicy => reasons.push("risk exceeds policy".into()),
        RiskStatus::Unknown => reasons.push("risk status unknown".into()),
        RiskStatus::HumanApprovalRequired => human_required = true,
        RiskStatus::WithinPolicy => {}
    }
    if !reasons.is_empty() {
        return GateDecision {
            outcome: GateOutcome::Blocked,
            reasons,
        };
    }
    if human_required {
        return decision(
            GateOutcome::HumanRequired,
            "human authorization or risk approval required",
        );
    }

    if !is_hex(&action.hash, 64) {
        return decision(GateOutcome::Blocked, "frozen ActionSpec hash is invalid");
    }
    if !is_object_id(&candidate.commit_sha) || !is_object_id(&candidate.tree_sha) {
        return decision(
            GateOutcome::Blocked,
            "candidate commit or tree identity is invalid",
        );
    }
    if candidate.action_spec_hash != action.hash {
        return decision(GateOutcome::Blocked, "candidate ActionSpec hash mismatch");
    }
    if implementer_id.trim().is_empty() {
        return decision(GateOutcome::Blocked, "implementer identity is missing");
    }

    let Some(verifier) = verifier else {
        return decision(
            GateOutcome::NotTested,
            "independent verifier receipt is missing",
        );
    };
    let Some(reviewer) = reviewer else {
        return decision(
            GateOutcome::NotTested,
            "independent reviewer receipt is missing",
        );
    };

    for (role, receipt_id, principal_id, spec_hash, commit, tree) in [
        (
            "verifier",
            verifier.receipt_id.as_str(),
            verifier.principal_id.as_str(),
            verifier.action_spec_hash.as_str(),
            verifier.candidate_commit_sha.as_str(),
            verifier.candidate_tree_sha.as_str(),
        ),
        (
            "reviewer",
            reviewer.receipt_id.as_str(),
            reviewer.principal_id.as_str(),
            reviewer.action_spec_hash.as_str(),
            reviewer.candidate_commit_sha.as_str(),
            reviewer.candidate_tree_sha.as_str(),
        ),
    ] {
        if receipt_id.trim().is_empty() || principal_id.trim().is_empty() {
            return decision(
                GateOutcome::Blocked,
                format!("{role} receipt identity is missing"),
            );
        }
        if spec_hash != action.hash {
            return decision(
                GateOutcome::Blocked,
                format!("{role} ActionSpec hash mismatch"),
            );
        }
        if commit != candidate.commit_sha || tree != candidate.tree_sha {
            return decision(
                GateOutcome::Blocked,
                format!("{role} candidate revision mismatch"),
            );
        }
    }

    if verifier.principal_id == implementer_id || reviewer.principal_id == implementer_id {
        return decision(
            GateOutcome::Blocked,
            "implementer cannot verify or review their own candidate",
        );
    }
    if verifier.principal_id == reviewer.principal_id {
        return decision(
            GateOutcome::Blocked,
            "verifier and reviewer must be independent principals",
        );
    }
    if verifier.receipt_id == reviewer.receipt_id {
        return decision(
            GateOutcome::Blocked,
            "verifier and reviewer receipts must be distinct",
        );
    }
    if verifier.evidence == EvidenceKind::Mocked || reviewer.evidence == EvidenceKind::Mocked {
        return decision(
            GateOutcome::Blocked,
            "mocked evidence cannot satisfy the quality gate",
        );
    }
    if verifier.evidence == EvidenceKind::Unknown || reviewer.evidence == EvidenceKind::Unknown {
        return decision(
            GateOutcome::NotTested,
            "receipt evidence provenance is unknown",
        );
    }
    if verifier.result == ReceiptResult::Fail || reviewer.result == ReceiptResult::Fail {
        return decision(
            GateOutcome::RepairRequired,
            "verification or review found a failure",
        );
    }
    if verifier.result == ReceiptResult::ChangesRequested
        || reviewer.result == ReceiptResult::ChangesRequested
    {
        return decision(GateOutcome::RepairRequired, "review requires changes");
    }
    if verifier.result == ReceiptResult::NotTested || reviewer.result == ReceiptResult::NotTested {
        return decision(
            GateOutcome::NotTested,
            "verification or review was not tested",
        );
    }

    decision(
        GateOutcome::Accepted,
        "independent executed evidence accepts exact candidate",
    )
}

fn is_object_id(value: &str) -> bool {
    is_hex(value, 40) || is_hex(value, 64)
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn decision(outcome: GateOutcome, reason: impl Into<String>) -> GateDecision {
    GateDecision {
        outcome,
        reasons: vec![reason.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action() -> FrozenActionSpec {
        FrozenActionSpec {
            hash: "a".repeat(64),
        }
    }

    fn candidate() -> CandidateRevision {
        CandidateRevision {
            commit_sha: "b".repeat(40),
            tree_sha: "c".repeat(40),
            action_spec_hash: "a".repeat(64),
        }
    }

    fn verifier() -> VerifierReceipt {
        VerifierReceipt {
            receipt_id: "verify-1".into(),
            principal_id: "verifier".into(),
            action_spec_hash: "a".repeat(64),
            candidate_commit_sha: "b".repeat(40),
            candidate_tree_sha: "c".repeat(40),
            result: ReceiptResult::Pass,
            evidence: EvidenceKind::Executed,
        }
    }

    fn reviewer() -> ReviewerReceipt {
        ReviewerReceipt {
            receipt_id: "review-1".into(),
            principal_id: "reviewer".into(),
            action_spec_hash: "a".repeat(64),
            candidate_commit_sha: "b".repeat(40),
            candidate_tree_sha: "c".repeat(40),
            result: ReceiptResult::Pass,
            evidence: EvidenceKind::Executed,
        }
    }

    fn policy() -> GatePolicy {
        GatePolicy {
            authorization: AuthorizationStatus::Authorized,
            risk: RiskStatus::WithinPolicy,
        }
    }

    fn evaluate(v: Option<&VerifierReceipt>, r: Option<&ReviewerReceipt>) -> GateDecision {
        evaluate_quality_gate(&action(), &candidate(), "implementer", v, r, policy())
    }

    #[test]
    fn accepts_only_exact_candidate_with_independent_executed_passes() {
        assert_eq!(
            evaluate(Some(&verifier()), Some(&reviewer())).outcome,
            GateOutcome::Accepted
        );
    }

    #[test]
    fn missing_receipt_is_not_tested() {
        assert_eq!(
            evaluate(None, Some(&reviewer())).outcome,
            GateOutcome::NotTested
        );
        assert_eq!(
            evaluate(Some(&verifier()), None).outcome,
            GateOutcome::NotTested
        );
    }

    #[test]
    fn spec_and_candidate_mismatches_block_even_if_receipts_pass() {
        let mut candidate = candidate();
        candidate.action_spec_hash = "d".repeat(64);
        let result = evaluate_quality_gate(
            &action(),
            &candidate,
            "implementer",
            Some(&verifier()),
            Some(&reviewer()),
            policy(),
        );
        assert_eq!(result.outcome, GateOutcome::Blocked);

        let mut receipt = verifier();
        receipt.candidate_commit_sha = "e".repeat(40);
        assert_eq!(
            evaluate(Some(&receipt), Some(&reviewer())).outcome,
            GateOutcome::Blocked
        );
    }

    #[test]
    fn implementer_cannot_supply_independent_receipts() {
        let mut receipt = verifier();
        receipt.principal_id = "implementer".into();
        assert_eq!(
            evaluate(Some(&receipt), Some(&reviewer())).outcome,
            GateOutcome::Blocked
        );
    }

    #[test]
    fn verifier_and_reviewer_must_be_distinct() {
        let mut review = reviewer();
        review.principal_id = "verifier".into();
        assert_eq!(
            evaluate(Some(&verifier()), Some(&review)).outcome,
            GateOutcome::Blocked
        );
    }

    #[test]
    fn mocked_unknown_and_failed_evidence_never_accept() {
        let mut v = verifier();
        v.evidence = EvidenceKind::Mocked;
        assert_eq!(
            evaluate(Some(&v), Some(&reviewer())).outcome,
            GateOutcome::Blocked
        );
        v.evidence = EvidenceKind::Unknown;
        assert_eq!(
            evaluate(Some(&v), Some(&reviewer())).outcome,
            GateOutcome::NotTested
        );
        v.evidence = EvidenceKind::Executed;
        v.result = ReceiptResult::Fail;
        assert_eq!(
            evaluate(Some(&v), Some(&reviewer())).outcome,
            GateOutcome::RepairRequired
        );
    }

    #[test]
    fn policy_can_block_or_require_human_authority() {
        let mut p = policy();
        p.authorization = AuthorizationStatus::Denied;
        assert_eq!(
            evaluate_quality_gate(
                &action(),
                &candidate(),
                "implementer",
                Some(&verifier()),
                Some(&reviewer()),
                p
            )
            .outcome,
            GateOutcome::Blocked
        );
        p.authorization = AuthorizationStatus::HumanApprovalRequired;
        assert_eq!(
            evaluate_quality_gate(
                &action(),
                &candidate(),
                "implementer",
                Some(&verifier()),
                Some(&reviewer()),
                p
            )
            .outcome,
            GateOutcome::HumanRequired
        );
    }

    #[test]
    fn strict_receipt_deserialization_rejects_unknown_fields() {
        let json = format!(
            r#"{{"receiptId":"r","principalId":"p","actionSpecHash":"{}","candidateCommitSha":"{}","candidateTreeSha":"{}","result":"PASS","evidence":"EXECUTED","legacyRuntime":true}}"#,
            "a".repeat(64),
            "b".repeat(40),
            "c".repeat(40)
        );
        assert!(serde_json::from_str::<VerifierReceipt>(&json).is_err());
    }
}
