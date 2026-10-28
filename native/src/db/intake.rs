//! Durable unified-intake records, execution contexts and explicit execution
//! policy grants.
//!
//! This module owns persistence for the intake submission snapshot, append-only
//! operator messages and the exact profile/config grant provenance. It never
//! owns Lead or Campaign lifecycle state: those remain in `lead_requests`,
//! `lead_withdrawals`, `campaigns` and gate records.

use super::*;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

use crate::execution_profile::{ExecutionProfileContent, evaluate_executability};
use crate::intake::{
    EXECUTION_POLICY_MIN_DEADLINE_SECS, IntakeAuthorizationReason, IntakeAuthorizationState,
    IntakeCampaignBudget, IntakeMode, execution_policy_terms, intersect_campaign_budget,
};
use crate::lead::{ActionStatus, LeadPlanProposal, RiskClass, StandingDelegation, plan_hash};

use super::delegations::{DelegationUsage, create_standing_delegation_in_tx, load_delegation};
use super::execution_profiles::{
    insert_audit, load_bound_profile, load_current_content, load_provider_availability,
};
use super::lead::{
    create_draft_lead_request_in_tx, load_action_binding, load_frozen_plan_by_id,
    load_lead_request, prepare_draft_lead_request, withdrawal_in_tx,
};
use super::project_configs::load_project_config;

