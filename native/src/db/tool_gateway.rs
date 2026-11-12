use super::*;

use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

use crate::tool_gateway::GatewayAuditRecord;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolGatewayCallRecord {
    pub call_id: String,
    pub worker_id: String,
    pub attempt_id: String,
    pub generation: i64,
    pub role: String,
    pub tool: String,
    pub args_sha256: String,
    pub decision: String,
    pub response_bytes: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub created_at: String,
}

fn bounded_error(value: Option<&str>) -> Option<String> {
    value.map(|value| value.chars().take(512).collect())
}

impl Database {
    /// Persist one gateway decision bound to the exact Worker lease and
    /// Attempt. This is append-only evidence; a caller cannot update or delete
    /// the row through the API.
    pub async fn record_tool_gateway_call(
        &self,
        worker_id: &str,
        attempt_id: &str,
        generation: i64,
        audit: &GatewayAuditRecord,
    ) -> Result<ToolGatewayCallRecord, Error> {
        if worker_id.trim().is_empty() || worker_id.len() > 256 {
            return Err(Error::Invalid("tool gateway worker identity"));
        }
        if attempt_id.trim().is_empty() || attempt_id.len() > 256 {
            return Err(Error::Invalid("tool gateway attempt identity"));
        }
        if generation <= 0 {
            return Err(Error::Invalid("tool gateway generation"));
        }
        if audit.tool.trim().is_empty() || audit.tool.len() > 256 {
            return Err(Error::Invalid("tool gateway tool identity"));
        }
        if audit.args_sha256.len() != 64
            || !audit
                .args_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("tool gateway argument digest"));
        }
        if !matches!(audit.decision.as_str(), "ALLOW" | "DENY") {
            return Err(Error::Invalid("tool gateway decision"));
        }
        let record = ToolGatewayCallRecord {
            call_id: Uuid::new_v4().to_string(),
            worker_id: worker_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            generation,
            role: audit.role.as_str().to_owned(),
            tool: audit.tool.clone(),
            args_sha256: audit.args_sha256.clone(),
            decision: audit.decision.clone(),
            response_bytes: u64::try_from(audit.response_bytes).unwrap_or(u64::MAX),
            duration_ms: audit.duration_ms,
            error: bounded_error(audit.error.as_deref()),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query(
            "INSERT INTO tool_gateway_calls
             (call_id,worker_id,attempt_id,generation,role,tool,args_sha256,
              decision,response_bytes,duration_ms,error,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )
        .bind(&record.call_id)
        .bind(&record.worker_id)
        .bind(&record.attempt_id)
        .bind(record.generation)
        .bind(&record.role)
        .bind(&record.tool)
        .bind(&record.args_sha256)
        .bind(&record.decision)
        .bind(i64::try_from(record.response_bytes).unwrap_or(i64::MAX))
        .bind(i64::try_from(record.duration_ms).unwrap_or(i64::MAX))
        .bind(&record.error)
        .bind(&record.created_at)
        .execute(&self.pool)
        .await?;
        Ok(record)
    }

    /// Most recent gateway calls for the Office diagnostics projection. The
    /// caller cannot page beyond the bounded maximum.
    pub async fn recent_tool_gateway_calls(
        &self,
        limit: usize,
    ) -> Result<Vec<ToolGatewayCallRecord>, Error> {
        let limit = limit.clamp(1, 200) as i64;
        let rows = sqlx::query::query(
            "SELECT call_id,worker_id,attempt_id,generation,role,tool,args_sha256,
                    decision,response_bytes,duration_ms,error,created_at
             FROM tool_gateway_calls
             ORDER BY created_at DESC, call_id DESC
             LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ToolGatewayCallRecord {
                    call_id: row.try_get("call_id")?,
                    worker_id: row.try_get("worker_id")?,
                    attempt_id: row.try_get("attempt_id")?,
                    generation: row.try_get("generation")?,
                    role: row.try_get("role")?,
                    tool: row.try_get("tool")?,
                    args_sha256: row.try_get("args_sha256")?,
                    decision: row.try_get("decision")?,
                    response_bytes: u64::try_from(row.try_get::<i64, _>("response_bytes")?)
                        .unwrap_or(0),
                    duration_ms: u64::try_from(row.try_get::<i64, _>("duration_ms")?).unwrap_or(0),
                    error: row.try_get("error")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_gateway::GatewayRole;

    #[test]
    fn stored_role_is_the_canonical_uppercase_identity() {
        for role in [
            GatewayRole::Implementer,
            GatewayRole::Repairer,
            GatewayRole::Reviewer,
            GatewayRole::Verifier,
            GatewayRole::Planner,
        ] {
            assert_eq!(GatewayRole::parse(role.as_str()), Some(role));
        }
        assert_eq!(
            GatewayRole::parse("implementer"),
            Some(GatewayRole::Implementer)
        );
        assert_eq!(GatewayRole::parse("unknown"), None);
    }

    #[tokio::test]
    async fn call_audit_is_durable_and_ordered() {
        let path = std::env::temp_dir().join(format!(
            "factory-tool-gateway-audit-{}.sqlite",
            Uuid::new_v4()
        ));
        let db = Database::open(&path).await.expect("database");
        let audit = GatewayAuditRecord {
            role: GatewayRole::Implementer,
            tool: "context7.query-docs".into(),
            args_sha256: "a".repeat(64),
            decision: "ALLOW".into(),
            duration_ms: 12,
            response_bytes: 34,
            error: None,
        };
        let record = db
            .record_tool_gateway_call("worker", "attempt", 1, &audit)
            .await
            .expect("record");
        assert_eq!(record.role, "IMPLEMENTER");
        assert_eq!(record.decision, "ALLOW");
        assert_eq!(record.args_sha256, audit.args_sha256);
        assert!(
            db.record_tool_gateway_call("", "attempt", 1, &audit)
                .await
                .is_err()
        );
        let recent = db.recent_tool_gateway_calls(10).await.expect("recent");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].call_id, record.call_id);
        assert!(
            sqlx::query::query("UPDATE tool_gateway_calls SET decision='DENY' WHERE call_id=?1")
                .bind(&record.call_id)
                .execute(&db.pool)
                .await
                .is_err(),
            "gateway audit rows must be immutable"
        );
        assert!(
            sqlx::query::query("DELETE FROM tool_gateway_calls WHERE call_id=?1")
                .bind(&record.call_id)
                .execute(&db.pool)
                .await
                .is_err(),
            "gateway audit rows must not be deleted"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn error_text_is_bounded() {
        let value = bounded_error(Some(&"x".repeat(2000))).expect("bounded");
        assert_eq!(value.chars().count(), 512);
        assert!(bounded_error(None).is_none());
    }
}
