use super::Error;
use serde::Serialize;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePairingRecord {
    pub id: String,
    pub status: String,
    pub verification_code: String,
    pub device_name: Option<String>,
    pub device_session_id: Option<String>,
    pub created_by: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub claimed_at_ms: Option<i64>,
    pub decided_at_ms: Option<i64>,
}

impl DevicePairingRecord {
    fn from_row(row: SqliteRow) -> Self {
        Self {
            id: row.get("id"),
            status: row.get("status"),
            verification_code: row.get("verification_code"),
            device_name: row.get("device_name"),
            device_session_id: row.get("device_session_id"),
            created_by: row.get("created_by"),
            created_at_ms: row.get("created_at_ms"),
            expires_at_ms: row.get("expires_at_ms"),
            claimed_at_ms: row.get("claimed_at_ms"),
            decided_at_ms: row.get("decided_at_ms"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevicePairingPoll {
    pub status: String,
    pub verification_code: String,
    pub device_name: Option<String>,
    pub expires_at_ms: i64,
}

impl super::Database {
    pub async fn create_device_pairing(
        &self,
        id: &str,
        verification_code: &str,
        created_by: &str,
        created_at_ms: i64,
        expires_at_ms: i64,
    ) -> Result<(), Error> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        sqlx::query::query("INSERT INTO device_pairing_challenges (id,verification_code,status,created_by,created_at_ms,expires_at_ms) VALUES (?1,?2,'AVAILABLE',?3,?4,?5)")
            .bind(id).bind(verification_code).bind(created_by)
            .bind(created_at_ms).bind(expires_at_ms).execute(&mut *tx).await?;
        pairing_audit(&mut tx, id, "created", created_by, created_at_ms).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn claim_device_pairing(
        &self,
        id: &str,
        poll_token_sha256: &str,
        device_name: &str,
        now_ms: i64,
    ) -> Result<DevicePairingPoll, Error> {
        if device_name.trim().is_empty()
            || device_name.len() > 80
            || device_name.chars().any(char::is_control)
        {
            return Err(Error::Invalid("device name must contain 1-80 bytes"));
        }
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query::query("UPDATE device_pairing_challenges SET status='PENDING',poll_token_sha256=?1,device_name=?2,claimed_at_ms=?3 WHERE id=?4 AND status='AVAILABLE' AND expires_at_ms>?3")
            .bind(poll_token_sha256).bind(device_name.trim()).bind(now_ms).bind(id)
            .execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            tx.rollback().await?;
            return Err(Error::NotFound);
        }
        pairing_audit(&mut tx, id, "claimed", device_name.trim(), now_ms).await?;
        let row = sqlx::query::query("SELECT status,verification_code,device_name,expires_at_ms FROM device_pairing_challenges WHERE id=?1")
            .bind(id).fetch_one(&mut *tx).await?;
        let result = DevicePairingPoll {
            status: row.get("status"),
            verification_code: row.get("verification_code"),
            device_name: row.get("device_name"),
            expires_at_ms: row.get("expires_at_ms"),
        };
        tx.commit().await?;
        Ok(result)
    }

    pub async fn decide_device_pairing(
        &self,
        id: &str,
        approve: bool,
        verification_code: Option<&str>,
        actor: &str,
        now_ms: i64,
    ) -> Result<DevicePairingRecord, Error> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let next = if approve { "APPROVED" } else { "REJECTED" };
        let changed = sqlx::query::query("UPDATE device_pairing_challenges SET status=?1,decided_at_ms=?2 WHERE id=?3 AND status='PENDING' AND expires_at_ms>?2 AND (?4 IS NULL OR verification_code=?4)")
            .bind(next).bind(now_ms).bind(id).bind(verification_code).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            tx.rollback().await?;
            return Err(Error::Conflict(
                "pairing is no longer pending or has expired",
            ));
        }
        pairing_audit(
            &mut tx,
            id,
            if approve { "approved" } else { "rejected" },
            actor,
            now_ms,
        )
        .await?;
        let row = sqlx::query::query("SELECT * FROM device_pairing_challenges WHERE id=?1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        let record = DevicePairingRecord::from_row(row);
        tx.commit().await?;
        Ok(record)
    }

    pub async fn list_device_pairings(
        &self,
        now_ms: i64,
    ) -> Result<Vec<DevicePairingRecord>, Error> {
        let rows = sqlx::query::query("SELECT * FROM device_pairing_challenges WHERE status IN ('PENDING','APPROVED','REJECTED') AND expires_at_ms>?1 ORDER BY created_at_ms DESC")
            .bind(now_ms).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(DevicePairingRecord::from_row)
            .collect())
    }

    pub async fn poll_device_pairing(
        &self,
        id: &str,
        poll_token_sha256: &str,
        now_ms: i64,
    ) -> Result<DevicePairingPoll, Error> {
        let row = sqlx::query::query("SELECT status,verification_code,device_name,expires_at_ms FROM device_pairing_challenges WHERE id=?1 AND poll_token_sha256=?2 AND expires_at_ms>?3 AND status IN ('PENDING','APPROVED','REJECTED')")
            .bind(id).bind(poll_token_sha256).bind(now_ms).fetch_optional(&self.pool).await?;
        row.map(|row| DevicePairingPoll {
            status: row.get("status"),
            verification_code: row.get("verification_code"),
            device_name: row.get("device_name"),
            expires_at_ms: row.get("expires_at_ms"),
        })
        .ok_or(Error::NotFound)
    }

    pub async fn consume_device_pairing(
        &self,
        id: &str,
        poll_token_sha256: &str,
        now_ms: i64,
    ) -> Result<(), Error> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query::query("UPDATE device_pairing_challenges SET status='CONSUMED',consumed_at_ms=?1,poll_token_sha256=NULL WHERE id=?2 AND poll_token_sha256=?3 AND status='APPROVED' AND expires_at_ms>?1")
            .bind(now_ms).bind(id).bind(poll_token_sha256)
            .execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            tx.rollback().await?;
            return Err(Error::Conflict("pairing approval was already consumed"));
        }
        pairing_audit(&mut tx, id, "consumed", "approved-claimant", now_ms).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn attach_pairing_device_session(
        &self,
        id: &str,
        device_session_id: &str,
    ) -> Result<(), Error> {
        let changed = sqlx::query::query("UPDATE device_pairing_challenges SET device_session_id=?1 WHERE id=?2 AND status='CONSUMED' AND device_session_id IS NULL")
            .bind(device_session_id).bind(id).execute(&self.pool).await?.rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("pairing session is already attached"));
        }
        Ok(())
    }

    pub async fn revoke_pairing_device(
        &self,
        id: &str,
        actor: &str,
        now_ms: i64,
    ) -> Result<Option<String>, Error> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query::query("SELECT id,device_session_id FROM device_pairing_challenges WHERE (id=?1 OR device_session_id=?1) AND status='CONSUMED'")
            .bind(id).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let challenge_id: String = row.get("id");
        let session_id: String = row.get("device_session_id");
        pairing_audit(
            &mut tx,
            &challenge_id,
            "revocation_requested",
            actor,
            now_ms,
        )
        .await?;
        tx.commit().await?;
        Ok(Some(session_id))
    }
}

