//! Shared Execution Policy V2 and inference-ledger interface.
//!
//! This module is the frozen boundary between the execution-policy lane and the
//! shared-ledger lane. It contains typed identities, policy versions, progress
//! supervision values and monetary ledger DTOs; it deliberately contains no
//! database access, no provider call and no authorization decision.
//!
//! Decision mechanism: `DETERMINISTIC`. Money arithmetic, continuation
//! classification and input validation are mechanical. A model never creates,
//! widens or settles a monetary authorization.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Largest amount representable without overflowing SQLite's signed INTEGER.
pub const MAX_USD_MICROS: u64 = i64::MAX as u64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExecutionPolicyError {
    #[error("invalid execution policy value: {0}")]
    Invalid(String),
    #[error("monetary amount overflow")]
    Overflow,
}

/// Integer United States dollars, in millionths of a dollar.
///
/// The Factory never uses floating point for money. Construction is checked so
/// an amount always fits the durable `INTEGER` representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct UsdMicros(u64);

/// Deserialization must not bypass the SQLite `INTEGER` range. A derived
/// implementation would accept any `u64`, including values that cannot be
/// stored durably.
impl<'de> Deserialize<'de> for UsdMicros {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl UsdMicros {
    pub const ZERO: Self = Self(0);

    pub fn new(value: u64) -> Result<Self, ExecutionPolicyError> {
        if value > MAX_USD_MICROS {
            return Err(ExecutionPolicyError::Overflow);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Result<Self, ExecutionPolicyError> {
        self.0
            .checked_add(other.0)
            .ok_or(ExecutionPolicyError::Overflow)
            .and_then(Self::new)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, ExecutionPolicyError> {
        self.0
            .checked_sub(other.0)
            .ok_or(ExecutionPolicyError::Overflow)
            .and_then(Self::new)
    }
}

/// The policy version frozen into one authorization.
///
/// A legacy grant keeps its original evidence identity and is never decoded
/// using V2 defaults. New grants carry an explicit V2 policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LegacyExecutionPolicy {
    pub evidence_id: String,
    pub content_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum FrozenExecutionPolicy {
    #[serde(rename = "V1")]
    LegacyV1(LegacyExecutionPolicy),
    #[serde(rename = "V2")]
    V2(ExecutionPolicyV2),
    /// Subscription-funded execution has no monetary envelope. This is a new
    /// frozen contract; V1/V2 rows retain their original decoding semantics.
    #[serde(rename = "V3")]
    V3(Box<ExecutionPolicyV3>),
}

impl FrozenExecutionPolicy {
    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::LegacyV1(_))
    }

    pub fn v2(&self) -> Option<&ExecutionPolicyV2> {
        match self {
            Self::V2(policy) => Some(policy),
            Self::LegacyV1(_) | Self::V3(_) => None,
        }
    }

    pub fn v3(&self) -> Option<&ExecutionPolicyV3> {
        match self {
            Self::V3(policy) => Some(policy.as_ref()),
            Self::LegacyV1(_) | Self::V2(_) => None,
        }
    }
}

/// Funding classification frozen into an authorization. Subscription
/// admission is based on verified entitlement and no-charge route evidence,
/// never on a synthetic USD envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum FundingPolicy {
    Subscription {
        grant_id: String,
        binding_sha256: String,
        binding: Box<SubscriptionBinding>,
    },
    Billable {
        monetary_envelope_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum ProviderEntitlement {
    Verified {
        plan_id: String,
        provider_account_id: String,
    },
    Unknown {
        reason: String,
    },
    Revoked {
        observed_at_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum FundingRecoveryState {
    Disabled {
        recorded_at_ms: i64,
    },
    PausedQuota {
        observed_at_ms: i64,
        resets_at_ms: Option<i64>,
        reason: String,
    },
    Recoverable {
        observed_at_ms: i64,
        reason: String,
    },
}

/// Exact route types currently admitted by the subscription-only policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubscriptionRoute {
    OpenCodeGo,
    CodexChatGpt,
}

/// Provider-reported quota is an observation; unknown is not unlimited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum QuotaObservation {
    Available {
        observed_at_ms: i64,
        valid_until_ms: i64,
        remaining: Option<u64>,
    },
    Exhausted {
        observed_at_ms: i64,
        resets_at_ms: Option<i64>,
    },
    Unknown {
        observed_at_ms: i64,
        reason: String,
    },
}

/// Evidence that subscription routing cannot fall through to a paid balance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SubscriptionBinding {
    pub route: SubscriptionRoute,
    pub entitlement: ProviderEntitlement,
    pub provider_account_id: String,
    pub plan_id: String,
    pub model_id: String,
    pub settings_revision: String,
    pub entitlement_verified: bool,
    pub no_charge_route_verified: bool,
    pub extra_usage_disabled: bool,
    pub api_key_route_disabled: bool,
    pub quota: QuotaObservation,
}

impl SubscriptionBinding {
    pub fn is_verified_no_charge_route(&self) -> bool {
        let entitlement_matches = matches!(
            &self.entitlement,
            ProviderEntitlement::Verified { plan_id, provider_account_id }
                if plan_id == &self.plan_id && provider_account_id == &self.provider_account_id
        );
        let quota_available = matches!(
            self.quota,
            QuotaObservation::Available { observed_at_ms, valid_until_ms, .. }
                if observed_at_ms > 0 && valid_until_ms > observed_at_ms
        );
        entitlement_matches
            && self.entitlement_verified
            && self.no_charge_route_verified
            && self.extra_usage_disabled
            && self.api_key_route_disabled
            && quota_available
    }

