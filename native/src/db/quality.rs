use super::*;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use uuid::Uuid;

impl Database {
    pub fn record_candidate(
        &self,
        input: &NewCandidate,
        now_ms: i64,
    ) -> Result<CandidateRecord, Error> {
        validate_candidate_input(input)?;
        let context = self.candidate_context(&input.attempt_id)?;
        require_live_lease(&context, &input.worker_id, input.generation, now_ms)?;
        if context.spec_sha256 != input.action_spec_sha256 {
            return Err(Error::Forbidden(
                "candidate ActionSpec hash does not match the frozen spec".into(),
            ));
        }
        let verified = verify_candidate_commit(
            Path::new(&context.repository),
            &context.base_commit,
            &input.commit_sha,
            &input.tree_sha,
        )
        .map_err(|error| Error::Forbidden(format!("candidate lineage rejected: {error}")))?;

        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = candidate_context(&tx, &input.attempt_id)?;
        require_live_lease(&current, &input.worker_id, input.generation, now_ms)?;
        if current.spec_sha256 != input.action_spec_sha256
            || current.repository != context.repository
            || current.base_commit != context.base_commit
        {
            return Err(Error::Conflict(
                "attempt binding changed during candidate validation",
            ));
        }
        if let Some(existing) = load_candidate_for_attempt(&tx, &input.attempt_id)? {
            if existing.action_spec_sha256 == input.action_spec_sha256
                && existing
                    .commit_sha
                    .eq_ignore_ascii_case(&verified.commit_sha)
                && existing.tree_sha.eq_ignore_ascii_case(&verified.tree_sha)
            {
                tx.commit()?;
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
        tx.execute(
            "INSERT INTO candidates VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                candidate.id,
                candidate.attempt_id,
                candidate.action_spec_sha256,
                candidate.commit_sha,
                candidate.tree_sha,
                candidate.implementer_id,
                candidate.created_at
            ],
        )?;
        let changed = tx.execute(
            "UPDATE attempts SET status='CANDIDATE_READY' WHERE id=?1 AND status='LEASED'",
            [&candidate.attempt_id],
        )?;
        if changed != 1 {
            return Err(Error::Conflict("attempt is no longer leased"));
        }
        record_event(
            &tx,
            &candidate.id,
            "candidate.recorded",
            &format!(
                "attempt={} commit={} tree={}",
                candidate.attempt_id, candidate.commit_sha, candidate.tree_sha
            ),
        )?;
        tx.commit()?;
        Ok(candidate)
    }

    pub fn record_quality_receipt(
        &self,
        candidate_id: &str,
        role: QualityRole,
        input: &QualityReceiptInput,
    ) -> Result<QualityIngestResult, Error> {
        validate_receipt_input(input)?;
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let candidate = load_candidate(&tx, candidate_id)?;
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
        if let Some(existing) = load_receipt_by_id(&tx, &input.receipt_id)? {
            if existing.candidate_id == candidate_id
                && existing.role == role
                && receipt_matches(&existing, input)
            {
                let gate = load_gate(&tx, candidate_id)?;
                tx.commit()?;
                return Ok(QualityIngestResult {
                    receipt: existing,
                    gate,
                });
            }
            return Err(Error::Conflict("receipt id reused with different evidence"));
        }
        if load_receipt_by_role(&tx, candidate_id, role)?.is_some() {
            return Err(Error::Conflict(
                "candidate already has a receipt for this independent role",
            ));
        }
        let other_role = match role {
            QualityRole::Verifier => QualityRole::Reviewer,
            QualityRole::Reviewer => QualityRole::Verifier,
        };
        if let Some(other) = load_receipt_by_role(&tx, candidate_id, other_role)?
            && other.principal_id == input.principal_id
        {
            return Err(Error::Forbidden(
                "verifier and reviewer must be independent principals".into(),
            ));
        }
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
        tx.execute(
            "INSERT INTO quality_receipts VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                receipt.receipt_id,
                receipt.candidate_id,
                role.as_str(),
                receipt.principal_id,
                receipt.action_spec_sha256,
                receipt.commit_sha,
                receipt.tree_sha,
                receipt_result_str(receipt.result),
                evidence_kind_str(receipt.evidence),
                receipt.received_at
            ],
        )?;
        record_event(
            &tx,
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
        )?;

        let verifier = load_receipt_by_role(&tx, candidate_id, QualityRole::Verifier)?;
        let reviewer = load_receipt_by_role(&tx, candidate_id, QualityRole::Reviewer)?;
        let gate = if let (Some(verifier), Some(reviewer)) = (&verifier, &reviewer) {
            let policy = load_gate_policy(&tx, candidate_id)?;
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
            let reasons_json =
                serde_json::to_string(&gate.reasons).expect("gate reasons are serializable");
            tx.execute(
                "INSERT INTO gate_decisions VALUES (?1,?2,?3,?4)",
                params![
                    gate.candidate_id,
                    gate_outcome_str(gate.outcome),
                    reasons_json,
                    gate.decided_at
                ],
            )?;
            record_event(
                &tx,
                candidate_id,
                "quality.gate_decided",
                &format!("outcome={:?}", gate.outcome),
            )?;
            Some(gate)
        } else {
            None
        };
        tx.commit()?;
        Ok(QualityIngestResult { receipt, gate })
    }

