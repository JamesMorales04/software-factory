//! Unified intake domain.
//!
//! An intake request is an operator intent bound to one Project, one execution
//! profile revision and one durable Lead request. It does not own a lifecycle:
//! the derived state, withdrawal fact, frozen plan and Campaign/gate outcome
//! remain authoritative in the Lead and Campaign records.
//!
//! Decision mechanisms in this module are `DETERMINISTIC`: state projection,
//! authorization reason codes and execution-scope derivation are mechanical
//! facts, never model judgments. Approval freezes scope, tools, risk, route and
//! technical operation limits; deterministic progress supervision decides
//! continuation.

use serde::{Deserialize, Serialize};

use crate::execution_policy::{OperationLimits, ProgressPolicy};
use crate::execution_profile::ExecutionProfileContent;
use crate::lead::{
    Capability, LeadPlanProposal, ObjectiveClass, ProviderRoute, RiskClass, StandingDelegationV2,
};

/// Core-owned validity of one explicit execution-policy grant. This is a
/// grant-lifetime bound, not a Campaign productivity deadline.
pub const EXECUTION_POLICY_VALIDITY_DAYS: i64 = 30;
/// Bounded concurrent implementation capacity. It is not a lifetime work cap.
pub const EXECUTION_POLICY_MAX_PARALLEL: u32 = 1;

/// Material bounds of the non-autonomous grant that an operator is
/// acknowledging. This is a projection of Core policy, not a second authority.
/// It contains no turns, attempts, sessions, repair cycles, universal Campaign
/// deadline, cumulative tokens or per-role money.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeExecutionPolicyTerms {
    pub objective_classes: Vec<ObjectiveClass>,
    pub risk_classes: Vec<RiskClass>,
    pub autonomous_start: bool,
    pub validity_days: i64,
    pub max_parallel: u32,
    /// Per-operation timeout, not a universal Campaign deadline.
    pub max_timeout_secs: u32,
    pub progress: ProgressPolicy,
    pub operations: OperationLimits,
}

/// Derive the exact bounded grant terms from one profile revision. The
/// execution-policy writer and the `EXECUTION_POLICY_REQUIRED` projection both
/// use this function so the acknowledgement cannot disagree with the grant.
pub fn execution_policy_terms(profile: &ExecutionProfileContent) -> IntakeExecutionPolicyTerms {
    IntakeExecutionPolicyTerms {
        objective_classes: vec![
            ObjectiveClass::Feature,
            ObjectiveClass::Maintenance,
            ObjectiveClass::Refactor,
        ],
        risk_classes: vec![RiskClass::R0],
        autonomous_start: false,
        validity_days: EXECUTION_POLICY_VALIDITY_DAYS,
        max_parallel: EXECUTION_POLICY_MAX_PARALLEL,
        max_timeout_secs: profile.operations.tool_call_timeout_secs.get(),
        progress: profile.progress,
        operations: profile.operations,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntakeMode {
    ReviewPlan,
    Autonomous,
}

impl IntakeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReviewPlan => "REVIEW_PLAN",
            Self::Autonomous => "AUTONOMOUS",
        }
    }
}

/// Derived operator-facing intake state. This is a projection, not a persisted
/// second lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntakeState {
    Submitted,
    Assessing,
    PlanReady,
    AwaitingApproval,
    Executing,
    Blocked,
    Failed,
    Completed,
    Cancelled,
}

impl IntakeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "SUBMITTED",
            Self::Assessing => "ASSESSING",
            Self::PlanReady => "PLAN_READY",
            Self::AwaitingApproval => "AWAITING_APPROVAL",
            Self::Executing => "EXECUTING",
            Self::Blocked => "BLOCKED",
            Self::Failed => "FAILED",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// Stable reason codes surfaced to Office. A reason is diagnostic metadata; it