    pub fn is_verified_no_charge_route_at(&self, now_ms: i64) -> bool {
        let fresh_quota = matches!(
            self.quota,
            QuotaObservation::Available { observed_at_ms, valid_until_ms, .. }
                if observed_at_ms > 0 && observed_at_ms <= now_ms && valid_until_ms > now_ms
        );
        now_ms > 0 && self.is_verified_no_charge_route() && fresh_quota
    }
}

/// V3 replaces V2's mandatory envelope with an explicit funding policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionPolicyV3 {
    pub policy_id: String,
    pub authorization_id: String,
    pub authorization_revision: i64,
    pub funding: FundingPolicy,
    pub scope_sha256: String,
    pub progress: ProgressPolicy,
    pub operations: OperationLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FundingAdmissionDecision {
    Allowed,
    Disabled,
    UnverifiedEntitlement,
    UnknownQuota,
    BillableRouteForbidden,
    ExtraUsageUnverified,
    QuotaExhausted,
    ProviderUnavailable,
    AuthorizationInvalid,
}

/// Fail-closed runtime refusal while trusted subscription admission is absent.
/// This type has no successful variant and cannot confer dispatch authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SubscriptionFundingRefusal {
    #[error(
        "request or Campaign has no verified subscription funding binding; dispatch is NOT_VERIFIED"
    )]
    BindingRequired,
    #[error(
        "subscription funding execution remains disabled pending trusted provider verification"
    )]
    VerificationRequired,
    #[error("subscription funding evidence is invalid; provider dispatch is disabled")]
    InvalidEvidence,
}

impl SubscriptionFundingRefusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::BindingRequired => "SUBSCRIPTION_FUNDING_BINDING_REQUIRED",
            Self::VerificationRequired => "SUBSCRIPTION_FUNDING_VERIFICATION_REQUIRED",
            Self::InvalidEvidence => "SUBSCRIPTION_FUNDING_EVIDENCE_INVALID",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAuthenticationClass {
    OpenCodeSubscription,
    ChatGptPlan,
    ApiKey,
    Unknown,
}

/// Trusted facts needed for one deterministic subscription admission check.
/// `funding_enabled` is read from authoritative campaign state; provider and
/// authentication values come from the provider adapter, never model output.
pub struct FundingAdmissionContext<'a> {
    pub policy: &'a ExecutionPolicyV3,
    pub authorization_id: &'a str,
    pub authorization_revision: i64,
    pub authorization_revoked: bool,
    pub funding_revoked: bool,
    pub grant_id: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub authentication: ProviderAuthenticationClass,
    pub scope_sha256: &'a str,
    pub settings_revision: &'a str,
    pub funding_enabled: bool,
    pub provider_available: bool,
    pub now_ms: i64,
}

