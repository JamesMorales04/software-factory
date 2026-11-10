//! Bounded, redacted workflow trace view.
//!
//! This module is the read-side projection for WP11. It joins:
//!
//! - durable workflow events loaded from the Core event ledger;
//! - the pure deterministic [`crate::replay::build_trace`] projection;
//! - the durable `workflow_steps` rows;
//! - an operator-facing alert list.
//!
//! It never writes authority, never reads provider credentials and never
//! exposes a raw event payload. Every event payload is reduced through
//! [`crate::replay::redact_payload`] before it can reach an Office client.
//! Events are ordered by durable sequence, never by `recorded_at`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::planning::{WorkflowState, WorkflowStepStatus};
use crate::replay::{self, ReplayEvent, ReplayTrace};
use crate::workflow::{WorkflowRunRecord, WorkflowStepRecord};

/// Default number of durable events returned by a trace read.
pub const TRACE_EVENT_DEFAULT: usize = 500;
/// Hard bound for a trace read. A caller may request fewer events, never more.
pub const TRACE_EVENT_MAX: usize = 2_000;
/// Bound for a replay error string exposed to an operator client.
const MAX_TRACE_ERROR_CHARS: usize = 512;

/// A durable event loaded from the Core ledger together with its redacted
/// display payload. `malformed` is set when the ledger row cannot be mapped to
/// a complete workflow event, so the read side reports the anomaly instead of
/// inventing a transition.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedTraceEvent {
    pub event: ReplayEvent,
    pub payload: Value,
    pub malformed: bool,
}

/// One event as displayed in the Office event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TraceEventView {
    pub sequence: u64,
    pub step_id: Option<String>,
    pub kind: String,
    pub state_from: Option<WorkflowState>,
    pub state_to: Option<WorkflowState>,
    pub payload_sha256: String,
    pub recorded_at: String,
    /// Redacted operational payload. Unknown keys are `[REDACTED]`; secret or
    /// prompt-shaped keys are replaced by a SHA-256 fingerprint.
    pub payload: Value,
}

/// One durable step projected for the trace view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TraceStepView {
    pub step_id: String,
    pub kind: String,
    pub status: WorkflowStepStatus,
    pub attempt: u32,
    pub input_sha256: String,
    pub output_sha256: String,
    pub created_at: String,
    pub updated_at: String,
    /// Whether the replay projection still sees this step as pending/running.
    /// This is an anomaly indicator, not a replacement for the durable status.
    pub open: bool,
    /// Last workflow-event sequence that referenced this step, if any.
    pub last_sequence: Option<u64>,
}

/// The durable run identity included in a trace read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TraceRunView {
    pub id: String,
    pub request_id: String,
    pub campaign_id: Option<String>,
    pub state: WorkflowState,
    pub revision: u32,
    pub plan_sha256: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<WorkflowRunRecord> for TraceRunView {
    fn from(run: WorkflowRunRecord) -> Self {
        Self {
            id: run.id,
            request_id: run.request_id,
            campaign_id: run.campaign_id,
            state: run.state,
            revision: run.revision,
            plan_sha256: run.plan_sha256,
            created_at: run.created_at,
            updated_at: run.updated_at,
        }
    }
}

/// Stable alert categories. The Office localizes labels from the code; Core
/// never sends a rendered message that could contain a payload value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TraceAlertKind {
    DuplicateSequence,
    MissingTransition,
    OpenStep,
    TerminalState,
    NonTerminalState,
    TraceTruncated,
    ReplayRejected,
    StateDivergence,
    NoEvents,
    MalformedEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TraceAlertSeverity {
    Info,
    Warning,
    Critical,
}

/// One structured trace alert. Optional fields are populated only by the
/// relevant alert kind; the UI renders a localized description from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TraceAlert {
    pub kind: TraceAlertKind,
    pub severity: TraceAlertSeverity,
    pub sequence: Option<u64>,
    pub step_id: Option<String>,
    pub occurrences: Option<usize>,
    pub expected_from: Option<WorkflowState>,
    pub observed_from: Option<WorkflowState>,
    pub expected_state: Option<WorkflowState>,
    pub observed_state: Option<WorkflowState>,
}

