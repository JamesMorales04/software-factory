//! Deterministic repair authorization.
//!
//! Reviewer findings are advisory input. Core recomputes canonical severity
//! points, enforces critical-zero and risk budgets, verifies reviewer
//! independence, and decides whether a bounded repair cycle is authorized.
//! Model output never grants repair, acceptance or delivery authority.

use crate::execution_policy::AutonomyPolicy;
use crate::quality::ReceiptResult;
use crate::review::{FindingSeverity, ReviewFinding};

pub const MAX_REPAIR_POINTS: u16 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FindingScore {
    pub total_points: u16,
    pub critical_count: u32,
    pub blocking_count: u32,
    pub warning_count: u32,
    pub info_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairDecision {
    Authorize { cycle: u32, points: u16 },
    Deny { reason_code: String, reason: String },
    HumanRequired { reason_code: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRequest<'a> {
    pub risk_class: &'a str,
    pub policy: &'a AutonomyPolicy,
    pub findings: &'a [ReviewFinding],
    pub reviewer_result: ReceiptResult,
    pub reviewer_evidence_executed: bool,
    pub reviewer_principal: &'a str,
    pub implementer_principal: &'a str,
    pub verifier_principal: &'a str,
    pub prior_repair_cycles: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum AutonomyError {
    #[error("invalid repair authorization input: {0}")]
    Invalid(String),
}

/// Score findings using only Core-canonical severity weights. A mismatch,
/// missing rationale, overflow or empty findings set fails closed.
pub fn score_findings(findings: &[ReviewFinding]) -> Result<FindingScore, AutonomyError> {
    let mut score = FindingScore::default();
    for finding in findings {
        if finding.points != finding.severity.canonical_points() {
            return Err(AutonomyError::Invalid(
                "finding points do not match the canonical severity weight".into(),
            ));
        }
        if finding.rationale.trim().is_empty() {
            return Err(AutonomyError::Invalid(
                "finding rationale is required for repair authorization".into(),
            ));
        }
        score.total_points = score
            .total_points
            .checked_add(finding.points)
            .ok_or_else(|| AutonomyError::Invalid("finding point total overflowed".into()))?;
        match finding.severity {
            FindingSeverity::Critical => score.critical_count += 1,
            FindingSeverity::Blocking => score.blocking_count += 1,
            FindingSeverity::Warning => score.warning_count += 1,
            FindingSeverity::Info => score.info_count += 1,
        }
    }
    if score.total_points > MAX_REPAIR_POINTS {
        return Err(AutonomyError::Invalid(
            "finding point total exceeds the bounded maximum".into(),
        ));
    }
    Ok(score)
}

pub fn authorize_repair(request: &RepairRequest<'_>) -> Result<RepairDecision, AutonomyError> {
    let score = score_findings(request.findings)?;
    let deny = |code: &str, reason: &str| {
        Ok(RepairDecision::Deny {
            reason_code: code.into(),
            reason: reason.into(),
        })
    };
    let human = |code: &str, reason: &str| {
        Ok(RepairDecision::HumanRequired {
            reason_code: code.into(),
            reason: reason.into(),
        })
    };

    if request.reviewer_principal.trim().is_empty()
        || request.implementer_principal.trim().is_empty()
        || request.verifier_principal.trim().is_empty()
    {
        return deny(
            "identity_missing",
            "repair requires complete independent identities",
        );
    }
    if request.reviewer_principal == request.implementer_principal
        || request.reviewer_principal == request.verifier_principal
    {
        return deny(
            "reviewer_not_independent",
            "reviewer must be independent of the implementer and verifier",
        );
    }
    if !request.reviewer_evidence_executed {
        return deny(
            "review_not_executed",
            "repair requires executed review evidence",
        );
    }
    if request.reviewer_result == ReceiptResult::Pass {
        return deny(
            "contradictory_review",
            "a passing review cannot authorize repair",
        );
    }
    if request.reviewer_result == ReceiptResult::NotTested {
        return deny("review_not_tested", "repair requires a tested review");
    }
    if request.findings.is_empty() {
        return deny(
            "no_actionable_findings",
            "repair requires at least one concrete finding",
        );
    }
    if score.critical_count > 0 {
        return human(
            "critical_zero",
            "a critical finding always requires human review",
        );
    }
    let risk_index = match request.risk_class {
        "R0" => 0,
        "R1" => 1,
        "R2" | "R3" => {
            return human(
                "risk_requires_human",
                "R2 and R3 changes require explicit human authorization",
            );
        }
        _ => {
            return human(
                "unknown_risk",
                "an unknown risk class cannot authorize repair",
            );
        }
    };
    if !request.policy.enabled {
        return deny("autonomy_disabled", "autonomous repair is disabled");
    }
    let point_budget = request.policy.repair_point_budgets[risk_index];
    let cycle_budget = request.policy.repair_cycle_budgets[risk_index];
    if point_budget == 0 || cycle_budget == 0 {
        return deny(
            "risk_budget_missing",
            "the frozen policy has no repair budget for this risk class",
        );
    }
    if score.total_points > point_budget {
        return deny(
            "point_budget_exceeded",
            "findings exceed the frozen repair point budget",
        );
    }
    if request.prior_repair_cycles >= cycle_budget {
        return deny(
            "cycle_budget_exhausted",
            "the frozen repair cycle budget is exhausted",
        );
    }
    Ok(RepairDecision::Authorize {
        cycle: request.prior_repair_cycles + 1,
        points: score.total_points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{FindingCategory, ReviewFinding};

    fn policy() -> AutonomyPolicy {
        AutonomyPolicy {
            enabled: true,
            repair_point_budgets: [8, 5, 0, 0],
            repair_cycle_budgets: [2, 1, 0, 0],
        }
    }

    fn finding(severity: FindingSeverity) -> ReviewFinding {
        ReviewFinding {
            id: format!("{severity:?}"),
            severity,
            category: FindingCategory::Functional,
            message: "concrete defect".into(),
            points: severity.canonical_points(),
            rationale: "the exact diff demonstrates the defect".into(),
            evidence_refs: vec!["src/lib.rs:1".into()],
        }
    }

    fn repair_request<'a>(
        risk: &'a str,
        findings: &'a [ReviewFinding],
        policy: &'a AutonomyPolicy,
    ) -> RepairRequest<'a> {
        RepairRequest {
            risk_class: risk,
            policy,
            findings,
            reviewer_result: ReceiptResult::ChangesRequested,
            reviewer_evidence_executed: true,
            reviewer_principal: "reviewer",
            implementer_principal: "implementer",
            verifier_principal: "verifier",
            prior_repair_cycles: 0,
        }
    }

    #[test]
    fn canonical_points_and_budget_authorize_one_cycle() {
        let policy = policy();
        let findings = [finding(FindingSeverity::Blocking)];
        let decision = authorize_repair(&repair_request("R0", &findings, &policy)).unwrap();
        assert_eq!(
            decision,
            RepairDecision::Authorize {
                cycle: 1,
                points: 5
            }
        );
    }

    #[test]
    fn critical_findings_are_a_hard_human_stop() {
        let policy = policy();
        let findings = [finding(FindingSeverity::Critical)];
        assert!(matches!(
            authorize_repair(&repair_request("R0", &findings, &policy)).unwrap(),
            RepairDecision::HumanRequired { .. }
        ));
    }

    #[test]
    fn points_and_cycles_are_bounded_by_the_frozen_policy() {
        let policy = policy();
        let findings = [
            finding(FindingSeverity::Blocking),
            finding(FindingSeverity::Warning),
            finding(FindingSeverity::Warning),
        ];
        assert!(matches!(
            authorize_repair(&repair_request("R0", &findings, &policy)).unwrap(),
            RepairDecision::Deny { .. }
        ));

        let findings = [finding(FindingSeverity::Warning)];
        let mut request = repair_request("R0", &findings, &policy);
        request.prior_repair_cycles = 2;
        assert!(matches!(
            authorize_repair(&request).unwrap(),
            RepairDecision::Deny { .. }
        ));
    }

    #[test]
    fn independence_and_contradictory_results_fail_closed() {
        let policy = policy();
        let findings = [finding(FindingSeverity::Warning)];
        let mut request = repair_request("R0", &findings, &policy);
        request.reviewer_principal = "implementer";
        assert!(matches!(
            authorize_repair(&request).unwrap(),
            RepairDecision::Deny { .. }
        ));

        let mut request = repair_request("R0", &findings, &policy);
        request.reviewer_result = ReceiptResult::Pass;
        assert!(matches!(
            authorize_repair(&request).unwrap(),
            RepairDecision::Deny { .. }
        ));
    }

    #[test]
    fn disabled_or_unknown_risk_policies_cannot_authorize() {
        let findings = [finding(FindingSeverity::Warning)];
        let disabled = AutonomyPolicy::default();
        assert!(matches!(
            authorize_repair(&repair_request("R0", &findings, &disabled)).unwrap(),
            RepairDecision::Deny { .. }
        ));

        let policy = policy();
        assert!(matches!(
            authorize_repair(&repair_request("R2", &findings, &policy)).unwrap(),
            RepairDecision::HumanRequired { .. }
        ));
        assert!(matches!(
            authorize_repair(&repair_request("unknown", &findings, &policy)).unwrap(),
            RepairDecision::HumanRequired { .. }
        ));
    }
}
