use super::*;

use chrono::Utc;

impl Database {
    pub async fn register_worker_session(
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
                .expect("worker digest serializes")
                .as_bytes(),
        );
        let persisted_registration_key = format!(
            "worker-registration-v1:{}",
            hmac_sha256_hex(bootstrap_secret.as_bytes(), registration_key.as_bytes())
        );
        let token_hash = sha256_hex(session_token.as_bytes());
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let replayed = replay(
            &mut transaction,
            &persisted_registration_key,
            "register_worker_session",
            &digest,
        )
        .await?;
        if let Some(resource_id) = replayed {
            if resource_id != worker_id {
                return Err(Error::Conflict(
                    "registration key belongs to another Worker",
                ));
            }
            let changed = sqlx::query::query(
                "UPDATE worker_sessions SET token_sha256=?1,updated_at=?2 WHERE worker_id=?3",
            )
            .bind(&token_hash)
            .bind(Utc::now().to_rfc3339())
            .bind(worker_id)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
            if changed != 1 {
                return Err(Error::Conflict("Worker registration session is missing"));
            }
            record_event(
                &mut transaction,
                worker_id,
                "worker.session_rotated",
                "session token rotated; prior token invalidated",
            )
            .await?;
            transaction.commit().await?;
            return Ok(WorkerSession {
                worker_id: worker_id.into(),
                created: false,
            });
        }
        let existing: Option<(String,)> =
            sqlx::query_as::query_as("SELECT worker_id FROM worker_sessions WHERE worker_id=?1")
                .bind(worker_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing.is_some() {
            return Err(Error::Conflict(
                "Worker identity already registered; reuse its registration key to rotate session",
            ));
        }
        sqlx::query::query("INSERT INTO worker_sessions VALUES (?1,?2,?3)")
            .bind(worker_id)
            .bind(&token_hash)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(&persisted_registration_key)
            .bind("register_worker_session")
            .bind(&digest)
            .bind(worker_id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            worker_id,
            "worker.registered",
            "session token digest stored",
        )
        .await?;
        transaction.commit().await?;
        Ok(WorkerSession {
            worker_id: worker_id.into(),
            created: true,
        })
    }

