//! Persistence for risk-tiered delivery.
//!
//! Policy revisions are append-only. Delivery attempts keep an immutable
//! identity and record operational transitions through the append-only attempt
//! event log. Decisions, check receipts, incidents and manual reverts are
//! durable Core evidence, never Office or model input.

use super::*;

use crate::delivery::{
    CandidateSalvageEvidenceRecord, CandidateSalvageRecord, DeliveryAttemptRecord,
    DeliveryAttemptState, DeliveryAuthorization, DeliveryCheckReceiptRecord,
    DeliveryDecisionRecord, DeliveryIncidentRecord, DeliveryIncidentState,
    DeliveryIssueReceiptRecord, DeliveryMode, DeliveryPolicy, DeliveryPolicyInput,
    DeliveryPostMergeCheckRecord, DeliveryProjection, DeliveryRevertRecord, DeliveryRisk,
    policy_sha256,
};
use crate::quality::CandidateFinalSource;
use chrono::Utc;
use sqlx::row::Row;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DeliveryAttemptReservation {
    pub attempt: DeliveryAttemptRecord,
    pub created: bool,
    pub base_conflict: bool,
}

#[derive(Debug, Clone)]
pub struct NewDeliveryAttempt {
    pub idempotency_key: String,
    pub operation: &'static str,
    pub project_id: String,
    pub campaign_id: String,
    pub candidate_id: String,
    pub candidate_sha: String,
    pub candidate_tree_sha: String,
    pub content_digest: String,
    pub gate_revision: String,
    pub gate_outcome: String,
    pub policy_revision: u32,
    pub policy_sha256: String,
    pub risk_class: String,
    pub base_branch: String,
    pub expected_head_sha: String,
    pub merge_method: String,
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryAttemptUpdate {
    pub state: Option<DeliveryAttemptState>,
    pub authorization: Option<DeliveryAuthorization>,
    pub charter_grant_id: Option<String>,
    pub pull_request_number: Option<u64>,
    pub observed_head_sha: Option<String>,
    pub observed_base_sha: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub incident_id: Option<String>,
    pub post_merge_check_id: Option<String>,
    pub blocker_code: Option<String>,
    pub blocker_detail: Option<String>,
    pub merged_at: Option<String>,
    pub clear_blocker: bool,
}

pub struct NewDeliveryAttemptEvent<'a> {
    pub attempt_id: &'a str,
    pub kind: &'a str,
    pub detail: &'a str,
}

pub struct NewDeliveryDecision<'a> {
    pub attempt_id: &'a str,
    pub candidate_id: &'a str,
    pub candidate_sha: &'a str,
    pub gate_revision: &'a str,
    pub gate_outcome: &'a str,
    pub policy_revision: u32,
    pub policy_sha256: &'a str,
    pub charter_grant_id: Option<&'a str>,
    pub risk_class: &'a str,
    pub expected_head_sha: &'a str,
    pub base_branch: &'a str,
    pub checks_read_at: &'a str,
    pub branch_protection_read_at: &'a str,
    pub decision: &'a str,
    pub reasons: &'a [crate::delivery::DeliveryBlock],
}

pub struct NewDeliveryIncident<'a> {
    pub project_id: &'a str,
    pub attempt_id: &'a str,
    pub candidate_id: &'a str,
    pub merge_commit_sha: &'a str,
    pub kind: &'a str,
    pub detail: &'a str,
}

fn parse_mode(value: &str) -> Result<DeliveryMode, Error> {
    DeliveryMode::parse(value).ok_or(Error::Invalid("stored delivery policy mode"))
}

fn parse_risk(value: &str) -> Result<DeliveryRisk, Error> {
    DeliveryRisk::parse(value).ok_or(Error::Invalid("stored delivery risk class"))
}

fn parse_attempt_state(value: &str) -> Result<DeliveryAttemptState, Error> {
    DeliveryAttemptState::parse(value).ok_or(Error::Invalid("stored delivery attempt state"))
}

fn parse_authorization(value: Option<&str>) -> Result<Option<DeliveryAuthorization>, Error> {
    value
        .map(|value| {
            DeliveryAuthorization::parse(value)
                .ok_or(Error::Invalid("stored delivery authorization"))
        })
        .transpose()
}

fn parse_incident_state(value: &str) -> Result<DeliveryIncidentState, Error> {
    DeliveryIncidentState::parse(value).ok_or(Error::Invalid("stored delivery incident state"))
}