async fn pairing_audit(
    connection: &mut SqliteConnection,
    challenge_id: &str,
    action: &str,
    actor: &str,
    now_ms: i64,
) -> Result<(), Error> {
    sqlx::query::query("INSERT INTO device_pairing_audit (challenge_id,action,actor,occurred_at_ms) VALUES (?1,?2,?3,?4)")
        .bind(challenge_id).bind(action).bind(actor).bind(now_ms)
        .execute(connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::query_scalar::query_scalar;
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    async fn database() -> (super::super::Database, PathBuf) {
        let path = std::env::temp_dir().join(format!("factory-pairing-{}.sqlite", Uuid::new_v4()));
        let db = super::super::Database::open(&path)
            .await
            .expect("database opens");
        (db, path)
    }

    #[tokio::test]
    async fn pairing_is_single_use_approval_bound_and_audited() {
        let (db, path) = database().await;
        db.create_device_pairing("challenge", "204817", "owner-session", 100, 300_100)
            .await
            .expect("challenge persists");
        let claimed = db
            .claim_device_pairing("challenge", "poll-hash", "Phone", 101)
            .await
            .expect("first claim succeeds");
        assert_eq!(claimed.status, "PENDING");
        assert_eq!(claimed.verification_code, "204817");
        assert!(
            db.claim_device_pairing("challenge", "again", "Phone", 102)
                .await
                .is_err()
        );
        assert!(
            db.poll_device_pairing("challenge", "wrong-poll", 103)
                .await
                .is_err()
        );
        assert!(
            db.decide_device_pairing("challenge", true, Some("999999"), "owner-session", 104)
                .await
                .is_err()
        );
        db.decide_device_pairing("challenge", true, Some("204817"), "owner-session", 104)
            .await
            .expect("owner approves");
        assert_eq!(
            db.poll_device_pairing("challenge", "poll-hash", 105)
                .await
                .expect("approved phone can poll")
                .status,
            "APPROVED"
        );
        db.consume_device_pairing("challenge", "poll-hash", 106)
            .await
            .expect("approval exchange is consumed");
        assert!(
            db.consume_device_pairing("challenge", "poll-hash", 107)
                .await
                .is_err()
        );
        let audit: i64 = query_scalar(
            "SELECT COUNT(*) FROM device_pairing_audit WHERE challenge_id='challenge'",
        )
        .fetch_one(&db.pool)
        .await
        .expect("audit query");
        assert_eq!(audit, 4);
        db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn concurrent_approval_exchange_has_one_winner() {
        let (db, path) = database().await;
        db.create_device_pairing("race", "654321", "owner", 100, 300_100)
            .await
            .expect("challenge persists");
        db.claim_device_pairing("race", "poll-hash", "Phone", 101)
            .await
            .expect("claim succeeds");
        db.decide_device_pairing("race", true, Some("654321"), "owner", 102)
            .await
            .expect("owner approves");
        let (first, second) = tokio::join!(
            db.consume_device_pairing("race", "poll-hash", 103),
            db.consume_device_pairing("race", "poll-hash", 103),
        );
        assert_ne!(first.is_ok(), second.is_ok());
        db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn rejected_and_expired_challenges_cannot_be_claimed_or_approved() {
        let (db, path) = database().await;
        db.create_device_pairing("reject", "112233", "owner", 100, 200)
            .await
            .expect("challenge persists");
        db.claim_device_pairing("reject", "poll", "Phone", 101)
            .await
            .expect("claim succeeds");
        db.decide_device_pairing("reject", false, None, "owner", 102)
            .await
            .expect("owner rejects");
        assert!(db.poll_device_pairing("reject", "poll", 103).await.is_ok());
        assert!(
            db.decide_device_pairing("reject", true, Some("112233"), "owner", 104)
                .await
                .is_err()
        );
        db.create_device_pairing("expired", "112234", "owner", 100, 200)
            .await
            .expect("challenge persists");
        assert!(
            db.claim_device_pairing("expired", "poll", "Phone", 200)
                .await
                .is_err()
        );
        db.close().await;
        let _ = fs::remove_file(path);
    }
}
