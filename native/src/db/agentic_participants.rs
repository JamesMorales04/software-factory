//! Durable deterministic ParticipantPlan records.
//!
//! The plan is a Core compilation from route, assurance, WorkGraph and stack.
//! It is a projection of authority, not a grant: tools, paths and identities
//! remain constrained by the delegation, leases and frozen ActionSpec.

use super::*;
use chrono::Utc;
use sqlx::row::Row;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticParticipantPlanRecord {
    pub request_id: String,
    pub graph_sha256: Option<String>,
    pub route: String,
    pub assurance: String,
    pub work_units: u32,
    pub reviewer_count: u32,
    pub plan_json: String,
    pub plan_sha256: String,
    pub created_at: String,
    pub updated_at: String,
}

fn from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticParticipantPlanRecord, Error> {
    Ok(AgenticParticipantPlanRecord {
        request_id: row.try_get("request_id")?,
        graph_sha256: row.try_get("graph_sha256")?,
        route: row.try_get("route")?,
        assurance: row.try_get("assurance")?,
        work_units: u32::try_from(row.try_get::<i64, _>("work_units")?)
            .map_err(|_| Error::Invalid("participant plan work units"))?,
        reviewer_count: u32::try_from(row.try_get::<i64, _>("reviewer_count")?)
            .map_err(|_| Error::Invalid("participant plan reviewer count"))?,
        plan_json: row.try_get("plan_json")?,
        plan_sha256: row.try_get("plan_sha256")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl Database {
    pub async fn save_agentic_participant_plan(
        &self,
        request_id: &str,
        graph_sha256: Option<&str>,
        plan: &crate::agentic::ParticipantPlan,
    ) -> Result<AgenticParticipantPlanRecord, Error> {
        let plan_json =
            serde_json::to_string(plan).map_err(|_| Error::Invalid("participant plan encoding"))?;
        let plan_sha256 = sha256_hex(plan_json.as_bytes());
        let work_units = plan
            .assignments
            .iter()
            .filter(|assignment| assignment.role == "Implementer")
            .map(|assignment| assignment.count)
            .max()
            .unwrap_or(1);
        if let Some(graph_sha256) = graph_sha256
            && (graph_sha256.len() != 64
                || !graph_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(Error::Invalid("participant plan graph digest"));
        }
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO agentic_participant_plans
             (request_id,graph_sha256,route,assurance,work_units,reviewer_count,
              plan_json,plan_sha256,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)
             ON CONFLICT(request_id) DO UPDATE SET
               graph_sha256=excluded.graph_sha256,
               route=excluded.route,
               assurance=excluded.assurance,
               work_units=excluded.work_units,
               reviewer_count=excluded.reviewer_count,
               plan_json=excluded.plan_json,
               plan_sha256=excluded.plan_sha256,
               updated_at=excluded.updated_at",
        )
        .bind(request_id)
        .bind(graph_sha256)
        .bind(format!("{:?}", plan.route))
        .bind(format!("{:?}", plan.assurance))
        .bind(i64::from(work_units))
        .bind(i64::from(plan.reviewer_count))
        .bind(&plan_json)
        .bind(&plan_sha256)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_participant_plan(request_id).await
    }

    pub async fn agentic_participant_plan(
        &self,
        request_id: &str,
    ) -> Result<AgenticParticipantPlanRecord, Error> {
        let row = sqlx::query::query(
            "SELECT request_id,graph_sha256,route,assurance,work_units,reviewer_count,
                    plan_json,plan_sha256,created_at,updated_at
             FROM agentic_participant_plans WHERE request_id=?1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        from_row(&row)
    }
}