fn policy_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<DeliveryPolicy, Error> {
    Ok(DeliveryPolicy {
        schema_version: row.try_get::<i64, _>("schema_version")? as u32,
        project_id: row.try_get("project_id")?,
        revision: row.try_get::<i64, _>("revision")? as u32,
        policy_sha256: row.try_get("policy_sha256")?,
        mode: parse_mode(&row.try_get::<String, _>("mode")?)?,
        r1_enabled: row.try_get::<bool, _>("r1_enabled")?,
        auto_merge_base_branch: row.try_get("auto_merge_base_branch")?,
        manual_base_branches: serde_json::from_str(
            &row.try_get::<String, _>("manual_base_branches_json")?,
        )
        .map_err(|_| Error::Invalid("stored delivery manual bases"))?,
        required_checks: serde_json::from_str(&row.try_get::<String, _>("required_checks_json")?)
            .map_err(|_| Error::Invalid("stored delivery required checks"))?,
        merge_method: row.try_get("merge_method")?,
        post_merge_check_required: row.try_get::<bool, _>("post_merge_check_required")?,
        post_merge_check_id: row.try_get("post_merge_check_id")?,
        post_merge_timeout_secs: row.try_get::<i64, _>("post_merge_timeout_secs")? as u64,
        kill_switch: row.try_get::<bool, _>("kill_switch")?,
        created_by: row.try_get("created_by")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

fn policy_matches_input(policy: &DeliveryPolicy, input: &DeliveryPolicyInput) -> bool {
    policy.mode
        == if input.enabled {
            DeliveryMode::RiskTiered
        } else {
            DeliveryMode::Manual
        }
        && policy.r1_enabled == input.r1_enabled
        && policy.auto_merge_base_branch == input.auto_merge_base_branch
        && policy.manual_base_branches == input.manual_base_branches
        && policy.required_checks == input.required_checks
        && policy.merge_method == input.merge_method
        && policy.post_merge_check_id == input.post_merge_check_id
        && policy.post_merge_timeout_secs == input.post_merge_timeout_secs
        && policy.kill_switch == input.kill_switch
        && policy.expires_at == input.expires_at
}

fn attempt_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<DeliveryAttemptRecord, Error> {
    let authorization = row.try_get::<Option<String>, _>("authorization")?;
    Ok(DeliveryAttemptRecord {
        attempt_id: row.try_get("attempt_id")?,
        idempotency_key: row.try_get("idempotency_key")?,
        schema_version: row.try_get::<i64, _>("schema_version")? as u32,
        operation: row.try_get("operation")?,
        project_id: row.try_get("project_id")?,
        campaign_id: row.try_get("campaign_id")?,
        candidate_id: row.try_get("candidate_id")?,
        candidate_sha: row.try_get("candidate_sha")?,
        candidate_tree_sha: row.try_get("candidate_tree_sha")?,
        content_digest: row.try_get("content_digest")?,
        gate_revision: row.try_get("gate_revision")?,
        gate_outcome: row.try_get("gate_outcome")?,
        policy_revision: row.try_get::<i64, _>("policy_revision")? as u32,
        policy_sha256: row.try_get("policy_sha256")?,
        risk_class: parse_risk(&row.try_get::<String, _>("risk_class")?)?,
        authorization: parse_authorization(authorization.as_deref())?,
        charter_grant_id: row.try_get("charter_grant_id")?,
        pull_request_number: row
            .try_get::<Option<i64>, _>("pull_request_number")?
            .map(|value| value as u64),
        base_branch: row.try_get("base_branch")?,
        expected_head_sha: row.try_get("expected_head_sha")?,
        observed_head_sha: row.try_get("observed_head_sha")?,
        observed_base_sha: row.try_get("observed_base_sha")?,
        merge_method: row.try_get("merge_method")?,
        state: parse_attempt_state(&row.try_get::<String, _>("state")?)?,
        merge_commit_sha: row.try_get("merge_commit_sha")?,
        incident_id: row.try_get("incident_id")?,
        post_merge_check_id: row.try_get("post_merge_check_id")?,
        blocker_code: row.try_get("blocker_code")?,
        blocker_detail: row.try_get("blocker_detail")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        merged_at: row.try_get("merged_at")?,
    })
}

fn incident_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<DeliveryIncidentRecord, Error> {
    Ok(DeliveryIncidentRecord {
        incident_id: row.try_get("incident_id")?,
        project_id: row.try_get("project_id")?,
        attempt_id: row.try_get("attempt_id")?,
        candidate_id: row.try_get("candidate_id")?,
        merge_commit_sha: row.try_get("merge_commit_sha")?,
        kind: row.try_get("kind")?,
        state: parse_incident_state(&row.try_get::<String, _>("state")?)?,
        detail: row.try_get("detail")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        acknowledged_by: row.try_get("acknowledged_by")?,
        acknowledged_at: row.try_get("acknowledged_at")?,
        resolved_at: row.try_get("resolved_at")?,
    })
}

fn decision_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<DeliveryDecisionRecord, Error> {
    Ok(DeliveryDecisionRecord {
        decision_id: row.try_get("decision_id")?,
        attempt_id: row.try_get("attempt_id")?,
        candidate_id: row.try_get("candidate_id")?,
        candidate_sha: row.try_get("candidate_sha")?,
        gate_revision: row.try_get("gate_revision")?,
        gate_outcome: row.try_get("gate_outcome")?,
        policy_revision: row.try_get::<i64, _>("policy_revision")? as u32,
        policy_sha256: row.try_get("policy_sha256")?,
        charter_grant_id: row.try_get("charter_grant_id")?,
        risk_class: parse_risk(&row.try_get::<String, _>("risk_class")?)?,
        expected_head_sha: row.try_get("expected_head_sha")?,
        base_branch: row.try_get("base_branch")?,
        checks_read_at: row.try_get("checks_read_at")?,
        branch_protection_read_at: row.try_get("branch_protection_read_at")?,
        decision: row.try_get("decision")?,
        reasons: serde_json::from_str(&row.try_get::<String, _>("reasons_json")?)
            .map_err(|_| Error::Invalid("stored delivery decision reasons"))?,
        decided_at: row.try_get("decided_at")?,
    })
}

fn is_object_id(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Database {
    /// Exact base commit recorded for a candidate, preferring the Attempt
    /// spec over the Project default. Delivery must validate the candidate
    /// against the base it was actually produced from.
    pub async fn candidate_base_commit(&self, candidate_id: &str) -> Result<String, Error> {
        let row: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT COALESCE(aps.base_commit,p.base_commit)
             FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             JOIN projects p ON p.id=ac.project_id
             LEFT JOIN attempt_specs aps ON aps.attempt_id=a.id
             WHERE cd.id=?1",
        )
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(base,)| base).ok_or(Error::NotFound)
    }

    /// Read the exact candidate's final-source provenance and the immutable
    /// Core-derived stagnation-salvage evidence. R1+ salvage never becomes
    /// delivery authority; delivery must fail closed on a missing or mismatched
    /// evidence record.
    pub async fn candidate_salvage_record(
        &self,
        candidate_id: &str,
    ) -> Result<CandidateSalvageRecord, Error> {
        let row: Option<(String, String, String)> = sqlx::query_as::query_as(
            "SELECT final_source,attempt_id,commit_sha FROM candidates WHERE id=?1",
        )
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        let (final_source, attempt_id, commit_sha) = row.ok_or(Error::NotFound)?;
        let final_source = CandidateFinalSource::parse(&final_source)
            .ok_or(Error::Invalid("candidate final source"))?;
        if final_source == CandidateFinalSource::ModelFinal {
            return Ok(CandidateSalvageRecord {
                final_source,
                evidence: None,
            });
        }
        let detail: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT detail FROM events
             WHERE subject_id=?1 AND kind='worker.stagnation_finalized'
             ORDER BY sequence DESC LIMIT 1",
        )
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        let detail = detail
            .map(|(detail,)| detail)
            .ok_or(Error::Invalid("stagnation salvage evidence is missing"))?;
        let value: serde_json::Value = serde_json::from_str(&detail)
            .map_err(|_| Error::Invalid("stagnation salvage evidence is malformed"))?;
        let field = |name: &str| {
            value
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or(Error::Invalid("stagnation salvage evidence field"))
        };
        if field("candidateId")? != candidate_id
            || field("commitSha")? != commit_sha
            || field("attemptId")? != attempt_id
            || field("decision")? != "DIAGNOSE"
            || value.get("written").and_then(serde_json::Value::as_bool) != Some(true)
        {
            return Err(Error::Invalid(
                "stagnation salvage evidence does not match the exact candidate",
            ));
        }
        let frozen_checks = value
            .get("frozenChecks")
            .and_then(serde_json::Value::as_u64)
            .ok_or(Error::Invalid("stagnation salvage frozen checks"))?;
        let passed_checks = value
            .get("passedChecks")
            .and_then(serde_json::Value::as_u64)
            .ok_or(Error::Invalid("stagnation salvage passed checks"))?;
        if frozen_checks == 0 || passed_checks != frozen_checks {
            return Err(Error::Invalid(
                "stagnation salvage does not prove every frozen check passed",
            ));
        }
        let changed_paths = value
            .get("diff")
            .and_then(|diff| diff.get("changedPaths"))
            .and_then(serde_json::Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|paths| !paths.is_empty())
            .ok_or(Error::Invalid("stagnation salvage changed paths"))?;
        let check_receipt_ids = value
            .get("checkReceipts")
            .and_then(serde_json::Value::as_array)
            .map(|receipts| {
                receipts
                    .iter()
                    .filter_map(|receipt| {
                        receipt
                            .get("receiptId")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|receipts| !receipts.is_empty())
            .ok_or(Error::Invalid("stagnation salvage check receipts"))?;
        if check_receipt_ids.len() as u64 != frozen_checks {
            return Err(Error::Invalid(
                "stagnation salvage receipts do not match the frozen check count",
            ));
        }
        Ok(CandidateSalvageRecord {
            final_source,
            evidence: Some(CandidateSalvageEvidenceRecord {
                candidate_id: candidate_id.to_owned(),
                commit_sha,
                attempt_id,
                work_item_id: field("workItemId")?,
                frozen_checks,
                passed_checks,
                written: true,
                decision: "DIAGNOSE".into(),
                changed_paths,
                check_receipt_ids,
            }),
        })
    }

    pub async fn delivery_policy(&self, project_id: &str) -> Result<Option<DeliveryPolicy>, Error> {
        let row = sqlx::query::query(
            "SELECT project_id,revision,schema_version,policy_sha256,mode,r1_enabled,
                    auto_merge_base_branch,manual_base_branches_json,required_checks_json,
                    merge_method,post_merge_check_required,post_merge_check_id,
                    post_merge_timeout_secs,kill_switch,created_by,created_at,expires_at
             FROM delivery_policies WHERE project_id=?1 ORDER BY revision DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(policy_from_row).transpose()
    }

    pub async fn delivery_policy_revision(
        &self,
        project_id: &str,
        revision: u32,
    ) -> Result<DeliveryPolicy, Error> {
        let row = sqlx::query::query(
            "SELECT project_id,revision,schema_version,policy_sha256,mode,r1_enabled,
                    auto_merge_base_branch,manual_base_branches_json,required_checks_json,
                    merge_method,post_merge_check_required,post_merge_check_id,
                    post_merge_timeout_secs,kill_switch,created_by,created_at,expires_at
             FROM delivery_policies WHERE project_id=?1 AND revision=?2",
        )
        .bind(project_id)
        .bind(revision as i64)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        policy_from_row(&row)
    }

    pub async fn delivery_policies(&self, project_id: &str) -> Result<Vec<DeliveryPolicy>, Error> {
        let rows = sqlx::query::query(
            "SELECT project_id,revision,schema_version,policy_sha256,mode,r1_enabled,
                    auto_merge_base_branch,manual_base_branches_json,required_checks_json,
                    merge_method,post_merge_check_required,post_merge_check_id,
                    post_merge_timeout_secs,kill_switch,created_by,created_at,expires_at
             FROM delivery_policies WHERE project_id=?1 ORDER BY revision DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(policy_from_row).collect()
    }

    /// Append a policy revision, or return the current revision when the
    /// requested content is identical. Revision numbering is serialized.
    pub async fn append_delivery_policy(
        &self,
        project_id: &str,
        default_branch: &str,
        input: &DeliveryPolicyInput,
        actor: &str,
    ) -> Result<(DeliveryPolicy, bool), Error> {
        input.validate(default_branch).map_err(Error::Validation)?;
        if actor.trim().is_empty() || actor.len() > 256 {
            return Err(Error::Invalid("delivery policy actor"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = {
            let row = sqlx::query::query(
                "SELECT project_id,revision,schema_version,policy_sha256,mode,r1_enabled,
                        auto_merge_base_branch,manual_base_branches_json,required_checks_json,
                        merge_method,post_merge_check_required,post_merge_check_id,
                        post_merge_timeout_secs,kill_switch,created_by,created_at,expires_at
                 FROM delivery_policies WHERE project_id=?1 ORDER BY revision DESC LIMIT 1",
            )
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?;
            row.as_ref().map(policy_from_row).transpose()?
        };
        if let Some(current) = &current
            && policy_matches_input(current, input)
        {
            transaction.commit().await?;
            return Ok((current.clone(), false));
        }
        let exists: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM projects WHERE id=?1")
                .bind(project_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if exists.is_none() {
            return Err(Error::NotFound);
        }
        let revision = current
            .as_ref()
            .map(|policy| policy.revision + 1)
            .unwrap_or(1);
        let mode = if input.enabled {
            DeliveryMode::RiskTiered
        } else {
            DeliveryMode::Manual
        };
        let hash = policy_sha256(project_id, revision, mode, input);
        let now = Utc::now().to_rfc3339();
        let manual_json = serde_json::to_string(&input.manual_base_branches)
            .map_err(|_| Error::Invalid("delivery manual bases"))?;
        let checks_json = serde_json::to_string(&input.required_checks)
            .map_err(|_| Error::Invalid("delivery required checks"))?;
        sqlx::query::query(
            "INSERT INTO delivery_policies
             (project_id,revision,schema_version,policy_sha256,mode,r1_enabled,
              auto_merge_base_branch,manual_base_branches_json,required_checks_json,
              merge_method,post_merge_check_required,post_merge_check_id,
              post_merge_timeout_secs,kill_switch,created_by,created_at,expires_at)
             VALUES (?1,?2,1,?3,?4,?5,?6,?7,?8,'squash',1,?9,?10,?11,?12,?13,?14)",
        )
        .bind(project_id)
        .bind(revision as i64)
        .bind(&hash)
        .bind(mode.as_str())
        .bind(input.r1_enabled)
        .bind(&input.auto_merge_base_branch)
        .bind(&manual_json)
        .bind(&checks_json)
        .bind(&input.post_merge_check_id)
        .bind(input.post_merge_timeout_secs as i64)
        .bind(input.kill_switch)
        .bind(actor)
        .bind(&now)
        .bind(&input.expires_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.delivery_policy_revision(project_id, revision)
            .await
            .map(|policy| (policy, true))
    }

    pub async fn create_delivery_attempt(
        &self,
        input: NewDeliveryAttempt,
    ) -> Result<DeliveryAttemptReservation, Error> {
        let attempt_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing_row = sqlx::query::query(
            "SELECT attempt_id,idempotency_key,schema_version,operation,project_id,campaign_id,
                    candidate_id,candidate_sha,candidate_tree_sha,content_digest,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,risk_class,authorization,
                    charter_grant_id,pull_request_number,base_branch,expected_head_sha,
                    observed_head_sha,observed_base_sha,merge_method,state,merge_commit_sha,
                    incident_id,post_merge_check_id,blocker_code,blocker_detail,created_at,
                    updated_at,merged_at
             FROM delivery_attempts
             WHERE candidate_id=?1 AND policy_revision=?2 AND expected_head_sha=?3
               AND operation=?4
             ORDER BY created_at,attempt_id LIMIT 1",
        )
        .bind(&input.candidate_id)
        .bind(input.policy_revision as i64)
        .bind(&input.expected_head_sha)
        .bind(input.operation)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = existing_row {
            let attempt = attempt_from_row(&row)?;
            if attempt.base_branch != input.base_branch {
                transaction.commit().await?;
                return Ok(DeliveryAttemptReservation {
                    attempt,
                    created: false,
                    base_conflict: true,
                });
            }
            transaction.commit().await?;
            return Ok(DeliveryAttemptReservation {
                attempt,
                created: false,
                base_conflict: false,
            });
        }
        let inserted = sqlx::query::query(
            "INSERT INTO delivery_attempts
             (attempt_id,idempotency_key,schema_version,operation,project_id,campaign_id,
              candidate_id,candidate_sha,candidate_tree_sha,content_digest,gate_revision,
              gate_outcome,policy_revision,policy_sha256,risk_class,authorization,
              charter_grant_id,pull_request_number,base_branch,expected_head_sha,
              observed_head_sha,observed_base_sha,merge_method,state,merge_commit_sha,
              incident_id,post_merge_check_id,blocker_code,blocker_detail,created_at,
              updated_at,merged_at)
             VALUES (?1,?2,1,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,NULL,NULL,NULL,
                     ?15,?16,NULL,NULL,'squash','PENDING',NULL,NULL,NULL,NULL,NULL,?17,?17,NULL)
             ON CONFLICT(idempotency_key) DO NOTHING",
        )
        .bind(&attempt_id)
        .bind(&input.idempotency_key)
        .bind(input.operation)
        .bind(&input.project_id)
        .bind(&input.campaign_id)
        .bind(&input.candidate_id)
        .bind(&input.candidate_sha)
        .bind(&input.candidate_tree_sha)
        .bind(&input.content_digest)
        .bind(&input.gate_revision)
        .bind(&input.gate_outcome)
        .bind(input.policy_revision as i64)
        .bind(&input.policy_sha256)
        .bind(&input.risk_class)
        .bind(&input.base_branch)
        .bind(&input.expected_head_sha)
        .bind(&now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let created = inserted == 1;
        let row = sqlx::query::query(
            "SELECT attempt_id,idempotency_key,schema_version,operation,project_id,campaign_id,
                    candidate_id,candidate_sha,candidate_tree_sha,content_digest,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,risk_class,authorization,
                    charter_grant_id,pull_request_number,base_branch,expected_head_sha,
                    observed_head_sha,observed_base_sha,merge_method,state,merge_commit_sha,
                    incident_id,post_merge_check_id,blocker_code,blocker_detail,created_at,
                    updated_at,merged_at
             FROM delivery_attempts WHERE idempotency_key=?1",
        )
        .bind(&input.idempotency_key)
        .fetch_one(&mut *transaction)
        .await?;
        let attempt = attempt_from_row(&row)?;
        if attempt.project_id != input.project_id
            || attempt.campaign_id != input.campaign_id
            || attempt.candidate_id != input.candidate_id
            || !attempt
                .candidate_sha
                .eq_ignore_ascii_case(&input.candidate_sha)
            || attempt.policy_revision != input.policy_revision
            || attempt.expected_head_sha != input.expected_head_sha
            || attempt.operation != input.operation
        {
            return Err(Error::Conflict(
                "delivery idempotency key was reused with different exact inputs",
            ));
        }
        if created {
            sqlx::query::query(
                "INSERT INTO delivery_attempt_events(event_id,attempt_id,kind,detail,recorded_at)
                 VALUES (?1,?2,'CREATED','exact candidate and policy revision bound',?3)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&attempt.attempt_id)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
        }
        let base_conflict = attempt.base_branch != input.base_branch;
        transaction.commit().await?;
        Ok(DeliveryAttemptReservation {
            attempt,
            created,
            base_conflict,
        })
    }

    pub async fn delivery_attempt(&self, attempt_id: &str) -> Result<DeliveryAttemptRecord, Error> {
        let row = sqlx::query::query(
            "SELECT attempt_id,idempotency_key,schema_version,operation,project_id,campaign_id,
                    candidate_id,candidate_sha,candidate_tree_sha,content_digest,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,risk_class,authorization,
                    charter_grant_id,pull_request_number,base_branch,expected_head_sha,
                    observed_head_sha,observed_base_sha,merge_method,state,merge_commit_sha,
                    incident_id,post_merge_check_id,blocker_code,blocker_detail,created_at,
                    updated_at,merged_at
             FROM delivery_attempts WHERE attempt_id=?1",
        )
        .bind(attempt_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        attempt_from_row(&row)
    }

    pub async fn latest_delivery_attempt(
        &self,
        campaign_id: &str,
        candidate_id: &str,
    ) -> Result<Option<DeliveryAttemptRecord>, Error> {
        let row = sqlx::query::query(
            "SELECT attempt_id,idempotency_key,schema_version,operation,project_id,campaign_id,
                    candidate_id,candidate_sha,candidate_tree_sha,content_digest,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,risk_class,authorization,
                    charter_grant_id,pull_request_number,base_branch,expected_head_sha,
                    observed_head_sha,observed_base_sha,merge_method,state,merge_commit_sha,
                    incident_id,post_merge_check_id,blocker_code,blocker_detail,created_at,
                    updated_at,merged_at
             FROM delivery_attempts
             WHERE campaign_id=?1 AND candidate_id=?2
             ORDER BY created_at DESC,attempt_id DESC LIMIT 1",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(attempt_from_row).transpose()
    }

    pub async fn update_delivery_attempt(
        &self,
        attempt_id: &str,
        input: DeliveryAttemptUpdate,
    ) -> Result<DeliveryAttemptRecord, Error> {
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        let state = input.state.map(DeliveryAttemptState::as_str);
        let authorization = input.authorization.map(DeliveryAuthorization::as_str);
        let affected = sqlx::query::query(
            "UPDATE delivery_attempts SET
               state=COALESCE(?1,state),
               authorization=COALESCE(?2,authorization),
               charter_grant_id=COALESCE(?3,charter_grant_id),
               observed_head_sha=COALESCE(?4,observed_head_sha),
               observed_base_sha=COALESCE(?5,observed_base_sha),
               merge_commit_sha=COALESCE(?6,merge_commit_sha),
               incident_id=COALESCE(?7,incident_id),
               post_merge_check_id=COALESCE(?8,post_merge_check_id),
               blocker_code=CASE WHEN ?15 THEN NULL ELSE COALESCE(?9,blocker_code) END,
               blocker_detail=CASE WHEN ?15 THEN NULL ELSE COALESCE(?10,blocker_detail) END,
               merged_at=COALESCE(?11,merged_at),
               pull_request_number=COALESCE(?12,pull_request_number),
               updated_at=?13
             WHERE attempt_id=?14",
        )
        .bind(state)
        .bind(authorization)
        .bind(input.charter_grant_id)
        .bind(input.observed_head_sha)
        .bind(input.observed_base_sha)
        .bind(input.merge_commit_sha)
        .bind(input.incident_id)
        .bind(input.post_merge_check_id)
        .bind(input.blocker_code)
        .bind(input.blocker_detail)
        .bind(input.merged_at)
        .bind(input.pull_request_number.map(|number| number as i64))
        .bind(&now)
        .bind(attempt_id)
        .bind(input.clear_blocker)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(Error::NotFound);
        }
        self.delivery_attempt(attempt_id).await
    }

    /// Atomically claim the merge write. A duplicate concurrent delivery must
    /// not call the merge API twice even when both requests read the same
    /// durable attempt.
    pub async fn claim_delivery_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<(bool, DeliveryAttemptRecord), Error> {
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        let claimed = sqlx::query::query(
            "UPDATE delivery_attempts SET state='MERGING',updated_at=?1
             WHERE attempt_id=?2 AND state IN ('PENDING','AUTHORIZED','BLOCKED')",
        )
        .bind(&now)
        .bind(attempt_id)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1;
        let attempt = self.delivery_attempt(attempt_id).await?;
        Ok((claimed, attempt))
    }

    pub async fn record_delivery_attempt_event(
        &self,
        input: NewDeliveryAttemptEvent<'_>,
    ) -> Result<(), Error> {
        sqlx::query::query(
            "INSERT INTO delivery_attempt_events(event_id,attempt_id,kind,detail,recorded_at)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(input.attempt_id)
        .bind(input.kind)
        .bind(input.detail)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_delivery_decision(
        &self,
        input: NewDeliveryDecision<'_>,
    ) -> Result<DeliveryDecisionRecord, Error> {
        let reasons_json =
            serde_json::to_string(input.reasons).map_err(|_| Error::Invalid("delivery reasons"))?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO delivery_decisions
             (decision_id,attempt_id,candidate_id,candidate_sha,gate_revision,gate_outcome,
              policy_revision,policy_sha256,charter_grant_id,risk_class,expected_head_sha,
              base_branch,checks_read_at,branch_protection_read_at,decision,reasons_json,
              decided_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        )
        .bind(&id)
        .bind(input.attempt_id)
        .bind(input.candidate_id)
        .bind(input.candidate_sha)
        .bind(input.gate_revision)
        .bind(input.gate_outcome)
        .bind(input.policy_revision as i64)
        .bind(input.policy_sha256)
        .bind(input.charter_grant_id)
        .bind(input.risk_class)
        .bind(input.expected_head_sha)
        .bind(input.base_branch)
        .bind(input.checks_read_at)
        .bind(input.branch_protection_read_at)
        .bind(input.decision)
        .bind(&reasons_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query::query(
            "SELECT decision_id,attempt_id,candidate_id,candidate_sha,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,charter_grant_id,risk_class,
                    expected_head_sha,base_branch,checks_read_at,branch_protection_read_at,
                    decision,reasons_json,decided_at
             FROM delivery_decisions WHERE decision_id=?1",
        )
        .bind(&id)
        .fetch_one(&self.pool)
        .await?;
        decision_from_row(&row)
    }

    pub async fn latest_delivery_decision(
        &self,
        attempt_id: &str,
    ) -> Result<Option<DeliveryDecisionRecord>, Error> {
        let row = sqlx::query::query(
            "SELECT decision_id,attempt_id,candidate_id,candidate_sha,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,charter_grant_id,risk_class,
                    expected_head_sha,base_branch,checks_read_at,branch_protection_read_at,
                    decision,reasons_json,decided_at
             FROM delivery_decisions WHERE attempt_id=?1
             ORDER BY decided_at DESC,decision_id DESC LIMIT 1",
        )
        .bind(attempt_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(decision_from_row).transpose()
    }

    pub async fn record_delivery_check_receipts(
        &self,
        receipts: &[DeliveryCheckReceiptRecord],
    ) -> Result<(), Error> {
        for receipt in receipts {
            sqlx::query::query(
                "INSERT INTO delivery_check_receipts
                 (attempt_id,check_name,head_sha,state,conclusion,required_by_protection,observed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
            )
            .bind(&receipt.attempt_id)
            .bind(&receipt.check_name)
            .bind(&receipt.head_sha)
            .bind(&receipt.state)
            .bind(&receipt.conclusion)
            .bind(receipt.required_by_protection)
            .bind(&receipt.observed_at)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn record_delivery_post_merge_check(
        &self,
        attempt_id: &str,
        check_id: &str,
        merge_commit_sha: &str,
        status: &str,
        detail: &str,
    ) -> Result<DeliveryPostMergeCheckRecord, Error> {
        if !matches!(status, "PENDING" | "PASSED" | "FAILED" | "MISSING") {
            return Err(Error::Invalid("post-merge check status"));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO delivery_post_merge_checks
             (attempt_id,check_id,merge_commit_sha,status,detail,observed_at)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(attempt_id,check_id) DO UPDATE SET
               status=excluded.status, detail=excluded.detail, observed_at=excluded.observed_at",
        )
        .bind(attempt_id)
        .bind(check_id)
        .bind(merge_commit_sha)
        .bind(status)
        .bind(detail)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query::query(
            "SELECT attempt_id,check_id,merge_commit_sha,status,detail,observed_at
             FROM delivery_post_merge_checks WHERE attempt_id=?1 AND check_id=?2",
        )
        .bind(attempt_id)
        .bind(check_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(DeliveryPostMergeCheckRecord {
            attempt_id: row.try_get("attempt_id")?,
            check_id: row.try_get("check_id")?,
            merge_commit_sha: row.try_get("merge_commit_sha")?,
            status: row.try_get("status")?,
            detail: row.try_get("detail")?,
            observed_at: row.try_get("observed_at")?,
        })
    }

    pub async fn delivery_post_merge_check(
        &self,
        attempt_id: &str,
        check_id: &str,
    ) -> Result<DeliveryPostMergeCheckRecord, Error> {
        let row = sqlx::query::query(
            "SELECT attempt_id,check_id,merge_commit_sha,status,detail,observed_at
             FROM delivery_post_merge_checks WHERE attempt_id=?1 AND check_id=?2",
        )
        .bind(attempt_id)
        .bind(check_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        Ok(DeliveryPostMergeCheckRecord {
            attempt_id: row.try_get("attempt_id")?,
            check_id: row.try_get("check_id")?,
            merge_commit_sha: row.try_get("merge_commit_sha")?,
            status: row.try_get("status")?,
            detail: row.try_get("detail")?,
            observed_at: row.try_get("observed_at")?,
        })
    }

    pub async fn open_delivery_incident(
        &self,
        input: NewDeliveryIncident<'_>,
    ) -> Result<DeliveryIncidentRecord, Error> {
        if input.kind.trim().is_empty() || input.detail.trim().is_empty() {
            return Err(Error::Invalid("delivery incident fields"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query::query(
            "SELECT incident_id,project_id,attempt_id,candidate_id,merge_commit_sha,kind,state,
                    detail,created_at,updated_at,acknowledged_by,acknowledged_at,resolved_at
             FROM delivery_incidents
             WHERE attempt_id=?1 AND kind=?2 AND state IN ('OPEN','ACKNOWLEDGED','REVERT_PENDING')
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(input.attempt_id)
        .bind(input.kind)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = existing {
            transaction.commit().await?;
            return incident_from_row(&row);
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO delivery_incidents
             (incident_id,project_id,attempt_id,candidate_id,merge_commit_sha,kind,state,detail,
              created_at,updated_at,acknowledged_by,acknowledged_at,resolved_at)
             VALUES (?1,?2,?3,?4,?5,?6,'OPEN',?7,?8,?8,NULL,NULL,NULL)",
        )
        .bind(&id)
        .bind(input.project_id)
        .bind(input.attempt_id)
        .bind(input.candidate_id)
        .bind(input.merge_commit_sha)
        .bind(input.kind)
        .bind(input.detail)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query::query(
            "SELECT incident_id,project_id,attempt_id,candidate_id,merge_commit_sha,kind,state,
                    detail,created_at,updated_at,acknowledged_by,acknowledged_at,resolved_at
             FROM delivery_incidents WHERE incident_id=?1",
        )
        .bind(&id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        incident_from_row(&row)
    }

    pub async fn active_delivery_incident(
        &self,
        project_id: &str,
    ) -> Result<Option<DeliveryIncidentRecord>, Error> {
        let row = sqlx::query::query(
            "SELECT incident_id,project_id,attempt_id,candidate_id,merge_commit_sha,kind,state,
                    detail,created_at,updated_at,acknowledged_by,acknowledged_at,resolved_at
             FROM delivery_incidents
             WHERE project_id=?1 AND state IN ('OPEN','ACKNOWLEDGED','REVERT_PENDING')
             ORDER BY created_at DESC,incident_id DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(incident_from_row).transpose()
    }

    pub async fn delivery_incidents(
        &self,
        project_id: &str,
    ) -> Result<Vec<DeliveryIncidentRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT incident_id,project_id,attempt_id,candidate_id,merge_commit_sha,kind,state,
                    detail,created_at,updated_at,acknowledged_by,acknowledged_at,resolved_at
             FROM delivery_incidents WHERE project_id=?1 ORDER BY created_at DESC,incident_id DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(incident_from_row).collect()
    }

    pub async fn record_delivery_revert(
        &self,
        incident_id: &str,
        revert_pr_number: u64,
        revert_merge_sha: &str,
        actor: &str,
        rationale: &str,
    ) -> Result<DeliveryRevertRecord, Error> {
        if revert_pr_number == 0
            || !is_object_id(revert_merge_sha)
            || actor.trim().is_empty()
            || rationale.trim().is_empty()
            || rationale.len() > 4_000
        {
            return Err(Error::Invalid("delivery revert record"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let incident = sqlx::query::query(
            "SELECT incident_id,attempt_id,state FROM delivery_incidents WHERE incident_id=?1",
        )
        .bind(incident_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let state: String = incident.try_get("state")?;
        let attempt_id: String = incident.try_get("attempt_id")?;
        if matches!(state.as_str(), "CLOSED") {
            return Err(Error::Conflict("delivery incident is closed"));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let inserted = sqlx::query::query(
            "INSERT INTO delivery_revert_records
             (revert_id,incident_id,revert_pr_number,revert_merge_sha,actor,rationale,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(incident_id,revert_merge_sha) DO NOTHING",
        )
        .bind(&id)
        .bind(incident_id)
        .bind(revert_pr_number as i64)
        .bind(revert_merge_sha)
        .bind(actor)
        .bind(rationale)
        .bind(&now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let row = sqlx::query::query(
            "SELECT revert_id,incident_id,revert_pr_number,revert_merge_sha,actor,rationale,created_at
             FROM delivery_revert_records
             WHERE incident_id=?1 AND revert_merge_sha=?2",
        )
        .bind(incident_id)
        .bind(revert_merge_sha)
        .fetch_one(&mut *transaction)
        .await?;
        if inserted == 1 {
            sqlx::query::query(
                "UPDATE delivery_incidents SET state='REVERTED',resolved_at=?1,updated_at=?1
                 WHERE incident_id=?2",
            )
            .bind(&now)
            .bind(incident_id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query::query(
                "UPDATE delivery_attempts SET state='REVERTED',updated_at=?1 WHERE attempt_id=?2",
            )
            .bind(&now)
            .bind(&attempt_id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(DeliveryRevertRecord {
            revert_id: row.try_get("revert_id")?,
            incident_id: row.try_get("incident_id")?,
            revert_pr_number: row.try_get::<i64, _>("revert_pr_number")? as u64,
            revert_merge_sha: row.try_get("revert_merge_sha")?,
            actor: row.try_get("actor")?,
            rationale: row.try_get("rationale")?,
            created_at: row.try_get("created_at")?,
        })
    }

    pub async fn record_delivery_issue_receipts(
        &self,
        receipts: &[DeliveryIssueReceiptRecord],
    ) -> Result<(), Error> {
        for receipt in receipts {
            sqlx::query::query(
                "INSERT INTO delivery_issue_receipts
                 (receipt_id,attempt_id,issue_link_id,action,state,external_id,detail,recorded_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(attempt_id,issue_link_id,action) DO NOTHING",
            )
            .bind(&receipt.receipt_id)
            .bind(&receipt.attempt_id)
            .bind(&receipt.issue_link_id)
            .bind(&receipt.action)
            .bind(&receipt.state)
            .bind(&receipt.external_id)
            .bind(&receipt.detail)
            .bind(&receipt.recorded_at)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn delivery_issue_receipts(
        &self,
        attempt_id: &str,
    ) -> Result<Vec<DeliveryIssueReceiptRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT receipt_id,attempt_id,issue_link_id,action,state,external_id,detail,recorded_at
             FROM delivery_issue_receipts WHERE attempt_id=?1 ORDER BY recorded_at,receipt_id",
        )
        .bind(attempt_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(DeliveryIssueReceiptRecord {
                    receipt_id: row.try_get("receipt_id")?,
                    attempt_id: row.try_get("attempt_id")?,
                    issue_link_id: row.try_get("issue_link_id")?,
                    action: row.try_get("action")?,
                    state: row.try_get("state")?,
                    external_id: row.try_get("external_id")?,
                    detail: row.try_get("detail")?,
                    recorded_at: row.try_get("recorded_at")?,
                })
            })
            .collect()
    }

    pub async fn delivery_projection(
        &self,
        campaign_id: &str,
        candidate_id: &str,
    ) -> Result<DeliveryProjection, Error> {
        let project_id: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT a.project_id FROM candidates cd
             JOIN attempts at ON at.id=cd.attempt_id
             JOIN runs r ON r.id=at.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions a ON a.id=c.action_id
             WHERE c.id=?1 AND cd.id=?2",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        let project_id = project_id.ok_or(Error::NotFound)?.0;
        let policy = self.delivery_policy(&project_id).await?;
        let attempt_rows = sqlx::query::query(
            "SELECT attempt_id,idempotency_key,schema_version,operation,project_id,campaign_id,
                    candidate_id,candidate_sha,candidate_tree_sha,content_digest,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,risk_class,authorization,
                    charter_grant_id,pull_request_number,base_branch,expected_head_sha,
                    observed_head_sha,observed_base_sha,merge_method,state,merge_commit_sha,
                    incident_id,post_merge_check_id,blocker_code,blocker_detail,created_at,
                    updated_at,merged_at
             FROM delivery_attempts WHERE campaign_id=?1 AND candidate_id=?2
             ORDER BY created_at DESC,attempt_id DESC",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        let attempts = attempt_rows
            .iter()
            .map(attempt_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let decision_rows = sqlx::query::query(
            "SELECT decision_id,attempt_id,candidate_id,candidate_sha,gate_revision,
                    gate_outcome,policy_revision,policy_sha256,charter_grant_id,risk_class,
                    expected_head_sha,base_branch,checks_read_at,branch_protection_read_at,
                    decision,reasons_json,decided_at
             FROM delivery_decisions
             WHERE attempt_id IN (SELECT attempt_id FROM delivery_attempts
                                  WHERE campaign_id=?1 AND candidate_id=?2)
             ORDER BY decided_at,decision_id",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        let decisions = decision_rows
            .iter()
            .map(decision_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let check_rows = sqlx::query::query(
            "SELECT attempt_id,check_name,head_sha,state,conclusion,required_by_protection,observed_at
             FROM delivery_check_receipts
             WHERE attempt_id IN (SELECT attempt_id FROM delivery_attempts
                                  WHERE campaign_id=?1 AND candidate_id=?2)
             ORDER BY check_name",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        let check_receipts = check_rows
            .iter()
            .map(|row| {
                Ok(DeliveryCheckReceiptRecord {
                    attempt_id: row.try_get("attempt_id")?,
                    check_name: row.try_get("check_name")?,
                    head_sha: row.try_get("head_sha")?,
                    state: row.try_get("state")?,
                    conclusion: row.try_get("conclusion")?,
                    required_by_protection: row.try_get("required_by_protection")?,
                    observed_at: row.try_get("observed_at")?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let post_rows = sqlx::query::query(
            "SELECT attempt_id,check_id,merge_commit_sha,status,detail,observed_at
             FROM delivery_post_merge_checks
             WHERE attempt_id IN (SELECT attempt_id FROM delivery_attempts
                                  WHERE campaign_id=?1 AND candidate_id=?2)
             ORDER BY observed_at",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        let post_merge_checks = post_rows
            .iter()
            .map(|row| {
                Ok(DeliveryPostMergeCheckRecord {
                    attempt_id: row.try_get("attempt_id")?,
                    check_id: row.try_get("check_id")?,
                    merge_commit_sha: row.try_get("merge_commit_sha")?,
                    status: row.try_get("status")?,
                    detail: row.try_get("detail")?,
                    observed_at: row.try_get("observed_at")?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let incidents = self
            .delivery_incidents(&project_id)
            .await?
            .into_iter()
            .filter(|incident| incident.candidate_id == candidate_id)
            .collect::<Vec<_>>();
        let revert_rows = sqlx::query::query(
            "SELECT r.revert_id,r.incident_id,r.revert_pr_number,r.revert_merge_sha,r.actor,
                    r.rationale,r.created_at
             FROM delivery_revert_records r
             JOIN delivery_incidents i ON i.incident_id=r.incident_id
             WHERE i.candidate_id=?1 ORDER BY r.created_at,r.revert_id",
        )
        .bind(candidate_id)
        .fetch_all(&self.pool)
        .await?;
        let reverts = revert_rows
            .iter()
            .map(|row| {
                Ok(DeliveryRevertRecord {
                    revert_id: row.try_get("revert_id")?,
                    incident_id: row.try_get("incident_id")?,
                    revert_pr_number: row.try_get::<i64, _>("revert_pr_number")? as u64,
                    revert_merge_sha: row.try_get("revert_merge_sha")?,
                    actor: row.try_get("actor")?,
                    rationale: row.try_get("rationale")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let mut issue_receipts = Vec::new();
        for attempt in &attempts {
            issue_receipts.extend(self.delivery_issue_receipts(&attempt.attempt_id).await?);
        }
        Ok(DeliveryProjection {
            policy,
            attempts,
            decisions,
            check_receipts,
            post_merge_checks,
            incidents,
            reverts,
            issue_receipts,
        })
    }
}
