//! Durable workflow state machine, checkpoints, idempotency and outbox.
//!
//! WP1 owns this module. Core remains the authority; this module only records
//! transitions and external-effect intent/result.
//!
//! The state graph is intentionally small and deterministic. A model, Worker or
//! projection cannot choose a transition: Core calls the transition function
//! with the exact source state it observed, and the database fences the write by
//! the stored revision. Terminal states are immutable.
//!
//! External projections (GitHub, OpenSpec, notifications) are written through
//! the outbox. Recording an intent is not an external effect; consumers must
//! reconcile the durable/external state before retrying. Idempotency keys make
//! the replay of a completed operation return the prior result instead of
//! repeating the effect.

use serde::{Deserialize, Serialize};

use crate::planning::{WorkflowState, WorkflowStepStatus};

/// Maximum characters accepted for an opaque identifier such as a run, step or
/// request id.
pub const MAX_IDENTIFIER_CHARS: usize = 200;
/// Maximum characters accepted for a short operation or step kind.
pub const MAX_KIND_CHARS: usize = 120;
/// Maximum characters accepted for an operator/audit detail string.
pub const MAX_DETAIL_CHARS: usize = 4_000;
/// Maximum bytes accepted for a JSON payload stored by the workflow layer.
pub const MAX_JSON_BYTES: usize = 1024 * 1024;
/// Idempotency records are retained for this many days. Expiry is a cleanup
/// boundary; a replay inside the window always returns the recorded result.
pub const IDEMPOTENCY_RETENTION_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowError {
    #[error("invalid workflow input: {0}")]
    Invalid(String),
    #[error("illegal workflow transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: WorkflowState,
        to: WorkflowState,
    },
}

fn invalid(message: impl Into<String>) -> WorkflowError {
    WorkflowError::Invalid(message.into())
}

/// Return the durable SQL representation of a workflow state.
pub fn workflow_state_code(state: WorkflowState) -> &'static str {
    match state {
        WorkflowState::Submitted => "SUBMITTED",
        WorkflowState::Assessing => "ASSESSING",
        WorkflowState::Planned => "PLANNED",
        WorkflowState::PlanReview => "PLAN_REVIEW",
        WorkflowState::Frozen => "FROZEN",
        WorkflowState::Executing => "EXECUTING",
        WorkflowState::Verifying => "VERIFYING",
        WorkflowState::Reviewing => "REVIEWING",
        WorkflowState::Gate => "GATE",
        WorkflowState::Repairing => "REPAIRING",
        WorkflowState::Replanning => "REPLANNING",
        WorkflowState::Converging => "CONVERGING",
        WorkflowState::Accepted => "ACCEPTED",
        WorkflowState::Blocked => "BLOCKED",
        WorkflowState::Failed => "FAILED",
        WorkflowState::Cancelled => "CANCELLED",
    }
}

/// Parse a durable SQL workflow state. Unknown values fail closed.
pub fn parse_workflow_state(value: &str) -> Result<WorkflowState, WorkflowError> {
    match value {
        "SUBMITTED" => Ok(WorkflowState::Submitted),
        "ASSESSING" => Ok(WorkflowState::Assessing),
        "PLANNED" => Ok(WorkflowState::Planned),
        "PLAN_REVIEW" => Ok(WorkflowState::PlanReview),
        "FROZEN" => Ok(WorkflowState::Frozen),
        "EXECUTING" => Ok(WorkflowState::Executing),
        "VERIFYING" => Ok(WorkflowState::Verifying),
        "REVIEWING" => Ok(WorkflowState::Reviewing),
        "GATE" => Ok(WorkflowState::Gate),
        "REPAIRING" => Ok(WorkflowState::Repairing),
        "REPLANNING" => Ok(WorkflowState::Replanning),
        "CONVERGING" => Ok(WorkflowState::Converging),
        "ACCEPTED" => Ok(WorkflowState::Accepted),
        "BLOCKED" => Ok(WorkflowState::Blocked),
        "FAILED" => Ok(WorkflowState::Failed),
        "CANCELLED" => Ok(WorkflowState::Cancelled),
        other => Err(invalid(format!("unknown workflow state {other:?}"))),
    }
}

