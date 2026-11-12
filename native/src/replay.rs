//! Workflow event replay and observability projections.
//!
//! This module is a pure, deterministic projection over durable workflow
//! events. It never reads a database, calls a provider or mutates authority.
//! Sequence order is the replay order; `recorded_at` is retained for display
//! and never used to order events.
//!
//! The projection fails closed on an ambiguous ledger: conflicting duplicate
//! sequences, incomplete transitions, disallowed transitions and non-canonical
//! payload hashes are rejected. Byte-identical duplicate sequences are
//! collapsed and reported because an at-least-once event projection may replay
//! the same event. Gaps in the state chain are reported as
//! [`MissingTransition`] so an operator can see a lost event without inventing
//! state.
//!
//! Step lifecycle events use an explicit, small vocabulary so the projection
//! does not guess from arbitrary event kinds:
//!
//! - `step.pending` (or `workflow.step.pending`) opens a pending step;
//! - `step.running` / `step.started` (with a `workflow.` prefix allowed) marks
//!   it running;
//! - `step.succeeded`, `step.completed`, `step.finished`, `step.failed`,
//!   `step.blocked` or `step.cancelled` closes it.
//!
//! Checkpoint events use `checkpoint`, `workflow.checkpoint` or
//! `workflow.checkpoint.recorded`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    planning::{WorkflowState, WorkflowStepStatus},
    workflow::{MAX_KIND_CHARS, sha256_hex},
};

const MAX_RECORDED_AT_CHARS: usize = 64;
const MAX_REDACTION_DEPTH: usize = 8;
const MAX_SAFE_STRING_CHARS: usize = 256;
const MAX_REPLAY_EVENTS: usize = 100_000;
const MAX_PAYLOAD_COLLECTION_ITEMS: usize = 1_024;
const MAX_PAYLOAD_OBJECT_KEYS: usize = 256;
const REDACTED_LABEL: &str = "[REDACTED]";
const HASH_LABEL_PREFIX: &str = "sha256:";

/// Key fragments that always identify secret-shaped or prompt-shaped data.
/// Keys are normalized by removing separators and lowercasing before matching.
const SECRET_KEY_MARKERS: &[&str] = &[
    "token",
    "password",
    "secret",
    "authorization",
    "apikey",
    "prompt",
    "content",
];

/// Small allowlist of operational payload keys that may retain their values.
/// Everything else is redacted. Sensitive markers win over this allowlist.
const SAFE_PAYLOAD_KEYS: &[&str] = &[
    "kind",
    "state",
    "statefrom",
    "stateto",
    "status",
    "outcome",
    "sequence",
    "stepid",
    "runid",
    "requestid",
    "attempt",
    "count",
    "durationms",
    "bytes",
    "sha256",
    "payloadsha256",
    "terminal",
];

/// One durable workflow event in replay order.
///
/// `state_from` and `state_to` are both present for a state transition and
/// both absent for a non-transition event. A half-present transition is
/// rejected by [`build_trace`]. `step_id` is absent for run-level events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReplayEvent {
    pub sequence: u64,
    pub request_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub kind: String,
    pub state_from: Option<WorkflowState>,
    pub state_to: Option<WorkflowState>,
    pub payload_sha256: String,
    pub recorded_at: String,
}

/// A checkpoint projected from the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReplayCheckpoint {
    pub sequence: u64,
    pub step_id: Option<String>,
    pub state: Option<WorkflowState>,
    pub payload_sha256: String,
    pub recorded_at: String,
}

/// A step whose latest lifecycle event is pending or running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenStep {
    pub step_id: String,
    pub status: WorkflowStepStatus,
    pub last_sequence: u64,
}

/// A sequence id observed more than once with byte-identical event content.
/// The duplicates are rejected from the ordered trace and reported here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DuplicateSequence {
    pub sequence: u64,
    pub occurrences: usize,
}

/// A state transition whose `state_from` did not match the previously
/// projected state. The event itself is still valid; the gap is evidence that
/// an earlier event is missing from this trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MissingTransition {
    pub sequence: u64,
    pub expected_from: WorkflowState,
    pub observed_from: WorkflowState,
}

/// The deterministic replay projection of one workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReplayTrace {
    pub events: Vec<ReplayEvent>,
    pub final_state: Option<WorkflowState>,
    pub terminal: bool,
    pub checkpoints: Vec<ReplayCheckpoint>,
    pub open_steps: Vec<OpenStep>,
    pub duplicate_sequences: Vec<DuplicateSequence>,
    pub missing_transitions: Vec<MissingTransition>,
}

