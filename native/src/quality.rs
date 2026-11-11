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

impl ReceiptResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::ChangesRequested => "CHANGES_REQUESTED",
            Self::NotTested => "NOT_TESTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceKind {
    Executed,
    Mocked,
    Unknown,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executed => "EXECUTED",
            Self::Mocked => "MOCKED",
            Self::Unknown => "UNKNOWN",
        }
    }
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

/// Provenance of the terminal Candidate submission. `ModelFinal` is the normal
/// provider final path; `StagnationSalvage` is the deterministic Worker
/// completion after Core returned `Diagnose` for a written, fully checked
/// candidate. This is durable evidence, not an authority grant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CandidateFinalSource {
    #[default]
    ModelFinal,
    StagnationSalvage,
}

impl CandidateFinalSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelFinal => "MODEL_FINAL",
            Self::StagnationSalvage => "STAGNATION_SALVAGE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "MODEL_FINAL" => Some(Self::ModelFinal),
            "STAGNATION_SALVAGE" => Some(Self::StagnationSalvage),
            _ => None,
        }
    }
}

/// Typed Gate reason for a stagnation salvage that requires an authenticated
/// owner/human decision before acceptance. The automatic Gate never accepts an
/// R1+ salvage and never merges one.
pub const SALVAGE_HUMAN_REVIEW_REQUIRED: &str = "SALVAGE_HUMAN_REVIEW_REQUIRED";

/// Stable reason code for a salvage submission whose frozen ActionSpec has no
/// acceptance checks. An empty check list cannot prove a candidate.
pub const SALVAGE_EMPTY_FROZEN_CHECKS: &str = "SALVAGE_EMPTY_FROZEN_CHECKS";

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
    let reviewers: Vec<&ReviewerReceipt> = reviewer.into_iter().collect();
    evaluate_quality_gate_with_reviewers(
        action,
        candidate,
        implementer_id,
        verifier,
        &reviewers,
        policy,
    )
}

