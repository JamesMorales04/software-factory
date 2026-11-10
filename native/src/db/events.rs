//! Read-side event ledger projections.
//!
//! WP11 owns this module. It maps durable `events` rows into the pure replay
//! vocabulary without maintaining a second state-machine projection. The
//! durable sequence is the only ordering key; `recorded_at` is display data.

use super::*;

use serde_json::Value;

#[cfg(test)]
use crate::planning::WorkflowState;
use crate::replay::{self, ReplayEvent};
use crate::trace::{self, LoadedTraceEvent, TRACE_EVENT_MAX, WorkflowTraceView};
use crate::workflow::{self, WorkflowRunRecord};

impl Database {
    /// Load a bounded, redacted trace for the latest workflow run bound to a
    /// request. Events are read newest-first inside a bounded subquery and then
    /// returned in ascending durable sequence order.
    pub async fn workflow_trace(
        &self,
        request_id: &str,
        limit: usize,
    ) -> Result<WorkflowTraceView, Error> {
        workflow::validate_identifier("request id", request_id)?;
        let limit = limit.clamp(1, TRACE_EVENT_MAX);
        let Some(run) = self.workflow_run_for_request(request_id).await? else {
            return Ok(trace::empty(request_id, limit));
        };

        let rows: Vec<(i64, String, String, String)> = sqlx::query_as::query_as(
            "SELECT sequence,kind,detail,recorded_at FROM (
               SELECT sequence,kind,detail,recorded_at FROM events
               WHERE subject_id=?1 ORDER BY sequence DESC LIMIT ?2
             ) ORDER BY sequence",
        )
        .bind(&run.id)
        .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await?;

        let truncated = rows.len() > limit;
        let rows = if truncated {
            rows.into_iter().skip(1).collect::<Vec<_>>()
        } else {
            rows
        };

        let events = rows
            .into_iter()
            .map(|(sequence, kind, detail, recorded_at)| {
                loaded_event(&run, sequence, kind, detail, recorded_at)
            })
            .collect::<Vec<_>>();
        let (steps, steps_truncated) = self
            .workflow_steps_bounded(&run.id, trace::TRACE_STEP_MAX)
            .await?;

        Ok(trace::project(
            request_id,
            run,
            events,
            steps,
            trace::TraceBounds::new(limit, truncated, trace::TRACE_STEP_MAX, steps_truncated),
        ))
    }
}