/// never grants authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeAuthorizationReason {
    ExecutionPolicyRequired,
    ExecutionProfileStale,
    ExecutionProfileNotExecutable,
    ProjectConfigChanged,
    DelegationRevoked,
    DelegationExpired,
    DelegationCapacityExhausted,
    ExecutionPolicyScopeChanged,
    FrozenPlanRequired,
    PlanNotReady,
    Withdrawn,
    CampaignStarted,
    RequestBlocked,
    RequestFailed,
}

impl IntakeAuthorizationReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionPolicyRequired => "EXECUTION_POLICY_REQUIRED",
            Self::ExecutionProfileStale => "EXECUTION_PROFILE_STALE",
            Self::ExecutionProfileNotExecutable => "EXECUTION_PROFILE_NOT_EXECUTABLE",
            Self::ProjectConfigChanged => "PROJECT_CONFIG_CHANGED",
            Self::DelegationRevoked => "DELEGATION_REVOKED",
            Self::DelegationExpired => "DELEGATION_EXPIRED",
            Self::DelegationCapacityExhausted => "DELEGATION_CAPACITY_EXHAUSTED",
            Self::ExecutionPolicyScopeChanged => "EXECUTION_POLICY_SCOPE_CHANGED",
            Self::FrozenPlanRequired => "FROZEN_PLAN_REQUIRED",
            Self::PlanNotReady => "PLAN_NOT_READY",
            Self::Withdrawn => "INTAKE_WITHDRAWN",
            Self::CampaignStarted => "CAMPAIGN_STARTED",
            Self::RequestBlocked => "REQUEST_BLOCKED",
            Self::RequestFailed => "REQUEST_FAILED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeAuthorizationState {
    pub required: bool,
    pub available: bool,
    pub reason: Option<String>,
}

impl IntakeAuthorizationState {
    pub fn available() -> Self {
        Self {
            required: true,
            available: true,
            reason: None,
        }
    }

    pub fn blocked(reason: IntakeAuthorizationReason) -> Self {
        Self {
            required: true,
            available: false,
            reason: Some(reason.as_str().to_owned()),
        }
    }

    pub fn not_required() -> Self {
        Self {
            required: false,
            available: false,
            reason: None,
        }
    }
}

/// The exact frozen scope of an operator-approved Campaign. Productivity
/// counters are deliberately absent: an approval grants scope, tools, risk,
/// route and technical operation bounds, and progress supervision decides
/// continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeExecutionScope {
    pub risk_class: RiskClass,
    pub route: ProviderRoute,
    pub allowed_paths: Vec<String>,
    pub capabilities: Vec<Capability>,
    pub max_parallel: u32,
    pub max_timeout_secs: u32,
    pub progress: ProgressPolicy,
    pub operations: OperationLimits,
}