    pub async fn workers(&self) -> Result<Vec<WorkerSummary>, Error> {
        let rows: Vec<(String, String)> = sqlx::query_as::query_as(
            "SELECT worker_id,updated_at FROM worker_sessions ORDER BY updated_at,worker_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(worker_id, updated_at)| WorkerSummary {
                worker_id,
                updated_at,
            })
            .collect())
    }

    pub async fn worker_session_matches(
        &self,
        worker_id: &str,
        token: &str,
    ) -> Result<bool, Error> {
        if token.len() < 32 || token.len() > 256 {
            return Ok(false);
        }
        let stored: Option<(String,)> =
            sqlx::query_as::query_as("SELECT token_sha256 FROM worker_sessions WHERE worker_id=?1")
                .bind(worker_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(stored.is_some_and(|(stored,)| {
            constant_time_eq(stored.as_bytes(), sha256_hex(token.as_bytes()).as_bytes())
        }))
    }

    pub async fn claim_attempt(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<Option<WorkerLease>, Error> {
        Ok(self
            .claim_candidate(worker_id, now_ms, lease_ms)
            .await?
            .map(|(lease, _)| lease))
    }

    /// Claim a fixture-authorized Attempt and return the frozen assignment data
    /// the Worker needs. Provider/isolated modes are not returned by this slice.
    pub async fn claim_assignment(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<Option<WorkerAssignment>, Error> {
        let Some((lease, attempt_id)) = self.claim_candidate(worker_id, now_ms, lease_ms).await?
        else {
            return Ok(None);
        };
        match self.load_assignment(&lease, &attempt_id).await {
            Ok(assignment) => Ok(Some(assignment)),
            Err(error) => {
                let _ = self.release_claim(&lease, &attempt_id).await;
                Err(error)
            }
        }
    }

    async fn load_assignment(
        &self,
        lease: &WorkerLease,
        attempt_id: &str,
    ) -> Result<WorkerAssignment, Error> {
        let row = sqlx::query::query(
            "SELECT c.action_id,p.repository,s.spec_json,s.spec_sha256,ca.execution_mode
             FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN projects p ON p.id=ac.project_id
             JOIN action_specs s ON s.action_id=ac.id
             LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             WHERE a.id=?1",
        )
        .bind(attempt_id)
        .fetch_one(&self.pool)
        .await?;
        use sqlx::row::Row;
        let action_id: String = row.try_get("action_id")?;
        let repository: String = row.try_get("repository")?;
        let spec_json: String = row.try_get("spec_json")?;
        let spec_sha256: String = row.try_get("spec_sha256")?;
        let execution_mode: Option<String> = row.try_get("execution_mode")?;
        let execution_mode = execution_mode.unwrap_or_else(|| "ISOLATED".into());
        if execution_mode != "FIXTURE" {
            return Err(Error::Forbidden(
                "only deterministic fixture assignments are enabled".into(),
            ));
        }
        let spec: ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        if sha256_hex(spec_json.as_bytes()) != spec_sha256 {
            return Err(Error::Invalid("stored ActionSpec digest mismatch"));
        }
        if spec.allowed_paths.is_empty() {
            return Err(Error::Forbidden(
                "assignment has no frozen allowed-path scope".into(),
            ));
        }
        Ok(WorkerAssignment {
            lease: lease.clone(),
            action_id,
            action_spec: spec,
            action_spec_sha256: spec_sha256,
            repository,
            execution_mode,
        })
    }

    async fn release_claim(&self, lease: &WorkerLease, attempt_id: &str) -> Result<(), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        sqlx::query::query("UPDATE attempts SET status='QUEUED' WHERE id=?1 AND status='LEASED'")
            .bind(attempt_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query(
            "DELETE FROM worker_leases WHERE attempt_id=?1 AND worker_id=?2 AND generation=?3",
        )
        .bind(attempt_id)
        .bind(&lease.worker_id)
        .bind(lease.generation)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            attempt_id,
            "worker.claim_released",
            "assignment validation failed before dispatch",
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn claim_candidate(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_ms: i64,
    ) -> Result<Option<(WorkerLease, String)>, Error> {
        if worker_id.trim().is_empty() || lease_ms <= 0 || lease_ms > 60_000 {
            return Err(Error::Invalid("worker lease request"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let candidate: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT a.id FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE a.status IN ('QUEUED','LEASED') AND c.status='AUTHORIZED'
             AND ca.execution_mode='FIXTURE'
             AND (l.attempt_id IS NULL OR l.expires_at_ms<=?1)
             ORDER BY a.created_at,a.id LIMIT 1",
        )
        .bind(now_ms)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((attempt_id,)) = candidate else {
            transaction.commit().await?;
            return Ok(None);
        };
        let prior: Option<(String, i64)> = sqlx::query_as::query_as(
            "SELECT worker_id,generation FROM worker_leases WHERE attempt_id=?1",
        )
        .bind(&attempt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let generation = match prior {
            Some((_, generation)) => generation
                .checked_add(1)
                .ok_or(Error::Invalid("worker generation exhausted"))?,
            None => 1,
        };
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or(Error::Invalid("worker lease expiry overflow"))?;
        sqlx::query::query(
            "INSERT INTO worker_leases VALUES (?1,?2,?3,?4)
             ON CONFLICT(attempt_id) DO UPDATE SET
               worker_id=excluded.worker_id,
               generation=excluded.generation,
               expires_at_ms=excluded.expires_at_ms",
        )
        .bind(&attempt_id)
        .bind(worker_id)
        .bind(generation)
        .bind(expires_at_ms)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("UPDATE attempts SET status='LEASED' WHERE id=?1")
            .bind(&attempt_id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &attempt_id,
            "worker.claimed",
            &format!("generation={generation}"),
        )
        .await?;
        transaction.commit().await?;
        Ok(Some((
            WorkerLease {
                attempt_id: attempt_id.clone(),
                worker_id: worker_id.into(),
                generation,
                expires_at_ms,
            },
            attempt_id,
        )))
    }

    pub async fn heartbeat(
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
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current: Option<(String, i64, i64, String)> = sqlx::query_as::query_as(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status
             FROM worker_leases l JOIN attempts a ON a.id=l.attempt_id
             WHERE l.attempt_id=?1",
        )
        .bind(attempt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (owner, saved_generation, expires_at, status) = current.ok_or(Error::NotFound)?;
        if owner != worker_id || saved_generation != generation || expires_at <= now_ms {
            return Err(Error::Invalid("stale or expired worker lease"));
        }
        if status != "LEASED" {
            return Err(Error::Invalid("attempt is not leased"));
        }
        let expires_at_ms = now_ms
            .checked_add(lease_ms)
            .ok_or_else(|| Error::Invalid("worker lease expiry overflow"))?;
        sqlx::query::query(
            "UPDATE worker_leases SET expires_at_ms=?1
             WHERE attempt_id=?2 AND worker_id=?3 AND generation=?4",
        )
        .bind(expires_at_ms)
        .bind(attempt_id)
        .bind(worker_id)
        .bind(generation)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(WorkerLease {
            attempt_id: attempt_id.into(),
            worker_id: worker_id.into(),
            generation,
            expires_at_ms,
        })
    }

    pub async fn report_terminal(&self, report: &WorkerReport, now_ms: i64) -> Result<(), Error> {
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
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let receipt: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT request_digest,attempt_id FROM worker_receipts WHERE receipt_id=?1",
        )
        .bind(&report.receipt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((saved_digest, saved_attempt)) = receipt {
            if saved_digest == digest && saved_attempt == report.attempt_id {
                transaction.commit().await?;
                return Ok(());
            }
            return Err(Error::Invalid("receipt id reused with different report"));
        }
        let lease: Option<(String, i64, i64)> = sqlx::query_as::query_as(
            "SELECT worker_id,generation,expires_at_ms FROM worker_leases WHERE attempt_id=?1",
        )
        .bind(&report.attempt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (owner, generation, expires_at_ms) = lease.ok_or(Error::NotFound)?;
        if owner != report.worker_id || generation != report.generation || expires_at_ms <= now_ms {
            return Err(Error::Invalid("stale or expired worker lease"));
        }
        let changed =
            sqlx::query::query("UPDATE attempts SET status=?1 WHERE id=?2 AND status='LEASED'")
                .bind(&report.terminal_status)
                .bind(&report.attempt_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if changed != 1 {
            return Err(Error::Invalid("attempt is not leased"));
        }
        sqlx::query::query("INSERT INTO worker_receipts VALUES (?1,?2,?3,?4)")
            .bind(&report.receipt_id)
            .bind(&digest)
            .bind(&report.attempt_id)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &report.attempt_id,
            "worker.terminal_reported",
            &report.terminal_status,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}
