//! Reviewer role definitions, prompts and evidence validation.
//!
//! Reviewer roles are separate read-only sessions and principals. A role
//! prompt is context, never authority; Core validates identities and findings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::planning::{FindingStatus, ReviewerPlan, ReviewerRole};

pub const MAX_PROMPT_CHARS: usize = 4_000;

/// A concrete role-to-identity assignment compiled by Core from trusted
/// deployment configuration. It carries no prompt and no authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewerIdentityAssignment {
    pub role: ReviewerRole,
    pub principal_id: String,
    pub provider: String,
    pub model: String,
}

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

/// Validate reviewer independence from authenticated identity, not from
/// provider/model choice.
///
/// The required reviewer count and the required role set both come from the
/// frozen [`ReviewerPlan`]. Every required role needs exactly one assignment,
/// every assignment needs a unique non-empty principal, and no reviewer may
/// share a principal with the implementer, the verifier or another reviewer.
/// Provider and model may be identical across assignments.
pub fn validate_reviewer_independence(
    plan: &ReviewerPlan,
    assignments: &[ReviewerIdentityAssignment],
    implementer_principal: &str,
    verifier_principal: &str,
) -> Result<(), String> {
    plan.validate()?;
    let required = plan.required_count();
    if assignments.len() < required {
        return Err(format!(
            "reviewer independence requires {required} independent reviewer identities"
        ));
    }
    if implementer_principal.trim().is_empty() || verifier_principal.trim().is_empty() {
        return Err("implementer and verifier principals are required".into());
    }
    let mut principals = std::collections::BTreeSet::new();
    let mut roles = std::collections::BTreeSet::new();
    for assignment in assignments {
        if assignment.principal_id.trim().is_empty() {
            return Err("every reviewer assignment requires a non-empty principal".into());
        }
        if !plan.required.contains(&assignment.role) {
            return Err(format!(
                "reviewer role {:?} is not required by the frozen plan",
                assignment.role
            ));
        }
        if !roles.insert(assignment.role) {
            return Err(format!(
                "reviewer role {:?} has more than one assignment",
                assignment.role
            ));
        }
        if !principals.insert(assignment.principal_id.as_str()) {
            return Err("reviewer principals must be independent and unique".into());
        }
        if assignment.principal_id == implementer_principal {
            return Err("the implementer cannot be assigned a reviewer role".into());
        }
        if assignment.principal_id == verifier_principal {
            return Err("the verifier cannot be assigned a reviewer role".into());
        }
    }
    for role in &plan.required {
        if !roles.contains(role) {
            return Err(format!("required reviewer role {role:?} has no assignment"));
        }
    }
    Ok(())
}

/// Non-blocking observation: whether the assignments happen to use more than
/// one provider/model pair. It never affects plan or Gate acceptance.
pub fn model_diversity_observation(assignments: &[ReviewerIdentityAssignment]) -> bool {
    assignments
        .iter()
        .map(|assignment| (assignment.provider.as_str(), assignment.model.as_str()))
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1
}

/// Compile one identity assignment per required role from a trusted
/// role-to-principal map. Missing roles fail closed.
pub fn identity_assignments(
    plan: &ReviewerPlan,
    provider: &str,
    model: &str,
    principals: &std::collections::BTreeMap<ReviewerRole, String>,
) -> Result<Vec<ReviewerIdentityAssignment>, String> {
    plan.validate()?;
    let mut assignments = Vec::new();
    for role in &plan.required {
        let principal = principals
            .get(role)
            .filter(|principal| !principal.trim().is_empty())
            .ok_or_else(|| format!("required reviewer role {role:?} has no identity"))?;
        assignments.push(ReviewerIdentityAssignment {
            role: *role,
            principal_id: principal.clone(),
            provider: provider.to_owned(),
            model: model.to_owned(),
        });
    }
    Ok(assignments)
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

    fn identities(plan: &ReviewerPlan) -> Vec<ReviewerIdentityAssignment> {
        plan.required
            .iter()
            .map(|role| ReviewerIdentityAssignment {
                role: *role,
                principal_id: format!("reviewer-{}", role_slug(*role)),
                provider: "codex".into(),
                model: "gpt-6-luna".into(),
            })
            .collect()
    }

    #[test]
    fn same_provider_and_model_with_distinct_principals_is_independent() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        let assignments = identities(&plan);
        validate_reviewer_independence(&plan, &assignments, "implementer", "verifier").unwrap();
        assert!(!model_diversity_observation(&assignments));
    }

    #[test]
    fn duplicate_reviewer_principal_is_rejected() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        let mut assignments = identities(&plan);
        assignments[1].principal_id = assignments[0].principal_id.clone();
        let error = validate_reviewer_independence(&plan, &assignments, "implementer", "verifier")
            .unwrap_err();
        assert!(error.contains("independent and unique"));
    }

    #[test]
    fn one_reviewer_for_critical_or_elevated_is_rejected() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        assert!(plan.required_count() >= 2);
        let assignments = identities(&plan);
        let error =
            validate_reviewer_independence(&plan, &assignments[..1], "implementer", "verifier")
                .unwrap_err();
        assert!(error.contains("independent reviewer identities"));
    }

    #[test]
    fn implementer_or_verifier_collusion_is_rejected() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        let assignments = identities(&plan);
        let mut implementer = assignments.clone();
        implementer[0].principal_id = "implementer".into();
        assert!(
            validate_reviewer_independence(&plan, &implementer, "implementer", "verifier")
                .unwrap_err()
                .contains("implementer")
        );
        let mut verifier = assignments;
        verifier[0].principal_id = "verifier".into();
        assert!(
            validate_reviewer_independence(&plan, &verifier, "implementer", "verifier")
                .unwrap_err()
                .contains("verifier")
        );
    }

    #[test]
    fn identity_assignments_fail_closed_when_a_role_is_missing() {
        let plan = ReviewerPlan::for_lane(PlanningLane::FormalSpec, true);
        let mut principals = std::collections::BTreeMap::new();
        principals.insert(ReviewerRole::Functional, "reviewer-functional".into());
        assert!(identity_assignments(&plan, "codex", "gpt-6-luna", &principals).is_err());
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