/// Return the durable SQL representation of a step status.
pub fn workflow_step_status_code(status: WorkflowStepStatus) -> &'static str {
    match status {
        WorkflowStepStatus::Pending => "PENDING",
        WorkflowStepStatus::Running => "RUNNING",
        WorkflowStepStatus::Succeeded => "SUCCEEDED",
        WorkflowStepStatus::Failed => "FAILED",
        WorkflowStepStatus::Blocked => "BLOCKED",
    }
}

/// Parse a durable SQL step status. Unknown values fail closed.
pub fn parse_workflow_step_status(value: &str) -> Result<WorkflowStepStatus, WorkflowError> {
    match value {
        "PENDING" => Ok(WorkflowStepStatus::Pending),
        "RUNNING" => Ok(WorkflowStepStatus::Running),
        "SUCCEEDED" => Ok(WorkflowStepStatus::Succeeded),
        "FAILED" => Ok(WorkflowStepStatus::Failed),
        "BLOCKED" => Ok(WorkflowStepStatus::Blocked),
        other => Err(invalid(format!("unknown workflow step status {other:?}"))),
    }
}

/// Deterministic transition guard.
///
/// The forward path is:
/// `SUBMITTED -> ASSESSING -> PLANNED -> PLAN_REVIEW -> FROZEN -> EXECUTING ->
/// VERIFYING -> REVIEWING -> GATE -> CONVERGING -> ACCEPTED`.
/// `GATE` may route to `REPAIRING` or `REPLANNING`; both return to
/// `EXECUTING`. A plan revision may return from `PLAN_REVIEW`/`PLANNED` to
/// `REPLANNING`. Every non-terminal state may fail closed to `BLOCKED`,
/// `FAILED` or `CANCELLED`. Terminal states accept no transition at all.
pub fn validate_transition(from: WorkflowState, to: WorkflowState) -> Result<(), WorkflowError> {
    if from.is_terminal() {
        return Err(WorkflowError::IllegalTransition { from, to });
    }
    if from == to {
        return Err(WorkflowError::IllegalTransition { from, to });
    }
    let allowed = match from {
        WorkflowState::Submitted => matches!(
            to,
            WorkflowState::Assessing
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Assessing => matches!(
            to,
            WorkflowState::Planned
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Planned => matches!(
            to,
            WorkflowState::PlanReview
                | WorkflowState::Replanning
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::PlanReview => matches!(
            to,
            WorkflowState::Frozen
                | WorkflowState::Replanning
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Frozen => matches!(
            to,
            WorkflowState::Executing
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Executing => matches!(
            to,
            WorkflowState::Verifying
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Verifying => matches!(
            to,
            WorkflowState::Reviewing
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Reviewing => matches!(
            to,
            WorkflowState::Gate
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Gate => matches!(
            to,
            WorkflowState::Converging
                | WorkflowState::Repairing
                | WorkflowState::Replanning
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Repairing => matches!(
            to,
            WorkflowState::Executing
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Replanning => matches!(
            to,
            WorkflowState::Planned
                | WorkflowState::PlanReview
                | WorkflowState::Executing
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Converging => matches!(
            to,
            WorkflowState::Accepted
                | WorkflowState::Blocked
                | WorkflowState::Failed
                | WorkflowState::Cancelled
        ),
        WorkflowState::Accepted
        | WorkflowState::Blocked
        | WorkflowState::Failed
        | WorkflowState::Cancelled => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(WorkflowError::IllegalTransition { from, to })
    }
}

/// Validate a non-empty, bounded opaque identifier.
pub fn validate_identifier(field: &str, value: &str) -> Result<(), WorkflowError> {
    validate_text(field, value, MAX_IDENTIFIER_CHARS, false)
}

/// Return whether a value is a canonical lowercase UUID string (36 chars,
/// 8-4-4-4-12). Core request IDs use this shape.
pub fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                matches!(byte, b'0'..=b'9' | b'a'..=b'f')
            }
        })
}

/// Core workflow runs use `workflow-<uuid>`.
pub fn is_workflow_run_id(value: &str) -> bool {
    value.strip_prefix("workflow-").is_some_and(is_uuid)
}

/// Core workflow steps use `step-<uuid>`.
pub fn is_workflow_step_id(value: &str) -> bool {
    value.strip_prefix("step-").is_some_and(is_uuid)
}

/// Validate a canonical UUID request identifier.
pub fn validate_uuid(field: &str, value: &str) -> Result<(), WorkflowError> {
    if !is_uuid(value) {
        return Err(invalid(format!(
            "{field} must be a canonical lowercase UUID"
        )));
    }
    Ok(())
}

/// Validate a generated workflow run identifier.
pub fn validate_workflow_run_id(field: &str, value: &str) -> Result<(), WorkflowError> {
    if !is_workflow_run_id(value) {
        return Err(invalid(format!(
            "{field} must be a workflow-<uuid> identifier"
        )));
    }
    Ok(())
}

/// Validate a generated workflow step identifier.
pub fn validate_workflow_step_id(field: &str, value: &str) -> Result<(), WorkflowError> {
    if !is_workflow_step_id(value) {
        return Err(invalid(format!("{field} must be a step-<uuid> identifier")));
    }
    Ok(())
}

/// Validate an optional identifier. `None` is accepted; `Some` must be valid.
pub fn validate_optional_identifier(field: &str, value: Option<&str>) -> Result<(), WorkflowError> {
    match value {
        Some(value) => validate_identifier(field, value),
        None => Ok(()),
    }
}

/// Validate a bounded text field. Control characters are rejected so durable
/// records cannot smuggle terminal/log control sequences.
pub fn validate_text(
    field: &str,
    value: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<(), WorkflowError> {
    if !allow_empty && value.trim().is_empty() {
        return Err(invalid(format!("{field} is required")));
    }
    if value.chars().count() > max_chars {
        return Err(invalid(format!(
            "{field} exceeds the {max_chars} character bound"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(format!("{field} contains a control character")));
    }
    Ok(())
}

/// Validate a lowercase/uppercase hexadecimal SHA-256 digest.
pub fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Validate a SHA-256 field. An empty value is allowed only when the caller is
/// recording an optional hash that is not known yet.
pub fn validate_sha256(field: &str, value: &str, allow_empty: bool) -> Result<(), WorkflowError> {
    if value.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(invalid(format!("{field} is required")))
        };
    }
    if !is_sha256(value) {
        return Err(invalid(format!("{field} must be a 64 character SHA-256")));
    }
    Ok(())
}

/// Validate a bounded JSON payload. Validation happens before the payload is
/// persisted so a malformed or oversized value never reaches the database.
pub fn validate_json_payload(field: &str, value: &str) -> Result<(), WorkflowError> {
    if value.len() > MAX_JSON_BYTES {
        return Err(invalid(format!(
            "{field} exceeds the {MAX_JSON_BYTES} byte bound"
        )));
    }
    if value.trim().is_empty() {
        return Err(invalid(format!("{field} is required")));
    }
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|error| invalid(format!("{field} is not valid JSON: {error}")))
}

/// Compute the lowercase hexadecimal SHA-256 of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// A durable workflow run projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunRecord {
    pub id: String,
    pub request_id: String,
    pub campaign_id: Option<String>,
    pub state: WorkflowState,
    pub revision: u32,
    pub plan_sha256: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A durable workflow step. `attempt` is per run and step kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStepRecord {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub status: WorkflowStepStatus,
    pub attempt: u32,
    pub input_sha256: String,
    pub output_sha256: String,
    pub detail: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A durable checkpoint bound to a run and work unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCheckpointRecord {
    pub id: String,
    pub run_id: String,
    pub unit_id: String,
    pub state_json: String,
    pub state_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OutboxStatus {
    Pending,
    Sent,
    Failed,
    Reconciled,
}

pub fn outbox_status_code(status: OutboxStatus) -> &'static str {
    match status {
        OutboxStatus::Pending => "PENDING",
        OutboxStatus::Sent => "SENT",
        OutboxStatus::Failed => "FAILED",
        OutboxStatus::Reconciled => "RECONCILED",
    }
}

pub fn parse_outbox_status(value: &str) -> Result<OutboxStatus, WorkflowError> {
    match value {
        "PENDING" => Ok(OutboxStatus::Pending),
        "SENT" => Ok(OutboxStatus::Sent),
        "FAILED" => Ok(OutboxStatus::Failed),
        "RECONCILED" => Ok(OutboxStatus::Reconciled),
        other => Err(invalid(format!("unknown outbox status {other:?}"))),
    }
}

/// A durable outbox intent and its delivery state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxRecord {
    pub id: String,
    pub kind: String,
    pub payload_json: String,
    pub payload_sha256: String,
    pub status: OutboxStatus,
    pub attempts: u32,
    pub last_error: String,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_path_is_accepted() {
        let path = [
            (WorkflowState::Submitted, WorkflowState::Assessing),
            (WorkflowState::Assessing, WorkflowState::Planned),
            (WorkflowState::Planned, WorkflowState::PlanReview),
            (WorkflowState::PlanReview, WorkflowState::Frozen),
            (WorkflowState::Frozen, WorkflowState::Executing),
            (WorkflowState::Executing, WorkflowState::Verifying),
            (WorkflowState::Verifying, WorkflowState::Reviewing),
            (WorkflowState::Reviewing, WorkflowState::Gate),
            (WorkflowState::Gate, WorkflowState::Converging),
            (WorkflowState::Converging, WorkflowState::Accepted),
        ];
        for (from, to) in path {
            assert!(
                validate_transition(from, to).is_ok(),
                "{from:?} -> {to:?} should be legal"
            );
        }
    }

    #[test]
    fn repair_and_replan_paths_return_to_execution() {
        for (from, to) in [
            (WorkflowState::Gate, WorkflowState::Repairing),
            (WorkflowState::Repairing, WorkflowState::Executing),
            (WorkflowState::Gate, WorkflowState::Replanning),
            (WorkflowState::Replanning, WorkflowState::Executing),
            (WorkflowState::Replanning, WorkflowState::Planned),
            (WorkflowState::PlanReview, WorkflowState::Replanning),
        ] {
            assert!(validate_transition(from, to).is_ok());
        }
    }

    #[test]
    fn state_skips_and_terminal_mutations_are_rejected() {
        for (from, to) in [
            (WorkflowState::Submitted, WorkflowState::Frozen),
            (WorkflowState::Submitted, WorkflowState::Executing),
            (WorkflowState::Assessing, WorkflowState::Executing),
            (WorkflowState::Executing, WorkflowState::Accepted),
            (WorkflowState::Gate, WorkflowState::Executing),
            (WorkflowState::Converging, WorkflowState::Executing),
        ] {
            assert!(
                validate_transition(from, to).is_err(),
                "{from:?} -> {to:?} must be rejected"
            );
        }
        for terminal in [
            WorkflowState::Accepted,
            WorkflowState::Blocked,
            WorkflowState::Failed,
            WorkflowState::Cancelled,
        ] {
            for to in [
                WorkflowState::Submitted,
                WorkflowState::Executing,
                WorkflowState::Accepted,
            ] {
                assert!(validate_transition(terminal, to).is_err());
            }
        }
    }

    #[test]
    fn every_non_terminal_state_can_fail_closed() {
        for from in [
            WorkflowState::Submitted,
            WorkflowState::Assessing,
            WorkflowState::Planned,
            WorkflowState::PlanReview,
            WorkflowState::Frozen,
            WorkflowState::Executing,
            WorkflowState::Verifying,
            WorkflowState::Reviewing,
            WorkflowState::Gate,
            WorkflowState::Repairing,
            WorkflowState::Replanning,
            WorkflowState::Converging,
        ] {
            for to in [
                WorkflowState::Blocked,
                WorkflowState::Failed,
                WorkflowState::Cancelled,
            ] {
                assert!(validate_transition(from, to).is_ok());
            }
        }
    }

    #[test]
    fn identifiers_hashes_and_json_are_bounded() {
        assert!(validate_identifier("id", "").is_err());
        assert!(validate_identifier("id", "run-1").is_ok());
        assert!(validate_identifier("id", &"x".repeat(MAX_IDENTIFIER_CHARS + 1)).is_err());
        assert!(validate_identifier("id", "run\u{0}").is_err());

        assert!(validate_sha256("hash", "", false).is_err());
        assert!(validate_sha256("hash", "", true).is_ok());
        assert!(validate_sha256("hash", &"a".repeat(64), false).is_ok());
        assert!(validate_sha256("hash", &"a".repeat(63), false).is_err());
        assert!(validate_sha256("hash", &"z".repeat(64), false).is_err());

        assert!(validate_json_payload("json", "{}").is_ok());
        assert!(validate_json_payload("json", "").is_err());
        assert!(validate_json_payload("json", "{").is_err());
        assert!(
            validate_json_payload("json", &format!("\"{}\"", "x".repeat(MAX_JSON_BYTES))).is_err()
        );
    }

    #[test]
    fn hash_is_deterministic_sha256() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