/// A typed difference between an expected and an actual replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayDifference {
    Run {
        expected: String,
        actual: String,
    },
    FinalState {
        expected: Option<WorkflowState>,
        actual: Option<WorkflowState>,
    },
    Terminal {
        expected: bool,
        actual: bool,
    },
    MissingSequence {
        sequence: u64,
    },
    UnexpectedSequence {
        sequence: u64,
    },
    StateTransition {
        sequence: u64,
        expected_from: Option<WorkflowState>,
        expected_to: Option<WorkflowState>,
        actual_from: Option<WorkflowState>,
        actual_to: Option<WorkflowState>,
    },
    PayloadHash {
        sequence: u64,
        expected: String,
        actual: String,
    },
}

/// Returns whether a recorded workflow transition is allowed.
///
/// This is a pure adapter over the canonical workflow state-machine guard in
/// [`crate::workflow::validate_transition`]. Replay does not maintain a second
/// transition policy; terminal states, forward edges and repair/replan loops
/// are owned by the durable workflow module.
pub fn transition_allowed(from: WorkflowState, to: WorkflowState) -> bool {
    crate::workflow::validate_transition(from, to).is_ok()
}

/// Builds a deterministic replay projection for one workflow run.
///
/// Events are sorted by `sequence` and validated per run. Identical duplicate
/// sequences are collapsed into [`ReplayTrace::duplicate_sequences`];
/// conflicting duplicates are rejected because the ledger is ambiguous.
/// Callers that require globally unique sequence ids can treat any non-empty
/// `duplicate_sequences` as a failure. Payload hashes must be lowercase
/// SHA-256 hex.
pub fn build_trace(mut events: Vec<ReplayEvent>) -> Result<ReplayTrace, String> {
    if events.len() > MAX_REPLAY_EVENTS {
        return Err("replay event count exceeds the bound".into());
    }
    for event in &events {
        validate_event(event)?;
    }
    ensure_single_identity(&events)?;

    events.sort_by_key(|event| event.sequence);

    let mut duplicate_sequences = Vec::new();
    let events = deduplicate_sequences(&events, &mut duplicate_sequences)?;
    ensure_strictly_increasing(&events)?;

    let mut current_state = None;
    let mut checkpoints = Vec::new();
    let mut open_steps = BTreeMap::new();
    let mut missing_transitions = Vec::new();

    for event in &events {
        apply_transition(event, &mut current_state, &mut missing_transitions)?;
        if is_checkpoint_kind(&event.kind) {
            checkpoints.push(ReplayCheckpoint {
                sequence: event.sequence,
                step_id: event.step_id.clone(),
                state: current_state,
                payload_sha256: event.payload_sha256.clone(),
                recorded_at: event.recorded_at.clone(),
            });
        }
        apply_step_lifecycle(event, &mut open_steps);
    }

    let final_state = current_state;
    let terminal = final_state.is_some_and(WorkflowState::is_terminal);

    Ok(ReplayTrace {
        events,
        final_state,
        terminal,
        checkpoints,
        open_steps: open_steps.into_values().collect(),
        duplicate_sequences,
        missing_transitions,
    })
}

/// Redacts a structured event payload using a small operational allowlist.
///
/// Secret-shaped keys (`token`, `password`, `secret`, `authorization`,
/// `api_key`, `prompt`, `content`, including separator/case variants) are
/// replaced with a SHA-256 fingerprint. Unknown keys are replaced with
/// `[REDACTED]`. Only allowlisted operational keys retain bounded values.
/// The function is pure and never inspects process credentials or external
/// state.
pub fn redact_payload(payload: &Value) -> Value {
    redact_value(payload, 0)
}

