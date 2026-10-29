//! Immutable execution receipts for frozen acceptance checks.
//!
//! A progress `TestsImproved` claim is not evidence of a real check by itself.
//! Core records one receipt per `run_command` result: the exact Campaign, work
//! item, Attempt, generation, frozen command, suite label, candidate identity,
//! exit code, expected exit and normalized outcome. Progress evidence must cite
//! that receipt and match every bound field.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;

use crate::execution_policy::{ProgressEvidence, ProgressKind};
use crate::lead::SpecCheck;

/// Worker-supplied execution facts. Core derives the expected exit, suite,
/// outcome, command digest and result digest from the frozen Attempt spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionCheckInput {
    pub command: Vec<String>,
    pub candidate_sha256: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// The immutable receipt returned to the Worker and later cited by progress
/// evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionCheckReceiptRecord {
    pub receipt_id: String,
    pub campaign_id: String,
    pub work_item_id: String,
    pub attempt_id: String,
    pub session_record_id: String,
    pub sequence: i64,
    pub call_id: String,
    pub suite: String,
    pub candidate_sha256: String,
    pub exit_code: Option<i32>,
    pub expected_exit: i32,
    pub outcome: String,
    pub result_sha256: String,
}

#[derive(Debug, Clone)]
pub(super) struct ExecutionCheckReceipt {
    pub rowid: i64,
    pub receipt_id: String,
    pub campaign_id: String,
    pub attempt_id: String,
    pub work_item_id: String,
    pub call_id: String,
    pub suite: String,
    pub command_json: String,
    pub command_sha256: String,
    pub candidate_sha256: String,
    pub exit_code: Option<i32>,
    pub expected_exit: i32,
    pub outcome: String,
    pub result_sha256: String,
}

pub(super) fn suite_for_command(command: &[String]) -> String {
    format!("check:{}", command.join("\u{0}"))
}

