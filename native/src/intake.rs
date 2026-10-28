//! Unified intake domain.
//!
//! An intake request is an operator intent bound to one Project, one execution
//! profile revision and one durable Lead request. It does not own a lifecycle:
//! the derived state, withdrawal fact, frozen plan and Campaign/gate outcome
//! remain authoritative in the Lead and Campaign records.
//!
//! Decision mechanisms in this module are `DETERMINISTIC`: state projection,
//! authorization reason codes and budget intersection are mechanical facts,
//! never model judgments.

use serde::{Deserialize, Serialize};

use crate::execution_profile::{CampaignBudgets, ExecutionProfileContent};
use crate::lead::{LeadPlanProposal, ObjectiveClass, RiskClass, StandingDelegation};

/// Conservative Core ceilings used when deriving an operator Campaign
/// authorization. These mirror the existing authorization boundary; they are
/// not new policy and may only narrow the frozen profile/plan/delegation.
const CORE_MAX_ATTEMPTS: u32 = 5;
const CORE_MAX_IMPLEMENTATION_SESSIONS: u32 = 2;
const CORE_MAX_TURNS: u32 = 4;
const CORE_MAX_DEADLINE_SECS: u64 = 600;
const CORE_MIN_DEADLINE_SECS: u64 = 30;

/// Bounded cumulative execution-policy grant terms. These are Core policy
/// constants, exposed in the `EXECUTION_POLICY_REQUIRED` scope so Office can
/// show the exact grant before acknowledgement instead of duplicating policy.
pub const EXECUTION_POLICY_REQUESTS: u32 = 4;
pub const EXECUTION_POLICY_VALIDITY_DAYS: i64 = 30;
pub const EXECUTION_POLICY_MIN_DEADLINE_SECS: u64 = 120;
pub const EXECUTION_POLICY_MAX_REPAIR_CYCLES: u32 = 0;
pub const EXECUTION_POLICY_MAX_PARALLEL: u32 = 1;
pub const EXECUTION_POLICY_ATTEMPTS_CAP: u32 = 5;

/// Material bounds of the non-autonomous grant that an operator is
/// acknowledging. This is a projection of Core policy, not a second authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeExecutionPolicyTerms {
    pub objective_classes: Vec<ObjectiveClass>,
    pub risk_classes: Vec<RiskClass>,
    pub autonomous_start: bool,
    pub validity_days: i64,
    pub max_requests: u32,
    pub max_attempts_per_request: u32,
    pub max_sessions_per_request: u32,
    pub max_turns_per_request: u32,
    pub max_input_tokens_per_request: u64,
    pub max_output_tokens_per_request: u64,
    pub max_cost_micros_per_request: u64,
    pub max_timeout_secs_per_request: u64,
    pub max_wall_clock_secs_per_request: u64,
    pub max_repair_cycles: u32,
    pub max_parallel: u32,
    pub max_cumulative_sessions: u32,
    pub max_cumulative_attempts: u32,
}