/// Compares an expected replay with an actual replay and returns typed
/// differences for run identity, final state, sequence coverage, state
/// transitions and payload hashes. An empty vector means the replays agree on
/// those properties.
pub fn compare_replay(expected: &ReplayTrace, actual: &ReplayTrace) -> Vec<ReplayDifference> {
    let mut differences = Vec::new();

    let expected_run = expected
        .events
        .first()
        .map(|event| event.run_id.as_str())
        .unwrap_or_default();
    let actual_run = actual
        .events
        .first()
        .map(|event| event.run_id.as_str())
        .unwrap_or_default();
    if expected_run != actual_run {
        differences.push(ReplayDifference::Run {
            expected: expected_run.to_owned(),
            actual: actual_run.to_owned(),
        });
    }

    if expected.final_state != actual.final_state {
        differences.push(ReplayDifference::FinalState {
            expected: expected.final_state,
            actual: actual.final_state,
        });
    }
    if expected.terminal != actual.terminal {
        differences.push(ReplayDifference::Terminal {
            expected: expected.terminal,
            actual: actual.terminal,
        });
    }

    let expected_events = expected
        .events
        .iter()
        .map(|event| (event.sequence, event))
        .collect::<BTreeMap<_, _>>();
    let actual_events = actual
        .events
        .iter()
        .map(|event| (event.sequence, event))
        .collect::<BTreeMap<_, _>>();
    let sequences = expected_events
        .keys()
        .chain(actual_events.keys())
        .copied()
        .collect::<BTreeSet<_>>();

    for sequence in sequences {
        match (expected_events.get(&sequence), actual_events.get(&sequence)) {
            (Some(expected_event), Some(actual_event)) => {
                let expected_transition = (expected_event.state_from, expected_event.state_to);
                let actual_transition = (actual_event.state_from, actual_event.state_to);
                if expected_transition != actual_transition {
                    differences.push(ReplayDifference::StateTransition {
                        sequence,
                        expected_from: expected_event.state_from,
                        expected_to: expected_event.state_to,
                        actual_from: actual_event.state_from,
                        actual_to: actual_event.state_to,
                    });
                }
                if expected_event.payload_sha256 != actual_event.payload_sha256 {
                    differences.push(ReplayDifference::PayloadHash {
                        sequence,
                        expected: expected_event.payload_sha256.clone(),
                        actual: actual_event.payload_sha256.clone(),
                    });
                }
            }
            (Some(_), None) => differences.push(ReplayDifference::MissingSequence { sequence }),
            (None, Some(_)) => differences.push(ReplayDifference::UnexpectedSequence { sequence }),
            (None, None) => {}
        }
    }

    differences
}

fn validate_event(event: &ReplayEvent) -> Result<(), String> {
    validate_uuid_text(&event.request_id, "requestId", event.sequence)?;
    validate_run_id_text(&event.run_id, "runId", event.sequence)?;
    if let Some(step_id) = &event.step_id {
        validate_step_id_text(step_id, "stepId", event.sequence)?;
    }
    validate_text(&event.kind, "kind", MAX_KIND_CHARS, event.sequence)?;
    validate_text(
        &event.recorded_at,
        "recordedAt",
        MAX_RECORDED_AT_CHARS,
        event.sequence,
    )?;
    if !is_lowercase_sha256(&event.payload_sha256) {
        return Err(format!(
            "replay event {} has a non-canonical payload SHA-256",
            event.sequence
        ));
    }
    Ok(())
}

fn validate_text(value: &str, field: &str, max_chars: usize, sequence: u64) -> Result<(), String> {
    crate::workflow::validate_text(field, value, max_chars, false)
        .map_err(|error| format!("replay event {sequence} has an invalid {field}: {error}"))
}

fn validate_uuid_text(value: &str, field: &str, sequence: u64) -> Result<(), String> {
    crate::workflow::validate_uuid(field, value)
        .map_err(|error| format!("replay event {sequence} has an invalid {field}: {error}"))
}

fn validate_run_id_text(value: &str, field: &str, sequence: u64) -> Result<(), String> {
    crate::workflow::validate_workflow_run_id(field, value)
        .map_err(|error| format!("replay event {sequence} has an invalid {field}: {error}"))
}

