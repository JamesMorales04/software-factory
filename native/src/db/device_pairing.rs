use super::{DeviceSessionRecord, Error};
use serde::Serialize;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};

/// Maximum failed verification attempts before a challenge is cancelled.
pub const MAX_FAILED_APPROVALS: i64 = 5;

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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditEntry {
    pub action: String,
    pub actor: String,
    pub device_name: Option<String>,
    pub occurred_at_ms: i64,
}

/// Inputs for one atomic pairing consumption and session creation.
pub struct PairingExchange<'a> {
    pub challenge_id: &'a str,
    pub poll_token_sha256: &'a str,
    pub session_id: &'a str,
    pub device_name: &'a str,
    pub session_token_sha256: &'a str,
    pub csrf_token_sha256: &'a str,
    pub now_ms: i64,
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
        // A no-op update acquires SQLite's write lock before the read below, so
        // concurrent attempts and decisions serialize across Core processes.
        let pending = sqlx::query::query(
            "UPDATE device_pairing_challenges SET failed_attempts=failed_attempts
             WHERE id=?1 AND status='PENDING' AND expires_at_ms>?2",
        )
        .bind(id)
        .bind(now_ms)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if pending != 1 {
            tx.rollback().await?;
            return Err(Error::Conflict(
                "pairing is no longer pending or has expired",
            ));
        }
        let row = sqlx::query::query(
            "SELECT verification_code,failed_attempts FROM device_pairing_challenges WHERE id=?1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let stored_code: String = row.get("verification_code");
        let failed_attempts: i64 = row.get("failed_attempts");
        if approve {
            let supplied = verification_code.unwrap_or("");
            if !super::device_sessions::constant_time_eq(
                stored_code.as_bytes(),
                supplied.as_bytes(),
            ) {
                let attempts = failed_attempts.saturating_add(1);
                if attempts >= MAX_FAILED_APPROVALS {
                    sqlx::query::query(
                        "UPDATE device_pairing_challenges
                         SET failed_attempts=?1,status='CANCELLED',decided_at_ms=?2
                         WHERE id=?3 AND status='PENDING'",
                    )
                    .bind(attempts)
                    .bind(now_ms)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                    pairing_audit(&mut tx, id, "cancelled", actor, now_ms).await?;
                    tx.commit().await?;
                    return Err(Error::Conflict(
                        "verification code did not match; pairing challenge was cancelled after repeated failures",
                    ));
                }
                sqlx::query::query(
                    "UPDATE device_pairing_challenges SET failed_attempts=?1 WHERE id=?2",
                )
                .bind(attempts)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                return Err(Error::Conflict("verification code did not match"));
            }
        }
        let epoch = super::access_revocation::access_epoch(&mut tx).await?;
        let next = if approve { "APPROVED" } else { "REJECTED" };
        sqlx::query::query(
            "UPDATE device_pairing_challenges
             SET status=?1,decided_at_ms=?2,approved_epoch=?3
             WHERE id=?4 AND status='PENDING'",
        )
        .bind(next)
        .bind(now_ms)
        .bind(approve.then_some(epoch))
        .bind(id)
        .execute(&mut *tx)
        .await?;
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
        let rows = sqlx::query::query("SELECT * FROM device_pairing_challenges WHERE status IN ('PENDING','APPROVED') AND expires_at_ms>?1 ORDER BY created_at_ms DESC")
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
        let row = sqlx::query::query("SELECT status,verification_code,device_name,expires_at_ms FROM device_pairing_challenges WHERE id=?1 AND poll_token_sha256=?2 AND status IN ('PENDING','APPROVED','REJECTED','CANCELLED','EXPIRED')")
            .bind(id).bind(poll_token_sha256).fetch_optional(&self.pool).await?;
        row.map(|row| {
            let status: String = row.get("status");
            let expires_at_ms: i64 = row.get("expires_at_ms");
            let status =
                if matches!(status.as_str(), "PENDING" | "APPROVED") && expires_at_ms <= now_ms {
                    "EXPIRED".to_owned()
                } else {
                    status
                };
            DevicePairingPoll {
                status,
                verification_code: row.get("verification_code"),
                device_name: row.get("device_name"),
                expires_at_ms,
            }
        })
        .ok_or(Error::NotFound)
    }

    pub async fn cancel_device_pairing(
        &self,
        id: &str,
        actor: &str,
        now_ms: i64,
    ) -> Result<DevicePairingRecord, Error> {
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query::query(
            "UPDATE device_pairing_challenges SET status='CANCELLED',decided_at_ms=?1
             WHERE id=?2 AND status IN ('AVAILABLE','PENDING','APPROVED') AND expires_at_ms>?1",
        )
        .bind(now_ms)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await?;
            return Err(Error::Conflict(
                "pairing is no longer awaiting a decision or has expired",
            ));
        }
        pairing_audit(&mut tx, id, "cancelled", actor, now_ms).await?;
        let row = sqlx::query::query("SELECT * FROM device_pairing_challenges WHERE id=?1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        let record = DevicePairingRecord::from_row(row);
        tx.commit().await?;
        Ok(record)
    }

    pub async fn security_audit(&self, limit: i64) -> Result<Vec<SecurityAuditEntry>, Error> {
        let limit = limit.clamp(1, 100);
        let mut entries = Vec::new();
        let session_rows = sqlx::query::query(
            "SELECT a.action, COALESCE(actor_s.device_name, a.actor) AS actor,
                    subject_s.device_name AS device_name, a.recorded_at_ms AS occurred_at_ms
             FROM device_session_audit a
             LEFT JOIN device_sessions subject_s ON subject_s.id = a.session_id
             LEFT JOIN device_sessions actor_s ON actor_s.id = a.actor
             ORDER BY a.sequence DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        for row in session_rows {
            entries.push(SecurityAuditEntry {
                action: row.get("action"),
                actor: row.get("actor"),
                device_name: row.get("device_name"),
                occurred_at_ms: row.get("occurred_at_ms"),
            });
        }
        let pairing_rows = sqlx::query::query(
            "SELECT a.action, COALESCE(actor_s.device_name, a.actor) AS actor,
                    c.device_name AS device_name, a.occurred_at_ms
             FROM device_pairing_audit a
             LEFT JOIN device_pairing_challenges c ON c.id = a.challenge_id
             LEFT JOIN device_sessions actor_s ON actor_s.id = a.actor
             ORDER BY a.sequence DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        for row in pairing_rows {
            entries.push(SecurityAuditEntry {
                action: row.get("action"),
                actor: row.get("actor"),
                device_name: row.get("device_name"),
                occurred_at_ms: row.get("occurred_at_ms"),
            });
        }
        entries.sort_by(|left, right| right.occurred_at_ms.cmp(&left.occurred_at_ms));
        entries.truncate(limit as usize);
        Ok(entries)
    }

    /// Atomically consume an approved challenge and create its device session.
    ///
    /// The epoch check fences creation against global revocation: a revocation
    /// that commits first advances the epoch, and a revocation that commits
    /// after this transaction revokes the session it just created.
    pub async fn exchange_device_pairing(
        &self,
        exchange: PairingExchange<'_>,
    ) -> Result<DeviceSessionRecord, Error> {
        let PairingExchange {
            challenge_id,
            poll_token_sha256,
            session_id,
            device_name,
            session_token_sha256,
            csrf_token_sha256,
            now_ms,
            expires_at_ms,
        } = exchange;
        if poll_token_sha256.len() != 64 {
            return Err(Error::Invalid("pairing poll digest"));
        }
        super::device_sessions::validate_session_input(
            session_id,
            device_name,
            session_token_sha256,
            csrf_token_sha256,
            now_ms,
            expires_at_ms,
        )?;
        let _guard = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let epoch = super::access_revocation::access_epoch(&mut tx).await?;
        let changed = sqlx::query::query(
            "UPDATE device_pairing_challenges
             SET status='CONSUMED',consumed_at_ms=?1,poll_token_sha256=NULL
             WHERE id=?2 AND poll_token_sha256=?3 AND status='APPROVED'
               AND approved_epoch=?4 AND expires_at_ms>?1",
        )
        .bind(now_ms)
        .bind(challenge_id)
        .bind(poll_token_sha256)
        .bind(epoch)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await?;
            return Err(Error::Conflict(
                "pairing approval was revoked, expired, or already consumed",
            ));
        }
        sqlx::query::query(
            "INSERT INTO device_sessions
             (id,device_name,session_token_sha256,csrf_token_sha256,created_at_ms,last_activity_at_ms,
              expires_at_ms,created_by)
             VALUES (?1,?2,?3,?4,?5,?5,?6,'approved-device-pairing')",
        )
        .bind(session_id)
        .bind(device_name.trim())
        .bind(session_token_sha256)
        .bind(csrf_token_sha256)
        .bind(now_ms)
        .bind(expires_at_ms)
        .execute(&mut *tx)
        .await?;
        sqlx::query::query(
            "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
             VALUES (?1,'approved-device-pairing','created',?2)",
        )
        .bind(session_id)
        .bind(now_ms)
        .execute(&mut *tx)
        .await?;
        sqlx::query::query("UPDATE device_pairing_challenges SET device_session_id=?1 WHERE id=?2")
            .bind(session_id)
            .bind(challenge_id)
            .execute(&mut *tx)
            .await?;
        pairing_audit(
            &mut tx,
            challenge_id,
            "consumed",
            "approved-device-pairing",
            now_ms,
        )
        .await?;
        tx.commit().await?;
        Ok(DeviceSessionRecord {
            id: session_id.to_owned(),
            name: device_name.trim().to_owned(),
            created_at_ms: now_ms,
            last_activity_at_ms: now_ms,
            expires_at_ms,
            revoked_at_ms: None,
        })
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
    use super::{MAX_FAILED_APPROVALS, PairingExchange};
    use crate::device_session;
    use sqlx::query_scalar::query_scalar;
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    const SESSION_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const CSRF_HASH: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    async fn database() -> (super::super::Database, PathBuf) {
        let path = std::env::temp_dir().join(format!("factory-pairing-{}.sqlite", Uuid::new_v4()));
        let db = super::super::Database::open(&path)
            .await
            .expect("database opens");
        (db, path)
    }

    fn digest(seed: char) -> String {
        seed.to_string().repeat(64)
    }

    async fn claimed_and_approved(
        db: &super::super::Database,
        id: &str,
        code: &str,
        poll_hash: &str,
    ) {
        db.create_device_pairing(id, code, "owner-session", 100, 300_100)
            .await
            .expect("challenge persists");
        db.claim_device_pairing(id, poll_hash, "Phone", 101)
            .await
            .expect("claim succeeds");
        db.decide_device_pairing(id, true, Some(code), "owner-session", 102)
            .await
            .expect("owner approves");
    }

    #[tokio::test]
    async fn pairing_is_single_use_approval_bound_and_audited() {
        let (db, path) = database().await;
        let poll = digest('a');
        db.create_device_pairing("challenge", "204817", "owner-session", 100, 300_100)
            .await
            .expect("challenge persists");
        let claimed = db
            .claim_device_pairing("challenge", &poll, "Phone", 101)
            .await
            .expect("first claim succeeds");
        assert_eq!(claimed.status, "PENDING");
        assert_eq!(claimed.verification_code, "204817");
        assert!(
            db.claim_device_pairing("challenge", &digest('d'), "Phone", 102)
                .await
                .is_err()
        );
        assert!(
            db.poll_device_pairing("challenge", &digest('e'), 103)
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
            db.poll_device_pairing("challenge", &poll, 105)
                .await
                .expect("approved phone can poll")
                .status,
            "APPROVED"
        );
        let session = db
            .exchange_device_pairing(PairingExchange {
                challenge_id: "challenge",
                poll_token_sha256: &poll,
                session_id: "session-one",
                device_name: "Phone",
                session_token_sha256: SESSION_HASH,
                csrf_token_sha256: CSRF_HASH,
                now_ms: 106,
                expires_at_ms: 106 + 1_000,
            })
            .await
            .expect("approval exchange is consumed");
        assert_eq!(session.name, "Phone");
        assert!(
            db.device_session_by_token_hash(SESSION_HASH, 106)
                .await
                .expect("session lookup")
                .is_some()
        );
        assert!(
            db.exchange_device_pairing(PairingExchange {
                challenge_id: "challenge",
                poll_token_sha256: &poll,
                session_id: "session-two",
                device_name: "Phone",
                session_token_sha256: SESSION_HASH,
                csrf_token_sha256: CSRF_HASH,
                now_ms: 107,
                expires_at_ms: 107 + 1_000,
            })
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
        let poll = digest('a');
        claimed_and_approved(&db, "race", "654321", &poll).await;
        let exchange = |session_id: &'static str| PairingExchange {
            challenge_id: "race",
            poll_token_sha256: &poll,
            session_id,
            device_name: "Phone",
            session_token_sha256: SESSION_HASH,
            csrf_token_sha256: CSRF_HASH,
            now_ms: 103,
            expires_at_ms: 2_103,
        };
        let (first, second) = tokio::join!(
            db.exchange_device_pairing(exchange("session-one")),
            db.exchange_device_pairing(exchange("session-two")),
        );
        assert_ne!(first.is_ok(), second.is_ok());
        assert_eq!(db.device_sessions().await.expect("sessions").len(), 1);
        db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn failed_attempts_are_challenge_scoped_and_capped_under_concurrency() {
        let (db, path) = database().await;
        let poll = digest('a');
        db.create_device_pairing("brute", "424242", "owner-session", 100, 300_100)
            .await
            .expect("challenge persists");
        db.claim_device_pairing("brute", &poll, "Phone", 101)
            .await
            .expect("claim succeeds");

        let mut attempts = Vec::new();
        for _ in 0..(MAX_FAILED_APPROVALS + 3) {
            let db = db.clone();
            attempts.push(tokio::spawn(async move {
                db.decide_device_pairing("brute", true, Some("000000"), "owner-session", 103)
                    .await
            }));
        }
        for attempt in attempts {
            assert!(attempt.await.expect("attempt task").is_err());
        }
        assert!(
            db.decide_device_pairing("brute", true, Some("424242"), "owner-session", 104)
                .await
                .is_err(),
            "the correct code must not work after the attempt cap is reached"
        );
        assert_eq!(
            db.poll_device_pairing("brute", &poll, 105)
                .await
                .expect("cancelled poll")
                .status,
            "CANCELLED"
        );

        let second_poll = digest('f');
        db.create_device_pairing("unaffected", "135790", "owner-session", 100, 300_100)
            .await
            .expect("second challenge persists");
        db.claim_device_pairing("unaffected", &second_poll, "Phone", 101)
            .await
            .expect("second claim succeeds");
        db.decide_device_pairing("unaffected", true, Some("000000"), "owner-session", 106)
            .await
            .expect_err("one wrong attempt fails");
        db.decide_device_pairing("unaffected", true, Some("135790"), "owner-session", 107)
            .await
            .expect("a different challenge keeps working");
        db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn global_revocation_cancels_approved_enrollment_and_advances_the_epoch() {
        for recovery in [false, true] {
            let (db, path) = database().await;
            let owner = device_session::issue_owner(&db, "Desktop", 90)
                .await
                .expect("owner session");
            let poll = digest('a');
            claimed_and_approved(&db, "pending-approval", "204817", &poll).await;

            if recovery {
                db.reset_owner_bootstrap("local-recovery", 103)
                    .await
                    .expect("local recovery revokes access");
            } else {
                db.revoke_all_device_sessions("owner-session", 103)
                    .await
                    .expect("revoke-all revokes access");
            }

            assert!(
                db.device_session_by_token_hash(
                    &device_session::sha256_hex(&owner.session_token),
                    104,
                )
                .await
                .expect("owner lookup")
                .is_none(),
                "global revocation must revoke the owner session"
            );
            assert_eq!(
                db.poll_device_pairing("pending-approval", &poll, 104)
                    .await
                    .expect("delayed poll")
                    .status,
                "CANCELLED"
            );
            assert!(
                db.exchange_device_pairing(PairingExchange {
                    challenge_id: "pending-approval",
                    poll_token_sha256: &poll,
                    session_id: "resurrected",
                    device_name: "Phone",
                    session_token_sha256: SESSION_HASH,
                    csrf_token_sha256: CSRF_HASH,
                    now_ms: 104,
                    expires_at_ms: 2_104,
                })
                .await
                .is_err(),
                "a delayed approval must not create a session"
            );
            assert!(
                db.device_sessions()
                    .await
                    .expect("sessions")
                    .iter()
                    .all(|session| session.revoked_at_ms.is_some())
            );

            let fresh_poll = digest('b');
            claimed_and_approved(&db, "fresh", "909090", &fresh_poll).await;
            db.exchange_device_pairing(PairingExchange {
                challenge_id: "fresh",
                poll_token_sha256: &fresh_poll,
                session_id: "fresh-session",
                device_name: "Phone",
                session_token_sha256: SESSION_HASH,
                csrf_token_sha256: CSRF_HASH,
                now_ms: 106,
                expires_at_ms: 2_106,
            })
            .await
            .expect("a new approval after revocation is usable");
            db.close().await;
            let _ = fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn concurrent_exchange_and_revocation_never_leave_a_live_session() {
        for _ in 0..10 {
            let (db, path) = database().await;
            let _owner = device_session::issue_owner(&db, "Desktop", 90)
                .await
                .expect("owner session");
            let poll = digest('a');
            claimed_and_approved(&db, "race", "654321", &poll).await;
            let (exchanged, revoked) = tokio::join!(
                db.exchange_device_pairing(PairingExchange {
                    challenge_id: "race",
                    poll_token_sha256: &poll,
                    session_id: "race-session",
                    device_name: "Phone",
                    session_token_sha256: SESSION_HASH,
                    csrf_token_sha256: CSRF_HASH,
                    now_ms: 104,
                    expires_at_ms: 2_104,
                }),
                db.revoke_all_device_sessions("owner-session", 104),
            );
            revoked.expect("revocation completes");
            if exchanged.is_ok() {
                assert!(
                    db.device_session_by_token_hash(SESSION_HASH, 105)
                        .await
                        .expect("session lookup")
                        .is_none(),
                    "an exchange racing a revocation must not leave a live session"
                );
            }
            assert!(
                db.device_sessions()
                    .await
                    .expect("sessions")
                    .iter()
                    .all(|session| session.revoked_at_ms.is_some()),
                "no session may remain active after the race"
            );
            db.close().await;
            let _ = fs::remove_file(path);
        }
    }

    #[tokio::test]
    async fn rejected_and_expired_challenges_cannot_be_claimed_or_approved() {
        let (db, path) = database().await;
        db.create_device_pairing("reject", "112233", "owner", 100, 200)
            .await
            .expect("challenge persists");
        let poll = digest('a');
        db.claim_device_pairing("reject", &poll, "Phone", 101)
            .await
            .expect("claim succeeds");
        db.decide_device_pairing("reject", false, None, "owner", 102)
            .await
            .expect("owner rejects");
        assert!(db.poll_device_pairing("reject", &poll, 103).await.is_ok());
        assert!(
            db.decide_device_pairing("reject", true, Some("112233"), "owner", 104)
                .await
                .is_err()
        );
        db.create_device_pairing("expired", "112234", "owner", 100, 200)
            .await
            .expect("challenge persists");
        assert!(
            db.claim_device_pairing("expired", &digest('b'), "Phone", 200)
                .await
                .is_err()
        );
        db.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn cancellation_and_expiry_are_terminal_and_audited() {
        let (db, path) = database().await;
        db.create_device_pairing("cancel", "123456", "owner", 100, 300_100)
            .await
            .expect("challenge persists");
        let poll = digest('a');
        db.claim_device_pairing("cancel", &poll, "Phone", 101)
            .await
            .expect("claim succeeds");
        let cancelled = db
            .cancel_device_pairing("cancel", "owner-session", 102)
            .await
            .expect("owner cancels");
        assert_eq!(cancelled.status, "CANCELLED");
        assert!(
            db.decide_device_pairing("cancel", true, Some("123456"), "owner", 103)
                .await
                .is_err()
        );
        assert_eq!(
            db.poll_device_pairing("cancel", &poll, 104)
                .await
                .expect("cancelled poll")
                .status,
            "CANCELLED"
        );

        db.create_device_pairing("expired", "654321", "owner", 100, 200)
            .await
            .expect("challenge persists");
        let expired_poll = digest('b');
        db.claim_device_pairing("expired", &expired_poll, "Phone", 101)
            .await
            .expect("claim succeeds");
        db.decide_device_pairing("expired", true, Some("654321"), "owner", 102)
            .await
            .expect("owner approves before expiry");
        assert_eq!(
            db.poll_device_pairing("expired", &expired_poll, 201)
                .await
                .expect("expired poll")
                .status,
            "EXPIRED"
        );
        assert!(
            db.exchange_device_pairing(PairingExchange {
                challenge_id: "expired",
                poll_token_sha256: &expired_poll,
                session_id: "expired-session",
                device_name: "Phone",
                session_token_sha256: SESSION_HASH,
                csrf_token_sha256: CSRF_HASH,
                now_ms: 202,
                expires_at_ms: 2_202,
            })
            .await
            .is_err()
        );

        let audit = db.security_audit(20).await.expect("audit is readable");
        assert!(audit.iter().any(|entry| entry.action == "cancelled"));
        assert!(audit.iter().any(|entry| entry.action == "approved"));
        assert!(audit.iter().any(|entry| entry.action == "claimed"));
        db.close().await;
        let _ = fs::remove_file(path);
    }
}