/// Aggregate multiple independent reviewer receipts. Every reviewer must bind
/// the same exact candidate, be independent of the implementer and verifier,
/// and return executed PASS evidence. One failing reviewer requires repair; a
/// missing, unknown or not-tested reviewer yields NOT_TESTED. Reviewer count
/// never changes the outcome of an otherwise failing receipt.
pub fn evaluate_quality_gate_with_reviewers(
    action: &FrozenActionSpec,
    candidate: &CandidateRevision,
    implementer_id: &str,
    verifier: Option<&VerifierReceipt>,
    reviewers: &[&ReviewerReceipt],
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
    if verifier.receipt_id.trim().is_empty() || verifier.principal_id.trim().is_empty() {
        return decision(GateOutcome::Blocked, "verifier receipt identity is missing");
    }
    if verifier.action_spec_hash != action.hash {
        return decision(GateOutcome::Blocked, "verifier ActionSpec hash mismatch");
    }
    if verifier.candidate_commit_sha != candidate.commit_sha
        || verifier.candidate_tree_sha != candidate.tree_sha
    {
        return decision(GateOutcome::Blocked, "verifier candidate revision mismatch");
    }
    if verifier.principal_id == implementer_id {
        return decision(
            GateOutcome::Blocked,
            "implementer cannot verify or review their own candidate",
        );
    }
    if verifier.evidence == EvidenceKind::Mocked {
        return decision(
            GateOutcome::Blocked,
            "mocked evidence cannot satisfy the quality gate",
        );
    }
    if verifier.evidence == EvidenceKind::Unknown {
        return decision(
            GateOutcome::NotTested,
            "receipt evidence provenance is unknown",
        );
    }
    match verifier.result {
        ReceiptResult::Fail | ReceiptResult::ChangesRequested => {
            return decision(GateOutcome::RepairRequired, "verification found a failure");
        }
        ReceiptResult::NotTested => {
            return decision(GateOutcome::NotTested, "verification was not tested");
        }
        ReceiptResult::Pass => {}
    }

    if reviewers.is_empty() {
        return decision(
            GateOutcome::NotTested,
            "independent reviewer receipt is missing",
        );
    }
    let mut principals = std::collections::BTreeSet::new();
    let mut receipts = std::collections::BTreeSet::new();
    let mut not_tested = false;
    for reviewer in reviewers {
        if reviewer.receipt_id.trim().is_empty() || reviewer.principal_id.trim().is_empty() {
            return decision(GateOutcome::Blocked, "reviewer receipt identity is missing");
        }
        if !principals.insert(reviewer.principal_id.as_str()) {
            return decision(
                GateOutcome::Blocked,
                "reviewer principals must be independent",
            );
        }
        if !receipts.insert(reviewer.receipt_id.as_str()) {
            return decision(GateOutcome::Blocked, "reviewer receipts must be distinct");
        }
        if reviewer.action_spec_hash != action.hash {
            return decision(GateOutcome::Blocked, "reviewer ActionSpec hash mismatch");
        }
        if reviewer.candidate_commit_sha != candidate.commit_sha
            || reviewer.candidate_tree_sha != candidate.tree_sha
        {
            return decision(GateOutcome::Blocked, "reviewer candidate revision mismatch");
        }
        if reviewer.principal_id == implementer_id {
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
        if reviewer.evidence == EvidenceKind::Mocked {
            return decision(
                GateOutcome::Blocked,
                "mocked evidence cannot satisfy the quality gate",
            );
        }
        if reviewer.evidence == EvidenceKind::Unknown {
            not_tested = true;
            continue;
        }
        match reviewer.result {
            ReceiptResult::Fail => {
                return decision(GateOutcome::RepairRequired, "review found a failure");
            }
            ReceiptResult::ChangesRequested => {
                return decision(GateOutcome::RepairRequired, "review requires changes");
            }
            ReceiptResult::NotTested => not_tested = true,
            ReceiptResult::Pass => {}
        }
    }
    if not_tested {
        return decision(
            GateOutcome::NotTested,
            "one or more reviews were not tested",
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

/// Apply the salvage-specific human-review requirement without weakening any
/// existing Gate rule. R0 salvage remains eligible for the normal Gate. R1-R3
/// salvage can never be accepted by the automatic Gate: an accepting or already
/// human-required decision becomes an explicit typed `HUMAN_REQUIRED` hold, and
/// a blocking decision keeps its stronger outcome with the typed reason added.
pub fn apply_salvage_review_policy(
    decision: GateDecision,
    final_source: CandidateFinalSource,
    risk_class: Option<&str>,
) -> GateDecision {
    if final_source != CandidateFinalSource::StagnationSalvage || matches!(risk_class, Some("R0")) {
        return decision;
    }
    let reason = format!(
        "{SALVAGE_HUMAN_REVIEW_REQUIRED}: R1+ stagnation salvage requires an authenticated owner/human review before acceptance; automatic acceptance and merge are disabled"
    );
    match decision.outcome {
        GateOutcome::Accepted | GateOutcome::HumanRequired => GateDecision {
            outcome: GateOutcome::HumanRequired,
            reasons: vec![reason],
        },
        _ => {
            let mut decision = decision;
            decision.reasons.push(reason);
            decision
        }
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

    fn second_reviewer() -> ReviewerReceipt {
        ReviewerReceipt {
            receipt_id: "review-2".into(),
            principal_id: "reviewer-2".into(),
            action_spec_hash: "a".repeat(64),
            candidate_commit_sha: "b".repeat(40),
            candidate_tree_sha: "c".repeat(40),
            result: ReceiptResult::Pass,
            evidence: EvidenceKind::Executed,
        }
    }

    fn multi(reviewers: &[&ReviewerReceipt]) -> GateDecision {
        evaluate_quality_gate_with_reviewers(
            &action(),
            &candidate(),
            "implementer",
            Some(&verifier()),
            reviewers,
            policy(),
        )
    }

    #[test]
    fn multiple_independent_reviewers_all_pass_before_acceptance() {
        assert_eq!(
            multi(&[&reviewer(), &second_reviewer()]).outcome,
            GateOutcome::Accepted
        );
        let mut failed = second_reviewer();
        failed.result = ReceiptResult::ChangesRequested;
        assert_eq!(
            multi(&[&reviewer(), &failed]).outcome,
            GateOutcome::RepairRequired
        );
        let mut untested = second_reviewer();
        untested.result = ReceiptResult::NotTested;
        assert_eq!(
            multi(&[&reviewer(), &untested]).outcome,
            GateOutcome::NotTested
        );
    }

    #[test]
    fn duplicate_reviewer_principals_or_receipts_are_blocked() {
        let duplicate_principal = ReviewerReceipt {
            principal_id: "reviewer".into(),
            ..second_reviewer()
        };
        assert_eq!(
            multi(&[&reviewer(), &duplicate_principal]).outcome,
            GateOutcome::Blocked
        );
        let duplicate_receipt = ReviewerReceipt {
            receipt_id: "review-1".into(),
            ..second_reviewer()
        };
        assert_eq!(
            multi(&[&reviewer(), &duplicate_receipt]).outcome,
            GateOutcome::Blocked
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
    fn failed_verifier_requires_repair_without_a_reviewer() {
        let mut failed = verifier();
        failed.result = ReceiptResult::Fail;
        assert_eq!(
            evaluate(Some(&failed), None).outcome,
            GateOutcome::RepairRequired
        );

        let mut changes = verifier();
        changes.result = ReceiptResult::ChangesRequested;
        assert_eq!(
            evaluate(Some(&changes), None).outcome,
            GateOutcome::RepairRequired
        );

        let mut not_tested = verifier();
        not_tested.result = ReceiptResult::NotTested;
        assert_eq!(
            evaluate(Some(&not_tested), None).outcome,
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

    #[test]
    fn r0_salvage_keeps_the_normal_gate_outcome() {
        let accepted = evaluate(Some(&verifier()), Some(&reviewer()));
        assert_eq!(
            apply_salvage_review_policy(
                accepted.clone(),
                CandidateFinalSource::StagnationSalvage,
                Some("R0")
            ),
            accepted
        );
    }

    #[test]
    fn r1_salvage_can_never_be_accepted_automatically() {
        let reviewed = apply_salvage_review_policy(
            evaluate(Some(&verifier()), Some(&reviewer())),
            CandidateFinalSource::StagnationSalvage,
            Some("R1"),
        );
        assert_eq!(reviewed.outcome, GateOutcome::HumanRequired);
        assert!(
            reviewed
                .reasons
                .iter()
                .any(|reason| reason.contains(SALVAGE_HUMAN_REVIEW_REQUIRED)),
            "the human-required hold must carry the typed salvage reason: {reviewed:?}"
        );

        let blocked = apply_salvage_review_policy(
            GateDecision {
                outcome: GateOutcome::Blocked,
                reasons: vec!["risk exceeds policy".into()],
            },
            CandidateFinalSource::StagnationSalvage,
            Some("R2"),
        );
        assert_eq!(blocked.outcome, GateOutcome::Blocked);
        assert!(
            blocked
                .reasons
                .iter()
                .any(|reason| reason.contains(SALVAGE_HUMAN_REVIEW_REQUIRED))
        );
    }

    #[test]
    fn model_final_is_never_relabelled_as_salvage() {
        let normal = evaluate(Some(&verifier()), Some(&reviewer()));
        assert_eq!(
            apply_salvage_review_policy(
                normal.clone(),
                CandidateFinalSource::ModelFinal,
                Some("R1")
            ),
            normal
        );
    }
}