/// Deterministic admission. This only evaluates evidence supplied by a
/// trusted provider adapter; user labels and model responses are not evidence.
pub fn evaluate_funding_admission(
    context: &FundingAdmissionContext<'_>,
) -> FundingAdmissionDecision {
    if !context.provider_available {
        return FundingAdmissionDecision::ProviderUnavailable;
    }
    if !context.funding_enabled {
        return FundingAdmissionDecision::Disabled;
    }
    let policy = context.policy;
    let (provider, model, authentication, scope_sha256, now_ms) = (
        context.provider,
        context.model,
        context.authentication,
        context.scope_sha256,
        context.now_ms,
    );
    if policy.policy_id.trim().is_empty()
        || policy.authorization_id.trim().is_empty()
        || policy.authorization_id != context.authorization_id
        || policy.authorization_revision <= 0
        || policy.authorization_revision != context.authorization_revision
        || context.authorization_revoked
        || context.funding_revoked
        || context.grant_id.trim().is_empty()
        || policy.scope_sha256 != scope_sha256
        || scope_sha256.len() != 64
        || !scope_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || now_ms <= 0
    {
        return FundingAdmissionDecision::AuthorizationInvalid;
    }
    let FundingPolicy::Subscription {
        grant_id, binding, ..
    } = &policy.funding
    else {
        return FundingAdmissionDecision::BillableRouteForbidden;
    };
    if grant_id != context.grant_id {
        return FundingAdmissionDecision::AuthorizationInvalid;
    }
    let expected = match binding.route {
        SubscriptionRoute::OpenCodeGo => (
            "opencode",
            ProviderAuthenticationClass::OpenCodeSubscription,
        ),
        SubscriptionRoute::CodexChatGpt => ("codex", ProviderAuthenticationClass::ChatGptPlan),
    };
    if provider != expected.0
        || model != binding.model_id
        || authentication != expected.1
        || context.settings_revision != binding.settings_revision
    {
        return FundingAdmissionDecision::BillableRouteForbidden;
    }
    if !binding.extra_usage_disabled
        || !binding.api_key_route_disabled
        || !binding.no_charge_route_verified
    {
        return FundingAdmissionDecision::ExtraUsageUnverified;
    }
    if binding.provider_account_id.trim().is_empty()
        || binding.plan_id.trim().is_empty()
        || binding.model_id.trim().is_empty()
        || binding.settings_revision.trim().is_empty()
    {
        return FundingAdmissionDecision::AuthorizationInvalid;
    }
    match binding.quota {
        QuotaObservation::Exhausted { .. } => return FundingAdmissionDecision::QuotaExhausted,
        QuotaObservation::Unknown { .. } => return FundingAdmissionDecision::UnknownQuota,
        QuotaObservation::Available {
            observed_at_ms,
            valid_until_ms,
            ..
        } if observed_at_ms <= 0 || observed_at_ms > now_ms || valid_until_ms <= now_ms => {
            return FundingAdmissionDecision::UnknownQuota;
        }
        QuotaObservation::Available { .. } => {}
    }
    if !binding.is_verified_no_charge_route_at(now_ms) {
        return FundingAdmissionDecision::UnverifiedEntitlement;
    }
    FundingAdmissionDecision::Allowed
}

/// Technical operation bounds. These are per-operation and capacity limits,
/// never cumulative productivity budgets for a Campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OperationLimits {
    pub model_request_timeout_secs: std::num::NonZeroU32,
    pub tool_call_timeout_secs: std::num::NonZeroU32,
    pub max_request_bytes: std::num::NonZeroU32,
    pub max_response_bytes: std::num::NonZeroU32,
    pub max_generation_tokens: std::num::NonZeroU32,
    pub max_in_flight_operations: std::num::NonZeroU32,
}

/// Deterministic progress supervision. Thresholds trigger diagnosis or
/// escalation; they do not terminate a legitimate Campaign on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressPolicy {
    pub diagnose_after_stagnant_checkpoints: std::num::NonZeroU32,
    pub escalate_after_unsuccessful_replans: std::num::NonZeroU32,
    pub observation_timeout_secs: std::num::NonZeroU32,
}

/// A typed, versioned execution policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionPolicyV2 {
    pub policy_id: String,
    pub authorization_id: String,
    pub monetary_envelope_id: String,
    pub scope_sha256: String,
    pub progress: ProgressPolicy,
    pub operations: OperationLimits,
}

/// Observable engineering progress. Every variant must be backed by a durable
/// artifact reference; prose, heartbeat, elapsed time and token spend do not
/// count as progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProgressKind {
    CandidateChanged,
    TestsImproved,
    FindingsResolved,
    WorkGraphAdvanced,
    UnitAccepted,
    RepairValidated,
    BlockerIdentified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressEvidence {
    pub evidence_id: String,
    pub campaign_id: String,
    pub work_item_id: String,
    /// Attempt that produced this observation. Required for V2 progress.
    #[serde(default)]
    pub attempt_id: String,
    pub kind: ProgressKind,
    pub artifact_ref: String,
    pub artifact_sha256: String,
    pub baseline_ref: String,
    pub observed_sequence: u64,
    /// The immutable Core execution receipt for `TestsImproved`.
    #[serde(default)]
    pub execution_receipt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProgressCheckpoint {
    pub checkpoint_id: String,
    pub work_item_id: String,
    pub operation_id: String,
    pub evidence_ids: Vec<String>,
    pub outcome_sha256: String,
}

/// What Core may do after a terminal operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContinuationDecision {
    Continue,
    Diagnose,
    Replan,
    Escalate,
    AwaitAuthorization,
    AwaitReconciliation,
    Cancel,
    Complete,
}

