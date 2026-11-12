//! Planning lane floors and route decisions.
//!
//! The route is deterministic: [`crate::planning::deterministic_lane`] is the
//! floor and an owner request may only raise it. In particular, an owner
//! request for `DIRECT` can never turn greenfield, large, missing-docs,
//! public-contract, data/schema, security-boundary, cross-module or multi-unit
//! work into a shallow plan.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::agentic::Assurance;
use crate::planning::{PlanningLane, PlanningSignals, ReviewerPlan, deterministic_lane};

/// An artifact that a planning lane must produce before its plan can be frozen.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanningArtifactKind {
    Spec,
    Stack,
    Architecture,
    Documentation,
    Skills,
    WorkGraph,
    DoD,
    OpenSpec,
}

/// Owner-supplied planning constraints.
///
/// These are advisory inputs. [`route_for`] applies the deterministic floor and
/// only ever raises the lane; it never treats an owner preference as a reason
/// to weaken planning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UserPlanningConstraints {
    /// Requested lane, where `None` means automatic routing. A requested
    /// `DIRECT`/`DELEGATED` lane cannot lower a `FORMAL_SPEC` floor. `REPAIR`
    /// requires an explicit repair signal and is rejected here otherwise.
    #[serde(default)]
    pub requested_lane: Option<PlanningLane>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub framework: Option<String>,
    #[serde(default)]
    pub deploy_target: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
}

/// Inputs to the deterministic planning route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PlanningRouteInput {
    /// Observed and assessed planning signals. The deterministic floor is
    /// derived from these values alone.
    pub signals: PlanningSignals,
    #[serde(default)]
    pub user_constraints: UserPlanningConstraints,
}

/// Resolve the effective planning lane.
///
/// The deterministic floor always wins. An owner may request a deeper lane
/// (`DIRECT` -> `DELEGATED`/`FORMAL_SPEC`, or `DELEGATED` -> `FORMAL_SPEC`),
/// but may never request a shallower one. An explicit repair signal always
/// resolves to `REPAIR`; requesting `REPAIR` without that signal is rejected
/// rather than silently treating ordinary work as a repair.
pub fn route_for(input: &PlanningRouteInput) -> Result<PlanningLane, String> {
    let floor = deterministic_lane(&input.signals)?;
    if floor == PlanningLane::Repair {
        return Ok(PlanningLane::Repair);
    }
    let Some(requested) = input.user_constraints.requested_lane else {
        return Ok(floor);
    };
    if requested == PlanningLane::Repair {
        return Err("the REPAIR lane requires an explicit repair signal".into());
    }
    Ok(if lane_rank(requested) > lane_rank(floor) {
        requested
    } else {
        floor
    })
}

/// Artifacts that the lane must produce. Conditional artifacts (for example a
/// stack revision for a repair that changes the stack) are added by the
/// Finding Router; this is the minimum always-required set.
pub fn required_artifacts(lane: PlanningLane) -> Vec<PlanningArtifactKind> {
    use PlanningArtifactKind::*;
    match lane {
        PlanningLane::Direct => vec![Spec, Documentation, WorkGraph, DoD],
        PlanningLane::Delegated => {
            vec![Spec, Stack, Architecture, Documentation, WorkGraph, DoD]
        }
        PlanningLane::FormalSpec => vec![
            Spec,
            Stack,
            Architecture,
            Documentation,
            Skills,
            WorkGraph,
            DoD,
            OpenSpec,
        ],
        PlanningLane::Repair => vec![Spec, Documentation, WorkGraph, DoD],
    }
}

/// Required and optional reviewer roles for the lane and assurance floor.
pub fn required_reviewers(lane: PlanningLane, assurance: Assurance) -> ReviewerPlan {
    ReviewerPlan::for_lane(lane, assurance >= Assurance::Elevated)
}

/// Plan review is mandatory for every lane, including `DIRECT` and `REPAIR`.
/// Core may auto-approve under a frozen charter, but it never skips review.
pub fn plan_review_required(_lane: PlanningLane) -> bool {
    true
}

/// OpenSpec is mandatory for `FORMAL_SPEC`. `DELEGATED` and `DIRECT` may use a
/// compact Core plan or an optional OpenSpec change; `REPAIR` follows the
/// finding's scope.
pub fn openspec_required(lane: PlanningLane) -> bool {
    matches!(lane, PlanningLane::FormalSpec)
}