fn validate_step_id_text(value: &str, field: &str, sequence: u64) -> Result<(), String> {
    crate::workflow::validate_workflow_step_id(field, value)
        .map_err(|error| format!("replay event {sequence} has an invalid {field}: {error}"))
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn ensure_single_identity(events: &[ReplayEvent]) -> Result<(), String> {
    let runs = events
        .iter()
        .map(|event| event.run_id.as_str())
        .collect::<BTreeSet<_>>();
    if runs.len() > 1 {
        return Err(format!(
            "replay trace spans multiple runs: {}",
            runs.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let requests = events
        .iter()
        .map(|event| event.request_id.as_str())
        .collect::<BTreeSet<_>>();
    if requests.len() > 1 {
        return Err(format!(
            "replay trace spans multiple requests: {}",
            requests.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

fn deduplicate_sequences(
    events: &[ReplayEvent],
    duplicate_sequences: &mut Vec<DuplicateSequence>,
) -> Result<Vec<ReplayEvent>, String> {
    let mut deduplicated = Vec::with_capacity(events.len());

    for group in events.chunk_by(|left, right| left.sequence == right.sequence) {
        let Some(first) = group.first() else {
            continue;
        };
        if group.len() > 1 {
            if group.iter().any(|event| event != first) {
                return Err(format!(
                    "conflicting duplicate sequence {}: the ledger is ambiguous",
                    first.sequence
                ));
            }
            duplicate_sequences.push(DuplicateSequence {
                sequence: first.sequence,
                occurrences: group.len(),
            });
        }
        deduplicated.push(first.clone());
    }

    Ok(deduplicated)
}

fn ensure_strictly_increasing(events: &[ReplayEvent]) -> Result<(), String> {
    for pair in events.windows(2) {
        if pair[1].sequence <= pair[0].sequence {
            return Err(format!(
                "replay sequence {} is not strictly increasing after {}",
                pair[1].sequence, pair[0].sequence
            ));
        }
    }
    Ok(())
}

fn apply_transition(
    event: &ReplayEvent,
    current_state: &mut Option<WorkflowState>,
    missing_transitions: &mut Vec<MissingTransition>,
) -> Result<(), String> {
    match (event.state_from, event.state_to) {
        (None, None) => Ok(()),
        (Some(from), Some(to)) => {
            if let Err(error) = crate::workflow::validate_transition(from, to) {
                return Err(format!(
                    "invalid workflow transition at sequence {}: {error}",
                    event.sequence
                ));
            }
            let expected_from = current_state.unwrap_or(WorkflowState::Submitted);
            if from != expected_from {
                missing_transitions.push(MissingTransition {
                    sequence: event.sequence,
                    expected_from,
                    observed_from: from,
                });
            }
            *current_state = Some(to);
            Ok(())
        }
        _ => Err(format!(
            "incomplete workflow transition at sequence {}: stateFrom and stateTo must both be present or both absent",
            event.sequence
        )),
    }
}

fn apply_step_lifecycle(event: &ReplayEvent, open_steps: &mut BTreeMap<String, OpenStep>) {
    let Some(step_id) = event.step_id.as_deref() else {
        return;
    };
    let Some(lifecycle) = step_lifecycle(&event.kind) else {
        return;
    };

    match lifecycle {
        StepLifecycle::Pending => {
            open_steps.insert(
                step_id.to_owned(),
                OpenStep {
                    step_id: step_id.to_owned(),
                    status: WorkflowStepStatus::Pending,
                    last_sequence: event.sequence,
                },
            );
        }
        StepLifecycle::Running => {
            open_steps.insert(
                step_id.to_owned(),
                OpenStep {
                    step_id: step_id.to_owned(),
                    status: WorkflowStepStatus::Running,
                    last_sequence: event.sequence,
                },
            );
        }
        StepLifecycle::Closed => {
            open_steps.remove(step_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepLifecycle {
    Pending,
    Running,
    Closed,
}

fn step_lifecycle(kind: &str) -> Option<StepLifecycle> {
    let kind = kind.trim().to_ascii_lowercase();
    match kind.as_str() {
        "step.pending" | "workflow.step.pending" => Some(StepLifecycle::Pending),
        "step.running" | "workflow.step.running" | "step.started" | "workflow.step.started" => {
            Some(StepLifecycle::Running)
        }
        "step.succeeded"
        | "workflow.step.succeeded"
        | "step.completed"
        | "workflow.step.completed"
        | "step.finished"
        | "workflow.step.finished"
        | "step.failed"
        | "workflow.step.failed"
        | "step.blocked"
        | "workflow.step.blocked"
        | "step.cancelled"
        | "workflow.step.cancelled" => Some(StepLifecycle::Closed),
        _ => None,
    }
}

fn is_checkpoint_kind(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "checkpoint" | "workflow.checkpoint" | "workflow.checkpoint.recorded"
    )
}

fn redact_value(value: &Value, depth: usize) -> Value {
    if depth >= MAX_REDACTION_DEPTH {
        return Value::String(hash_label(value));
    }

    let Value::Object(map) = value else {
        return Value::String(hash_label(value));
    };
    if map.len() > MAX_PAYLOAD_OBJECT_KEYS {
        return Value::String(hash_label(value));
    }

    let mut redacted = Map::new();
    for (key, value) in map {
        let normalized = normalize_key(key);
        let display_key = canonical_payload_key(&normalized, key);
        let replacement = if is_secret_key(&normalized) {
            Value::String(hash_label(value))
        } else if is_safe_key(&normalized) {
            redact_safe_value(&normalized, value, depth + 1)
        } else {
            Value::String(REDACTED_LABEL.to_owned())
        };
        redacted.insert(display_key, replacement);
    }
    Value::Object(redacted)
}

/// Return the only property name that may be exposed for a payload key.
///
/// Known safe and secret keys use a canonical label; any other property name is
/// replaced by a bounded, deterministic hash label. This prevents a sensitive
/// value embedded in a property name from reaching a browser even when the
/// value itself is redacted.
fn canonical_payload_key(normalized: &str, original: &str) -> String {
    let canonical = match normalized {
        "kind" => Some("kind"),
        "state" => Some("state"),
        "statefrom" => Some("stateFrom"),
        "stateto" => Some("stateTo"),
        "status" => Some("status"),
        "outcome" => Some("outcome"),
        "sequence" => Some("sequence"),
        "stepid" => Some("stepId"),
        "runid" => Some("runId"),
        "requestid" => Some("requestId"),
        "attempt" => Some("attempt"),
        "count" => Some("count"),
        "durationms" => Some("durationMs"),
        "bytes" => Some("bytes"),
        "sha256" => Some("sha256"),
        "payloadsha256" => Some("payloadSha256"),
        "terminal" => Some("terminal"),
        "token" => Some("token"),
        "password" => Some("password"),
        "secret" => Some("secret"),
        "authorization" => Some("authorization"),
        "apikey" => Some("api_key"),
        "prompt" => Some("prompt"),
        "content" => Some("content"),
        _ => None,
    };
    canonical.map_or_else(
        || {
            let digest = sha256_hex(original.as_bytes());
            format!("redacted_{}", &digest[..12])
        },
        str::to_owned,
    )
}

fn redact_safe_value(key: &str, value: &Value, depth: usize) -> Value {
    if depth >= MAX_REDACTION_DEPTH {
        return Value::String(hash_label(value));
    }

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => {
            if is_safe_payload_string(key, text) {
                Value::String(text.chars().take(MAX_SAFE_STRING_CHARS).collect())
            } else {
                Value::String(hash_label(value))
            }
        }
        Value::Array(items) => {
            if items.len() > MAX_PAYLOAD_COLLECTION_ITEMS {
                return Value::String(hash_label(value));
            }
            Value::Array(
                items
                    .iter()
                    .map(|item| redact_safe_value(key, item, depth + 1))
                    .collect(),
            )
        }
        Value::Object(_) => redact_value(value, depth),
    }
}

/// Retain a string only when its key has a validated operational grammar.
///
/// An allowlisted key is not permission to retain arbitrary free text. State
/// and status must parse as the canonical workflow enums, hashes must be
/// lowercase SHA-256, identifiers must have the operational identifier shape,
/// and every other safe key retains only booleans/numbers. Invalid strings are
/// replaced by a SHA-256 fingerprint before they can reach a trace response.
fn is_safe_payload_string(key: &str, text: &str) -> bool {
    match key {
        "state" | "statefrom" | "stateto" => crate::workflow::parse_workflow_state(text).is_ok(),
        "status" => {
            crate::workflow::parse_workflow_state(text).is_ok()
                || crate::workflow::parse_workflow_step_status(text).is_ok()
        }
        "outcome" => matches!(
            text,
            "ACCEPTED"
                | "REPAIR_REQUIRED"
                | "BLOCKED"
                | "HUMAN_REQUIRED"
                | "NOT_TESTED"
                | "PASS"
                | "FAIL"
                | "CHANGES_REQUESTED"
        ),
        "sha256" | "payloadsha256" => is_lowercase_sha256(text),
        "stepid" => crate::workflow::is_workflow_step_id(text),
        "runid" => crate::workflow::is_workflow_run_id(text),
        "requestid" => crate::workflow::is_uuid(text),
        // `kind` is already exposed as the validated top-level event kind.
        // Numeric keys retain numbers via the scalar branch. Any string value
        // for these or any other allowlisted key is hashed.
        _ => false,
    }
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn is_secret_key(normalized: &str) -> bool {
    SECRET_KEY_MARKERS
        .iter()
        .any(|marker| normalized.contains(*marker))
}

fn is_safe_key(normalized: &str) -> bool {
    SAFE_PAYLOAD_KEYS.contains(&normalized)
}

fn hash_label(value: &Value) -> String {
    serde_json::to_vec(value).map_or_else(
        |_| REDACTED_LABEL.to_owned(),
        |bytes| {
            let digest = sha256_hex(&bytes);
            format!("{HASH_LABEL_PREFIX}{digest}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(character: char) -> String {
        character.to_string().repeat(64)
    }

    const REQUEST_ID: &str = "00000000-0000-4000-8000-000000000001";
    const RUN_ID: &str = "workflow-00000000-0000-4000-8000-000000000002";

    fn step_id(suffix: u8) -> String {
        format!("step-00000000-0000-4000-8000-{suffix:012}")
    }

    fn event(
        sequence: u64,
        kind: &str,
        from: Option<WorkflowState>,
        to: Option<WorkflowState>,
    ) -> ReplayEvent {
        ReplayEvent {
            sequence,
            request_id: REQUEST_ID.into(),
            run_id: RUN_ID.into(),
            step_id: None,
            kind: kind.into(),
            state_from: from,
            state_to: to,
            payload_sha256: sha('a'),
            recorded_at: "2026-11-09T00:00:00Z".into(),
        }
    }

    fn transition(sequence: u64, from: WorkflowState, to: WorkflowState) -> ReplayEvent {
        event(sequence, "workflow.transitioned", Some(from), Some(to))
    }

    fn valid_trace_events() -> Vec<ReplayEvent> {
        vec![
            transition(1, WorkflowState::Submitted, WorkflowState::Assessing),
            transition(2, WorkflowState::Assessing, WorkflowState::Planned),
            transition(3, WorkflowState::Planned, WorkflowState::PlanReview),
            transition(4, WorkflowState::PlanReview, WorkflowState::Frozen),
            transition(5, WorkflowState::Frozen, WorkflowState::Executing),
            transition(6, WorkflowState::Executing, WorkflowState::Verifying),
            transition(7, WorkflowState::Verifying, WorkflowState::Reviewing),
            transition(8, WorkflowState::Reviewing, WorkflowState::Gate),
            transition(9, WorkflowState::Gate, WorkflowState::Converging),
            transition(10, WorkflowState::Converging, WorkflowState::Accepted),
        ]
    }

    #[test]
    fn valid_trace_is_sorted_and_reaches_terminal_state() {
        let mut events = valid_trace_events();
        events.reverse();

        let trace = build_trace(events).expect("valid replay trace");

        assert_eq!(
            trace
                .events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            (1..=10).collect::<Vec<_>>()
        );
        assert_eq!(trace.final_state, Some(WorkflowState::Accepted));
        assert!(trace.terminal);
        assert!(trace.checkpoints.is_empty());
        assert!(trace.open_steps.is_empty());
        assert!(trace.duplicate_sequences.is_empty());
        assert!(trace.missing_transitions.is_empty());
    }

    #[test]
    fn checkpoints_capture_the_state_at_their_sequence() {
        let trace = build_trace(vec![
            transition(1, WorkflowState::Submitted, WorkflowState::Assessing),
            transition(2, WorkflowState::Assessing, WorkflowState::Planned),
            event(3, "workflow.checkpoint", None, None),
            transition(4, WorkflowState::Planned, WorkflowState::PlanReview),
        ])
        .expect("checkpoint trace");

        assert_eq!(trace.checkpoints.len(), 1);
        assert_eq!(trace.checkpoints[0].sequence, 3);
        assert_eq!(trace.checkpoints[0].state, Some(WorkflowState::Planned));
        assert_eq!(trace.checkpoints[0].payload_sha256, sha('a'));
    }

    #[test]
    fn identical_duplicate_sequences_are_collapsed_and_reported() {
        let first = transition(1, WorkflowState::Submitted, WorkflowState::Assessing);
        let trace = build_trace(vec![
            first.clone(),
            first,
            transition(2, WorkflowState::Assessing, WorkflowState::Planned),
        ])
        .expect("identical replay is idempotent");

        assert_eq!(trace.events.len(), 2);
        assert_eq!(
            trace.duplicate_sequences,
            vec![DuplicateSequence {
                sequence: 1,
                occurrences: 2,
            }]
        );
        assert_eq!(trace.final_state, Some(WorkflowState::Planned));
    }

    #[test]
    fn conflicting_duplicate_sequences_are_rejected() {
        let first = transition(1, WorkflowState::Submitted, WorkflowState::Assessing);
        let mut conflicting = first.clone();
        conflicting.kind = "workflow.conflict".into();

        let error =
            build_trace(vec![first, conflicting]).expect_err("conflicting duplicate must fail");
        assert!(error.contains("duplicate sequence 1"), "{error}");
    }

    #[test]
    fn invalid_and_incomplete_transitions_are_rejected() {
        let invalid = build_trace(vec![transition(
            1,
            WorkflowState::Submitted,
            WorkflowState::Executing,
        )])
        .expect_err("invalid transition must fail");
        assert!(invalid.contains("invalid workflow transition"), "{invalid}");

        let mut incomplete = transition(1, WorkflowState::Submitted, WorkflowState::Assessing);
        incomplete.state_to = None;
        let incomplete_error =
            build_trace(vec![incomplete]).expect_err("incomplete transition must fail");
        assert!(
            incomplete_error.contains("incomplete workflow transition"),
            "{incomplete_error}"
        );
    }

    #[test]
    fn terminal_states_are_immutable() {
        for terminal in [
            WorkflowState::Accepted,
            WorkflowState::Blocked,
            WorkflowState::Failed,
            WorkflowState::Cancelled,
        ] {
            for target in [
                WorkflowState::Submitted,
                WorkflowState::Executing,
                WorkflowState::Accepted,
            ] {
                assert!(!transition_allowed(terminal, target));
            }
        }

        let mut events = valid_trace_events();
        events.push(transition(
            11,
            WorkflowState::Accepted,
            WorkflowState::Executing,
        ));
        let error = build_trace(events).expect_err("terminal state must not transition");
        assert!(error.contains("invalid workflow transition"), "{error}");
    }

    #[test]
    fn pending_and_running_steps_remain_open_until_closed() {
        let pending_id = step_id(1);
        let running_id = step_id(2);
        let closed_id = step_id(3);
        let mut pending = event(1, "step.pending", None, None);
        pending.step_id = Some(pending_id.clone());
        let mut running = event(2, "step.running", None, None);
        running.step_id = Some(running_id.clone());
        let mut started = event(3, "step.started", None, None);
        started.step_id = Some(closed_id.clone());
        let mut succeeded = event(4, "step.succeeded", None, None);
        succeeded.step_id = Some(closed_id);

        let trace = build_trace(vec![pending, running, started, succeeded]).expect("open steps");

        assert_eq!(
            trace.open_steps,
            vec![
                OpenStep {
                    step_id: pending_id,
                    status: WorkflowStepStatus::Pending,
                    last_sequence: 1,
                },
                OpenStep {
                    step_id: running_id,
                    status: WorkflowStepStatus::Running,
                    last_sequence: 2,
                },
            ]
        );
        assert!(!trace.terminal);
    }

    #[test]
    fn terminal_trace_reports_open_steps_for_diagnosis() {
        let mut events = valid_trace_events();
        let late_step = step_id(4);
        let mut running = event(11, "step.running", None, None);
        running.step_id = Some(late_step.clone());
        events.push(running);

        let trace = build_trace(events).expect("projection still reports the anomaly");

        assert!(trace.terminal);
        assert_eq!(trace.open_steps.len(), 1);
        assert_eq!(trace.open_steps[0].step_id, late_step);
    }

    #[test]
    fn chain_gaps_are_reported_without_inventing_state() {
        let trace = build_trace(vec![
            transition(5, WorkflowState::Frozen, WorkflowState::Executing),
            transition(6, WorkflowState::Executing, WorkflowState::Verifying),
        ])
        .expect("valid transitions with a missing prefix");

        assert_eq!(trace.final_state, Some(WorkflowState::Verifying));
        assert_eq!(
            trace.missing_transitions,
            vec![MissingTransition {
                sequence: 5,
                expected_from: WorkflowState::Submitted,
                observed_from: WorkflowState::Frozen,
            }]
        );
    }

    #[test]
    fn payload_hashes_must_be_lowercase_sha256() {
        let invalid_hashes = [
            String::new(),
            "abc".to_owned(),
            "A".repeat(64),
            "g".repeat(64),
        ];
        for invalid in invalid_hashes {
            let mut event = transition(1, WorkflowState::Submitted, WorkflowState::Assessing);
            event.payload_sha256 = invalid;
            let error = build_trace(vec![event]).expect_err("invalid hash must fail");
            assert!(error.contains("SHA-256"), "{error}");
        }
    }

    #[test]
    fn trace_must_replay_a_single_run() {
        let mut other = transition(1, WorkflowState::Submitted, WorkflowState::Assessing);
        other.run_id = "workflow-00000000-0000-4000-8000-000000000003".into();
        let error = build_trace(vec![
            transition(1, WorkflowState::Submitted, WorkflowState::Assessing),
            other,
        ])
        .expect_err("mixed runs are ambiguous");
        assert!(error.contains("multiple runs"), "{error}");
    }

    #[test]
    fn redaction_removes_secret_shaped_values_and_unknown_keys() {
        let payload = serde_json::json!({
            "token": "secret-token-value",
            "password": "hunter2",
            "secret": "shh",
            "authorization": "Bearer abc.def",
            "api_key": "key-123",
            "apiKey": "key-456",
            "prompt": "raw prompt body",
            "content": "source body",
            "state": "EXECUTING",
            "sequence": 7,
            "stepId": "step-00000000-0000-4000-8000-000000000004",
            "unknown": "untrusted detail",
            "nested": {"token": "nested-secret"}
        });

        let redacted = redact_payload(&payload);
        let serialized = serde_json::to_string(&redacted).expect("serializable redaction");

        for secret in [
            "secret-token-value",
            "hunter2",
            "shh",
            "Bearer abc.def",
            "key-123",
            "key-456",
            "raw prompt body",
            "source body",
            "untrusted detail",
            "nested-secret",
        ] {
            assert!(!serialized.contains(secret), "redaction leaked {secret}");
        }
        assert_eq!(redacted["state"], "EXECUTING");
        assert_eq!(redacted["sequence"], 7);
        assert_eq!(
            redacted["stepId"],
            "step-00000000-0000-4000-8000-000000000004"
        );
        assert!(!serialized.contains("unknown"));
        assert!(
            redacted["token"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
    }

    #[test]
    fn allowlisted_keys_do_not_retain_free_form_strings() {
        let payload = serde_json::json!({
            "state": "raw prompt or token",
            "status": "raw token",
            "stepId": "raw prompt",
            "sha256": "not-a-hash",
            "kind": "raw prompt",
            "outcome": "ACCEPTED",
            "sequence": 7,
            "terminal": false,
        });

        let redacted = redact_payload(&payload);
        let serialized = serde_json::to_string(&redacted).expect("serializable redaction");

        for raw in [
            "raw prompt or token",
            "raw token",
            "raw prompt",
            "not-a-hash",
        ] {
            assert!(!serialized.contains(raw), "redaction retained {raw}");
        }
        assert_eq!(redacted["outcome"], "ACCEPTED");
        assert_eq!(redacted["sequence"], 7);
        assert_eq!(redacted["terminal"], false);
        for key in ["state", "status", "stepId", "sha256", "kind"] {
            assert!(
                redacted[key]
                    .as_str()
                    .is_some_and(|value| value.starts_with("sha256:")),
                "{key} must be hashed"
            );
        }
    }

    #[test]
    fn sensitive_or_unrecognized_property_names_are_not_exposed() {
        let payload = serde_json::json!({
            "prompt sk-proj-very-secret": "value",
            "state sk-other-secret": "EXECUTING",
            "safe raw detail": "detail",
            "state": "EXECUTING",
        });

        let redacted = redact_payload(&payload);
        let object = redacted.as_object().expect("redacted object");
        let serialized = serde_json::to_string(&redacted).expect("serializable redaction");

        assert_eq!(object["state"], "EXECUTING");
        for raw in [
            "sk-proj-very-secret",
            "state sk-other-secret",
            "safe raw detail",
        ] {
            assert!(!serialized.contains(raw), "property name leaked {raw}");
        }
        assert!(
            object
                .keys()
                .all(|key| key == "state" || key.starts_with("redacted_"))
        );
    }

    #[test]
    fn redaction_recurses_into_allowlisted_objects() {
        let payload = serde_json::json!({
            "state": {"status": "RUNNING", "token": "nested-secret"}
        });

        let redacted = redact_payload(&payload);

        assert_eq!(redacted["state"]["status"], "RUNNING");
        assert!(!redacted.to_string().contains("nested-secret"));
        assert!(
            redacted["state"]["token"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
    }

    #[test]
    fn redaction_hashes_non_object_payloads() {
        let redacted = redact_payload(&serde_json::json!("raw prompt body"));

        assert!(
            redacted
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
        assert!(!redacted.to_string().contains("raw prompt body"));
    }

    #[test]
    fn comparison_reports_state_sequence_and_hash_differences() {
        let expected = build_trace(valid_trace_events()).expect("expected trace");

        let mut actual_events = valid_trace_events();
        actual_events.retain(|event| event.sequence != 4);
        for event in &mut actual_events {
            if event.sequence == 6 {
                event.payload_sha256 = sha('b');
            }
            if event.sequence == 10 {
                event.state_to = Some(WorkflowState::Blocked);
            }
        }
        actual_events.push(event(11, "checkpoint", None, None));
        let actual = build_trace(actual_events).expect("actual trace");

        let differences = compare_replay(&expected, &actual);

        assert!(differences.contains(&ReplayDifference::FinalState {
            expected: Some(WorkflowState::Accepted),
            actual: Some(WorkflowState::Blocked),
        }));
        assert!(differences.contains(&ReplayDifference::MissingSequence { sequence: 4 }));
        assert!(differences.contains(&ReplayDifference::UnexpectedSequence { sequence: 11 }));
        assert!(differences.contains(&ReplayDifference::PayloadHash {
            sequence: 6,
            expected: sha('a'),
            actual: sha('b'),
        }));
        assert!(differences.iter().any(|difference| matches!(
            difference,
            ReplayDifference::StateTransition { sequence: 10, .. }
        )));
    }

    #[test]
    fn identical_replays_have_no_differences() {
        let trace = build_trace(valid_trace_events()).expect("trace");

        assert!(compare_replay(&trace, &trace).is_empty());
    }

    #[test]
    fn comparison_reports_terminal_flag_differences() {
        let expected = build_trace(valid_trace_events()).expect("expected trace");
        let mut events = valid_trace_events();
        events.pop();
        let actual = build_trace(events).expect("actual trace");

        assert!(
            compare_replay(&expected, &actual).contains(&ReplayDifference::Terminal {
                expected: true,
                actual: false,
            })
        );
    }

    #[test]
    fn comparison_reports_run_identity_differences() {
        let expected = build_trace(valid_trace_events()).expect("expected trace");
        let mut events = valid_trace_events();
        for event in &mut events {
            event.run_id = "workflow-00000000-0000-4000-8000-000000000003".into();
        }
        let actual = build_trace(events).expect("actual trace");

        assert!(
            compare_replay(&expected, &actual).contains(&ReplayDifference::Run {
                expected: RUN_ID.into(),
                actual: "workflow-00000000-0000-4000-8000-000000000003".into(),
            })
        );
    }
}