/// Insert the receipt for one `run_command` result in the same transaction as
/// the provider tool event. The command must exactly match a frozen check in
/// the Attempt spec; the Worker cannot name an arbitrary suite.
pub(super) async fn record_execution_check_receipt_in_tx(
    connection: &mut SqliteConnection,
    session_record_id: &str,
    sequence: i64,
    generation: i64,
    call_id: &str,
    input: &ExecutionCheckInput,
) -> Result<ExecutionCheckReceiptRecord, Error> {
    if sequence <= 0
        || generation <= 0
        || session_record_id.trim().is_empty()
        || call_id.trim().is_empty()
        || input.command.is_empty()
        || input
            .command
            .iter()
            .any(|part| part.trim().is_empty() || part.len() > 4_096)
        || !is_sha256(&input.candidate_sha256)
    {
        return Err(Error::Invalid("execution check receipt"));
    }
    let row = sqlx::query::query(
        "SELECT ps.attempt_id,ps.generation,r.campaign_id,
                COALESCE(lt.unit_id,li.integration_id,r.campaign_id) AS work_item_id
         FROM provider_sessions ps
         JOIN attempts a ON a.id=ps.attempt_id
         JOIN runs r ON r.id=a.run_id
         LEFT JOIN lead_tasks lt ON lt.attempt_id=a.id
         LEFT JOIN lead_integrations li ON li.attempt_id=a.id
         WHERE ps.session_record_id=?1",
    )
    .bind(session_record_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    let attempt_id: String = row.try_get("attempt_id")?;
    let saved_generation: i64 = row.try_get("generation")?;
    let campaign_id: String = row.try_get("campaign_id")?;
    let work_item_id: String = row.try_get("work_item_id")?;
    if saved_generation != generation {
        return Err(Error::Forbidden(
            "execution check receipt generation differs from the provider session".into(),
        ));
    }
    let check = frozen_check_for_attempt(connection, &attempt_id, &input.command).await?;
    let suite = suite_for_command(&input.command);
    let expected_exit = check.expect_exit;
    let outcome = if input.exit_code == Some(expected_exit) {
        "PASS"
    } else {
        "FAIL"
    };
    let command_json = serde_json::to_string(&input.command)
        .map_err(|_| Error::Invalid("execution check command encoding"))?;
    let command_sha256 = sha256_hex(command_json.as_bytes());
    let result_sha256 = execution_result_sha256(
        &campaign_id,
        &work_item_id,
        &attempt_id,
        &suite,
        &command_sha256,
        &input.candidate_sha256,
        input.exit_code,
        expected_exit,
        outcome,
    );
    let receipt_id = format!("{session_record_id}:{sequence}");
    let created_at = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO execution_check_receipts
         (receipt_id,session_record_id,sequence,campaign_id,attempt_id,work_item_id,
          generation,call_id,suite,command_json,command_sha256,candidate_sha256,
          exit_code,expected_exit,outcome,result_sha256,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
    )
    .bind(&receipt_id)
    .bind(session_record_id)
    .bind(sequence)
    .bind(&campaign_id)
    .bind(&attempt_id)
    .bind(&work_item_id)
    .bind(generation)
    .bind(call_id)
    .bind(&suite)
    .bind(&command_json)
    .bind(&command_sha256)
    .bind(&input.candidate_sha256)
    .bind(input.exit_code)
    .bind(expected_exit)
    .bind(outcome)
    .bind(&result_sha256)
    .bind(&created_at)
    .execute(&mut *connection)
    .await?;
    Ok(ExecutionCheckReceiptRecord {
        receipt_id,
        campaign_id,
        work_item_id,
        attempt_id,
        session_record_id: session_record_id.to_owned(),
        sequence,
        call_id: call_id.to_owned(),
        suite,
        candidate_sha256: input.candidate_sha256.clone(),
        exit_code: input.exit_code,
        expected_exit,
        outcome: outcome.into(),
        result_sha256,
    })
}

/// Load the frozen check for an Attempt. `attempt_specs` wins over the Action
/// spec, matching the Worker's authorized command list.
pub(super) async fn frozen_check_for_attempt(
    connection: &mut SqliteConnection,
    attempt_id: &str,
    command: &[String],
) -> Result<SpecCheck, Error> {
    let row: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT COALESCE(aps.spec_json,s.spec_json)
         FROM attempts a
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         JOIN actions ac ON ac.id=c.action_id
         JOIN action_specs s ON s.action_id=ac.id
         LEFT JOIN attempt_specs aps ON aps.attempt_id=a.id
         WHERE a.id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((spec_json,)) = row else {
        return Err(Error::NotFound);
    };
    let spec: ActionSpec = serde_json::from_str(&spec_json)
        .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
    spec.checks
        .into_iter()
        .find(|check| check.command.as_slice() == command)
        .ok_or_else(|| Error::Forbidden("command does not match a frozen acceptance check".into()))
}

pub(super) async fn verify_tests_improved_evidence_in_tx(
    connection: &mut SqliteConnection,
    campaign_id: &str,
    item: &ProgressEvidence,
    suite: &str,
    outcome: &str,
    call_id: &str,
) -> Result<(), Error> {
    if item.kind != ProgressKind::TestsImproved
        || item.attempt_id.trim().is_empty()
        || item.execution_receipt_id.trim().is_empty()
    {
        return Err(Error::Forbidden(
            "test progress requires a bound execution receipt".into(),
        ));
    }
    let receipt = load_execution_check_receipt(connection, &item.execution_receipt_id)
        .await?
        .ok_or_else(|| {
            Error::Forbidden("test progress cites an unknown execution receipt".into())
        })?;
    if receipt.campaign_id != campaign_id
        || receipt.work_item_id != item.work_item_id
        || receipt.attempt_id != item.attempt_id
    {
        return Err(Error::Forbidden(
            "execution receipt does not belong to this campaign, work item and attempt".into(),
        ));
    }
    if receipt.suite != suite
        || receipt.suite != item.baseline_ref
        || receipt.call_id != call_id
        || receipt.outcome != outcome
    {
        return Err(Error::Forbidden(
            "test progress suite, call id or outcome differs from the execution receipt".into(),
        ));
    }
    if receipt.result_sha256 != item.artifact_sha256
        || receipt.command_sha256 != sha256_hex(receipt.command_json.as_bytes())
        || !is_sha256(&receipt.candidate_sha256)
    {
        return Err(Error::Forbidden(
            "test progress digest or candidate identity was altered".into(),
        ));
    }
    let command: Vec<String> = serde_json::from_str(&receipt.command_json)
        .map_err(|_| Error::Forbidden("execution receipt command is invalid".into()))?;
    let check = frozen_check_for_attempt(connection, &receipt.attempt_id, &command).await?;
    if check.expect_exit != receipt.expected_exit || suite_for_command(&command) != receipt.suite {
        return Err(Error::Forbidden(
            "execution receipt no longer matches the frozen check".into(),
        ));
    }
    let derived_outcome = if receipt.exit_code == Some(receipt.expected_exit) {
        "PASS"
    } else {
        "FAIL"
    };
    if derived_outcome != receipt.outcome {
        return Err(Error::Forbidden(
            "execution receipt outcome differs from its exit code".into(),
        ));
    }
    let belongs: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT COALESCE(lt.unit_id,li.integration_id,r.campaign_id)
         FROM attempts a
         JOIN runs r ON r.id=a.run_id
         LEFT JOIN lead_tasks lt ON lt.attempt_id=a.id
         LEFT JOIN lead_integrations li ON li.attempt_id=a.id
         WHERE a.id=?1 AND r.campaign_id=?2",
    )
    .bind(&receipt.attempt_id)
    .bind(campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    if belongs.as_ref().map(|(work_item,)| work_item.as_str())
        != Some(receipt.work_item_id.as_str())
    {
        return Err(Error::Forbidden(
            "execution receipt attempt is foreign to this campaign or work item".into(),
        ));
    }
    let consumed: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT evidence_id FROM progress_evidence WHERE execution_receipt_id=?1 LIMIT 1",
    )
    .bind(&receipt.receipt_id)
    .fetch_optional(&mut *connection)
    .await?;
    if consumed.is_some() {
        return Err(Error::Forbidden(
            "execution receipt was already consumed by progress evidence".into(),
        ));
    }
    if receipt.outcome == "PASS" {
        let prior: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT outcome,candidate_sha256 FROM execution_check_receipts
             WHERE campaign_id=?1 AND work_item_id=?2 AND suite=?3 AND command_sha256=?4
               AND rowid < ?5
             ORDER BY rowid DESC LIMIT 1",
        )
        .bind(campaign_id)
        .bind(&receipt.work_item_id)
        .bind(&receipt.suite)
        .bind(&receipt.command_sha256)
        .bind(receipt.rowid)
        .fetch_optional(&mut *connection)
        .await?;
        let Some((prior_outcome, prior_candidate)) = prior else {
            return Err(Error::Forbidden(
                "test PASS has no earlier FAIL receipt for this check".into(),
            ));
        };
        if prior_outcome != "FAIL" {
            return Err(Error::Forbidden(
                "test PASS requires the preceding receipt for this check to be FAIL".into(),
            ));
        }
        if prior_candidate == receipt.candidate_sha256 {
            return Err(Error::Forbidden(
                "test PASS candidate did not advance from the failing candidate".into(),
            ));
        }
    }
    Ok(())
}

