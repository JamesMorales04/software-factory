use super::*;

use chrono::Utc;
use sqlx::SqliteConnection;
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

        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = candidate_context(&mut transaction, &input.attempt_id).await?;
        require_live_lease(&current, &input.worker_id, input.generation, now_ms)?;
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
            if existing.action_spec_sha256 == input.action_spec_sha256
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
        let candidate = CandidateRecord {
            id: Uuid::new_v4().to_string(),
            attempt_id: input.attempt_id.clone(),
            action_spec_sha256: input.action_spec_sha256.clone(),
            commit_sha: verified.commit_sha.clone(),
            tree_sha: verified.tree_sha.clone(),
            implementer_id: input.worker_id.clone(),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query("INSERT INTO candidates VALUES (?1,?2,?3,?4,?5,?6,?7)")
            .bind(&candidate.id)
            .bind(&candidate.attempt_id)
            .bind(&candidate.action_spec_sha256)
            .bind(&candidate.commit_sha)
            .bind(&candidate.tree_sha)
            .bind(&candidate.implementer_id)
            .bind(&candidate.created_at)
            .execute(&mut *transaction)
            .await?;
        let changed = sqlx::query(
            "UPDATE attempts SET status='CANDIDATE_READY' WHERE id=?1 AND status='LEASED'",
        )
        .bind(&candidate.attempt_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("attempt is no longer leased"));
        }
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
        let receipt =
            insert_receipt(&mut transaction, candidate_id, role, input, &candidate).await?;
        let gate = decide_gate(&mut transaction, candidate_id, &candidate).await?;
        transaction.commit().await?;
        Ok(QualityIngestResult { receipt, gate })
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
        let rows: Vec<ReceiptColumns> = sqlx::query_as(
            "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,result,evidence,received_at
             FROM quality_receipts WHERE candidate_id=?1 ORDER BY role",
        )
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(receipt_from_columns).collect()
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
);

async fn candidate_context(
    connection: &mut SqliteConnection,
    attempt_id: &str,
) -> Result<CandidateContext, Error> {
    let row: Option<CandidateContextRow> = sqlx::query_as(
        "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status,c.status,p.repository,p.base_commit,s.spec_sha256
         FROM attempts a
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         JOIN actions ac ON ac.id=c.action_id
         JOIN projects p ON p.id=ac.project_id
         JOIN action_specs s ON s.action_id=ac.id
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
    })
}