const INTAKE_LIST_SCAN_LIMIT: i64 = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeRequestRecord {
    pub id: String,
    pub project_id: String,
    pub objective: String,
    pub mode: IntakeMode,
    pub profile_id: String,
    pub profile_revision_id: String,
    pub profile_revision: u32,
    pub profile_hash: String,
    pub profile_name: String,
    pub action_id: String,
    pub delegation_id: String,
    pub lead_request_id: String,
    pub campaign_id: Option<String>,
    pub submission_json: String,
    pub submission_hash: String,
    pub idempotency_key: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeMessageRecord {
    pub id: String,
    pub request_id: String,
    pub role: String,
    pub body: String,
    pub body_sha256: String,
    pub idempotency_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadExecutionContextRecord {
    pub request_id: String,
    pub project_id: String,
    pub profile_id: String,
    pub profile_revision_id: String,
    pub profile_revision: u32,
    pub profile_hash: String,
    pub profile_name: String,
    pub project_config_hash: String,
    pub delegation_hash: String,
    pub context_json: String,
    pub context_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPolicyGrantRecord {
    pub delegation_id: String,
    pub project_id: String,
    pub principal_id: String,
    pub profile_id: String,
    pub profile_revision_id: String,
    pub profile_revision: u32,
    pub profile_hash: String,
    pub project_config_hash: String,
    pub acknowledged: bool,
    pub idempotency_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewExecutionPolicyGrant<'a> {
    pub project_id: &'a str,
    pub profile_id: &'a str,
    pub expected_revision: u32,
    pub expected_revision_hash: &'a str,
    pub project_config_hash: &'a str,
    pub acknowledge: bool,
    pub idempotency_key: &'a str,
    pub principal_id: &'a str,
    pub principal_label: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ExecutionPolicyGrantResult {
    pub grant: ExecutionPolicyGrantRecord,
    pub delegation: StandingDelegation,
    pub usage: DelegationUsage,
    pub created: bool,
}

/// Everything Core needs to validate an explicit approval without trusting a
/// client-supplied budget or identity. The frozen plan, exact profile
/// revision, grant provenance and derived budget are re-read from durable
/// records.
#[derive(Debug, Clone)]
pub struct IntakeApprovalContext {
    pub intake: IntakeRequestRecord,
    pub lead: super::lead::LeadRequestRecord,
    pub plan: super::lead::FrozenLeadPlanRecord,
    pub proposal: LeadPlanProposal,
    pub context: LeadExecutionContextRecord,
    pub profile: ExecutionProfileContent,
    pub grant: StandingDelegation,
    pub usage: DelegationUsage,
    pub config: crate::lead::ProjectConfig,
    pub action_spec: ActionSpec,
    pub spec_sha256: String,
    pub mechanism: String,
    pub budget: IntakeCampaignBudget,
}

struct ResolvedIntakeProfile {
    profile_id: String,
    revision_id: String,
    revision: u32,
    content_hash: String,
    content: ExecutionProfileContent,
}

struct PreparedIntakeRequest {
    submission_json: String,
    submission_hash: String,
    assessment_input_json: String,
    lead_idempotency_key: String,
}

impl Database {
    /// Create one intake request, its draft Action, Lead request, execution
    /// context and idempotency record in a single transaction. No Campaign is
    /// created and no autonomy is enabled.
    pub async fn create_intake_request(
        &self,
        project_id: &str,
        objective: &str,
        execution_profile_id: Option<&str>,
        mode: IntakeMode,
        idempotency_key: &str,
        principal_id: &str,
    ) -> Result<(IntakeRequestRecord, bool), Error> {
        validate_intake_submission(
            project_id,
            objective,
            execution_profile_id,
            idempotency_key,
            principal_id,
        )?;
        let prepared = prepare_intake_request(
            project_id,
            objective,
            execution_profile_id,
            mode,
            principal_id,
        )?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "create_intake_request",
            &prepared.submission_hash,
        )
        .await?
        {
            let existing = load_intake_request(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok((existing, false));
        }
        let config = load_project_config(&mut transaction, project_id).await?;
        if !config.active {
            return Err(Error::IntakeConflict {
                code: "PROJECT_INACTIVE",
                message: "the Project configuration is inactive".into(),
            });
        }
        let profile =
            resolve_intake_profile_in_tx(&mut transaction, project_id, execution_profile_id)
                .await?;
        let grant = find_matching_execution_policy_grant_in_tx(
            &mut transaction,
            project_id,
            principal_id,
            &profile,
            &config,
        )
        .await?;
        let draft = NewDraftLeadRequest {
            project_id,
            delegation_id: &grant.id,
            objective,
            project_config_hash: &config.config_hash,
            assessment_input_json: &prepared.assessment_input_json,
            idempotency_key: &prepared.lead_idempotency_key,
        };
        let prepared_lead = prepare_draft_lead_request(&draft)?;
        let (lead, _) =
            create_draft_lead_request_in_tx(&mut transaction, &draft, &prepared_lead).await?;
        let now = Utc::now().to_rfc3339();
        let record = IntakeRequestRecord {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            objective: objective.into(),
            mode,
            profile_id: profile.profile_id.clone(),
            profile_revision_id: profile.revision_id.clone(),
            profile_revision: profile.revision,
            profile_hash: profile.content_hash.clone(),
            profile_name: profile.content.name.clone(),
            action_id: lead.action_id.clone(),
            delegation_id: grant.id.clone(),
            lead_request_id: lead.id.clone(),
            campaign_id: None,
            submission_json: prepared.submission_json.clone(),
            submission_hash: prepared.submission_hash.clone(),
            idempotency_key: idempotency_key.into(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        sqlx::query::query(
            "INSERT INTO intake_requests
             (id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
              profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
              submission_json,submission_hash,idempotency_key,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?14,?15,?16,?16)",
        )
        .bind(&record.id)
        .bind(&record.project_id)
        .bind(&record.objective)
        .bind(mode.as_str())
        .bind(&record.profile_id)
        .bind(&record.profile_revision_id)
        .bind(i64::from(record.profile_revision))
        .bind(&record.profile_hash)
        .bind(&record.profile_name)
        .bind(&record.action_id)
        .bind(&record.delegation_id)
        .bind(&record.lead_request_id)
        .bind(&record.submission_json)
        .bind(&record.submission_hash)
        .bind(&record.idempotency_key)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;
        let context_json = serde_json::to_string(&serde_json::json!({
            "schema":"factory-lead-execution-context-v1",
            "projectId":project_id,
            "profile":{
                "id":profile.profile_id,
                "revisionId":profile.revision_id,
                "revision":profile.revision,
                "contentHash":profile.content_hash,
                "name":profile.content.name,
                "content":profile.content,
            },
            "projectConfigHash":config.config_hash,
            "delegationHash":grant.content_hash,
        }))
        .map_err(|_| Error::Invalid("lead execution context encoding"))?;
        let context_hash = sha256_hex(context_json.as_bytes());
        sqlx::query::query(
            "INSERT INTO lead_execution_context
             (request_id,project_id,profile_id,profile_revision_id,profile_revision,profile_hash,
              profile_name,project_config_hash,delegation_hash,context_json,context_hash,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )
        .bind(&lead.id)
        .bind(project_id)
        .bind(&record.profile_id)
        .bind(&record.profile_revision_id)
        .bind(i64::from(record.profile_revision))
        .bind(&record.profile_hash)
        .bind(&record.profile_name)
        .bind(&config.config_hash)
        .bind(&grant.content_hash)
        .bind(&context_json)
        .bind(&context_hash)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("create_intake_request")
            .bind(&record.submission_hash)
            .bind(&record.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &record.id,
            "intake.request_created",
            &format!(
                "project={} profile={} revision={} lead={}",
                record.project_id,
                record.profile_id,
                record.profile_revision,
                record.lead_request_id
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok((record, true))
    }

    pub async fn intake_request(&self, id: &str) -> Result<IntakeRequestRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_intake_request(&mut connection, id).await
    }

    /// Deterministic ActionSpec-shaped context for the bounded Lead assessment
    /// of a draft Action. It has no executable checks; the Lead proposes the
    /// plan checks and freeze inserts the first executable spec. This value is
    /// never persisted as an ActionSpec and never used by a Worker or Gate.
    pub async fn intake_assessment_spec(&self, intake_id: &str) -> Result<ActionSpec, Error> {
        let mut connection = self.pool.acquire().await?;
        let record = load_intake_request(&mut connection, intake_id).await?;
        let lead = load_lead_request(&mut connection, &record.lead_request_id).await?;
        let config = load_project_config(&mut connection, &record.project_id).await?;
        let context = load_execution_context(&mut connection, &lead.id)
            .await?
            .ok_or_else(|| Error::IntakeConflict {
                code: "INTAKE_PROFILE_CONTEXT_MISSING",
                message: "the request has no frozen execution profile context".into(),
            })?;
        let profile = load_frozen_profile_content(&mut connection, &context).await?;
        Ok(ActionSpec {
            schema_version: 1,
            project_id: config.project_id.clone(),
            repository: config.repository.clone(),
            base_commit: config.base_commit.clone(),
            objective_sha256: sha256_hex(lead.objective.as_bytes()),
            objective: lead.objective,
            provider: profile.roles.implementer.provider.clone(),
            model: profile.roles.implementer.model.clone(),
            allowed_paths: config.allowed_paths,
            checks: Vec::new(),
        })
    }

    /// Bounded scan used by the list projection. State filtering is applied by
    /// the API projection after deriving the authoritative Lead state.
    pub async fn intake_requests(&self) -> Result<Vec<IntakeRequestRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT id FROM intake_requests ORDER BY created_at DESC,id DESC LIMIT ?1",
        )
        .bind(INTAKE_LIST_SCAN_LIMIT)
        .fetch_all(&mut *connection)
        .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            records.push(load_intake_request(&mut connection, &id).await?);
        }
        Ok(records)
    }

    pub async fn intake_messages(
        &self,
        request_id: &str,
    ) -> Result<Vec<IntakeMessageRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_intake_messages(&mut connection, request_id).await
    }

    pub async fn lead_execution_context(
        &self,
        request_id: &str,
    ) -> Result<Option<LeadExecutionContextRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_execution_context(&mut connection, request_id).await
    }

    /// Append one durable operator message. Replays return the original row.
    /// A frozen plan, withdrawal or started Campaign refuses new messages:
    /// Core cannot distinguish an informational note from a material amendment,
    /// so a frozen request requires a new request instead of mutating evidence.
    pub async fn append_intake_message(
        &self,
        request_id: &str,
        body: &str,
        idempotency_key: &str,
    ) -> Result<(IntakeMessageRecord, bool), Error> {
        if body.trim().is_empty()
            || body.chars().count() > 8_000
            || body.trim() != body
            || idempotency_key.trim().is_empty()
            || idempotency_key.len() > 256
        {
            return Err(Error::Invalid("intake message input"));
        }
        let body_sha256 = sha256_hex(body.as_bytes());
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "append_intake_message",
            &body_sha256,
        )
        .await?
        {
            let existing = load_intake_message(&mut transaction, &id).await?;
            if existing.request_id != request_id || existing.body != body {
                return Err(Error::Conflict(
                    "message idempotency key identifies another message",
                ));
            }
            transaction.commit().await?;
            return Ok((existing, false));
        }
        let record = load_intake_request(&mut transaction, request_id).await?;
        let lead = load_lead_request(&mut transaction, &record.lead_request_id).await?;
        if withdrawal_in_tx(&mut transaction, &lead.id).await? {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::Withdrawn.as_str(),
                message: "the request was withdrawn; it cannot accept new messages".into(),
            });
        }
        if lead.campaign_id.is_some() {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::CampaignStarted.as_str(),
                message: "a Campaign has started; use the Campaign workspace for operator notes"
                    .into(),
            });
        }
        let frozen: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM frozen_lead_plans WHERE request_id=?1")
                .bind(&lead.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if frozen.is_some() {
            return Err(Error::IntakeConflict {
                code: "INTAKE_PLAN_FROZEN",
                message: "the plan is frozen; create a new request to amend it".into(),
            });
        }
        let now = Utc::now().to_rfc3339();
        let message = IntakeMessageRecord {
            id: Uuid::new_v4().to_string(),
            request_id: record.id.clone(),
            role: "operator".into(),
            body: body.into(),
            body_sha256: body_sha256.clone(),
            idempotency_key: idempotency_key.into(),
            created_at: now.clone(),
        };
        sqlx::query::query(
            "INSERT INTO intake_messages
             (id,request_id,role,body,body_sha256,idempotency_key,created_at)
             VALUES (?1,?2,'operator',?3,?4,?5,?6)",
        )
        .bind(&message.id)
        .bind(&message.request_id)
        .bind(&message.body)
        .bind(&message.body_sha256)
        .bind(&message.idempotency_key)
        .bind(&message.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("append_intake_message")
            .bind(&body_sha256)
            .bind(&message.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &record.id,
            "intake.message_recorded",
            &message.id,
        )
        .await?;
        transaction.commit().await?;
        Ok((message, true))
    }

    /// Create one explicit operator-authorized, non-autonomous standing
    /// execution policy grant from an exact profile revision and project
    /// configuration. Re-enabling exports a new grant after acknowledgement;
    /// an existing grant is never widened or replenished.
    pub async fn create_execution_policy_grant(
        &self,
        input: NewExecutionPolicyGrant<'_>,
    ) -> Result<ExecutionPolicyGrantResult, Error> {
        validate_execution_policy_input(&input)?;
        let digest = execution_policy_digest(&input);
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(delegation_id) = replay(
            &mut transaction,
            input.idempotency_key,
            "create_execution_policy_grant",
            &digest,
        )
        .await?
        {
            let grant = load_execution_policy_grant(&mut transaction, &delegation_id).await?;
            let delegation = load_delegation(&mut transaction, &delegation_id).await?;
            let usage = load_delegation_usage(&mut transaction, &delegation_id).await?;
            transaction.commit().await?;
            return Ok(ExecutionPolicyGrantResult {
                grant,
                delegation,
                usage,
                created: false,
            });
        }
        let config = load_project_config(&mut transaction, input.project_id).await?;
        if !config.active || config.config_hash != input.project_config_hash {
            return Err(Error::IntakeConflict {
                code: "PROJECT_CONFIG_CHANGED",
                message: "the Project configuration is inactive or the supplied hash is stale"
                    .into(),
            });
        }
        let current = load_current_content(&mut transaction, input.profile_id).await?;
        if current.revision != input.expected_revision {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_STALE",
                message: format!(
                    "the current profile revision is {}, not the acknowledged {}",
                    current.revision, input.expected_revision
                ),
            });
        }
        let profile_hash: (String,) = sqlx::query_as::query_as(
            "SELECT content_hash FROM execution_profile_revisions WHERE revision_id=?1",
        )
        .bind(&current.revision_id)
        .fetch_one(&mut *transaction)
        .await?;
        if current.content.compute_content_hash() != profile_hash.0
            || profile_hash.0 != input.expected_revision_hash
        {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_STALE",
                message: "the acknowledged profile revision hash is stale".into(),
            });
        }
        let availability = load_provider_availability(&mut transaction).await?;
        let (executable, warnings) = evaluate_executability(&current.content, &availability);
        if !executable {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
                message: format!(
                    "the execution profile is not executable: {}",
                    warnings.join("; ")
                ),
            });
        }
        if let Some((delegation, grant)) = find_reusable_execution_policy_grant_in_tx(
            &mut transaction,
            &input,
            &current.revision_id,
            &profile_hash.0,
        )
        .await?
        {
            let usage = load_delegation_usage(&mut transaction, &delegation.id).await?;
            transaction.commit().await?;
            return Ok(ExecutionPolicyGrantResult {
                grant,
                delegation,
                usage,
                created: false,
            });
        }
        if config.allowed_paths.is_empty() || config.capabilities.is_empty() {
            return Err(Error::IntakeConflict {
                code: "PROJECT_SCOPE_REQUIRED",
                message:
                    "configure explicit Project paths and capabilities before granting execution"
                        .into(),
            });
        }
        if current.content.budgets.deadline_secs < EXECUTION_POLICY_MIN_DEADLINE_SECS {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_DEADLINE_TOO_SHORT",
                message: format!(
                    "the profile deadline of {}s is too short for a bounded Campaign",
                    current.content.budgets.deadline_secs
                ),
            });
        }
        if current.content.budgets.max_sessions < 2 {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_SESSIONS_TOO_LOW",
                message:
                    "the profile needs at least two sessions for assessment and implementation"
                        .into(),
            });
        }
        let terms = execution_policy_terms(&current.content);
        let mut delegation = StandingDelegation {
            id: Uuid::new_v4().to_string(),
            principal_id: input.principal_id.into(),
            audit_id: format!("execution-policy:{}", Uuid::new_v4()),
            project_id: input.project_id.into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: terms.objective_classes.clone(),
            allowed_paths: config.allowed_paths.clone(),
            capabilities: config.capabilities.clone(),
            risk_classes: terms.risk_classes.clone(),
            autonomous_start: terms.autonomous_start,
            autonomous_risk_classes: Vec::new(),
            route: crate::lead::ProviderRoute {
                provider: current.content.roles.implementer.provider.clone(),
                model: current.content.roles.implementer.model.clone(),
                mode: "build".into(),
            },
            max_attempts: terms.max_attempts_per_request,
            max_timeout_secs: terms.max_timeout_secs_per_request as u32,
            max_repair_cycles: terms.max_repair_cycles,
            max_wall_clock_secs: terms.max_wall_clock_secs_per_request as u32,
            max_parallel: terms.max_parallel,
            max_sessions: terms.max_sessions_per_request,
            max_requests: terms.max_requests,
            max_cumulative_sessions: terms.max_cumulative_sessions,
            max_cumulative_attempts: terms.max_cumulative_attempts,
            expires_at: Utc::now() + chrono::Duration::days(terms.validity_days),
            revoked_at: None,
        };
        delegation.content_hash = delegation.compute_content_hash();
        let delegation_key = format!(
            "execution-policy-delegation:{}",
            sha256_hex(input.idempotency_key.as_bytes())
        );
        let (delegation, _) =
            create_standing_delegation_in_tx(&mut transaction, &delegation, &delegation_key)
                .await?;
        let now = Utc::now().to_rfc3339();
        let grant = ExecutionPolicyGrantRecord {
            delegation_id: delegation.id.clone(),
            project_id: input.project_id.into(),
            principal_id: input.principal_id.into(),
            profile_id: input.profile_id.into(),
            profile_revision_id: current.revision_id.clone(),
            profile_revision: current.revision,
            profile_hash: profile_hash.0.clone(),
            project_config_hash: input.project_config_hash.into(),
            acknowledged: true,
            idempotency_key: input.idempotency_key.into(),
            created_at: now.clone(),
        };
        sqlx::query::query(
            "INSERT INTO execution_policy_grants
             (delegation_id,project_id,principal_id,profile_id,profile_revision_id,
              profile_revision,profile_hash,project_config_hash,acknowledged,idempotency_key,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10)",
        )
        .bind(&grant.delegation_id)
        .bind(&grant.project_id)
        .bind(&grant.principal_id)
        .bind(&grant.profile_id)
        .bind(&grant.profile_revision_id)
        .bind(i64::from(grant.profile_revision))
        .bind(&grant.profile_hash)
        .bind(&grant.project_config_hash)
        .bind(&grant.idempotency_key)
        .bind(&grant.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(input.idempotency_key)
            .bind("create_execution_policy_grant")
            .bind(&digest)
            .bind(&delegation.id)
            .execute(&mut *transaction)
            .await?;
        insert_audit(
            &mut transaction,
            &grant.profile_id,
            Some(grant.profile_revision),
            "execution_policy_granted",
            input.principal_id,
            &format!(
                "project={} delegation={} config={} acknowledged=true",
                grant.project_id, grant.delegation_id, grant.project_config_hash
            ),
            &now,
        )
        .await?;
        let actor = input.principal_label.unwrap_or(input.principal_id);
        record_event(
            &mut transaction,
            &delegation.id,
            "execution_policy.granted",
            &format!(
                "project={} profile={} revision={} actor={} non_autonomous=true",
                grant.project_id, grant.profile_id, grant.profile_revision, actor
            ),
        )
        .await?;
        let usage = load_delegation_usage(&mut transaction, &delegation.id).await?;
        transaction.commit().await?;
        Ok(ExecutionPolicyGrantResult {
            grant,
            delegation,
            usage,
            created: true,
        })
    }

    pub async fn execution_policy_grant(
        &self,
        delegation_id: &str,
    ) -> Result<ExecutionPolicyGrantRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_execution_policy_grant(&mut connection, delegation_id).await
    }

    /// Truthful Campaign status and latest Quality Gate outcome for a request
    /// summary. Status and gate remain separate facts.
    pub async fn campaign_status_and_gate(
        &self,
        campaign_id: &str,
    ) -> Result<(String, Option<String>), Error> {
        let status: (String,) =
            sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                .bind(campaign_id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(Error::NotFound)?;
        let gate: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT gd.outcome FROM gate_decisions gd
             JOIN candidates c ON c.id=gd.candidate_id
             JOIN attempts a ON a.id=c.attempt_id
             JOIN runs r ON r.id=a.run_id
             WHERE r.campaign_id=?1
             ORDER BY gd.decided_at DESC LIMIT 1",
        )
        .bind(campaign_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok((status.0, gate.map(|(outcome,)| outcome)))
    }

    /// Re-read and validate the exact frozen identity for an explicit approval.
    /// This is the Core-owned concurrency guard used by the approve endpoint and
    /// by the truthful authorization projection.
    pub async fn intake_approval_context(
        &self,
        intake_id: &str,
    ) -> Result<IntakeApprovalContext, Error> {
        let mut connection = self.pool.acquire().await?;
        load_intake_approval_context(&mut connection, intake_id, false).await
    }

    /// Approval context for an idempotent start replay. Exact replay is
    /// resolved by the shared start boundary before lifecycle checks; this
    /// loader permits an already-started request so the original Campaign can
    /// be returned instead of rewriting replay as a conflict.
    pub async fn intake_start_context(
        &self,
        intake_id: &str,
    ) -> Result<IntakeApprovalContext, Error> {
        let mut connection = self.pool.acquire().await?;
        load_intake_approval_context(&mut connection, intake_id, true).await
    }

    /// Truthful authorization state for the request projection. It reports a
    /// stable reason code when approval cannot currently derive an
    /// authorization; it never synthesizes one.
    pub async fn intake_authorization_state(
        &self,
        intake_id: &str,
        state: crate::intake::IntakeState,
    ) -> IntakeAuthorizationState {
        use crate::intake::IntakeState;
        match state {
            IntakeState::Executing | IntakeState::Completed => {
                IntakeAuthorizationState::not_required()
            }
            IntakeState::Cancelled => IntakeAuthorizationState {
                required: false,
                available: false,
                reason: Some(IntakeAuthorizationReason::Withdrawn.as_str().into()),
            },
            IntakeState::Blocked | IntakeState::Failed => IntakeAuthorizationState::not_required(),
            IntakeState::Submitted | IntakeState::Assessing => {
                IntakeAuthorizationState::blocked(IntakeAuthorizationReason::FrozenPlanRequired)
            }
            IntakeState::PlanReady => {
                IntakeAuthorizationState::blocked(IntakeAuthorizationReason::PlanNotReady)
            }
            IntakeState::AwaitingApproval => match self.intake_approval_context(intake_id).await {
                Ok(_) => IntakeAuthorizationState::available(),
                Err(Error::IntakeConflict { code, .. }) => IntakeAuthorizationState {
                    required: true,
                    available: false,
                    reason: Some(code.into()),
                },
                Err(_) => IntakeAuthorizationState {
                    required: true,
                    available: false,
                    reason: Some("AUTHORIZATION_UNAVAILABLE".into()),
                },
            },
        }
    }
}

