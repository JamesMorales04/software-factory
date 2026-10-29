//! Durable standing-delegation snapshots and usage telemetry.
//!
//! Issuance records a proposed grant; it does not activate autonomous start,
//! grant paid-provider spend or bypass protected risk approval. Core policy
//! remains the sole authority that may act on a valid grant.
//!
//! Cumulative request/session/attempt counts are diagnosis telemetry. They are
//! never admission ceilings: a legitimate Campaign is not terminated because a
//! historical counter is large. Revocation, expiry, scope, route and lease
//! fencing remain authoritative.

use super::*;

use chrono::{DateTime, Utc};
use sqlx_sqlite::SqliteConnection;

use crate::lead::{StandingDelegation, StandingDelegationV2};

/// Version-neutral revocation receipt. It never decodes or reinterprets the
/// immutable grant content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationRevocation {
    pub id: String,
    pub schema_version: u32,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationUsage {
    pub delegation_id: String,
    pub requests: u32,
    pub sessions: u32,
    pub attempts: u32,
}

impl Database {
    /// Record a proposed standing delegation. The content hash is sealed by
    /// Core if the caller left it empty; a supplied mismatching hash is
    /// rejected by domain validation. Replaying the same idempotency key and
    /// hash returns the existing grant. No autonomous execution is enabled.
    pub async fn create_standing_delegation(
        &self,
        grant: &StandingDelegation,
        idempotency_key: &str,
    ) -> Result<(StandingDelegation, bool), Error> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(Error::Invalid("delegation idempotency key"));
        }
        let mut sealed = grant.clone();
        if sealed.content_hash.is_empty() {
            sealed.content_hash = sealed.compute_content_hash();
        }
        sealed.validate(Utc::now())?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let result =
            create_standing_delegation_in_tx(&mut transaction, &sealed, idempotency_key).await?;
        transaction.commit().await?;
        Ok(result)
    }

    /// Versioned delegation identities for operator projections. Callers load
    /// the exact V1 or V2 record; legacy caps are never projected as V2.
    pub async fn standing_delegation_versions(&self) -> Result<Vec<(String, u32)>, Error> {
        let rows: Vec<(String, i64)> = sqlx::query_as::query_as(
            "SELECT id,schema_version FROM standing_delegations ORDER BY created_at,id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(id, version)| {
                let version = u32::try_from(version)
                    .map_err(|_| Error::Invalid("stored delegation schema version"))?;
                Ok((id, version))
            })
            .collect()
    }

    pub async fn standing_delegation(&self, id: &str) -> Result<StandingDelegation, Error> {
        let mut connection = self.pool.acquire().await?;
        load_delegation(&mut connection, id).await
    }

    /// Record an Execution Policy V2 authority. Replaying the same idempotency
    /// key and content returns the existing grant.
    pub async fn create_standing_delegation_v2(
        &self,
        grant: &StandingDelegationV2,
        idempotency_key: &str,
    ) -> Result<(StandingDelegationV2, bool), Error> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(Error::Invalid("delegation idempotency key"));
        }
        let mut sealed = grant.clone();
        if sealed.content_hash.is_empty() {
            sealed.content_hash = sealed.compute_content_hash();
        }
        sealed.validate(Utc::now())?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let result =
            create_standing_delegation_v2_in_tx(&mut transaction, &sealed, idempotency_key).await?;
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn standing_delegation_v2(&self, id: &str) -> Result<StandingDelegationV2, Error> {
        let mut connection = self.pool.acquire().await?;
        load_delegation_v2(&mut connection, id).await
    }

    /// Revocation is one-way and leaves immutable grant content untouched.
    /// Repeated revocation returns the current state.
    pub async fn revoke_standing_delegation(
        &self,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<DelegationRevocation, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current: Option<(i64, Option<String>)> = sqlx::query_as::query_as(
            "SELECT schema_version,revoked_at FROM standing_delegations WHERE id=?1",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (schema_version, revoked_at) = current.ok_or(Error::NotFound)?;
        let schema_version = u32::try_from(schema_version)
            .map_err(|_| Error::Invalid("stored delegation schema version"))?;
        if let Some(revoked_at) = revoked_at {
            let revoked_at = DateTime::parse_from_rfc3339(&revoked_at)
                .map_err(|_| Error::Invalid("stored delegation revocation encoding"))?
                .with_timezone(&Utc);
            transaction.commit().await?;
            return Ok(DelegationRevocation {
                id: id.to_owned(),
                schema_version,
                revoked_at: Some(revoked_at),
            });
        }
        let changed = sqlx::query::query(
            "UPDATE standing_delegations SET revoked_at=?1 WHERE id=?2 AND revoked_at IS NULL",
        )
        .bind(at.to_rfc3339())
        .bind(id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed == 1 {
            sqlx::query::query(
                "UPDATE lead_requests SET state='BLOCKED',failure_reason='delegation revoked',updated_at=?1
                 WHERE delegation_id=?2 AND state IN ('SUBMITTED','ASSESSING','PLANNED','FROZEN','READY','RUNNING')",
            )
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "UPDATE campaigns SET status='CANCELLED'
                 WHERE status IN ('AUTHORIZED','RUNNING')
                   AND action_id IN (SELECT action_id FROM lead_requests WHERE delegation_id=?1)",
            )
            .bind(id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "UPDATE runs SET status='CANCELLED'
                 WHERE status IN ('QUEUED','RUNNING')
                   AND campaign_id IN (
                     SELECT c.id FROM campaigns c
                     JOIN lead_requests l ON l.action_id=c.action_id
                     WHERE l.delegation_id=?1)",
            )
            .bind(id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "UPDATE attempts SET status='CANCELLED'
                 WHERE status IN ('QUEUED','LEASED')
                   AND run_id IN (
                     SELECT r.id FROM runs r
                     JOIN campaigns c ON c.id=r.campaign_id
                     JOIN lead_requests l ON l.action_id=c.action_id
                     WHERE l.delegation_id=?1)",
            )
            .bind(id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "WITH linked AS (
                   SELECT c.id AS campaign_id FROM campaigns c
                   JOIN lead_requests l ON l.action_id=c.action_id
                   WHERE l.delegation_id=?1
                 )
                 UPDATE lead_tasks SET status='FAILED',updated_at=?2
                 WHERE campaign_id IN (SELECT campaign_id FROM linked)
                   AND status IN ('PENDING','RUNNING')",
            )
            .bind(id)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "WITH linked AS (
                   SELECT c.id AS campaign_id FROM campaigns c
                   JOIN lead_requests l ON l.action_id=c.action_id
                   WHERE l.delegation_id=?1
                 )
                 UPDATE lead_integrations SET status='FAILED',updated_at=?2
                 WHERE campaign_id IN (SELECT campaign_id FROM linked)
                   AND status IN ('QUEUED','RUNNING')",
            )
            .bind(id)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "UPDATE provider_sessions
                 SET state='CANCEL_REQUESTED',
                     error='delegation revoked; external termination unconfirmed',
                     updated_at=?2
                 WHERE state IN ('INTENT','ACTIVE','ORPHANED')
                   AND attempt_id IN (
                     SELECT a.id FROM attempts a
                     JOIN runs r ON r.id=a.run_id
                     JOIN campaigns c ON c.id=r.campaign_id
                     JOIN lead_requests l ON l.action_id=c.action_id
                     WHERE l.delegation_id=?1
                   )",
            )
            .bind(id)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "DELETE FROM worker_leases
                 WHERE attempt_id IN (
                   SELECT a.id FROM attempts a
                   JOIN runs r ON r.id=a.run_id
                   JOIN campaigns c ON c.id=r.campaign_id
                   JOIN lead_requests l ON l.action_id=c.action_id
                   WHERE l.delegation_id=?1)",
            )
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        }
        record_event(
            &mut transaction,
            id,
            "delegation.revoked",
            "one-way revocation recorded; open Lead work blocked and Campaign work fenced",
        )
        .await?;
        transaction.commit().await?;
        Ok(DelegationRevocation {
            id: id.to_owned(),
            schema_version,
            revoked_at: Some(at),
        })
    }

    pub async fn delegation_usage(&self, id: &str) -> Result<DelegationUsage, Error> {
        let row: Option<(i64, i64, i64)> = sqlx::query_as::query_as(
            "SELECT requests,sessions,attempts FROM delegation_usage WHERE delegation_id=?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let (requests, sessions, attempts) = row.ok_or(Error::NotFound)?;
        Ok(DelegationUsage {
            delegation_id: id.to_owned(),
            requests: u32::try_from(requests)
                .map_err(|_| Error::Constraint("usage overflow".into()))?,
            sessions: u32::try_from(sessions)
                .map_err(|_| Error::Constraint("usage overflow".into()))?,
            attempts: u32::try_from(attempts)
                .map_err(|_| Error::Constraint("usage overflow".into()))?,
        })
    }

    /// Atomically record cumulative request/session/attempt activity. The
    /// grant must remain active, but the counters never refuse legitimate work.
    /// Monetary authority lives in the shared inference ledger, not here.
    pub async fn record_delegation_usage(
        &self,
        id: &str,
        requests: u32,
        sessions: u32,
        attempts: u32,
        now: DateTime<Utc>,
    ) -> Result<DelegationUsage, Error> {
        if requests == 0 && sessions == 0 && attempts == 0 {
            return Err(Error::Invalid("delegation usage record is empty"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let grant = load_delegation_v2(&mut transaction, id).await?;
        if grant.revoked_at.is_some() {
            return Err(Error::Forbidden("delegation is revoked".into()));
        }
        if grant.expires_at <= now {
            return Err(Error::Forbidden("delegation is expired".into()));
        }
        let usage =
            record_usage_in_tx(&mut transaction, &grant.id, requests, sessions, attempts).await?;
        record_event(
            &mut transaction,
            id,
            "delegation.reserved",
            &format!(
                "requests={} sessions={} attempts={}",
                usage.requests, usage.sessions, usage.attempts
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(usage)
    }
}

pub(super) async fn create_standing_delegation_in_tx(
    connection: &mut SqliteConnection,
    sealed: &StandingDelegation,
    idempotency_key: &str,
) -> Result<(StandingDelegation, bool), Error> {
    if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
        return Err(Error::Invalid("delegation idempotency key"));
    }
    sealed.validate(Utc::now())?;
    if let Some(id) = replay(
        connection,
        idempotency_key,
        "create_standing_delegation",
        &sealed.content_hash,
    )
    .await?
    {
        if id == sealed.id {
            let existing = load_delegation(connection, &sealed.id).await?;
            return Ok((existing, false));
        }
        return Err(Error::Conflict(
            "idempotency key belongs to another delegation",
        ));
    }
    let project: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM projects WHERE id=?1")
            .bind(&sealed.project_id)
            .fetch_optional(&mut *connection)
            .await?;
    if project.is_none() {
        return Err(Error::NotFound);
    }
    let existing: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM standing_delegations WHERE id=?1")
            .bind(&sealed.id)
            .fetch_optional(&mut *connection)
            .await?;
    if existing.is_some() {
        return Err(Error::Conflict(
            "delegation id already exists with immutable content",
        ));
    }
    let content_json =
        serde_json::to_string(sealed).map_err(|_| Error::Invalid("delegation encoding"))?;
    sqlx::query::query(
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,?8)",
    )
    .bind(&sealed.id)
    .bind(&sealed.principal_id)
    .bind(&sealed.audit_id)
    .bind(&sealed.project_id)
    .bind(sealed.revision)
    .bind(&sealed.content_hash)
    .bind(&content_json)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "INSERT INTO delegation_usage (delegation_id,requests,sessions,attempts)
         VALUES (?1,0,0,0)",
    )
    .bind(&sealed.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
        .bind(idempotency_key)
        .bind("create_standing_delegation")
        .bind(&sealed.content_hash)
        .bind(&sealed.id)
        .execute(&mut *connection)
        .await?;
    record_event(
        connection,
        &sealed.id,
        "delegation.recorded",
        "proposed grant; autonomous execution remains closed",
    )
    .await?;
    Ok((sealed.clone(), true))
}

/// Store an Execution Policy V2 authority. The legacy `content_json` column is
/// intentionally empty for a V2 row so the V1 decoder fails closed instead of
/// reinterpreting V2 bytes. The V2 authority JSON and hash live in dedicated
/// columns and are immutable.
pub(super) async fn create_standing_delegation_v2_in_tx(
    connection: &mut SqliteConnection,
    sealed: &StandingDelegationV2,
    idempotency_key: &str,
) -> Result<(StandingDelegationV2, bool), Error> {
    if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
        return Err(Error::Invalid("delegation idempotency key"));
    }
    sealed.validate(Utc::now())?;
    if let Some(id) = replay(
        connection,
        idempotency_key,
        "create_standing_delegation_v2",
        &sealed.content_hash,
    )
    .await?
    {
        if id == sealed.id {
            let existing = load_delegation_v2(connection, &sealed.id).await?;
            return Ok((existing, false));
        }
        return Err(Error::Conflict(
            "idempotency key belongs to another delegation",
        ));
    }
    let project: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM projects WHERE id=?1")
            .bind(&sealed.project_id)
            .fetch_optional(&mut *connection)
            .await?;
    if project.is_none() {
        return Err(Error::NotFound);
    }
    let existing: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM standing_delegations WHERE id=?1")
            .bind(&sealed.id)
            .fetch_optional(&mut *connection)
            .await?;
    if existing.is_some() {
        return Err(Error::Conflict(
            "delegation id already exists with immutable content",
        ));
    }
    let authority_json =
        serde_json::to_string(sealed).map_err(|_| Error::Invalid("delegation encoding"))?;
    let authority_sha256 = sha256_hex(authority_json.as_bytes());
    sqlx::query::query(
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at,
          created_at,schema_version,authority_json,authority_sha256)
         VALUES (?1,?2,?3,?4,?5,?6,'',NULL,?7,2,?8,?9)",
    )
    .bind(&sealed.id)
    .bind(&sealed.principal_id)
    .bind(&sealed.audit_id)
    .bind(&sealed.project_id)
    .bind(sealed.revision)
    .bind(&sealed.content_hash)
    .bind(Utc::now().to_rfc3339())
    .bind(&authority_json)
    .bind(&authority_sha256)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "INSERT INTO delegation_usage (delegation_id,requests,sessions,attempts)
         VALUES (?1,0,0,0)",
    )
    .bind(&sealed.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
        .bind(idempotency_key)
        .bind("create_standing_delegation_v2")
        .bind(&sealed.content_hash)
        .bind(&sealed.id)
        .execute(&mut *connection)
        .await?;
    record_event(
        connection,
        &sealed.id,
        "delegation.recorded",
        "V2 authority recorded; non-autonomous scope with technical operation limits",
    )
    .await?;
    Ok((sealed.clone(), true))
}

pub(super) async fn load_delegation_v2(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<StandingDelegationV2, Error> {
    use sqlx::row::Row;

    let row = sqlx::query::query(
        "SELECT id,principal_id,audit_id,project_id,revision,content_hash,authority_json,
                authority_sha256,revoked_at,schema_version
         FROM standing_delegations WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    let schema_version: i64 = row.try_get("schema_version")?;
    if schema_version != 2 {
        return Err(Error::Forbidden(
            "legacy V1 delegation is historical evidence; create a linked V2 request instead"
                .into(),
        ));
    }
    let id: String = row.try_get("id")?;
    let principal_id: String = row.try_get("principal_id")?;
    let audit_id: String = row.try_get("audit_id")?;
    let project_id: String = row.try_get("project_id")?;
    let revision: i64 = row.try_get("revision")?;
    let content_hash: String = row.try_get("content_hash")?;
    let authority_json: String = row.try_get("authority_json")?;
    let authority_sha256: String = row.try_get("authority_sha256")?;
    let revoked_at: Option<String> = row.try_get("revoked_at")?;
    if sha256_hex(authority_json.as_bytes()) != authority_sha256 {
        return Err(Error::Invalid(
            "stored V2 delegation authority digest mismatch",
        ));
    }
    let mut grant: StandingDelegationV2 = serde_json::from_str(&authority_json)
        .map_err(|_| Error::Invalid("stored V2 delegation encoding"))?;
    if grant.id != id
        || grant.principal_id != principal_id
        || grant.audit_id != audit_id
        || grant.project_id != project_id
        || i64::from(grant.revision) != revision
        || grant.content_hash != content_hash
        || grant.compute_content_hash() != content_hash
    {
        return Err(Error::Invalid(
            "stored V2 delegation does not match its identity",
        ));
    }
    grant.revoked_at = revoked_at
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| Error::Invalid("stored delegation revocation encoding"))?
        .map(|value| value.with_timezone(&Utc));
    Ok(grant)
}

/// Record usage without a ceiling. This is telemetry and diagnosis, never an
/// admission decision.
pub(super) async fn record_usage_in_tx(
    connection: &mut SqliteConnection,
    delegation_id: &str,
    requests: u32,
    sessions: u32,
    attempts: u32,
) -> Result<DelegationUsage, Error> {
    let (current_requests, current_sessions, current_attempts): (i64, i64, i64) =
        sqlx::query_as::query_as(
            "SELECT requests,sessions,attempts FROM delegation_usage WHERE delegation_id=?1",
        )
        .bind(delegation_id)
        .fetch_one(&mut *connection)
        .await?;
    let next_requests = checked_usage(current_requests, requests)?;
    let next_sessions = checked_usage(current_sessions, sessions)?;
    let next_attempts = checked_usage(current_attempts, attempts)?;
    sqlx::query::query(
        "UPDATE delegation_usage
         SET requests=?1,sessions=?2,attempts=?3 WHERE delegation_id=?4",
    )
    .bind(next_requests)
    .bind(next_sessions)
    .bind(next_attempts)
    .bind(delegation_id)
    .execute(&mut *connection)
    .await?;
    Ok(DelegationUsage {
        delegation_id: delegation_id.to_owned(),
        requests: u32::try_from(next_requests)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
        sessions: u32::try_from(next_sessions)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
        attempts: u32::try_from(next_attempts)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
    })
}

pub(super) async fn record_usage_v2_in_tx(
    connection: &mut SqliteConnection,
    grant: &StandingDelegationV2,
    requests: u32,
    sessions: u32,
    attempts: u32,
) -> Result<DelegationUsage, Error> {
    record_usage_in_tx(connection, &grant.id, requests, sessions, attempts).await
}

fn checked_usage(current: i64, added: u32) -> Result<i64, Error> {
    current
        .checked_add(i64::from(added))
        .ok_or(Error::Constraint("delegation usage overflow".into()))
}

pub(super) async fn release_usage_in_tx(
    transaction: &mut sqlx_sqlite::SqliteConnection,
    delegation_id: &str,
    requests: u32,
    sessions: u32,
    attempts: u32,
) -> Result<(), Error> {
    if requests == 0 && sessions == 0 && attempts == 0 {
        return Err(Error::Invalid("delegation release is empty"));
    }
    let row: Option<(i64, i64, i64)> = sqlx::query_as::query_as(
        "SELECT requests,sessions,attempts FROM delegation_usage WHERE delegation_id=?1",
    )
    .bind(delegation_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let (saved_requests, saved_sessions, saved_attempts) = row.ok_or(Error::NotFound)?;
    let requests = saved_requests
        .checked_sub(i64::from(requests))
        .filter(|value| *value >= 0)
        .ok_or(Error::Constraint(
            "delegation request release underflow".to_string(),
        ))?;
    let sessions = saved_sessions
        .checked_sub(i64::from(sessions))
        .filter(|value| *value >= 0)
        .ok_or(Error::Constraint(
            "delegation session release underflow".to_string(),
        ))?;
    let attempts = saved_attempts
        .checked_sub(i64::from(attempts))
        .filter(|value| *value >= 0)
        .ok_or(Error::Constraint(
            "delegation attempt release underflow".to_string(),
        ))?;
    sqlx::query::query(
        "UPDATE delegation_usage SET requests=?1,sessions=?2,attempts=?3 WHERE delegation_id=?4",
    )
    .bind(requests)
    .bind(sessions)
    .bind(attempts)
    .bind(delegation_id)
    .execute(&mut *transaction)
    .await?;
    record_event(
        transaction,
        delegation_id,
        "delegation.released",
        &format!("requests={requests} sessions={sessions} attempts={attempts}"),
    )
    .await?;
    Ok(())
}

pub(super) async fn load_delegation(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<StandingDelegation, Error> {
    use sqlx::row::Row;

    let row = sqlx::query::query(
        "SELECT id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at,
                schema_version
         FROM standing_delegations WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    let schema_version: i64 = row.try_get("schema_version")?;
    if schema_version != 1 {
        return Err(Error::Forbidden(
            "V2 delegation cannot be decoded through the legacy V1 evidence path".into(),
        ));
    }
    let id: String = row.try_get("id")?;
    let principal_id: String = row.try_get("principal_id")?;
    let audit_id: String = row.try_get("audit_id")?;
    let project_id: String = row.try_get("project_id")?;
    let revision: i64 = row.try_get("revision")?;
    let content_hash: String = row.try_get("content_hash")?;
    let content_json: String = row.try_get("content_json")?;
    let revoked_at: Option<String> = row.try_get("revoked_at")?;
    let mut grant: StandingDelegation = serde_json::from_str(&content_json)
        .map_err(|_| Error::Invalid("stored delegation encoding"))?;
    if grant.id != id
        || grant.principal_id != principal_id
        || grant.audit_id != audit_id
        || grant.project_id != project_id
        || i64::from(grant.revision) != revision
        || grant.content_hash != content_hash
        || grant.compute_content_hash() != content_hash
    {
        return Err(Error::Invalid(
            "stored delegation does not match its identity",
        ));
    }
    grant.revoked_at = revoked_at
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| Error::Invalid("stored delegation revocation encoding"))?
        .map(|value| value.with_timezone(&Utc));
    Ok(grant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_policy::{OperationLimits, ProgressPolicy};
    use crate::lead::{
        Capability, ObjectiveClass, ProviderRoute, RiskClass, StandingDelegation,
        StandingDelegationV2,
    };
    use std::num::NonZeroU32;

    async fn fixture_db() -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-delegation-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query("INSERT INTO projects(id,repository,base_commit,created_at,identity) VALUES (?1,?2,?3,?4,'')")
            .bind("project")
            .bind("/fixture")
            .bind("base")
            .bind(Utc::now().to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();
        (path, db)
    }

    fn operations() -> OperationLimits {
        OperationLimits {
            model_request_timeout_secs: NonZeroU32::new(300).unwrap(),
            tool_call_timeout_secs: NonZeroU32::new(600).unwrap(),
            max_request_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_response_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_generation_tokens: NonZeroU32::new(8_192).unwrap(),
            max_in_flight_operations: NonZeroU32::new(4).unwrap(),
        }
    }

    fn progress() -> ProgressPolicy {
        ProgressPolicy {
            diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
            observation_timeout_secs: NonZeroU32::new(300).unwrap(),
        }
    }

    fn grant_v2(id: &str) -> StandingDelegationV2 {
        let mut grant = StandingDelegationV2 {
            id: id.into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit],
            risk_classes: vec![RiskClass::R0, RiskClass::R1],
            autonomous_start: true,
            autonomous_risk_classes: vec![RiskClass::R0],
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_timeout_secs: 300,
            max_parallel: 2,
            operations: operations(),
            progress: progress(),
            expires_at: Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    fn legacy_grant(id: &str) -> StandingDelegation {
        let mut grant = StandingDelegation {
            id: id.into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit],
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_requests: 2,
            max_attempts: 2,
            max_timeout_secs: 300,
            max_repair_cycles: 1,
            max_wall_clock_secs: 1_800,
            max_parallel: 1,
            max_sessions: 4,
            max_cumulative_sessions: 8,
            max_cumulative_attempts: 16,
            expires_at: Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    #[tokio::test]
    async fn v2_issuance_is_idempotent_immutable_and_revocation_is_one_way() {
        let (path, db) = fixture_db().await;
        let value = grant_v2("grant-a");
        let (created, inserted) = db
            .create_standing_delegation_v2(&value, "grant-key")
            .await
            .unwrap();
        assert!(inserted);
        assert_eq!(created.content_hash, value.content_hash);
        let (replayed, inserted) = db
            .create_standing_delegation_v2(&value, "grant-key")
            .await
            .unwrap();
        assert!(!inserted);
        assert_eq!(replayed.id, value.id);
        let mut changed = value.clone();
        changed.max_parallel = 1;
        changed.content_hash = changed.compute_content_hash();
        assert!(matches!(
            db.create_standing_delegation_v2(&changed, "grant-key")
                .await,
            Err(Error::Conflict(_))
        ));
        assert!(
            sqlx::query::query(
                "UPDATE standing_delegations SET authority_sha256='tampered' WHERE id=?1"
            )
            .bind(&value.id)
            .execute(&db.pool)
            .await
            .is_err()
        );
        let revoked = db
            .revoke_standing_delegation(&value.id, Utc::now())
            .await
            .unwrap();
        assert!(revoked.revoked_at.is_some());
        let again = db
            .revoke_standing_delegation(&value.id, Utc::now())
            .await
            .unwrap();
        assert_eq!(again.revoked_at, revoked.revoked_at);
        assert!(matches!(
            db.record_delegation_usage(&value.id, 1, 1, 1, Utc::now())
                .await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn usage_counters_are_telemetry_and_never_terminate_work() {
        let (path, db) = fixture_db().await;
        let value = grant_v2("grant-b");
        db.create_standing_delegation_v2(&value, "grant-key")
            .await
            .unwrap();
        for _ in 0..50 {
            db.record_delegation_usage(&value.id, 1, 1, 1, Utc::now())
                .await
                .unwrap();
        }
        let usage = db.delegation_usage(&value.id).await.unwrap();
        assert_eq!(
            (usage.requests, usage.sessions, usage.attempts),
            (50, 50, 50)
        );
        assert_eq!(db.standing_delegation_v2(&value.id).await.unwrap(), value);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn legacy_v1_bytes_round_trip_and_never_decode_as_v2() {
        let (path, db) = fixture_db().await;
        let legacy = legacy_grant("grant-v1");
        db.create_standing_delegation(&legacy, "legacy-key")
            .await
            .unwrap();
        let loaded = db.standing_delegation(&legacy.id).await.unwrap();
        assert_eq!(loaded.content_hash, legacy.content_hash);
        assert_eq!(loaded.max_attempts, legacy.max_attempts);
        assert!(matches!(
            db.standing_delegation_v2(&legacy.id).await,
            Err(Error::Forbidden(_))
        ));
        let value = grant_v2("grant-v2");
        db.create_standing_delegation_v2(&value, "v2-key")
            .await
            .unwrap();
        assert!(matches!(
            db.standing_delegation(&value.id).await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn expired_and_unknown_grants_fail_closed() {
        let (path, db) = fixture_db().await;
        let mut expired = grant_v2("grant-expired");
        expired.expires_at = Utc::now() - chrono::Duration::seconds(1);
        expired.content_hash = expired.compute_content_hash();
        assert!(matches!(
            db.create_standing_delegation_v2(&expired, "expired-key")
                .await,
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.record_delegation_usage("missing", 1, 1, 1, Utc::now())
                .await,
            Err(Error::NotFound)
        ));
        let value = grant_v2("grant-c");
        db.create_standing_delegation_v2(&value, "valid-key")
            .await
            .unwrap();
        assert!(matches!(
            db.record_delegation_usage(&value.id, 0, 0, 0, Utc::now())
                .await,
            Err(Error::Invalid(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
