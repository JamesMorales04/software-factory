use super::*;

use crate::autonomy::{self, RepairDecision, RepairRequest};
use crate::execution_policy::AutonomyPolicy;
use crate::execution_profile::ExecutionProfileContent;
use crate::semantic_review::ReviewerSelection;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};
use std::collections::BTreeMap;
use std::path::Path;
use uuid::Uuid;

impl Database {
    pub async fn record_candidate(
        &self,
        input: &NewCandidate,
        now_ms: i64,
    ) -> Result<CandidateRecord, Error> {
        validate_candidate_input(input)?;
        let context = {
            let mut connection = self.pool.acquire().await?;
            candidate_context(&mut connection, &input.attempt_id).await?
        };
        require_live_lease(&context, &input.worker_id, input.generation, now_ms)?;
        if context.kind == "INTEGRATION" {
            return Err(Error::Forbidden(
                "integration attempts require the integrated-candidate endpoint".into(),
            ));
        }
        if context.spec_sha256 != input.action_spec_sha256 {
            return Err(Error::Forbidden(
                "candidate ActionSpec hash does not match the frozen spec".into(),
            ));
        }
        let repository = context.repository.clone();
        let base_commit = context.base_commit.clone();
        let commit_sha = input.commit_sha.clone();
        let tree_sha = input.tree_sha.clone();
        let verified = tokio::task::spawn_blocking(move || {
            verify_candidate_commit(Path::new(&repository), &base_commit, &commit_sha, &tree_sha)
        })
        .await
        .map_err(|error| {
            Error::Forbidden(format!("candidate lineage verification failed: {error}"))
        })?
        .map_err(|error| Error::Forbidden(format!("candidate lineage rejected: {error}")))?;
        let spec: ActionSpec = serde_json::from_str(&context.spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        if !spec.allowed_paths.is_empty() {
            let repository = context.repository.clone();
            let base_commit = context.base_commit.clone();
            let commit_sha = verified.commit_sha.clone();
            let changed = tokio::task::spawn_blocking(move || {
                crate::workspace::changed_paths(Path::new(&repository), &base_commit, &commit_sha)
            })
            .await
            .map_err(|error| Error::Forbidden(format!("candidate path task failed: {error}")))?
            .map_err(|error| Error::Forbidden(format!("candidate path check failed: {error}")))?;
            if let Some(path) = changed
                .iter()
                .find(|path| !crate::workspace::path_covered(path, &spec.allowed_paths))
            {
                return Err(Error::Forbidden(format!(
                    "candidate path {path:?} is outside the frozen allowed paths"
                )));
            }
        }

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = candidate_context(&mut transaction, &input.attempt_id).await?;
        if current.spec_sha256 != input.action_spec_sha256
            || current.repository != context.repository
            || current.base_commit != context.base_commit
        {
            return Err(Error::Conflict(
                "attempt binding changed during candidate validation",
            ));
        }
        if let Some(existing) =
            load_candidate_for_attempt(&mut transaction, &input.attempt_id).await?
        {
            if existing.implementer_id == input.worker_id
                && existing.action_spec_sha256 == input.action_spec_sha256
                && existing
                    .commit_sha
                    .eq_ignore_ascii_case(&verified.commit_sha)
                && existing.tree_sha.eq_ignore_ascii_case(&verified.tree_sha)
            {
                transaction.commit().await?;
                return Ok(existing);
            }
            return Err(Error::Conflict(
                "attempt already recorded a different candidate revision",
            ));
        }
        require_live_lease(
            &current,
            &input.worker_id,
            input.generation,
            Utc::now().timestamp_millis(),
        )?;
        let candidate = CandidateRecord {
            id: Uuid::new_v4().to_string(),
            attempt_id: input.attempt_id.clone(),
            action_spec_sha256: input.action_spec_sha256.clone(),
            commit_sha: verified.commit_sha.clone(),
            tree_sha: verified.tree_sha.clone(),
            implementer_id: input.worker_id.clone(),
            created_at: Utc::now().to_rfc3339(),
            integration: false,
            lineage: Vec::new(),
        };
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,integration,lineage_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,'[]')",
        )
            .bind(&candidate.id)
            .bind(&candidate.attempt_id)
            .bind(&candidate.action_spec_sha256)
            .bind(&candidate.commit_sha)
            .bind(&candidate.tree_sha)
            .bind(&candidate.implementer_id)
            .bind(&candidate.created_at)
            .execute(&mut *transaction)
            .await?;
        let changed = sqlx::query::query(
            "UPDATE attempts SET status='CANDIDATE_READY' WHERE id=?1 AND status='LEASED'",
        )
        .bind(&candidate.attempt_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("attempt is no longer leased"));
        }
        super::lead::advance_lead_task(
            &mut transaction,
            &candidate.attempt_id,
            &candidate.commit_sha,
        )
        .await?;
        record_event(
            &mut transaction,
            &candidate.id,
            "candidate.recorded",
            &format!(
                "attempt={} commit={} tree={}",
                candidate.attempt_id, candidate.commit_sha, candidate.tree_sha
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(candidate)
    }

    pub async fn record_quality_receipt(
        &self,
        candidate_id: &str,
        role: QualityRole,
        input: &QualityReceiptInput,
    ) -> Result<QualityIngestResult, Error> {
        validate_receipt_input(role, input)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let candidate = validate_receipt_binding(&mut transaction, candidate_id, input).await?;
        if role == QualityRole::Reviewer {
            let requirement =
                reviewer_requirement_for_candidate(&mut transaction, candidate_id).await?;
            if !requirement.roles.is_empty() {
                let Some(reviewer_role) = input.reviewer_role else {
                    return Err(Error::Forbidden(
                        "reviewer role is required by the frozen reviewer plan".into(),
                    ));
                };
                if !requirement.roles.contains(&reviewer_role) {
                    return Err(Error::Forbidden(
                        "reviewer role is not required by the frozen reviewer plan".into(),
                    ));
                }
            }
        }
        if let Some(receipt) = replayed_receipt(&mut transaction, candidate_id, role, input).await?
        {
            let gate = load_gate(&mut transaction, candidate_id).await?;
            transaction.commit().await?;
            return Ok(QualityIngestResult { receipt, gate });
        }
        ensure_receipt_available(&mut transaction, candidate_id, role, input).await?;
        validate_model_review_receipt(&mut transaction, candidate_id, role, input).await?;
        let receipt =
            insert_receipt(&mut transaction, candidate_id, role, input, &candidate).await?;
        let gate = decide_gate(&mut transaction, candidate_id, &candidate).await?;
        transaction.commit().await?;
        Ok(QualityIngestResult { receipt, gate })
    }

    /// Candidates awaiting one independent quality role. The reviewer queue is
    /// gated on an executed verifier PASS for the same immutable candidate.
    pub async fn pending_quality_candidates(&self, role: &str) -> Result<Vec<String>, Error> {
        if !matches!(role, "verifier" | "reviewer") {
            return Err(Error::Invalid("pending quality role"));
        }
        let required_reviewers = "COALESCE(
               (SELECT rr.reviewer_count FROM candidates c2
                JOIN attempts a2 ON a2.id=c2.attempt_id
                JOIN runs r2 ON r2.id=a2.run_id
                LEFT JOIN campaign_review_requirements rr ON rr.campaign_id=r2.campaign_id
                WHERE c2.id=c.id),
               (SELECT pp.reviewer_count FROM candidates c2
                JOIN attempts a2 ON a2.id=c2.attempt_id
                JOIN runs r2 ON r2.id=a2.run_id
                JOIN intake_requests ir ON ir.campaign_id=r2.campaign_id
                JOIN agentic_participant_plans pp ON pp.request_id=ir.id
                WHERE c2.id=c.id),
               1)";
        let missing = match role {
            "verifier" => {
                "NOT EXISTS (SELECT 1 FROM quality_receipts r
                                       WHERE r.candidate_id=c.id AND r.role='verifier')"
            }
            "reviewer" => {
                "((SELECT COUNT(DISTINCT r.principal_id) FROM quality_receipts r
                             WHERE r.candidate_id=c.id AND r.role='reviewer') < required_reviewers)"
            }
            _ => unreachable!(),
        };
        let reviewer_gate = if role == "reviewer" {
            "AND EXISTS (SELECT 1 FROM quality_receipts vr
                         WHERE vr.candidate_id=c.id AND vr.role='verifier' AND vr.result='PASS')"
        } else {
            ""
        };
        let missing = missing.replace("required_reviewers", required_reviewers);
        let query = format!(
            "SELECT c.id FROM candidates c
             WHERE {missing}
               AND NOT EXISTS (SELECT 1 FROM gate_decisions g WHERE g.candidate_id=c.id)
               {reviewer_gate}
             ORDER BY c.created_at ASC
             LIMIT 8"
        );
        let rows: Vec<(String,)> = sqlx::query_as::query_as(&query)
            .bind(role)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Candidates that this authenticated reviewer identity may still review:
    /// the verifier must have passed, the Gate must be undecided, the
    /// candidate must still need this role, and neither this principal nor
    /// this role may already have a receipt. This prevents a fleet from
    /// double-reviewing a role while Core remains the authority.
    pub async fn pending_quality_candidates_for_reviewer(
        &self,
        principal_id: &str,
        reviewer_role: crate::planning::ReviewerRole,
    ) -> Result<Vec<String>, Error> {
        if principal_id.trim().is_empty() {
            return Err(Error::Invalid("reviewer principal"));
        }
        let rows: Vec<(String,)> = sqlx::query_as::query_as(
            "SELECT c.id FROM candidates c
             WHERE NOT EXISTS (SELECT 1 FROM gate_decisions g WHERE g.candidate_id=c.id)
               AND EXISTS (SELECT 1 FROM quality_receipts vr
                           WHERE vr.candidate_id=c.id AND vr.role='verifier' AND vr.result='PASS')
               AND NOT EXISTS (SELECT 1 FROM quality_receipts r
                               WHERE r.candidate_id=c.id AND r.role='reviewer'
                                 AND (r.principal_id=?1 OR r.reviewer_role=?2))
             ORDER BY c.created_at ASC
             LIMIT 32",
        )
        .bind(principal_id)
        .bind(reviewer_role.as_str())
        .fetch_all(&self.pool)
        .await?;
        let mut pending = Vec::new();
        for (candidate_id,) in rows {
            let requirement = {
                let mut connection = self.pool.acquire().await?;
                reviewer_requirement_for_candidate(&mut connection, &candidate_id).await?
            };
            if !requirement.roles.is_empty() && !requirement.roles.contains(&reviewer_role) {
                continue;
            }
            let receipts = self.quality_receipts(&candidate_id).await?;
            let covered = receipts.iter().any(|receipt| {
                receipt.role == QualityRole::Reviewer
                    && (receipt.principal_id == principal_id
                        || receipt.reviewer_role == Some(reviewer_role))
            });
            let independent = receipts
                .iter()
                .filter(|receipt| receipt.role == QualityRole::Reviewer)
                .map(|receipt| receipt.principal_id.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            let covered_roles = receipts
                .iter()
                .filter(|receipt| receipt.role == QualityRole::Reviewer)
                .filter_map(|receipt| receipt.reviewer_role)
                .filter(|role| requirement.roles.contains(role))
                .collect::<std::collections::BTreeSet<_>>();
            let role_missing =
                !requirement.roles.is_empty() && !covered_roles.contains(&reviewer_role);
            let count_missing = independent < requirement.count;
            if !covered && (role_missing || count_missing) {
                pending.push(candidate_id);
            }
        }
        pending.truncate(8);
        Ok(pending)
    }

    pub async fn candidate_inspection(&self, id: &str) -> Result<CandidateInspection, Error> {
        let candidate = self.candidate(id).await?;
        let mut connection = self.pool.acquire().await?;
        super::lead::validate_lead_attempt_lineage(&mut connection, &candidate.attempt_id).await?;
        let row = sqlx::query::query(
            "SELECT COALESCE(aps.action_id,c.action_id) AS action_id,p.repository,
                    COALESCE(aps.base_commit,p.base_commit) AS base_commit,
                    COALESCE(aps.spec_json,s.spec_json) AS spec_json,
                    COALESCE(aps.spec_sha256,s.spec_sha256) AS spec_sha256
             FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN projects p ON p.id=ac.project_id
             JOIN action_specs s ON s.action_id=ac.id
             LEFT JOIN attempt_specs aps ON aps.attempt_id=a.id
             WHERE cd.id=?1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        use sqlx::row::Row;
        let action_id: String = row.try_get("action_id")?;
        let repository: String = row.try_get("repository")?;
        let base_commit: String = row.try_get("base_commit")?;
        let spec_json: String = row.try_get("spec_json")?;
        let spec_sha256: String = row.try_get("spec_sha256")?;
        if candidate.action_spec_sha256 != spec_sha256
            || sha256_hex(spec_json.as_bytes()) != spec_sha256
        {
            return Err(Error::Invalid("candidate ActionSpec binding mismatch"));
        }
        let action_spec: ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        Ok(CandidateInspection {
            integration: candidate.integration,
            lineage: candidate.lineage.clone(),
            candidate,
            action_id,
            action_spec,
            action_spec_sha256: spec_sha256,
            repository,
            base_commit,
        })
    }

    pub async fn record_integrated_candidate(
        &self,
        input: &NewIntegratedCandidate,
        now_ms: i64,
    ) -> Result<CandidateRecord, Error> {
        if input.attempt_id.trim().is_empty()
            || input.worker_id.trim().is_empty()
            || input.generation <= 0
            || !is_sha256(&input.action_spec_sha256)
            || !is_git_oid(&input.commit_sha)
            || !is_git_oid(&input.tree_sha)
        {
            return Err(Error::Invalid("integrated candidate submission"));
        }
        let context = {
            let mut connection = self.pool.acquire().await?;
            candidate_context(&mut connection, &input.attempt_id).await?
        };
        require_live_lease(&context, &input.worker_id, input.generation, now_ms)?;
        if context.kind != "INTEGRATION" || context.integration_candidates.len() < 2 {
            return Err(Error::Forbidden(
                "attempt is not a multi-candidate integration".into(),
            ));
        }
        if context.spec_sha256 != input.action_spec_sha256 {
            return Err(Error::Forbidden(
                "integration ActionSpec hash does not match the frozen spec".into(),
            ));
        }
        let repository = context.repository.clone();
        let base_commit = context.base_commit.clone();
        let commit_sha = input.commit_sha.clone();
        let tree_sha = input.tree_sha.clone();
        let expected = context.integration_candidates.clone();
        let verified = tokio::task::spawn_blocking(move || {
            crate::workspace::verify_integrated_candidate(
                Path::new(&repository),
                &base_commit,
                &commit_sha,
                &tree_sha,
                &expected,
            )
        })
        .await
        .map_err(|error| {
            Error::Forbidden(format!("integration lineage verification failed: {error}"))
        })?
        .map_err(|error| Error::Forbidden(format!("integration lineage rejected: {error}")))?;
        let spec: ActionSpec = serde_json::from_str(&context.spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        if !spec.allowed_paths.is_empty() {
            let repository = context.repository.clone();
            let base_commit = context.base_commit.clone();
            let commit_sha = verified.commit_sha.clone();
            let changed = tokio::task::spawn_blocking(move || {
                crate::workspace::changed_paths(Path::new(&repository), &base_commit, &commit_sha)
            })
            .await
            .map_err(|error| Error::Forbidden(format!("integration path task failed: {error}")))?
            .map_err(|error| Error::Forbidden(format!("integration path check failed: {error}")))?;
            if let Some(path) = changed
                .iter()
                .find(|path| !crate::workspace::path_covered(path, &spec.allowed_paths))
            {
                return Err(Error::Forbidden(format!(
                    "integrated path {path:?} is outside the frozen allowed paths"
                )));
            }
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = candidate_context(&mut transaction, &input.attempt_id).await?;
        if current.spec_sha256 != input.action_spec_sha256
            || current.repository != context.repository
            || current.base_commit != context.base_commit
            || current.integration_candidates != context.integration_candidates
        {
            return Err(Error::Conflict(
                "integration binding changed during candidate validation",
            ));
        }
        if let Some(existing) =
            load_candidate_for_attempt(&mut transaction, &input.attempt_id).await?
        {
            if existing.integration
                && existing.implementer_id == input.worker_id
                && existing.action_spec_sha256 == input.action_spec_sha256
                && existing
                    .commit_sha
                    .eq_ignore_ascii_case(&verified.commit_sha)
                && existing.tree_sha.eq_ignore_ascii_case(&verified.tree_sha)
            {
                transaction.commit().await?;
                return Ok(existing);
            }
            return Err(Error::Conflict(
                "integration attempt already recorded a different candidate",
            ));
        }
        require_live_lease(
            &current,
            &input.worker_id,
            input.generation,
            Utc::now().timestamp_millis(),
        )?;
        let lineage_json = serde_json::to_string(&verified.parent_shas)
            .map_err(|_| Error::Invalid("integration lineage encoding"))?;
        let candidate = CandidateRecord {
            id: Uuid::new_v4().to_string(),
            attempt_id: input.attempt_id.clone(),
            action_spec_sha256: input.action_spec_sha256.clone(),
            commit_sha: verified.commit_sha.clone(),
            tree_sha: verified.tree_sha.clone(),
            implementer_id: input.worker_id.clone(),
            created_at: Utc::now().to_rfc3339(),
            integration: true,
            lineage: verified.parent_shas.clone(),
        };
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,integration,lineage_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8)",
        )
        .bind(&candidate.id)
        .bind(&candidate.attempt_id)
        .bind(&candidate.action_spec_sha256)
        .bind(&candidate.commit_sha)
        .bind(&candidate.tree_sha)
        .bind(&candidate.implementer_id)
        .bind(&candidate.created_at)
        .bind(lineage_json)
        .execute(&mut *transaction)
        .await?;
        let changed = sqlx::query::query(
            "UPDATE attempts SET status='CANDIDATE_READY' WHERE id=?1 AND status='LEASED'",
        )
        .bind(&candidate.attempt_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("integration attempt is no longer leased"));
        }
        super::lead::advance_lead_task(
            &mut transaction,
            &candidate.attempt_id,
            &candidate.commit_sha,
        )
        .await?;
        record_event(
            &mut transaction,
            &candidate.id,
            "candidate.integrated",
            &format!(
                "attempt={} commit={} parents={}",
                candidate.attempt_id,
                candidate.commit_sha,
                candidate.lineage.len()
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(candidate)
    }

    pub async fn candidates(&self) -> Result<Vec<CandidateRecord>, Error> {
        let rows: Vec<CandidateColumns> = sqlx::query_as::query_as(
            "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,
                    integration,lineage_json
             FROM candidates ORDER BY created_at,id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(candidate_from_columns).collect())
    }

    /// Latest candidate produced by a Campaign. Repair always starts from the
    /// exact rejected candidate, never from the Project base commit.
    pub async fn latest_candidate_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<CandidateRecord, Error> {
        let row: Option<CandidateColumns> = sqlx::query_as::query_as(
            "SELECT cd.id,cd.attempt_id,cd.action_spec_sha256,cd.commit_sha,cd.tree_sha,
                    cd.implementer_id,cd.created_at,cd.integration,cd.lineage_json
             FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             WHERE r.campaign_id=?1
             ORDER BY cd.created_at DESC,cd.id DESC LIMIT 1",
        )
        .bind(campaign_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(candidate_from_columns).ok_or(Error::NotFound)
    }

    /// Accepted repair candidates whose Project base has not necessarily been
    /// advanced yet. The Core watcher uses this to promote the accepted repair
    /// without a second operator action.
    pub async fn accepted_repair_candidates(&self) -> Result<Vec<(String, String)>, Error> {
        let rows: Vec<(String, String)> = sqlx::query_as::query_as(
            "SELECT ir.project_id, cd.commit_sha
             FROM repair_requests rr
             JOIN intake_requests ir ON ir.id=rr.request_id
             JOIN runs r ON r.campaign_id=ir.campaign_id
             JOIN attempts a ON a.run_id=r.id
             JOIN candidates cd ON cd.attempt_id=a.id
             JOIN gate_decisions_current gd ON gd.candidate_id=cd.id
             WHERE gd.outcome='ACCEPTED'
             ORDER BY cd.created_at DESC,cd.id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn repair_request_for_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<Option<String>, Error> {
        let row: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT request_id FROM repair_requests WHERE candidate_id=?1",
        )
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(request_id,)| request_id))
    }

    pub async fn save_repair_request(
        &self,
        candidate_id: &str,
        repair_request_id: &str,
    ) -> Result<String, Error> {
        if candidate_id.trim().is_empty() || repair_request_id.trim().is_empty() {
            return Err(Error::Invalid("repair request link"));
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO repair_requests(candidate_id,request_id,created_at)
             VALUES (?1,?2,?3)
             ON CONFLICT(candidate_id) DO NOTHING",
        )
        .bind(candidate_id)
        .bind(repair_request_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        self.repair_request_for_candidate(candidate_id)
            .await?
            .ok_or(Error::NotFound)
    }

    /// Reviewer receipts aggregated into the immutable Gate decision.
    pub async fn gate_reviewer_ids(&self, candidate_id: &str) -> Result<Vec<String>, Error> {
        let rows: Vec<(String,)> = sqlx::query_as::query_as(
            "SELECT reviewer_receipt_id FROM gate_decision_reviewers
             WHERE candidate_id=?1 ORDER BY reviewer_receipt_id ASC",
        )
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    pub async fn candidate(&self, id: &str) -> Result<CandidateRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_candidate(&mut connection, id).await
    }

    pub async fn reviewer_selection(&self, candidate_id: &str) -> Result<ReviewerSelection, Error> {
        let mut connection = self.pool.acquire().await?;
        reviewer_selection_for_candidate(&mut connection, candidate_id).await
    }

    pub async fn gate_for_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<Option<GateRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_gate(&mut connection, candidate_id).await
    }

    pub async fn repair_authorization_for_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<Option<AutonomyRepairRecord>, Error> {
        let row = sqlx::query::query(
            "SELECT repair_id,campaign_id,candidate_id,work_item_id,repair_trigger,risk_class,
                    total_points,critical_count,blocking_count,warning_count,info_count,
                    cycle_index,decision,reason_code,reason,policy_sha256,reviewer_receipt_id,
                    created_at
             FROM autonomy_repairs WHERE candidate_id=?1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(AutonomyRepairRecord {
            repair_id: row.try_get("repair_id")?,
            campaign_id: row.try_get("campaign_id")?,
            candidate_id: row.try_get("candidate_id")?,
            work_item_id: row.try_get("work_item_id")?,
            repair_trigger: row.try_get("repair_trigger")?,
            risk_class: row.try_get("risk_class")?,
            total_points: row.try_get::<i64, _>("total_points")? as u16,
            critical_count: row.try_get::<i64, _>("critical_count")? as u32,
            blocking_count: row.try_get::<i64, _>("blocking_count")? as u32,
            warning_count: row.try_get::<i64, _>("warning_count")? as u32,
            info_count: row.try_get::<i64, _>("info_count")? as u32,
            cycle_index: row.try_get::<i64, _>("cycle_index")? as u32,
            decision: row.try_get("decision")?,
            reason_code: row.try_get("reason_code")?,
            reason: row.try_get("reason")?,
            policy_sha256: row.try_get("policy_sha256")?,
            reviewer_receipt_id: row.try_get("reviewer_receipt_id")?,
            created_at: row.try_get("created_at")?,
        }))
    }

    /// Required reviewer roles frozen for this candidate. Empty for a legacy
    /// candidate with no participant-plan role requirement.
    pub async fn candidate_reviewer_roles(
        &self,
        candidate_id: &str,
    ) -> Result<Vec<crate::planning::ReviewerRole>, Error> {
        let mut connection = self.pool.acquire().await?;
        Ok(
            reviewer_requirement_for_candidate(&mut connection, candidate_id)
                .await?
                .roles,
        )
    }

    pub async fn quality_receipts(
        &self,
        candidate_id: &str,
    ) -> Result<Vec<QualityReceiptRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT receipt_id,candidate_id,role,principal_id,reviewer_role,action_spec_sha256,
                    commit_sha,tree_sha,result,evidence,findings_json,mechanism,model_provider,
                    model_name,model_reservation_id,model_request_sha256,input_tokens,output_tokens,
                    total_tokens,cost_micros,usage_known,received_at
             FROM quality_receipts WHERE candidate_id=?1 ORDER BY role",
        )
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(receipt_from_row).collect()
    }

    /// Re-evaluate the Gate from the existing immutable receipts and the
    /// current finding dispositions. This accepts no new evidence and appends
    /// a revision only when the deterministic outcome or reasons changed.
    pub async fn reevaluate_gate_for_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<Option<GateRecord>, Error> {
        if candidate_id.trim().is_empty() {
            return Err(Error::Invalid("candidate id"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let gate = reevaluate_gate_for_candidate_in_tx(&mut transaction, candidate_id).await?;
        transaction.commit().await?;
        Ok(gate)
    }
}

#[derive(Debug)]
struct CandidateContext {
    worker_id: String,
    generation: i64,
    expires_at_ms: i64,
    attempt_status: String,
    campaign_status: String,
    repository: String,
    base_commit: String,
    spec_sha256: String,
    spec_json: String,
    execution_mode: String,
    kind: String,
    integration_candidates: Vec<String>,
}

type CandidateContextRow = (
    Option<String>,
    Option<i64>,
    Option<i64>,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

async fn candidate_context(
    connection: &mut SqliteConnection,
    attempt_id: &str,
) -> Result<CandidateContext, Error> {
    let row: Option<CandidateContextRow> = sqlx::query_as::query_as(
        "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status,c.status,p.repository,
                COALESCE(aps.base_commit,p.base_commit),COALESCE(aps.spec_sha256,s.spec_sha256),
                COALESCE(aps.spec_json,s.spec_json),COALESCE(ca.execution_mode,'NONE'),
                COALESCE(aps.kind,'UNIT'),COALESCE(aps.integration_candidates_json,'[]')
         FROM attempts a
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         JOIN actions ac ON ac.id=c.action_id
         JOIN projects p ON p.id=ac.project_id
         JOIN action_specs s ON s.action_id=ac.id
         LEFT JOIN attempt_specs aps ON aps.attempt_id=a.id
         LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
         LEFT JOIN worker_leases l ON l.attempt_id=a.id
         WHERE a.id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(connection)
    .await?;
    let (
        worker_id,
        generation,
        expires_at_ms,
        attempt_status,
        campaign_status,
        repository,
        base_commit,
        spec_sha256,
        spec_json,
        execution_mode,
        kind,
        integration_candidates,
    ) = row.ok_or(Error::NotFound)?;
    Ok(CandidateContext {
        worker_id: worker_id.unwrap_or_default(),
        generation: generation.unwrap_or(0),
        expires_at_ms: expires_at_ms.unwrap_or(0),
        attempt_status,
        campaign_status,
        repository,
        base_commit,
        spec_sha256,
        spec_json,
        execution_mode,
        kind,
        integration_candidates: serde_json::from_str(&integration_candidates)
            .map_err(|_| Error::Invalid("stored integration candidates"))?,
    })
}

fn require_live_lease(
    context: &CandidateContext,
    worker_id: &str,
    generation: i64,
    now_ms: i64,
) -> Result<(), Error> {
    if context.campaign_status != "AUTHORIZED"
        || !matches!(
            context.execution_mode.as_str(),
            "FIXTURE" | "TRUSTED_LOCAL_PROVIDER"
        )
    {
        return Err(Error::Forbidden(
            "campaign lacks an active execution authorization".into(),
        ));
    }
    if context.attempt_status != "LEASED"
        || context.worker_id != worker_id
        || context.generation != generation
        || context.expires_at_ms <= now_ms
    {
        return Err(Error::Forbidden(
            "candidate submission requires the current unexpired lease generation".into(),
        ));
    }
    Ok(())
}

fn validate_candidate_input(input: &NewCandidate) -> Result<(), Error> {
    if input.attempt_id.trim().is_empty()
        || input.attempt_id.len() > 128
        || input.worker_id.trim().is_empty()
        || input.worker_id.len() > 128
        || input.generation <= 0
        || !is_sha256(&input.action_spec_sha256)
        || !is_git_oid(&input.commit_sha)
        || !is_git_oid(&input.tree_sha)
    {
        return Err(Error::Invalid("candidate submission"));
    }
    Ok(())
}

fn validate_receipt_input(role: QualityRole, input: &QualityReceiptInput) -> Result<(), Error> {
    if input.receipt_id.trim().is_empty()
        || input.receipt_id.len() > 128
        || input.principal_id.trim().is_empty()
        || input.principal_id.len() > 128
        || !is_sha256(&input.action_spec_sha256)
        || !is_git_oid(&input.commit_sha)
        || !is_git_oid(&input.tree_sha)
    {
        return Err(Error::Invalid("quality receipt"));
    }
    if role == QualityRole::Verifier && input.reviewer_role.is_some() {
        return Err(Error::Invalid(
            "a verifier receipt cannot claim a reviewer role",
        ));
    }
    crate::semantic_review::validate_findings(&input.findings)
        .map_err(|_| Error::Invalid("quality receipt findings"))?;
    if input.result == ReceiptResult::Pass
        && input.findings.iter().any(|finding| {
            matches!(
                finding.severity,
                crate::review::FindingSeverity::Blocking | crate::review::FindingSeverity::Critical
            )
        })
    {
        return Err(Error::Invalid(
            "a passing review cannot carry blocking or critical findings",
        ));
    }
    for value in [
        input.model_provider.as_str(),
        input.model_name.as_str(),
        input.model_reservation_id.as_str(),
    ] {
        if value.len() > 200 || value.chars().any(char::is_control) {
            return Err(Error::Invalid("quality receipt model provenance"));
        }
    }
    if input.mechanism == ReviewMechanism::Deterministic
        && (!input.model_reservation_id.is_empty()
            || input.input_tokens != 0
            || input.output_tokens != 0
            || input.total_tokens != 0
            || input.cost_micros != 0
            || input.usage_known)
    {
        return Err(Error::Invalid(
            "deterministic receipt cannot claim model usage",
        ));
    }
    if input.mechanism.is_model_backed()
        && input.result != ReceiptResult::Pass
        && input.findings.is_empty()
    {
        return Err(Error::Invalid(
            "a non-passing model review requires a concrete finding",
        ));
    }
    Ok(())
}

fn is_git_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn validate_receipt_binding(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    input: &QualityReceiptInput,
) -> Result<CandidateRecord, Error> {
    let candidate = load_candidate(connection, candidate_id).await?;
    if input.action_spec_sha256 != candidate.action_spec_sha256
        || !input.commit_sha.eq_ignore_ascii_case(&candidate.commit_sha)
        || !input.tree_sha.eq_ignore_ascii_case(&candidate.tree_sha)
    {
        return Err(Error::Forbidden(
            "receipt is not bound to the exact candidate and ActionSpec".into(),
        ));
    }
    if input.principal_id == candidate.implementer_id {
        return Err(Error::Forbidden(
            "implementer cannot provide independent quality evidence".into(),
        ));
    }
    Ok(candidate)
}

async fn replayed_receipt(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
    input: &QualityReceiptInput,
) -> Result<Option<QualityReceiptRecord>, Error> {
    let Some(existing) = load_receipt_by_id(connection, &input.receipt_id).await? else {
        return Ok(None);
    };
    if existing.candidate_id == candidate_id
        && existing.role == role
        && receipt_matches(&existing, input)
    {
        return Ok(Some(existing));
    }
    Err(Error::Conflict("receipt id reused with different evidence"))
}

async fn ensure_receipt_available(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
    input: &QualityReceiptInput,
) -> Result<(), Error> {
    let existing = load_receipts_by_role(connection, candidate_id, role).await?;
    match role {
        QualityRole::Verifier if !existing.is_empty() => {
            return Err(Error::Conflict("candidate already has a verifier receipt"));
        }
        QualityRole::Reviewer
            if existing
                .iter()
                .any(|receipt| receipt.principal_id == input.principal_id) =>
        {
            return Err(Error::Conflict(
                "candidate already has this reviewer's independent receipt",
            ));
        }
        QualityRole::Reviewer
            if input.reviewer_role.is_some()
                && existing
                    .iter()
                    .any(|receipt| receipt.reviewer_role == input.reviewer_role) =>
        {
            return Err(Error::Conflict(
                "candidate already has a receipt for this reviewer role",
            ));
        }
        _ => {}
    }
    let other_role = match role {
        QualityRole::Verifier => QualityRole::Reviewer,
        QualityRole::Reviewer => QualityRole::Verifier,
    };
    if let Some(other) = load_receipt_by_role(connection, candidate_id, other_role).await?
        && other.principal_id == input.principal_id
    {
        return Err(Error::Forbidden(
            "verifier and reviewer must be independent principals".into(),
        ));
    }
    Ok(())
}

/// Create the durable OPEN disposition for every reviewer finding. A finding
/// without a disposition can never be treated as closed.
async fn ensure_finding_dispositions(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    findings: &[crate::review::ReviewFinding],
    reviewer_principal: &str,
) -> Result<(), Error> {
    if findings.is_empty() {
        return Ok(());
    }
    let request: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT ir.id
         FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         JOIN intake_requests ir ON ir.campaign_id=r.campaign_id
         WHERE cd.id=?1
         LIMIT 1",
    )
    .bind(candidate_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((request_id,)) = request else {
        return Ok(());
    };
    let now = Utc::now().to_rfc3339();
    for finding in findings {
        let disposition = crate::planning::findings::open_disposition(
            finding,
            &request_id,
            candidate_id,
            reviewer_principal,
        );
        let evidence_refs_json = serde_json::to_string(&disposition.evidence_refs)
            .map_err(|_| Error::Invalid("finding evidence encoding"))?;
        sqlx::query::query(
            "INSERT OR IGNORE INTO finding_dispositions
             (id,request_id,candidate_id,finding_id,route,status,reviewer_principal,
              resolved_by,evidence_refs_json,owner_decision,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,'OPEN',?6,'',?7,NULL,?8,?8)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&disposition.request_id)
        .bind(candidate_id)
        .bind(&disposition.finding_id)
        .bind(crate::planning::findings::route_code(disposition.route))
        .bind(reviewer_principal)
        .bind(evidence_refs_json)
        .bind(&now)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

async fn insert_receipt(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
    input: &QualityReceiptInput,
    candidate: &CandidateRecord,
) -> Result<QualityReceiptRecord, Error> {
    let findings_json = serde_json::to_string(&input.findings)
        .map_err(|_| Error::Invalid("quality receipt findings"))?;
    let receipt = QualityReceiptRecord {
        receipt_id: input.receipt_id.clone(),
        candidate_id: candidate_id.to_owned(),
        role,
        principal_id: input.principal_id.clone(),
        reviewer_role: input.reviewer_role,
        action_spec_sha256: input.action_spec_sha256.clone(),
        commit_sha: candidate.commit_sha.clone(),
        tree_sha: candidate.tree_sha.clone(),
        result: input.result,
        evidence: input.evidence,
        findings: input.findings.clone(),
        mechanism: input.mechanism,
        model_provider: input.model_provider.clone(),
        model_name: input.model_name.clone(),
        model_reservation_id: input.model_reservation_id.clone(),
        model_request_sha256: input.model_request_sha256.clone(),
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        total_tokens: input.total_tokens,
        cost_micros: input.cost_micros,
        usage_known: input.usage_known,
        received_at: Utc::now().to_rfc3339(),
    };
    sqlx::query::query(
        "INSERT INTO quality_receipts
         (receipt_id,candidate_id,role,principal_id,reviewer_role,action_spec_sha256,commit_sha,
          tree_sha,result,evidence,findings_json,mechanism,model_provider,model_name,
          model_reservation_id,model_request_sha256,input_tokens,output_tokens,total_tokens,
          cost_micros,usage_known,received_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
    )
    .bind(&receipt.receipt_id)
    .bind(&receipt.candidate_id)
    .bind(role.as_str())
    .bind(&receipt.principal_id)
    .bind(
        receipt
            .reviewer_role
            .map(|role| role.as_str())
            .unwrap_or(""),
    )
    .bind(&receipt.action_spec_sha256)
    .bind(&receipt.commit_sha)
    .bind(&receipt.tree_sha)
    .bind(receipt_result_str(receipt.result))
    .bind(evidence_kind_str(receipt.evidence))
    .bind(findings_json)
    .bind(receipt.mechanism.as_str())
    .bind(&receipt.model_provider)
    .bind(&receipt.model_name)
    .bind(&receipt.model_reservation_id)
    .bind(&receipt.model_request_sha256)
    .bind(i64::try_from(receipt.input_tokens).map_err(|_| Error::Invalid("quality receipt usage"))?)
    .bind(
        i64::try_from(receipt.output_tokens)
            .map_err(|_| Error::Invalid("quality receipt usage"))?,
    )
    .bind(i64::try_from(receipt.total_tokens).map_err(|_| Error::Invalid("quality receipt usage"))?)
    .bind(i64::try_from(receipt.cost_micros).map_err(|_| Error::Invalid("quality receipt usage"))?)
    .bind(i64::from(receipt.usage_known))
    .bind(&receipt.received_at)
    .execute(&mut *connection)
    .await?;
    if role == QualityRole::Reviewer {
        ensure_finding_dispositions(
            connection,
            candidate_id,
            &receipt.findings,
            &receipt.principal_id,
        )
        .await?;
    }
    record_event(
        connection,
        candidate_id,
        "quality.receipt_recorded",
        &format!(
            "role={} receipt={} principal={} result={:?} evidence={:?}",
            role.as_str(),
            receipt.receipt_id,
            receipt.principal_id,
            receipt.result,
            receipt.evidence
        ),
    )
    .await?;
    Ok(receipt)
}

type ModelReviewReservationRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
);

async fn validate_model_review_receipt(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
    input: &QualityReceiptInput,
) -> Result<(), Error> {
    if role != QualityRole::Reviewer || !input.mechanism.is_model_backed() {
        return Ok(());
    }
    let selection = reviewer_selection_for_candidate(connection, candidate_id).await?;
    match (&input.mechanism, &selection) {
        (ReviewMechanism::CodexAppServer, ReviewerSelection::Native { provider, model }) => {
            // Trust boundary: a CODEX_APP_SERVER receipt is authored by the
            // dedicated reviewer role. Core binds the exact candidate,
            // mechanism, frozen provider/model, no-reservation rule, known
            // usage for approval and request-hash shape, but the App Server
            // turn is attested by that reviewer process. A compromised
            // reviewer token is equivalent to a compromised reviewer; see
            // ADR-0032. This is not a provider-issued attestation.
            if input.model_provider != *provider || input.model_name != *model {
                return Err(Error::Forbidden(
                    "native review provenance does not match the frozen reviewer route".into(),
                ));
            }
            if input.result == ReceiptResult::Pass
                && (!input.usage_known
                    || !input.model_reservation_id.is_empty()
                    || input.model_request_sha256.len() != 64
                    || !input
                        .model_request_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit()))
            {
                return Err(Error::Forbidden(
                    "native review approval requires known usage, no reservation and a request hash"
                        .into(),
                ));
            }
            return Ok(());
        }
        (ReviewMechanism::Model, ReviewerSelection::Chat { provider, model }) => {
            if input.model_provider != *provider || input.model_name != *model {
                return Err(Error::Forbidden(
                    "model review provenance does not match the frozen reviewer route".into(),
                ));
            }
        }
        _ => {
            return Err(Error::Forbidden(
                "reviewer mechanism does not match the frozen reviewer route".into(),
            ));
        }
    }
    if input.result != ReceiptResult::Pass {
        return Ok(());
    }
    if !input.usage_known
        || input.model_provider.trim().is_empty()
        || input.model_name.trim().is_empty()
        || input.model_reservation_id.trim().is_empty()
        || input.model_request_sha256.len() != 64
        || !input
            .model_request_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::Forbidden(
            "model review approval requires settled model provenance and known usage".into(),
        ));
    }
    let row: Option<ModelReviewReservationRow> = sqlx::query_as::query_as(
        "SELECT m.role,m.provider,m.model,m.request_sha256,m.input_tokens,m.output_tokens,
                    m.total_tokens,m.cost_micros,m.usage_known,m.state,m.scope_id,m.objective_id,
                    m.principal_id,ac.project_id
             FROM model_call_reservations m
             JOIN candidates cd ON cd.id=?2
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             WHERE m.reservation_id=?1",
    )
    .bind(&input.model_reservation_id)
    .bind(candidate_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((
        role,
        provider,
        model,
        request_sha256,
        input_tokens,
        output_tokens,
        total_tokens,
        cost_micros,
        usage_known,
        state,
        scope_id,
        objective_id,
        principal_id,
        _project_id,
    )) = row
    else {
        return Err(Error::Forbidden(
            "model review approval has no matching model-call reservation".into(),
        ));
    };
    if role != "SEMANTIC_REVIEWER"
        || state != "SETTLED"
        || usage_known != 1
        || scope_id != candidate_id
        || objective_id != candidate_id
        || principal_id != input.principal_id
        || provider != input.model_provider
        || model != input.model_name
        || request_sha256 != input.model_request_sha256.to_ascii_lowercase()
        || u64::try_from(input_tokens).ok() != Some(input.input_tokens)
        || u64::try_from(output_tokens).ok() != Some(input.output_tokens)
        || u64::try_from(total_tokens).ok() != Some(input.total_tokens)
        || u64::try_from(cost_micros).ok() != Some(input.cost_micros)
    {
        return Err(Error::Forbidden(
            "model review approval does not match a settled reservation for this candidate".into(),
        ));
    }
    Ok(())
}

async fn semantic_review_enabled(connection: &mut SqliteConnection) -> Result<bool, Error> {
    let enabled: Option<(i64,)> =
        sqlx::query_as::query_as("SELECT semantic_review_enabled FROM factory_settings WHERE id=1")
            .fetch_optional(&mut *connection)
            .await?;
    Ok(enabled.is_some_and(|(enabled,)| enabled != 0))
}

/// Resolve the frozen semantic-review route for a candidate. The Execution
/// Profile revision is authoritative; a global env flag cannot override it.
async fn reviewer_selection_for_candidate_in_tx(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<ReviewerSelection, Error> {
    if !semantic_review_enabled(connection).await? {
        return Ok(ReviewerSelection::Deterministic);
    }
    let profile: Option<(Option<String>,)> = sqlx::query_as::query_as(
        "SELECT epr.content_json
         FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         LEFT JOIN campaign_execution_profile_bindings b ON b.campaign_id=c.id
         LEFT JOIN lead_start_evidence e ON e.campaign_id=c.id
         LEFT JOIN execution_profile_revisions epr
           ON epr.revision_id = COALESCE(b.profile_revision_id, e.profile_revision_id)
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(&mut *connection)
    .await?;
    let (provider, model) = match profile.and_then(|(raw,)| raw) {
        Some(raw) => {
            let content: ExecutionProfileContent = serde_json::from_str(&raw)
                .map_err(|_| Error::Invalid("frozen execution profile content"))?;
            (
                content.roles.semantic_reviewer.provider.clone(),
                content.roles.semantic_reviewer.model.clone(),
            )
        }
        None => {
            // Legacy Campaign without a frozen profile: use the enabled
            // default connection only as an explicit, non-substituting route.
            let legacy: Option<(String, String, String)> = sqlx::query_as::query_as(
                "SELECT provider,kind,model FROM provider_settings
                 WHERE is_default=1 AND enabled=1 AND credential_ciphertext<>'' LIMIT 1",
            )
            .fetch_optional(&mut *connection)
            .await?;
            let Some((provider, _kind, model)) = legacy else {
                return Err(Error::Unavailable(
                    "semantic review is enabled but no reviewer route is configured".into(),
                ));
            };
            (provider, model)
        }
    };
    let settings: Option<(String, bool, String)> = sqlx::query_as::query_as(
        "SELECT kind,enabled,credential_ciphertext FROM provider_settings WHERE provider=?1",
    )
    .bind(&provider)
    .fetch_optional(&mut *connection)
    .await?;
    let native_review = std::env::var("FACTORY_ENABLE_CODEX_REVIEW").as_deref() == Ok("1");
    let Some((kind, enabled, credential)) = settings else {
        if provider == "codex" && native_review {
            // The dedicated reviewer container owns the ChatGPT login; Core
            // has no provider_settings credential for it. The reviewer adapter
            // verifies its own auth and returns fail-closed evidence if absent.
            return Ok(ReviewerSelection::Native { provider, model });
        }
        return Err(Error::Unavailable(format!(
            "reviewer provider {provider} is not configured"
        )));
    };
    if !enabled {
        return Err(Error::Unavailable(format!(
            "reviewer provider {provider} is disabled"
        )));
    }
    let adapter = crate::provider_adapter::ProviderAdapter::from_kind(&kind).ok_or_else(|| {
        Error::Unavailable(format!("reviewer provider kind {kind} is unsupported"))
    })?;
    match adapter.review_transport() {
        Some(crate::provider_adapter::ReviewTransport::ChatCompletions) => {
            if credential.is_empty() {
                return Err(Error::Unavailable(format!(
                    "reviewer provider {provider} has no stored credential"
                )));
            }
            Ok(ReviewerSelection::Chat { provider, model })
        }
        Some(crate::provider_adapter::ReviewTransport::NativeAdapter) => {
            Ok(ReviewerSelection::Native { provider, model })
        }
        None => Err(Error::Unavailable(format!(
            "reviewer provider {provider} has no review transport"
        ))),
    }
}

async fn reviewer_selection_for_candidate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<ReviewerSelection, Error> {
    reviewer_selection_for_candidate_in_tx(connection, candidate_id).await
}

#[derive(Debug, Clone, Default)]
struct ReviewerRequirement {
    count: usize,
    roles: Vec<crate::planning::ReviewerRole>,
    assignments: BTreeMap<crate::planning::ReviewerRole, String>,
}

type ReviewerRequirementRow = (i64, Option<String>, Option<String>, Option<String>);

impl ReviewerRequirement {
    /// A multi-role Gate may only count receipts when the frozen plan binds
    /// exactly the required role set to principals. An empty assignment set is
    /// valid only for a legacy plan with no required roles.
    fn assignments_match_roles(&self) -> bool {
        if self.roles.is_empty() {
            return self.assignments.is_empty();
        }
        self.assignments.len() == self.roles.len()
            && self
                .roles
                .iter()
                .all(|role| self.assignments.contains_key(role))
    }
}

fn unique_roles(
    roles: impl IntoIterator<Item = crate::planning::ReviewerRole>,
) -> Vec<crate::planning::ReviewerRole> {
    let mut seen = std::collections::BTreeSet::new();
    roles
        .into_iter()
        .filter(|role| seen.insert(*role))
        .collect()
}

async fn reviewer_requirement_for_candidate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<ReviewerRequirement, Error> {
    let row: Option<ReviewerRequirementRow> = sqlx::query_as::query_as(
        "SELECT COALESCE(
               (SELECT rr.reviewer_count FROM campaign_review_requirements rr
                WHERE rr.campaign_id=c.id),
               (SELECT pp.reviewer_count FROM intake_requests ir
                JOIN agentic_participant_plans pp ON pp.request_id=ir.id
                WHERE ir.campaign_id=c.id),
               1),
               (SELECT rr.required_roles_json FROM campaign_review_requirements rr
                WHERE rr.campaign_id=c.id),
               (SELECT rr.required_assignments_json FROM campaign_review_requirements rr
                WHERE rr.campaign_id=c.id),
               (SELECT pp.plan_json FROM intake_requests ir
                JOIN agentic_participant_plans pp ON pp.request_id=ir.id
                WHERE ir.campaign_id=c.id)
             FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(&mut *connection)
    .await?;
    let (stored_count, roles_json, assignments_json, plan_json) =
        row.unwrap_or((1, None, None, None));
    let mut requirement = ReviewerRequirement {
        count: usize::try_from(stored_count).unwrap_or(1).max(1),
        ..ReviewerRequirement::default()
    };
    if let Some(roles_json) = roles_json
        && let Ok(values) = serde_json::from_str::<Vec<String>>(&roles_json)
    {
        requirement.roles = unique_roles(
            values
                .iter()
                .filter_map(|value| crate::planning::ReviewerRole::parse(value)),
        );
    }
    if let Some(assignments_json) = assignments_json
        && let Ok(assignments) = serde_json::from_str::<
            Vec<crate::review_roles::ReviewerIdentityAssignment>,
        >(&assignments_json)
    {
        requirement.assignments = assignments
            .into_iter()
            .map(|assignment| (assignment.role, assignment.principal_id))
            .collect();
    }
    if let Some(plan_json) = plan_json
        && let Ok(plan) = serde_json::from_str::<crate::agentic::ParticipantPlan>(&plan_json)
    {
        if requirement.roles.is_empty() {
            requirement.roles = unique_roles(plan.reviewer_roles);
        }
        if requirement.assignments.is_empty() {
            requirement.assignments = plan
                .reviewer_assignments
                .into_iter()
                .map(|assignment| (assignment.role, assignment.principal_id))
                .collect();
        }
    }
    requirement.count = requirement.count.max(requirement.roles.len());
    Ok(requirement)
}

/// Informational findings on a passing, executed review are observations.
/// They remain durable in the receipt and disposition ledger, but they never
/// block acceptance; warning and higher severities keep their existing rules.
fn informational_observation(
    reviewer: &QualityReceiptRecord,
    finding: &crate::review::ReviewFinding,
) -> bool {
    reviewer.result == ReceiptResult::Pass
        && reviewer.evidence == EvidenceKind::Executed
        && finding.severity == crate::review::FindingSeverity::Info
}

/// Repair authorization consumes actionable findings only. The observation
/// exception is deliberately receipt-aware: Info remains actionable on a
/// non-PASS or non-EXECUTED receipt, exactly as before the observation rule.
fn is_actionable_finding(
    reviewer: &QualityReceiptRecord,
    finding: &crate::review::ReviewFinding,
) -> bool {
    !informational_observation(reviewer, finding)
}

fn actionable_findings(reviewer: &QualityReceiptRecord) -> Vec<crate::review::ReviewFinding> {
    reviewer
        .findings
        .iter()
        .filter(|finding| is_actionable_finding(reviewer, finding))
        .cloned()
        .collect()
}

struct EvaluatedGate {
    decision: crate::quality::GateDecision,
    verifier: QualityReceiptRecord,
    reviewers: Vec<QualityReceiptRecord>,
}

async fn evaluate_gate_decision(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    candidate: &CandidateRecord,
) -> Result<Option<EvaluatedGate>, Error> {
    let verifier = load_receipt_by_role(connection, candidate_id, QualityRole::Verifier).await?;
    let Some(verifier) = verifier.as_ref() else {
        return Ok(None);
    };
    let reviewer_records =
        load_receipts_by_role(connection, candidate_id, QualityRole::Reviewer).await?;
    let requirement = reviewer_requirement_for_candidate(connection, candidate_id).await?;
    let reviewer_failed = reviewer_records.iter().any(|receipt| {
        matches!(
            receipt.result,
            ReceiptResult::Fail | ReceiptResult::ChangesRequested
        )
    });
    let assignment_set_invalid = !requirement.assignments_match_roles();
    let mut principals = std::collections::BTreeSet::new();
    let mut covered_roles = std::collections::BTreeSet::new();
    let mut assignment_mismatch = false;
    let mut unrequired_role = false;
    let mut roleless_receipt = false;
    for receipt in &reviewer_records {
        // The same principal never counts twice, even if historical or
        // malformed rows exist.
        principals.insert(receipt.principal_id.as_str());
        let Some(role) = receipt.reviewer_role else {
            // A legacy receipt with no role cannot be silently included once
            // the frozen plan requires role coverage.
            if !requirement.roles.is_empty() {
                roleless_receipt = true;
            }
            continue;
        };
        if !requirement.roles.contains(&role) {
            // A historical or foreign receipt for a role outside the frozen
            // plan is not ignored: it makes the Gate fail closed.
            unrequired_role = true;
            continue;
        }
        if let Some(expected) = requirement.assignments.get(&role)
            && expected != &receipt.principal_id
        {
            assignment_mismatch = true;
        }
        covered_roles.insert(role);
    }
    let coverage_ok = !assignment_set_invalid
        && !assignment_mismatch
        && principals.len() >= requirement.count
        && covered_roles.len() >= requirement.roles.len();

    let policy = load_gate_policy(connection, candidate_id).await?;
    let reviewer_values = reviewer_records
        .iter()
        .map(to_reviewer_receipt)
        .collect::<Vec<_>>();
    let reviewer_refs = reviewer_values.iter().collect::<Vec<_>>();
    let mut decision = crate::quality::evaluate_quality_gate_with_reviewers(
        &FrozenActionSpec {
            hash: candidate.action_spec_sha256.clone(),
        },
        &crate::quality::CandidateRevision {
            commit_sha: candidate.commit_sha.clone(),
            tree_sha: candidate.tree_sha.clone(),
            action_spec_hash: candidate.action_spec_sha256.clone(),
        },
        &candidate.implementer_id,
        Some(&to_verifier_receipt(verifier)),
        &reviewer_refs,
        policy,
    );
    if !reviewer_records.is_empty() && assignment_set_invalid {
        decision = crate::quality::GateDecision {
            outcome: GateOutcome::Blocked,
            reasons: vec![
                "reviewer plan does not bind every required role to an authenticated principal"
                    .into(),
            ],
        };
    } else if !reviewer_records.is_empty() && unrequired_role {
        decision = crate::quality::GateDecision {
            outcome: GateOutcome::Blocked,
            reasons: vec!["reviewer receipt role is not required by the frozen plan".into()],
        };
    } else if !reviewer_records.is_empty() && roleless_receipt {
        decision = crate::quality::GateDecision {
            outcome: GateOutcome::Blocked,
            reasons: vec![
                "reviewer receipt has no role while the frozen plan requires role coverage".into(),
            ],
        };
    } else if !reviewer_records.is_empty() && assignment_mismatch {
        decision = crate::quality::GateDecision {
            outcome: GateOutcome::Blocked,
            reasons: vec!["reviewer principal does not match the frozen role assignment".into()],
        };
    }
    // A passing reviewer set waits until the deterministic plan's reviewer
    // count and required role coverage are satisfied, so an incoming
    // independent reviewer cannot be invalidated by an early Gate. A failing
    // verifier or reviewer, malformed evidence or an unknown receipt still
    // decides immediately.
    if !coverage_ok && !reviewer_failed {
        let only_missing_reviewer = decision.outcome == GateOutcome::NotTested
            && decision
                .reasons
                .iter()
                .any(|reason| reason == "independent reviewer receipt is missing");
        if decision.outcome == GateOutcome::Accepted || only_missing_reviewer {
            return Ok(None);
        }
    }
    for reviewer in &reviewer_records {
        // A critical finding is a hard human stop even if a malformed or
        // historical receipt also claimed PASS. A PASS with a blocking finding
        // is contradictory and can never accept the candidate.
        if reviewer
            .findings
            .iter()
            .any(|finding| finding.severity == crate::review::FindingSeverity::Critical)
            && matches!(
                decision.outcome,
                GateOutcome::Accepted | GateOutcome::RepairRequired
            )
        {
            decision = crate::quality::GateDecision {
                outcome: GateOutcome::HumanRequired,
                reasons: vec!["critical review finding requires human review".into()],
            };
        } else if reviewer.result == ReceiptResult::Pass
            && reviewer
                .findings
                .iter()
                .any(|finding| finding.severity == crate::review::FindingSeverity::Blocking)
            && decision.outcome == GateOutcome::Accepted
        {
            decision = crate::quality::GateDecision {
                outcome: GateOutcome::RepairRequired,
                reasons: vec!["reviewer approval contradicts a blocking finding".into()],
            };
        }
    }
    // Every reviewer finding is a durable disposition. A missing or OPEN
    // disposition blocks acceptance even when the reviewer receipt itself
    // passed; the finding must be repaired, resolved with evidence or refuted
    // by an authenticated owner. Informational findings on a passing, executed
    // review are observations: they remain visible in the receipt and
    // disposition ledger but do not block an otherwise accepted Gate.
    let mut open_findings = 0usize;
    let mut observations = 0usize;
    for reviewer in &reviewer_records {
        for finding in &reviewer.findings {
            if informational_observation(reviewer, finding) {
                observations += 1;
                continue;
            }
            let status: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT status FROM finding_dispositions
                 WHERE candidate_id=?1 AND finding_id=?2",
            )
            .bind(candidate_id)
            .bind(&finding.id)
            .fetch_optional(&mut *connection)
            .await?;
            let closed = matches!(
                status.as_ref().map(|(status,)| status.as_str()),
                Some("RESOLVED") | Some("REFUTED")
            );
            if !closed {
                open_findings += 1;
            }
        }
    }
    if open_findings > 0 && decision.outcome == GateOutcome::Accepted {
        decision = crate::quality::GateDecision {
            outcome: GateOutcome::RepairRequired,
            reasons: vec![format!("{open_findings} reviewer finding(s) remain open")],
        };
    }
    if observations > 0 && decision.outcome == GateOutcome::Accepted {
        decision.reasons.push(format!(
            "{observations} informational reviewer finding(s) recorded as observations"
        ));
    }
    match reviewer_selection_for_candidate_in_tx(connection, candidate_id).await {
        Ok(ReviewerSelection::Deterministic) => {
            if decision.outcome == GateOutcome::Accepted
                && reviewer_records
                    .iter()
                    .any(|receipt| receipt.mechanism.is_model_backed())
            {
                decision = crate::quality::GateDecision {
                    outcome: GateOutcome::NotTested,
                    reasons: vec![
                        "semantic review is disabled; deterministic reviewer evidence is required"
                            .into(),
                    ],
                };
            }
        }
        Ok(selection) => {
            if decision.outcome == GateOutcome::Accepted
                && reviewer_records
                    .iter()
                    .any(|receipt| !receipt.mechanism.is_model_backed())
            {
                let expected = match selection {
                    ReviewerSelection::Chat { .. } => "MODEL",
                    ReviewerSelection::Native { .. } => "CODEX_APP_SERVER",
                    ReviewerSelection::Deterministic => unreachable!(),
                };
                decision = crate::quality::GateDecision {
                    outcome: GateOutcome::NotTested,
                    reasons: vec![format!(
                        "reviewer mechanism does not match the frozen {expected} route"
                    )],
                };
            }
        }
        Err(Error::Unavailable(reason)) => {
            if decision.outcome == GateOutcome::Accepted {
                decision = crate::quality::GateDecision {
                    outcome: GateOutcome::NotTested,
                    reasons: vec![reason],
                };
            }
        }
        Err(error) => return Err(error),
    }
    if reviewer_records.is_empty()
        && decision
            .reasons
            .iter()
            .any(|reason| reason == "independent reviewer receipt is missing")
    {
        return Ok(None);
    }
    Ok(Some(EvaluatedGate {
        decision,
        verifier: verifier.clone(),
        reviewers: reviewer_records,
    }))
}

async fn decide_gate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    candidate: &CandidateRecord,
) -> Result<Option<GateRecord>, Error> {
    let Some(evaluated) = evaluate_gate_decision(connection, candidate_id, candidate).await? else {
        return Ok(None);
    };
    let gate = GateRecord {
        candidate_id: candidate_id.to_owned(),
        outcome: evaluated.decision.outcome,
        reasons: evaluated.decision.reasons,
        decided_at: Utc::now().to_rfc3339(),
    };
    let reasons_json = serde_json::to_string(&gate.reasons).expect("gate reasons are serializable");
    sqlx::query::query("INSERT INTO gate_decisions VALUES (?1,?2,?3,?4)")
        .bind(&gate.candidate_id)
        .bind(gate_outcome_str(gate.outcome))
        .bind(reasons_json)
        .bind(&gate.decided_at)
        .execute(&mut *connection)
        .await?;
    for reviewer in &evaluated.reviewers {
        sqlx::query::query(
            "INSERT OR IGNORE INTO gate_decision_reviewers (candidate_id,reviewer_receipt_id)
             VALUES (?1,?2)",
        )
        .bind(&gate.candidate_id)
        .bind(&reviewer.receipt_id)
        .execute(&mut *connection)
        .await?;
    }
    record_event(
        connection,
        candidate_id,
        "quality.gate_decided",
        &format!("outcome={:?}", gate.outcome),
    )
    .await?;
    project_gate_to_run(connection, candidate_id, &gate).await?;
    if let Some(reviewer) = evaluated.reviewers.iter().find(|reviewer| {
        reviewer
            .findings
            .iter()
            .any(|finding| is_actionable_finding(reviewer, finding))
    }) {
        record_gate_repair_authorization(
            connection,
            &gate,
            candidate,
            reviewer,
            &evaluated.verifier,
        )
        .await?;
    }
    Ok(Some(gate))
}

async fn reevaluate_gate_for_candidate_in_tx(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<Option<GateRecord>, Error> {
    let candidate = load_candidate(connection, candidate_id).await?;
    let Some(current) = load_gate(connection, candidate_id).await? else {
        return Ok(None);
    };
    let Some(evaluated) = evaluate_gate_decision(connection, candidate_id, &candidate).await?
    else {
        return Ok(Some(current));
    };
    if current.outcome == evaluated.decision.outcome
        && current.reasons == evaluated.decision.reasons
    {
        return Ok(Some(current));
    }
    let gate = GateRecord {
        candidate_id: candidate_id.to_owned(),
        outcome: evaluated.decision.outcome,
        reasons: evaluated.decision.reasons,
        decided_at: Utc::now().to_rfc3339(),
    };
    let reasons_json = serde_json::to_string(&gate.reasons).expect("gate reasons are serializable");
    sqlx::query::query(
        "INSERT INTO gate_decision_revisions (candidate_id,outcome,reasons_json,decided_at)
         VALUES (?1,?2,?3,?4)",
    )
    .bind(&gate.candidate_id)
    .bind(gate_outcome_str(gate.outcome))
    .bind(reasons_json)
    .bind(&gate.decided_at)
    .execute(&mut *connection)
    .await?;
    record_event(
        connection,
        candidate_id,
        "quality.gate_reevaluated",
        &format!("outcome={:?}", gate.outcome),
    )
    .await?;
    project_gate_to_run(connection, candidate_id, &gate).await?;
    Ok(Some(gate))
}

async fn project_gate_to_run(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    gate: &GateRecord,
) -> Result<(), Error> {
    let run: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT r.id FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(&mut *connection)
    .await?;
    let (run_id,) = run.ok_or(Error::NotFound)?;
    let run_status = run_status_for_gate(gate.outcome);
    let run_changed = sqlx::query::query(
        "UPDATE runs SET status=?1 WHERE id=?2 AND status IN ('QUEUED','RUNNING','REPAIR_REQUIRED')",
    )
    .bind(run_status)
    .bind(&run_id)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if run_changed == 1 {
        record_event(
            connection,
            &run_id,
            "run.status_changed",
            &format!(
                "status={run_status} outcome={}",
                gate_outcome_str(gate.outcome)
            ),
        )
        .await?;
    }
    Ok(())
}

struct RepairContext {
    campaign_id: String,
    work_item_id: String,
    risk_class: String,
    autonomy: AutonomyPolicy,
    prior_authorized_cycles: u32,
}

async fn repair_context(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<RepairContext, Error> {
    let row: Option<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT r.campaign_id,
                COALESCE(lt.unit_id, li.integration_id, a.id),
                COALESCE(ca.risk_class, '')
         FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         LEFT JOIN lead_tasks lt ON lt.attempt_id=a.id
         LEFT JOIN lead_integrations li ON li.attempt_id=a.id
         LEFT JOIN campaign_authorizations ca ON ca.campaign_id=r.campaign_id
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(&mut *connection)
    .await?;
    let (campaign_id, work_item_id, risk_class) = row.ok_or(Error::NotFound)?;
    let profile: Option<(Option<String>,)> = sqlx::query_as::query_as(
        "SELECT epr.content_json
         FROM lead_start_evidence e
         LEFT JOIN execution_profile_revisions epr
           ON epr.revision_id = e.profile_revision_id
         WHERE e.campaign_id = ?1",
    )
    .bind(&campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    let autonomy = profile
        .and_then(|(raw,)| raw)
        .and_then(|raw| {
            serde_json::from_str::<ExecutionProfileContent>(&raw)
                .ok()
                .map(|content| content.progress.autonomy)
        })
        .unwrap_or_default();
    let prior: i64 = sqlx::query_scalar::query_scalar(
        "SELECT COUNT(*) FROM autonomy_repairs
         WHERE campaign_id=?1 AND work_item_id=?2 AND decision='AUTHORIZED'",
    )
    .bind(&campaign_id)
    .bind(&work_item_id)
    .fetch_one(&mut *connection)
    .await?;
    Ok(RepairContext {
        campaign_id,
        work_item_id,
        risk_class,
        autonomy,
        prior_authorized_cycles: u32::try_from(prior).unwrap_or(u32::MAX),
    })
}

async fn record_gate_repair_authorization(
    connection: &mut SqliteConnection,
    gate: &GateRecord,
    candidate: &CandidateRecord,
    reviewer: &QualityReceiptRecord,
    verifier: &QualityReceiptRecord,
) -> Result<(), Error> {
    let findings = actionable_findings(reviewer);
    if !matches!(
        gate.outcome,
        GateOutcome::RepairRequired | GateOutcome::HumanRequired
    ) || findings.is_empty()
    {
        return Ok(());
    }
    let score = match autonomy::score_findings(&findings) {
        Ok(score) => score,
        Err(_) => return Ok(()),
    };
    let context = repair_context(connection, &gate.candidate_id).await?;
    let decision = autonomy::authorize_repair(&RepairRequest {
        risk_class: &context.risk_class,
        policy: &context.autonomy,
        findings: &findings,
        reviewer_result: reviewer.result,
        reviewer_evidence_executed: reviewer.evidence == EvidenceKind::Executed,
        reviewer_principal: &reviewer.principal_id,
        implementer_principal: &candidate.implementer_id,
        verifier_principal: &verifier.principal_id,
        prior_repair_cycles: context.prior_authorized_cycles,
    })
    .map_err(|_| Error::Invalid("autonomy repair authorization"))?;
    let (decision, reason_code, reason, cycle_index) = match decision {
        RepairDecision::Authorize { cycle, .. } => (
            "AUTHORIZED",
            "authorized".to_string(),
            "bounded repair authorized".to_string(),
            cycle,
        ),
        RepairDecision::Deny {
            reason_code,
            reason,
        } => (
            "DENIED",
            reason_code,
            reason,
            context.prior_authorized_cycles,
        ),
        RepairDecision::HumanRequired {
            reason_code,
            reason,
        } => (
            "HUMAN_REQUIRED",
            reason_code,
            reason,
            context.prior_authorized_cycles,
        ),
    };
    let policy_sha256 = sha256_hex(
        serde_json::to_vec(&context.autonomy)
            .map_err(|_| Error::Invalid("autonomy policy encoding"))?
            .as_slice(),
    );
    sqlx::query::query(
        "INSERT OR IGNORE INTO autonomy_repairs
         (repair_id,campaign_id,candidate_id,work_item_id,repair_trigger,risk_class,
          total_points,critical_count,blocking_count,warning_count,info_count,cycle_index,
          decision,reason_code,reason,policy_sha256,reviewer_receipt_id,created_at)
         VALUES (?1,?2,?3,?4,'QUALITY_GATE',?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&context.campaign_id)
    .bind(&gate.candidate_id)
    .bind(&context.work_item_id)
    .bind(&context.risk_class)
    .bind(score.total_points)
    .bind(score.critical_count)
    .bind(score.blocking_count)
    .bind(score.warning_count)
    .bind(score.info_count)
    .bind(cycle_index)
    .bind(decision)
    .bind(reason_code)
    .bind(reason)
    .bind(policy_sha256)
    .bind(&reviewer.receipt_id)
    .bind(&gate.decided_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

type CandidateColumns = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
);

fn candidate_from_columns(columns: CandidateColumns) -> CandidateRecord {
    let (
        id,
        attempt_id,
        action_spec_sha256,
        commit_sha,
        tree_sha,
        implementer_id,
        created_at,
        integration,
        lineage,
    ) = columns;
    CandidateRecord {
        id,
        attempt_id,
        action_spec_sha256,
        commit_sha,
        tree_sha,
        implementer_id,
        created_at,
        integration: integration == 1,
        lineage: serde_json::from_str(&lineage).unwrap_or_default(),
    }
}

async fn load_candidate(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<CandidateRecord, Error> {
    let row: Option<CandidateColumns> = sqlx::query_as::query_as(
        "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,
                integration,lineage_json
         FROM candidates WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?;
    row.map(candidate_from_columns).ok_or(Error::NotFound)
}

async fn load_candidate_for_attempt(
    connection: &mut SqliteConnection,
    attempt_id: &str,
) -> Result<Option<CandidateRecord>, Error> {
    let row: Option<CandidateColumns> = sqlx::query_as::query_as(
        "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,
                integration,lineage_json
         FROM candidates WHERE attempt_id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(connection)
    .await?;
    Ok(row.map(candidate_from_columns))
}

fn receipt_from_row(row: &SqliteRow) -> Result<QualityReceiptRecord, Error> {
    let role: String = row.try_get("role")?;
    let reviewer_role: String = row.try_get("reviewer_role")?;
    let result: String = row.try_get("result")?;
    let evidence: String = row.try_get("evidence")?;
    let findings: String = row.try_get("findings_json")?;
    let mechanism: String = row.try_get("mechanism")?;
    let input_tokens: i64 = row.try_get("input_tokens")?;
    let output_tokens: i64 = row.try_get("output_tokens")?;
    let total_tokens: i64 = row.try_get("total_tokens")?;
    let cost_micros: i64 = row.try_get("cost_micros")?;
    let usage_known: i64 = row.try_get("usage_known")?;
    Ok(QualityReceiptRecord {
        receipt_id: row.try_get("receipt_id")?,
        candidate_id: row.try_get("candidate_id")?,
        role: parse_role(&role)
            .map_err(|()| Error::Invalid("stored quality receipt role is invalid"))?,
        principal_id: row.try_get("principal_id")?,
        reviewer_role: if reviewer_role.is_empty() {
            None
        } else {
            Some(
                crate::planning::ReviewerRole::parse(&reviewer_role)
                    .ok_or(Error::Invalid("stored reviewer role is invalid"))?,
            )
        },
        action_spec_sha256: row.try_get("action_spec_sha256")?,
        commit_sha: row.try_get("commit_sha")?,
        tree_sha: row.try_get("tree_sha")?,
        result: parse_receipt_result(&result)
            .ok_or(Error::Invalid("stored quality receipt result is invalid"))?,
        evidence: parse_evidence_kind(&evidence)
            .ok_or(Error::Invalid("stored quality receipt evidence is invalid"))?,
        findings: serde_json::from_str(&findings)
            .map_err(|_| Error::Invalid("stored quality receipt findings are invalid"))?,
        mechanism: ReviewMechanism::parse(&mechanism).ok_or(Error::Invalid(
            "stored quality receipt mechanism is invalid",
        ))?,
        model_provider: row.try_get("model_provider")?,
        model_name: row.try_get("model_name")?,
        model_reservation_id: row.try_get("model_reservation_id")?,
        model_request_sha256: row.try_get("model_request_sha256")?,
        input_tokens: u64::try_from(input_tokens)
            .map_err(|_| Error::Invalid("stored quality receipt usage is invalid"))?,
        output_tokens: u64::try_from(output_tokens)
            .map_err(|_| Error::Invalid("stored quality receipt usage is invalid"))?,
        total_tokens: u64::try_from(total_tokens)
            .map_err(|_| Error::Invalid("stored quality receipt usage is invalid"))?,
        cost_micros: u64::try_from(cost_micros)
            .map_err(|_| Error::Invalid("stored quality receipt usage is invalid"))?,
        usage_known: usage_known != 0,
        received_at: row.try_get("received_at")?,
    })
}

async fn load_receipt_by_id(
    connection: &mut SqliteConnection,
    receipt_id: &str,
) -> Result<Option<QualityReceiptRecord>, Error> {
    let row = sqlx::query::query(
        "SELECT receipt_id,candidate_id,role,principal_id,reviewer_role,action_spec_sha256,
                commit_sha,tree_sha,result,evidence,findings_json,mechanism,model_provider,
                model_name,model_reservation_id,model_request_sha256,input_tokens,output_tokens,
                total_tokens,cost_micros,usage_known,received_at
         FROM quality_receipts WHERE receipt_id=?1",
    )
    .bind(receipt_id)
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(receipt_from_row).transpose()
}

async fn load_receipts_by_role(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
) -> Result<Vec<QualityReceiptRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT receipt_id,candidate_id,role,principal_id,reviewer_role,action_spec_sha256,
                commit_sha,tree_sha,result,evidence,findings_json,mechanism,model_provider,
                model_name,model_reservation_id,model_request_sha256,input_tokens,output_tokens,
                total_tokens,cost_micros,usage_known,received_at
         FROM quality_receipts WHERE candidate_id=?1 AND role=?2
         ORDER BY received_at ASC,receipt_id ASC",
    )
    .bind(candidate_id)
    .bind(role.as_str())
    .fetch_all(connection)
    .await?;
    rows.iter().map(receipt_from_row).collect()
}

async fn load_receipt_by_role(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
) -> Result<Option<QualityReceiptRecord>, Error> {
    Ok(load_receipts_by_role(connection, candidate_id, role)
        .await?
        .into_iter()
        .next())
}

fn receipt_matches(existing: &QualityReceiptRecord, input: &QualityReceiptInput) -> bool {
    existing.principal_id == input.principal_id
        && existing.reviewer_role == input.reviewer_role
        && existing.action_spec_sha256 == input.action_spec_sha256
        && existing.commit_sha.eq_ignore_ascii_case(&input.commit_sha)
        && existing.tree_sha.eq_ignore_ascii_case(&input.tree_sha)
        && existing.result == input.result
        && existing.evidence == input.evidence
        && existing.findings == input.findings
        && existing.mechanism == input.mechanism
        && existing.model_provider == input.model_provider
        && existing.model_name == input.model_name
        && existing.model_reservation_id == input.model_reservation_id
        && existing.model_request_sha256 == input.model_request_sha256
        && existing.input_tokens == input.input_tokens
        && existing.output_tokens == input.output_tokens
        && existing.total_tokens == input.total_tokens
        && existing.cost_micros == input.cost_micros
        && existing.usage_known == input.usage_known
}

async fn load_gate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<Option<GateRecord>, Error> {
    let row: Option<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT outcome,reasons_json,decided_at FROM gate_decisions_current WHERE candidate_id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(connection)
    .await?;
    row.map(|(outcome, reasons, decided_at)| {
        Ok(GateRecord {
            candidate_id: candidate_id.to_owned(),
            outcome: parse_gate_outcome(&outcome)
                .ok_or(Error::Invalid("stored gate outcome is invalid"))?,
            reasons: serde_json::from_str(&reasons)
                .map_err(|_| Error::Invalid("stored gate reasons are invalid"))?,
            decided_at,
        })
    })
    .transpose()
}

async fn load_gate_policy(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<GatePolicy, Error> {
    let row: Option<(String, Option<String>, Option<String>)> = sqlx::query_as::query_as(
        "SELECT c.status,ca.risk_class,ca.execution_mode FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(connection)
    .await?;
    let (campaign_status, risk, execution_mode) = row.ok_or(Error::NotFound)?;
    let authorization = match (campaign_status.as_str(), execution_mode.as_deref()) {
        ("AUTHORIZED", Some("FIXTURE" | "TRUSTED_LOCAL_PROVIDER")) => {
            AuthorizationStatus::Authorized
        }
        ("BLOCKED_AUTHORIZATION", _) => AuthorizationStatus::HumanApprovalRequired,
        _ => AuthorizationStatus::Denied,
    };
    let risk = match risk.as_deref() {
        Some("R0") => RiskStatus::WithinPolicy,
        Some("R1") => RiskStatus::HumanApprovalRequired,
        Some("R2" | "R3") => RiskStatus::ExceedsPolicy,
        _ => RiskStatus::Unknown,
    };
    Ok(GatePolicy {
        authorization,
        risk,
    })
}

fn to_verifier_receipt(record: &QualityReceiptRecord) -> VerifierReceipt {
    VerifierReceipt {
        receipt_id: record.receipt_id.clone(),
        principal_id: record.principal_id.clone(),
        action_spec_hash: record.action_spec_sha256.clone(),
        candidate_commit_sha: record.commit_sha.clone(),
        candidate_tree_sha: record.tree_sha.clone(),
        result: record.result,
        evidence: record.evidence,
    }
}

fn to_reviewer_receipt(record: &QualityReceiptRecord) -> ReviewerReceipt {
    ReviewerReceipt {
        receipt_id: record.receipt_id.clone(),
        principal_id: record.principal_id.clone(),
        action_spec_hash: record.action_spec_sha256.clone(),
        candidate_commit_sha: record.commit_sha.clone(),
        candidate_tree_sha: record.tree_sha.clone(),
        result: record.result,
        evidence: record.evidence,
    }
}

fn parse_role(value: &str) -> Result<QualityRole, ()> {
    match value {
        "verifier" => Ok(QualityRole::Verifier),
        "reviewer" => Ok(QualityRole::Reviewer),
        _ => Err(()),
    }
}

fn receipt_result_str(value: ReceiptResult) -> &'static str {
    match value {
        ReceiptResult::Pass => "PASS",
        ReceiptResult::Fail => "FAIL",
        ReceiptResult::ChangesRequested => "CHANGES_REQUESTED",
        ReceiptResult::NotTested => "NOT_TESTED",
    }
}

fn parse_receipt_result(value: &str) -> Option<ReceiptResult> {
    match value {
        "PASS" => Some(ReceiptResult::Pass),
        "FAIL" => Some(ReceiptResult::Fail),
        "CHANGES_REQUESTED" => Some(ReceiptResult::ChangesRequested),
        "NOT_TESTED" => Some(ReceiptResult::NotTested),
        _ => None,
    }
}

fn evidence_kind_str(value: EvidenceKind) -> &'static str {
    match value {
        EvidenceKind::Executed => "EXECUTED",
        EvidenceKind::Mocked => "MOCKED",
        EvidenceKind::Unknown => "UNKNOWN",
    }
}

fn parse_evidence_kind(value: &str) -> Option<EvidenceKind> {
    match value {
        "EXECUTED" => Some(EvidenceKind::Executed),
        "MOCKED" => Some(EvidenceKind::Mocked),
        "UNKNOWN" => Some(EvidenceKind::Unknown),
        _ => None,
    }
}

fn run_status_for_gate(value: GateOutcome) -> &'static str {
    match value {
        GateOutcome::Accepted => "ACCEPTED",
        GateOutcome::RepairRequired => "REPAIR_REQUIRED",
        GateOutcome::Blocked | GateOutcome::HumanRequired | GateOutcome::NotTested => "REJECTED",
    }
}

fn gate_outcome_str(value: GateOutcome) -> &'static str {
    match value {
        GateOutcome::Accepted => "ACCEPTED",
        GateOutcome::RepairRequired => "REPAIR_REQUIRED",
        GateOutcome::Blocked => "BLOCKED",
        GateOutcome::HumanRequired => "HUMAN_REQUIRED",
        GateOutcome::NotTested => "NOT_TESTED",
    }
}

fn parse_gate_outcome(value: &str) -> Option<GateOutcome> {
    match value {
        "ACCEPTED" => Some(GateOutcome::Accepted),
        "REPAIR_REQUIRED" => Some(GateOutcome::RepairRequired),
        "BLOCKED" => Some(GateOutcome::Blocked),
        "HUMAN_REQUIRED" => Some(GateOutcome::HumanRequired),
        "NOT_TESTED" => Some(GateOutcome::NotTested),
        _ => None,
    }
}
