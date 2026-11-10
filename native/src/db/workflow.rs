//! Database operations for the durable workflow and outbox.
//!
//! All writes are serialized through `Database::write_lock`. A workflow
//! transition is fenced by both the observed source state and the stored
//! revision, so a stale caller cannot skip or overwrite another transition.
//! Steps can only be finished once; checkpoints and outbox intents are
//! content-deduplicated; idempotency keys return the prior result on replay.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

use crate::planning::{WorkflowState, WorkflowStepStatus};
use crate::workflow::{
    self, IDEMPOTENCY_RETENTION_DAYS, MAX_DETAIL_CHARS, MAX_IDENTIFIER_CHARS, MAX_KIND_CHARS,
    OutboxRecord, OutboxStatus, WorkflowCheckpointRecord, WorkflowError, WorkflowRunRecord,
    WorkflowStepRecord,
};

impl From<WorkflowError> for Error {
    fn from(error: WorkflowError) -> Self {
        match error {
            WorkflowError::Invalid(message) => Self::Validation(message),
            WorkflowError::IllegalTransition { .. } => {
                Self::Conflict("illegal workflow transition")
            }
        }
    }
}

fn bounded_u32(field: &str, value: i64) -> Result<u32, Error> {
    u32::try_from(value)
        .map_err(|_| Error::Validation(format!("{field} is outside the supported range")))
}