/// Every billable inference role is attributed to the same Campaign envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InferenceRole {
    Lead,
    Implementer,
    SemanticReviewer,
    Evaluation,
    Auxiliary,
}

impl InferenceRole {
    pub const ALL: [Self; 5] = [
        Self::Lead,
        Self::Implementer,
        Self::SemanticReviewer,
        Self::Evaluation,
        Self::Auxiliary,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "LEAD",
            Self::Implementer => "IMPLEMENTER",
            Self::SemanticReviewer => "SEMANTIC_REVIEWER",
            Self::Evaluation => "EVALUATION",
            Self::Auxiliary => "AUXILIARY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvelopeState {
    Active,
    Revoked,
    Closed,
    Overdrawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReservationState {
    Reserved,
    Settled,
    Released,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReserveInferenceInput {
    pub envelope_id: String,
    pub operation_id: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub role: InferenceRole,
    pub principal_id: String,
    pub parent_operation_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub pricing_revision: String,
    pub pricing_snapshot_json: String,
    pub upper_bound_micros: UsdMicros,
    pub expires_at_ms: i64,
    /// Authorization revision the caller believes is current. Core re-reads
    /// the durable revision inside the ledger transaction and rejects a stale
    /// expectation; this field is never trusted on its own.
    pub authorization_revision: i64,
    /// The exact scope digest bound to the frozen policy.
    pub scope_sha256: String,
    /// Fencing identity for Worker-dispatched calls. Assessment and review
    /// calls legitimately have no Worker lease.
    pub worker_id: Option<String>,
    pub generation: Option<i64>,
}

impl ReserveInferenceInput {
    /// Mechanical validation only. Authorization, fencing and balance checks
    /// belong to the Core ledger transaction.
    pub fn validate(&self) -> Result<(), ExecutionPolicyError> {
        for (name, value) in [
            ("envelope_id", &self.envelope_id),
            ("operation_id", &self.operation_id),
            ("idempotency_key", &self.idempotency_key),
            ("principal_id", &self.principal_id),
            ("provider", &self.provider),
            ("model", &self.model),
            ("pricing_revision", &self.pricing_revision),
        ] {
            if value.trim().is_empty() {
                return Err(ExecutionPolicyError::Invalid(format!("{name} is empty")));
            }
        }
        if !is_sha256(&self.request_sha256) {
            return Err(ExecutionPolicyError::Invalid(
                "request_sha256 is not a lowercase SHA-256".into(),
            ));
        }
        if self.pricing_snapshot_json.trim().is_empty() {
            return Err(ExecutionPolicyError::Invalid(
                "pricing_snapshot_json is empty".into(),
            ));
        }
        if self.expires_at_ms <= 0 {
            return Err(ExecutionPolicyError::Invalid(
                "expires_at_ms must be positive".into(),
            ));
        }
        if self.authorization_revision <= 0 {
            return Err(ExecutionPolicyError::Invalid(
                "authorization_revision must be positive".into(),
            ));
        }
        if !is_sha256(&self.scope_sha256) {
            return Err(ExecutionPolicyError::Invalid(
                "scope_sha256 is not a lowercase SHA-256".into(),
            ));
        }
        match (&self.worker_id, self.generation) {
            (None, None) => {}
            (Some(worker), Some(generation)) if !worker.trim().is_empty() && generation > 0 => {}
            _ => {
                return Err(ExecutionPolicyError::Invalid(
                    "worker_id and generation must be both absent or both valid".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InferenceReservation {
    pub reservation_id: String,
    pub envelope_id: String,
    pub operation_id: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub role: InferenceRole,
    pub principal_id: String,
    pub parent_operation_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub pricing_revision: String,
    pub pricing_snapshot_json: String,
    pub authorization_revision: i64,
    pub worker_id: Option<String>,
    pub generation: Option<i64>,
    pub state: ReservationState,
    pub upper_bound_micros: UsdMicros,
    pub settled_micros: Option<UsdMicros>,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InferenceSettlement {
    pub reservation_id: String,
    pub settled_micros: UsdMicros,
    /// Settlement identity is `(reservation_id, settled_micros,
    /// usage_evidence_sha256)`: an identical duplicate replays its receipt, a
    /// different amount or evidence for the same reservation conflicts.
    pub usage_evidence_sha256: String,
}

impl InferenceSettlement {
    pub fn validate(&self) -> Result<(), ExecutionPolicyError> {
        if self.reservation_id.trim().is_empty() {
            return Err(ExecutionPolicyError::Invalid(
                "reservation_id is empty".into(),
            ));
        }
        if !is_sha256(&self.usage_evidence_sha256) {
            return Err(ExecutionPolicyError::Invalid(
                "usage_evidence_sha256 is not a lowercase SHA-256".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpendingStatus {
    pub envelope_id: String,
    pub currency: String,
    pub state: EnvelopeState,
    pub authorized_micros: UsdMicros,
    pub settled_micros: UsdMicros,
    pub reserved_micros: UsdMicros,
    pub ambiguous_micros: UsdMicros,
    /// Floors at zero; an overrun is reported in `overrun_micros` instead of
    /// producing a negative available balance.
    pub available_micros: UsdMicros,
    pub overrun_micros: UsdMicros,
    pub authorization_revision: i64,
    pub billing_basis: String,
    pub pricing_provenance: String,
    pub unresolved_count: u64,
    pub by_role: Vec<SpendingBreakdown>,
    pub by_provider: Vec<SpendingBreakdown>,
    pub as_of: String,
}

/// Attributed spending for one role or provider. Breakdowns are telemetry and
/// never create per-role authorization ceilings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SpendingBreakdown {
    pub key: String,
    pub settled_micros: UsdMicros,
    pub reserved_micros: UsdMicros,
    pub ambiguous_micros: UsdMicros,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InferenceLedgerError {
    #[error("inference envelope not found")]
    NotFound,
    #[error("inference envelope is not active")]
    NotActive,
    #[error("authorized spending is exhausted")]
    Exhausted,
    #[error("inference operation conflicts with its recorded identity")]
    Conflict,
    #[error("inference outcome is ambiguous; reconciliation is required")]
    Ambiguous,
    #[error("invalid inference ledger request: {0}")]
    Invalid(String),
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn policy() -> ExecutionPolicyV2 {
        ExecutionPolicyV2 {
            policy_id: "policy".into(),
            authorization_id: "authorization".into(),
            monetary_envelope_id: "envelope".into(),
            scope_sha256: "a".repeat(64),
            progress: ProgressPolicy {
                diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
                escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
                observation_timeout_secs: NonZeroU32::new(300).unwrap(),
            },
            operations: OperationLimits {
                model_request_timeout_secs: NonZeroU32::new(300).unwrap(),
                tool_call_timeout_secs: NonZeroU32::new(600).unwrap(),
                max_request_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
                max_response_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
                max_generation_tokens: NonZeroU32::new(8_192).unwrap(),
                max_in_flight_operations: NonZeroU32::new(4).unwrap(),
            },
        }
    }

    #[test]
    fn money_arithmetic_is_checked() {
        let one = UsdMicros::new(1).unwrap();
        assert_eq!(one.checked_add(one).unwrap(), UsdMicros::new(2).unwrap());
        assert_eq!(
            UsdMicros::new(MAX_USD_MICROS)
                .unwrap()
                .checked_add(one)
                .unwrap_err(),
            ExecutionPolicyError::Overflow
        );
        assert_eq!(
            UsdMicros::new(MAX_USD_MICROS + 1).unwrap_err(),
            ExecutionPolicyError::Overflow
        );
        assert_eq!(
            one.checked_sub(UsdMicros::new(2).unwrap()).unwrap_err(),
            ExecutionPolicyError::Overflow
        );
    }

    #[test]
    fn v2_policy_has_no_productivity_counters() {
        let value = serde_json::to_value(FrozenExecutionPolicy::V2(policy())).unwrap();
        for forbidden in [
            "maxTurns",
            "maxAttempts",
            "maxSessions",
            "maxRepairCycles",
            "maxWallClockSecs",
            "maxInputTokens",
            "maxOutputTokens",
            "maxCostMicros",
        ] {
            assert!(value.get(forbidden).is_none(), "{forbidden} must be absent");
        }
        assert!(value["version"] == "V2");
        let decoded: FrozenExecutionPolicy = serde_json::from_value(value).unwrap();
        assert!(!decoded.is_legacy());
    }

    #[test]
    fn legacy_policy_keeps_its_evidence_identity() {
        let legacy = FrozenExecutionPolicy::LegacyV1(LegacyExecutionPolicy {
            evidence_id: "evidence".into(),
            content_sha256: "b".repeat(64),
        });
        let value = serde_json::to_value(&legacy).unwrap();
        assert_eq!(value["version"], "V1");
        assert_eq!(value["evidenceId"], "evidence");
        assert_eq!(
            serde_json::from_value::<FrozenExecutionPolicy>(value).unwrap(),
            legacy
        );
    }

    #[test]
    fn reserve_input_validation_rejects_bad_digest_and_empty_identity() {
        let input = ReserveInferenceInput {
            envelope_id: "envelope".into(),
            operation_id: "operation".into(),
            idempotency_key: "idempotency".into(),
            request_sha256: "c".repeat(64),
            role: InferenceRole::Lead,
            principal_id: "owner".into(),
            parent_operation_id: None,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            pricing_revision: "models.dev@1".into(),
            pricing_snapshot_json: "{}".into(),
            upper_bound_micros: UsdMicros::new(1_000).unwrap(),
            expires_at_ms: 1_800_000_000_000,
            authorization_revision: 1,
            scope_sha256: "d".repeat(64),
            worker_id: Some("worker-1".into()),
            generation: Some(3),
        };
        assert!(input.validate().is_ok());
        let mut bad = input.clone();
        bad.request_sha256 = "nope".into();
        assert!(bad.validate().is_err());
        let mut empty = input;
        empty.envelope_id = " ".into();
        assert!(empty.validate().is_err());
    }

    #[test]
    fn money_deserialization_cannot_bypass_the_sqlite_range() {
        let max: UsdMicros = serde_json::from_str(&MAX_USD_MICROS.to_string()).unwrap();
        assert_eq!(max.get(), MAX_USD_MICROS);
        for invalid in [
            "9223372036854775808",
            "18446744073709551615",
            "-1",
            "1.5",
            "\"1\"",
        ] {
            assert!(
                serde_json::from_str::<UsdMicros>(invalid).is_err(),
                "{invalid} must be rejected"
            );
        }
    }

    #[test]
    fn reserve_context_requires_stale_revision_and_fencing_pairing() {
        let mut input = ReserveInferenceInput {
            envelope_id: "envelope".into(),
            operation_id: "operation".into(),
            idempotency_key: "idempotency".into(),
            request_sha256: "c".repeat(64),
            role: InferenceRole::Implementer,
            principal_id: "owner".into(),
            parent_operation_id: None,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            pricing_revision: "models.dev@1".into(),
            pricing_snapshot_json: "{}".into(),
            upper_bound_micros: UsdMicros::new(1_000).unwrap(),
            expires_at_ms: 1_800_000_000_000,
            authorization_revision: 2,
            scope_sha256: "d".repeat(64),
            worker_id: Some("worker-1".into()),
            generation: Some(3),
        };
        assert!(input.validate().is_ok());
        input.authorization_revision = 0;
        assert!(input.validate().is_err());
        let mut unfenced = input.clone();
        unfenced.authorization_revision = 2;
        unfenced.worker_id = Some("worker-1".into());
        unfenced.generation = None;
        assert!(unfenced.validate().is_err());
        let mut bad_scope = input;
        bad_scope.worker_id = None;
        bad_scope.generation = None;
        bad_scope.scope_sha256 = "not-a-digest".into();
        assert!(bad_scope.validate().is_err());
    }

    #[test]
    fn all_inference_roles_are_attributed() {
        assert_eq!(InferenceRole::ALL.len(), 5);
        assert_eq!(InferenceRole::Auxiliary.as_str(), "AUXILIARY");
    }
}

#[cfg(test)]
mod subscription_funding_tests {
    use super::*;

    fn admission_policy() -> ExecutionPolicyV3 {
        let binding = SubscriptionBinding {
            route: SubscriptionRoute::OpenCodeGo,
            entitlement: ProviderEntitlement::Verified {
                plan_id: "go".into(),
                provider_account_id: "account".into(),
            },
            provider_account_id: "account".into(),
            plan_id: "go".into(),
            model_id: "model".into(),
            settings_revision: "revision-1".into(),
            entitlement_verified: true,
            no_charge_route_verified: true,
            extra_usage_disabled: true,
            api_key_route_disabled: true,
            quota: QuotaObservation::Available {
                observed_at_ms: 10,
                valid_until_ms: 20,
                remaining: Some(100),
            },
        };
        ExecutionPolicyV3 {
            policy_id: "policy-1".into(),
            authorization_id: "auth-1".into(),
            authorization_revision: 1,
            funding: FundingPolicy::Subscription {
                grant_id: "grant-1".into(),
                binding_sha256: "a".repeat(64),
                binding: Box::new(binding),
            },
            scope_sha256: "b".repeat(64),
            progress: ProgressPolicy {
                diagnose_after_stagnant_checkpoints: std::num::NonZeroU32::new(2).unwrap(),
                escalate_after_unsuccessful_replans: std::num::NonZeroU32::new(2).unwrap(),
                observation_timeout_secs: std::num::NonZeroU32::new(30).unwrap(),
            },
            operations: OperationLimits {
                model_request_timeout_secs: std::num::NonZeroU32::new(60).unwrap(),
                tool_call_timeout_secs: std::num::NonZeroU32::new(60).unwrap(),
                max_request_bytes: std::num::NonZeroU32::new(1024).unwrap(),
                max_response_bytes: std::num::NonZeroU32::new(1024).unwrap(),
                max_generation_tokens: std::num::NonZeroU32::new(1024).unwrap(),
                max_in_flight_operations: std::num::NonZeroU32::new(1).unwrap(),
            },
        }
    }

    fn admission(policy: &ExecutionPolicyV3) -> FundingAdmissionDecision {
        admission_for(
            policy,
            "opencode",
            "model",
            ProviderAuthenticationClass::OpenCodeSubscription,
            &"b".repeat(64),
            true,
        )
    }

    fn admission_for(
        policy: &ExecutionPolicyV3,
        provider: &str,
        model: &str,
        authentication: ProviderAuthenticationClass,
        scope_sha256: &str,
        funding_enabled: bool,
    ) -> FundingAdmissionDecision {
        evaluate_funding_admission(&FundingAdmissionContext {
            policy,
            authorization_id: "auth-1",
            authorization_revision: 1,
            authorization_revoked: false,
            funding_revoked: false,
            grant_id: "grant-1",
            provider,
            model,
            authentication,
            scope_sha256,
            settings_revision: "revision-1",
            funding_enabled,
            provider_available: true,
            now_ms: 15,
        })
    }

    #[test]
    fn subscription_funding_policy_round_trips_without_usd_envelope() {
        let policy = FundingPolicy::Subscription {
            grant_id: "grant-1".into(),
            binding_sha256: "a".repeat(64),
            binding: Box::new(SubscriptionBinding {
                route: SubscriptionRoute::OpenCodeGo,
                entitlement: ProviderEntitlement::Verified {
                    plan_id: "go".into(),
                    provider_account_id: "account".into(),
                },
                provider_account_id: "account".into(),
                plan_id: "go".into(),
                model_id: "model".into(),
                settings_revision: "r1".into(),
                entitlement_verified: true,
                no_charge_route_verified: true,
                extra_usage_disabled: true,
                api_key_route_disabled: true,
                quota: QuotaObservation::Available {
                    observed_at_ms: 1,
                    valid_until_ms: 2,
                    remaining: None,
                },
            }),
        };
        let encoded = serde_json::to_string(&policy).unwrap();
        let decoded: FundingPolicy = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, policy);
        assert!(!encoded.contains("usd"));
        assert!(!encoded.contains("envelope"));
    }

    #[test]
    fn subscription_quota_unknown_is_not_treated_as_available() {
        let quota = QuotaObservation::Unknown {
            observed_at_ms: 1,
            reason: "provider did not expose quota".into(),
        };
        let encoded = serde_json::to_string(&quota).unwrap();
        assert!(encoded.contains("UNKNOWN"));
        assert!(!matches!(quota, QuotaObservation::Available { .. }));
    }

    #[test]
    fn no_charge_admission_requires_matching_verified_entitlement_and_fresh_quota() {
        let binding = SubscriptionBinding {
            route: SubscriptionRoute::OpenCodeGo,
            entitlement: ProviderEntitlement::Verified {
                plan_id: "go".into(),
                provider_account_id: "account".into(),
            },
            provider_account_id: "account".into(),
            plan_id: "go".into(),
            model_id: "model".into(),
            settings_revision: "revision-1".into(),
            entitlement_verified: true,
            no_charge_route_verified: true,
            extra_usage_disabled: true,
            api_key_route_disabled: true,
            quota: QuotaObservation::Available {
                observed_at_ms: 10,
                valid_until_ms: 20,
                remaining: None,
            },
        };
        assert!(binding.is_verified_no_charge_route());
        let mut mismatched = binding;
        mismatched.plan_id = "other-plan".into();
        assert!(!mismatched.is_verified_no_charge_route());
    }

    #[test]
    fn funding_admission_requires_enabled_verified_subscription_and_fresh_quota() {
        let mut policy = admission_policy();
        assert_eq!(admission(&policy), FundingAdmissionDecision::Allowed);

        assert_eq!(
            admission_for(
                &policy,
                "opencode",
                "model",
                ProviderAuthenticationClass::OpenCodeSubscription,
                &"b".repeat(64),
                false,
            ),
            FundingAdmissionDecision::Disabled
        );
        if let FundingPolicy::Subscription { binding, .. } = &mut policy.funding {
            binding.extra_usage_disabled = false;
        } else {
            panic!("expected subscription binding");
        }
        assert_eq!(
            admission(&policy),
            FundingAdmissionDecision::ExtraUsageUnverified
        );
        if let FundingPolicy::Subscription { binding, .. } = &mut policy.funding {
            binding.extra_usage_disabled = true;
            binding.quota = QuotaObservation::Unknown {
                observed_at_ms: 10,
                reason: "provider quota is not observable".into(),
            };
        }
        assert_eq!(admission(&policy), FundingAdmissionDecision::UnknownQuota);
        if let FundingPolicy::Subscription { binding, .. } = &mut policy.funding {
            binding.quota = QuotaObservation::Available {
                observed_at_ms: 10,
                valid_until_ms: 14,
                remaining: Some(10),
            };
        }
        assert_eq!(admission(&policy), FundingAdmissionDecision::UnknownQuota);
    }

    #[test]
    fn funding_admission_refuses_changed_route_or_billable_authentication() {
        let policy = admission_policy();
        assert_eq!(
            admission_for(
                &policy,
                "opencode",
                "different-model",
                ProviderAuthenticationClass::OpenCodeSubscription,
                &"b".repeat(64),
                true,
            ),
            FundingAdmissionDecision::BillableRouteForbidden
        );
        assert_eq!(
            admission_for(
                &policy,
                "opencode",
                "model",
                ProviderAuthenticationClass::ApiKey,
                &"b".repeat(64),
                true,
            ),
            FundingAdmissionDecision::BillableRouteForbidden
        );
        assert_eq!(
            admission_for(
                &policy,
                "opencode",
                "model",
                ProviderAuthenticationClass::OpenCodeSubscription,
                &"c".repeat(64),
                true,
            ),
            FundingAdmissionDecision::AuthorizationInvalid
        );
    }

    #[test]
    fn funding_admission_fences_revocation_and_authorization_or_settings_revisions() {
        let policy = admission_policy();
        let scope_sha256 = "b".repeat(64);
        let mut context = FundingAdmissionContext {
            policy: &policy,
            authorization_id: "auth-1",
            authorization_revision: 1,
            authorization_revoked: false,
            funding_revoked: false,
            grant_id: "grant-1",
            provider: "opencode",
            model: "model",
            authentication: ProviderAuthenticationClass::OpenCodeSubscription,
            scope_sha256: &scope_sha256,
            settings_revision: "revision-1",
            funding_enabled: true,
            provider_available: true,
            now_ms: 15,
        };
        context.authorization_revision = 2;
        assert_eq!(
            evaluate_funding_admission(&context),
            FundingAdmissionDecision::AuthorizationInvalid
        );
        context.authorization_revision = 1;
        context.authorization_revoked = true;
        assert_eq!(
            evaluate_funding_admission(&context),
            FundingAdmissionDecision::AuthorizationInvalid
        );
        context.authorization_revoked = false;
        context.funding_revoked = true;
        assert_eq!(
            evaluate_funding_admission(&context),
            FundingAdmissionDecision::AuthorizationInvalid
        );
        context.funding_revoked = false;
        context.settings_revision = "revision-2";
        assert_eq!(
            evaluate_funding_admission(&context),
            FundingAdmissionDecision::BillableRouteForbidden
        );
    }

    #[test]
    fn subscription_binding_rejects_unrecognized_fields() {
        let value = serde_json::json!({
            "providerAccountId": "acct",
            "planId": "go",
            "modelId": "model",
            "settingsRevision": "r1",
            "entitlementVerified": true,
            "noChargeRouteVerified": true,
            "extraUsageDisabled": true,
            "apiKeyRouteDisabled": true,
            "quota": { "state": "UNKNOWN", "observed_at_ms": 1, "reason": "missing" },
            "paidFallback": true
        });
        assert!(serde_json::from_value::<SubscriptionBinding>(value).is_err());
    }
}