/// Derive the exact bounded grant terms from one profile revision. The
/// execution-policy writer and the `EXECUTION_POLICY_REQUIRED` projection both
/// use this function so the acknowledgement cannot disagree with the grant.
pub fn execution_policy_terms(profile: &ExecutionProfileContent) -> IntakeExecutionPolicyTerms {
    let implementer = &profile.roles.implementer;
    let max_attempts = profile
        .budgets
        .max_attempts
        .min(EXECUTION_POLICY_ATTEMPTS_CAP);
    let max_sessions = profile.budgets.max_sessions;
    IntakeExecutionPolicyTerms {
        objective_classes: vec![
            ObjectiveClass::Feature,
            ObjectiveClass::Maintenance,
            ObjectiveClass::Refactor,
        ],
        risk_classes: vec![RiskClass::R0],
        autonomous_start: false,
        validity_days: EXECUTION_POLICY_VALIDITY_DAYS,
        max_requests: EXECUTION_POLICY_REQUESTS,
        max_attempts_per_request: max_attempts,
        max_sessions_per_request: max_sessions,
        max_turns_per_request: profile.budgets.max_turns,
        max_input_tokens_per_request: profile
            .budgets
            .max_input_tokens
            .min(implementer.max_input_tokens),
        max_output_tokens_per_request: profile
            .budgets
            .max_output_tokens
            .min(implementer.max_output_tokens),
        max_cost_micros_per_request: profile
            .budgets
            .max_cost_micros
            .min(implementer.max_cost_micros),
        max_timeout_secs_per_request: profile.budgets.deadline_secs.clamp(30, 600),
        max_wall_clock_secs_per_request: profile
            .budgets
            .deadline_secs
            .clamp(EXECUTION_POLICY_MIN_DEADLINE_SECS, 7_200),
        max_repair_cycles: EXECUTION_POLICY_MAX_REPAIR_CYCLES,
        max_parallel: EXECUTION_POLICY_MAX_PARALLEL,
        max_cumulative_sessions: max_sessions
            .saturating_mul(EXECUTION_POLICY_REQUESTS)
            .min(crate::lead::MAX_CUMULATIVE_SESSIONS),
        max_cumulative_attempts: max_attempts
            .saturating_mul(EXECUTION_POLICY_REQUESTS)
            .min(crate::lead::MAX_CUMULATIVE_ATTEMPTS),
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

/// Budget intersect for one operator-approved Campaign. Every value is the
/// minimum of the frozen profile campaign budget, the frozen implementer role
/// route, frozen plan, active delegation and the existing Core authorization
/// ceilings. Plan and delegation do not carry token/cost/turn ceilings today,
/// so those dimensions use the stricter of the profile campaign budget and the
/// implementer route; this is a documented limitation, not additional
/// authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeCampaignBudget {
    pub max_attempts: u32,
    pub max_sessions: u32,
    pub max_turns: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
    pub deadline_secs: u64,
}

/// Derive the conservative implementation envelope for an explicit operator
/// approval. This never broadens the frozen plan; insufficient capacity is a
/// typed refusal and the caller must not silently increase a limit.
pub fn intersect_campaign_budget(
    profile: &ExecutionProfileContent,
    proposal: &LeadPlanProposal,
    grant: &StandingDelegation,
    behavior: &IntakeMode,
) -> Result<IntakeCampaignBudget, IntakeBudgetError> {
    let profile_budgets: &CampaignBudgets = &profile.budgets;
    let implementer = &profile.roles.implementer;
    // The frozen plan's session ceiling includes the assessment session. The
    // Campaign authorization may only use the remaining implementation part.
    let plan_sessions = proposal.max_sessions.saturating_sub(1);
    let max_sessions = profile_budgets
        .max_sessions
        .min(plan_sessions)
        .min(grant.max_sessions)
        .min(CORE_MAX_IMPLEMENTATION_SESSIONS);
    if max_sessions == 0 {
        return Err(IntakeBudgetError::NoImplementationSession);
    }
    let max_attempts = profile_budgets
        .max_attempts
        .min(proposal.budget.max_attempts)
        .min(grant.max_attempts)
        .min(CORE_MAX_ATTEMPTS);
    if max_attempts == 0 {
        return Err(IntakeBudgetError::NoAttempt);
    }
    let max_turns = profile_budgets.max_turns.min(CORE_MAX_TURNS);
    let deadline_secs = profile_budgets
        .deadline_secs
        .min(u64::from(proposal.max_wall_clock_secs))
        .min(u64::from(grant.max_wall_clock_secs))
        .min(CORE_MAX_DEADLINE_SECS);
    if deadline_secs < CORE_MIN_DEADLINE_SECS {
        return Err(IntakeBudgetError::DeadlineTooShort);
    }
    // Autonomous submission mode is not a start authority. It is accepted as
    // an operator intent label only; this derivation is always the explicit
    // operator path.
    let _ = behavior;
    if proposal.risk_class != RiskClass::R0 {
        return Err(IntakeBudgetError::NotR0);
    }
    Ok(IntakeCampaignBudget {
        max_attempts,
        max_sessions,
        max_turns,
        max_input_tokens: profile_budgets
            .max_input_tokens
            .min(implementer.max_input_tokens),
        max_output_tokens: profile_budgets
            .max_output_tokens
            .min(implementer.max_output_tokens),
        max_cost_micros: profile_budgets
            .max_cost_micros
            .min(implementer.max_cost_micros),
        deadline_secs,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeBudgetError {
    NoImplementationSession,
    NoAttempt,
    DeadlineTooShort,
    NotR0,
}

impl IntakeBudgetError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::NoImplementationSession => "AUTHORIZATION_UNAVAILABLE_NO_SESSION",
            Self::NoAttempt => "AUTHORIZATION_UNAVAILABLE_NO_ATTEMPT",
            Self::DeadlineTooShort => "AUTHORIZATION_UNAVAILABLE_DEADLINE",
            Self::NotR0 => "AUTHORIZATION_UNAVAILABLE_RISK",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_profile::{
        AutoStartPolicy, ExecutionProfileCategory, ExecutionProfileRoles, ExecutionRoleRoute,
        ReasoningEffort,
    };
    use crate::lead::{
        CheckKind, ObjectiveClass, ProviderRoute, QualityProfile, SpecBudget, SpecCheck,
        WorkUnitKind,
    };
    use chrono::Utc;

    fn route() -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            reasoning: Some(ReasoningEffort::Medium),
            max_input_tokens: 64_000,
            max_output_tokens: 8_192,
            max_cost_micros: 300_000,
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
            budgets: CampaignBudgets {
                max_attempts: 3,
                max_sessions: 4,
                max_turns: 4,
                max_input_tokens: 40_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                deadline_secs: 1_800,
            },
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

    fn grant() -> StandingDelegation {
        let mut grant = StandingDelegation {
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
            max_attempts: 3,
            max_timeout_secs: 300,
            max_repair_cycles: 0,
            max_wall_clock_secs: 600,
            max_parallel: 1,
            max_sessions: 3,
            max_requests: 4,
            max_cumulative_sessions: 12,
            max_cumulative_attempts: 12,
            expires_at: Utc::now() + chrono::Duration::days(30),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    #[test]
    fn budget_is_the_minimum_of_profile_plan_grant_and_core() {
        let budget =
            intersect_campaign_budget(&profile(), &proposal(), &grant(), &IntakeMode::ReviewPlan)
                .unwrap();
        assert_eq!(budget.max_attempts, 2);
        assert_eq!(budget.max_sessions, 2);
        assert_eq!(budget.max_turns, 4);
        assert_eq!(budget.max_input_tokens, 40_000);
        assert_eq!(budget.max_output_tokens, 8_192);
        assert_eq!(budget.max_cost_micros, 300_000);
        assert_eq!(budget.deadline_secs, 600);
    }

    #[test]
    fn implementer_route_caps_bind_tokens_and_cost_independently() {
        let mut input_capped = profile();
        input_capped.roles.implementer.max_input_tokens = 10_000;
        let budget = intersect_campaign_budget(
            &input_capped,
            &proposal(),
            &grant(),
            &IntakeMode::ReviewPlan,
        )
        .unwrap();
        assert_eq!(budget.max_input_tokens, 10_000);
        assert_eq!(budget.max_output_tokens, 8_192);
        assert_eq!(budget.max_cost_micros, 300_000);

        let mut output_capped = profile();
        output_capped.roles.implementer.max_output_tokens = 1_000;
        let budget = intersect_campaign_budget(
            &output_capped,
            &proposal(),
            &grant(),
            &IntakeMode::ReviewPlan,
        )
        .unwrap();
        assert_eq!(budget.max_input_tokens, 40_000);
        assert_eq!(budget.max_output_tokens, 1_000);
        assert_eq!(budget.max_cost_micros, 300_000);

        let mut cost_capped = profile();
        cost_capped.roles.implementer.max_cost_micros = 100_000;
        let budget =
            intersect_campaign_budget(&cost_capped, &proposal(), &grant(), &IntakeMode::ReviewPlan)
                .unwrap();
        assert_eq!(budget.max_input_tokens, 40_000);
        assert_eq!(budget.max_output_tokens, 8_192);
        assert_eq!(budget.max_cost_micros, 100_000);
    }

    #[test]
    fn non_r0_plan_is_not_operator_startable() {
        let mut proposal = proposal();
        proposal.risk_class = RiskClass::R1;
        assert_eq!(
            intersect_campaign_budget(&profile(), &proposal, &grant(), &IntakeMode::ReviewPlan),
            Err(IntakeBudgetError::NotR0)
        );
    }

    #[test]
    fn zero_implementation_sessions_fail_closed() {
        let mut proposal = proposal();
        proposal.max_sessions = 1;
        assert_eq!(
            intersect_campaign_budget(&profile(), &proposal, &grant(), &IntakeMode::ReviewPlan),
            Err(IntakeBudgetError::NoImplementationSession)
        );
    }
}