fn run_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<WorkflowRunRecord, Error> {
    let state: String = row.try_get("state")?;
    Ok(WorkflowRunRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        campaign_id: row.try_get("campaign_id")?,
        state: workflow::parse_workflow_state(&state)?,
        revision: bounded_u32("workflow revision", row.try_get("revision")?)?,
        plan_sha256: row.try_get("plan_sha256")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn step_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<WorkflowStepRecord, Error> {
    let status: String = row.try_get("status")?;
    Ok(WorkflowStepRecord {
        id: row.try_get("id")?,
        run_id: row.try_get("run_id")?,
        kind: row.try_get("kind")?,
        status: workflow::parse_workflow_step_status(&status)?,
        attempt: bounded_u32("workflow step attempt", row.try_get("attempt")?)?,
        input_sha256: row.try_get("input_sha256")?,
        output_sha256: row.try_get("output_sha256")?,
        detail: row.try_get("detail")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn checkpoint_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<WorkflowCheckpointRecord, Error> {
    Ok(WorkflowCheckpointRecord {
        id: row.try_get("id")?,
        run_id: row.try_get("run_id")?,
        unit_id: row.try_get("unit_id")?,
        state_json: row.try_get("state_json")?,
        state_sha256: row.try_get("state_sha256")?,
        created_at: row.try_get("created_at")?,
    })
}

fn outbox_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<OutboxRecord, Error> {
    let status: String = row.try_get("status")?;
    Ok(OutboxRecord {
        id: row.try_get("id")?,
        kind: row.try_get("kind")?,
        payload_json: row.try_get("payload_json")?,
        payload_sha256: row.try_get("payload_sha256")?,
        status: workflow::parse_outbox_status(&status)?,
        attempts: bounded_u32("outbox attempts", row.try_get("attempts")?)?,
        last_error: row.try_get("last_error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_run(
    connection: &mut sqlx_sqlite::SqliteConnection,
    run_id: &str,
) -> Result<WorkflowRunRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,campaign_id,state,revision,plan_sha256,created_at,updated_at
         FROM workflow_runs WHERE id=?1",
    )
    .bind(run_id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    run_from_row(&row)
}

async fn load_step(
    connection: &mut sqlx_sqlite::SqliteConnection,
    step_id: &str,
) -> Result<WorkflowStepRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,run_id,kind,status,attempt,input_sha256,output_sha256,detail,
                created_at,updated_at
         FROM workflow_steps WHERE id=?1",
    )
    .bind(step_id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    step_from_row(&row)
}

async fn load_checkpoint(
    connection: &mut sqlx_sqlite::SqliteConnection,
    checkpoint_id: &str,
) -> Result<WorkflowCheckpointRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,run_id,unit_id,state_json,state_sha256,created_at
         FROM workflow_checkpoints WHERE id=?1",
    )
    .bind(checkpoint_id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    checkpoint_from_row(&row)
}

async fn load_outbox(
    connection: &mut sqlx_sqlite::SqliteConnection,
    outbox_id: &str,
) -> Result<OutboxRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,kind,payload_json,payload_sha256,status,attempts,last_error,
                created_at,updated_at
         FROM outbox WHERE id=?1",
    )
    .bind(outbox_id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    outbox_from_row(&row)
}

impl Database {
    /// Ensure exactly one workflow run is bound to a request. Repeating the call
    /// with the same identity returns the existing run. A later non-empty
    /// campaign or plan hash may fill an empty binding, but a conflicting
    /// non-empty value is rejected rather than silently overwritten.
    pub async fn ensure_workflow_run(
        &self,
        request_id: &str,
        campaign_id: Option<&str>,
        plan_sha256: &str,
    ) -> Result<String, Error> {
        workflow::validate_identifier("request id", request_id)?;
        workflow::validate_optional_identifier("campaign id", campaign_id)?;
        workflow::validate_sha256("plan sha256", plan_sha256, true)?;

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String, Option<String>, String)> = sqlx::query_as::query_as(
            "SELECT id,campaign_id,plan_sha256 FROM workflow_runs
             WHERE request_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let now = Utc::now().to_rfc3339();

        if let Some((run_id, stored_campaign, stored_plan)) = existing {
            let next_campaign = match (stored_campaign.as_deref(), campaign_id) {
                (Some(stored), Some(provided)) if stored != provided => {
                    return Err(Error::Conflict(
                        "workflow campaign binding conflicts with the existing run",
                    ));
                }
                (Some(stored), _) => Some(stored.to_string()),
                (None, provided) => provided.map(str::to_string),
            };
            let next_plan = match (stored_plan.as_str(), plan_sha256) {
                (stored, provided)
                    if !stored.is_empty() && !provided.is_empty() && stored != provided =>
                {
                    return Err(Error::Conflict(
                        "workflow plan hash conflicts with the existing run",
                    ));
                }
                (stored, provided) if stored.is_empty() && !provided.is_empty() => {
                    provided.to_string()
                }
                (stored, _) => stored.to_string(),
            };
            if next_campaign.as_deref() != stored_campaign.as_deref() || next_plan != stored_plan {
                sqlx::query::query(
                    "UPDATE workflow_runs SET campaign_id=?1,plan_sha256=?2,updated_at=?3
                     WHERE id=?4",
                )
                .bind(next_campaign.as_deref())
                .bind(&next_plan)
                .bind(&now)
                .bind(&run_id)
                .execute(&mut *transaction)
                .await?;
            }
            transaction.commit().await?;
            return Ok(run_id);
        }

        let run_id = format!("workflow-{}", Uuid::new_v4());
        sqlx::query::query(
            "INSERT INTO workflow_runs
             (id,request_id,campaign_id,state,revision,plan_sha256,created_at,updated_at)
             VALUES (?1,?2,?3,'SUBMITTED',1,?4,?5,?5)",
        )
        .bind(&run_id)
        .bind(request_id)
        .bind(campaign_id)
        .bind(plan_sha256)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        super::record_event(
            &mut transaction,
            &run_id,
            "run.created",
            &serde_json::json!({"state": "SUBMITTED"}).to_string(),
        )
        .await?;
        transaction.commit().await?;
        Ok(run_id)
    }

    /// Load a workflow run by id.
    pub async fn workflow_run(&self, run_id: &str) -> Result<WorkflowRunRecord, Error> {
        workflow::validate_identifier("run id", run_id)?;
        let row = sqlx::query::query(
            "SELECT id,request_id,campaign_id,state,revision,plan_sha256,created_at,updated_at
             FROM workflow_runs WHERE id=?1",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        run_from_row(&row)
    }

    /// Load the latest workflow run bound to a request.
    pub async fn workflow_run_for_request(
        &self,
        request_id: &str,
    ) -> Result<Option<WorkflowRunRecord>, Error> {
        workflow::validate_identifier("request id", request_id)?;
        let row = sqlx::query::query(
            "SELECT id,request_id,campaign_id,state,revision,plan_sha256,created_at,updated_at
             FROM workflow_runs WHERE request_id=?1
             ORDER BY created_at DESC,id DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(run_from_row).transpose()
    }

    /// Apply one deterministic transition. The caller must present the exact
    /// source state it observed; the update is additionally fenced by the
    /// stored revision. The returned value is the new revision.
    pub async fn transition_workflow(
        &self,
        run_id: &str,
        from: WorkflowState,
        to: WorkflowState,
        detail: &str,
    ) -> Result<u32, Error> {
        workflow::validate_identifier("run id", run_id)?;
        workflow::validate_transition(from, to)?;
        workflow::validate_text("transition detail", detail, MAX_DETAIL_CHARS, true)?;

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let run = load_run(&mut transaction, run_id).await?;
        if run.state != from {
            return Err(Error::Conflict(
                "workflow state does not match the expected source state",
            ));
        }
        if to == WorkflowState::Accepted {
            let (running_steps,): (i64,) = sqlx::query_as::query_as(
                "SELECT COUNT(*) FROM workflow_steps WHERE run_id=?1 AND status='RUNNING'",
            )
            .bind(run_id)
            .fetch_one(&mut *transaction)
            .await?;
            if running_steps != 0 {
                return Err(Error::Conflict(
                    "cannot accept a workflow with running steps",
                ));
            }
        }
        let next_revision = run
            .revision
            .checked_add(1)
            .ok_or(Error::Conflict("workflow revision overflow"))?;
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query::query(
            "UPDATE workflow_runs SET state=?1,revision=?2,updated_at=?3
             WHERE id=?4 AND state=?5 AND revision=?6",
        )
        .bind(workflow::workflow_state_code(to))
        .bind(i64::from(next_revision))
        .bind(&now)
        .bind(run_id)
        .bind(workflow::workflow_state_code(from))
        .bind(i64::from(run.revision))
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(Error::Conflict(
                "workflow revision was fenced by another transition",
            ));
        }
        let detail_json = serde_json::json!({
            "workflow": true,
            "from": workflow::workflow_state_code(from),
            "to": workflow::workflow_state_code(to),
            "revision": next_revision,
            "detail": detail,
        })
        .to_string();
        super::record_event(&mut transaction, run_id, "run.status_changed", &detail_json).await?;
        transaction.commit().await?;
        Ok(next_revision)
    }

    /// Start a new attempt of a step. The step is immediately durable as
    /// `RUNNING`; a crash leaves it recoverable by `recover_running_steps`.
    pub async fn begin_step(
        &self,
        run_id: &str,
        kind: &str,
        input_sha256: &str,
    ) -> Result<WorkflowStepRecord, Error> {
        workflow::validate_identifier("run id", run_id)?;
        workflow::validate_text("step kind", kind, MAX_KIND_CHARS, false)?;
        workflow::validate_sha256("step input sha256", input_sha256, true)?;

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let run = load_run(&mut transaction, run_id).await?;
        if run.state.is_terminal() {
            return Err(Error::Conflict(
                "cannot begin a workflow step on a terminal run",
            ));
        }
        let (max_attempt,): (i64,) = sqlx::query_as::query_as(
            "SELECT COALESCE(MAX(attempt),0) FROM workflow_steps WHERE run_id=?1 AND kind=?2",
        )
        .bind(run_id)
        .bind(kind)
        .fetch_one(&mut *transaction)
        .await?;
        let next_attempt = max_attempt
            .checked_add(1)
            .ok_or(Error::Conflict("workflow step attempt overflow"))?;
        let attempt = bounded_u32("workflow step attempt", next_attempt)?;
        let step_id = format!("step-{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO workflow_steps
             (id,run_id,kind,status,attempt,input_sha256,output_sha256,detail,
              created_at,updated_at)
             VALUES (?1,?2,?3,'RUNNING',?4,?5,'','',?6,?6)",
        )
        .bind(&step_id)
        .bind(run_id)
        .bind(kind)
        .bind(i64::from(attempt))
        .bind(input_sha256)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let step = load_step(&mut transaction, &step_id).await?;
        super::record_event(
            &mut transaction,
            run_id,
            "workflow.step.started",
            &serde_json::json!({
                "stepId": step.id,
                "stepKind": step.kind,
                "attempt": step.attempt,
                "status": "RUNNING",
            })
            .to_string(),
        )
        .await?;
        transaction.commit().await?;
        Ok(step)
    }

    /// Finish a running step exactly once. `SUCCEEDED`, `FAILED` and `BLOCKED`
    /// are the only legal terminal statuses; replaying a finish is rejected so
    /// a duplicate handler cannot overwrite a durable outcome.
    pub async fn finish_step(
        &self,
        step_id: &str,
        status: WorkflowStepStatus,
        output_sha256: &str,
        detail: &str,
    ) -> Result<WorkflowStepRecord, Error> {
        workflow::validate_identifier("step id", step_id)?;
        workflow::validate_sha256("step output sha256", output_sha256, true)?;
        workflow::validate_text("step detail", detail, MAX_DETAIL_CHARS, true)?;
        if !matches!(
            status,
            WorkflowStepStatus::Succeeded
                | WorkflowStepStatus::Failed
                | WorkflowStepStatus::Blocked
        ) {
            return Err(Error::Validation(
                "workflow step must finish in SUCCEEDED, FAILED or BLOCKED".into(),
            ));
        }

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let step = load_step(&mut transaction, step_id).await?;
        if step.status != WorkflowStepStatus::Running {
            return Err(Error::Conflict("workflow step is not running"));
        }
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query::query(
            "UPDATE workflow_steps
             SET status=?1,output_sha256=?2,detail=?3,updated_at=?4
             WHERE id=?5 AND status='RUNNING'",
        )
        .bind(workflow::workflow_step_status_code(status))
        .bind(output_sha256)
        .bind(detail)
        .bind(&now)
        .bind(step_id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(Error::Conflict("workflow step was already finished"));
        }
        let step = load_step(&mut transaction, step_id).await?;
        let event_kind = match status {
            WorkflowStepStatus::Succeeded => "workflow.step.succeeded",
            WorkflowStepStatus::Failed => "workflow.step.failed",
            WorkflowStepStatus::Blocked => "workflow.step.blocked",
            WorkflowStepStatus::Pending | WorkflowStepStatus::Running => {
                return Err(Error::Validation(
                    "workflow step must finish in SUCCEEDED, FAILED or BLOCKED".into(),
                ));
            }
        };
        super::record_event(
            &mut transaction,
            &step.run_id,
            event_kind,
            &serde_json::json!({
                "stepId": step.id,
                "stepKind": step.kind,
                "attempt": step.attempt,
                "status": workflow::workflow_step_status_code(status),
            })
            .to_string(),
        )
        .await?;
        transaction.commit().await?;
        Ok(step)
    }

    /// Persist a checkpoint with a content hash. Repeating the same run/unit
    /// and state returns the existing checkpoint instead of creating a
    /// duplicate record.
    pub async fn save_checkpoint(
        &self,
        run_id: &str,
        unit_id: &str,
        state_json: &str,
    ) -> Result<WorkflowCheckpointRecord, Error> {
        workflow::validate_identifier("run id", run_id)?;
        workflow::validate_text("checkpoint unit id", unit_id, MAX_IDENTIFIER_CHARS, true)?;
        workflow::validate_json_payload("checkpoint state", state_json)?;
        let state_sha256 = workflow::sha256_hex(state_json.as_bytes());

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let run: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM workflow_runs WHERE id=?1")
                .bind(run_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if run.is_none() {
            return Err(Error::NotFound);
        }
        let existing: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT id FROM workflow_checkpoints
             WHERE run_id=?1 AND unit_id=?2 AND state_sha256=?3
             ORDER BY created_at,id LIMIT 1",
        )
        .bind(run_id)
        .bind(unit_id)
        .bind(&state_sha256)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((checkpoint_id,)) = existing {
            let checkpoint = load_checkpoint(&mut transaction, &checkpoint_id).await?;
            transaction.commit().await?;
            return Ok(checkpoint);
        }
        let checkpoint_id = format!("checkpoint-{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO workflow_checkpoints
             (id,run_id,unit_id,state_json,state_sha256,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(&checkpoint_id)
        .bind(run_id)
        .bind(unit_id)
        .bind(state_json)
        .bind(&state_sha256)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        super::record_event(
            &mut transaction,
            run_id,
            "workflow.checkpoint.recorded",
            &serde_json::json!({"unitId": unit_id, "sha256": state_sha256}).to_string(),
        )
        .await?;
        let checkpoint = load_checkpoint(&mut transaction, &checkpoint_id).await?;
        transaction.commit().await?;
        Ok(checkpoint)
    }

    /// Record an idempotent operation result. The first call stores the result
    /// and returns it; a replay with the same operation and payload returns the
    /// prior result. A different operation or payload is a conflict.
    pub async fn idempotent(
        &self,
        key: &str,
        operation: &str,
        result_json: &str,
    ) -> Result<String, Error> {
        workflow::validate_text("idempotency key", key, MAX_IDENTIFIER_CHARS, false)?;
        workflow::validate_text("idempotency operation", operation, MAX_KIND_CHARS, false)?;
        workflow::validate_json_payload("idempotency result", result_json)?;

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT operation,result_json FROM idempotency_keys WHERE key=?1",
        )
        .bind(key)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((stored_operation, stored_result)) = existing {
            if stored_operation != operation {
                return Err(Error::Conflict(
                    "idempotency key reused for another operation",
                ));
            }
            if stored_result != result_json {
                return Err(Error::Conflict(
                    "idempotency key reused with a different result payload",
                ));
            }
            transaction.commit().await?;
            return Ok(stored_result);
        }
        let now = Utc::now();
        let expires_at = (now + chrono::Duration::days(IDEMPOTENCY_RETENTION_DAYS)).to_rfc3339();
        sqlx::query::query(
            "INSERT INTO idempotency_keys(key,operation,result_json,created_at,expires_at)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(key)
        .bind(operation)
        .bind(result_json)
        .bind(now.to_rfc3339())
        .bind(&expires_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(result_json.to_string())
    }

    /// Enqueue an external projection intent. The same kind and payload hash
    /// returns the existing row, so a replay cannot create a duplicate effect.
    pub async fn enqueue_outbox(&self, kind: &str, payload_json: &str) -> Result<String, Error> {
        workflow::validate_text("outbox kind", kind, MAX_KIND_CHARS, false)?;
        workflow::validate_json_payload("outbox payload", payload_json)?;
        let payload_sha256 = workflow::sha256_hex(payload_json.as_bytes());

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT id FROM outbox WHERE kind=?1 AND payload_sha256=?2
             ORDER BY created_at,id LIMIT 1",
        )
        .bind(kind)
        .bind(&payload_sha256)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((outbox_id,)) = existing {
            transaction.commit().await?;
            return Ok(outbox_id);
        }
        let outbox_id = format!("outbox-{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO outbox
             (id,kind,payload_json,payload_sha256,status,attempts,last_error,
              created_at,updated_at)
             VALUES (?1,?2,?3,?4,'PENDING',0,'',?5,?5)",
        )
        .bind(&outbox_id)
        .bind(kind)
        .bind(payload_json)
        .bind(&payload_sha256)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(outbox_id)
    }

    /// Load an outbox row by id.
    pub async fn outbox_record(&self, outbox_id: &str) -> Result<OutboxRecord, Error> {
        workflow::validate_identifier("outbox id", outbox_id)?;
        let row = sqlx::query::query(
            "SELECT id,kind,payload_json,payload_sha256,status,attempts,last_error,
                    created_at,updated_at
             FROM outbox WHERE id=?1",
        )
        .bind(outbox_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        outbox_from_row(&row)
    }

    /// Mark an outbox intent delivered. Repeating the call is safe; a
    /// reconciled row is already beyond delivery and is returned unchanged.
    pub async fn mark_outbox_sent(&self, outbox_id: &str) -> Result<OutboxRecord, Error> {
        workflow::validate_identifier("outbox id", outbox_id)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let record = load_outbox(&mut transaction, outbox_id).await?;
        match record.status {
            OutboxStatus::Sent | OutboxStatus::Reconciled => {
                transaction.commit().await?;
                return Ok(record);
            }
            OutboxStatus::Pending | OutboxStatus::Failed => {
                sqlx::query::query("UPDATE outbox SET status='SENT',updated_at=?1 WHERE id=?2")
                    .bind(Utc::now().to_rfc3339())
                    .bind(outbox_id)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        let record = load_outbox(&mut transaction, outbox_id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Record a delivery failure and increment the attempt counter. A sent or
    /// reconciled row cannot be moved back to failed.
    pub async fn mark_outbox_failed(
        &self,
        outbox_id: &str,
        error: &str,
    ) -> Result<OutboxRecord, Error> {
        workflow::validate_identifier("outbox id", outbox_id)?;
        workflow::validate_text("outbox error", error, MAX_DETAIL_CHARS, false)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let record = load_outbox(&mut transaction, outbox_id).await?;
        match record.status {
            OutboxStatus::Sent | OutboxStatus::Reconciled => {
                return Err(Error::Conflict(
                    "a sent or reconciled outbox entry cannot be marked failed",
                ));
            }
            OutboxStatus::Pending | OutboxStatus::Failed => {
                let attempts = record
                    .attempts
                    .checked_add(1)
                    .ok_or(Error::Conflict("outbox attempt overflow"))?;
                sqlx::query::query(
                    "UPDATE outbox SET status='FAILED',attempts=?1,last_error=?2,updated_at=?3
                     WHERE id=?4",
                )
                .bind(i64::from(attempts))
                .bind(error)
                .bind(Utc::now().to_rfc3339())
                .bind(outbox_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
        let record = load_outbox(&mut transaction, outbox_id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Mark an outbox intent reconciled against the external system. Repeating
    /// the call is safe.
    pub async fn mark_outbox_reconciled(&self, outbox_id: &str) -> Result<OutboxRecord, Error> {
        workflow::validate_identifier("outbox id", outbox_id)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let record = load_outbox(&mut transaction, outbox_id).await?;
        if record.status == OutboxStatus::Reconciled {
            transaction.commit().await?;
            return Ok(record);
        }
        sqlx::query::query("UPDATE outbox SET status='RECONCILED',updated_at=?1 WHERE id=?2")
            .bind(Utc::now().to_rfc3339())
            .bind(outbox_id)
            .execute(&mut *transaction)
            .await?;
        let record = load_outbox(&mut transaction, outbox_id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Return every step left `RUNNING` by a crash or timeout. The caller must
    /// reconcile the external effect before retrying or finishing the step.
    pub async fn recover_running_steps(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowStepRecord>, Error> {
        workflow::validate_identifier("run id", run_id)?;
        let run: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM workflow_runs WHERE id=?1")
                .bind(run_id)
                .fetch_optional(&self.pool)
                .await?;
        if run.is_none() {
            return Err(Error::NotFound);
        }
        let rows = sqlx::query::query(
            "SELECT id,run_id,kind,status,attempt,input_sha256,output_sha256,detail,
                    created_at,updated_at
             FROM workflow_steps
             WHERE run_id=?1 AND status='RUNNING'
             ORDER BY created_at,id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(step_from_row).collect()
    }

    /// Reclaim steps left RUNNING by a crashed process. The caller has
    /// reconciled external effects first; this marks the old attempt BLOCKED so
    /// a new fenced attempt can begin. It never guesses success.
    pub async fn reclaim_running_steps(
        &self,
        run_id: &str,
        detail: &str,
    ) -> Result<Vec<WorkflowStepRecord>, Error> {
        workflow::validate_identifier("run id", run_id)?;
        workflow::validate_text("reclaim detail", detail, MAX_DETAIL_CHARS, false)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let run = load_run(&mut transaction, run_id).await?;
        if run.state.is_terminal() {
            return Err(Error::Conflict(
                "cannot reclaim steps on a terminal workflow",
            ));
        }
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query::query(
            "UPDATE workflow_steps
             SET status='BLOCKED',detail=?1,updated_at=?2
             WHERE run_id=?3 AND status='RUNNING'",
        )
        .bind(detail)
        .bind(&now)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            transaction.commit().await?;
            return Ok(Vec::new());
        }
        let rows = sqlx::query::query(
            "SELECT id,run_id,kind,status,attempt,input_sha256,output_sha256,detail,
                    created_at,updated_at
             FROM workflow_steps
             WHERE run_id=?1 AND status='BLOCKED' AND updated_at=?2
             ORDER BY created_at,id",
        )
        .bind(run_id)
        .bind(&now)
        .fetch_all(&mut *transaction)
        .await?;
        let steps = rows
            .iter()
            .map(step_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        for step in &steps {
            super::record_event(
                &mut transaction,
                run_id,
                "workflow.step.blocked",
                &serde_json::json!({
                    "stepId": step.id,
                    "stepKind": step.kind,
                    "attempt": step.attempt,
                    "status": "BLOCKED",
                })
                .to_string(),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(steps)
    }

    /// Load the durable step history for a run in creation order.
    pub async fn workflow_steps(&self, run_id: &str) -> Result<Vec<WorkflowStepRecord>, Error> {
        workflow::validate_identifier("run id", run_id)?;
        let rows = sqlx::query::query(
            "SELECT id,run_id,kind,status,attempt,input_sha256,output_sha256,detail,
                    created_at,updated_at
             FROM workflow_steps WHERE run_id=?1 ORDER BY created_at,id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(step_from_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::workflow::{MAX_JSON_BYTES, parse_outbox_status, sha256_hex, validate_transition};

    async fn temp_db() -> Database {
        let path = temp_path();
        Database::open(&path).await.unwrap()
    }

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("factory-workflow-{}.sqlite", Uuid::new_v4()))
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

    async fn seeded_db(request_id: &str) -> Database {
        let db = temp_db().await;
        seed_request(&db, request_id).await;
        db
    }

    #[tokio::test]
    async fn workflow_transitions_are_validated_fenced_and_terminal() {
        let db = seeded_db("request-transition").await;
        let run_id = db
            .ensure_workflow_run("request-transition", None, "")
            .await
            .unwrap();

        // A forward skip is illegal and leaves the durable row untouched.
        assert!(
            db.transition_workflow(
                &run_id,
                WorkflowState::Submitted,
                WorkflowState::Frozen,
                "skip",
            )
            .await
            .is_err()
        );
        let run = db.workflow_run(&run_id).await.unwrap();
        assert_eq!(run.state, WorkflowState::Submitted);
        assert_eq!(run.revision, 1);

        db.transition_workflow(
            &run_id,
            WorkflowState::Submitted,
            WorkflowState::Assessing,
            "assess",
        )
        .await
        .unwrap();

        // Replaying the old source state is fenced by the durable revision.
        assert!(
            db.transition_workflow(
                &run_id,
                WorkflowState::Submitted,
                WorkflowState::Assessing,
                "stale",
            )
            .await
            .is_err()
        );
        assert!(
            db.transition_workflow(
                &run_id,
                WorkflowState::Assessing,
                WorkflowState::Executing,
                "skip",
            )
            .await
            .is_err()
        );

        let path = [
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
            db.transition_workflow(&run_id, from, to, "advance")
                .await
                .unwrap();
        }
        let run = db.workflow_run(&run_id).await.unwrap();
        assert_eq!(run.state, WorkflowState::Accepted);
        assert_eq!(run.revision, 11);
        assert!(run.state.is_terminal());

        // Terminal states are immutable even to another terminal state.
        assert!(
            db.transition_workflow(
                &run_id,
                WorkflowState::Accepted,
                WorkflowState::Cancelled,
                "late cancel",
            )
            .await
            .is_err()
        );
        let run = db.workflow_run(&run_id).await.unwrap();
        assert_eq!(run.state, WorkflowState::Accepted);
        assert_eq!(run.revision, 11);

        // The durable event ledger records the transitions, including detail.
        let events = db.events().await.unwrap();
        let transition_events: Vec<_> = events
            .iter()
            .filter(|event| event.kind == "run.status_changed" && event.subject_id == run_id)
            .collect();
        assert_eq!(transition_events.len(), 10);
        assert!(
            transition_events
                .iter()
                .any(|event| event.detail.contains("assess"))
        );
    }

    #[tokio::test]
    async fn ensure_workflow_run_is_idempotent_and_rejects_conflicts() {
        let db = seeded_db("request-ensure").await;
        let plan = "a".repeat(64);
        let first = db
            .ensure_workflow_run("request-ensure", None, "")
            .await
            .unwrap();
        let second = db
            .ensure_workflow_run("request-ensure", Some("campaign"), &plan)
            .await
            .unwrap();
        assert_eq!(first, second);

        let run = db.workflow_run(&first).await.unwrap();
        assert_eq!(run.request_id, "request-ensure");
        assert_eq!(run.campaign_id.as_deref(), Some("campaign"));
        assert_eq!(run.plan_sha256, plan);
        assert_eq!(run.state, WorkflowState::Submitted);
        assert_eq!(run.revision, 1);

        // Identical replay is safe.
        assert_eq!(
            db.ensure_workflow_run("request-ensure", Some("campaign"), &plan)
                .await
                .unwrap(),
            first
        );
        // A different non-empty binding is not silently overwritten.
        assert!(
            db.ensure_workflow_run("request-ensure", Some("other-campaign"), &plan)
                .await
                .is_err()
        );
        assert!(
            db.ensure_workflow_run("request-ensure", Some("campaign"), &"b".repeat(64))
                .await
                .is_err()
        );

        assert!(db.ensure_workflow_run("", None, "").await.is_err());
        assert!(
            db.ensure_workflow_run("request-ensure", Some(""), "")
                .await
                .is_err()
        );
        assert!(
            db.ensure_workflow_run("request-ensure", None, "not-a-hash")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn workflow_state_and_effects_survive_reopen() {
        let path = temp_path();
        let db = Database::open(&path).await.unwrap();
        seed_request(&db, "request-restart").await;
        let plan = "a".repeat(64);
        let run_id = db
            .ensure_workflow_run("request-restart", Some("campaign"), &plan)
            .await
            .unwrap();
        db.transition_workflow(
            &run_id,
            WorkflowState::Submitted,
            WorkflowState::Assessing,
            "assess",
        )
        .await
        .unwrap();
        let step = db.begin_step(&run_id, "unit.execute", "").await.unwrap();
        let checkpoint = db
            .save_checkpoint(&run_id, "unit-1", r#"{"done":false}"#)
            .await
            .unwrap();
        let outbox_id = db
            .enqueue_outbox("github.issue.create", r#"{"number":1}"#)
            .await
            .unwrap();
        let idempotent_result = db
            .idempotent("key-restart", "issue.create", r#"{"number":1}"#)
            .await
            .unwrap();
        db.close().await;

        let reopened = Database::open(&path).await.unwrap();
        assert_eq!(
            reopened
                .ensure_workflow_run("request-restart", Some("campaign"), &plan)
                .await
                .unwrap(),
            run_id
        );
        let run = reopened.workflow_run(&run_id).await.unwrap();
        assert_eq!(run.state, WorkflowState::Assessing);
        assert_eq!(run.revision, 2);

        let running = reopened.recover_running_steps(&run_id).await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, step.id);
        assert_eq!(
            reopened
                .save_checkpoint(&run_id, "unit-1", r#"{"done":false}"#)
                .await
                .unwrap()
                .id,
            checkpoint.id
        );
        assert_eq!(
            reopened.outbox_record(&outbox_id).await.unwrap().status,
            OutboxStatus::Pending
        );
        assert_eq!(
            reopened
                .idempotent("key-restart", "issue.create", r#"{"number":1}"#)
                .await
                .unwrap(),
            idempotent_result
        );
        reopened
            .transition_workflow(
                &run_id,
                WorkflowState::Assessing,
                WorkflowState::Planned,
                "plan",
            )
            .await
            .unwrap();
        assert_eq!(reopened.workflow_run(&run_id).await.unwrap().revision, 3);
        reopened.close().await;
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[tokio::test]
    async fn idempotency_replay_returns_prior_result_and_conflicts_are_rejected() {
        let db = seeded_db("request-idempotency").await;
        let first = db
            .idempotent("key-1", "github.issue.create", r#"{"number":7}"#)
            .await
            .unwrap();
        let replay = db
            .idempotent("key-1", "github.issue.create", r#"{"number":7}"#)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(first, r#"{"number":7}"#);

        assert!(
            db.idempotent("key-1", "github.issue.update", r#"{"number":7}"#)
                .await
                .is_err()
        );
        assert!(
            db.idempotent("key-1", "github.issue.create", r#"{"number":8}"#)
                .await
                .is_err()
        );
        let (count,): (i64,) = sqlx::query_as::query_as("SELECT COUNT(*) FROM idempotency_keys")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);

        assert!(db.idempotent("", "operation", "{}").await.is_err());
        assert!(db.idempotent("key-2", "", "{}").await.is_err());
        assert!(
            db.idempotent("key-2", "operation", "not-json")
                .await
                .is_err()
        );
        assert!(
            db.idempotent(
                "key-2",
                "operation",
                &format!("\"{}\"", "x".repeat(MAX_JSON_BYTES)),
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn outbox_lifecycle_is_replay_safe() {
        let db = seeded_db("request-outbox").await;
        let payload = r#"{"repository":"owner/repo","number":1}"#;
        let outbox_id = db
            .enqueue_outbox("github.issue.create", payload)
            .await
            .unwrap();
        // The same intent is deduplicated by kind and payload hash.
        assert_eq!(
            db.enqueue_outbox("github.issue.create", payload)
                .await
                .unwrap(),
            outbox_id
        );

        let record = db.outbox_record(&outbox_id).await.unwrap();
        assert_eq!(record.status, OutboxStatus::Pending);
        assert_eq!(record.attempts, 0);
        assert_eq!(record.payload_sha256, sha256_hex(payload.as_bytes()));

        let sent = db.mark_outbox_sent(&outbox_id).await.unwrap();
        assert_eq!(sent.status, OutboxStatus::Sent);
        let sent_again = db.mark_outbox_sent(&outbox_id).await.unwrap();
        assert_eq!(sent_again.status, OutboxStatus::Sent);

        let reconciled = db.mark_outbox_reconciled(&outbox_id).await.unwrap();
        assert_eq!(reconciled.status, OutboxStatus::Reconciled);
        let reconciled_again = db.mark_outbox_reconciled(&outbox_id).await.unwrap();
        assert_eq!(reconciled_again.status, OutboxStatus::Reconciled);
        // A reconciled entry is already beyond delivery.
        assert_eq!(
            db.mark_outbox_sent(&outbox_id).await.unwrap().status,
            OutboxStatus::Reconciled
        );
        assert!(db.mark_outbox_failed(&outbox_id, "late").await.is_err());

        let failed_id = db
            .enqueue_outbox("openspec.archive", r#"{"change":"c1"}"#)
            .await
            .unwrap();
        let failed = db
            .mark_outbox_failed(&failed_id, "rate limited")
            .await
            .unwrap();
        assert_eq!(failed.status, OutboxStatus::Failed);
        assert_eq!(failed.attempts, 1);
        assert_eq!(failed.last_error, "rate limited");
        let failed_again = db
            .mark_outbox_failed(&failed_id, "still limited")
            .await
            .unwrap();
        assert_eq!(failed_again.status, OutboxStatus::Failed);
        assert_eq!(failed_again.attempts, 2);
        assert_eq!(failed_again.last_error, "still limited");
        assert_eq!(
            db.mark_outbox_sent(&failed_id).await.unwrap().status,
            OutboxStatus::Sent
        );
        assert_eq!(
            db.mark_outbox_reconciled(&failed_id).await.unwrap().status,
            OutboxStatus::Reconciled
        );

        assert!(db.enqueue_outbox("", "{}").await.is_err());
        assert!(
            db.enqueue_outbox("github.issue.create", "not-json")
                .await
                .is_err()
        );
        assert!(
            db.enqueue_outbox(
                "github.issue.create",
                &format!("\"{}\"", "x".repeat(MAX_JSON_BYTES)),
            )
            .await
            .is_err()
        );
        assert!(db.outbox_record("missing").await.is_err());
    }

    #[tokio::test]
    async fn checkpoint_hash_is_content_addressed_and_replay_safe() {
        let db = seeded_db("request-checkpoint").await;
        let run_id = db
            .ensure_workflow_run("request-checkpoint", None, "")
            .await
            .unwrap();
        let state = r#"{"unit":"unit-1","completed":["a"],"next":"b"}"#;
        let first = db.save_checkpoint(&run_id, "unit-1", state).await.unwrap();
        assert_eq!(first.state_sha256, sha256_hex(state.as_bytes()));
        assert_eq!(first.state_sha256.len(), 64);

        let replay = db.save_checkpoint(&run_id, "unit-1", state).await.unwrap();
        assert_eq!(first.id, replay.id);

        let other = db
            .save_checkpoint(&run_id, "unit-1", r#"{"unit":"unit-1","completed":[]}"#)
            .await
            .unwrap();
        assert_ne!(first.state_sha256, other.state_sha256);
        let (count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM workflow_checkpoints")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(count, 2);

        assert!(
            db.save_checkpoint(&run_id, "unit-1", "not-json")
                .await
                .is_err()
        );
        assert!(
            db.save_checkpoint(
                &run_id,
                "unit-1",
                &format!("\"{}\"", "x".repeat(MAX_JSON_BYTES)),
            )
            .await
            .is_err()
        );
        assert!(
            db.save_checkpoint("missing-run", "unit-1", "{}")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn recovery_returns_only_running_steps_and_finish_is_exactly_once() {
        let db = seeded_db("request-recovery").await;
        let run_id = db
            .ensure_workflow_run("request-recovery", None, "")
            .await
            .unwrap();
        let first = db
            .begin_step(&run_id, "unit.execute", &"a".repeat(64))
            .await
            .unwrap();
        assert_eq!(first.status, WorkflowStepStatus::Running);
        assert_eq!(first.attempt, 1);

        let second = db.begin_step(&run_id, "unit.execute", "").await.unwrap();
        assert_eq!(second.attempt, 2);
        let third = db.begin_step(&run_id, "unit.verify", "").await.unwrap();
        assert_eq!(third.attempt, 1);

        let finished = db
            .finish_step(
                &first.id,
                WorkflowStepStatus::Succeeded,
                &"b".repeat(64),
                "done",
            )
            .await
            .unwrap();
        assert_eq!(finished.status, WorkflowStepStatus::Succeeded);
        assert_eq!(finished.output_sha256, "b".repeat(64));

        let running = db.recover_running_steps(&run_id).await.unwrap();
        assert_eq!(running.len(), 2);
        assert!(
            running
                .iter()
                .all(|step| step.status == WorkflowStepStatus::Running)
        );
        assert!(running.iter().any(|step| step.id == second.id));
        assert!(running.iter().any(|step| step.id == third.id));
        assert!(!running.iter().any(|step| step.id == first.id));

        let reclaimed = db
            .reclaim_running_steps(&run_id, "restart reconciliation")
            .await
            .unwrap();
        assert_eq!(reclaimed.len(), 2);
        assert!(
            reclaimed
                .iter()
                .all(|step| step.status == WorkflowStepStatus::Blocked)
        );
        assert!(db.recover_running_steps(&run_id).await.unwrap().is_empty());

        // A concurrent finisher wins: reclaim must never overwrite a terminal
        // step that was completed before reconciliation.
        let raced = db.begin_step(&run_id, "unit.raced", "").await.unwrap();
        let finished_raced = db
            .finish_step(
                &raced.id,
                WorkflowStepStatus::Succeeded,
                &"c".repeat(64),
                "finished before reclaim",
            )
            .await
            .unwrap();
        assert_eq!(finished_raced.status, WorkflowStepStatus::Succeeded);
        let reclaimed_after_finish = db
            .reclaim_running_steps(&run_id, "late reconciliation")
            .await
            .unwrap();
        assert!(
            reclaimed_after_finish
                .iter()
                .all(|step| step.id != raced.id)
        );
        let raced_after = db
            .workflow_steps(&run_id)
            .await
            .unwrap()
            .into_iter()
            .find(|step| step.id == raced.id)
            .unwrap();
        assert_eq!(raced_after.status, WorkflowStepStatus::Succeeded);

        // A reclaim that wins the race fences the late finisher: the finish
        // update only matches RUNNING and therefore cannot overwrite BLOCKED.
        let reclaimed_first = db
            .begin_step(&run_id, "unit.reclaim-wins", "")
            .await
            .unwrap();
        let reclaimed_now = db
            .reclaim_running_steps(&run_id, "reclaim wins the race")
            .await
            .unwrap();
        assert!(
            reclaimed_now
                .iter()
                .any(|step| step.id == reclaimed_first.id)
        );
        assert!(
            db.finish_step(
                &reclaimed_first.id,
                WorkflowStepStatus::Succeeded,
                &"e".repeat(64),
                "late finisher",
            )
            .await
            .is_err()
        );

        let replacement = db.begin_step(&run_id, "unit.execute", "").await.unwrap();
        assert_eq!(replacement.attempt, 3);
        let replacement_verify = db.begin_step(&run_id, "unit.verify", "").await.unwrap();

        // A step can only be finished once, and only from RUNNING.
        assert!(
            db.finish_step(&first.id, WorkflowStepStatus::Failed, "", "again")
                .await
                .is_err()
        );
        assert!(
            db.finish_step(&second.id, WorkflowStepStatus::Running, "", "")
                .await
                .is_err()
        );
        assert!(
            db.finish_step(&second.id, WorkflowStepStatus::Pending, "", "")
                .await
                .is_err()
        );
        assert!(
            db.finish_step(&second.id, WorkflowStepStatus::Succeeded, "not-a-hash", "")
                .await
                .is_err()
        );
        assert!(db.recover_running_steps("missing-run").await.is_err());

        // A workflow cannot be accepted while a step is still running.
        let path_to_converging = [
            (WorkflowState::Submitted, WorkflowState::Assessing),
            (WorkflowState::Assessing, WorkflowState::Planned),
            (WorkflowState::Planned, WorkflowState::PlanReview),
            (WorkflowState::PlanReview, WorkflowState::Frozen),
            (WorkflowState::Frozen, WorkflowState::Executing),
            (WorkflowState::Executing, WorkflowState::Verifying),
            (WorkflowState::Verifying, WorkflowState::Reviewing),
            (WorkflowState::Reviewing, WorkflowState::Gate),
            (WorkflowState::Gate, WorkflowState::Converging),
        ];
        for (from, to) in path_to_converging {
            db.transition_workflow(&run_id, from, to, "advance")
                .await
                .unwrap();
        }
        assert!(
            db.transition_workflow(
                &run_id,
                WorkflowState::Converging,
                WorkflowState::Accepted,
                "premature",
            )
            .await
            .is_err()
        );

        let blocked = db
            .finish_step(
                &replacement.id,
                WorkflowStepStatus::Blocked,
                "",
                "needs owner",
            )
            .await
            .unwrap();
        assert_eq!(blocked.status, WorkflowStepStatus::Blocked);
        let failed = db
            .finish_step(
                &replacement_verify.id,
                WorkflowStepStatus::Failed,
                "",
                "check failed",
            )
            .await
            .unwrap();
        assert_eq!(failed.status, WorkflowStepStatus::Failed);
        assert!(db.recover_running_steps(&run_id).await.unwrap().is_empty());
        db.transition_workflow(
            &run_id,
            WorkflowState::Converging,
            WorkflowState::Accepted,
            "accept",
        )
        .await
        .unwrap();

        // A terminal workflow cannot start a new step.
        assert!(db.begin_step(&run_id, "unit.execute", "").await.is_err());
    }

    #[tokio::test]
    async fn transition_validation_matches_the_contract() {
        assert!(validate_transition(WorkflowState::Submitted, WorkflowState::Assessing).is_ok());
        assert!(validate_transition(WorkflowState::Submitted, WorkflowState::Accepted).is_err());
        assert!(validate_transition(WorkflowState::Accepted, WorkflowState::Executing).is_err());
        assert!(parse_outbox_status("PENDING").is_ok());
        assert!(parse_outbox_status("UNKNOWN").is_err());
    }
}