impl TraceAlert {
    fn new(kind: TraceAlertKind, severity: TraceAlertSeverity) -> Self {
        Self {
            kind,
            severity,
            sequence: None,
            step_id: None,
            occurrences: None,
            expected_from: None,
            observed_from: None,
            expected_state: None,
            observed_state: None,
        }
    }
}

/// The complete trace read returned by `GET /agentic/requests/{id}/trace`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkflowTraceView {
    pub request_id: String,
    pub run: Option<TraceRunView>,
    /// Pure replay projection. `None` when no run exists or the ledger cannot
    /// be replayed safely; `replay_error` then explains the rejection.
    pub trace: Option<ReplayTrace>,
    /// Display projection of the replay event list, in the same sequence order,
    /// with redacted payloads.
    pub events: Vec<TraceEventView>,
    pub steps: Vec<TraceStepView>,
    pub alerts: Vec<TraceAlert>,
    pub terminal: bool,
    pub event_limit: usize,
    pub truncated: bool,
    pub replay_error: Option<String>,
}

/// Empty trace for a request that has no durable workflow run yet.
pub fn empty(request_id: &str, event_limit: usize) -> WorkflowTraceView {
    WorkflowTraceView {
        request_id: request_id.to_owned(),
        run: None,
        trace: None,
        events: Vec::new(),
        steps: Vec::new(),
        alerts: Vec::new(),
        terminal: false,
        event_limit: event_limit.clamp(1, TRACE_EVENT_MAX),
        truncated: false,
        replay_error: None,
    }
}

