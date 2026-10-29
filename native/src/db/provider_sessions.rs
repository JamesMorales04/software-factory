//! Durable provider-session intent and terminal accounting.
//!
//! Intent is recorded before any external provider effect. Terminal evidence is
//! explicit; a disconnect or cleanup request is never treated as proof of stop.

use super::*;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

type IntentContext = (
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
);
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
    #[serde(default)]
    pub provider_directory: String,
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
pub struct ProviderTurnAuthorization {
    pub reservation_id: String,
    pub session_record_id: String,
    pub attempt_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_total_tokens: u64,
    pub max_cost_micros: u64,
    pub request_sha256: String,
    pub expires_at_ms: i64,
    #[serde(default = "default_max_requests")]
    pub max_requests: u32,
    #[serde(default)]
    pub requests_bound: u32,
}

fn default_max_requests() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTurnUsage {
    pub reservation_id: String,
    pub worker_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTurnBindingInput {
    pub reservation_id: String,
    pub session_record_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub request_sha256: String,
    pub model: String,
    pub path: String,
    pub body_bytes: u64,
    #[serde(default)]
    pub sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTurnBinding {
    pub reservation_id: String,
    pub request_sha256: String,
    pub bound_at: String,
    #[serde(default)]
    pub sequence: u32,
    #[serde(default)]
    pub requests_bound: u32,
    #[serde(default = "default_max_requests")]
    pub max_requests: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTurnReservationRecord {
    pub reservation_id: String,
    pub session_record_id: String,
    pub attempt_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub state: String,
    pub actual_request_sha256: String,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_total_tokens: u64,
    pub max_cost_micros: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub error: String,
    pub expires_at_ms: i64,
    pub max_requests: u32,
    pub requests_bound: u32,
    pub created_at: String,
    pub updated_at: String,
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
    pub async fn authorize_provider_turn(
        &self,
        session_record_id: &str,
        worker_id: &str,
        generation: i64,
        requested_output_tokens: u64,
        request_sha256: &str,
        max_requests: u32,
    ) -> Result<ProviderTurnAuthorization, Error> {
        if requested_output_tokens == 0
            || requested_output_tokens > 4096
            || max_requests == 0
            || max_requests > 8
            || request_sha256.len() != 64
            || !request_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("provider turn request"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        use sqlx::row::Row;
        let now_ms = Utc::now().timestamp_millis();
        sqlx::query::query(
            "UPDATE provider_turn_reservations
             SET state='RELEASED',error='expired before any egress request',updated_at=?1
             WHERE state='RESERVED' AND requests_bound=0 AND expires_at_ms<=?2",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        let session = sqlx::query::query(
            "SELECT attempt_id,worker_id,generation,state,provider,model
             FROM provider_sessions WHERE session_record_id=?1",
        )
        .bind(session_record_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(session) = session else {
            return Err(Error::NotFound);
        };
        let attempt_id: String = session.try_get("attempt_id")?;
        let session_worker: String = session.try_get("worker_id")?;
        let session_generation: i64 = session.try_get("generation")?;
        let state: String = session.try_get("state")?;
        let provider: String = session.try_get("provider")?;
        let model: String = session.try_get("model")?;
        if provider != "codex" && max_requests != 1 {
            return Err(Error::Invalid(
                "only a Codex provider turn may bind multiple egress requests",
            ));
        }
        if session_worker != worker_id || session_generation != generation || state != "ACTIVE" {
            return Err(Error::Forbidden(
                "provider turn is not authorized by the active session".into(),
            ));
        }
        let lease = sqlx::query::query(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status,c.status AS campaign_status,
                    r.campaign_id,ca.schema_version,ca.max_input_tokens,ca.max_output_tokens,
                    ca.max_cost_micros,ca.deadline_secs
             FROM attempts a JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE a.id=?1",
        )
        .bind(&attempt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(lease) = lease else {
            return Err(Error::Forbidden(
                "provider turn has no current lease".into(),
            ));
        };
        let lease_worker: Option<String> = lease.try_get("worker_id")?;
        let lease_generation: Option<i64> = lease.try_get("generation")?;
        let lease_expiry: Option<i64> = lease.try_get("expires_at_ms")?;
        let attempt_status: String = lease.try_get("status")?;
        let campaign_status: String = lease.try_get("campaign_status")?;
        if lease_worker.as_deref() != Some(worker_id)
            || lease_generation != Some(generation)
            || lease_expiry.is_none_or(|expiry| expiry <= now_ms)
            || attempt_status != "LEASED"
            || campaign_status != "AUTHORIZED"
        {
            return Err(Error::Forbidden(
                "provider turn is not authorized by the current lease".into(),
            ));
        }
        let campaign_id: String = lease.try_get("campaign_id")?;
        let authorization_version: i64 = lease.try_get("schema_version")?;
        if authorization_version != 1 {
            // Execution Policy V2 has no cumulative per-role token/cost ceiling.
            // Per-request reservation is the shared inference ledger's job
            // (#73, migration 0035). Fail closed until that linkage exists.
            return Err(Error::Forbidden(
                "V2 provider turn authorization requires the shared inference ledger".into(),
            ));
        }
        let max_input: i64 = lease.try_get("max_input_tokens")?;
        let max_output: i64 = lease.try_get("max_output_tokens")?;
        let max_cost: i64 = lease.try_get("max_cost_micros")?;
        let deadline_secs: i64 = lease.try_get("deadline_secs")?;
        let used: (i64, i64, i64, i64) = sqlx::query_as::query_as(
            "SELECT COALESCE(SUM(ps.input_tokens),0),COALESCE(SUM(ps.output_tokens),0),
                    COALESCE(SUM(ps.total_tokens),0),COALESCE(SUM(ps.cost_micros),0)
             FROM provider_sessions ps JOIN attempts a ON a.id=ps.attempt_id
             JOIN runs r ON r.id=a.run_id WHERE r.campaign_id=?1",
        )
        .bind(&campaign_id)
        .fetch_one(&mut *transaction)
        .await?;
        let reserved: (i64, i64, i64) = sqlx::query_as::query_as(
            "SELECT COALESCE(SUM(ptr.max_input_tokens),0),
                    COALESCE(SUM(ptr.max_output_tokens),0),
                    COALESCE(SUM(ptr.max_cost_micros),0)
             FROM provider_turn_reservations ptr JOIN attempts a ON a.id=ptr.attempt_id
             JOIN runs r ON r.id=a.run_id
             WHERE r.campaign_id=?1 AND ptr.state='RESERVED'",
        )
        .bind(&campaign_id)
        .fetch_one(&mut *transaction)
        .await?;
        let used_input = used.0;
        let used_output = used.1;
        let used_total = used.2;
        let used_cost = used.3;
        let max_total = max_input.saturating_add(max_output);
        let remaining_input = max_input
            .saturating_sub(used_input)
            .saturating_sub(reserved.0);
        let remaining_output = max_output
            .saturating_sub(used_output)
            .saturating_sub(reserved.1);
        let remaining_total = max_total
            .saturating_sub(used_total)
            .saturating_sub(reserved.0)
            .saturating_sub(reserved.1);
        let remaining_cost = max_cost
            .saturating_sub(used_cost)
            .saturating_sub(reserved.2);
        let grant_output = i64::try_from(requested_output_tokens)
            .map_err(|_| Error::Invalid("provider turn output"))?
            .min(remaining_output)
            .min(remaining_total);
        if remaining_input <= 0 || grant_output <= 0 || remaining_cost <= 0 {
            return Err(Error::Forbidden("provider turn budget is exhausted".into()));
        }
        let reservation_id = Uuid::new_v4().to_string();
        let expires_at_ms = now_ms
            .saturating_add(deadline_secs.saturating_mul(1000))
            .saturating_add(30_000);
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO provider_turn_reservations
             (reservation_id,session_record_id,attempt_id,generation,provider,model,
              max_input_tokens,max_output_tokens,max_total_tokens,max_cost_micros,
              request_sha256,state,expires_at_ms,created_at,updated_at,max_requests,requests_bound)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'RESERVED',?12,?13,?13,?14,0)",
        )
        .bind(&reservation_id)
        .bind(session_record_id)
        .bind(&attempt_id)
        .bind(generation)
        .bind(&provider)
        .bind(&model)
        .bind(remaining_input)
        .bind(grant_output)
        .bind(remaining_total)
        .bind(remaining_cost)
        .bind(request_sha256)
        .bind(expires_at_ms)
        .bind(&now)
        .bind(i64::from(max_requests))
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &attempt_id,
            "provider.turn_authorized",
            &format!("reservation={reservation_id} output={grant_output} cost={remaining_cost}"),
        )
        .await?;
        transaction.commit().await?;
        Ok(ProviderTurnAuthorization {
            reservation_id,
            session_record_id: session_record_id.into(),
            attempt_id,
            generation,
            provider,
            model,
            max_input_tokens: u64::try_from(remaining_input)
                .map_err(|_| Error::Invalid("provider turn input"))?,
            max_output_tokens: u64::try_from(grant_output)
                .map_err(|_| Error::Invalid("provider turn output"))?,
            max_total_tokens: u64::try_from(remaining_total)
                .map_err(|_| Error::Invalid("provider turn total"))?,
            max_cost_micros: u64::try_from(remaining_cost)
                .map_err(|_| Error::Invalid("provider turn cost"))?,
            request_sha256: request_sha256.into(),
            expires_at_ms,
            max_requests,
            requests_bound: 0,
        })
    }

    pub async fn bind_provider_turn(
        &self,
        input: &ProviderTurnBindingInput,
    ) -> Result<ProviderTurnBinding, Error> {
        if input.reservation_id.trim().is_empty()
            || input.session_record_id.trim().is_empty()
            || input.worker_id.trim().is_empty()
            || input.generation <= 0
            || !is_sha256(&input.request_sha256)
            || input.model.trim().is_empty()
            || !matches!(
                input.path.as_str(),
                "/v1/chat/completions" | "/v1/responses"
            )
            || input.body_bytes == 0
            || input.body_bytes > 2 * 1024 * 1024
        {
            return Err(Error::Invalid("provider turn binding request"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        use sqlx::row::Row;
        let row = sqlx::query::query(
            "SELECT ptr.session_record_id,ptr.attempt_id,ptr.generation,ptr.model,ptr.state,
                    ptr.actual_request_sha256,ptr.expires_at_ms,ptr.max_requests,
                    ptr.requests_bound,
                    ps.worker_id,ps.state AS session_state,
                    l.worker_id AS lease_worker,l.generation AS lease_generation,
                    l.expires_at_ms AS lease_expires, a.status AS attempt_status,
                    c.status AS campaign_status
             FROM provider_turn_reservations ptr
             JOIN provider_sessions ps ON ps.session_record_id=ptr.session_record_id
             JOIN attempts a ON a.id=ptr.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE ptr.reservation_id=?1",
        )
        .bind(&input.reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return Err(Error::NotFound);
        };
        let session_record_id: String = row.try_get("session_record_id")?;
        let attempt_id: String = row.try_get("attempt_id")?;
        let generation: i64 = row.try_get("generation")?;
        let model: String = row.try_get("model")?;
        let state: String = row.try_get("state")?;
        let actual_request_sha256: String = row.try_get("actual_request_sha256")?;
        let expires_at_ms: i64 = row.try_get("expires_at_ms")?;
        let max_requests: i64 = row.try_get("max_requests")?;
        let requests_bound: i64 = row.try_get("requests_bound")?;
        let session_worker: String = row.try_get("worker_id")?;
        let session_state: String = row.try_get("session_state")?;
        let lease_worker: Option<String> = row.try_get("lease_worker")?;
        let lease_generation: Option<i64> = row.try_get("lease_generation")?;
        let lease_expires: Option<i64> = row.try_get("lease_expires")?;
        let attempt_status: String = row.try_get("attempt_status")?;
        let campaign_status: String = row.try_get("campaign_status")?;
        let now_ms = Utc::now().timestamp_millis();
        if session_record_id != input.session_record_id
            || generation != input.generation
            || session_worker != input.worker_id
            || session_state != "ACTIVE"
            || state != "RESERVED"
            || expires_at_ms <= now_ms
            || model != input.model
            || lease_worker.as_deref() != Some(input.worker_id.as_str())
            || lease_generation != Some(input.generation)
            || lease_expires.is_none_or(|expiry| expiry <= now_ms)
            || attempt_status != "LEASED"
            || campaign_status != "AUTHORIZED"
        {
            return Err(Error::Forbidden(
                "provider turn binding is not authorized by the active session, lease and reservation"
                    .into(),
            ));
        }
        let sequence = i64::from(input.sequence);
        if sequence != requests_bound {
            return Err(Error::Conflict(
                "provider turn egress request sequence is not contiguous",
            ));
        }
        if sequence >= max_requests {
            return Err(Error::Forbidden(
                "provider turn egress request budget is exhausted".into(),
            ));
        }
        if sequence == 0 && !actual_request_sha256.is_empty() {
            return Err(Error::Conflict("provider turn request is already bound"));
        }
        let now = Utc::now().to_rfc3339();
        let request_id = Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO provider_turn_egress_requests
             (request_id,reservation_id,sequence,actual_request_sha256,model,path,body_bytes,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        )
        .bind(&request_id)
        .bind(&input.reservation_id)
        .bind(sequence)
        .bind(&input.request_sha256)
        .bind(&input.model)
        .bind(&input.path)
        .bind(i64::try_from(input.body_bytes).map_err(|_| Error::Invalid("provider turn body bytes"))?)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let first_digest = if sequence == 0 {
            input.request_sha256.clone()
        } else {
            actual_request_sha256
        };
        let changed = sqlx::query::query(
            "UPDATE provider_turn_reservations
             SET actual_request_sha256=?1,requests_bound=requests_bound+1,updated_at=?2
             WHERE reservation_id=?3 AND state='RESERVED' AND requests_bound=?4",
        )
        .bind(&first_digest)
        .bind(&now)
        .bind(&input.reservation_id)
        .bind(sequence)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("provider turn egress request raced"));
        }
        record_event(
            &mut transaction,
            &attempt_id,
            "provider.turn_request_bound",
            &format!(
                "reservation={} sequence={sequence} path={} model={} bytes={} digest={}",
                input.reservation_id,
                input.path,
                input.model,
                input.body_bytes,
                input.request_sha256
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(ProviderTurnBinding {
            reservation_id: input.reservation_id.clone(),
            request_sha256: input.request_sha256.clone(),
            bound_at: now,
            sequence: input.sequence,
            requests_bound: u32::try_from(requests_bound + 1).unwrap_or(u32::MAX),
            max_requests: u32::try_from(max_requests).unwrap_or(u32::MAX),
        })
    }

    pub async fn resolve_provider_turn_reservation(
        &self,
        reservation_id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<(), Error> {
        if reservation_id.trim().is_empty()
            || actor.trim().is_empty()
            || actor.len() > 128
            || reason.trim().is_empty()
            || reason.len() > 500
        {
            return Err(Error::Invalid("provider turn resolution identity"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        use sqlx::row::Row;
        let row = sqlx::query::query(
            "SELECT session_record_id,attempt_id,max_input_tokens,max_output_tokens,
                    max_total_tokens,max_cost_micros,actual_request_sha256
             FROM provider_turn_reservations
             WHERE reservation_id=?1 AND state='RESERVED'",
        )
        .bind(reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return Err(Error::Conflict("provider turn is not reserved"));
        };
        let session_record_id: String = row.try_get("session_record_id")?;
        let attempt_id: String = row.try_get("attempt_id")?;
        let max_input: i64 = row.try_get("max_input_tokens")?;
        let max_output: i64 = row.try_get("max_output_tokens")?;
        let max_total: i64 = row.try_get("max_total_tokens")?;
        let max_cost: i64 = row.try_get("max_cost_micros")?;
        let actual_request_sha256: String = row.try_get("actual_request_sha256")?;
        if actual_request_sha256.is_empty() {
            return Err(Error::Conflict(
                "provider turn has no bound request; use the worker release path",
            ));
        }
        let now = Utc::now().to_rfc3339();
        let error = format!(
            "resolved by {actor}: {reason}; worst-case bound reservation charged because usage is unknown"
        );
        sqlx::query::query(
            "UPDATE provider_turn_reservations
             SET state='SETTLED',input_tokens=?1,output_tokens=?2,total_tokens=?3,
                 cost_micros=?4,usage_known=0,error=?5,updated_at=?6
             WHERE reservation_id=?7 AND state='RESERVED'",
        )
        .bind(max_input)
        .bind(max_output)
        .bind(max_total)
        .bind(max_cost)
        .bind(&error)
        .bind(&now)
        .bind(reservation_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE provider_sessions
             SET input_tokens=input_tokens+?1,output_tokens=output_tokens+?2,
                 total_tokens=total_tokens+?3,cost_micros=cost_micros+?4,
                 usage_known=0,
                 error=?5,updated_at=?6
             WHERE session_record_id=?7",
        )
        .bind(max_input)
        .bind(max_output)
        .bind(max_total)
        .bind(max_cost)
        .bind(&error)
        .bind(&now)
        .bind(&session_record_id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &attempt_id,
            "provider.turn_resolved",
            &format!(
                "reservation={reservation_id} actor={actor} reason={reason} settlement=worst_case_bound"
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn settle_provider_turn(
        &self,
        usage: &ProviderTurnUsage,
    ) -> Result<ProviderTurnUsage, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        use sqlx::row::Row;
        let row = sqlx::query::query(
            "SELECT session_record_id,attempt_id,state,max_input_tokens,max_output_tokens,
                    max_total_tokens,max_cost_micros,input_tokens,output_tokens,total_tokens,
                    cost_micros,usage_known
             FROM provider_turn_reservations WHERE reservation_id=?1",
        )
        .bind(&usage.reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return Err(Error::NotFound);
        };
        let session_record_id: String = row.try_get("session_record_id")?;
        let attempt_id: String = row.try_get("attempt_id")?;
        let state: String = row.try_get("state")?;
        let session_worker: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT worker_id FROM provider_sessions WHERE session_record_id=?1",
        )
        .bind(&session_record_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if session_worker.as_ref().map(|(worker,)| worker.as_str())
            != Some(usage.worker_id.as_str())
        {
            return Err(Error::Forbidden(
                "provider turn worker does not match".into(),
            ));
        }
        if state == "SETTLED" {
            let prior = ProviderTurnUsage {
                reservation_id: usage.reservation_id.clone(),
                worker_id: usage.worker_id.clone(),
                input_tokens: u64::try_from(row.try_get::<i64, _>("input_tokens")?)
                    .map_err(|_| Error::Invalid("stored turn input"))?,
                output_tokens: u64::try_from(row.try_get::<i64, _>("output_tokens")?)
                    .map_err(|_| Error::Invalid("stored turn output"))?,
                total_tokens: u64::try_from(row.try_get::<i64, _>("total_tokens")?)
                    .map_err(|_| Error::Invalid("stored turn total"))?,
                cost_micros: u64::try_from(row.try_get::<i64, _>("cost_micros")?)
                    .map_err(|_| Error::Invalid("stored turn cost"))?,
                usage_known: row.try_get::<i64, _>("usage_known")? != 0,
            };
            if prior == *usage {
                return Ok(prior);
            }
            return Err(Error::Conflict("provider turn already settled"));
        }
        if state != "RESERVED" {
            return Err(Error::Conflict("provider turn is not reserved"));
        }
        if !usage.usage_known {
            return Err(Error::Forbidden(
                "provider turn usage is unknown; operator resolution is required".into(),
            ));
        }
        let max_input: i64 = row.try_get("max_input_tokens")?;
        let max_output: i64 = row.try_get("max_output_tokens")?;
        let max_total: i64 = row.try_get("max_total_tokens")?;
        let max_cost: i64 = row.try_get("max_cost_micros")?;
        let input = i64::try_from(usage.input_tokens)
            .map_err(|_| Error::Invalid("provider turn input usage"))?;
        let output = i64::try_from(usage.output_tokens)
            .map_err(|_| Error::Invalid("provider turn output usage"))?;
        let total = i64::try_from(usage.total_tokens)
            .map_err(|_| Error::Invalid("provider turn total usage"))?;
        let cost = i64::try_from(usage.cost_micros)
            .map_err(|_| Error::Invalid("provider turn cost usage"))?;
        let exceeds =
            input > max_input || output > max_output || total > max_total || cost > max_cost;
        let error = if exceeds {
            "provider turn exceeded its authorized budget"
        } else {
            ""
        };
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE provider_turn_reservations SET state='SETTLED',input_tokens=?1,
             output_tokens=?2,total_tokens=?3,cost_micros=?4,usage_known=?5,error=?6,updated_at=?7
             WHERE reservation_id=?8",
        )
        .bind(input)
        .bind(output)
        .bind(total)
        .bind(cost)
        .bind(i64::from(usage.usage_known))
        .bind(nullable(error))
        .bind(&now)
        .bind(&usage.reservation_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE provider_sessions
             SET input_tokens=input_tokens+?1,output_tokens=output_tokens+?2,
                 total_tokens=total_tokens+?3,cost_micros=cost_micros+?4,
                 usage_known=usage_known & ?5,updated_at=?6
             WHERE session_record_id=?7",
        )
        .bind(input)
        .bind(output)
        .bind(total)
        .bind(cost)
        .bind(i64::from(usage.usage_known))
        .bind(&now)
        .bind(&session_record_id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &attempt_id,
            "provider.turn_settled",
            &format!(
                "reservation={} input={input} output={output} total={total} cost={cost}",
                usage.reservation_id
            ),
        )
        .await?;
        transaction.commit().await?;
        if exceeds {
            return Err(Error::Forbidden(error.into()));
        }
        Ok(usage.clone())
    }

    pub async fn release_provider_turn(
        &self,
        reservation_id: &str,
        session_record_id: &str,
        worker_id: &str,
        generation: i64,
        reason: &str,
    ) -> Result<(), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let binding: Option<(String, i64)> = sqlx::query_as::query_as(
            "SELECT ptr.actual_request_sha256,ptr.requests_bound
             FROM provider_turn_reservations ptr
             JOIN provider_sessions ps ON ps.session_record_id=ptr.session_record_id
             WHERE ptr.reservation_id=?1 AND ptr.session_record_id=?2
               AND ps.worker_id=?3 AND ptr.generation=?4 AND ptr.state='RESERVED'",
        )
        .bind(reservation_id)
        .bind(session_record_id)
        .bind(worker_id)
        .bind(generation)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((actual_request_sha256, requests_bound)) = binding else {
            return Err(Error::Conflict("provider turn is not reserved"));
        };
        if !actual_request_sha256.is_empty() || requests_bound != 0 {
            return Err(Error::Conflict(
                "provider turn has a bound request; explicit operator resolution is required",
            ));
        }
        let changed = sqlx::query::query(
            "UPDATE provider_turn_reservations
             SET state='RELEASED',error=?1,updated_at=?2
             WHERE reservation_id=?3 AND state='RESERVED'",
        )
        .bind(reason)
        .bind(Utc::now().to_rfc3339())
        .bind(reservation_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("provider turn is not reserved"));
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Count and sum the ceilings of every currently open provider turn.
    /// Unlike the bounded recent listing, this covers the full ledger.
    pub async fn provider_turn_reservation_budget(&self) -> Result<(u64, u64), Error> {
        let row: (i64, i64) = sqlx::query_as::query_as(
            "SELECT COUNT(*),COALESCE(SUM(max_cost_micros),0)
             FROM provider_turn_reservations WHERE state='RESERVED'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((
            u64::try_from(row.0).map_err(|_| Error::Invalid("provider turn count"))?,
            u64::try_from(row.1).map_err(|_| Error::Invalid("provider turn reservation"))?,
        ))
    }

    pub async fn provider_turn_reservations(
        &self,
        limit: usize,
    ) -> Result<Vec<ProviderTurnReservationRecord>, Error> {
        let limit = limit.clamp(1, 200) as i64;
        use sqlx::row::Row;
        let rows = sqlx::query::query(
            "SELECT reservation_id,session_record_id,attempt_id,generation,provider,model,
                    state,actual_request_sha256,max_input_tokens,max_output_tokens,
                    max_total_tokens,max_cost_micros,input_tokens,output_tokens,total_tokens,
                    cost_micros,usage_known,COALESCE(error,'') AS error,expires_at_ms,
                    max_requests,requests_bound,created_at,updated_at
             FROM provider_turn_reservations ORDER BY created_at DESC,reservation_id LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ProviderTurnReservationRecord {
                    reservation_id: row.try_get("reservation_id")?,
                    session_record_id: row.try_get("session_record_id")?,
                    attempt_id: row.try_get("attempt_id")?,
                    generation: row.try_get("generation")?,
                    provider: row.try_get("provider")?,
                    model: row.try_get("model")?,
                    state: row.try_get("state")?,
                    actual_request_sha256: row.try_get("actual_request_sha256")?,
                    max_input_tokens: u64::try_from(row.try_get::<i64, _>("max_input_tokens")?)
                        .map_err(|_| Error::Invalid("stored turn input budget"))?,
                    max_output_tokens: u64::try_from(row.try_get::<i64, _>("max_output_tokens")?)
                        .map_err(|_| Error::Invalid("stored turn output budget"))?,
                    max_total_tokens: u64::try_from(row.try_get::<i64, _>("max_total_tokens")?)
                        .map_err(|_| Error::Invalid("stored turn total budget"))?,
                    max_cost_micros: u64::try_from(row.try_get::<i64, _>("max_cost_micros")?)
                        .map_err(|_| Error::Invalid("stored turn cost budget"))?,
                    input_tokens: u64::try_from(row.try_get::<i64, _>("input_tokens")?)
                        .map_err(|_| Error::Invalid("stored turn input"))?,
                    output_tokens: u64::try_from(row.try_get::<i64, _>("output_tokens")?)
                        .map_err(|_| Error::Invalid("stored turn output"))?,
                    total_tokens: u64::try_from(row.try_get::<i64, _>("total_tokens")?)
                        .map_err(|_| Error::Invalid("stored turn total"))?,
                    cost_micros: u64::try_from(row.try_get::<i64, _>("cost_micros")?)
                        .map_err(|_| Error::Invalid("stored turn cost"))?,
                    usage_known: row.try_get::<i64, _>("usage_known")? != 0,
                    error: row.try_get("error")?,
                    expires_at_ms: row.try_get("expires_at_ms")?,
                    max_requests: u32::try_from(row.try_get::<i64, _>("max_requests")?)
                        .map_err(|_| Error::Invalid("stored turn max requests"))?,
                    requests_bound: u32::try_from(row.try_get::<i64, _>("requests_bound")?)
                        .map_err(|_| Error::Invalid("stored turn requests bound"))?,
                    created_at: row.try_get("created_at")?,
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect()
    }

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
                        ca.provider,ca.model,ca.provider_mode,r.campaign_id,ca.schema_version
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
            authorization_version,
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
        let (stored_max_sessions,): (Option<i64>,) = sqlx::query_as::query_as(
            "SELECT max_sessions FROM campaign_authorizations WHERE campaign_id=?1",
        )
        .bind(&campaign_id)
        .fetch_one(&mut *transaction)
        .await?;
        match authorization_version {
            1 => {
                let max_sessions = stored_max_sessions.ok_or(Error::Invalid(
                    "legacy provider authorization is missing its session cap",
                ))?;
                if session_count >= max_sessions {
                    return Err(Error::Forbidden(
                        "provider session budget is exhausted".into(),
                    ));
                }
            }
            2 => {
                // V2 has no lifetime provider-session cap. Session count remains
                // diagnostic telemetry.
            }
            _ => return Err(Error::Invalid("provider authorization schema version")),
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
            provider_directory: input.provider_directory.clone(),
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
              provider_session_id,provider_directory,turn_id,input_tokens,output_tokens,total_tokens,
              cost_micros,usage_known,cleanup_receipt,error,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'INTENT',NULL,?8,NULL,0,0,0,0,0,NULL,NULL,?9,?9)",
        )
        .bind(&record.session_record_id)
        .bind(&record.attempt_id)
        .bind(&record.worker_id)
        .bind(record.generation)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.provider_mode)
        .bind(nullable(&record.provider_directory))
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
            && (input.cleanup_receipt.trim().is_empty()
                || !input.usage_known
                || input.cleanup_receipt.contains("unconfirmed"))
        {
            return Err(Error::Forbidden(
                "completed provider session requires confirmed cleanup and known usage".into(),
            ));
        }
        if input.state == "FAILED"
            && (input.cleanup_receipt.trim().is_empty()
                || input.cleanup_receipt.contains("unconfirmed"))
        {
            return Err(Error::Forbidden(
                "failed provider session requires a confirmed cleanup receipt or no_external_session"
                    .into(),
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
        let campaign: (String, i64, Option<i64>, Option<i64>, Option<i64>) =
            sqlx::query_as::query_as(
                "SELECT r.campaign_id,ca.schema_version,ca.max_input_tokens,
                        ca.max_output_tokens,ca.max_cost_micros
                 FROM attempts a JOIN runs r ON r.id=a.run_id
                 JOIN campaign_authorizations ca ON ca.campaign_id=r.campaign_id
                 WHERE a.id=?1",
            )
            .bind(&attempt_id)
            .fetch_one(&mut *transaction)
            .await?;
        let (campaign_id, authorization_version, max_input, max_output, max_cost) = campaign;
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
        let existing: (i64, i64, i64, i64) = sqlx::query_as::query_as(
            "SELECT input_tokens,output_tokens,total_tokens,cost_micros
             FROM provider_sessions WHERE session_record_id=?1",
        )
        .bind(&record_id)
        .fetch_one(&mut *transaction)
        .await?;
        let session_input = existing
            .0
            .max(input.input_tokens.min(i64::MAX as u64) as i64);
        let session_output = existing
            .1
            .max(input.output_tokens.min(i64::MAX as u64) as i64);
        let session_total = existing
            .2
            .max(input.total_tokens.min(i64::MAX as u64) as i64);
        let session_cost = existing
            .3
            .max(input.cost_micros.min(i64::MAX as u64) as i64);
        let next_input = used_input.saturating_add(session_input);
        let next_output = used_output.saturating_add(session_output);
        let next_total = used_total.saturating_add(session_total);
        let next_cost = used_cost.saturating_add(session_cost);
        let over_budget = match authorization_version {
            1 => {
                let max_input = max_input.ok_or(Error::Invalid(
                    "legacy provider authorization is missing its input cap",
                ))?;
                let max_output = max_output.ok_or(Error::Invalid(
                    "legacy provider authorization is missing its output cap",
                ))?;
                let max_cost = max_cost.ok_or(Error::Invalid(
                    "legacy provider authorization is missing its cost cap",
                ))?;
                let max_total = max_input.saturating_add(max_output);
                next_input > max_input
                    || next_output > max_output
                    || next_total > max_total
                    || next_cost > max_cost
            }
            // V2 usage is recorded truthfully. Monetary authority lives in the
            // shared inference ledger; provider-session totals are evidence
            // projections, never a cumulative role ceiling.
            2 => false,
            _ => return Err(Error::Invalid("provider authorization schema version")),
        };
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
        .bind(session_input)
        .bind(session_output)
        .bind(session_total)
        .bind(session_cost)
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
        self.unresolved_provider_sessions_for_worker(None).await
    }

    /// Recovery Worker view scoped to the worker that owns the session.
    pub async fn unresolved_provider_sessions_for_worker(
        &self,
        worker_id: Option<&str>,
    ) -> Result<Vec<ProviderSessionRecord>, Error> {
        let ids: Vec<(String,)> = if let Some(worker_id) = worker_id {
            sqlx::query_as::query_as(
                "SELECT session_record_id FROM provider_sessions
                 WHERE worker_id=?1 AND state IN ('INTENT','ACTIVE','CANCEL_REQUESTED','ORPHANED')
                 ORDER BY created_at,session_record_id",
            )
            .bind(worker_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::query_as(
                "SELECT session_record_id FROM provider_sessions
                 WHERE state IN ('INTENT','ACTIVE','CANCEL_REQUESTED','ORPHANED')
                 ORDER BY created_at,session_record_id",
            )
            .fetch_all(&self.pool)
            .await?
        };
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

    pub async fn provider_sessions(&self) -> Result<Vec<ProviderSessionRecord>, Error> {
        let ids: Vec<(String,)> = sqlx::query_as::query_as(
            "SELECT session_record_id FROM provider_sessions ORDER BY updated_at DESC LIMIT 100",
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
        fixture_with_authorization(max_input, false).await
    }

    async fn v2_fixture() -> (std::path::PathBuf, Database) {
        fixture_with_authorization(100, true).await
    }

    async fn fixture_with_authorization(
        max_input: i64,
        v2: bool,
    ) -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-provider-session-{}.sqlite",
            Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture',?1,?2,'')",
        )
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
        if v2 {
            let hash = "a".repeat(64);
            sqlx::query::query(
                "INSERT INTO standing_delegations
                 (id,principal_id,audit_id,project_id,revision,content_hash,content_json,
                  revoked_at,created_at,schema_version,authority_json,authority_sha256)
                 VALUES ('grant','test','audit','project',1,?1,'',NULL,?2,2,'{}',?1)",
            )
            .bind(&hash)
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
            sqlx::query::query(
                "INSERT INTO delegation_usage(delegation_id,requests,sessions,attempts)
                 VALUES ('grant',0,0,0)",
            )
            .execute(&db.pool)
            .await
            .unwrap();
            sqlx::query::query(
                "INSERT INTO execution_policies
                 (policy_id,schema_version,authorization_id,monetary_envelope_id,scope_sha256,
                  progress_policy_json,operation_limits_json,policy_json,policy_sha256,created_at)
                 VALUES ('policy',2,'auth','envelope',?1,'{}','{}','{}',?1,?2)",
            )
            .bind(&hash)
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
            sqlx::query::query(
                "INSERT INTO campaign_authorizations
                 (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
                  authorized_paths_json,provider,model,provider_mode,delegation_id,
                  allowed_tools_json,created_at,schema_version,policy_id)
                 VALUES ('campaign','auth','R0','TRUSTED_LOCAL_PROVIDER','test','test','[\"src\"]',
                         'opencode','deepseek-v4.1-flash','build','grant','[]',?1,2,'policy')",
            )
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        } else {
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
        }
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

    async fn active_turn_fixture(max_input: i64) -> (std::path::PathBuf, Database, String) {
        let (path, db) = fixture(max_input).await;
        active_turn_fixture_with(db, path).await
    }

    async fn active_turn_fixture_v2() -> (std::path::PathBuf, Database, String) {
        let (path, db) = v2_fixture().await;
        active_turn_fixture_with(db, path).await
    }

    async fn active_turn_fixture_with(
        db: Database,
        path: std::path::PathBuf,
    ) -> (std::path::PathBuf, Database, String) {
        let now = Utc::now().timestamp_millis();
        sqlx::query::query("INSERT INTO worker_leases VALUES ('attempt','worker',1,?1)")
            .bind(now + 60_000)
            .execute(&db.pool)
            .await
            .unwrap();
        insert_session(&db, "session-turn", "attempt").await;
        db.mark_provider_active("session-turn", "worker", 1, "ses_turn", "/tmp/turn")
            .await
            .unwrap();
        (path, db, "session-turn".into())
    }

    #[tokio::test]
    async fn provider_profile_requires_live_v2_assignment_and_lease_fence() {
        let (_path, db, _session) = active_turn_fixture_v2().await;
        let now_ms = Utc::now().timestamp_millis();
        assert_eq!(
            db.assigned_provider_route("worker", "attempt", 1, now_ms)
                .await
                .unwrap(),
            ("opencode".into(), "deepseek-v4.1-flash".into())
        );

        sqlx::query::query("UPDATE attempts SET status='CANDIDATE_READY' WHERE id='attempt'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(matches!(
            db.assigned_provider_route("worker", "attempt", 1, now_ms)
                .await,
            Err(Error::Forbidden(_))
        ));

        sqlx::query::query("UPDATE attempts SET status='LEASED' WHERE id='attempt'")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query("UPDATE campaigns SET status='CANCELLED' WHERE id='campaign'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(matches!(
            db.assigned_provider_route("worker", "attempt", 1, now_ms)
                .await,
            Err(Error::Forbidden(_))
        ));

        sqlx::query::query("UPDATE campaigns SET status='AUTHORIZED' WHERE id='campaign'")
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query("UPDATE worker_leases SET expires_at_ms=1 WHERE attempt_id='attempt'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(matches!(
            db.assigned_provider_route("worker", "attempt", 1, now_ms)
                .await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
    }

    #[tokio::test]
    async fn provider_turn_binding_binds_the_exact_request_and_requires_operator_release() {
        let (_path, db, session) = active_turn_fixture(100).await;
        let digest = "a".repeat(64);
        let unbound = db
            .authorize_provider_turn(&session, "worker", 1, 50, &digest, 1)
            .await
            .unwrap();
        db.release_provider_turn(
            &unbound.reservation_id,
            "session-turn",
            "worker",
            1,
            "worker released",
        )
        .await
        .unwrap();

        let turn = db
            .authorize_provider_turn(&session, "worker", 1, 50, &digest, 1)
            .await
            .unwrap();
        let actual = "b".repeat(64);
        let binding = db
            .bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session.clone(),
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: actual.clone(),
                model: "deepseek-v4.1-flash".into(),
                path: "/v1/chat/completions".into(),
                body_bytes: 123,
                sequence: 0,
            })
            .await
            .unwrap();
        assert_eq!(binding.request_sha256, actual);
        assert!(matches!(
            db.bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session.clone(),
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: "c".repeat(64),
                model: "deepseek-v4.1-flash".into(),
                path: "/v1/chat/completions".into(),
                body_bytes: 123,
                sequence: 0,
            })
            .await,
            Err(Error::Forbidden(_) | Error::Conflict(_))
        ));
        assert!(matches!(
            db.release_provider_turn(
                &turn.reservation_id,
                &turn.session_record_id,
                "worker",
                turn.generation,
                "worker released"
            )
            .await,
            Err(Error::Conflict(_))
        ));
        db.resolve_provider_turn_reservation(&turn.reservation_id, "operator", "reviewed")
            .await
            .unwrap();
        let (state, input, output, cost): (String, i64, i64, i64) = sqlx::query_as::query_as(
            "SELECT state,input_tokens,output_tokens,cost_micros
             FROM provider_turn_reservations WHERE reservation_id=?1",
        )
        .bind(&turn.reservation_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(state, "SETTLED");
        assert!(input > 0 && output > 0 && cost > 0);
        assert!(matches!(
            db.release_provider_turn(
                &turn.reservation_id,
                &turn.session_record_id,
                "worker",
                turn.generation,
                "worker released"
            )
            .await,
            Err(Error::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn provider_turn_binding_rejects_wrong_identity_scope_and_expiry() {
        let (_path, db, session) = active_turn_fixture(1000).await;
        let digest = "a".repeat(64);
        let turn = db
            .authorize_provider_turn(&session, "worker", 1, 50, &digest, 1)
            .await
            .unwrap();
        for (model, path, worker) in [
            ("other-model", "/v1/chat/completions", "worker"),
            ("deepseek-v4.1-flash", "/v1/embeddings", "worker"),
            (
                "deepseek-v4.1-flash",
                "/v1/chat/completions",
                "other-worker",
            ),
        ] {
            let result = db
                .bind_provider_turn(&ProviderTurnBindingInput {
                    reservation_id: turn.reservation_id.clone(),
                    session_record_id: session.clone(),
                    worker_id: worker.into(),
                    generation: 1,
                    request_sha256: "b".repeat(64),
                    model: model.into(),
                    path: path.into(),
                    body_bytes: 123,
                    sequence: 0,
                })
                .await;
            assert!(
                matches!(result, Err(Error::Forbidden(_) | Error::Invalid(_))),
                "{result:?}"
            );
        }
        sqlx::query::query(
            "UPDATE provider_turn_reservations SET expires_at_ms=1 WHERE reservation_id=?1",
        )
        .bind(&turn.reservation_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let expired = db
            .bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session,
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: "b".repeat(64),
                model: "deepseek-v4.1-flash".into(),
                path: "/v1/chat/completions".into(),
                body_bytes: 123,
                sequence: 0,
            })
            .await;
        assert!(matches!(expired, Err(Error::Forbidden(_))), "{expired:?}");
    }

    #[tokio::test]
    async fn unknown_turn_usage_leaves_the_bound_reservation_reserved() {
        let (_path, db, session) = active_turn_fixture(100).await;
        let turn = db
            .authorize_provider_turn(&session, "worker", 1, 50, &"a".repeat(64), 1)
            .await
            .unwrap();
        db.bind_provider_turn(&ProviderTurnBindingInput {
            reservation_id: turn.reservation_id.clone(),
            session_record_id: session.clone(),
            worker_id: "worker".into(),
            generation: 1,
            request_sha256: "b".repeat(64),
            model: "deepseek-v4.1-flash".into(),
            path: "/v1/chat/completions".into(),
            body_bytes: 123,
            sequence: 0,
        })
        .await
        .unwrap();
        let result = db
            .settle_provider_turn(&ProviderTurnUsage {
                reservation_id: turn.reservation_id.clone(),
                worker_id: "worker".into(),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_micros: 0,
                usage_known: false,
            })
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))), "{result:?}");
        let (state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM provider_turn_reservations WHERE reservation_id=?1",
        )
        .bind(&turn.reservation_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(state, "RESERVED");
        db.resolve_provider_turn_reservation(&turn.reservation_id, "operator", "unknown usage")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn failed_terminal_preserves_prior_settled_session_usage() {
        let (_path, db, session) = active_turn_fixture(100).await;
        let turn = db
            .authorize_provider_turn(&session, "worker", 1, 50, &"a".repeat(64), 1)
            .await
            .unwrap();
        db.settle_provider_turn(&ProviderTurnUsage {
            reservation_id: turn.reservation_id,
            worker_id: "worker".into(),
            input_tokens: 4,
            output_tokens: 20,
            total_tokens: 24,
            cost_micros: 100,
            usage_known: true,
        })
        .await
        .unwrap();
        db.record_provider_terminal(&ProviderTerminal {
            session_record_id: session.clone(),
            worker_id: "worker".into(),
            generation: 1,
            state: "FAILED".into(),
            provider_session_id: "ses_prior".into(),
            turn_id: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_micros: 0,
            usage_known: false,
            cleanup_receipt: "sandbox_process_terminated_verified".into(),
            error: "failed after one settled turn".into(),
        })
        .await
        .unwrap();
        let (input, output, total, cost): (i64, i64, i64, i64) = sqlx::query_as::query_as(
            "SELECT input_tokens,output_tokens,total_tokens,cost_micros
             FROM provider_sessions WHERE session_record_id=?1",
        )
        .bind(&session)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!((input, output, total, cost), (4, 20, 24, 100));
    }

    #[tokio::test]
    async fn provider_turn_authorization_reserves_settles_and_fences_budget() {
        let (path, db, session) = active_turn_fixture(10).await;
        let digest = "a".repeat(64);
        let first = db
            .authorize_provider_turn(&session, "worker", 1, 50, &digest, 1)
            .await
            .unwrap();
        assert_eq!(first.max_output_tokens, 50);
        assert_eq!(first.max_input_tokens, 10);
        let usage = ProviderTurnUsage {
            reservation_id: first.reservation_id.clone(),
            worker_id: "worker".into(),
            input_tokens: 4,
            output_tokens: 20,
            total_tokens: 24,
            cost_micros: 100,
            usage_known: true,
        };
        db.settle_provider_turn(&usage).await.unwrap();
        db.settle_provider_turn(&usage).await.unwrap();
        let mut different = usage.clone();
        different.output_tokens = 21;
        assert!(matches!(
            db.settle_provider_turn(&different).await,
            Err(Error::Conflict(_))
        ));
        let second = db
            .authorize_provider_turn(&session, "worker", 1, 80, &digest, 1)
            .await
            .unwrap();
        assert_eq!(second.max_output_tokens, 80);
        db.settle_provider_turn(&ProviderTurnUsage {
            reservation_id: second.reservation_id,
            worker_id: "worker".into(),
            input_tokens: 4,
            output_tokens: 80,
            total_tokens: 84,
            cost_micros: 900,
            usage_known: true,
        })
        .await
        .unwrap();
        assert!(matches!(
            db.authorize_provider_turn(&session, "worker", 1, 1, &digest, 1)
                .await,
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.authorize_provider_turn(&session, "worker", 2, 1, &digest, 1)
                .await,
            Err(Error::Forbidden(_))
        ));
        sqlx::query::query("UPDATE campaigns SET status='CANCELLED' WHERE id='campaign'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(matches!(
            db.authorize_provider_turn(&session, "worker", 1, 1, &digest, 1)
                .await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn concurrent_turn_reservations_cannot_reuse_the_input_budget() {
        let (path, db, session) = active_turn_fixture(10).await;
        let digest = "a".repeat(64);
        let first = db
            .authorize_provider_turn(&session, "worker", 1, 10, &digest, 1)
            .await
            .unwrap();
        assert_eq!(first.max_input_tokens, 10);
        assert!(matches!(
            db.authorize_provider_turn(&session, "worker", 1, 10, &digest, 1)
                .await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unknown_turn_usage_fails_closed() {
        let (path, db, session) = active_turn_fixture(10).await;
        let authorization = db
            .authorize_provider_turn(&session, "worker", 1, 10, &"b".repeat(64), 1)
            .await
            .unwrap();
        let result = db
            .settle_provider_turn(&ProviderTurnUsage {
                reservation_id: authorization.reservation_id,
                worker_id: "worker".into(),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_micros: 0,
                usage_known: false,
            })
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))));
        let (state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM provider_sessions WHERE session_record_id='session-turn'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(state, "ACTIVE");
        let (reservation_state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM provider_turn_reservations WHERE session_record_id='session-turn'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(reservation_state, "RESERVED");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn failed_terminal_requires_a_cleanup_receipt() {
        let (path, db) = fixture(100).await;
        insert_session(&db, "session-failed-cleanup", "attempt").await;
        let result = db
            .record_provider_terminal(&ProviderTerminal {
                session_record_id: "session-failed-cleanup".into(),
                worker_id: "worker".into(),
                generation: 1,
                state: "FAILED".into(),
                provider_session_id: String::new(),
                turn_id: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_micros: 0,
                usage_known: false,
                cleanup_receipt: String::new(),
                error: "failed".into(),
            })
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))), "{result:?}");
        db.close().await;
        let _ = std::fs::remove_file(path);
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
    async fn codex_turn_binds_a_bounded_sequence_of_actual_requests() {
        let (_path, db, session) = active_turn_fixture(100).await;
        sqlx::query::query(
            "UPDATE provider_sessions SET provider='codex',model='gpt-6-luna' WHERE session_record_id=?1",
        )
        .bind(&session)
        .execute(&db.pool)
        .await
        .unwrap();
        let turn = db
            .authorize_provider_turn(&session, "worker", 1, 50, &"a".repeat(64), 8)
            .await
            .unwrap();
        assert_eq!(turn.max_requests, 8);
        let first = db
            .bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session.clone(),
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: "b".repeat(64),
                model: "gpt-6-luna".into(),
                path: "/v1/responses".into(),
                body_bytes: 123,
                sequence: 0,
            })
            .await
            .unwrap();
        assert_eq!(first.requests_bound, 1);
        let second = db
            .bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session.clone(),
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: "c".repeat(64),
                model: "gpt-6-luna".into(),
                path: "/v1/responses".into(),
                body_bytes: 456,
                sequence: 1,
            })
            .await
            .unwrap();
        assert_eq!(second.requests_bound, 2);
        assert!(matches!(
            db.bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session.clone(),
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: "d".repeat(64),
                model: "gpt-6-luna".into(),
                path: "/v1/responses".into(),
                body_bytes: 1,
                sequence: 3,
            })
            .await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            db.bind_provider_turn(&ProviderTurnBindingInput {
                reservation_id: turn.reservation_id.clone(),
                session_record_id: session.clone(),
                worker_id: "worker".into(),
                generation: 1,
                request_sha256: "e".repeat(64),
                model: "gpt-6-luna".into(),
                path: "/v1/responses".into(),
                body_bytes: 1,
                sequence: 1,
            })
            .await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            db.release_provider_turn(
                &turn.reservation_id,
                &turn.session_record_id,
                "worker",
                turn.generation,
                "worker released"
            )
            .await,
            Err(Error::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn expired_unbound_turn_reservation_is_reclaimed() {
        let (_path, db, session) = active_turn_fixture(100).await;
        let first = db
            .authorize_provider_turn(&session, "worker", 1, 50, &"a".repeat(64), 1)
            .await
            .unwrap();
        sqlx::query::query(
            "UPDATE provider_turn_reservations SET expires_at_ms=1 WHERE reservation_id=?1",
        )
        .bind(&first.reservation_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let second = db
            .authorize_provider_turn(&session, "worker", 1, 50, &"b".repeat(64), 1)
            .await
            .unwrap();
        assert_ne!(first.reservation_id, second.reservation_id);
        let (state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM provider_turn_reservations WHERE reservation_id=?1",
        )
        .bind(&first.reservation_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(state, "RELEASED");
    }

    #[tokio::test]
    async fn non_codex_turns_cannot_bind_multiple_requests() {
        let (_path, db, session) = active_turn_fixture(100).await;
        assert!(matches!(
            db.authorize_provider_turn(&session, "worker", 1, 50, &"a".repeat(64), 8)
                .await,
            Err(Error::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn dynamic_tool_events_are_bounded_and_unique_per_session() {
        let (path, db) = fixture(100).await;
        insert_session(&db, "session-tools", "attempt").await;
        db.mark_provider_active("session-tools", "worker", 1, "ses_tools", "/tmp/tools")
            .await
            .unwrap();
        for index in 0..8 {
            db.record_provider_tool_event(&ProviderToolEventInput {
                session_record_id: "session-tools".into(),
                worker_id: "worker".into(),
                generation: 1,
                call_id: format!("call-{index}"),
                tool: "write_file".into(),
                decision: "REQUEST".into(),
                detail: "requested".into(),
            })
            .await
            .unwrap();
        }
        assert!(matches!(
            db.record_provider_tool_event(&ProviderToolEventInput {
                session_record_id: "session-tools".into(),
                worker_id: "worker".into(),
                generation: 1,
                call_id: "call-over-budget".into(),
                tool: "write_file".into(),
                decision: "REQUEST".into(),
                detail: "requested".into(),
            })
            .await,
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.record_provider_tool_event(&ProviderToolEventInput {
                session_record_id: "session-tools".into(),
                worker_id: "worker".into(),
                generation: 1,
                call_id: "call-0".into(),
                tool: "write_file".into(),
                decision: "REQUEST".into(),
                detail: "duplicate".into(),
            })
            .await,
            Err(Error::Conflict(_))
        ));
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

    #[tokio::test]
    async fn delegation_revocation_fences_the_active_provider_session_and_lease() {
        use crate::lead::{
            Capability, ObjectiveClass, ProviderRoute, RiskClass, StandingDelegation,
        };

        let (path, db, session) = active_turn_fixture(100).await;
        let mut grant = StandingDelegation {
            id: "grant".into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Maintenance],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_attempts: 1,
            max_timeout_secs: 600,
            max_repair_cycles: 0,
            max_wall_clock_secs: 1_800,
            max_parallel: 1,
            max_sessions: 1,
            max_requests: 8,
            max_cumulative_sessions: 1,
            max_cumulative_attempts: 1,
            expires_at: Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        db.create_standing_delegation(&grant, "revocation-grant")
            .await
            .unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,
              project_config_hash,state,campaign_id,created_at,updated_at)
             VALUES ('request','project','action','grant','objective',?1,?1,'RUNNING','campaign',?2,?2)",
        )
        .bind("a".repeat(64))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();

        assert_eq!(db.provider_session(&session).await.unwrap().state, "ACTIVE");
        let revoked = db
            .revoke_standing_delegation("grant", Utc::now())
            .await
            .unwrap();
        assert!(revoked.revoked_at.is_some());
        let (lead_state,): (String,) =
            sqlx::query_as::query_as("SELECT state FROM lead_requests WHERE id='request'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(lead_state, "BLOCKED");

        let (state, error): (String, Option<String>) = sqlx::query_as::query_as(
            "SELECT state,error FROM provider_sessions WHERE session_record_id=?1",
        )
        .bind(&session)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(state, "CANCEL_REQUESTED");
        assert!(error.unwrap_or_default().contains("delegation revoked"));

        let (leases,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM worker_leases WHERE attempt_id='attempt'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(leases, 0);
        let (campaign,): (String,) =
            sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id='campaign'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(campaign, "CANCELLED");
        let (attempt,): (String,) =
            sqlx::query_as::query_as("SELECT status FROM attempts WHERE id='attempt'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(attempt, "CANCELLED");
        assert!(
            db.authorize_provider_turn(&session, "worker", 1, 100, &"a".repeat(64), 1)
                .await
                .is_err()
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderToolEventInput {
    pub session_record_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub call_id: String,
    pub tool: String,
    pub decision: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderToolEventRecord {
    pub session_record_id: String,
    pub sequence: i64,
    pub call_id: String,
    pub tool: String,
    pub decision: String,
    pub detail: String,
    pub created_at: String,
}

impl Database {
    pub async fn record_provider_tool_event(
        &self,
        input: &ProviderToolEventInput,
    ) -> Result<ProviderToolEventRecord, Error> {
        if input.call_id.trim().is_empty()
            || input.tool.trim().is_empty()
            || !matches!(
                input.decision.as_str(),
                "REQUEST" | "ALLOW" | "DENY" | "RESULT" | "ERROR"
            )
        {
            return Err(Error::Invalid("provider tool event"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row: Option<(String, i64)> = sqlx::query_as::query_as(
            "SELECT worker_id,generation FROM provider_sessions
             WHERE session_record_id=?1 AND state IN ('ACTIVE','CANCEL_REQUESTED')",
        )
        .bind(&input.session_record_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (worker_id, generation) = row.ok_or(Error::Forbidden(
            "provider tool event requires an active owned session".into(),
        ))?;
        if worker_id != input.worker_id || generation != input.generation {
            return Err(Error::Forbidden(
                "provider tool event owner mismatch".into(),
            ));
        }
        if input.decision == "REQUEST" {
            let duplicate: Option<(i64,)> = sqlx::query_as::query_as(
                "SELECT 1 FROM provider_tool_events
                 WHERE session_record_id=?1 AND call_id=?2 AND decision='REQUEST' LIMIT 1",
            )
            .bind(&input.session_record_id)
            .bind(&input.call_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if duplicate.is_some() {
                return Err(Error::Conflict(
                    "provider dynamic tool call id was already used",
                ));
            }
            let (max_turns,): (i64,) = sqlx::query_as::query_as(
                "SELECT ca.max_turns FROM provider_sessions ps
                 JOIN attempts a ON a.id=ps.attempt_id
                 JOIN runs r ON r.id=a.run_id
                 JOIN campaign_authorizations ca ON ca.campaign_id=r.campaign_id
                 WHERE ps.session_record_id=?1",
            )
            .bind(&input.session_record_id)
            .fetch_one(&mut *transaction)
            .await?;
            let (requests,): (i64,) = sqlx::query_as::query_as(
                "SELECT COUNT(*) FROM provider_tool_events
                 WHERE session_record_id=?1 AND decision='REQUEST'",
            )
            .bind(&input.session_record_id)
            .fetch_one(&mut *transaction)
            .await?;
            if requests >= max_turns.saturating_mul(8) {
                return Err(Error::Forbidden(
                    "provider dynamic tool call budget is exhausted".into(),
                ));
            }
        }
        let (sequence,): (i64,) = sqlx::query_as::query_as(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM provider_tool_events WHERE session_record_id=?1",
        )
        .bind(&input.session_record_id)
        .fetch_one(&mut *transaction)
        .await?;
        let created_at = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO provider_tool_events
             (session_record_id,sequence,call_id,tool,decision,detail,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(&input.session_record_id)
        .bind(sequence)
        .bind(&input.call_id)
        .bind(&input.tool)
        .bind(&input.decision)
        .bind(&input.detail)
        .bind(&created_at)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &input.session_record_id,
            "provider.tool_event",
            &format!(
                "call={} tool={} decision={}",
                input.call_id, input.tool, input.decision
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(ProviderToolEventRecord {
            session_record_id: input.session_record_id.clone(),
            sequence,
            call_id: input.call_id.clone(),
            tool: input.tool.clone(),
            decision: input.decision.clone(),
            detail: input.detail.clone(),
            created_at,
        })
    }

    pub async fn provider_tool_events(
        &self,
        session_record_id: &str,
    ) -> Result<Vec<ProviderToolEventRecord>, Error> {
        let rows: Vec<(String, i64, String, String, String, String, String)> =
            sqlx::query_as::query_as(
                "SELECT session_record_id,sequence,call_id,tool,decision,detail,created_at
                 FROM provider_tool_events WHERE session_record_id=?1 ORDER BY sequence",
            )
            .bind(session_record_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(
                |(session_record_id, sequence, call_id, tool, decision, detail, created_at)| {
                    ProviderToolEventRecord {
                        session_record_id,
                        sequence,
                        call_id,
                        tool,
                        decision,
                        detail,
                        created_at,
                    }
                },
            )
            .collect())
    }
}