fn validate_intake_submission(
    project_id: &str,
    objective: &str,
    execution_profile_id: Option<&str>,
    idempotency_key: &str,
    principal_id: &str,
) -> Result<(), Error> {
    if project_id.trim().is_empty()
        || project_id.len() > 128
        || objective.trim().is_empty()
        || objective.chars().count() > 20_000
        || idempotency_key.trim().is_empty()
        || idempotency_key.len() > 256
        || principal_id.trim().is_empty()
        || principal_id.len() > 200
        || execution_profile_id.is_some_and(|id| id.trim().is_empty() || id.len() > 128)
    {
        return Err(Error::Invalid("intake request input"));
    }
    Ok(())
}

fn prepare_intake_request(
    project_id: &str,
    objective: &str,
    execution_profile_id: Option<&str>,
    mode: IntakeMode,
    principal_id: &str,
) -> Result<PreparedIntakeRequest, Error> {
    let submission_json = serde_json::to_string(&serde_json::json!({
        "schema":"factory-intake-submission-v1",
        "projectId":project_id,
        "objective":objective,
        "mode":mode,
        "executionProfileId":execution_profile_id,
        "principalId":principal_id,
    }))
    .map_err(|_| Error::Invalid("intake submission encoding"))?;
    let submission_hash = sha256_hex(submission_json.as_bytes());
    let assessment_input_json = serde_json::to_string(&serde_json::json!({
        "schema":"factory-intake-assessment-context-v1",
        "projectId":project_id,
        "objective":objective,
        "requestedProfileId":execution_profile_id,
    }))
    .map_err(|_| Error::Invalid("intake assessment encoding"))?;
    Ok(PreparedIntakeRequest {
        submission_json,
        submission_hash,
        assessment_input_json,
        lead_idempotency_key: format!("intake-lead:{}", Uuid::new_v4()),
    })
}

async fn resolve_intake_profile_in_tx(
    connection: &mut SqliteConnection,
    project_id: &str,
    execution_profile_id: Option<&str>,
) -> Result<ResolvedIntakeProfile, Error> {
    let profile_id = match execution_profile_id {
        Some(id) => id.to_owned(),
        None => {
            if let Some(id) = load_bound_profile(connection, "project", Some(project_id)).await? {
                id
            } else {
                load_bound_profile(connection, "global", None)
                    .await?
                    .ok_or_else(|| Error::IntakeConflict {
                        code: "EXECUTION_PROFILE_REQUIRED",
                        message: "no Project or global execution profile default is configured"
                            .into(),
                    })?
            }
        }
    };
    let current = load_current_content(connection, &profile_id)
        .await
        .map_err(|error| match error {
            Error::NotFound => Error::IntakeConflict {
                code: "EXECUTION_PROFILE_REQUIRED",
                message: "the selected execution profile does not exist".into(),
            },
            other => other,
        })?;
    let content_hash: (String,) = sqlx::query_as::query_as(
        "SELECT content_hash FROM execution_profile_revisions WHERE revision_id=?1",
    )
    .bind(&current.revision_id)
    .fetch_one(&mut *connection)
    .await?;
    if current.content.compute_content_hash() != content_hash.0 {
        return Err(Error::Conflict(
            "stored execution profile hash is inconsistent",
        ));
    }
    let availability = load_provider_availability(connection).await?;
    let (executable, warnings) = evaluate_executability(&current.content, &availability);
    if !executable {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
            message: format!(
                "the selected execution profile is not executable: {}",
                warnings.join("; ")
            ),
        });
    }
    Ok(ResolvedIntakeProfile {
        profile_id,
        revision_id: current.revision_id,
        revision: current.revision,
        content_hash: content_hash.0,
        content: current.content,
    })
}