fn lane_rank(lane: PlanningLane) -> u8 {
    match lane {
        PlanningLane::Direct => 0,
        PlanningLane::Delegated => 1,
        PlanningLane::FormalSpec => 2,
        PlanningLane::Repair => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::ReviewerRole;

    fn signals() -> PlanningSignals {
        PlanningSignals {
            objective: "bounded change".into(),
            greenfield: false,
            large: false,
            missing_docs: false,
            missing_stack: false,
            architecture_uncertainty: false,
            public_contract: false,
            data_or_schema: false,
            security_boundary: false,
            cross_module: false,
            changed_files: 1,
            expected_units: 0,
            explicit_repair: false,
        }
    }

    fn input(signals: PlanningSignals) -> PlanningRouteInput {
        PlanningRouteInput {
            signals,
            user_constraints: UserPlanningConstraints::default(),
        }
    }

    fn requested(signals: PlanningSignals, lane: PlanningLane) -> PlanningRouteInput {
        PlanningRouteInput {
            signals,
            user_constraints: UserPlanningConstraints {
                requested_lane: Some(lane),
                ..UserPlanningConstraints::default()
            },
        }
    }

    #[test]
    fn greenfield_routes_formal_spec() {
        let mut signals = signals();
        signals.greenfield = true;
        signals.missing_stack = true;
        assert_eq!(
            route_for(&input(signals)).unwrap(),
            PlanningLane::FormalSpec
        );
    }

    #[test]
    fn large_objective_routes_formal_spec() {
        let mut signals = signals();
        signals.large = true;
        assert_eq!(
            route_for(&input(signals)).unwrap(),
            PlanningLane::FormalSpec
        );
    }

    #[test]
    fn missing_docs_routes_formal_spec() {
        let mut signals = signals();
        signals.missing_docs = true;
        assert_eq!(
            route_for(&input(signals)).unwrap(),
            PlanningLane::FormalSpec
        );
    }

    #[test]
    fn user_direct_cannot_lower_a_formal_spec_floor() {
        let mut signals = signals();
        signals.greenfield = true;
        signals.missing_docs = true;
        let routed = route_for(&requested(signals, PlanningLane::Direct)).unwrap();
        assert_eq!(routed, PlanningLane::FormalSpec);
    }

    #[test]
    fn user_can_raise_a_direct_floor() {
        let routed = route_for(&requested(signals(), PlanningLane::FormalSpec)).unwrap();
        assert_eq!(routed, PlanningLane::FormalSpec);
        let routed = route_for(&requested(signals(), PlanningLane::Delegated)).unwrap();
        assert_eq!(routed, PlanningLane::Delegated);
    }

    #[test]
    fn bounded_existing_work_routes_direct() {
        assert_eq!(route_for(&input(signals())).unwrap(), PlanningLane::Direct);
    }

    #[test]
    fn explicit_repair_routes_repair_and_cannot_be_lowered() {
        let mut signals = signals();
        signals.explicit_repair = true;
        assert_eq!(
            route_for(&input(signals.clone())).unwrap(),
            PlanningLane::Repair
        );
        assert_eq!(
            route_for(&requested(signals, PlanningLane::Direct)).unwrap(),
            PlanningLane::Repair
        );
    }

    #[test]
    fn requested_repair_without_a_repair_signal_is_rejected() {
        let error = route_for(&requested(signals(), PlanningLane::Repair)).unwrap_err();
        assert!(error.contains("explicit repair signal"));
    }

    #[test]
    fn formal_spec_requires_every_artifact_including_openspec() {
        let artifacts = required_artifacts(PlanningLane::FormalSpec);
        for kind in [
            PlanningArtifactKind::Spec,
            PlanningArtifactKind::Stack,
            PlanningArtifactKind::Architecture,
            PlanningArtifactKind::Documentation,
            PlanningArtifactKind::Skills,
            PlanningArtifactKind::WorkGraph,
            PlanningArtifactKind::DoD,
            PlanningArtifactKind::OpenSpec,
        ] {
            assert!(artifacts.contains(&kind), "missing {kind:?}");
        }
    }

    #[test]
    fn shallow_lanes_do_not_require_openspec_or_full_stack_artifacts() {
        let direct = required_artifacts(PlanningLane::Direct);
        assert!(direct.contains(&PlanningArtifactKind::WorkGraph));
        assert!(direct.contains(&PlanningArtifactKind::DoD));
        assert!(!direct.contains(&PlanningArtifactKind::OpenSpec));
        assert!(!direct.contains(&PlanningArtifactKind::Stack));

        let delegated = required_artifacts(PlanningLane::Delegated);
        assert!(delegated.contains(&PlanningArtifactKind::Architecture));
        assert!(!delegated.contains(&PlanningArtifactKind::OpenSpec));
    }

    #[test]
    fn plan_review_is_required_for_every_lane() {
        for lane in [
            PlanningLane::Direct,
            PlanningLane::Delegated,
            PlanningLane::FormalSpec,
            PlanningLane::Repair,
        ] {
            assert!(plan_review_required(lane));
        }
    }

    #[test]
    fn openspec_is_only_mandatory_for_formal_spec() {
        assert!(openspec_required(PlanningLane::FormalSpec));
        assert!(!openspec_required(PlanningLane::Direct));
        assert!(!openspec_required(PlanningLane::Delegated));
        assert!(!openspec_required(PlanningLane::Repair));
    }

    #[test]
    fn reviewers_follow_the_lane_and_escalate_operability() {
        let standard = required_reviewers(PlanningLane::FormalSpec, Assurance::Standard);
        assert!(standard.validate().is_ok());
        assert!(standard.required.contains(&ReviewerRole::Security));
        assert!(!standard.roles.contains(&ReviewerRole::Operability));

        let elevated = required_reviewers(PlanningLane::FormalSpec, Assurance::Elevated);
        assert!(elevated.validate().is_ok());
        assert!(elevated.roles.contains(&ReviewerRole::Operability));
        assert!(elevated.required.contains(&ReviewerRole::Operability));
        assert_eq!(elevated.required_count(), 6);

        let direct = required_reviewers(PlanningLane::Direct, Assurance::None);
        assert_eq!(
            direct.required,
            vec![ReviewerRole::Functional, ReviewerRole::CodeQuality]
        );
    }
}