fn require_live_lease(
    context: &CandidateContext,
    worker_id: &str,
    generation: i64,
    now_ms: i64,
) -> Result<(), Error> {
    if context.campaign_status != "AUTHORIZED" {
        return Err(Error::Forbidden(
            "campaign lacks explicit execution authorization".into(),
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
        received_at: Utc::now().to_rfc3339(),
    };
    sqlx::query("INSERT INTO quality_receipts VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")
        .bind(&receipt.receipt_id)
        .bind(&receipt.candidate_id)
        .bind(role.as_str())
        .bind(&receipt.principal_id)
        .bind(&receipt.action_spec_sha256)
        .bind(&receipt.commit_sha)
        .bind(&receipt.tree_sha)
        .bind(receipt_result_str(receipt.result))
        .bind(evidence_kind_str(receipt.evidence))
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

async fn decide_gate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    candidate: &CandidateRecord,
) -> Result<Option<GateRecord>, Error> {
    let verifier = load_receipt_by_role(connection, candidate_id, QualityRole::Verifier).await?;
    let reviewer = load_receipt_by_role(connection, candidate_id, QualityRole::Reviewer).await?;
    let (Some(verifier), Some(reviewer)) = (&verifier, &reviewer) else {
        return Ok(None);
    };
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
        Some(&to_reviewer_receipt(reviewer)),
        policy,
    );
    let gate = GateRecord {
        candidate_id: candidate_id.to_owned(),
        outcome: decision.outcome,
        reasons: decision.reasons,
        decided_at: Utc::now().to_rfc3339(),
    };
    let reasons_json = serde_json::to_string(&gate.reasons).expect("gate reasons are serializable");
    sqlx::query("INSERT INTO gate_decisions VALUES (?1,?2,?3,?4)")
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
    Ok(Some(gate))
}

type CandidateColumns = (String, String, String, String, String, String, String);

fn candidate_from_columns(columns: CandidateColumns) -> CandidateRecord {
    let (id, attempt_id, action_spec_sha256, commit_sha, tree_sha, implementer_id, created_at) =
        columns;
    CandidateRecord {
        id,
        attempt_id,
        action_spec_sha256,
        commit_sha,
        tree_sha,
        implementer_id,
        created_at,
    }
}

async fn load_candidate(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<CandidateRecord, Error> {
    let row: Option<CandidateColumns> = sqlx::query_as(
        "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at
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
    let row: Option<CandidateColumns> = sqlx::query_as(
        "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at
         FROM candidates WHERE attempt_id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(connection)
    .await?;
    Ok(row.map(candidate_from_columns))
}

type ReceiptColumns = (
    String,
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

fn receipt_from_columns(columns: ReceiptColumns) -> Result<QualityReceiptRecord, Error> {
    let (
        receipt_id,
        candidate_id,
        role,
        principal_id,
        action_spec_sha256,
        commit_sha,
        tree_sha,
        result,
        evidence,
        received_at,
    ) = columns;
    Ok(QualityReceiptRecord {
        receipt_id,
        candidate_id,
        role: parse_role(&role)
            .map_err(|()| Error::Invalid("stored quality receipt role is invalid"))?,
        principal_id,
        action_spec_sha256,
        commit_sha,
        tree_sha,
        result: parse_receipt_result(&result)
            .ok_or(Error::Invalid("stored quality receipt result is invalid"))?,
        evidence: parse_evidence_kind(&evidence)
            .ok_or(Error::Invalid("stored quality receipt evidence is invalid"))?,
        received_at,
    })
}

async fn load_receipt_by_id(
    connection: &mut SqliteConnection,
    receipt_id: &str,
) -> Result<Option<QualityReceiptRecord>, Error> {
    let row: Option<ReceiptColumns> = sqlx::query_as(
        "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,result,evidence,received_at
         FROM quality_receipts WHERE receipt_id=?1",
    )
    .bind(receipt_id)
    .fetch_optional(connection)
    .await?;
    row.map(receipt_from_columns).transpose()
}

async fn load_receipt_by_role(
    connection: &mut SqliteConnection,
    candidate_id: &str,
    role: QualityRole,
) -> Result<Option<QualityReceiptRecord>, Error> {
    let row: Option<ReceiptColumns> = sqlx::query_as(
        "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,result,evidence,received_at
         FROM quality_receipts WHERE candidate_id=?1 AND role=?2",
    )
    .bind(candidate_id)
    .bind(role.as_str())
    .fetch_optional(connection)
    .await?;
    row.map(receipt_from_columns).transpose()
}

fn receipt_matches(existing: &QualityReceiptRecord, input: &QualityReceiptInput) -> bool {
    existing.principal_id == input.principal_id
        && existing.action_spec_sha256 == input.action_spec_sha256
        && existing.commit_sha.eq_ignore_ascii_case(&input.commit_sha)
        && existing.tree_sha.eq_ignore_ascii_case(&input.tree_sha)
        && existing.result == input.result
        && existing.evidence == input.evidence
}

async fn load_gate(
    connection: &mut SqliteConnection,
    candidate_id: &str,
) -> Result<Option<GateRecord>, Error> {
    let row: Option<(String, String, String)> = sqlx::query_as(
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
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT c.status,ac.risk FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         JOIN actions ac ON ac.id=c.action_id
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_optional(connection)
    .await?;
    let (campaign_status, risk) = row.ok_or(Error::NotFound)?;
    let authorization = match campaign_status.as_str() {
        "AUTHORIZED" => AuthorizationStatus::Authorized,
        "BLOCKED_AUTHORIZATION" => AuthorizationStatus::HumanApprovalRequired,
        _ => AuthorizationStatus::Unknown,
    };
    let risk = match risk.as_str() {
        "R0" => RiskStatus::WithinPolicy,
        "R1" => RiskStatus::HumanApprovalRequired,
        "R2" | "R3" => RiskStatus::ExceedsPolicy,
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