async fn find_matching_execution_policy_grant_in_tx(
    connection: &mut SqliteConnection,
    project_id: &str,
    principal_id: &str,
    profile: &ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
) -> Result<StandingDelegation, Error> {
    let rows = sqlx::query::query(
        "SELECT delegation_id FROM execution_policy_grants
         WHERE project_id=?1 AND principal_id=?2 AND profile_revision_id=?3
           AND profile_hash=?4 AND project_config_hash=?5
         ORDER BY created_at DESC,delegation_id DESC",
    )
    .bind(project_id)
    .bind(principal_id)
    .bind(&profile.revision_id)
    .bind(&profile.content_hash)
    .bind(&config.config_hash)
    .fetch_all(&mut *connection)
    .await?;
    for row in rows {
        let delegation_id: String = row.try_get("delegation_id")?;
        let grant = load_delegation(connection, &delegation_id).await?;
        if grant.revoked_at.is_some() || grant.expires_at <= Utc::now() {
            continue;
        }
        if !delegation_matches_policy_scope(&grant, profile, config) {
            continue;
        }
        let usage = load_delegation_usage(connection, &grant.id).await?;
        let sessions = u64::from(profile.content.budgets.max_sessions);
        let attempts = u64::from(grant.max_attempts);
        if u64::from(usage.requests) + 1 > u64::from(grant.max_requests)
            || u64::from(usage.sessions) + sessions > u64::from(grant.max_cumulative_sessions)
            || u64::from(usage.attempts) + attempts > u64::from(grant.max_cumulative_attempts)
        {
            continue;
        }
        return Ok(grant);
    }
    Err(Error::IntakeConflict {
        code: IntakeAuthorizationReason::ExecutionPolicyRequired.as_str(),
        message:
            "no active non-autonomous execution policy matches this Project and exact profile revision; enable governed execution explicitly"
                .into(),
    })
}

fn delegation_matches_policy_scope(
    grant: &StandingDelegation,
    profile: &ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
) -> bool {
    grant.risk_classes == vec![RiskClass::R0]
        && !grant.autonomous_start
        && grant.autonomous_risk_classes.is_empty()
        && grant.route.provider == profile.content.roles.implementer.provider
        && grant.route.model == profile.content.roles.implementer.model
        && grant.route.mode == "build"
        && grant.allowed_paths == config.allowed_paths
        && grant.capabilities == config.capabilities
        && grant.max_attempts <= profile.content.budgets.max_attempts
        && grant.max_sessions <= profile.content.budgets.max_sessions
        && u64::from(grant.max_timeout_secs) <= profile.content.budgets.deadline_secs
        && u64::from(grant.max_wall_clock_secs) <= profile.content.budgets.deadline_secs
        && grant.max_parallel == 1
        && grant.max_repair_cycles == 0
}

async fn find_reusable_execution_policy_grant_in_tx(
    connection: &mut SqliteConnection,
    input: &NewExecutionPolicyGrant<'_>,
    revision_id: &str,
    profile_hash: &str,
) -> Result<Option<(StandingDelegation, ExecutionPolicyGrantRecord)>, Error> {
    let rows = sqlx::query::query(
        "SELECT delegation_id FROM execution_policy_grants
         WHERE project_id=?1 AND principal_id=?2 AND profile_id=?3
           AND profile_revision_id=?4 AND profile_hash=?5 AND project_config_hash=?6
         ORDER BY created_at DESC,delegation_id DESC",
    )
    .bind(input.project_id)
    .bind(input.principal_id)
    .bind(input.profile_id)
    .bind(revision_id)
    .bind(profile_hash)
    .bind(input.project_config_hash)
    .fetch_all(&mut *connection)
    .await?;
    for row in rows {
        let delegation_id: String = row.try_get("delegation_id")?;
        let delegation = load_delegation(connection, &delegation_id).await?;
        if delegation.revoked_at.is_some() || delegation.expires_at <= Utc::now() {
            continue;
        }
        let usage = load_delegation_usage(connection, &delegation.id).await?;
        if u64::from(usage.requests) + 1 > u64::from(delegation.max_requests)
            || u64::from(usage.sessions) + u64::from(delegation.max_sessions)
                > u64::from(delegation.max_cumulative_sessions)
            || u64::from(usage.attempts) + u64::from(delegation.max_attempts)
                > u64::from(delegation.max_cumulative_attempts)
        {
            continue;
        }
        let grant = load_execution_policy_grant(connection, &delegation.id).await?;
        return Ok(Some((delegation, grant)));
    }
    Ok(None)
}

async fn load_intake_approval_context(
    connection: &mut SqliteConnection,
    intake_id: &str,
    allow_started: bool,
) -> Result<IntakeApprovalContext, Error> {
    let intake = load_intake_request(connection, intake_id).await?;
    let lead = load_lead_request(connection, &intake.lead_request_id).await?;
    if withdrawal_in_tx(connection, &lead.id).await? {
        return Err(Error::IntakeConflict {
            code: IntakeAuthorizationReason::Withdrawn.as_str(),
            message: "the request was withdrawn".into(),
        });
    }
    if !allow_started && let Some(campaign_id) = &lead.campaign_id {
        let status: (String,) =
            sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                .bind(campaign_id)
                .fetch_one(&mut *connection)
                .await?;
        return Err(Error::AlreadyStarted {
            campaign_id: campaign_id.clone(),
            campaign_status: status.0,
        });
    }
    let plan_id: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM frozen_lead_plans WHERE request_id=?1")
            .bind(&lead.id)
            .fetch_optional(&mut *connection)
            .await?;
    let plan = match plan_id {
        Some((id,)) => load_frozen_plan_by_id(connection, &id).await?,
        None => {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::FrozenPlanRequired.as_str(),
                message: "the plan is not frozen".into(),
            });
        }
    };
    let proposal_json: (String,) =
        sqlx::query_as::query_as("SELECT proposal_json FROM lead_proposals WHERE id=?1")
            .bind(&plan.proposal_id)
            .fetch_one(&mut *connection)
            .await?;
    let proposal: LeadPlanProposal = serde_json::from_str(&proposal_json.0)
        .map_err(|_| Error::Invalid("stored proposal encoding"))?;
    let context = load_execution_context(connection, &lead.id)
        .await?
        .ok_or_else(|| Error::IntakeConflict {
            code: "INTAKE_PROFILE_CONTEXT_MISSING",
            message: "the Lead request has no frozen execution profile context".into(),
        })?;
    let grant = load_delegation(connection, &intake.delegation_id).await?;
    if context.project_id != intake.project_id
        || context.profile_id != intake.profile_id
        || context.profile_revision_id != intake.profile_revision_id
        || context.profile_revision != intake.profile_revision
        || context.profile_hash != intake.profile_hash
        || context.project_config_hash != lead.project_config_hash
        || context.delegation_hash != grant.content_hash
    {
        return Err(Error::IntakeConflict {
            code: "INTAKE_PROFILE_CONTEXT_MISMATCH",
            message: "the frozen execution context does not match the intake snapshot".into(),
        });
    }
    if sha256_hex(context.context_json.as_bytes()) != context.context_hash {
        return Err(Error::Invalid("stored execution context digest mismatch"));
    }
    let profile = load_frozen_profile_content(connection, &context).await?;
    let now = Utc::now();
    if !allow_started {
        if grant.revoked_at.is_some() {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::DelegationRevoked.as_str(),
                message: "the execution policy delegation is revoked".into(),
            });
        }
        if grant.expires_at <= now {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::DelegationExpired.as_str(),
                message: "the execution policy delegation is expired".into(),
            });
        }
        grant.validate(now)?;
        if grant.project_id != intake.project_id {
            return Err(Error::Forbidden(
                "execution policy delegation belongs to another Project".into(),
            ));
        }
        let policy_grant = load_execution_policy_grant(connection, &grant.id).await?;
        if policy_grant.project_id != intake.project_id
            || policy_grant.profile_id != context.profile_id
            || policy_grant.profile_revision_id != context.profile_revision_id
            || policy_grant.profile_hash != context.profile_hash
            || policy_grant.project_config_hash != context.project_config_hash
            || !policy_grant.acknowledged
        {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::ExecutionPolicyRequired.as_str(),
                message: "the delegation has no matching explicit execution policy grant".into(),
            });
        }
        if policy_grant.principal_id != grant.principal_id {
            return Err(Error::Forbidden(
                "execution policy grant identity is inconsistent".into(),
            ));
        }
        if lead.state != crate::lead::LeadRequestState::Ready {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::PlanNotReady.as_str(),
                message: "the frozen plan is not ready for explicit approval".into(),
            });
        }
        if !delegation_matches_frozen_profile(&grant, &profile) {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::ExecutionPolicyScopeChanged.as_str(),
                message: "the delegation route or R0 scope no longer matches the frozen profile"
                    .into(),
            });
        }
    }
    let config = load_project_config(connection, &intake.project_id).await?;
    if !allow_started
        && (!config.active
            || config.config_hash != context.project_config_hash
            || proposal.base_revision != config.base_commit)
    {
        return Err(Error::IntakeConflict {
            code: IntakeAuthorizationReason::ProjectConfigChanged.as_str(),
            message: "the Project configuration or base changed after the request".into(),
        });
    }
    if !allow_started
        && (grant.allowed_paths != config.allowed_paths
            || grant.capabilities != config.capabilities)
    {
        return Err(Error::IntakeConflict {
            code: IntakeAuthorizationReason::ExecutionPolicyScopeChanged.as_str(),
            message: "the Project scope changed after the execution policy grant".into(),
        });
    }
    let request_hash = sha256_hex(
        serde_json::to_string(&(
            &lead.project_id,
            &lead.action_id,
            &lead.delegation_id,
            &lead.objective,
            &lead.project_config_hash,
        ))
        .expect("lead request digest serializes")
        .as_bytes(),
    );
    if request_hash != lead.request_hash {
        return Err(Error::Invalid("Lead request hash mismatch"));
    }
    let recomputed_plan_hash = plan_hash(&proposal, &grant, &lead.project_config_hash);
    if recomputed_plan_hash != plan.plan_hash
        || plan.config_hash != lead.project_config_hash
        || plan.delegation_hash != grant.content_hash
    {
        return Err(Error::IntakeConflict {
            code: "PLAN_BINDING_STALE",
            message: "the frozen plan no longer matches its request, config or delegation".into(),
        });
    }
    if proposal.objective != lead.objective {
        return Err(Error::Forbidden(
            "proposal objective differs from the original request".into(),
        ));
    }
    if proposal.risk_class != RiskClass::R0 {
        return Err(Error::IntakeConflict {
            code: "AUTHORIZATION_UNAVAILABLE_RISK",
            message: "explicit operator start is limited to a deterministically eligible R0 plan"
                .into(),
        });
    }
    if !allow_started {
        let action = load_action_binding(connection, &lead.action_id).await?;
        if action.status != ActionStatus::Pending {
            return Err(Error::Forbidden("action is not startable".into()));
        }
        proposal.validate(&grant, &config.binding(), &action, now)?;
    }
    let (action_spec, spec_sha256) = load_action_spec(connection, &lead.action_id).await?;
    if action_spec.allowed_paths.is_empty()
        || action_spec
            .allowed_paths
            .iter()
            .any(|path| !crate::workspace::path_covered(path, &grant.allowed_paths))
        || action_spec.provider != profile.roles.implementer.provider
        || action_spec.model != profile.roles.implementer.model
    {
        return Err(Error::IntakeConflict {
            code: IntakeAuthorizationReason::ExecutionPolicyScopeChanged.as_str(),
            message: "the frozen ActionSpec no longer matches the grant or implementer route"
                .into(),
        });
    }
    let mechanism: (String,) =
        sqlx::query_as::query_as("SELECT mechanism FROM lead_assessments WHERE request_id=?1")
            .bind(&lead.id)
            .fetch_one(&mut *connection)
            .await?;
    if !matches!(mechanism.0.as_str(), "MODEL" | "FIXTURE") {
        return Err(Error::Invalid("stored lead assessment mechanism"));
    }
    if !allow_started && mechanism.0 == "MODEL" {
        let availability = load_provider_availability(connection).await?;
        let (executable, warnings) = evaluate_executability(&profile, &availability);
        if !executable {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::ExecutionProfileNotExecutable.as_str(),
                message: format!(
                    "the frozen execution profile is not executable: {}",
                    warnings.join("; ")
                ),
            });
        }
    }
    let usage = load_delegation_usage(connection, &grant.id).await?;
    let budget =
        intersect_campaign_budget(&profile, &proposal, &grant, &intake.mode).map_err(|error| {
            Error::IntakeConflict {
                code: error.reason(),
                message: "the frozen plan cannot be bounded for implementation".into(),
            }
        })?;
    if !allow_started
        && u64::from(usage.attempts) + u64::from(budget.max_attempts)
            > u64::from(grant.max_cumulative_attempts)
    {
        return Err(Error::IntakeConflict {
            code: IntakeAuthorizationReason::DelegationCapacityExhausted.as_str(),
            message: "the execution policy has no remaining attempt capacity".into(),
        });
    }
    Ok(IntakeApprovalContext {
        intake,
        lead,
        plan,
        proposal,
        context,
        profile,
        grant,
        usage,
        config,
        action_spec,
        spec_sha256,
        mechanism: mechanism.0,
        budget,
    })
}