    pub fn candidate(&self, id: &str) -> Result<CandidateRecord, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        load_candidate(&connection, id)
    }

    pub fn gate_for_candidate(&self, candidate_id: &str) -> Result<Option<GateRecord>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        load_gate(&connection, candidate_id)
    }

    pub fn quality_receipts(&self, candidate_id: &str) -> Result<Vec<QualityReceiptRecord>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,result,evidence,received_at
             FROM quality_receipts WHERE candidate_id=?1 ORDER BY role",
        )?;
        let rows = statement.query_map([candidate_id], receipt_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Error::from)
    }

    fn candidate_context(&self, attempt_id: &str) -> Result<CandidateContext, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        candidate_context(&connection, attempt_id)
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

fn candidate_context(connection: &Connection, attempt_id: &str) -> Result<CandidateContext, Error> {
    let context = connection
        .query_row(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status,c.status,p.repository,p.base_commit,s.spec_sha256
             FROM attempts a
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN projects p ON p.id=ac.project_id
             JOIN action_specs s ON s.action_id=ac.id
             LEFT JOIN worker_leases l ON l.attempt_id=a.id
             WHERE a.id=?1",
            [attempt_id],
            |row| {
                Ok(CandidateContext {
                    worker_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    generation: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    expires_at_ms: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    attempt_status: row.get(3)?,
                    campaign_status: row.get(4)?,
                    repository: row.get(5)?,
                    base_commit: row.get(6)?,
                    spec_sha256: row.get(7)?,
                })
            },
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    Ok(context)
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

fn candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CandidateRecord> {
    Ok(CandidateRecord {
        id: row.get(0)?,
        attempt_id: row.get(1)?,
        action_spec_sha256: row.get(2)?,
        commit_sha: row.get(3)?,
        tree_sha: row.get(4)?,
        implementer_id: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn load_candidate(connection: &Connection, id: &str) -> Result<CandidateRecord, Error> {
    Ok(connection.query_row(
        "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at
         FROM candidates WHERE id=?1",
        [id],
        candidate_row,
    )?)
}

fn load_candidate_for_attempt(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Option<CandidateRecord>, Error> {
    Ok(connection
        .query_row(
            "SELECT id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at
             FROM candidates WHERE attempt_id=?1",
            [attempt_id],
            candidate_row,
        )
        .optional()?)
}

fn receipt_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<QualityReceiptRecord> {
    let role: String = row.get(2)?;
    let result: String = row.get(7)?;
    let evidence: String = row.get(8)?;
    Ok(QualityReceiptRecord {
        receipt_id: row.get(0)?,
        candidate_id: row.get(1)?,
        role: parse_role(&role).map_err(|_| {
            rusqlite::Error::InvalidColumnType(2, "role".into(), rusqlite::types::Type::Text)
        })?,
        principal_id: row.get(3)?,
        action_spec_sha256: row.get(4)?,
        commit_sha: row.get(5)?,
        tree_sha: row.get(6)?,
        result: parse_receipt_result(&result).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(7, "result".into(), rusqlite::types::Type::Text)
        })?,
        evidence: parse_evidence_kind(&evidence).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(8, "evidence".into(), rusqlite::types::Type::Text)
        })?,
        received_at: row.get(9)?,
    })
}

fn load_receipt_by_id(
    connection: &Connection,
    receipt_id: &str,
) -> Result<Option<QualityReceiptRecord>, Error> {
    Ok(connection
        .query_row(
            "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,result,evidence,received_at
             FROM quality_receipts WHERE receipt_id=?1",
            [receipt_id],
            receipt_row,
        )
        .optional()?)
}

fn load_receipt_by_role(
    connection: &Connection,
    candidate_id: &str,
    role: QualityRole,
) -> Result<Option<QualityReceiptRecord>, Error> {
    Ok(connection
        .query_row(
            "SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,result,evidence,received_at
             FROM quality_receipts WHERE candidate_id=?1 AND role=?2",
            params![candidate_id, role.as_str()],
            receipt_row,
        )
        .optional()?)
}

fn receipt_matches(existing: &QualityReceiptRecord, input: &QualityReceiptInput) -> bool {
    existing.principal_id == input.principal_id
        && existing.action_spec_sha256 == input.action_spec_sha256
        && existing.commit_sha.eq_ignore_ascii_case(&input.commit_sha)
        && existing.tree_sha.eq_ignore_ascii_case(&input.tree_sha)
        && existing.result == input.result
        && existing.evidence == input.evidence
}

fn load_gate(connection: &Connection, candidate_id: &str) -> Result<Option<GateRecord>, Error> {
    let row: Option<(String, String, String)> = connection
        .query_row(
            "SELECT outcome,reasons_json,decided_at FROM gate_decisions WHERE candidate_id=?1",
            [candidate_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
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

fn load_gate_policy(connection: &Connection, candidate_id: &str) -> Result<GatePolicy, Error> {
    let (campaign_status, risk): (String, String) = connection
        .query_row(
            "SELECT c.status,ac.risk FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             WHERE cd.id=?1",
            [candidate_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
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
