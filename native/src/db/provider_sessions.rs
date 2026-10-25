//! Durable provider-session intent and terminal accounting.
//!
//! Intent is recorded before any external provider effect. Terminal evidence is
//! explicit; a disconnect or cleanup request is never treated as proof of stop.

use super::*;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

type IntentContext = (String, i64, i64, String, String, String, String, String);
type TerminalRow = (String, String, i64, String, String, String, String, String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewProviderIntent {
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTerminal {
    pub session_record_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub state: String,
    #[serde(default)]
    pub provider_session_id: String,
    #[serde(default)]
    pub turn_id: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_micros: u64,
    #[serde(default)]
    pub usage_known: bool,
    #[serde(default)]
    pub cleanup_receipt: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSessionRecord {
    pub session_record_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
    pub state: String,
    pub provider_session_id: String,
    pub provider_directory: String,
    pub turn_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub cleanup_receipt: String,
    pub error: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Database {
    pub async fn record_provider_intent(
        &self,
        input: &NewProviderIntent,
    ) -> Result<ProviderSessionRecord, Error> {
        if input.attempt_id.trim().is_empty()
            || input.worker_id.trim().is_empty()
            || input.generation <= 0
            || input.provider.is_empty()
            || input.model.is_empty()
            || input.provider_mode.is_empty()
        {
            return Err(Error::Invalid("provider session intent"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let context: Option<IntentContext> = sqlx::query_as::query_as(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,ca.execution_mode,
                        ca.provider,ca.model,ca.provider_mode,r.campaign_id
                 FROM attempts a
                 JOIN runs r ON r.id=a.run_id
                 JOIN campaigns c ON c.id=r.campaign_id
                 LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
                 JOIN worker_leases l ON l.attempt_id=a.id
                 WHERE a.id=?1",
        )
        .bind(&input.attempt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (
            worker_id,
            generation,
            expires_at_ms,
            mode,
            provider,
            model,
            provider_mode,
            campaign_id,
        ) = context.ok_or(Error::Forbidden(
            "provider intent requires a live Worker lease and authorization".into(),
        ))?;
        if worker_id != input.worker_id
            || generation != input.generation
            || expires_at_ms <= Utc::now().timestamp_millis()
            || mode != "TRUSTED_LOCAL_PROVIDER"
            || provider != input.provider
            || model != input.model
            || provider_mode != input.provider_mode
        {
            return Err(Error::Forbidden(
                "provider intent does not match the live lease or authorization".into(),
            ));
        }
        let existing: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT session_record_id FROM provider_sessions WHERE attempt_id=?1 AND generation=?2",
        )
        .bind(&input.attempt_id)
        .bind(input.generation)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((existing_id,)) = existing {
            let record = load_provider_session(&mut transaction, &existing_id).await?;
            if record.worker_id == input.worker_id
                && record.provider == input.provider
                && record.model == input.model
                && record.provider_mode == input.provider_mode
                && matches!(record.state.as_str(), "INTENT" | "ACTIVE")
            {
                transaction.commit().await?;
                return Ok(record);
            }
            return Err(Error::Conflict(
                "attempt/generation already has different provider session intent",
            ));
        }
        let (session_count,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM provider_sessions ps
             JOIN attempts a ON a.id=ps.attempt_id
             JOIN runs r ON r.id=a.run_id
             WHERE r.campaign_id=?1",
        )
        .bind(&campaign_id)
        .fetch_one(&mut *transaction)
        .await?;
        let (max_sessions,): (i64,) = sqlx::query_as::query_as(
            "SELECT max_sessions FROM campaign_authorizations WHERE campaign_id=?1",
        )
        .bind(&campaign_id)
        .fetch_one(&mut *transaction)
        .await?;
        if session_count >= max_sessions {
            return Err(Error::Forbidden(
                "provider session budget is exhausted".into(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let record = ProviderSessionRecord {
            session_record_id: Uuid::new_v4().to_string(),
            attempt_id: input.attempt_id.clone(),
            worker_id: input.worker_id.clone(),
            generation: input.generation,
            provider: input.provider.clone(),
            model: input.model.clone(),
            provider_mode: input.provider_mode.clone(),
            state: "INTENT".into(),
            provider_session_id: String::new(),
            provider_directory: String::new(),
            turn_id: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_micros: 0,
            usage_known: false,
            cleanup_receipt: String::new(),
            error: String::new(),
            created_at: now.clone(),
            updated_at: now,
        };
        sqlx::query::query(
            "INSERT INTO provider_sessions
             (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,state,
              provider_session_id,turn_id,input_tokens,output_tokens,total_tokens,cost_micros,
              usage_known,cleanup_receipt,error,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'INTENT',NULL,NULL,0,0,0,0,0,NULL,NULL,?8,?8)",
        )
        .bind(&record.session_record_id)
        .bind(&record.attempt_id)
        .bind(&record.worker_id)
        .bind(record.generation)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.provider_mode)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &record.attempt_id,
            "provider.session_intended",
            "provider session intent recorded before external effect",
        )
        .await?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn record_provider_terminal(
        &self,
        input: &ProviderTerminal,
    ) -> Result<ProviderSessionRecord, Error> {
        if !matches!(input.state.as_str(), "COMPLETED" | "FAILED" | "CANCELLED") {
            return Err(Error::Invalid("provider terminal state"));
        }
        if input.state == "COMPLETED"
            && (input.cleanup_receipt.trim().is_empty() || !input.usage_known)
        {
            return Err(Error::Forbidden(
                "completed provider session requires confirmed cleanup and known usage".into(),
            ));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row: Option<TerminalRow> = sqlx::query_as::query_as(
            "SELECT session_record_id,state,generation,worker_id,attempt_id,
                        COALESCE(provider_session_id,''),COALESCE(turn_id,''),provider_mode
                 FROM provider_sessions WHERE session_record_id=?1",
        )
        .bind(&input.session_record_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (record_id, state, generation, worker_id, attempt_id, _, _, _) =
            row.ok_or(Error::NotFound)?;
        if generation != input.generation || worker_id != input.worker_id {
            return Err(Error::Forbidden(
                "provider terminal evidence does not match the session owner".into(),
            ));
        }
        if !matches!(state.as_str(), "INTENT" | "ACTIVE") {
            return Err(Error::Conflict("provider session is already terminal"));
        }
        let campaign: (String, i64, i64, i64) = sqlx::query_as::query_as(
            "SELECT r.campaign_id,ca.max_input_tokens,ca.max_output_tokens,ca.max_cost_micros
             FROM attempts a JOIN runs r ON r.id=a.run_id
             JOIN campaign_authorizations ca ON ca.campaign_id=r.campaign_id
             WHERE a.id=?1",
        )
        .bind(&attempt_id)
        .fetch_one(&mut *transaction)
        .await?;
        let (campaign_id, max_input, max_output, max_cost) = campaign;
        let max_total = max_input.saturating_add(max_output);
        let (used_input, used_output, used_total, used_cost): (i64, i64, i64, i64) =
            sqlx::query_as::query_as(
                "SELECT COALESCE(SUM(ps.input_tokens),0),COALESCE(SUM(ps.output_tokens),0),
                        COALESCE(SUM(ps.total_tokens),0),COALESCE(SUM(ps.cost_micros),0)
                 FROM provider_sessions ps
                 JOIN attempts a ON a.id=ps.attempt_id
                 JOIN runs r ON r.id=a.run_id
                 WHERE r.campaign_id=?1 AND ps.session_record_id<>?2",
            )
            .bind(&campaign_id)
            .bind(&record_id)
            .fetch_one(&mut *transaction)
            .await?;
        let next_input = used_input.saturating_add(input.input_tokens.min(i64::MAX as u64) as i64);
        let next_output =
            used_output.saturating_add(input.output_tokens.min(i64::MAX as u64) as i64);
        let next_total = used_total.saturating_add(input.total_tokens.min(i64::MAX as u64) as i64);
        let next_cost = used_cost.saturating_add(input.cost_micros.min(i64::MAX as u64) as i64);
        let over_budget = next_input > max_input
            || next_output > max_output
            || next_total > max_total
            || next_cost > max_cost;
        let cancel_requested = state == "CANCEL_REQUESTED";
        let stored_state = if over_budget {
            "FAILED"
        } else if cancel_requested && input.state == "COMPLETED" {
            "CANCELLED"
        } else {
            input.state.as_str()
        };
        let error = if over_budget {
            "provider usage exceeded the authorized budget"
        } else if cancel_requested && input.state == "COMPLETED" {
            "cancellation was requested before provider completion"
        } else {
            input.error.as_str()
        };
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE provider_sessions
             SET state=?1,provider_session_id=?2,turn_id=?3,input_tokens=?4,output_tokens=?5,
                 total_tokens=?6,cost_micros=?7,usage_known=?8,cleanup_receipt=?9,error=?10,updated_at=?11
             WHERE session_record_id=?12",
        )
        .bind(stored_state)
        .bind(nullable(&input.provider_session_id))
        .bind(nullable(&input.turn_id))
        .bind(i64::try_from(input.input_tokens).map_err(|_| Error::Invalid("provider input usage"))?)
        .bind(i64::try_from(input.output_tokens).map_err(|_| Error::Invalid("provider output usage"))?)
        .bind(i64::try_from(input.total_tokens).map_err(|_| Error::Invalid("provider total usage"))?)
        .bind(i64::try_from(input.cost_micros).map_err(|_| Error::Invalid("provider cost usage"))?)
        .bind(i64::from(input.usage_known))
        .bind(nullable(&input.cleanup_receipt))
        .bind(nullable(error))
        .bind(&now)
        .bind(&record_id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &attempt_id,
            "provider.session_terminal",
            &format!("state={stored_state} usage_known={}", input.usage_known),
        )
        .await?;
        let record = load_provider_session(&mut transaction, &record_id).await?;
        transaction.commit().await?;
        if over_budget {
            return Err(Error::Forbidden(
                "provider usage exceeded the authorized budget".into(),
            ));
        }
        if cancel_requested && input.state == "COMPLETED" {
            return Err(Error::Forbidden(
                "provider cancellation was requested before completion".into(),
            ));
        }
        Ok(record)
    }

    pub async fn mark_provider_active(
        &self,
        session_record_id: &str,
        worker_id: &str,
        generation: i64,
        provider_session_id: &str,
        provider_directory: &str,
    ) -> Result<ProviderSessionRecord, Error> {
        if provider_session_id.trim().is_empty() {
            return Err(Error::Invalid("provider session id"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let changed = sqlx::query::query(
            "UPDATE provider_sessions
             SET state='ACTIVE',provider_session_id=?1,provider_directory=?2,updated_at=?3
             WHERE session_record_id=?4 AND worker_id=?5 AND generation=?6 AND state='INTENT'",
        )
        .bind(provider_session_id)
        .bind(provider_directory)
        .bind(Utc::now().to_rfc3339())
        .bind(session_record_id)
        .bind(worker_id)
        .bind(generation)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("provider session is not an owned intent"));
        }
        record_event(
            &mut transaction,
            session_record_id,
            "provider.session_active",
            "provider session identity bound after external creation",
        )
        .await?;
        let record = load_provider_session(&mut transaction, session_record_id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn record_provider_cleanup(
        &self,
        session_record_id: &str,
        worker_id: &str,
        generation: i64,
        cleanup_receipt: &str,
    ) -> Result<ProviderSessionRecord, Error> {
        if cleanup_receipt.trim().is_empty() {
            return Err(Error::Invalid("provider cleanup receipt"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let changed = sqlx::query::query(
            "UPDATE provider_sessions
             SET state='CLEANED',cleanup_receipt=?1,updated_at=?2
             WHERE session_record_id=?3 AND worker_id=?4 AND generation=?5
               AND state IN ('INTENT','ACTIVE','CANCEL_REQUESTED','CANCELLED','ORPHANED')",
        )
        .bind(cleanup_receipt)
        .bind(Utc::now().to_rfc3339())
        .bind(session_record_id)
        .bind(worker_id)
        .bind(generation)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict(
                "provider session is not cleanable by this owner",
            ));
        }
        record_event(
            &mut transaction,
            session_record_id,
            "provider.session_cleaned",
            "external cleanup confirmed",
        )
        .await?;
        let record = load_provider_session(&mut transaction, session_record_id).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Recovery Worker view: sessions whose external lifecycle is not terminal.
    pub async fn unresolved_provider_sessions(&self) -> Result<Vec<ProviderSessionRecord>, Error> {
        let ids: Vec<(String,)> = sqlx::query_as::query_as(
            "SELECT session_record_id FROM provider_sessions
             WHERE state IN ('INTENT','ACTIVE','CANCEL_REQUESTED','ORPHANED')
             ORDER BY created_at,session_record_id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut connection = self.pool.acquire().await?;
        let mut records = Vec::with_capacity(ids.len());
        for (id,) in ids {
            records.push(load_provider_session(&mut connection, &id).await?);
        }
        Ok(records)
    }

    pub async fn provider_session(
        &self,
        session_record_id: &str,
    ) -> Result<ProviderSessionRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_provider_session(&mut connection, session_record_id).await
    }

    pub async fn provider_sessions_for_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<Vec<ProviderSessionRecord>, Error> {
        let ids: Vec<(String,)> = sqlx::query_as::query_as(
            "SELECT session_record_id FROM provider_sessions WHERE attempt_id=?1 ORDER BY created_at",
        )
        .bind(attempt_id)
        .fetch_all(&self.pool)
        .await?;
        let mut connection = self.pool.acquire().await?;
        let mut records = Vec::with_capacity(ids.len());
        for (id,) in ids {
            records.push(load_provider_session(&mut connection, &id).await?);
        }
        Ok(records)
    }
}

fn nullable(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

async fn load_provider_session(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<ProviderSessionRecord, Error> {
    use sqlx::row::Row;
    let row = sqlx::query::query(
        "SELECT session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,
                state,COALESCE(provider_session_id,'') AS provider_session_id,
                COALESCE(provider_directory,'') AS provider_directory,
                COALESCE(turn_id,'') AS turn_id,input_tokens,output_tokens,
                total_tokens,cost_micros,usage_known,COALESCE(cleanup_receipt,'') AS cleanup_receipt,COALESCE(error,'') AS error,
                created_at,updated_at
         FROM provider_sessions WHERE session_record_id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(ProviderSessionRecord {
        session_record_id: row.try_get("session_record_id")?,
        attempt_id: row.try_get("attempt_id")?,
        worker_id: row.try_get("worker_id")?,
        generation: row.try_get("generation")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        provider_mode: row.try_get("provider_mode")?,
        state: row.try_get("state")?,
        provider_session_id: row.try_get("provider_session_id")?,
        provider_directory: row.try_get("provider_directory")?,
        turn_id: row.try_get("turn_id")?,
        input_tokens: u64::try_from(row.try_get::<i64, _>("input_tokens")?)
            .map_err(|_| Error::Invalid("stored provider input usage"))?,
        output_tokens: u64::try_from(row.try_get::<i64, _>("output_tokens")?)
            .map_err(|_| Error::Invalid("stored provider output usage"))?,
        total_tokens: u64::try_from(row.try_get::<i64, _>("total_tokens")?)
            .map_err(|_| Error::Invalid("stored provider total usage"))?,
        cost_micros: u64::try_from(row.try_get::<i64, _>("cost_micros")?)
            .map_err(|_| Error::Invalid("stored provider cost usage"))?,
        usage_known: row.try_get::<i64, _>("usage_known")? != 0,
        cleanup_receipt: row.try_get("cleanup_receipt")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    async fn fixture(max_input: i64) -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-provider-session-{}.sqlite",
            Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query("INSERT INTO projects VALUES ('project','/fixture',?1,?2,'')")
            .bind("a".repeat(40))
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO actions VALUES ('action','project','objective','opencode','deepseek-v4.1-flash','R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        let spec = r#"{"schema_version":1,"project_id":"project","repository":"/fixture","base_commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","objective":"objective","objective_sha256":"hash","provider":"opencode","model":"deepseek-v4.1-flash","allowed_paths":["src"],"checks":[]}"#;
        sqlx::query::query(
            "INSERT INTO action_specs(action_id,spec_json,spec_sha256) VALUES ('action',?1,?2)",
        )
        .bind(spec)
        .bind(crate::db::sha256_hex(spec.as_bytes()))
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query("INSERT INTO campaigns VALUES ('campaign','action','AUTHORIZED',?1)")
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query("INSERT INTO runs VALUES ('run','campaign','QUEUED',?1)")
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES ('attempt','run','LEASED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
              authorized_paths_json,max_attempts,provider,model,provider_mode,delegation_id,
              max_sessions,max_turns,max_input_tokens,max_output_tokens,max_cost_micros,
              deadline_secs,allowed_tools_json,created_at)
             VALUES ('campaign','auth','R0','TRUSTED_LOCAL_PROVIDER','test','test','[\"src\"]',1,
                     'opencode','deepseek-v4.1-flash','build','grant',1,1,?1,100,1000,30,'[]',?2)",
        )
        .bind(max_input)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        (path, db)
    }

    async fn insert_session(db: &Database, id: &str, attempt: &str) {
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO provider_sessions
             (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,state,
              created_at,updated_at)
             VALUES (?1,?2,'worker',1,'opencode','deepseek-v4.1-flash','build','INTENT',?3,?3)",
        )
        .bind(id)
        .bind(attempt)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn terminal_evidence_requires_cleanup_and_enforces_budget() {
        let (path, db) = fixture(5).await;
        insert_session(&db, "session-budget", "attempt").await;
        let result = db
            .record_provider_terminal(&ProviderTerminal {
                session_record_id: "session-budget".into(),
                worker_id: "worker".into(),
                generation: 1,
                state: "COMPLETED".into(),
                provider_session_id: "ses_budget".into(),
                turn_id: "msg_budget".into(),
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                cost_micros: 100,
                usage_known: true,
                cleanup_receipt: "deleted:404".into(),
                error: String::new(),
            })
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))), "{result:?}");
        let (state, input, cleanup): (String, i64, Option<String>) =
            sqlx::query_as::query_as(
                "SELECT state,input_tokens,cleanup_receipt FROM provider_sessions WHERE session_record_id='session-budget'",
            )
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(state, "FAILED");
        assert_eq!(input, 10);
        assert_eq!(cleanup.as_deref(), Some("deleted:404"));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unresolved_provider_session_blocks_replacement_dispatch() {
        let (path, db) = fixture(100).await;
        insert_session(&db, "session-unresolved", "attempt").await;
        sqlx::query::query("UPDATE attempts SET status='QUEUED' WHERE id='attempt'")
            .execute(&db.pool)
            .await
            .unwrap();
        let result = db
            .claim_assignment("worker", Utc::now().timestamp_millis(), 60_000)
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))), "{result:?}");
        let (leases,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM worker_leases WHERE attempt_id='attempt'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(leases, 0);
        let (status,): (String,) =
            sqlx::query_as::query_as("SELECT status FROM attempts WHERE id='attempt'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(status, "QUEUED");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn completed_without_cleanup_is_rejected_before_mutation() {
        let (path, db) = fixture(100).await;
        insert_session(&db, "session-cleanup", "attempt").await;
        let result = db
            .record_provider_terminal(&ProviderTerminal {
                session_record_id: "session-cleanup".into(),
                worker_id: "worker".into(),
                generation: 1,
                state: "COMPLETED".into(),
                provider_session_id: "ses_cleanup".into(),
                turn_id: "msg_cleanup".into(),
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                cost_micros: 1,
                usage_known: true,
                cleanup_receipt: String::new(),
                error: String::new(),
            })
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))), "{result:?}");
        let (state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM provider_sessions WHERE session_record_id='session-cleanup'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(state, "INTENT");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