fn delegation_matches_frozen_profile(
    grant: &StandingDelegation,
    profile: &ExecutionProfileContent,
) -> bool {
    grant.risk_classes == vec![RiskClass::R0]
        && !grant.autonomous_start
        && grant.autonomous_risk_classes.is_empty()
        && grant.route.provider == profile.roles.implementer.provider
        && grant.route.model == profile.roles.implementer.model
        && grant.route.mode == "build"
        && grant.max_attempts <= profile.budgets.max_attempts
        && grant.max_sessions <= profile.budgets.max_sessions
        && u64::from(grant.max_timeout_secs) <= profile.budgets.deadline_secs
        && u64::from(grant.max_wall_clock_secs) <= profile.budgets.deadline_secs
        && grant.max_parallel == 1
        && grant.max_repair_cycles == 0
}

pub(super) async fn load_frozen_profile_content(
    connection: &mut SqliteConnection,
    context: &LeadExecutionContextRecord,
) -> Result<ExecutionProfileContent, Error> {
    let row: (String, String, i64) = sqlx::query_as::query_as(
        "SELECT content_json,content_hash,revision FROM execution_profile_revisions WHERE revision_id=?1",
    )
    .bind(&context.profile_revision_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::IntakeConflict {
        code: "INTAKE_PROFILE_CONTEXT_MISSING",
        message: "the frozen profile revision no longer exists".into(),
    })?;
    if row.1 != context.profile_hash || u32::try_from(row.2).ok() != Some(context.profile_revision)
    {
        return Err(Error::IntakeConflict {
            code: "INTAKE_PROFILE_CONTEXT_MISMATCH",
            message: "the frozen profile revision hash or number changed".into(),
        });
    }
    let content: ExecutionProfileContent = serde_json::from_str(&row.0)
        .map_err(|_| Error::Invalid("stored execution profile content encoding"))?;
    content.validate()?;
    if content.compute_content_hash() != context.profile_hash {
        return Err(Error::IntakeConflict {
            code: "INTAKE_PROFILE_CONTEXT_MISMATCH",
            message: "the frozen profile content hash is inconsistent".into(),
        });
    }
    Ok(content)
}

async fn load_action_spec(
    connection: &mut SqliteConnection,
    action_id: &str,
) -> Result<(ActionSpec, String), Error> {
    let row: (String, String) = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
    )
    .bind(action_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| Error::IntakeConflict {
        code: IntakeAuthorizationReason::FrozenPlanRequired.as_str(),
        message: "the Action has no frozen executable spec".into(),
    })?;
    if sha256_hex(row.0.as_bytes()) != row.1 {
        return Err(Error::Invalid("frozen ActionSpec digest mismatch"));
    }
    let spec =
        serde_json::from_str(&row.0).map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
    Ok((spec, row.1))
}

async fn load_delegation_usage(
    connection: &mut SqliteConnection,
    delegation_id: &str,
) -> Result<DelegationUsage, Error> {
    let row: Option<(i64, i64, i64)> = sqlx::query_as::query_as(
        "SELECT requests,sessions,attempts FROM delegation_usage WHERE delegation_id=?1",
    )
    .bind(delegation_id)
    .fetch_optional(&mut *connection)
    .await?;
    let (requests, sessions, attempts) = row.ok_or(Error::NotFound)?;
    Ok(DelegationUsage {
        delegation_id: delegation_id.into(),
        requests: u32::try_from(requests)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
        sessions: u32::try_from(sessions)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
        attempts: u32::try_from(attempts)
            .map_err(|_| Error::Constraint("usage overflow".into()))?,
    })
}

fn validate_execution_policy_input(input: &NewExecutionPolicyGrant<'_>) -> Result<(), Error> {
    if !input.acknowledge {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_POLICY_ACKNOWLEDGEMENT_REQUIRED",
            message: "explicit acknowledgement is required to grant execution policy".into(),
        });
    }
    if input.project_id.trim().is_empty()
        || input.project_id.len() > 128
        || input.profile_id.trim().is_empty()
        || input.profile_id.len() > 128
        || input.expected_revision == 0
        || !is_sha256(input.expected_revision_hash)
        || !is_sha256(input.project_config_hash)
        || input.idempotency_key.trim().is_empty()
        || input.idempotency_key.len() > 256
        || input.principal_id.trim().is_empty()
        || input.principal_id.len() > 200
        || input
            .principal_label
            .is_some_and(|label| label.trim().is_empty() || label.len() > 200)
    {
        return Err(Error::Invalid("execution policy grant input"));
    }
    Ok(())
}

fn execution_policy_digest(input: &NewExecutionPolicyGrant<'_>) -> String {
    sha256_hex(
        serde_json::to_string(&(
            "execution-policy-v1",
            input.project_id,
            input.profile_id,
            input.expected_revision,
            input.expected_revision_hash,
            input.project_config_hash,
            input.acknowledge,
            input.principal_id,
        ))
        .expect("execution policy digest serializes")
        .as_bytes(),
    )
}

