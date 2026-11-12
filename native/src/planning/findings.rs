//! Deterministic Finding Router.
//!
//! Every reviewer finding is routed to the role that can resolve it. Routing is
//! a Core decision derived from the finding category; a model never chooses the
//! route, closes a finding or grants a repair cycle. A finding remains blocking
//! until its durable disposition is `RESOLVED` with evidence or `REFUTED` by an
//! authenticated owner who is not the reviewer.

use crate::planning::{FindingDisposition, FindingRoute, FindingStatus};
use crate::review::{FindingCategory, FindingSeverity, ReviewFinding};

/// Route a finding category to the role that owns the correction.
pub fn route_for_category(category: FindingCategory) -> FindingRoute {
    match category {
        FindingCategory::Functional => FindingRoute::Repairer,
        FindingCategory::Architecture => FindingRoute::Planner,
        FindingCategory::Security => FindingRoute::SecurityReviewer,
        FindingCategory::Testing => FindingRoute::TestPlanner,
        FindingCategory::Scope => FindingRoute::SpecManager,
        FindingCategory::Other => FindingRoute::Documentation,
    }
}

/// Stable SQL representation of a finding route.
pub fn route_code(route: FindingRoute) -> &'static str {
    match route {
        FindingRoute::Repairer => "REPAIRER",
        FindingRoute::TestPlanner => "TEST_PLANNER",
        FindingRoute::Planner => "PLANNER",
        FindingRoute::SpecManager => "SPEC_MANAGER",
        FindingRoute::SecurityReviewer => "SECURITY_REVIEWER",
        FindingRoute::Documentation => "DOCUMENTATION",
    }
}

/// Parse a durable route code. Unknown values fail closed.
pub fn parse_route_code(value: &str) -> Option<FindingRoute> {
    match value {
        "REPAIRER" => Some(FindingRoute::Repairer),
        "TEST_PLANNER" => Some(FindingRoute::TestPlanner),
        "PLANNER" => Some(FindingRoute::Planner),
        "SPEC_MANAGER" => Some(FindingRoute::SpecManager),
        "SECURITY_REVIEWER" => Some(FindingRoute::SecurityReviewer),
        "DOCUMENTATION" => Some(FindingRoute::Documentation),
        _ => None,
    }
}

/// Build the initial OPEN disposition for a reviewer finding.
pub fn open_disposition(
    finding: &ReviewFinding,
    request_id: &str,
    candidate_id: &str,
    reviewer_principal: &str,
) -> FindingDisposition {
    FindingDisposition {
        request_id: request_id.to_string(),
        finding_id: finding.id.clone(),
        route: route_for_category(finding.category),
        status: FindingStatus::Open,
        candidate_id: candidate_id.to_string(),
        reviewer_principal: reviewer_principal.to_string(),
        resolved_by: String::new(),
        evidence_refs: Vec::new(),
        owner_principal: None,
        owner_decision: None,
    }
}

/// A finding blocks the Gate unless its disposition is closed.
pub fn finding_blocks(disposition: Option<&FindingDisposition>) -> bool {
    disposition.is_none_or(|disposition| disposition.status == FindingStatus::Open)
}

/// Critical findings always require an owner decision even when the route is
/// deterministic; the Gate must not silently accept them.
pub fn requires_owner(finding: &ReviewFinding) -> bool {
    finding.severity == FindingSeverity::Critical
}

/// Validate a closure or refutation against the finding's reviewer identity.
pub fn validate_closure(
    finding: &ReviewFinding,
    disposition: &FindingDisposition,
    authenticated_principal: &str,
) -> Result<(), String> {
    disposition.validate()?;
    if disposition.finding_id != finding.id {
        return Err("finding disposition does not match the finding".into());
    }
    if disposition.status == FindingStatus::Resolved {
        if disposition.resolved_by != authenticated_principal {
            return Err("the resolving principal must be the authenticated operator".into());
        }
        if disposition.evidence_refs.is_empty() {
            return Err("a resolved finding requires evidence".into());
        }
    }
    if disposition.status == FindingStatus::Refuted {
        let owner = disposition
            .owner_principal
            .as_deref()
            .ok_or_else(|| "a refutation requires an owner principal".to_string())?;
        if owner != authenticated_principal {
            return Err("the refuting principal must be the authenticated owner".into());
        }
        if owner == disposition.reviewer_principal {
            return Err("a reviewer cannot refute its own finding".into());
        }
        if requires_owner(finding) {
            // Critical findings may be refuted only by an owner decision; the
            // normal validation already requires the decision reference.
            if disposition
                .owner_decision
                .as_deref()
                .is_none_or(str::is_empty)
            {
                return Err("a critical finding refutation requires an owner decision".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(category: FindingCategory, severity: FindingSeverity) -> ReviewFinding {
        ReviewFinding {
            id: "F-001".into(),
            severity,
            category,
            message: "concrete defect".into(),
            points: severity.canonical_points(),
            rationale: "the diff demonstrates the defect".into(),
            evidence_refs: vec!["src/lib.rs:1".into()],
        }
    }

    #[test]
    fn every_category_has_a_deterministic_route() {
        assert_eq!(
            route_for_category(FindingCategory::Functional),
            FindingRoute::Repairer
        );
        assert_eq!(
            route_for_category(FindingCategory::Architecture),
            FindingRoute::Planner
        );
        assert_eq!(
            route_for_category(FindingCategory::Security),
            FindingRoute::SecurityReviewer
        );
        assert_eq!(
            route_for_category(FindingCategory::Testing),
            FindingRoute::TestPlanner
        );
        assert_eq!(
            route_for_category(FindingCategory::Scope),
            FindingRoute::SpecManager
        );
        assert_eq!(
            route_for_category(FindingCategory::Other),
            FindingRoute::Documentation
        );
    }

    #[test]
    fn open_findings_block_until_closed_with_evidence() {
        let finding = finding(FindingCategory::Functional, FindingSeverity::Warning);
        let mut disposition = open_disposition(&finding, "request-1", "candidate-1", "reviewer-1");
        assert!(finding_blocks(Some(&disposition)));
        disposition.status = FindingStatus::Resolved;
        disposition.resolved_by = "operator".into();
        disposition.evidence_refs = vec!["test:exporter_returns_calendar".into()];
        validate_closure(&finding, &disposition, "operator").unwrap();
        assert!(!finding_blocks(Some(&disposition)));
    }

    #[test]
    fn a_reviewer_cannot_refute_its_own_finding() {
        let finding = finding(FindingCategory::Security, FindingSeverity::Critical);
        let mut disposition = open_disposition(&finding, "request-1", "candidate-1", "reviewer-1");
        disposition.status = FindingStatus::Refuted;
        disposition.owner_principal = Some("reviewer-1".into());
        disposition.owner_decision = Some("owner-decision-1".into());
        disposition.evidence_refs = vec!["evidence".into()];
        assert!(validate_closure(&finding, &disposition, "reviewer-1").is_err());
    }
}
