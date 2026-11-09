//! Reviewer role definitions, prompts and evidence validation.
//!
//! Reviewer roles are separate read-only sessions and principals. A role
//! prompt is context, never authority; Core validates identities and findings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::planning::{FindingStatus, ReviewerPlan, ReviewerRole};

pub const MAX_PROMPT_CHARS: usize = 4_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewerAssignment {
    pub role: ReviewerRole,
    pub principal_id: String,
    pub provider: String,
    pub model: String,
    pub prompt: String,
}

impl ReviewerAssignment {
    pub fn validate(&self) -> Result<(), String> {
        if self.principal_id.trim().is_empty()
            || self.provider.trim().is_empty()
            || self.model.trim().is_empty()
        {
            return Err("reviewer principal, provider and model are required".into());
        }
        if self.prompt.trim().is_empty() || self.prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err("reviewer prompt is required and bounded".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewerEvidence {
    pub role: ReviewerRole,
    pub principal_id: String,
    pub result: String,
    pub evidence: String,
    pub findings: Vec<ReviewerFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewerFinding {
    pub id: String,
    pub severity: String,
    pub status: FindingStatus,
    pub message: String,
    pub evidence_refs: Vec<String>,
}

impl ReviewerEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if self.principal_id.trim().is_empty()
            || !matches!(
                self.result.as_str(),
                "PASS" | "CHANGES_REQUESTED" | "NOT_TESTED"
            )
            || !matches!(self.evidence.as_str(), "EXECUTED" | "MOCKED" | "UNKNOWN")
        {
            return Err("reviewer evidence identity/result is invalid".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for finding in &self.findings {
            if finding.id.trim().is_empty() || finding.message.trim().is_empty() {
                return Err("reviewer finding id and message are required".into());
            }
            if !ids.insert(finding.id.as_str()) {
                return Err("reviewer finding ids must be unique".into());
            }
            if !matches!(
                finding.severity.as_str(),
                "critical" | "blocking" | "warning" | "info"
            ) {
                return Err("reviewer finding severity is not canonical".into());
            }
            if finding.evidence_refs.is_empty() {
                return Err("every reviewer finding requires evidence references".into());
            }
        }
        if self.result == "PASS"
            && self
                .findings
                .iter()
                .any(|f| f.status == FindingStatus::Open)
        {
            return Err("a passing review cannot carry an open finding".into());
        }
        Ok(())
    }
}

pub fn assignments(
    plan: &ReviewerPlan,
    provider: &str,
    model: &str,
) -> Result<Vec<ReviewerAssignment>, String> {
    plan.validate()?;
    let mut assignments = Vec::new();
    for role in &plan.required {
        assignments.push(ReviewerAssignment {
            role: *role,
            principal_id: format!("reviewer-{}", role_slug(*role)),
            provider: provider.to_owned(),
            model: model.to_owned(),
            prompt: role_prompt(*role),
        });
    }
    for assignment in &assignments {
        assignment.validate()?;
    }
    Ok(assignments)
}

/// Critical assurance requires at least two distinct reviewer provider/model
/// pairs. This is a plan-level check; it never accepts a receipt.
pub fn validate_critical_diversity(assignments: &[ReviewerAssignment]) -> Result<(), String> {
    if assignments.len() < 2 {
        return Err("critical assurance requires at least two reviewers".into());
    }
    let distinct = assignments
        .iter()
        .map(|assignment| (assignment.provider.as_str(), assignment.model.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    if distinct.len() < 2 {
        return Err("critical assurance requires provider or model diversity".into());
    }
    Ok(())
}

pub fn role_slug(role: ReviewerRole) -> &'static str {
    match role {
        ReviewerRole::Functional => "functional",
        ReviewerRole::Architecture => "architecture",
        ReviewerRole::CodeQuality => "code-quality",
        ReviewerRole::Security => "security",
        ReviewerRole::Testing => "testing",
        ReviewerRole::Operability => "operability",
    }
}

pub fn role_prompt(role: ReviewerRole) -> String {
    let focus = match role {
        ReviewerRole::Functional => {
            "Verify observable behavior against the frozen spec, including edge cases, negative cases, empty states and recovery."
        }
        ReviewerRole::Architecture => {
            "Verify component boundaries, interfaces, data ownership, stack/ADR fidelity, coupling, service placement and container-native constraints."
        }
        ReviewerRole::CodeQuality => {
            "Verify readability, naming, error handling, duplication, cohesion, unsafe/global state, dead code, warnings and maintainability."
        }
        ReviewerRole::Security => {
            "Verify secrets, authorization, input validation, path traversal, injection, dependency supply chain, unsafe effects and trust boundaries."
        }
        ReviewerRole::Testing => {
            "Verify requirement-to-check-to-test-to-evidence traceability, coverage gaps, deterministic tests and honest failure behavior."
        }
        ReviewerRole::Operability => {
            "Verify runtime, observability, resource limits, failure recovery, deployment and non-functional requirements."
        }
    };
    format!(
        "You are the independent {role:?} reviewer. You are read-only: never edit code, never accept work, never grant authority. {focus} \
Review the exact frozen candidate, spec/OpenSpec, stack decision, architecture, documentation plan, Definition of Done, traceability, diff and verifier evidence. \
Every finding must have a unique id, canonical severity, concrete evidence references and an actionable message. Any open finding blocks acceptance. \
Do not approve from the candidate description alone; check the exact source. Do not invent files, symbols or requirements. If evidence is insufficient, return NOT_TESTED with a concrete finding. \
Return only the typed reviewer JSON contract."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{PlanningLane, ReviewerPlan};

    #[test]
    fn assignments_are_separate_per_required_role() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        let list = assignments(&plan, "codex", "gpt-6-luna").unwrap();
        assert_eq!(list.len(), plan.required.len());
        let principals: std::collections::BTreeSet<_> =
            list.iter().map(|a| a.principal_id.as_str()).collect();
        assert_eq!(principals.len(), list.len());
        assert!(
            list.iter()
                .all(|a| a.prompt.chars().count() <= MAX_PROMPT_CHARS)
        );
    }

    #[test]
    fn passing_review_rejects_open_findings() {
        let evidence = ReviewerEvidence {
            role: ReviewerRole::Architecture,
            principal_id: "reviewer-architecture".into(),
            result: "PASS".into(),
            evidence: "EXECUTED".into(),
            findings: vec![ReviewerFinding {
                id: "f1".into(),
                severity: "warning".into(),
                status: FindingStatus::Open,
                message: "missing ADR".into(),
                evidence_refs: vec!["src/lib.rs".into()],
            }],
        };
        assert!(evidence.validate().is_err());
    }

    #[test]
    fn duplicate_ids_and_noncanonical_severity_are_rejected() {
        let mut evidence = ReviewerEvidence {
            role: ReviewerRole::Testing,
            principal_id: "reviewer-testing".into(),
            result: "CHANGES_REQUESTED".into(),
            evidence: "EXECUTED".into(),
            findings: vec![
                ReviewerFinding {
                    id: "f1".into(),
                    severity: "warning".into(),
                    status: FindingStatus::Open,
                    message: "one".into(),
                    evidence_refs: vec!["evidence-1".into()],
                },
                ReviewerFinding {
                    id: "f1".into(),
                    severity: "warning".into(),
                    status: FindingStatus::Open,
                    message: "two".into(),
                    evidence_refs: vec!["evidence-2".into()],
                },
            ],
        };
        assert!(evidence.validate().is_err());
        evidence.findings[1].id = "f2".into();
        evidence.findings[0].severity = "severe".into();
        assert!(evidence.validate().is_err());
    }

    #[test]
    fn critical_diversity_requires_two_provider_or_model_pairs() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        let assignments = assignments(&plan, "codex", "gpt-6-luna").unwrap();
        assert!(validate_critical_diversity(&assignments).is_err());
        let mut diverse = assignments;
        diverse[0].model = "other-model".into();
        validate_critical_diversity(&diverse).unwrap();
    }

    #[test]
    fn closed_finding_requires_evidence() {
        let mut evidence = ReviewerEvidence {
            role: ReviewerRole::Security,
            principal_id: "reviewer-security".into(),
            result: "CHANGES_REQUESTED".into(),
            evidence: "EXECUTED".into(),
            findings: vec![ReviewerFinding {
                id: "f1".into(),
                severity: "blocking".into(),
                status: FindingStatus::Resolved,
                message: "fixed".into(),
                evidence_refs: vec!["receipt".into()],
            }],
        };
        assert!(evidence.validate().is_ok());
        evidence.findings[0].evidence_refs.clear();
        assert!(evidence.validate().is_err());
    }
}