async fn load_intake_request(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<IntakeRequestRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
                profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
                submission_json,submission_hash,idempotency_key,created_at,updated_at
         FROM intake_requests WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    let mode: String = row.try_get("mode")?;
    let mode = match mode.as_str() {
        "REVIEW_PLAN" => IntakeMode::ReviewPlan,
        "AUTONOMOUS" => IntakeMode::Autonomous,
        _ => return Err(Error::Invalid("stored intake mode")),
    };
    let revision: i64 = row.try_get("profile_revision")?;
    Ok(IntakeRequestRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        objective: row.try_get("objective")?,
        mode,
        profile_id: row.try_get("profile_id")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_revision: u32::try_from(revision)
            .map_err(|_| Error::Conflict("stored intake profile revision is invalid"))?,
        profile_hash: row.try_get("profile_hash")?,
        profile_name: row.try_get("profile_name")?,
        action_id: row.try_get("action_id")?,
        delegation_id: row.try_get("delegation_id")?,
        lead_request_id: row.try_get("lead_request_id")?,
        campaign_id: row.try_get("campaign_id")?,
        submission_json: row.try_get("submission_json")?,
        submission_hash: row.try_get("submission_hash")?,
        idempotency_key: row.try_get("idempotency_key")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_intake_messages(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Vec<IntakeMessageRecord>, Error> {
    let rows = sqlx::query::query(
        "SELECT id,request_id,role,body,body_sha256,idempotency_key,created_at
         FROM intake_messages WHERE request_id=?1 ORDER BY created_at,id",
    )
    .bind(request_id)
    .fetch_all(&mut *connection)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(IntakeMessageRecord {
                id: row.try_get("id")?,
                request_id: row.try_get("request_id")?,
                role: row.try_get("role")?,
                body: row.try_get("body")?,
                body_sha256: row.try_get("body_sha256")?,
                idempotency_key: row.try_get("idempotency_key")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect()
}

async fn load_intake_message(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<IntakeMessageRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,role,body,body_sha256,idempotency_key,created_at
         FROM intake_messages WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(IntakeMessageRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        role: row.try_get("role")?,
        body: row.try_get("body")?,
        body_sha256: row.try_get("body_sha256")?,
        idempotency_key: row.try_get("idempotency_key")?,
        created_at: row.try_get("created_at")?,
    })
}

pub(super) async fn load_execution_context(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Option<LeadExecutionContextRecord>, Error> {
    let row = sqlx::query::query(
        "SELECT request_id,project_id,profile_id,profile_revision_id,profile_revision,
                profile_hash,profile_name,project_config_hash,delegation_hash,context_json,
                context_hash,created_at
         FROM lead_execution_context WHERE request_id=?1",
    )
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let revision: i64 = row.try_get("profile_revision")?;
    Ok(Some(LeadExecutionContextRecord {
        request_id: row.try_get("request_id")?,
        project_id: row.try_get("project_id")?,
        profile_id: row.try_get("profile_id")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_revision: u32::try_from(revision)
            .map_err(|_| Error::Conflict("stored execution context revision is invalid"))?,
        profile_hash: row.try_get("profile_hash")?,
        profile_name: row.try_get("profile_name")?,
        project_config_hash: row.try_get("project_config_hash")?,
        delegation_hash: row.try_get("delegation_hash")?,
        context_json: row.try_get("context_json")?,
        context_hash: row.try_get("context_hash")?,
        created_at: row.try_get("created_at")?,
    }))
}

pub(super) async fn load_execution_policy_grant(
    connection: &mut SqliteConnection,
    delegation_id: &str,
) -> Result<ExecutionPolicyGrantRecord, Error> {
    let row = sqlx::query::query(
        "SELECT delegation_id,project_id,principal_id,profile_id,profile_revision_id,
                profile_revision,profile_hash,project_config_hash,acknowledged,idempotency_key,
                created_at
         FROM execution_policy_grants WHERE delegation_id=?1",
    )
    .bind(delegation_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    let revision: i64 = row.try_get("profile_revision")?;
    let acknowledged: i64 = row.try_get("acknowledged")?;
    Ok(ExecutionPolicyGrantRecord {
        delegation_id: row.try_get("delegation_id")?,
        project_id: row.try_get("project_id")?,
        principal_id: row.try_get("principal_id")?,
        profile_id: row.try_get("profile_id")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_revision: u32::try_from(revision)
            .map_err(|_| Error::Conflict("stored grant revision is invalid"))?,
        profile_hash: row.try_get("profile_hash")?,
        project_config_hash: row.try_get("project_config_hash")?,
        acknowledged: acknowledged == 1,
        idempotency_key: row.try_get("idempotency_key")?,
        created_at: row.try_get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        AuthenticatedOperatorApproval, ExpectedStartBinding, NewProjectConfig,
        ProviderSettingsWrite, StartAuthority,
    };
    use crate::execution_profile::{
        AutoStartPolicy, CampaignBudgets, ExecutionProfileCategory, ExecutionProfileRoles,
        ExecutionRoleRoute, ReasoningEffort,
    };
    use crate::lead::{
        ActionBinding, AssessmentInput, Capability, CheckKind, LeadRequestState, QualityProfile,
        SpecCheck,
    };
    use std::path::PathBuf;

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()))
    }

    fn route() -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            reasoning: Some(ReasoningEffort::Medium),
            max_input_tokens: 64_000,
            max_output_tokens: 8_192,
            max_cost_micros: 300_000,
        }
    }

    fn profile_content(name: &str) -> ExecutionProfileContent {
        ExecutionProfileContent {
            name: name.into(),
            description: "test profile".into(),
            category: ExecutionProfileCategory::Balanced,
            roles: ExecutionProfileRoles {
                lead: route(),
                implementer: route(),
                semantic_reviewer: route(),
            },
            budgets: CampaignBudgets {
                max_attempts: 2,
                max_sessions: 3,
                max_turns: 4,
                max_input_tokens: 40_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                deadline_secs: 600,
            },
            auto_start: AutoStartPolicy {
                enabled: false,
                risk_classes: Vec::new(),
            },
        }
    }

    async fn configure_provider(db: &Database, provider: &str, kind: &str, model: &str) {
        db.save_provider_settings(
            &ProviderSettingsWrite {
                provider: provider.into(),
                kind: kind.into(),
                model: model.into(),
                base_url: "https://provider.example/v1".into(),
                auth_mode: "api_key".into(),
                credential_ciphertext: "ciphertext".into(),
                credential_nonce: "nonce".into(),
                credential_fingerprint: "fingerprint".into(),
                enabled: true,
                is_default: provider == "opencode-go",
                max_turns: 4,
                max_input_tokens: 100_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                input_micros_per_million: 1_000,
                output_micros_per_million: 2_000,
            },
            "test-operator",
        )
        .await
        .unwrap();
    }

    async fn fixture() -> (
        PathBuf,
        Database,
        crate::db::ExecutionProfileRecord,
        crate::lead::ProjectConfig,
    ) {
        let path = temp_path("factory-intake");
        let db = Database::open(&path).await.unwrap();
        let base = "b".repeat(40);
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture',?1,?2,'fixture')",
        )
        .bind(&base)
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        let mut transaction = db.pool.begin().await.unwrap();
        db.ensure_project_config(&mut transaction, "project")
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let config = db
            .update_project_config(
                "project",
                &NewProjectConfig {
                    expected_revision: 1,
                    allowed_paths: vec!["src".into()],
                    capabilities: vec![Capability::RepositoryEdit],
                    quality_profile: QualityProfile::Generic,
                    active: true,
                },
            )
            .await
            .unwrap();
        configure_provider(&db, "opencode-go", "opencode", "deepseek-v4.1-flash").await;
        configure_provider(&db, "codex", "codex", "gpt-6-luna").await;
        let profile = db
            .create_execution_profile(&profile_content("Balanced"), "owner")
            .await
            .unwrap();
        (path, db, profile, config)
    }

    async fn create_policy(
        db: &Database,
        profile: &crate::db::ExecutionProfileRecord,
        config: &crate::lead::ProjectConfig,
    ) -> ExecutionPolicyGrantResult {
        db.create_execution_policy_grant(NewExecutionPolicyGrant {
            project_id: "project",
            profile_id: &profile.id,
            expected_revision: profile.revision,
            expected_revision_hash: &profile.content_hash,
            project_config_hash: &config.config_hash,
            acknowledge: true,
            idempotency_key: "policy-key",
            principal_id: "operator-token",
            principal_label: None,
        })
        .await
        .unwrap()
    }

    async fn create_request(
        db: &Database,
        profile: &crate::db::ExecutionProfileRecord,
        key: &str,
    ) -> IntakeRequestRecord {
        db.create_intake_request(
            "project",
            "Implement a bounded intake endpoint.",
            Some(&profile.id),
            IntakeMode::ReviewPlan,
            key,
            "operator-token",
        )
        .await
        .unwrap()
        .0
    }

    fn checks() -> Vec<SpecCheck> {
        vec![
            SpecCheck {
                id: "build".into(),
                kind: CheckKind::Build,
                description: "build".into(),
                command: vec!["cargo".into(), "build".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "test".into(),
                kind: CheckKind::Test,
                description: "test".into(),
                command: vec!["cargo".into(), "test".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "functional".into(),
                kind: CheckKind::Functional,
                description: "functional".into(),
                command: vec!["cargo".into(), "test".into(), "intake".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "negative".into(),
                kind: CheckKind::Negative,
                description: "negative".into(),
                command: vec!["cargo".into(), "test".into(), "--bad".into()],
                expect_exit: 1,
                timeout_secs: 60,
            },
        ]
    }

    async fn record_fixture_plan(
        db: &Database,
        request: &IntakeRequestRecord,
    ) -> IntakeApprovalContext {
        let lead = db.lead_request(&request.lead_request_id).await.unwrap();
        let config = db.project_config(&request.project_id).await.unwrap();
        let grant = db
            .standing_delegation(&request.delegation_id)
            .await
            .unwrap();
        db.begin_lead_assessment(&lead.id).await.unwrap();
        let binding = ActionBinding {
            id: lead.action_id.clone(),
            project_id: lead.project_id.clone(),
            status: ActionStatus::Pending,
        };
        let spec = ActionSpec {
            schema_version: 1,
            project_id: request.project_id.clone(),
            repository: config.repository.clone(),
            base_commit: config.base_commit.clone(),
            objective: lead.objective.clone(),
            objective_sha256: sha256_hex(lead.objective.as_bytes()),
            provider: grant.route.provider.clone(),
            model: grant.route.model.clone(),
            allowed_paths: config.allowed_paths.clone(),
            checks: checks(),
        };
        let (assessment, proposal) = crate::lead::fixture_lead_assessment(
            &lead.id,
            &config,
            &binding,
            &spec,
            &lead.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        let input = AssessmentInput {
            project_id: request.project_id.clone(),
            action_id: lead.action_id.clone(),
            request_id: lead.id.clone(),
            objective: lead.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from(["repository-files".into()]),
        };
        db.record_lead_assessment(
            &lead.id,
            "FIXTURE",
            &assessment,
            &input,
            "fixture-assessment",
        )
        .await
        .unwrap();
        db.record_lead_proposal(&lead.id, &proposal, "fixture-proposal")
            .await
            .unwrap();
        db.freeze_lead_plan_to_ready(&lead.id, "fixture-freeze")
            .await
            .unwrap();
        db.intake_approval_context(&request.id).await.unwrap()
    }

    fn authorization(context: &IntakeApprovalContext) -> NewCampaignAuthorization {
        let budget = intersect_campaign_budget(
            &context.profile,
            &context.proposal,
            &context.grant,
            &context.intake.mode,
        )
        .unwrap();
        NewCampaignAuthorization {
            authorization_id: format!("intake-{}", context.intake.id),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "operator-token".into(),
            rationale: "reviewed".into(),
            authorized_paths: context.action_spec.allowed_paths.clone(),
            max_attempts: budget.max_attempts,
            provider: context.profile.roles.implementer.provider.clone(),
            model: context.profile.roles.implementer.model.clone(),
            provider_mode: "build".into(),
            delegation_id: context.grant.id.clone(),
            max_sessions: budget.max_sessions,
            max_turns: budget.max_turns,
            max_input_tokens: budget.max_input_tokens,
            max_output_tokens: budget.max_output_tokens,
            max_cost_micros: budget.max_cost_micros,
            deadline_secs: budget.deadline_secs,
            allowed_tools: Vec::new(),
        }
    }

    fn binding(context: &IntakeApprovalContext) -> ExpectedStartBinding {
        ExpectedStartBinding::new(
            &context.lead.request_hash,
            &context.plan.plan_hash,
            &context.spec_sha256,
            &context.lead.project_config_hash,
            &context.grant.content_hash,
            Some(&context.context.profile_revision_id),
            Some(&context.context.profile_hash),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn intake_creation_requires_an_explicit_policy_and_replays() {
        let (path, db, profile, config) = fixture().await;
        let missing = db
            .create_intake_request(
                "project",
                "Implement a bounded intake endpoint.",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "request-no-policy",
                "operator-token",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            missing,
            Error::IntakeConflict {
                code: "EXECUTION_POLICY_REQUIRED",
                ..
            }
        ));

        let policy = create_policy(&db, &profile, &config).await;
        assert!(policy.created);
        assert!(!policy.delegation.autonomous_start);
        assert_eq!(policy.delegation.risk_classes, vec![RiskClass::R0]);
        assert_eq!(policy.delegation.route.provider, "opencode-go");
        assert_eq!(policy.delegation.max_requests, 4);
        assert!(policy.delegation.expires_at > Utc::now());

        let (request, created) = db
            .create_intake_request(
                "project",
                "Implement a bounded intake endpoint.",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "request-key",
                "operator-token",
            )
            .await
            .unwrap();
        assert!(created);
        assert_eq!(request.profile_revision_id, profile.revision_id);
        assert_eq!(request.profile_hash, profile.content_hash);
        assert_eq!(request.delegation_id, policy.delegation.id);
        assert_eq!(request.campaign_id, None);

        let draft = db.action_draft(&request.action_id).await.unwrap();
        assert!(sha256_hex(draft.assessment_input_json.as_bytes()) == draft.input_sha256);
        let (spec_rows,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM action_specs WHERE action_id=?1")
                .bind(&request.action_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(spec_rows, 0);
        let context = db
            .lead_execution_context(&request.lead_request_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(context.profile_revision_id, profile.revision_id);
        assert_eq!(context.project_config_hash, config.config_hash);
        assert_eq!(
            sha256_hex(context.context_json.as_bytes()),
            context.context_hash
        );

        let (replayed, created) = db
            .create_intake_request(
                "project",
                "Implement a bounded intake endpoint.",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "request-key",
                "operator-token",
            )
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(replayed.id, request.id);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn policy_grant_is_acknowledged_non_autonomous_and_revocation_fences_creation() {
        let (path, db, profile, config) = fixture().await;
        let error = db
            .create_execution_policy_grant(NewExecutionPolicyGrant {
                project_id: "project",
                profile_id: &profile.id,
                expected_revision: profile.revision,
                expected_revision_hash: &profile.content_hash,
                project_config_hash: &config.config_hash,
                acknowledge: false,
                idempotency_key: "no-ack",
                principal_id: "operator-token",
                principal_label: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "EXECUTION_POLICY_ACKNOWLEDGEMENT_REQUIRED",
                ..
            }
        ));

        let first = create_policy(&db, &profile, &config).await;
        let replay = db
            .create_execution_policy_grant(NewExecutionPolicyGrant {
                project_id: "project",
                profile_id: &profile.id,
                expected_revision: profile.revision,
                expected_revision_hash: &profile.content_hash,
                project_config_hash: &config.config_hash,
                acknowledge: true,
                idempotency_key: "policy-key",
                principal_id: "operator-token",
                principal_label: None,
            })
            .await
            .unwrap();
        assert!(!replay.created);
        assert_eq!(replay.delegation.id, first.delegation.id);

        db.revoke_standing_delegation(&first.delegation.id, Utc::now())
            .await
            .unwrap();
        let error = db
            .create_intake_request(
                "project",
                "Implement a bounded intake endpoint.",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "after-revoke",
                "operator-token",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "EXECUTION_POLICY_REQUIRED",
                ..
            }
        ));

        let second = db
            .create_execution_policy_grant(NewExecutionPolicyGrant {
                project_id: "project",
                profile_id: &profile.id,
                expected_revision: profile.revision,
                expected_revision_hash: &profile.content_hash,
                project_config_hash: &config.config_hash,
                acknowledge: true,
                idempotency_key: "policy-key-2",
                principal_id: "operator-token",
                principal_label: None,
            })
            .await
            .unwrap();
        assert!(second.created);
        assert_ne!(second.delegation.id, first.delegation.id);
        assert!(!second.delegation.autonomous_start);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn profile_revision_change_requires_a_new_explicit_grant() {
        let (path, db, profile, config) = fixture().await;
        let first = create_policy(&db, &profile, &config).await;
        let updated = db
            .update_execution_profile(
                &profile.id,
                profile.revision,
                &profile_content("v2"),
                "owner",
            )
            .await
            .unwrap();
        assert_ne!(updated.revision_id, first.grant.profile_revision_id);
        let error = db
            .create_intake_request(
                "project",
                "Implement a bounded intake endpoint.",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "stale-profile",
                "operator-token",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "EXECUTION_POLICY_REQUIRED",
                ..
            }
        ));
        let second = db
            .create_execution_policy_grant(NewExecutionPolicyGrant {
                project_id: "project",
                profile_id: &profile.id,
                expected_revision: updated.revision,
                expected_revision_hash: &updated.content_hash,
                project_config_hash: &config.config_hash,
                acknowledge: true,
                idempotency_key: "policy-key-v2",
                principal_id: "operator-token",
                principal_label: None,
            })
            .await
            .unwrap();
        assert!(second.created);
        assert_eq!(second.grant.profile_revision_id, updated.revision_id);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn frozen_plan_binds_profile_revision_and_exact_approval_replays() {
        let (path, db, profile, config) = fixture().await;
        let _policy = create_policy(&db, &profile, &config).await;
        let request = create_request(&db, &profile, "request-approve").await;
        let context = record_fixture_plan(&db, &request).await;
        assert_eq!(context.intake.profile_revision_id, profile.revision_id);
        assert_eq!(context.context.profile_hash, profile.content_hash);
        assert_eq!(context.plan.config_hash, config.config_hash);
        assert!(!context.action_spec.checks.is_empty());

        let wrong_plan = ExpectedStartBinding::new(
            &context.lead.request_hash,
            &"0".repeat(64),
            &context.spec_sha256,
            &context.lead.project_config_hash,
            &context.grant.content_hash,
            Some(&context.context.profile_revision_id),
            Some(&context.context.profile_hash),
        )
        .unwrap();
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "approve-wrong-plan",
                StartAuthority::Operator(
                    AuthenticatedOperatorApproval::new("operator-token", None, "wrong").unwrap(),
                ),
                Some(&wrong_plan),
                Some(&authorization(&context)),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Conflict(_)), "{error:?}");

        let wrong_profile = ExpectedStartBinding::new(
            &context.lead.request_hash,
            &context.plan.plan_hash,
            &context.spec_sha256,
            &context.lead.project_config_hash,
            &context.grant.content_hash,
            Some(&context.context.profile_revision_id),
            Some(&"0".repeat(64)),
        )
        .unwrap();
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "approve-wrong-profile",
                StartAuthority::Operator(
                    AuthenticatedOperatorApproval::new("operator-token", None, "wrong").unwrap(),
                ),
                Some(&wrong_profile),
                Some(&authorization(&context)),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Conflict(_)), "{error:?}");

        let approval = AuthenticatedOperatorApproval::new(
            "operator-token",
            None,
            "Reviewed the frozen scope.",
        )
        .unwrap();
        let result = db
            .start_lead_campaign(
                &context.lead.id,
                "approve-key",
                StartAuthority::Operator(approval.clone()),
                Some(&binding(&context)),
                Some(&authorization(&context)),
            )
            .await
            .unwrap();
        assert!(!result.replayed);
        let evidence = db
            .lead_start_evidence(&context.lead.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            evidence.profile_revision_id.as_deref(),
            Some(profile.revision_id.as_str())
        );
        assert_eq!(
            evidence.profile_hash.as_deref(),
            Some(profile.content_hash.as_str())
        );
        let stored = db.intake_request(&request.id).await.unwrap();
        assert_eq!(
            stored.campaign_id.as_deref(),
            Some(result.campaign.id.as_str())
        );

        let replay = db
            .start_lead_campaign(
                &context.lead.id,
                "approve-key",
                StartAuthority::Operator(approval),
                Some(&binding(&context)),
                Some(&authorization(&context)),
            )
            .await
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.campaign.id, result.campaign.id);

        let second_key = db
            .start_lead_campaign(
                &context.lead.id,
                "approve-second-key",
                StartAuthority::Operator(
                    AuthenticatedOperatorApproval::new("operator-token", None, "second").unwrap(),
                ),
                Some(&binding(&context)),
                Some(&authorization(&context)),
            )
            .await
            .unwrap_err();
        assert!(matches!(second_key, Error::AlreadyStarted { .. }));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn approval_budget_and_start_enforce_implementer_route_caps() {
        let (path, db, _base_profile, config) = fixture().await;
        let mut route_capped = profile_content("Route capped");
        route_capped.roles.implementer.max_input_tokens = 10_000;
        route_capped.roles.implementer.max_output_tokens = 1_000;
        route_capped.roles.implementer.max_cost_micros = 100_000;
        let profile = db
            .create_execution_profile(&route_capped, "owner")
            .await
            .unwrap();
        db.create_execution_policy_grant(NewExecutionPolicyGrant {
            project_id: "project",
            profile_id: &profile.id,
            expected_revision: profile.revision,
            expected_revision_hash: &profile.content_hash,
            project_config_hash: &config.config_hash,
            acknowledge: true,
            idempotency_key: "policy-route-capped",
            principal_id: "operator-token",
            principal_label: None,
        })
        .await
        .unwrap();
        let request = create_request(&db, &profile, "request-route-capped").await;
        let context = record_fixture_plan(&db, &request).await;
        assert_eq!(context.budget.max_input_tokens, 10_000);
        assert_eq!(context.budget.max_output_tokens, 1_000);
        assert_eq!(context.budget.max_cost_micros, 100_000);

        let approval = |rationale: &'static str| {
            AuthenticatedOperatorApproval::new("operator-token", None, rationale).unwrap()
        };
        let mut inflated_input = authorization(&context);
        inflated_input.max_input_tokens = 10_001;
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "route-input",
                StartAuthority::Operator(approval("input")),
                Some(&binding(&context)),
                Some(&inflated_input),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");

        let mut inflated_output = authorization(&context);
        inflated_output.max_output_tokens = 1_001;
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "route-output",
                StartAuthority::Operator(approval("output")),
                Some(&binding(&context)),
                Some(&inflated_output),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");

        let mut inflated_cost = authorization(&context);
        inflated_cost.max_cost_micros = 100_001;
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "route-cost",
                StartAuthority::Operator(approval("cost")),
                Some(&binding(&context)),
                Some(&inflated_cost),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");

        let result = db
            .start_lead_campaign(
                &context.lead.id,
                "route-ok",
                StartAuthority::Operator(approval("ok")),
                Some(&binding(&context)),
                Some(&authorization(&context)),
            )
            .await
            .unwrap();
        assert!(!result.replayed);
        let evidence = db
            .lead_start_evidence(&context.lead.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.campaign_id, result.campaign.id);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn withdrawal_fences_message_assessment_and_start() {
        let (path, db, profile, config) = fixture().await;
        let _policy = create_policy(&db, &profile, &config).await;
        let request = create_request(&db, &profile, "request-withdraw").await;
        let (message, created) = db
            .append_intake_message(&request.id, "Please keep the API stable.", "message-1")
            .await
            .unwrap();
        assert!(created);
        let (replay, created) = db
            .append_intake_message(&request.id, "Please keep the API stable.", "message-1")
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(replay.id, message.id);
        let error = db
            .append_intake_message(&request.id, "Different body.", "message-1")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Conflict(_)));

        db.withdraw_lead_request(&request.lead_request_id, "operator-token", None, "stop")
            .await
            .unwrap();
        let error = db
            .append_intake_message(&request.id, "late", "message-2")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "INTAKE_WITHDRAWN",
                ..
            }
        ));
        let error = db
            .begin_lead_assessment(&request.lead_request_id)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Withdrawn));
        let error = db
            .freeze_lead_plan_to_ready(&request.lead_request_id, "withdraw-freeze")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Withdrawn));
        let error = db
            .start_lead_campaign(
                &request.lead_request_id,
                "withdraw-start",
                StartAuthority::Operator(
                    AuthenticatedOperatorApproval::new("operator-token", None, "stop").unwrap(),
                ),
                Some(
                    &ExpectedStartBinding::new(
                        &"a".repeat(64),
                        &"b".repeat(64),
                        &"c".repeat(64),
                        &"d".repeat(64),
                        &"e".repeat(64),
                        Some("revision"),
                        Some(&"f".repeat(64)),
                    )
                    .unwrap(),
                ),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Withdrawn));
        let state = db
            .intake_authorization_state(&request.id, crate::intake::IntakeState::Cancelled)
            .await;
        assert_eq!(state.reason.as_deref(), Some("INTAKE_WITHDRAWN"));
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn message_after_freeze_is_refused_and_authorization_is_truthful() {
        let (path, db, profile, config) = fixture().await;
        let _policy = create_policy(&db, &profile, &config).await;
        let request = create_request(&db, &profile, "request-freeze-message").await;
        db.append_intake_message(&request.id, "before freeze", "message-before")
            .await
            .unwrap();
        let _context = record_fixture_plan(&db, &request).await;
        let error = db
            .append_intake_message(&request.id, "amendment", "message-after")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "INTAKE_PLAN_FROZEN",
                ..
            }
        ));
        let messages = db.intake_messages(&request.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "before freeze");
        let records = db.intake_requests().await.unwrap();
        assert_eq!(records.len(), 1);
        let approval = db
            .intake_authorization_state(&request.id, crate::intake::IntakeState::AwaitingApproval)
            .await;
        assert!(approval.available);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn freeze_is_rejected_on_stale_config() {
        let (path, db, profile, config) = fixture().await;
        let _policy = create_policy(&db, &profile, &config).await;
        let request = create_request(&db, &profile, "request-stale-config").await;
        let lead = db.lead_request(&request.lead_request_id).await.unwrap();
        let grant = db
            .standing_delegation(&request.delegation_id)
            .await
            .unwrap();
        db.begin_lead_assessment(&lead.id).await.unwrap();
        let binding = ActionBinding {
            id: lead.action_id.clone(),
            project_id: lead.project_id.clone(),
            status: ActionStatus::Pending,
        };
        let spec = ActionSpec {
            schema_version: 1,
            project_id: request.project_id.clone(),
            repository: config.repository.clone(),
            base_commit: config.base_commit.clone(),
            objective: lead.objective.clone(),
            objective_sha256: sha256_hex(lead.objective.as_bytes()),
            provider: grant.route.provider.clone(),
            model: grant.route.model.clone(),
            allowed_paths: config.allowed_paths.clone(),
            checks: checks(),
        };
        let (assessment, proposal) = crate::lead::fixture_lead_assessment(
            &lead.id,
            &config,
            &binding,
            &spec,
            &lead.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        let input = AssessmentInput {
            project_id: request.project_id.clone(),
            action_id: lead.action_id.clone(),
            request_id: lead.id.clone(),
            objective: lead.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from(["repository-files".into()]),
        };
        db.record_lead_assessment(&lead.id, "FIXTURE", &assessment, &input, "stale-a")
            .await
            .unwrap();
        db.record_lead_proposal(&lead.id, &proposal, "stale-p")
            .await
            .unwrap();
        db.update_project_config(
            "project",
            &NewProjectConfig {
                expected_revision: config.revision,
                allowed_paths: vec!["src".into()],
                capabilities: vec![Capability::RepositoryEdit],
                quality_profile: QualityProfile::Generic,
                active: false,
            },
        )
        .await
        .unwrap();
        let error = db
            .freeze_lead_plan_to_ready(&lead.id, "stale-freeze")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)));
        let reloaded = db.lead_request(&request.lead_request_id).await.unwrap();
        assert_eq!(reloaded.state, LeadRequestState::Planned);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn approval_context_rejects_revoked_delegation() {
        let (path, db, profile, config) = fixture().await;
        let _policy = create_policy(&db, &profile, &config).await;
        let request = create_request(&db, &profile, "request-context").await;
        let context = record_fixture_plan(&db, &request).await;
        db.revoke_standing_delegation(&context.grant.id, Utc::now())
            .await
            .unwrap();
        let error = db.intake_approval_context(&request.id).await.unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "DELEGATION_REVOKED",
                ..
            }
        ));
        let _ = std::fs::remove_file(path);
    }
}
