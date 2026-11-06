use super::*;

use chrono::Utc;
use uuid::Uuid;

fn validate_assignment_provider_route(
    execution_mode: &str,
    provider: &str,
    model: &str,
    provider_mode: &str,
    delegation_id: &str,
) -> Result<(), Error> {
    if execution_mode == "TRUSTED_LOCAL_PROVIDER"
        && (provider_mode.is_empty() || delegation_id.is_empty())
    {
        return Err(Error::Forbidden(
            "provider assignment has no route identity".into(),
        ));
    }
    if execution_mode == "TRUSTED_LOCAL_PROVIDER" && (provider.is_empty() || model.is_empty()) {
        return Err(Error::Forbidden(
            "provider assignment has no authorized route".into(),
        ));
    }
    Ok(())
}

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

    /// Return the provider route for a live Worker lease. This binds provider
    /// credential disclosure to the exact attempt, generation and worker.
    pub async fn assigned_provider_route(
        &self,
        worker_id: &str,
        attempt_id: &str,
        generation: i64,
        now_ms: i64,
    ) -> Result<(String, String), Error> {
        if worker_id.trim().is_empty() || attempt_id.trim().is_empty() || generation <= 0 {
            return Err(Error::Invalid("provider assignment identity"));
        }
        let row: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT COALESCE(ca.provider,''),COALESCE(ca.model,'')
             FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             JOIN execution_policies p ON p.policy_id=ca.policy_id
             JOIN standing_delegations d ON d.id=ca.delegation_id
             JOIN worker_leases wl ON wl.attempt_id=a.id
             WHERE a.id=?1 AND wl.worker_id=?2 AND wl.generation=?3
               AND wl.expires_at_ms>?4 AND a.status='LEASED' AND c.status='AUTHORIZED'
               AND ca.schema_version=2 AND d.schema_version=2 AND d.revoked_at IS NULL
               AND ca.execution_mode='TRUSTED_LOCAL_PROVIDER'",
        )
        .bind(attempt_id)
        .bind(worker_id)
        .bind(generation)
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await?;
        let Some((provider, model)) = row else {
            return Err(Error::Forbidden(
                "no live V2 authorized provider assignment for this Worker attempt".into(),
            ));
        };
        if provider.is_empty() || model.is_empty() {
            return Err(Error::Forbidden(
                "provider assignment has no authorized route".into(),
            ));
        }
        Ok((provider, model))
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

    /// Claim an authorized Attempt and return the frozen assignment data the
    /// Worker needs. Isolated mode is never returned.
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
        let mut connection = self.pool.acquire().await?;
        super::lead::validate_lead_attempt_lineage(&mut connection, attempt_id).await?;
        let row = sqlx::query::query(
            "SELECT COALESCE(aps.action_id,c.action_id) AS action_id,
                    r.campaign_id,COALESCE(lt.unit_id,li.integration_id,c.id) AS work_item_id,
                    p.repository,
                    COALESCE(aps.spec_json,s.spec_json) AS spec_json,
                    COALESCE(aps.spec_sha256,s.spec_sha256) AS spec_sha256,
                    COALESCE(aps.base_commit,p.base_commit) AS base_commit,
                    COALESCE(aps.kind,'UNIT') AS kind,
                    COALESCE(aps.integration_candidates_json,'[]') AS integration_candidates,
                    ca.execution_mode,COALESCE(ca.provider,'') AS provider,
                    COALESCE(ca.model,'') AS model,COALESCE(ca.provider_mode,'') AS provider_mode,
                    COALESCE(ca.delegation_id,'') AS delegation_id,
                    ca.schema_version AS authorization_version,
                    ca.policy_id AS policy_id,
                    xp.policy_json AS policy_json,
                    d.revoked_at AS delegation_revoked_at,
                    COALESCE(ca.allowed_tools_json,'[]') AS allowed_tools
             FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN projects p ON p.id=ac.project_id
             JOIN action_specs s ON s.action_id=ac.id
             LEFT JOIN attempt_specs aps ON aps.attempt_id=a.id
             LEFT JOIN lead_tasks lt ON lt.attempt_id=a.id
             LEFT JOIN lead_integrations li ON li.attempt_id=a.id
             LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             LEFT JOIN execution_policies xp ON xp.policy_id=ca.policy_id
             LEFT JOIN standing_delegations d ON d.id=ca.delegation_id
             WHERE a.id=?1",
        )
        .bind(attempt_id)
        .fetch_one(&self.pool)
        .await?;
        use sqlx::row::Row;
        let action_id: String = row.try_get("action_id")?;
        let campaign_id: String = row.try_get("campaign_id")?;
        let work_item_id: String = row.try_get("work_item_id")?;
        let progress_last_sequence: u64 = {
            let state_json: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT state_json FROM campaign_progress_state WHERE campaign_id=?1",
            )
            .bind(&campaign_id)
            .fetch_optional(&self.pool)
            .await?;
            state_json
                .and_then(|(json,)| {
                    serde_json::from_str::<crate::progress::ProgressSupervisorState>(&json).ok()
                })
                .and_then(|state| {
                    state
                        .work_items
                        .values()
                        .map(|item| item.last_sequence)
                        .max()
                })
                .unwrap_or(0)
        };
        let repository: String = row.try_get("repository")?;
        let spec_json: String = row.try_get("spec_json")?;
        let spec_sha256: String = row.try_get("spec_sha256")?;
        let kind: String = row.try_get("kind")?;
        let integration_candidates: String = row.try_get("integration_candidates")?;
        let integration_candidates: Vec<String> = serde_json::from_str(&integration_candidates)
            .map_err(|_| Error::Invalid("stored integration candidates"))?;
        let execution_mode: Option<String> = row.try_get("execution_mode")?;
        let execution_mode = execution_mode.unwrap_or_else(|| "ISOLATED".into());
        if !matches!(
            execution_mode.as_str(),
            "FIXTURE" | "TRUSTED_LOCAL_PROVIDER"
        ) {
            return Err(Error::Forbidden(
                "assignment execution mode is not enabled".into(),
            ));
        }
        let unresolved: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT session_record_id FROM provider_sessions
             WHERE attempt_id=?1 AND state IN ('INTENT','ACTIVE') LIMIT 1",
        )
        .bind(attempt_id)
        .fetch_optional(&self.pool)
        .await?;
        if unresolved.is_some() {
            return Err(Error::Forbidden(
                "unresolved provider session blocks replacement dispatch".into(),
            ));
        }
        let provider: String = row.try_get("provider")?;
        let model: String = row.try_get("model")?;
        let provider_mode: String = row.try_get("provider_mode")?;
        let delegation_id: String = row.try_get("delegation_id")?;
        let allowed_tools: String = row.try_get("allowed_tools")?;
        let revocation: Option<String> = row.try_get("delegation_revoked_at")?;
        let authorization_version: Option<i64> = row.try_get("authorization_version")?;
        if authorization_version != Some(2) {
            return Err(Error::Forbidden(
                "legacy V1 authorization is historical evidence and cannot be resumed; create a linked V2 request"
                    .into(),
            ));
        }
        let policy_json: Option<String> = row.try_get("policy_json")?;
        let policy_json = policy_json
            .ok_or_else(|| Error::Forbidden("V2 assignment has no frozen policy".into()))?;
        let execution_policy = super::authorization::decode_execution_policy(&policy_json)?;
        validate_assignment_provider_route(
            &execution_mode,
            &provider,
            &model,
            &provider_mode,
            &delegation_id,
        )?;
        if revocation.is_some() {
            return Err(Error::Forbidden(
                "delegation was revoked; assignment is no longer authorized".into(),
            ));
        }
        if let Option::<String>::Some(policy_id) = row.try_get("policy_id")?
            && policy_id != execution_policy.policy_id
        {
            return Err(Error::Invalid("assignment policy identity mismatch"));
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
            campaign_id,
            work_item_id,
            progress_last_sequence,
            action_id,
            action_spec: spec,
            action_spec_sha256: spec_sha256,
            repository,
            execution_mode,
            kind,
            integration_candidates,
            provider,
            model,
            provider_mode,
            delegation_id,
            execution_policy: Some(crate::execution_policy::FrozenExecutionPolicy::V2(
                execution_policy,
            )),
            allowed_tools: serde_json::from_str(&allowed_tools)
                .map_err(|_| Error::Invalid("stored allowed tools encoding"))?,
            attempt_grant: String::new(),
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
        let candidate: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT a.id,a.run_id FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             LEFT JOIN lead_tasks lt ON lt.attempt_id=a.id
             LEFT JOIN lead_integrations li ON li.attempt_id=a.id
             WHERE a.status IN ('QUEUED','LEASED') AND c.status='AUTHORIZED'
             AND r.status IN ('QUEUED','RUNNING','REPAIR_REQUIRED')
             AND ca.execution_mode IN ('FIXTURE','TRUSTED_LOCAL_PROVIDER')
             AND (l.attempt_id IS NULL OR l.expires_at_ms<=?1)
             AND NOT EXISTS (
               SELECT 1 FROM progress_holds ph
               WHERE ph.campaign_id=c.id
                 AND ph.work_item_id=COALESCE(lt.unit_id,li.integration_id,c.id)
                 AND ph.resolved_at IS NULL
             )
             ORDER BY a.created_at,a.id LIMIT 1",
        )
        .bind(now_ms)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((attempt_id, run_id)) = candidate else {
            transaction.commit().await?;
            return Ok(None);
        };
        super::lead::validate_lead_attempt_lineage(&mut transaction, &attempt_id).await?;
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
        let run_changed =
            sqlx::query::query(
                "UPDATE runs SET status='RUNNING' WHERE id=?1 AND status IN ('QUEUED','REPAIR_REQUIRED')",
            )
                .bind(&run_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if run_changed == 1 {
            record_event(
                &mut transaction,
                &run_id,
                "run.status_changed",
                &format!("status=RUNNING attempt={attempt_id}"),
            )
            .await?;
        }
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

    pub async fn workspace_operation_authorization(
        &self,
        attempt_id: &str,
        worker_id: &str,
        generation: i64,
        operation: &str,
    ) -> Result<WorkspaceOperationAuthorization, Error> {
        if !matches!(
            operation,
            "write" | "run" | "finalize" | "apply" | "execute"
        ) {
            return Err(Error::Invalid("workspace operation"));
        }
        let now_ms = Utc::now().timestamp_millis();
        let row = sqlx::query::query(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status,c.status AS campaign_status,
                    ca.execution_mode AS execution_mode,
                    COALESCE(aps.spec_json,s.spec_json) AS spec_json
             FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN action_specs s ON s.action_id=ac.id
             LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
             LEFT JOIN attempt_specs aps ON aps.attempt_id=a.id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE a.id=?1",
        )
        .bind(attempt_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Err(Error::NotFound);
        };
        use sqlx::row::Row;
        let lease_worker: Option<String> = row.try_get("worker_id")?;
        let lease_generation: Option<i64> = row.try_get("generation")?;
        let lease_expiry: Option<i64> = row.try_get("expires_at_ms")?;
        let attempt_status: String = row.try_get("status")?;
        let campaign_status: String = row.try_get("campaign_status")?;
        let execution_mode: Option<String> = row.try_get("execution_mode")?;
        if lease_worker.as_deref() != Some(worker_id)
            || lease_generation != Some(generation)
            || lease_expiry.is_none_or(|expiry| expiry <= now_ms)
            || attempt_status != "LEASED"
            || campaign_status != "AUTHORIZED"
        {
            return Err(Error::Forbidden(
                "workspace operation is not authorized by the current lease".into(),
            ));
        }
        if operation == "execute" && execution_mode.as_deref() != Some("FIXTURE") {
            return Err(Error::Forbidden(
                "execute operation is reserved for FIXTURE attempts".into(),
            ));
        }
        if matches!(operation, "write" | "run" | "finalize" | "apply") {
            let provider_state: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT state FROM provider_sessions WHERE attempt_id=?1 AND generation=?2",
            )
            .bind(attempt_id)
            .bind(generation)
            .fetch_optional(&self.pool)
            .await?;
            if provider_state.as_ref().map(|(state,)| state.as_str()) != Some("ACTIVE") {
                return Err(Error::Forbidden(
                    "workspace operation requires an active provider session".into(),
                ));
            }
        }
        let spec_json: String = row.try_get("spec_json")?;
        let spec: ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        let checks = spec
            .checks
            .iter()
            .map(|check| check.command.clone())
            .collect();
        Ok(WorkspaceOperationAuthorization {
            attempt_id: attempt_id.into(),
            worker_id: worker_id.into(),
            generation,
            allowed_paths: spec.allowed_paths,
            checks,
        })
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
            .ok_or_else(|| Error::Invalid("worker lease expiry overflow"))?
            .max(expires_at);
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
            || !matches!(
                report.terminal_status.as_str(),
                "FAILED" | "CANCELLED" | "BLOCKED"
            )
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
        let lease: Option<(String, i64, i64, String)> = sqlx::query_as::query_as(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,a.run_id
             FROM worker_leases l JOIN attempts a ON a.id=l.attempt_id
             WHERE l.attempt_id=?1",
        )
        .bind(&report.attempt_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (owner, generation, expires_at_ms, run_id) = lease.ok_or(Error::NotFound)?;
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
        let run_changed = sqlx::query::query(
            "UPDATE runs SET status=?1 WHERE id=?2 AND status IN ('QUEUED','RUNNING','REPAIR_REQUIRED')",
        )
        .bind(&report.terminal_status)
        .bind(&run_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if run_changed == 1 {
            record_event(
                &mut transaction,
                &run_id,
                "run.status_changed",
                &format!(
                    "status={} attempt={}",
                    report.terminal_status, report.attempt_id
                ),
            )
            .await?;
        }
        if report.terminal_status == "BLOCKED" {
            // A deterministic progress hold is a control state, not a failed
            // campaign. Preserve Lead work and record a hold so no attempt can
            // be redispatched until an operator decision or verified change.
            let work_item: Option<(String, String)> = sqlx::query_as::query_as(
                "SELECT r.campaign_id,COALESCE(lt.unit_id,r.campaign_id)
                 FROM attempts a
                 JOIN runs r ON r.id=a.run_id
                 LEFT JOIN lead_tasks lt ON lt.attempt_id=a.id
                 WHERE a.id=?1",
            )
            .bind(&report.attempt_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if let Some((campaign_id, work_item_id)) = work_item {
                sqlx::query::query(
                    "INSERT INTO progress_holds
                     (hold_id,campaign_id,work_item_id,state,reason,checkpoint_id,created_at)
                     SELECT ?1,?2,?3,'BLOCKED',?4,NULL,?5
                     WHERE NOT EXISTS (
                       SELECT 1 FROM progress_holds
                       WHERE campaign_id=?2 AND work_item_id=?3 AND resolved_at IS NULL
                     )",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&campaign_id)
                .bind(&work_item_id)
                .bind(format!(
                    "worker reported BLOCKED receipt={} result={}",
                    report.receipt_id, report.result_sha256
                ))
                .bind(Utc::now().to_rfc3339())
                .execute(&mut *transaction)
                .await?;
            }
        } else {
            super::lead::fail_lead_work(
                &mut transaction,
                &report.attempt_id,
                &format!("worker terminal {}", report.terminal_status),
            )
            .await?;
        }
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

#[cfg(test)]
mod tests {
    use super::validate_assignment_provider_route;
    use crate::db::Error;

    #[test]
    fn fixture_assignments_do_not_require_provider_route_identity() {
        assert!(validate_assignment_provider_route("FIXTURE", "", "", "", "").is_ok());
    }

    #[test]
    fn provider_assignments_require_route_and_delegation_identity() {
        assert!(matches!(
            validate_assignment_provider_route(
                "TRUSTED_LOCAL_PROVIDER",
                "opencode",
                "model",
                "build",
                ""
            ),
            Err(Error::Forbidden(message)) if message == "provider assignment has no route identity"
        ));
        assert!(matches!(
            validate_assignment_provider_route(
                "TRUSTED_LOCAL_PROVIDER",
                "opencode",
                "model",
                "",
                "delegation"
            ),
            Err(Error::Forbidden(message)) if message == "provider assignment has no route identity"
        ));
        assert!(matches!(
            validate_assignment_provider_route(
                "TRUSTED_LOCAL_PROVIDER",
                "",
                "model",
                "build",
                "delegation"
            ),
            Err(Error::Forbidden(message)) if message == "provider assignment has no authorized route"
        ));
    }
}