async fn load_execution_check_receipt(
    connection: &mut SqliteConnection,
    receipt_id: &str,
) -> Result<Option<ExecutionCheckReceipt>, Error> {
    let row = sqlx::query::query(
        "SELECT rowid,receipt_id,session_record_id,sequence,campaign_id,attempt_id,work_item_id,
                generation,call_id,suite,command_json,command_sha256,candidate_sha256,
                exit_code,expected_exit,outcome,result_sha256,created_at
         FROM execution_check_receipts WHERE receipt_id=?1",
    )
    .bind(receipt_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(ExecutionCheckReceipt {
        rowid: row.try_get("rowid")?,
        receipt_id: row.try_get("receipt_id")?,
        campaign_id: row.try_get("campaign_id")?,
        attempt_id: row.try_get("attempt_id")?,
        work_item_id: row.try_get("work_item_id")?,
        call_id: row.try_get("call_id")?,
        suite: row.try_get("suite")?,
        command_json: row.try_get("command_json")?,
        command_sha256: row.try_get("command_sha256")?,
        candidate_sha256: row.try_get("candidate_sha256")?,
        exit_code: row.try_get("exit_code")?,
        expected_exit: row.try_get("expected_exit")?,
        outcome: row.try_get("outcome")?,
        result_sha256: row.try_get("result_sha256")?,
    }))
}

#[allow(clippy::too_many_arguments)]
fn execution_result_sha256(
    campaign_id: &str,
    work_item_id: &str,
    attempt_id: &str,
    suite: &str,
    command_sha256: &str,
    candidate_sha256: &str,
    exit_code: Option<i32>,
    expected_exit: i32,
    outcome: &str,
) -> String {
    let payload = serde_json::json!({
        "campaignId": campaign_id,
        "workItemId": work_item_id,
        "attemptId": attempt_id,
        "suite": suite,
        "commandSha256": command_sha256,
        "candidateSha256": candidate_sha256,
        "exitCode": exit_code,
        "expectedExit": expected_exit,
        "outcome": outcome,
    });
    sha256_hex(payload.to_string().as_bytes())
}