fn loaded_event(
    run: &WorkflowRunRecord,
    sequence: i64,
    kind: String,
    detail: String,
    recorded_at: String,
) -> LoadedTraceEvent {
    let detail_parse = serde_json::from_str::<Value>(&detail);
    let detail_is_json_like = matches!(
        detail.trim_start().as_bytes().first(),
        Some(b'{') | Some(b'[')
    );
    let detail_value = detail_parse.as_ref().ok().cloned();
    let transition_event = matches!(
        kind.as_str(),
        "run.status_changed" | "workflow.transitioned"
    );
    // A transition row must carry a JSON object with both canonical states.
    // Legacy run status rows use a plain `status=...` string; those are not
    // claimed to be transitions. JSON-looking text that fails to parse is a
    // malformed transition, not an ordinary event.
    let mut malformed = transition_event && detail_parse.is_err() && detail_is_json_like;

    let state_from = transition_event
        .then(|| {
            detail_value
                .as_ref()
                .and_then(|value| value.get("from"))
                .and_then(Value::as_str)
        })
        .flatten()
        .and_then(|value| match workflow::parse_workflow_state(value) {
            Ok(state) => Some(state),
            Err(_) => {
                malformed = true;
                None
            }
        });
    let state_to = transition_event
        .then(|| {
            detail_value
                .as_ref()
                .and_then(|value| value.get("to"))
                .and_then(Value::as_str)
        })
        .flatten()
        .and_then(|value| match workflow::parse_workflow_state(value) {
            Ok(state) => Some(state),
            Err(_) => {
                malformed = true;
                None
            }
        });
    if transition_event
        && detail_value.as_ref().is_some_and(|value| {
            value.get("from").and_then(Value::as_str).is_none()
                || value.get("to").and_then(Value::as_str).is_none()
                || state_from.is_none()
                || state_to.is_none()
        })
    {
        malformed = true;
    }

    let step_id = detail_value
        .as_ref()
        .and_then(|value| value.get("stepId").or_else(|| value.get("step_id")))
        .and_then(Value::as_str)
        .and_then(
            |value| match workflow::validate_operational_identifier("step id", value) {
                Ok(()) => Some(value.to_owned()),
                Err(_) => {
                    malformed = true;
                    None
                }
            },
        );

    let payload = detail_value.as_ref().map_or_else(
        || replay::redact_payload(&Value::String(detail.clone())),
        replay::redact_payload,
    );

    LoadedTraceEvent {
        event: ReplayEvent {
            sequence: u64::try_from(sequence).unwrap_or(0),
            request_id: run.request_id.clone(),
            run_id: run.id.clone(),
            step_id,
            kind,
            state_from,
            state_to,
            payload_sha256: workflow::sha256_hex(detail.as_bytes()),
            recorded_at,
        },
        payload,
        malformed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use uuid::Uuid;

    use crate::planning::WorkflowStepStatus;
    use crate::trace::TraceAlertKind;

    async fn temp_db() -> Database {
        let path = std::env::temp_dir().join(format!("factory-trace-{}.sqlite", Uuid::new_v4()));
        Database::open(&path).await.unwrap()
    }

    async fn seed_request(db: &Database, request_id: &str) {
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/repo',?1,?2,'')",
        )
        .bind("a".repeat(40))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES ('action','project','objective','opencode','model','R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO standing_delegations
             (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
             VALUES ('delegation','owner','audit','project',1,'hash','{}',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,
              project_config_hash,state,created_at,updated_at)
             VALUES ('lead','project','action','delegation','objective','hash','hash',
                     'FROZEN',?1,?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO execution_profiles(id,current_revision,content_hash,created_at,updated_at)
             VALUES ('profile',1,'hash',?1,?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO execution_profile_revisions
             (revision_id,profile_id,revision,content_json,content_hash,actor,created_at)
             VALUES ('profile-revision','profile',1,'{}','hash','owner',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO intake_requests
             (id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
              profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
              submission_json,submission_hash,idempotency_key,created_at,updated_at)
             VALUES (?1,'project','objective','REVIEW_PLAN','profile','profile-revision',1,
                     'hash','profile','action','delegation','lead',NULL,'{}','hash',?2,?3,?3)",
        )
        .bind(request_id)
        .bind(format!("intake-{request_id}"))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn trace_loader_maps_transitions_and_step_lifecycle_in_sequence_order() {
        let db = temp_db().await;
        seed_request(&db, "request-trace").await;
        let run_id = db
            .ensure_workflow_run("request-trace", None, "")
            .await
            .unwrap();
        db.transition_workflow(
            &run_id,
            WorkflowState::Submitted,
            WorkflowState::Assessing,
            "token=secret-token-value",
        )
        .await
        .unwrap();
        let step = db.begin_step(&run_id, "unit.execute", "").await.unwrap();
        db.finish_step(
            &step.id,
            WorkflowStepStatus::Succeeded,
            "",
            "raw prompt body",
        )
        .await
        .unwrap();

        let view = db.workflow_trace("request-trace", 100).await.unwrap();

        assert!(
            view.events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert_eq!(view.steps.len(), 1);
        assert_eq!(view.steps[0].status, WorkflowStepStatus::Succeeded);
        assert_eq!(view.events.len(), 4);
        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::NonTerminalState)
        );

        let serialized = serde_json::to_string(&view).unwrap();
        assert!(!serialized.contains("secret-token-value"));
        assert!(!serialized.contains("raw prompt body"));
    }

    #[tokio::test]
    async fn trace_loader_reports_a_gap_and_bounds_the_newest_events() {
        let db = temp_db().await;
        seed_request(&db, "request-gap").await;
        let run_id = db
            .ensure_workflow_run("request-gap", None, "")
            .await
            .unwrap();
        for (from, to) in [
            (WorkflowState::Submitted, WorkflowState::Assessing),
            (WorkflowState::Assessing, WorkflowState::Planned),
            (WorkflowState::Planned, WorkflowState::PlanReview),
        ] {
            db.transition_workflow(&run_id, from, to, "advance")
                .await
                .unwrap();
        }
        let (first_sequence,): (i64,) = sqlx::query_as::query_as(
            "SELECT MIN(sequence) FROM events WHERE subject_id=?1 AND kind='run.status_changed'",
        )
        .bind(&run_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        sqlx::query::query("DELETE FROM events WHERE sequence=?1")
            .bind(first_sequence)
            .execute(&db.pool)
            .await
            .unwrap();

        let view = db.workflow_trace("request-gap", 10).await.unwrap();
        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::MissingTransition)
        );

        let bounded = db.workflow_trace("request-gap", 1).await.unwrap();
        assert!(bounded.truncated);
        assert_eq!(bounded.event_limit, 1);
        assert_eq!(bounded.events.len(), 1);
        assert!(
            bounded
                .alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::TraceTruncated)
        );
    }

    #[tokio::test]
    async fn malformed_transition_rows_are_flagged() {
        let db = temp_db().await;
        seed_request(&db, "request-malformed").await;
        let run_id = db
            .ensure_workflow_run("request-malformed", None, "")
            .await
            .unwrap();
        let now = Utc::now().to_rfc3339();
        for detail in [
            "{broken",
            r#"{"status":"RUNNING"}"#,
            r#"{"from":"NOT_A_STATE","to":"ASSESSING"}"#,
        ] {
            sqlx::query::query(
                "INSERT INTO events(subject_id,kind,detail,recorded_at)
                 VALUES (?1,'run.status_changed',?2,?3)",
            )
            .bind(&run_id)
            .bind(detail)
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        }

        let view = db.workflow_trace("request-malformed", 100).await.unwrap();

        assert!(
            view.alerts
                .iter()
                .filter(|alert| alert.kind == TraceAlertKind::MalformedEvent)
                .count()
                >= 3
        );
        assert!(view.replay_error.is_some());
        assert!(
            view.alerts
                .iter()
                .any(|alert| alert.kind == TraceAlertKind::ReplayRejected)
        );
    }

    #[tokio::test]
    async fn trace_loader_returns_empty_view_without_a_run() {
        let db = temp_db().await;
        seed_request(&db, "request-no-run").await;

        let view = db.workflow_trace("request-no-run", 50).await.unwrap();

        assert!(view.run.is_none());
        assert!(view.events.is_empty());
        assert!(view.alerts.is_empty());
        assert_eq!(view.event_limit, 50);
    }
}