/// Build the bounded, redacted view for one durable run.
///
/// `events` must already be in ascending durable sequence order. The function
/// reuses [`replay::build_trace`] for every ordering, transition, duplicate and
/// open-step decision; it does not maintain a second state-machine projection.
pub fn project(
    request_id: &str,
    run: WorkflowRunRecord,
    events: Vec<LoadedTraceEvent>,
    steps: Vec<WorkflowStepRecord>,
    truncated: bool,
    event_limit: usize,
) -> WorkflowTraceView {
    let event_limit = event_limit.clamp(1, TRACE_EVENT_MAX);
    let mut alerts = Vec::new();
    let mut malformed_sequences = Vec::new();
    let mut payload_by_sequence = BTreeMap::new();
    let mut raw_events = Vec::with_capacity(events.len());

    for loaded in &events {
        if loaded.malformed {
            malformed_sequences.push(loaded.event.sequence);
        }
        payload_by_sequence.insert(loaded.event.sequence, loaded.payload.clone());
        raw_events.push(loaded.event.clone());
    }

    let replay_result = replay::build_trace(raw_events);
    let (trace, display_events, terminal, replay_error) = match replay_result {
        Ok(trace) => {
            let display_events = trace
                .events
                .iter()
                .map(|event| TraceEventView {
                    sequence: event.sequence,
                    step_id: event.step_id.clone(),
                    kind: event.kind.clone(),
                    state_from: event.state_from,
                    state_to: event.state_to,
                    payload_sha256: event.payload_sha256.clone(),
                    recorded_at: event.recorded_at.clone(),
                    payload: payload_by_sequence
                        .get(&event.sequence)
                        .cloned()
                        .unwrap_or(Value::Null),
                })
                .collect::<Vec<_>>();
            let terminal = trace.terminal;
            (Some(trace), display_events, terminal, None)
        }
        Err(error) => {
            let display_events = events.iter().map(event_view).collect::<Vec<_>>();
            let mut alert =
                TraceAlert::new(TraceAlertKind::ReplayRejected, TraceAlertSeverity::Critical);
            alert.sequence = events.last().map(|loaded| loaded.event.sequence);
            alerts.push(alert);
            let replay_error = Some(sanitize_error(&error));
            (None, display_events, false, replay_error)
        }
    };

    for sequence in malformed_sequences {
        let mut alert =
            TraceAlert::new(TraceAlertKind::MalformedEvent, TraceAlertSeverity::Warning);
        alert.sequence = Some(sequence);
        alerts.push(alert);
    }

    let open_step_ids = trace
        .as_ref()
        .map(|trace| {
            trace
                .open_steps
                .iter()
                .map(|step| step.step_id.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    if let Some(trace) = &trace {
        for duplicate in &trace.duplicate_sequences {
            let mut alert = TraceAlert::new(
                TraceAlertKind::DuplicateSequence,
                TraceAlertSeverity::Warning,
            );
            alert.sequence = Some(duplicate.sequence);
            alert.occurrences = Some(duplicate.occurrences);
            alerts.push(alert);
        }
        for missing in &trace.missing_transitions {
            let mut alert = TraceAlert::new(
                TraceAlertKind::MissingTransition,
                TraceAlertSeverity::Warning,
            );
            alert.sequence = Some(missing.sequence);
            alert.expected_from = Some(missing.expected_from);
            alert.observed_from = Some(missing.observed_from);
            alerts.push(alert);
        }
        for step in &trace.open_steps {
            let mut alert = TraceAlert::new(
                TraceAlertKind::OpenStep,
                if trace.terminal {
                    TraceAlertSeverity::Critical
                } else {
                    TraceAlertSeverity::Info
                },
            );
            alert.step_id = Some(step.step_id.clone());
            alert.sequence = Some(step.last_sequence);
            alerts.push(alert);
        }

        if !trace.events.is_empty() && trace.final_state != Some(run.state) {
            let mut alert = TraceAlert::new(
                TraceAlertKind::StateDivergence,
                TraceAlertSeverity::Critical,
            );
            alert.expected_state = Some(run.state);
            alert.observed_state = trace.final_state;
            alerts.push(alert);
        }

        let mut state_alert = if trace.terminal {
            TraceAlert::new(TraceAlertKind::TerminalState, TraceAlertSeverity::Info)
        } else {
            TraceAlert::new(TraceAlertKind::NonTerminalState, TraceAlertSeverity::Info)
        };
        state_alert.sequence = trace.events.last().map(|event| event.sequence);
        alerts.push(state_alert);

        if trace.events.is_empty() {
            alerts.push(TraceAlert::new(
                TraceAlertKind::NoEvents,
                TraceAlertSeverity::Warning,
            ));
        }
    }

    if truncated {
        let mut alert =
            TraceAlert::new(TraceAlertKind::TraceTruncated, TraceAlertSeverity::Warning);
        alert.sequence = display_events.first().map(|event| event.sequence);
        alerts.push(alert);
    }

    let last_sequence_by_step =
        display_events
            .iter()
            .fold(BTreeMap::<&str, u64>::new(), |mut sequences, event| {
                if let Some(step_id) = event.step_id.as_deref() {
                    sequences
                        .entry(step_id)
                        .and_modify(|sequence| *sequence = (*sequence).max(event.sequence))
                        .or_insert(event.sequence);
                }
                sequences
            });

    let steps = steps
        .into_iter()
        .map(|step| TraceStepView {
            open: open_step_ids.contains(&step.id),
            last_sequence: last_sequence_by_step.get(step.id.as_str()).copied(),
            step_id: step.id,
            kind: step.kind,
            status: step.status,
            attempt: step.attempt,
            input_sha256: step.input_sha256,
            output_sha256: step.output_sha256,
            created_at: step.created_at,
            updated_at: step.updated_at,
        })
        .collect::<Vec<_>>();

    WorkflowTraceView {
        request_id: request_id.to_owned(),
        run: Some(run.into()),
        trace,
        events: display_events,
        steps,
        alerts,
        terminal,
        event_limit,
        truncated,
        replay_error,
    }
}

fn event_view(loaded: &LoadedTraceEvent) -> TraceEventView {
    TraceEventView {
        sequence: loaded.event.sequence,
        step_id: loaded.event.step_id.clone(),
        kind: loaded.event.kind.clone(),
        state_from: loaded.event.state_from,
        state_to: loaded.event.state_to,
        payload_sha256: loaded.event.payload_sha256.clone(),
        recorded_at: loaded.event.recorded_at.clone(),
        payload: loaded.payload.clone(),
    }
}

fn sanitize_error(error: &str) -> String {
    error
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_TRACE_ERROR_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(character: char) -> String {
        character.to_string().repeat(64)
    }

    fn event(
        sequence: u64,
        kind: &str,
        from: Option<WorkflowState>,
        to: Option<WorkflowState>,
    ) -> ReplayEvent {
        ReplayEvent {
            sequence,
            request_id: "request-1".into(),
            run_id: "run-1".into(),
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

    fn loaded(event: ReplayEvent, payload: Value) -> LoadedTraceEvent {
        LoadedTraceEvent {
            event,
            payload,
            malformed: false,
        }
    }

    fn run(state: WorkflowState) -> WorkflowRunRecord {
        WorkflowRunRecord {
            id: "run-1".into(),
            request_id: "request-1".into(),
            campaign_id: None,
            state,
            revision: 1,
            plan_sha256: String::new(),
            created_at: "2026-11-09T00:00:00Z".into(),
            updated_at: "2026-11-09T00:00:00Z".into(),
        }
    }

    fn step(id: &str, status: WorkflowStepStatus) -> WorkflowStepRecord {
        WorkflowStepRecord {
            id: id.into(),
            run_id: "run-1".into(),
            kind: "unit.execute".into(),
            status,
            attempt: 1,
            input_sha256: sha('b'),
            output_sha256: sha('c'),
            detail: "not exposed".into(),
            created_at: "2026-11-09T00:00:00Z".into(),
            updated_at: "2026-11-09T00:00:01Z".into(),
        }
    }

    fn valid_terminal_events() -> Vec<LoadedTraceEvent> {
        vec![
            loaded(
                transition(1, WorkflowState::Submitted, WorkflowState::Assessing),
                Value::Null,
            ),
            loaded(
                transition(2, WorkflowState::Assessing, WorkflowState::Planned),
                Value::Null,
            ),
            loaded(
                transition(3, WorkflowState::Planned, WorkflowState::PlanReview),
                Value::Null,
            ),
            loaded(
                transition(4, WorkflowState::PlanReview, WorkflowState::Frozen),
                Value::Null,
            ),
            loaded(
                transition(5, WorkflowState::Frozen, WorkflowState::Executing),
                Value::Null,
            ),
            loaded(
                transition(6, WorkflowState::Executing, WorkflowState::Verifying),
                Value::Null,
            ),
            loaded(
                transition(7, WorkflowState::Verifying, WorkflowState::Reviewing),
                Value::Null,
            ),
            loaded(
                transition(8, WorkflowState::Reviewing, WorkflowState::Gate),
                Value::Null,
            ),
            loaded(
                transition(9, WorkflowState::Gate, WorkflowState::Converging),
                Value::Null,
            ),
            loaded(
                transition(10, WorkflowState::Converging, WorkflowState::Accepted),
                Value::Null,
            ),
        ]
    }

    #[test]
    fn terminal_run_reports_terminal_alert_and_no_open_steps() {
        let view = project(
            "request-1",
            run(WorkflowState::Accepted),
            valid_terminal_events(),
            vec![step("step-1", WorkflowStepStatus::Succeeded)],
            false,
            TRACE_EVENT_DEFAULT,
        );

        assert!(view.terminal);
        assert_eq!(
            view.trace.as_ref().unwrap().final_state,
            Some(WorkflowState::Accepted)
        );
        assert_eq!(view.events.len(), 10);
        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::TerminalState)
        );
        assert!(
            !view
                .alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::NonTerminalState)
        );
        assert_eq!(view.steps[0].status, WorkflowStepStatus::Succeeded);
        assert!(!view.steps[0].open);
        assert_eq!(view.steps[0].last_sequence, None);
    }

    #[test]
    fn duplicate_gap_open_step_and_state_divergence_are_reported() {
        let mut started = event(3, "workflow.step.started", None, None);
        started.step_id = Some("step-open".into());
        let step_events = vec![
            loaded(
                transition(2, WorkflowState::Frozen, WorkflowState::Executing),
                Value::Null,
            ),
            loaded(
                transition(2, WorkflowState::Frozen, WorkflowState::Executing),
                Value::Null,
            ),
            loaded(started, Value::Null),
        ];
        let view = project(
            "request-1",
            run(WorkflowState::Accepted),
            step_events,
            vec![step("step-open", WorkflowStepStatus::Running)],
            false,
            TRACE_EVENT_DEFAULT,
        );

        let kinds = view
            .alerts
            .iter()
            .map(|alert| alert.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&TraceAlertKind::DuplicateSequence));
        assert!(kinds.contains(&TraceAlertKind::MissingTransition));
        assert!(kinds.contains(&TraceAlertKind::OpenStep));
        assert!(kinds.contains(&TraceAlertKind::StateDivergence));
        assert!(view.steps[0].open);
        assert_eq!(view.steps[0].last_sequence, Some(3));
    }

    #[test]
    fn payloads_are_redacted_before_they_reach_the_view() {
        let secret = "raw-prompt-and-token-value";
        let payload = replay::redact_payload(&serde_json::json!({
            "token": secret,
            "prompt": secret,
            "state": "EXECUTING",
            "unknown": secret,
        }));
        let view = project(
            "request-1",
            run(WorkflowState::Executing),
            vec![loaded(event(1, "workflow.event", None, None), payload)],
            Vec::new(),
            false,
            TRACE_EVENT_DEFAULT,
        );

        let serialized = serde_json::to_string(&view).expect("serializable trace view");
        assert!(!serialized.contains(secret));
        assert_eq!(view.events[0].payload["state"], "EXECUTING");
        assert_eq!(view.events[0].payload["unknown"], "[REDACTED]");
        assert!(
            view.events[0].payload["token"]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
    }

    #[test]
    fn replay_rejection_is_reported_without_raw_payload() {
        let secret = "raw-prompt-and-token-value";
        let payload = replay::redact_payload(&serde_json::json!({
            "token": secret,
            "prompt": secret,
        }));
        let view = project(
            "request-1",
            run(WorkflowState::Submitted),
            vec![loaded(
                transition(1, WorkflowState::Submitted, WorkflowState::Executing),
                payload,
            )],
            Vec::new(),
            false,
            TRACE_EVENT_DEFAULT,
        );

        assert!(view.trace.is_none());
        assert!(view.replay_error.is_some());
        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::ReplayRejected)
        );
        assert!(!serde_json::to_string(&view).unwrap().contains(secret));
        assert_eq!(view.events.len(), 1);
    }

    #[test]
    fn truncated_read_is_reported_and_bounded() {
        let view = project(
            "request-1",
            run(WorkflowState::Assessing),
            vec![loaded(
                transition(1, WorkflowState::Submitted, WorkflowState::Assessing),
                Value::Null,
            )],
            Vec::new(),
            true,
            25,
        );

        assert!(view.truncated);
        assert_eq!(view.event_limit, 25);
        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::TraceTruncated)
        );
    }

    #[test]
    fn malformed_event_is_reported() {
        let mut malformed = loaded(event(1, "workflow.step.started", None, None), Value::Null);
        malformed.malformed = true;

        let view = project(
            "request-1",
            run(WorkflowState::Submitted),
            vec![malformed],
            Vec::new(),
            false,
            TRACE_EVENT_DEFAULT,
        );

        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::MalformedEvent)
        );
    }

    #[test]
    fn empty_view_has_no_run_and_no_alerts() {
        let view = empty("request-1", 10_000);
        assert!(view.run.is_none());
        assert!(view.trace.is_none());
        assert!(view.events.is_empty());
        assert!(view.steps.is_empty());
        assert!(view.alerts.is_empty());
        assert_eq!(view.event_limit, TRACE_EVENT_MAX);
    }
}
