use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};
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
        validate_receipt_input(input)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let candidate = validate_receipt_binding(&mut transaction, candidate_id, input).await?;
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
        let reviewer_gate = if role == "reviewer" {
            "AND EXISTS (SELECT 1 FROM quality_receipts vr
                         WHERE vr.candidate_id=c.id AND vr.role='verifier' AND vr.result='PASS')"
        } else {
            ""
        };
        let query = format!(
            "SELECT c.id FROM candidates c
             WHERE NOT EXISTS (SELECT 1 FROM quality_receipts r
                               WHERE r.candidate_id=c.id AND r.role=?1)
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

    pub async fn candidate_inspection(&self, id: &str) -> Result<CandidateInspection, Error> {
        let candidate = self.candidate(id).await?;
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

    pub async fn candidate(&self, id: &str) -> Result<CandidateRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_candidate(&mut connection, id).await
    }

    pub async fn gate_for_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<Option<GateRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_gate(&mut connection, candidate_id).await
    }

    pub async fn quality_receipts(
        &self,
        candidate_id: &str,
    ) -> Result<Vec<QualityReceiptRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
                    result,evidence,findings_json,mechanism,model_provider,model_name,
                    model_reservation_id,model_request_sha256,input_tokens,output_tokens,
                    total_tokens,cost_micros,usage_known,received_at
             FROM quality_receipts WHERE candidate_id=?1 ORDER BY role",
        )
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(receipt_from_row).collect()
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

fn validate_receipt_input(input: &QualityReceiptInput) -> Result<(), Error> {
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
    crate::semantic_review::validate_findings(&input.findings)
        .map_err(|_| Error::Invalid("quality receipt findings"))?;
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
    if input.mechanism == ReviewMechanism::Model
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
    if load_receipt_by_role(connection, candidate_id, role)
        .await?
        .is_some()
    {
        return Err(Error::Conflict(
            "candidate already has a receipt for this independent role",
        ));
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
         (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
          result,evidence,findings_json,mechanism,model_provider,model_name,
          model_reservation_id,model_request_sha256,input_tokens,output_tokens,total_tokens,
          cost_micros,usage_known,received_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
    )
    .bind(&receipt.receipt_id)
    .bind(&receipt.candidate_id)
    .bind(role.as_str())
    .bind(&receipt.principal_id)
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
    if role != QualityRole::Reviewer
        || input.mechanism != ReviewMechanism::Model
        || input.result != ReceiptResult::Pass
    {
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

async fn decide_gate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    candidate: &CandidateRecord,
) -> Result<Option<GateRecord>, Error> {
    let verifier = load_receipt_by_role(connection, candidate_id, QualityRole::Verifier).await?;
    let Some(verifier) = verifier.as_ref() else {
        return Ok(None);
    };
    let reviewer = load_receipt_by_role(connection, candidate_id, QualityRole::Reviewer).await?;
    let policy = load_gate_policy(connection, candidate_id).await?;
    let decision = evaluate_quality_gate(
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
        reviewer.as_ref().map(to_reviewer_receipt).as_ref(),
        policy,
    );
    if reviewer.is_none()
        && decision
            .reasons
            .iter()
            .any(|reason| reason == "independent reviewer receipt is missing")
    {
        return Ok(None);
    }
    let gate = GateRecord {
        candidate_id: candidate_id.to_owned(),
        outcome: decision.outcome,
        reasons: decision.reasons,
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
    record_event(
        connection,
        candidate_id,
        "quality.gate_decided",
        &format!("outcome={:?}", gate.outcome),
    )
    .await?;
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
    Ok(Some(gate))
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
        "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
                result,evidence,findings_json,mechanism,model_provider,model_name,
                model_reservation_id,model_request_sha256,input_tokens,output_tokens,
                total_tokens,cost_micros,usage_known,received_at
         FROM quality_receipts WHERE receipt_id=?1",
    )
    .bind(receipt_id)
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(receipt_from_row).transpose()
}

async fn load_receipt_by_role(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
) -> Result<Option<QualityReceiptRecord>, Error> {
    let row = sqlx::query::query(
        "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
                result,evidence,findings_json,mechanism,model_provider,model_name,
                model_reservation_id,model_request_sha256,input_tokens,output_tokens,
                total_tokens,cost_micros,usage_known,received_at
         FROM quality_receipts WHERE candidate_id=?1 AND role=?2",
    )
    .bind(candidate_id)
    .bind(role.as_str())
    .fetch_optional(connection)
    .await?;
    row.as_ref().map(receipt_from_row).transpose()
}

fn receipt_matches(existing: &QualityReceiptRecord, input: &QualityReceiptInput) -> bool {
    existing.principal_id == input.principal_id
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
        "SELECT outcome,reasons_json,decided_at FROM gate_decisions WHERE candidate_id=?1",
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
