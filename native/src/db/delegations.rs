//! Durable standing-delegation snapshots and cumulative reservations.
//!
//! Issuance records a proposed grant; it does not activate autonomous start,
//! grant paid-provider spend or bypass protected risk approval. Core policy
//! remains the sole authority that may act on a valid grant.

use super::*;

use chrono::{DateTime, Utc};
use sqlx_sqlite::SqliteConnection;

use crate::lead::StandingDelegation;

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
        let content_json =
            serde_json::to_string(&sealed).map_err(|_| Error::Invalid("delegation encoding"))?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "create_standing_delegation",
            &sealed.content_hash,
        )
        .await?
        {
            if id == sealed.id {
                let existing = load_delegation(&mut transaction, &sealed.id).await?;
                transaction.commit().await?;
                return Ok((existing, false));
            }
            return Err(Error::Conflict(
                "idempotency key belongs to another delegation",
            ));
        }
        let project: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM projects WHERE id=?1")
                .bind(&sealed.project_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if project.is_none() {
            return Err(Error::NotFound);
        }
        let existing: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM standing_delegations WHERE id=?1")
                .bind(&sealed.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing.is_some() {
            return Err(Error::Conflict(
                "delegation id already exists with immutable content",
            ));
        }
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
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "INSERT INTO delegation_usage (delegation_id,requests,sessions,attempts)
             VALUES (?1,0,0,0)",
        )
        .bind(&sealed.id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("create_standing_delegation")
            .bind(&sealed.content_hash)
            .bind(&sealed.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &sealed.id,
            "delegation.recorded",
            "proposed grant; autonomous execution remains closed",
        )
        .await?;
        transaction.commit().await?;
        Ok((sealed, true))
    }

    pub async fn standing_delegations(&self) -> Result<Vec<StandingDelegation>, Error> {
        let ids: Vec<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM standing_delegations ORDER BY created_at,id")
                .fetch_all(&self.pool)
                .await?;
        let mut grants = Vec::with_capacity(ids.len());
        let mut connection = self.pool.acquire().await?;
        for (id,) in ids {
            grants.push(load_delegation(&mut connection, &id).await?);
        }
        Ok(grants)
    }

    pub async fn standing_delegation(&self, id: &str) -> Result<StandingDelegation, Error> {
        let mut connection = self.pool.acquire().await?;
        load_delegation(&mut connection, id).await
    }

    /// Revocation is one-way and leaves immutable grant content untouched.
    /// Repeated revocation returns the current state.
    pub async fn revoke_standing_delegation(
        &self,
        id: &str,
        at: DateTime<Utc>,
    ) -> Result<StandingDelegation, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = load_delegation(&mut transaction, id).await?;
        if current.revoked_at.is_some() {
            transaction.commit().await?;
            return Ok(current);
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
        let revoked = load_delegation(&mut transaction, id).await?;
        transaction.commit().await?;
        Ok(revoked)
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

    /// Atomically reserve cumulative request/session/attempt units. The grant
    /// must be active; ceilings are checked with checked arithmetic and the
    /// update is serialized with other Core writes. This is Core authority,
    /// not a side ledger.
    pub async fn reserve_delegation(
        &self,
        id: &str,
        requests: u32,
        sessions: u32,
        attempts: u32,
        now: DateTime<Utc>,
    ) -> Result<DelegationUsage, Error> {
        if requests == 0 && sessions == 0 && attempts == 0 {
            return Err(Error::Invalid("delegation reservation is empty"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let grant = load_delegation(&mut transaction, id).await?;
        if grant.revoked_at.is_some() {
            return Err(Error::Forbidden("delegation is revoked".into()));
        }
        if grant.expires_at <= now {
            return Err(Error::Forbidden("delegation is expired".into()));
        }
        let usage =
            reserve_usage_in_tx(&mut transaction, &grant, requests, sessions, attempts).await?;
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

pub(super) async fn reserve_usage_in_tx(
    connection: &mut SqliteConnection,
    grant: &StandingDelegation,
    requests: u32,
    sessions: u32,
    attempts: u32,
) -> Result<DelegationUsage, Error> {
    let (current_requests, current_sessions, current_attempts): (i64, i64, i64) =
        sqlx::query_as::query_as(
            "SELECT requests,sessions,attempts FROM delegation_usage WHERE delegation_id=?1",
        )
        .bind(&grant.id)
        .fetch_one(&mut *connection)
        .await?;
    let next_requests = checked_usage(current_requests, requests)?;
    let next_sessions = checked_usage(current_sessions, sessions)?;
    let next_attempts = checked_usage(current_attempts, attempts)?;
    if next_requests > i64::from(grant.max_requests)
        || next_sessions > i64::from(grant.max_cumulative_sessions)
        || next_attempts > i64::from(grant.max_cumulative_attempts)
    {
        return Err(Error::Forbidden(
            "delegation cumulative ceiling is exhausted".into(),
        ));
    }
    sqlx::query::query(
        "UPDATE delegation_usage
         SET requests=?1,sessions=?2,attempts=?3 WHERE delegation_id=?4",
    )
    .bind(next_requests)
    .bind(next_sessions)
    .bind(next_attempts)
    .bind(&grant.id)
    .execute(&mut *connection)
    .await?;
    Ok(DelegationUsage {
        delegation_id: grant.id.clone(),
        requests: u32::try_from(next_requests)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
        sessions: u32::try_from(next_sessions)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
        attempts: u32::try_from(next_attempts)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
    })
}

fn checked_usage(current: i64, added: u32) -> Result<i64, Error> {
    current
        .checked_add(i64::from(added))
        .ok_or(Error::Constraint("delegation usage overflow".into()))
}

pub(super) async fn release_usage_in_tx(
    transaction: &mut sqlx_sqlite::SqliteConnection,
    grant: &StandingDelegation,
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
    .bind(&grant.id)
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
    .bind(&grant.id)
    .execute(&mut *transaction)
    .await?;
    record_event(
        transaction,
        &grant.id,
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
        "SELECT id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at
         FROM standing_delegations WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
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
    use crate::lead::{Capability, ObjectiveClass, ProviderRoute, RiskClass, StandingDelegation};

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

    fn grant(id: &str, requests: u32) -> StandingDelegation {
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
            risk_classes: vec![RiskClass::R0, RiskClass::R1],
            autonomous_start: true,
            autonomous_risk_classes: vec![RiskClass::R0],
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_attempts: 2,
            max_timeout_secs: 300,
            max_repair_cycles: 1,
            max_wall_clock_secs: 1_800,
            max_parallel: 1,
            max_sessions: 4,
            max_requests: requests,
            max_cumulative_sessions: 8,
            max_cumulative_attempts: 16,
            expires_at: Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    #[tokio::test]
    async fn issuance_is_idempotent_immutable_and_revocation_is_one_way() {
        let (path, db) = fixture_db().await;
        let value = grant("grant-a", 2);
        let (created, inserted) = db
            .create_standing_delegation(&value, "grant-key")
            .await
            .unwrap();
        assert!(inserted);
        assert_eq!(created.content_hash, value.content_hash);
        let (replayed, inserted) = db
            .create_standing_delegation(&value, "grant-key")
            .await
            .unwrap();
        assert!(!inserted);
        assert_eq!(replayed.id, value.id);
        let mut changed = value.clone();
        changed.max_requests = 3;
        changed.content_hash = changed.compute_content_hash();
        assert!(matches!(
            db.create_standing_delegation(&changed, "grant-key").await,
            Err(Error::Conflict(_))
        ));
        assert!(
            sqlx::query::query(
                "UPDATE standing_delegations SET content_hash='tampered' WHERE id=?1"
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
            db.reserve_delegation(&value.id, 1, 1, 1, Utc::now()).await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn reservations_respect_each_cumulative_ceiling_atomically() {
        let (path, db) = fixture_db().await;
        let value = grant("grant-b", 2);
        db.create_standing_delegation(&value, "grant-key")
            .await
            .unwrap();
        let usage = db
            .reserve_delegation(&value.id, 1, 2, 2, Utc::now())
            .await
            .unwrap();
        assert_eq!((usage.requests, usage.sessions, usage.attempts), (1, 2, 2));
        assert!(matches!(
            db.reserve_delegation(&value.id, 2, 1, 1, Utc::now()).await,
            Err(Error::Forbidden(_))
        ));
        assert_eq!(db.delegation_usage(&value.id).await.unwrap().requests, 1);

        let left = db.clone();
        let right = db.clone();
        let (a, b) = tokio::join!(
            async move {
                left.reserve_delegation("grant-b", 1, 1, 1, Utc::now())
                    .await
            },
            async move {
                right
                    .reserve_delegation("grant-b", 1, 1, 1, Utc::now())
                    .await
            }
        );
        assert!(a.is_ok() ^ b.is_ok(), "exactly one reservation wins");
        assert_eq!(db.delegation_usage(&value.id).await.unwrap().requests, 2);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn expired_and_unknown_grants_fail_closed() {
        let (path, db) = fixture_db().await;
        let mut expired = grant("grant-expired", 1);
        expired.expires_at = Utc::now() - chrono::Duration::seconds(1);
        expired.content_hash = expired.compute_content_hash();
        assert!(matches!(
            db.create_standing_delegation(&expired, "expired-key").await,
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.reserve_delegation("missing", 1, 1, 1, Utc::now()).await,
            Err(Error::NotFound)
        ));
        let value = grant("grant-c", 1);
        db.create_standing_delegation(&value, "valid-key")
            .await
            .unwrap();
        assert!(matches!(
            db.reserve_delegation(&value.id, 0, 0, 0, Utc::now()).await,
            Err(Error::Invalid(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
