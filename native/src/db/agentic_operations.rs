//! Durable agentic operation records and queue metadata.
//!
//! The record is a Core projection and scheduler input. It does not grant
//! authority; the Campaign/Run/Gate records remain authoritative.

use super::*;
use chrono::Utc;
use sqlx::row::Row;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticOperationRecord {
    pub id: String,
    pub request_id: String,
    pub project_id: String,
    pub director_id: String,
    pub operation_manager_id: String,
    pub priority: i64,
    pub status: String,
    pub worktree_policy: String,
    pub created_at: String,
    pub updated_at: String,
}

fn from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticOperationRecord, Error> {
    Ok(AgenticOperationRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        project_id: row.try_get("project_id")?,
        director_id: row.try_get("director_id")?,
        operation_manager_id: row.try_get("operation_manager_id")?,
        priority: row.try_get("priority")?,
        status: row.try_get("status")?,
        worktree_policy: row.try_get("worktree_policy")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Database {
    pub async fn agentic_operation_for_request(
        &self,
        request_id: &str,
    ) -> Result<AgenticOperationRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,request_id,project_id,director_id,operation_manager_id,priority,
                    status,worktree_policy,created_at,updated_at
             FROM agentic_operations WHERE request_id=?1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        from_row(&row)
    }

    pub async fn queued_agentic_operations(&self) -> Result<Vec<AgenticOperationRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT id,request_id,project_id,director_id,operation_manager_id,priority,
                    status,worktree_policy,created_at,updated_at
             FROM agentic_operations
             WHERE status IN ('QUEUED','STARTING')
             ORDER BY priority ASC,created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn set_agentic_operation_status(
        &self,
        request_id: &str,
        status: &str,
    ) -> Result<AgenticOperationRecord, Error> {
        if !matches!(
            status,
            "QUEUED"
                | "STARTING"
                | "EXECUTING"
                | "BLOCKED"
                | "COMPLETED"
                | "FAILED"
                | "CANCELLED"
                | "OBSERVED"
        ) {
            return Err(Error::Invalid("agentic operation status"));
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_operations SET status=?1,updated_at=?2 WHERE request_id=?3",
        )
        .bind(status)
        .bind(Utc::now().to_rfc3339())
        .bind(request_id)
        .execute(&self.pool)
        .await?;
        self.agentic_operation_for_request(request_id).await
    }

    pub async fn set_agentic_operation_priority(
        &self,
        request_id: &str,
        priority: i64,
    ) -> Result<AgenticOperationRecord, Error> {
        if !(0..=1_000).contains(&priority) {
            return Err(Error::Invalid("agentic operation priority"));
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_operations SET priority=?1,updated_at=?2 WHERE request_id=?3",
        )
        .bind(priority)
        .bind(Utc::now().to_rfc3339())
        .bind(request_id)
        .execute(&self.pool)
        .await?;
        self.agentic_operation_for_request(request_id).await
    }
}

impl Database {
    /// Reconcile the operation projection with the authoritative Campaign and
    /// Gate records. The operation row is derived state: a cancelled Campaign
    /// must never leave an operation claiming to be executing.
    pub async fn reconcile_agentic_operations(&self) -> Result<Vec<AgenticOperationRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT o.id,o.request_id,o.project_id,o.director_id,o.operation_manager_id,
                    o.priority,o.status,o.worktree_policy,o.created_at,o.updated_at,
                    ir.mode AS intake_mode,
                    lr.state AS lead_state,
                    lw.request_id AS withdrawn_request,
                    c.status AS campaign_status,
                    (SELECT gd.outcome FROM gate_decisions gd
                     JOIN candidates ca ON ca.id=gd.candidate_id
                     JOIN attempts a ON a.id=ca.attempt_id
                     JOIN runs r ON r.id=a.run_id
                     WHERE r.campaign_id=c.id
                     ORDER BY gd.decided_at DESC LIMIT 1) AS gate_outcome
             FROM agentic_operations o
             LEFT JOIN intake_requests ir ON ir.id=o.request_id
             LEFT JOIN lead_requests lr ON lr.id=ir.lead_request_id
             LEFT JOIN lead_withdrawals lw ON lw.request_id=lr.id
             LEFT JOIN campaigns c ON c.id=ir.campaign_id
             WHERE o.status IN ('QUEUED','STARTING','EXECUTING')",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut changed = Vec::new();
        for row in &rows {
            let operation = from_row(row)?;
            let campaign_status: Option<String> = row.try_get("campaign_status")?;
            let gate_outcome: Option<String> = row.try_get("gate_outcome")?;
            let intake_mode: Option<String> = row.try_get("intake_mode")?;
            let lead_state: Option<String> = row.try_get("lead_state")?;
            let withdrawn: Option<String> = row.try_get("withdrawn_request")?;
            let target = match (campaign_status.as_deref(), gate_outcome.as_deref()) {
                (Some("CANCELLED"), _) => Some("CANCELLED"),
                (Some("FAILED"), _) => Some("FAILED"),
                (_, Some("ACCEPTED")) => Some("COMPLETED"),
                (_, Some("REPAIR_REQUIRED" | "REJECTED")) => Some("FAILED"),
                (_, _) if withdrawn.is_some() => Some("CANCELLED"),
                (_, _) if lead_state.as_deref() == Some("BLOCKED") => Some("BLOCKED"),
                (_, _) if lead_state.as_deref() == Some("FAILED") => Some("FAILED"),
                (_, _) if intake_mode.as_deref() == Some("REVIEW_PLAN") => Some("OBSERVED"),
                _ => None,
            };
            if let Some(target) = target
                && target != operation.status
            {
                changed.push(
                    self.set_agentic_operation_status(&operation.request_id, target)
                        .await?,
                );
            }
        }
        Ok(changed)
    }
}
