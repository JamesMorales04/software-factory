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
}

impl FrozenExecutionPolicy {
    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::LegacyV1(_))
    }

    pub fn v2(&self) -> Option<&ExecutionPolicyV2> {
        match self {
            Self::V2(policy) => Some(policy),
            Self::LegacyV1(_) => None,
        }
    }
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