/// Derive the implementation scope for an explicit operator approval. This
/// never broadens the frozen plan or delegation. Insufficient scope is a typed
/// refusal; the caller must not invent a cap or silently widen authority.
pub fn validate_execution_scope(
    profile: &ExecutionProfileContent,
    proposal: &LeadPlanProposal,
    grant: &StandingDelegationV2,
    behavior: &IntakeMode,
) -> Result<IntakeExecutionScope, IntakeScopeError> {
    // Autonomous submission mode is not a start authority. It is accepted as
    // an operator intent label only; this derivation is always the explicit
    // operator path.
    let _ = behavior;
    if proposal.risk_class != RiskClass::R0 || !grant.risk_classes.contains(&RiskClass::R0) {
        return Err(IntakeScopeError::NotR0);
    }
    let implementer = &profile.roles.implementer;
    if implementer.provider != grant.route.provider
        || implementer.model != grant.route.model
        || grant.route.mode != "build"
        || proposal.route != grant.route
    {
        return Err(IntakeScopeError::RouteMismatch);
    }
    if proposal.max_parallel == 0
        || proposal.max_parallel > grant.max_parallel
        || proposal.max_parallel > EXECUTION_POLICY_MAX_PARALLEL
    {
        return Err(IntakeScopeError::ParallelUnavailable);
    }
    if proposal.budget.timeout_secs == 0
        || proposal.budget.timeout_secs > grant.max_timeout_secs
        || proposal.budget.timeout_secs > profile.operations.tool_call_timeout_secs.get()
    {
        return Err(IntakeScopeError::OperationTimeout);
    }
    Ok(IntakeExecutionScope {
        risk_class: RiskClass::R0,
        route: grant.route.clone(),
        allowed_paths: grant.allowed_paths.clone(),
        capabilities: grant.capabilities.clone(),
        max_parallel: proposal.max_parallel.min(EXECUTION_POLICY_MAX_PARALLEL),
        max_timeout_secs: proposal.budget.timeout_secs,
        progress: profile.progress,
        operations: profile.operations,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeScopeError {
    NotR0,
    RouteMismatch,
    ParallelUnavailable,
    OperationTimeout,
}

impl IntakeScopeError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotR0 => "AUTHORIZATION_UNAVAILABLE_RISK",
            Self::RouteMismatch => "AUTHORIZATION_UNAVAILABLE_ROUTE",
            Self::ParallelUnavailable => "AUTHORIZATION_UNAVAILABLE_CAPACITY",
            Self::OperationTimeout => "AUTHORIZATION_UNAVAILABLE_OPERATION_TIMEOUT",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_policy::ProgressPolicy;
    use crate::execution_profile::{
        AutoStartPolicy, ExecutionProfileCategory, ExecutionProfileRoles, ExecutionRoleRoute,
        ReasoningEffort,
    };
    use crate::lead::{
        CheckKind, ObjectiveClass, ProviderRoute, QualityProfile, SpecBudget, SpecCheck,
        WorkUnitKind,
    };
    use chrono::Utc;
    use std::num::NonZeroU32;

    fn route() -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            reasoning: Some(ReasoningEffort::Medium),
        }
    }

    fn operations() -> OperationLimits {
        OperationLimits {
            model_request_timeout_secs: NonZeroU32::new(300).unwrap(),
            tool_call_timeout_secs: NonZeroU32::new(600).unwrap(),
            max_request_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_response_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_generation_tokens: NonZeroU32::new(8_192).unwrap(),
            max_in_flight_operations: NonZeroU32::new(4).unwrap(),
        }
    }

    fn progress() -> ProgressPolicy {
        ProgressPolicy {
            diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
            observation_timeout_secs: NonZeroU32::new(300).unwrap(),
        }
    }

    fn profile() -> ExecutionProfileContent {
        ExecutionProfileContent {
            name: "Balanced".into(),
            description: "test".into(),
            category: ExecutionProfileCategory::Balanced,
            roles: ExecutionProfileRoles {
                lead: route(),
                implementer: route(),
                semantic_reviewer: route(),
            },
            operations: operations(),
            progress: progress(),
            auto_start: AutoStartPolicy {
                enabled: false,
                risk_classes: Vec::new(),
            },
        }
    }

    fn check(id: &str, kind: CheckKind, expect_exit: i32) -> SpecCheck {
        SpecCheck {
            id: id.into(),
            kind,
            description: id.into(),
            command: vec!["cargo".into(), "test".into()],
            expect_exit,
            timeout_secs: 60,
        }
    }

    fn proposal() -> LeadPlanProposal {
        LeadPlanProposal {
            project_id: "project".into(),
            action_id: "action".into(),
            base_revision: "a".repeat(40),
            project_config_hash: "b".repeat(64),
            objective: "objective".into(),
            class: ObjectiveClass::Feature,
            assumptions: Vec::new(),
            risks: Vec::new(),
            acceptance_criteria: vec![check("test", CheckKind::Test, 0)],
            negative_cases: vec![check("negative", CheckKind::Negative, 1)],
            quality_profile: QualityProfile::Generic,
            risk_class: RiskClass::R0,
            budget: SpecBudget {
                max_attempts: 2,
                timeout_secs: 300,
            },
            route: ProviderRoute {
                provider: "opencode-go".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_repair_cycles: 0,
            max_wall_clock_secs: 900,
            max_parallel: 1,
            max_sessions: 3,
            work_units: vec![crate::lead::LeadWorkUnit {
                id: "unit-1".into(),
                kind: WorkUnitKind::Implementation,
                description: "unit".into(),
                depends_on: Vec::new(),
                allowed_paths: vec!["src".into()],
                capabilities: vec![crate::lead::Capability::RepositoryEdit],
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            }],
        }
    }

    fn grant() -> StandingDelegationV2 {
        let mut grant = StandingDelegationV2 {
            id: "grant".into(),
            principal_id: "operator-token".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![crate::lead::Capability::RepositoryEdit],
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: ProviderRoute {
                provider: "opencode-go".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_timeout_secs: 300,
            max_parallel: 1,
            operations: operations(),
            progress: progress(),
            expires_at: Utc::now() + chrono::Duration::days(30),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    #[test]
    fn scope_is_explicit_and_has_no_productivity_counters() {
        let scope =
            validate_execution_scope(&profile(), &proposal(), &grant(), &IntakeMode::ReviewPlan)
                .unwrap();
        assert_eq!(scope.risk_class, RiskClass::R0);
        assert_eq!(scope.allowed_paths, vec!["src".to_owned()]);
        assert_eq!(scope.max_parallel, 1);
        assert_eq!(scope.max_timeout_secs, 300);
        assert!(scope.operations.tool_call_timeout_secs.get() >= 1);

        let terms = execution_policy_terms(&profile());
        let value = serde_json::to_value(&terms).unwrap();
        for forbidden in [
            "maxAttemptsPerRequest",
            "maxSessionsPerRequest",
            "maxTurnsPerRequest",
            "maxInputTokensPerRequest",
            "maxOutputTokensPerRequest",
            "maxCostMicrosPerRequest",
            "maxWallClockSecsPerRequest",
            "maxRepairCycles",
            "maxRequests",
            "maxCumulativeSessions",
            "maxCumulativeAttempts",
        ] {
            assert!(value.get(forbidden).is_none(), "{forbidden} must be absent");
        }
    }

    #[test]
    fn non_r0_plan_is_not_operator_startable() {
        let mut proposal = proposal();
        proposal.risk_class = RiskClass::R1;
        assert_eq!(
            validate_execution_scope(&profile(), &proposal, &grant(), &IntakeMode::ReviewPlan),
            Err(IntakeScopeError::NotR0)
        );
    }

    #[test]
    fn route_mismatch_and_parallel_capacity_fail_closed() {
        let mut mismatched = profile();
        mismatched.roles.implementer.model = "other-model".into();
        assert_eq!(
            validate_execution_scope(&mismatched, &proposal(), &grant(), &IntakeMode::ReviewPlan),
            Err(IntakeScopeError::RouteMismatch)
        );

        let mut parallel = proposal();
        parallel.max_parallel = 2;
        assert_eq!(
            validate_execution_scope(&profile(), &parallel, &grant(), &IntakeMode::ReviewPlan),
            Err(IntakeScopeError::ParallelUnavailable)
        );
    }

    #[test]
    fn per_operation_timeout_is_bounded_but_no_campaign_deadline_exists() {
        let mut grant = grant();
        grant.max_timeout_secs = 60;
        grant.content_hash = grant.compute_content_hash();
        assert_eq!(
            validate_execution_scope(&profile(), &proposal(), &grant, &IntakeMode::ReviewPlan),
            Err(IntakeScopeError::OperationTimeout)
        );

        let mut long = proposal();
        long.max_wall_clock_secs = 8 * 24 * 60 * 60;
        assert!(
            validate_execution_scope(
                &profile(),
                &long,
                &grant_with_timeout(600),
                &IntakeMode::ReviewPlan
            )
            .is_ok(),
            "a Campaign wall-clock hint is not authority"
        );
    }

    fn grant_with_timeout(timeout: u32) -> StandingDelegationV2 {
        let mut grant = grant();
        grant.max_timeout_secs = timeout;
        grant.content_hash = grant.compute_content_hash();
        grant
    }
}
