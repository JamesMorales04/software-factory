use super::{Database, Error};
use serde::{Deserialize, Serialize};
use sqlx::row::Row;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSessionRecord {
    pub id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub last_activity_at_ms: i64,
    pub expires_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct NewDeviceSession<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub session_token_sha256: &'a str,
    pub csrf_token_sha256: &'a str,
    pub now_ms: i64,
    pub expires_at_ms: i64,
    pub created_by: &'a str,
}

impl Database {
    /// Persist the first Owner device and claim the one-time owner bootstrap atomically.
    pub async fn create_owner_device_session(
        &self,
        id: &str,
        name: &str,
        session_token_sha256: &str,
        csrf_token_sha256: &str,
        now_ms: i64,
        expires_at_ms: i64,
    ) -> Result<DeviceSessionRecord, Error> {
        validate_session_input(
            id,
            name,
            session_token_sha256,
            csrf_token_sha256,
            now_ms,
            expires_at_ms,
        )?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query::query(
            "INSERT INTO device_sessions
             (id,device_name,session_token_sha256,csrf_token_sha256,created_at_ms,last_activity_at_ms,
              expires_at_ms,created_by)
             VALUES (?1,?2,?3,?4,?5,?5,?6,'owner-bootstrap')",
        )
        .bind(id)
        .bind(name.trim())
        .bind(session_token_sha256)
        .bind(csrf_token_sha256)
        .bind(now_ms)
        .bind(expires_at_ms)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "INSERT INTO owner_bootstrap_claim(id,session_id,claimed_at_ms) VALUES (1,?1,?2)",
        )
        .bind(id)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
             VALUES (?1,'owner-bootstrap','created',?2)",
        )
        .bind(id)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(DeviceSessionRecord {
            id: id.to_owned(),
            name: name.trim().to_owned(),
            created_at_ms: now_ms,
            last_activity_at_ms: now_ms,
            expires_at_ms,
            revoked_at_ms: None,
        })
    }

    /// Return true only while first-run Owner bootstrap has not been claimed.
    pub async fn owner_bootstrap_available(&self) -> Result<bool, Error> {
        let claimed: Option<i64> =
            sqlx::query_scalar::query_scalar("SELECT id FROM owner_bootstrap_claim WHERE id=1")
                .fetch_optional(&self.pool)
                .await?;
        Ok(claimed.is_none())
    }

    /// Revoke all sessions and reopen local Owner enrollment in one transaction.
    pub async fn reset_owner_bootstrap(&self, actor: &str, now_ms: i64) -> Result<u64, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar::query_scalar(
            "SELECT id FROM device_sessions WHERE revoked_at_ms IS NULL",
        )
        .fetch_all(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE device_sessions SET revoked_at_ms=?1 WHERE revoked_at_ms IS NULL",
        )
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        for id in &ids {
            sqlx::query::query(
                "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
                 VALUES (?1,?2,'revoked_all',?3)",
            )
            .bind(id)
            .bind(actor)
            .bind(now_ms)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query::query("DELETE FROM owner_bootstrap_claim WHERE id=1")
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(ids.len() as u64)
    }

    /// Persist only token digests and create an audit record for a new device session.
    pub async fn create_device_session(
        &self,
        input: NewDeviceSession<'_>,
    ) -> Result<DeviceSessionRecord, Error> {
        let NewDeviceSession {
            id,
            name,
            session_token_sha256,
            csrf_token_sha256,
            now_ms,
            expires_at_ms,
            created_by,
        } = input;
        validate_session_input(
            id,
            name,
            session_token_sha256,
            csrf_token_sha256,
            now_ms,
            expires_at_ms,
        )?;
        if created_by.is_empty() {
            return Err(Error::Invalid("device session actor"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query::query(
            "INSERT INTO device_sessions
             (id,device_name,session_token_sha256,csrf_token_sha256,created_at_ms,last_activity_at_ms,
              expires_at_ms,created_by)
             VALUES (?1,?2,?3,?4,?5,?5,?6,?7)",
        )
        .bind(id)
        .bind(name.trim())
        .bind(session_token_sha256)
        .bind(csrf_token_sha256)
        .bind(now_ms)
        .bind(expires_at_ms)
        .bind(created_by)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
             VALUES (?1,?2,'created',?3)",
        )
        .bind(id)
        .bind(created_by)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(DeviceSessionRecord {
            id: id.to_owned(),
            name: name.trim().to_owned(),
            created_at_ms: now_ms,
            last_activity_at_ms: now_ms,
            expires_at_ms,
            revoked_at_ms: None,
        })
    }

    /// Find a live session using a SHA-256 digest; raw credentials never enter storage.
    pub async fn device_session_by_token_hash(
        &self,
        token_hash: &str,
        now_ms: i64,
    ) -> Result<Option<DeviceSessionRecord>, Error> {
        let row = sqlx::query::query(
            "SELECT id,device_name,created_at_ms,last_activity_at_ms,expires_at_ms,revoked_at_ms
             FROM device_sessions
             WHERE session_token_sha256=?1 AND revoked_at_ms IS NULL AND expires_at_ms>?2",
        )
        .bind(token_hash)
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(DeviceSessionRecord {
                id: row.try_get("id")?,
                name: row.try_get("device_name")?,
                created_at_ms: row.try_get("created_at_ms")?,
                last_activity_at_ms: row.try_get("last_activity_at_ms")?,
                expires_at_ms: row.try_get("expires_at_ms")?,
                revoked_at_ms: row.try_get("revoked_at_ms")?,
            })
        })
        .transpose()
    }

    /// Verify CSRF material against the stored digest for an active session.
    pub async fn device_session_csrf_matches(
        &self,
        id: &str,
        csrf_hash: &str,
        now_ms: i64,
    ) -> Result<bool, Error> {
        let stored: Option<String> = sqlx::query_scalar::query_scalar(
            "SELECT csrf_token_sha256 FROM device_sessions
             WHERE id=?1 AND revoked_at_ms IS NULL AND expires_at_ms>?2",
        )
        .bind(id)
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await?;
        Ok(stored.is_some_and(|value| constant_time_eq(value.as_bytes(), csrf_hash.as_bytes())))
    }

    /// Replace both secrets for an existing device and audit the rotation atomically.
    pub async fn rotate_device_session(
        &self,
        id: &str,
        session_token_sha256: &str,
        csrf_token_sha256: &str,
        actor: &str,
        now_ms: i64,
    ) -> Result<bool, Error> {
        if session_token_sha256.len() != 64 || csrf_token_sha256.len() != 64 {
            return Err(Error::Invalid("device session token digest"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query::query(
            "UPDATE device_sessions SET session_token_sha256=?1,csrf_token_sha256=?2,
             last_activity_at_ms=?3 WHERE id=?4 AND revoked_at_ms IS NULL AND expires_at_ms>?3",
        )
        .bind(session_token_sha256)
        .bind(csrf_token_sha256)
        .bind(now_ms)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query::query(
            "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
             VALUES (?1,?2,'rotated',?3)",
        )
        .bind(id)
        .bind(actor)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Record session activity without extending its fixed expiration.
    pub async fn touch_device_session(&self, id: &str, now_ms: i64) -> Result<(), Error> {
        const TOUCH_INTERVAL_MS: i64 = 60_000;
        let last: Option<i64> = sqlx::query_scalar::query_scalar(
            "SELECT last_activity_at_ms FROM device_sessions
             WHERE id=?1 AND revoked_at_ms IS NULL AND expires_at_ms>?2",
        )
        .bind(id)
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await?;
        let Some(last) = last else {
            return Ok(());
        };
        if now_ms.saturating_sub(last) < TOUCH_INTERVAL_MS {
            return Ok(());
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE device_sessions SET last_activity_at_ms=?1
             WHERE id=?2 AND revoked_at_ms IS NULL AND expires_at_ms>?1
               AND last_activity_at_ms<=?3",
        )
        .bind(now_ms)
        .bind(id)
        .bind(now_ms.saturating_sub(TOUCH_INTERVAL_MS))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Revoke one device session and audit the security-sensitive operation.
    pub async fn revoke_device_session(
        &self,
        id: &str,
        actor: &str,
        now_ms: i64,
    ) -> Result<bool, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query::query(
            "UPDATE device_sessions SET revoked_at_ms=?1 WHERE id=?2 AND revoked_at_ms IS NULL",
        )
        .bind(now_ms)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            transaction.rollback().await?;
            return Ok(false);
        }
        sqlx::query::query(
            "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
             VALUES (?1,?2,'revoked',?3)",
        )
        .bind(id)
        .bind(actor)
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Revoke all active sessions and audit each affected session.
    pub async fn revoke_all_device_sessions(&self, actor: &str, now_ms: i64) -> Result<u64, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar::query_scalar(
            "SELECT id FROM device_sessions WHERE revoked_at_ms IS NULL",
        )
        .fetch_all(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE device_sessions SET revoked_at_ms=?1 WHERE revoked_at_ms IS NULL",
        )
        .bind(now_ms)
        .execute(&mut *transaction)
        .await?;
        for id in &ids {
            sqlx::query::query(
                "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
                 VALUES (?1,?2,'revoked_all',?3)",
            )
            .bind(id)
            .bind(actor)
            .bind(now_ms)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(ids.len() as u64)
    }

    /// List sessions for the authenticated device-management view.
    pub async fn device_sessions(&self) -> Result<Vec<DeviceSessionRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT id,device_name,created_at_ms,last_activity_at_ms,expires_at_ms,revoked_at_ms
             FROM device_sessions ORDER BY created_at_ms DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DeviceSessionRecord {
                    id: row.try_get("id")?,
                    name: row.try_get("device_name")?,
                    created_at_ms: row.try_get("created_at_ms")?,
                    last_activity_at_ms: row.try_get("last_activity_at_ms")?,
                    expires_at_ms: row.try_get("expires_at_ms")?,
                    revoked_at_ms: row.try_get("revoked_at_ms")?,
                })
            })
            .collect()
    }
}

fn validate_session_input(
    id: &str,
    name: &str,
    session_token_sha256: &str,
    csrf_token_sha256: &str,
    now_ms: i64,
    expires_at_ms: i64,
) -> Result<(), Error> {
    if id.is_empty()
        || name.trim().is_empty()
        || name.len() > 120
        || session_token_sha256.len() != 64
        || csrf_token_sha256.len() != 64
        || expires_at_ms <= now_ms
    {
        return Err(Error::Invalid("device session"));
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    difference == 0
}
