use super::*;

use chrono::Utc;
use rusqlite::{OptionalExtension, params};
impl Database {
    pub fn register_worker_session(
        &self,
        worker_id: &str,
        registration_key: &str,
        bootstrap_secret: &str,
        session_token: &str,
    ) -> Result<WorkerSession, Error> {
        if worker_id.trim().is_empty()
            || worker_id.len() > 128
            || registration_key.trim().is_empty()
            || registration_key.len() > 256
            || bootstrap_secret.len() < 32
            || session_token.len() < 32
        {
            return Err(Error::Invalid("worker registration"));
        }
        let digest = sha256_hex(
            serde_json::to_string(&(worker_id, "worker-session-v1"))
                .expect("tuple serializes")
                .as_bytes(),
        );
        let persisted_registration_key = format!(
            "worker-registration-v1:{}",
            hmac_sha256_hex(bootstrap_secret.as_bytes(), registration_key.as_bytes())
        );
        let token_hash = sha256_hex(session_token.as_bytes());
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let replayed = replay(
            &tx,
            &persisted_registration_key,
            "register_worker_session",
            &digest,
        )?;
        if let Some(resource_id) = replayed {
            if resource_id != worker_id {
                return Err(Error::Conflict(
                    "registration key belongs to another Worker",
                ));
            }
            tx.execute(
                "UPDATE worker_sessions SET token_sha256=?1,updated_at=?2 WHERE worker_id=?3",
                params![token_hash, Utc::now().to_rfc3339(), worker_id],
            )?;
            if tx.changes() != 1 {
                return Err(Error::Conflict("Worker registration session is missing"));
            }
            record_event(
                &tx,
                worker_id,
                "worker.session_rotated",
                "session token rotated; prior token invalidated",
            )?;
            tx.commit()?;
            return Ok(WorkerSession {
                worker_id: worker_id.into(),
                created: false,
            });
        }
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM worker_sessions WHERE worker_id=?1)",
            [worker_id],
            |r| r.get(0),
        )?;
        if existing {
            return Err(Error::Conflict(
                "Worker identity already registered; reuse its registration key to rotate session",
            ));
        }
        tx.execute(
            "INSERT INTO worker_sessions VALUES (?1,?2,?3)",
            params![worker_id, token_hash, Utc::now().to_rfc3339()],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![
                persisted_registration_key,
                "register_worker_session",
                digest,
                worker_id
            ],
        )?;
        record_event(
            &tx,
            worker_id,
            "worker.registered",
            "session token digest stored",
        )?;
        tx.commit()?;
        Ok(WorkerSession {
            worker_id: worker_id.into(),
            created: true,
        })
    }

    pub fn worker_session_matches(&self, worker_id: &str, token: &str) -> Result<bool, Error> {
        if token.len() < 32 || token.len() > 256 {
            return Ok(false);
        }
        let connection = self.0.lock().expect("database mutex poisoned");
        let stored: Option<String> = connection
            .query_row(
                "SELECT token_sha256 FROM worker_sessions WHERE worker_id=?1",
                [worker_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(stored.is_some_and(|stored| {
            constant_time_eq(stored.as_bytes(), sha256_hex(token.as_bytes()).as_bytes())
        }))
    }

    pub fn claim_attempt(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<Option<WorkerLease>, Error> {
        if worker_id.trim().is_empty() || lease_ms <= 0 || lease_ms > 60_000 {
            return Err(Error::Invalid("worker lease request"));
        }
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let candidate: Option<String> = tx.query_row(
            "SELECT a.id FROM attempts a JOIN runs r ON r.id=a.run_id JOIN campaigns c ON c.id=r.campaign_id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE a.status IN ('QUEUED','LEASED') AND c.status='AUTHORIZED' AND a.sandbox_ready=1
             AND (l.attempt_id IS NULL OR l.expires_at_ms<=?1) ORDER BY a.created_at,a.id LIMIT 1",
            [now_ms], |r| r.get(0)).optional()?;
        let Some(attempt_id) = candidate else {
            tx.commit()?;
            return Ok(None);
        };
        let prior: Option<(String, i64)> = tx
            .query_row(
                "SELECT worker_id,generation FROM worker_leases WHERE attempt_id=?1",
                [&attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let generation = match prior {
            Some((_, generation)) => generation
                .checked_add(1)
                .ok_or(Error::Invalid("worker generation exhausted"))?,
            None => 1,
        };
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or(Error::Invalid("worker lease expiry overflow"))?;
        tx.execute("INSERT INTO worker_leases VALUES (?1,?2,?3,?4) ON CONFLICT(attempt_id) DO UPDATE SET worker_id=excluded.worker_id,generation=excluded.generation,expires_at_ms=excluded.expires_at_ms", params![attempt_id,worker_id,generation,expires_at_ms])?;
        tx.execute(
            "UPDATE attempts SET status='LEASED' WHERE id=?1",
            [&attempt_id],
        )?;
        record_event(
            &tx,
            &attempt_id,
            "worker.claimed",
            &format!("generation={generation}"),
        )?;
        tx.commit()?;
        Ok(Some(WorkerLease {
            attempt_id,
            worker_id: worker_id.into(),
            generation,
            expires_at_ms,
        }))
    }

    pub fn heartbeat(
        &self,
        attempt_id: &str,
        worker_id: &str,
        generation: i64,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<WorkerLease, Error> {
        if lease_ms <= 0 || lease_ms > 60_000 {
            return Err(Error::Invalid("worker lease duration"));
        }
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        let current: (String,i64,i64,String)=tx.query_row("SELECT l.worker_id,l.generation,l.expires_at_ms,a.status FROM worker_leases l JOIN attempts a ON a.id=l.attempt_id WHERE l.attempt_id=?1",[attempt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(Error::NotFound)?;
        if current.0 != worker_id
            || current.1 != generation
            || current.2 <= now_ms
            || current.3 != "LEASED"
        {
            return Err(Error::Invalid("stale or expired worker lease"));
        }
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or(Error::Invalid("worker lease expiry overflow"))?;
        tx.execute("UPDATE worker_leases SET expires_at_ms=?1 WHERE attempt_id=?2 AND worker_id=?3 AND generation=?4",params![expires_at_ms,attempt_id,worker_id,generation])?;
        tx.commit()?;
        Ok(WorkerLease {
            attempt_id: attempt_id.into(),
            worker_id: worker_id.into(),
            generation,
            expires_at_ms,
        })
    }

    pub fn report_terminal(&self, report: &WorkerReport, now_ms: i64) -> Result<(), Error> {
        if report.worker_id.trim().is_empty()
            || report.receipt_id.trim().is_empty()
            || report.receipt_id.len() > 128
            || report.generation <= 0
            || !is_sha256(&report.result_sha256)
            || !matches!(report.terminal_status.as_str(), "FAILED" | "CANCELLED")
        {
            return Err(Error::Invalid("worker terminal report"));
        }
        let digest = sha256_hex(
            serde_json::to_string(report)
                .expect("report serializes")
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        let receipt: Option<(String, String)> = tx
            .query_row(
                "SELECT request_digest,attempt_id FROM worker_receipts WHERE receipt_id=?1",
                [&report.receipt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((saved_digest, saved_attempt)) = receipt {
            if saved_digest == digest && saved_attempt == report.attempt_id {
                tx.commit()?;
                return Ok(());
            }
            return Err(Error::Invalid("receipt id reused with different report"));
        }
        let lease: (String, i64, i64) = tx
            .query_row(
                "SELECT worker_id,generation,expires_at_ms FROM worker_leases WHERE attempt_id=?1",
                [&report.attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if lease.0 != report.worker_id || lease.1 != report.generation || lease.2 <= now_ms {
            return Err(Error::Invalid("stale or expired worker lease"));
        }
        let changed = tx.execute(
            "UPDATE attempts SET status=?1 WHERE id=?2 AND status='LEASED'",
            params![report.terminal_status, report.attempt_id],
        )?;
        if changed != 1 {
            return Err(Error::Invalid("attempt is not leased"));
        }
        tx.execute(
            "INSERT INTO worker_receipts VALUES (?1,?2,?3,?4)",
            params![
                report.receipt_id,
                digest,
                report.attempt_id,
                Utc::now().to_rfc3339()
            ],
        )?;
        record_event(
            &tx,
            &report.attempt_id,
            "worker.terminal_reported",
            &report.terminal_status,
        )?;
        tx.commit()?;
        Ok(())
    }
}
