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
    IntakeAuthorizationReason, IntakeAuthorizationState, IntakeExecutionScope, IntakeMode,
    execution_policy_terms, validate_execution_scope,
};
use crate::lead::{
    ActionStatus, LeadPlanProposal, RiskClass, StandingDelegationV2, plan_hash_v2,
    project_lead_request_state_with_reason,
};

use super::delegations::{
    DelegationUsage, create_standing_delegation_v2_in_tx, load_delegation_v2,
};
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
    /// Set only on a follow-up request: the terminal request that started it.
    pub parent_request_id: Option<String>,
    /// Set only on a follow-up request: the exact operator message that started
    /// it. Together with `parent_request_id` this makes message replay resolve
    /// to the same child instead of creating another request.
    pub source_message_id: Option<String>,
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
    /// Present when this message started a follow-up request.
    pub follow_up_request_id: Option<String>,
}

/// Result of recording an operator message. `follow_up` is present only when
/// the message started a new request from a terminal parent.
#[derive(Debug, Clone)]
pub struct IntakeMessageOutcome {
    pub message: IntakeMessageRecord,
    pub follow_up: Option<IntakeRequestRecord>,
    pub created: bool,
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
    pub autonomous: bool,
    pub subscription_confirmed: bool,
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
    pub autonomous: bool,
    pub subscription_confirmed: bool,
    pub idempotency_key: &'a str,
    pub principal_id: &'a str,
    pub principal_label: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ExecutionPolicyGrantResult {
    pub grant: ExecutionPolicyGrantRecord,
    pub delegation: StandingDelegationV2,
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
    pub grant: StandingDelegationV2,
    pub usage: DelegationUsage,
    pub config: crate::lead::ProjectConfig,
    pub action_spec: ActionSpec,
    pub spec_sha256: String,
    pub mechanism: String,
    pub scope: IntakeExecutionScope,
}

pub(super) struct ResolvedIntakeProfile {
    pub(super) profile_id: String,
    pub(super) revision_id: String,
    pub(super) revision: u32,
    pub(super) content_hash: String,
    pub(super) content: ExecutionProfileContent,
}

pub(super) struct PreparedIntakeRequest {
    pub(super) submission_json: String,
    pub(super) submission_hash: String,
    pub(super) assessment_input_json: String,
    pub(super) lead_idempotency_key: String,
}

impl Database {
    /// Create one intake request, its draft Action, Lead request, execution
    /// context and idempotency record in a single transaction. No Campaign is
    /// created and no autonomy is enabled.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_intake_request(
        &self,
        project_id: &str,
        objective: &str,
        execution_profile_id: Option<&str>,
        mode: IntakeMode,
        idempotency_key: &str,
        principal_id: &str,
        selected_issues: &[crate::issues::NewIssueSelection],
    ) -> Result<(IntakeRequestRecord, bool), Error> {
        self.create_intake_request_with_environment(
            project_id,
            objective,
            execution_profile_id,
            mode,
            idempotency_key,
            principal_id,
            selected_issues,
            None,
        )
        .await
    }

    /// Create an intake request with explicit production environment
    /// constraints. The constraints are part of the immutable submission
    /// snapshot and are the supported input path for a generated environment.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_intake_request_with_environment(
        &self,
        project_id: &str,
        objective: &str,
        execution_profile_id: Option<&str>,
        mode: IntakeMode,
        idempotency_key: &str,
        principal_id: &str,
        selected_issues: &[crate::issues::NewIssueSelection],
        environment_constraints: Option<
            &crate::planning::environment::RequestEnvironmentConstraints,
        >,
    ) -> Result<(IntakeRequestRecord, bool), Error> {
        validate_intake_submission(
            project_id,
            objective,
            execution_profile_id,
            idempotency_key,
            principal_id,
        )?;
        let prepared = prepare_intake_request_with_environment(
            project_id,
            objective,
            execution_profile_id,
            mode,
            principal_id,
            environment_constraints,
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
            if !Database::intake_request_issue_links_match_in_tx(
                &mut transaction,
                &id,
                selected_issues,
            )
            .await?
            {
                return Err(Error::IntakeConflict {
                    code: "ISSUE_SELECTION_CHANGED",
                    message:
                        "the idempotency key was already used with a different issue selection"
                            .into(),
                });
            }
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
        let profile = resolve_intake_profile_in_tx(
            &mut transaction,
            &self.provider_model_cache(),
            project_id,
            execution_profile_id,
        )
        .await?;
        let grant = find_matching_execution_policy_grant_in_tx(
            &mut transaction,
            project_id,
            principal_id,
            &profile,
            &config,
            mode,
        )
        .await?;
        if mode == IntakeMode::Autonomous {
            let policy = load_execution_policy_grant(&mut transaction, &grant.id).await?;
            if !policy.autonomous
                || !policy.subscription_confirmed
                || !crate::intake::autonomous_subscription_profile(&profile.content)
            {
                return Err(Error::IntakeConflict {
                    code: "AUTONOMOUS_POLICY_REQUIRED",
                    message:
                        "autonomous intake requires the owner-granted autonomous R0 policy with subscription confirmation and OpenCode Go subscription routes for the Lead and implementer"
                            .into(),
                });
            }
        }
        let issue_links = Database::resolve_selected_issue_links_in_tx(
            &mut transaction,
            project_id,
            selected_issues,
        )
        .await?;
        let record = create_intake_request_in_tx(
            &mut transaction,
            project_id,
            objective,
            mode,
            idempotency_key,
            profile,
            &config,
            &grant,
            prepared,
            &issue_links,
            None,
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
            dependency_plan: None,
            dependencies_required: false,
            environment_plan_id: None,
            environment_plan_sha256: None,
            environment_spec_sha256: None,
            environment_builder_image: None,
            environment_builder_image_digest: None,
            environment_required: false,

            environment_spec_id: None,
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

    /// Every unfinished autonomous request, oldest first. Startup recovery
    /// must not inherit the newest-500 list cap: an older interrupted request
    /// would otherwise remain visibly stuck forever.
    pub async fn unfinished_autonomous_intake_requests(
        &self,
    ) -> Result<Vec<IntakeRequestRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT ir.id FROM intake_requests ir
             JOIN lead_requests lr ON lr.id=ir.lead_request_id
             LEFT JOIN lead_withdrawals lw ON lw.request_id=lr.id
             WHERE ir.mode='AUTONOMOUS' AND ir.campaign_id IS NULL
               AND lr.state NOT IN ('BLOCKED','FAILED')
               AND lw.request_id IS NULL
             ORDER BY ir.created_at ASC,ir.id ASC",
        )
        .fetch_all(&mut *connection)
        .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            records.push(load_intake_request(&mut connection, &id).await?);
        }
        Ok(records)
    }

    pub async fn intake_request_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<IntakeRequestRecord, Error> {
        let row: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE campaign_id=?1")
                .bind(campaign_id)
                .fetch_optional(&self.pool)
                .await?;
        let (id,) = row.ok_or(Error::NotFound)?;
        self.intake_request(&id).await
    }

    pub async fn intake_request_for_lead(
        &self,
        lead_request_id: &str,
    ) -> Result<IntakeRequestRecord, Error> {
        let row: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE lead_request_id=?1")
                .bind(lead_request_id)
                .fetch_optional(&self.pool)
                .await?;
        let (id,) = row.ok_or(Error::NotFound)?;
        self.intake_request(&id).await
    }

    /// The single follow-up request started by a terminal parent, when present.
    pub async fn intake_follow_up_for_parent(
        &self,
        parent_request_id: &str,
    ) -> Result<Option<IntakeRequestRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_follow_up_for_parent(&mut connection, parent_request_id).await
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
    ///
    /// This compatibility entry point never starts a follow-up. The authenticated
    /// HTTP message path uses [`Self::append_intake_message_or_start_follow_up`].
    pub async fn append_intake_message(
        &self,
        request_id: &str,
        body: &str,
        idempotency_key: &str,
    ) -> Result<(IntakeMessageRecord, bool), Error> {
        let outcome = self
            .append_intake_message_internal(request_id, body, idempotency_key, None, false)
            .await?;
        Ok((outcome.message, outcome.created))
    }

    /// Record one authenticated operator message. A non-terminal request keeps
    /// the pre-freeze message behaviour. A terminal request (`COMPLETED` or
    /// `CANCELLED`) records the message and creates exactly one follow-up
    /// request in the same Project and Lead thread.
    ///
    /// The follow-up is not an amendment: it has a new Action, Lead request,
    /// execution context and policy binding, and must go through assessment,
    /// plan/environment freeze and a new Campaign. It never reuses the parent
    /// frozen plan, ActionSpec, candidate or Campaign.
    pub async fn append_intake_message_or_start_follow_up(
        &self,
        request_id: &str,
        body: &str,
        idempotency_key: &str,
        principal_id: &str,
    ) -> Result<IntakeMessageOutcome, Error> {
        if principal_id.trim().is_empty() || principal_id.len() > 200 {
            return Err(Error::Invalid("intake follow-up principal"));
        }
        self.append_intake_message_internal(
            request_id,
            body,
            idempotency_key,
            Some(principal_id),
            true,
        )
        .await
    }

    async fn append_intake_message_internal(
        &self,
        request_id: &str,
        body: &str,
        idempotency_key: &str,
        principal_id: Option<&str>,
        allow_follow_up: bool,
    ) -> Result<IntakeMessageOutcome, Error> {
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
            let mut existing = load_intake_message(&mut transaction, &id).await?;
            if existing.request_id != request_id || existing.body != body {
                return Err(Error::Conflict(
                    "message idempotency key identifies another message",
                ));
            }
            let follow_up = load_follow_up_for_message(&mut transaction, &existing.id).await?;
            existing.follow_up_request_id = follow_up.as_ref().map(|record| record.id.clone());
            transaction.commit().await?;
            return Ok(IntakeMessageOutcome {
                message: existing,
                follow_up,
                created: false,
            });
        }
        let record = load_intake_request(&mut transaction, request_id).await?;
        let lead = load_lead_request(&mut transaction, &record.lead_request_id).await?;
        if withdrawal_in_tx(&mut transaction, &lead.id).await? {
            return Err(Error::IntakeConflict {
                code: IntakeAuthorizationReason::Withdrawn.as_str(),
                message: "the request was withdrawn; it cannot accept new messages".into(),
            });
        }
        let state = load_intake_state_in_tx(&mut transaction, &lead).await?;
        if allow_follow_up && state.is_terminal() {
            if let Some(existing) = load_follow_up_for_parent(&mut transaction, &record.id).await? {
                return Err(Error::IntakeConflict {
                    code: "INTAKE_FOLLOW_UP_EXISTS",
                    message: format!(
                        "follow-up request {} already exists for this thread; continue there",
                        existing.id
                    ),
                });
            }
            let message = IntakeMessageRecord {
                id: Uuid::new_v4().to_string(),
                request_id: record.id.clone(),
                role: "operator".into(),
                body: body.into(),
                body_sha256: body_sha256.clone(),
                idempotency_key: idempotency_key.into(),
                created_at: Utc::now().to_rfc3339(),
                follow_up_request_id: None,
            };
            insert_intake_message(&mut transaction, &message).await?;
            let follow_up = create_intake_follow_up_in_tx(
                &mut transaction,
                &self.provider_model_cache(),
                &record,
                &message,
                principal_id.ok_or(Error::Invalid("intake follow-up principal"))?,
            )
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
            record_event(
                &mut transaction,
                &record.id,
                "intake.follow_up_created",
                &format!("follow_up={} message={}", follow_up.id, message.id),
            )
            .await?;
            transaction.commit().await?;
            let mut message = message;
            message.follow_up_request_id = Some(follow_up.id.clone());
            return Ok(IntakeMessageOutcome {
                message,
                follow_up: Some(follow_up),
                created: true,
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
        let message = IntakeMessageRecord {
            id: Uuid::new_v4().to_string(),
            request_id: record.id.clone(),
            role: "operator".into(),
            body: body.into(),
            body_sha256: body_sha256.clone(),
            idempotency_key: idempotency_key.into(),
            created_at: Utc::now().to_rfc3339(),
            follow_up_request_id: None,
        };
        insert_intake_message(&mut transaction, &message).await?;
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
        Ok(IntakeMessageOutcome {
            message,
            follow_up: None,
            created: true,
        })
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
            let delegation = load_delegation_v2(&mut transaction, &delegation_id).await?;
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
        let Some(profile) = current.content.v2().cloned() else {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
                message:
                    "a legacy V1 execution profile is historical evidence and cannot authorize new work"
                        .into(),
            });
        };
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
        if current.content.content_hash() != profile_hash.0
            || profile_hash.0 != input.expected_revision_hash
        {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_STALE",
                message: "the acknowledged profile revision hash is stale".into(),
            });
        }
        let availability =
            load_provider_availability(&mut transaction, &self.provider_model_cache()).await?;
        let (executable, warnings) = evaluate_executability(&profile, &availability);
        if !executable {
            return Err(Error::IntakeConflict {
                code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
                message: format!(
                    "the execution profile is not executable: {}",
                    warnings.join("; ")
                ),
            });
        }
        if input.autonomous
            && !(profile.auto_start.enabled
                && profile.auto_start.risk_classes.contains(&RiskClass::R0))
        {
            return Err(Error::IntakeConflict {
                code: "AUTONOMOUS_PROFILE_REQUIRED",
                message:
                    "autonomous execution requires the frozen profile to enable auto-start for R0"
                        .into(),
            });
        }
        if input.autonomous && !crate::intake::autonomous_subscription_profile(&profile) {
            return Err(Error::IntakeConflict {
                code: "AUTONOMOUS_PROFILE_REQUIRED",
                message:
                    "autonomous execution currently requires OpenCode Go subscription routes for both the Lead and the implementer"
                        .into(),
            });
        }
        if input.autonomous && !input.subscription_confirmed {
            return Err(Error::IntakeConflict {
                code: "SUBSCRIPTION_CONFIRMATION_REQUIRED",
                message:
                    "autonomous execution requires the one-time subscription Extra Usage confirmation"
                        .into(),
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
        let terms = execution_policy_terms(&profile);
        let mut delegation = StandingDelegationV2 {
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
            autonomous_start: terms.autonomous_start && input.autonomous,
            autonomous_risk_classes: if terms.autonomous_start && input.autonomous {
                vec![RiskClass::R0]
            } else {
                Vec::new()
            },
            route: crate::lead::ProviderRoute {
                provider: profile.roles.implementer.provider.clone(),
                model: profile.roles.implementer.model.clone(),
                mode: "build".into(),
            },
            max_timeout_secs: terms.max_timeout_secs,
            max_parallel: terms.max_parallel,
            operations: terms.operations,
            progress: terms.progress,
            expires_at: Utc::now() + chrono::Duration::days(terms.validity_days),
            revoked_at: None,
        };
        delegation.content_hash = delegation.compute_content_hash();
        let delegation_key = format!(
            "execution-policy-delegation:{}",
            sha256_hex(input.idempotency_key.as_bytes())
        );
        let (delegation, _) =
            create_standing_delegation_v2_in_tx(&mut transaction, &delegation, &delegation_key)
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
            autonomous: input.autonomous,
            subscription_confirmed: input.subscription_confirmed,
            idempotency_key: input.idempotency_key.into(),
            created_at: now.clone(),
        };
        sqlx::query::query(
            "INSERT INTO execution_policy_grants
             (delegation_id,project_id,principal_id,profile_id,profile_revision_id,
              profile_revision,profile_hash,project_config_hash,acknowledged,idempotency_key,created_at,
              schema_version,autonomous,subscription_confirmed)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10,2,?11,?12)",
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
        .bind(i64::from(input.autonomous))
        .bind(i64::from(input.subscription_confirmed))
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
                "project={} delegation={} config={} acknowledged=true autonomous={} subscription_confirmed={}",
                grant.project_id,
                grant.delegation_id,
                grant.project_config_hash,
                grant.autonomous,
                grant.subscription_confirmed
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
                "project={} profile={} revision={} actor={} autonomous={} subscription_confirmed={}",
                grant.project_id,
                grant.profile_id,
                grant.profile_revision,
                actor,
                grant.autonomous,
                grant.subscription_confirmed
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
            "SELECT gd.outcome FROM gate_decisions_current gd
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
        load_intake_approval_context(
            &mut connection,
            &self.provider_model_cache(),
            intake_id,
            false,
        )
        .await
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
        load_intake_approval_context(
            &mut connection,
            &self.provider_model_cache(),
            intake_id,
            true,
        )
        .await
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
            IntakeState::Blocked | IntakeState::Failed | IntakeState::Assessed => {
                IntakeAuthorizationState::not_required()
            }
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

async fn insert_intake_message(
    connection: &mut SqliteConnection,
    message: &IntakeMessageRecord,
) -> Result<(), Error> {
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
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn load_follow_up_for_message(
    connection: &mut SqliteConnection,
    message_id: &str,
) -> Result<Option<IntakeRequestRecord>, Error> {
    let row: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE source_message_id=?1")
            .bind(message_id)
            .fetch_optional(&mut *connection)
            .await?;
    match row {
        Some((id,)) => Ok(Some(load_intake_request(connection, &id).await?)),
        None => Ok(None),
    }
}

async fn load_follow_up_for_parent(
    connection: &mut SqliteConnection,
    parent_request_id: &str,
) -> Result<Option<IntakeRequestRecord>, Error> {
    let row: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE parent_request_id=?1")
            .bind(parent_request_id)
            .fetch_optional(&mut *connection)
            .await?;
    match row {
        Some((id,)) => Ok(Some(load_intake_request(connection, &id).await?)),
        None => Ok(None),
    }
}

/// Derive the authoritative intake state inside the message transaction. The
/// same precedence as the Office projection is used: Campaign/Gate outcome,
/// then withdrawal, then the Lead request's own progress.
async fn load_intake_state_in_tx(
    connection: &mut SqliteConnection,
    lead: &super::lead::LeadRequestRecord,
) -> Result<crate::intake::IntakeState, Error> {
    let campaign_status = match &lead.campaign_id {
        Some(campaign_id) => {
            let row: Option<(String,)> =
                sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                    .bind(campaign_id)
                    .fetch_optional(&mut *connection)
                    .await?;
            row.map(|(status,)| status)
        }
        None => None,
    };
    let gate_outcome = match &lead.campaign_id {
        Some(campaign_id) => {
            let row: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT gd.outcome FROM gate_decisions_current gd
                 JOIN candidates c ON c.id=gd.candidate_id
                 JOIN attempts a ON a.id=c.attempt_id
                 JOIN runs r ON r.id=a.run_id
                 WHERE r.campaign_id=?1
                 ORDER BY gd.decided_at DESC LIMIT 1",
            )
            .bind(campaign_id)
            .fetch_optional(&mut *connection)
            .await?;
            row.map(|(outcome,)| outcome)
        }
        None => None,
    };
    let withdrawn = withdrawal_in_tx(connection, &lead.id).await?;
    let projection = project_lead_request_state_with_reason(
        lead.state,
        campaign_status.as_deref(),
        gate_outcome.as_deref(),
        withdrawn,
        lead.failure_reason.as_deref(),
    );
    Ok(crate::intake::IntakeState::from_lead_projection(projection))
}

/// Create the follow-up record inside the already-open message transaction. It
/// resolves the same exact profile revision and the same owner-confirmed
/// execution-policy path. A Core-owned base advance between parent and
/// follow-up may invalidate the stored project-config hash; in that narrow case
/// the exact parent grant is re-bound to the current config only when every
/// material scope value is unchanged, the grant is still valid, and the
/// authenticated principal is the original granting principal.
async fn create_intake_follow_up_in_tx(
    connection: &mut SqliteConnection,
    cache: &crate::provider_catalog::ProviderModelCache,
    parent: &IntakeRequestRecord,
    message: &IntakeMessageRecord,
    principal_id: &str,
) -> Result<IntakeRequestRecord, Error> {
    let config = load_project_config(connection, &parent.project_id).await?;
    if !config.active {
        return Err(Error::IntakeConflict {
            code: "PROJECT_INACTIVE",
            message: "the Project configuration is inactive".into(),
        });
    }
    let profile = load_exact_intake_profile_in_tx(
        connection,
        cache,
        &parent.profile_id,
        &parent.profile_revision_id,
        parent.profile_revision,
        &parent.profile_hash,
    )
    .await?;
    let grant = resolve_follow_up_policy_grant_in_tx(
        connection,
        parent,
        message,
        principal_id,
        &profile,
        &config,
    )
    .await?;
    let prepared = prepare_follow_up_intake_request(
        &parent.project_id,
        &message.body,
        &parent.profile_id,
        parent.mode,
        principal_id,
        &parent.id,
        &message.id,
    )?;
    let idempotency_key = format!("intake-follow-up:{}", message.id);
    create_intake_request_in_tx(
        connection,
        &parent.project_id,
        &message.body,
        parent.mode,
        &idempotency_key,
        profile,
        &config,
        &grant,
        prepared,
        &[],
        Some((&parent.id, &message.id)),
    )
    .await
}

fn follow_up_policy_required() -> Error {
    Error::IntakeConflict {
        code: "FOLLOW_UP_EXECUTION_POLICY_REQUIRED",
        message:
            "the parent owner-confirmed execution policy is no longer valid for this Project, profile and scope"
                .into(),
    }
}

async fn resolve_follow_up_policy_grant_in_tx(
    connection: &mut SqliteConnection,
    parent: &IntakeRequestRecord,
    message: &IntakeMessageRecord,
    principal_id: &str,
    profile: &ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
) -> Result<StandingDelegationV2, Error> {
    // The parent's exact owner-confirmed policy path is authoritative. A caller
    // with their own matching grant may not continue another principal's thread.
    let parent_grant = load_delegation_v2(connection, &parent.delegation_id).await?;
    let parent_policy = load_execution_policy_grant(connection, &parent_grant.id).await?;
    if parent_policy.principal_id != principal_id {
        return Err(Error::IntakeConflict {
            code: "FOLLOW_UP_POLICY_PRINCIPAL_MISMATCH",
            message:
                "only the principal that confirmed the parent execution policy may continue it"
                    .into(),
        });
    }
    if let Some(grant) = matching_execution_policy_grant_in_tx(
        connection,
        &parent.project_id,
        principal_id,
        profile,
        config,
        parent.mode,
    )
    .await?
    {
        return Ok(grant);
    }
    let now = Utc::now();
    if parent_grant.revoked_at.is_some()
        || parent_grant.expires_at <= now
        || parent_policy.project_id != parent.project_id
        || parent_policy.profile_id != profile.profile_id
        || parent_policy.profile_revision_id != profile.revision_id
        || parent_policy.profile_hash != profile.content_hash
        || !parent_policy.acknowledged
        || !delegation_matches_policy_scope(&parent_grant, profile, config, parent.mode)
    {
        return Err(follow_up_policy_required());
    }
    if parent.mode == IntakeMode::Autonomous
        && !(parent_policy.autonomous && parent_policy.subscription_confirmed)
    {
        return Err(follow_up_policy_required());
    }
    // Re-bind the exact parent grant terms to the current Core-owned config. This
    // is not a widening: the material scope, route, risk and operation limits are
    // copied unchanged and are validated above against the current config.
    let scope_key = format!(
        "follow-up-policy:{}:{}:{}",
        parent.id, config.revision, config.config_hash
    );
    let scope_hash = sha256_hex(scope_key.as_bytes());
    let mut delegation = parent_grant.clone();
    delegation.id = format!("follow-up-policy-{scope_hash}");
    delegation.audit_id = format!("follow-up-policy:{}", message.id);
    delegation.revision = 1;
    delegation.content_hash = String::new();
    delegation.content_hash = delegation.compute_content_hash();
    let delegation_key = format!("follow-up-delegation:{scope_hash}");
    let (delegation, _) =
        create_standing_delegation_v2_in_tx(connection, &delegation, &delegation_key).await?;
    let now = Utc::now().to_rfc3339();
    let grant = ExecutionPolicyGrantRecord {
        delegation_id: delegation.id.clone(),
        project_id: parent.project_id.clone(),
        principal_id: principal_id.into(),
        profile_id: profile.profile_id.clone(),
        profile_revision_id: profile.revision_id.clone(),
        profile_revision: profile.revision,
        profile_hash: profile.content_hash.clone(),
        project_config_hash: config.config_hash.clone(),
        acknowledged: true,
        autonomous: parent_policy.autonomous,
        subscription_confirmed: parent_policy.subscription_confirmed,
        idempotency_key: format!("follow-up-grant:{scope_hash}"),
        created_at: now.clone(),
    };
    sqlx::query::query(
        "INSERT INTO execution_policy_grants
         (delegation_id,project_id,principal_id,profile_id,profile_revision_id,
          profile_revision,profile_hash,project_config_hash,acknowledged,idempotency_key,created_at,
          schema_version,autonomous,subscription_confirmed)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10,2,?11,?12)",
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
    .bind(i64::from(grant.autonomous))
    .bind(i64::from(grant.subscription_confirmed))
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
        .bind(&grant.idempotency_key)
        .bind("create_execution_policy_grant")
        .bind(&scope_hash)
        .bind(&delegation.id)
        .execute(&mut *connection)
        .await?;
    insert_audit(
        connection,
        &grant.profile_id,
        Some(grant.profile_revision),
        "execution_policy_granted",
        principal_id,
        &format!(
            "project={} delegation={} config={} source=follow_up parent={} message={} acknowledged=true autonomous={} subscription_confirmed={}",
            grant.project_id,
            grant.delegation_id,
            grant.project_config_hash,
            parent.id,
            message.id,
            grant.autonomous,
            grant.subscription_confirmed
        ),
        &now,
    )
    .await?;
    record_event(
        connection,
        &delegation.id,
        "execution_policy.granted",
        &format!(
            "project={} profile={} revision={} actor={} source=follow_up parent={} autonomous={} subscription_confirmed={}",
            grant.project_id,
            grant.profile_id,
            grant.profile_revision,
            principal_id,
            parent.id,
            grant.autonomous,
            grant.subscription_confirmed
        ),
    )
    .await?;
    Ok(delegation)
}

/// Insert the ordinary intake request, draft Lead request, execution context
/// and idempotency record inside an existing transaction. Callers own replay
/// lookup and all authority resolution; this helper only persists a validated
/// snapshot. It never creates a Campaign or dispatches a provider call.
#[allow(clippy::too_many_arguments)]
pub(super) async fn create_intake_request_in_tx(
    transaction: &mut SqliteConnection,
    project_id: &str,
    objective: &str,
    mode: IntakeMode,
    idempotency_key: &str,
    profile: ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
    grant: &StandingDelegationV2,
    prepared: PreparedIntakeRequest,
    issue_links: &[crate::issues::NewIssueLinkSpec],
    lineage: Option<(&str, &str)>,
) -> Result<IntakeRequestRecord, Error> {
    let draft = NewDraftLeadRequest {
        project_id,
        delegation_id: &grant.id,
        objective,
        project_config_hash: &config.config_hash,
        assessment_input_json: &prepared.assessment_input_json,
        idempotency_key: &prepared.lead_idempotency_key,
    };
    let prepared_lead = prepare_draft_lead_request(&draft)?;
    let (lead, _) = create_draft_lead_request_in_tx(transaction, &draft, &prepared_lead).await?;
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
        parent_request_id: lineage.map(|(parent, _)| parent.to_owned()),
        source_message_id: lineage.map(|(_, message)| message.to_owned()),
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
          parent_request_id,source_message_id,submission_json,submission_hash,idempotency_key,
          created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?14,?15,?16,?17,?18,?18)",
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
    .bind(record.parent_request_id.as_deref())
    .bind(record.source_message_id.as_deref())
    .bind(&record.submission_json)
    .bind(&record.submission_hash)
    .bind(&record.idempotency_key)
    .bind(&record.created_at)
    .execute(&mut *transaction)
    .await?;
    Database::insert_issue_links_in_tx(transaction, project_id, &record.id, issue_links).await?;
    sqlx::query::query(
        "INSERT INTO agentic_operations
         (id,request_id,project_id,director_id,operation_manager_id,priority,status,
          worktree_policy,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,100,'QUEUED','PER_ATTEMPT',?6,?6)",
    )
    .bind(format!("operation-{}", record.id))
    .bind(&record.id)
    .bind(project_id)
    .bind(format!("director:{}", grant.principal_id))
    .bind(format!("operation-manager:{}", record.id))
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    if !matches!(mode, IntakeMode::Autonomous) {
        sqlx::query::query("UPDATE agentic_operations SET status='OBSERVED' WHERE request_id=?1")
            .bind(&record.id)
            .execute(&mut *transaction)
            .await?;
    }
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
        transaction,
        &record.id,
        "intake.request_created",
        &format!(
            "project={} profile={} revision={} lead={}",
            record.project_id, record.profile_id, record.profile_revision, record.lead_request_id
        ),
    )
    .await?;
    Ok(record)
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

pub(super) fn prepare_intake_request(
    project_id: &str,
    objective: &str,
    execution_profile_id: Option<&str>,
    mode: IntakeMode,
    principal_id: &str,
) -> Result<PreparedIntakeRequest, Error> {
    prepare_intake_request_with_environment(
        project_id,
        objective,
        execution_profile_id,
        mode,
        principal_id,
        None,
    )
}

pub(super) fn prepare_intake_request_with_environment(
    project_id: &str,
    objective: &str,
    execution_profile_id: Option<&str>,
    mode: IntakeMode,
    principal_id: &str,
    environment_constraints: Option<&crate::planning::environment::RequestEnvironmentConstraints>,
) -> Result<PreparedIntakeRequest, Error> {
    prepare_intake_request_inner(
        project_id,
        objective,
        execution_profile_id,
        mode,
        principal_id,
        environment_constraints,
        None,
    )
}

/// Prepare the immutable submission snapshot for a follow-up request. The
/// parent request and exact source message are part of the snapshot, so a
/// follow-up cannot be confused with an independent request for the same body.
fn prepare_follow_up_intake_request(
    project_id: &str,
    objective: &str,
    execution_profile_id: &str,
    mode: IntakeMode,
    principal_id: &str,
    parent_request_id: &str,
    source_message_id: &str,
) -> Result<PreparedIntakeRequest, Error> {
    prepare_intake_request_inner(
        project_id,
        objective,
        Some(execution_profile_id),
        mode,
        principal_id,
        None,
        Some((parent_request_id, source_message_id)),
    )
}

fn prepare_intake_request_inner(
    project_id: &str,
    objective: &str,
    execution_profile_id: Option<&str>,
    mode: IntakeMode,
    principal_id: &str,
    environment_constraints: Option<&crate::planning::environment::RequestEnvironmentConstraints>,
    lineage: Option<(&str, &str)>,
) -> Result<PreparedIntakeRequest, Error> {
    let (parent_request_id, source_message_id) = lineage
        .map(|(parent, message)| (Some(parent), Some(message)))
        .unwrap_or((None, None));
    let submission_json = serde_json::to_string(&serde_json::json!({
        "schema":"factory-intake-submission-v1",
        "projectId":project_id,
        "objective":objective,
        "mode":mode,
        "executionProfileId":execution_profile_id,
        "principalId":principal_id,
        "environmentConstraints":environment_constraints,
        "parentRequestId":parent_request_id,
        "sourceMessageId":source_message_id,
    }))
    .map_err(|_| Error::Invalid("intake submission encoding"))?;
    let submission_hash = sha256_hex(submission_json.as_bytes());
    let assessment_input_json = serde_json::to_string(&serde_json::json!({
        "schema":"factory-intake-assessment-context-v1",
        "projectId":project_id,
        "objective":objective,
        "requestedProfileId":execution_profile_id,
        "parentRequestId":parent_request_id,
        "sourceMessageId":source_message_id,
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
    cache: &crate::provider_catalog::ProviderModelCache,
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
    if current.content.content_hash() != content_hash.0 {
        return Err(Error::Conflict(
            "stored execution profile hash is inconsistent",
        ));
    }
    let Some(content) = current.content.v2().cloned() else {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
            message:
                "a legacy V1 execution profile is historical evidence and cannot authorize new work"
                    .into(),
        });
    };
    let availability = load_provider_availability(connection, cache).await?;
    let (executable, warnings) = evaluate_executability(&content, &availability);
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
        content,
    })
}

/// Load and validate one exact immutable profile revision. This is used by
/// issue-intake policy and batch acknowledgements: a later profile edit never
/// silently substitutes a newer revision. Executability is re-evaluated against
/// the current provider availability and fails closed.
pub(super) async fn load_exact_intake_profile_in_tx(
    connection: &mut SqliteConnection,
    cache: &crate::provider_catalog::ProviderModelCache,
    profile_id: &str,
    revision_id: &str,
    revision: u32,
    content_hash: &str,
) -> Result<ResolvedIntakeProfile, Error> {
    let row = sqlx::query::query(
        "SELECT content_json,content_hash,revision,schema_version FROM execution_profile_revisions
         WHERE revision_id=?1 AND profile_id=?2",
    )
    .bind(revision_id)
    .bind(profile_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::IntakeConflict {
        code: "EXECUTION_PROFILE_REQUIRED",
        message: "the acknowledged execution profile revision no longer exists".into(),
    })?;
    let stored_hash: String = row.try_get("content_hash")?;
    let stored_revision = u32::try_from(row.try_get::<i64, _>("revision")?)
        .map_err(|_| Error::Conflict("stored execution profile revision is invalid"))?;
    if stored_revision != revision || stored_hash != content_hash || !is_sha256(content_hash) {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_PROFILE_STALE",
            message: "the acknowledged execution profile revision hash is stale".into(),
        });
    }
    let schema_version = u32::try_from(row.try_get::<i64, _>("schema_version")?)
        .map_err(|_| Error::Invalid("stored execution profile schema version"))?;
    let decoded = super::execution_profiles::decode_profile_content(
        schema_version,
        &row.try_get::<String, _>("content_json")?,
    )?;
    if decoded.content_hash() != content_hash {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_PROFILE_STALE",
            message: "stored execution profile content does not match its hash".into(),
        });
    }
    let Some(content) = decoded.v2().cloned() else {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
            message:
                "a legacy V1 execution profile is historical evidence and cannot authorize new work"
                    .into(),
        });
    };
    let availability = load_provider_availability(connection, cache).await?;
    let (executable, warnings) = evaluate_executability(&content, &availability);
    if !executable {
        return Err(Error::IntakeConflict {
            code: "EXECUTION_PROFILE_NOT_EXECUTABLE",
            message: format!(
                "the acknowledged execution profile is not executable: {}",
                warnings.join("; ")
            ),
        });
    }
    Ok(ResolvedIntakeProfile {
        profile_id: profile_id.into(),
        revision_id: revision_id.into(),
        revision,
        content_hash: content_hash.into(),
        content,
    })
}

async fn matching_execution_policy_grant_in_tx(
    connection: &mut SqliteConnection,
    project_id: &str,
    principal_id: &str,
    profile: &ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
    mode: IntakeMode,
) -> Result<Option<StandingDelegationV2>, Error> {
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
        let grant = load_delegation_v2(connection, &delegation_id).await?;
        if grant.revoked_at.is_some() || grant.expires_at <= Utc::now() {
            continue;
        }
        if !delegation_matches_policy_scope(&grant, profile, config, mode) {
            continue;
        }
        // Cumulative usage is diagnosis telemetry, never an admission ceiling.
        let _usage = load_delegation_usage(connection, &grant.id).await?;
        return Ok(Some(grant));
    }
    Ok(None)
}

async fn find_matching_execution_policy_grant_in_tx(
    connection: &mut SqliteConnection,
    project_id: &str,
    principal_id: &str,
    profile: &ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
    mode: IntakeMode,
) -> Result<StandingDelegationV2, Error> {
    if let Some(grant) = matching_execution_policy_grant_in_tx(
        connection,
        project_id,
        principal_id,
        profile,
        config,
        mode,
    )
    .await?
    {
        return Ok(grant);
    }
    match mode {
        IntakeMode::ReviewPlan => Err(Error::IntakeConflict {
            code: IntakeAuthorizationReason::ExecutionPolicyRequired.as_str(),
            message:
                "no active non-autonomous execution policy matches this Project and exact profile revision; enable governed execution explicitly"
                    .into(),
        }),
        IntakeMode::Autonomous => Err(Error::IntakeConflict {
            code: "AUTONOMOUS_POLICY_REQUIRED",
            message:
                "no active autonomous R0 execution policy matches this Project and exact profile revision; grant the autonomous policy explicitly"
                    .into(),
        }),
    }
}

fn delegation_matches_policy_scope(
    grant: &StandingDelegationV2,
    profile: &ResolvedIntakeProfile,
    config: &crate::lead::ProjectConfig,
    mode: IntakeMode,
) -> bool {
    let autonomy_matches = match mode {
        IntakeMode::ReviewPlan => {
            !grant.autonomous_start && grant.autonomous_risk_classes.is_empty()
        }
        IntakeMode::Autonomous => {
            grant.autonomous_start
                && grant.autonomous_risk_classes.contains(&RiskClass::R0)
                && profile.content.auto_start.enabled
                && profile
                    .content
                    .auto_start
                    .risk_classes
                    .contains(&RiskClass::R0)
        }
    };
    grant.risk_classes == vec![RiskClass::R0]
        && autonomy_matches
        && grant.route.provider == profile.content.roles.implementer.provider
        && grant.route.model == profile.content.roles.implementer.model
        && grant.route.mode == "build"
        && grant.allowed_paths == config.allowed_paths
        && grant.capabilities == config.capabilities
        && grant.max_timeout_secs <= profile.content.operations.tool_call_timeout_secs.get()
        && grant.max_parallel == 1
        && grant.progress == profile.content.progress
}

async fn find_reusable_execution_policy_grant_in_tx(
    connection: &mut SqliteConnection,
    input: &NewExecutionPolicyGrant<'_>,
    revision_id: &str,
    profile_hash: &str,
) -> Result<Option<(StandingDelegationV2, ExecutionPolicyGrantRecord)>, Error> {
    let rows = sqlx::query::query(
        "SELECT delegation_id FROM execution_policy_grants
         WHERE project_id=?1 AND principal_id=?2 AND profile_id=?3
           AND profile_revision_id=?4 AND profile_hash=?5 AND project_config_hash=?6
           AND autonomous=?7 AND subscription_confirmed=?8
         ORDER BY created_at DESC,delegation_id DESC",
    )
    .bind(input.project_id)
    .bind(input.principal_id)
    .bind(input.profile_id)
    .bind(revision_id)
    .bind(profile_hash)
    .bind(input.project_config_hash)
    .bind(i64::from(input.autonomous))
    .bind(i64::from(input.subscription_confirmed))
    .fetch_all(&mut *connection)
    .await?;
    for row in rows {
        let delegation_id: String = row.try_get("delegation_id")?;
        let delegation = load_delegation_v2(connection, &delegation_id).await?;
        if delegation.revoked_at.is_some() || delegation.expires_at <= Utc::now() {
            continue;
        }
        // Historical usage counts are exposed for diagnosis only.
        let _usage = load_delegation_usage(connection, &delegation.id).await?;
        let grant = load_execution_policy_grant(connection, &delegation.id).await?;
        return Ok(Some((delegation, grant)));
    }
    Ok(None)
}

async fn load_intake_approval_context(
    connection: &mut SqliteConnection,
    cache: &crate::provider_catalog::ProviderModelCache,
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
    let grant = load_delegation_v2(connection, &intake.delegation_id).await?;
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
        if !delegation_matches_frozen_profile(&grant, &profile, intake.mode) {
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
    let recomputed_plan_hash = plan_hash_v2(&proposal, &grant, &lead.project_config_hash);
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
        proposal.validate_v2(&grant, &config.binding(), &action, now)?;
    }
    let (action_spec, spec_sha256) = load_action_spec(connection, &lead.action_id).await?;
    if action_spec.dependencies_required && action_spec.dependency_plan.is_none() {
        return Err(Error::IntakeConflict {
            code: "DEPENDENCY_PLAN_REQUIRED",
            message:
                "the frozen plan requires dependency acquisition but no execution dependency plan was attached"
                    .into(),
        });
    }
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
        let availability = load_provider_availability(connection, cache).await?;
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
    let scope =
        validate_execution_scope(&profile, &proposal, &grant, &intake.mode).map_err(|error| {
            Error::IntakeConflict {
                code: error.reason(),
                message: "the frozen plan cannot be bound to the execution scope".into(),
            }
        })?;
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
        scope,
    })
}

fn delegation_matches_frozen_profile(
    grant: &StandingDelegationV2,
    profile: &ExecutionProfileContent,
    mode: IntakeMode,
) -> bool {
    let autonomy_matches = match mode {
        IntakeMode::ReviewPlan => {
            !grant.autonomous_start && grant.autonomous_risk_classes.is_empty()
        }
        IntakeMode::Autonomous => {
            grant.autonomous_start
                && grant.autonomous_risk_classes.contains(&RiskClass::R0)
                && profile.auto_start.enabled
                && profile.auto_start.risk_classes.contains(&RiskClass::R0)
        }
    };
    grant.risk_classes == vec![RiskClass::R0]
        && autonomy_matches
        && grant.route.provider == profile.roles.implementer.provider
        && grant.route.model == profile.roles.implementer.model
        && grant.route.mode == "build"
        && grant.max_timeout_secs <= profile.operations.tool_call_timeout_secs.get()
        && grant.max_parallel == 1
        && grant.progress == profile.progress
}

pub(super) async fn load_frozen_profile_content(
    connection: &mut SqliteConnection,
    context: &LeadExecutionContextRecord,
) -> Result<ExecutionProfileContent, Error> {
    let row: (String, String, i64, i64) = sqlx::query_as::query_as(
        "SELECT content_json,content_hash,revision,schema_version FROM execution_profile_revisions WHERE revision_id=?1",
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
    let schema_version = u32::try_from(row.3)
        .map_err(|_| Error::Invalid("stored execution profile schema version"))?;
    if schema_version != 2 {
        return Err(Error::IntakeConflict {
            code: "INTAKE_PROFILE_CONTEXT_MISMATCH",
            message: "a legacy V1 profile cannot authorize a V2 Campaign".into(),
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
    if input.subscription_confirmed && !input.autonomous {
        return Err(Error::IntakeConflict {
            code: "SUBSCRIPTION_CONFIRMATION_NOT_APPLICABLE",
            message: "subscription confirmation requires autonomous R0 execution".into(),
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
            "execution-policy-v2",
            input.project_id,
            input.profile_id,
            input.expected_revision,
            input.expected_revision_hash,
            input.project_config_hash,
            input.acknowledge,
            input.principal_id,
            input.autonomous,
            input.subscription_confirmed,
        ))
        .expect("execution policy digest serializes")
        .as_bytes(),
    )
}

pub(super) async fn load_intake_request(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<IntakeRequestRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
                profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
                parent_request_id,source_message_id,submission_json,submission_hash,
                idempotency_key,created_at,updated_at
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
        parent_request_id: row.try_get("parent_request_id")?,
        source_message_id: row.try_get("source_message_id")?,
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
        "SELECT m.id,m.request_id,m.role,m.body,m.body_sha256,m.idempotency_key,m.created_at,
                (SELECT r.id FROM intake_requests r WHERE r.source_message_id=m.id) AS follow_up_request_id
         FROM intake_messages m WHERE m.request_id=?1 ORDER BY m.created_at,m.id",
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
                follow_up_request_id: row.try_get("follow_up_request_id")?,
            })
        })
        .collect()
}

async fn load_intake_message(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<IntakeMessageRecord, Error> {
    let row = sqlx::query::query(
        "SELECT m.id,m.request_id,m.role,m.body,m.body_sha256,m.idempotency_key,m.created_at,
                (SELECT r.id FROM intake_requests r WHERE r.source_message_id=m.id) AS follow_up_request_id
         FROM intake_messages m WHERE m.id=?1",
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
        follow_up_request_id: row.try_get("follow_up_request_id")?,
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
                created_at,autonomous,subscription_confirmed
         FROM execution_policy_grants WHERE delegation_id=?1",
    )
    .bind(delegation_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::NotFound)?;
    let revision: i64 = row.try_get("profile_revision")?;
    let acknowledged: i64 = row.try_get("acknowledged")?;
    let autonomous: i64 = row.try_get("autonomous")?;
    let subscription_confirmed: i64 = row.try_get("subscription_confirmed")?;
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
        autonomous: autonomous == 1,
        subscription_confirmed: subscription_confirmed == 1,
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
    use crate::execution_policy::{OperationLimits, ProgressPolicy};
    use crate::execution_profile::{
        AutoStartPolicy, ExecutionProfileCategory, ExecutionProfileRoles, ExecutionRoleRoute,
        ReasoningEffort,
    };
    use crate::lead::{
        ActionBinding, AssessmentInput, Capability, CheckKind, LeadRequestState, QualityProfile,
        SpecCheck,
    };
    use std::num::NonZeroU32;
    use std::path::PathBuf;

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()))
    }

    fn route() -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            reasoning: Some(ReasoningEffort::Medium),
        }
    }

    fn operations() -> OperationLimits {
        OperationLimits {
            model_request_timeout_secs: NonZeroU32::new(300).unwrap(),
            tool_call_timeout_secs: NonZeroU32::new(600).unwrap(),
            max_request_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_response_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_generation_tokens: NonZeroU32::new(8_192).unwrap(),
            max_in_flight_operations: NonZeroU32::new(4).unwrap(),
        }
    }

    fn progress() -> ProgressPolicy {
        ProgressPolicy {
            diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
            observation_timeout_secs: NonZeroU32::new(300).unwrap(),
            autonomy: Default::default(),
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
            operations: operations(),
            progress: progress(),
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
            "INSERT INTO projects(id,repository,base_commit,created_at,identity,default_branch)
             VALUES ('project','/fixture',?1,?2,'fixture','main')",
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
            autonomous: false,
            subscription_confirmed: false,
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
            &[],
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
                environment_plan_id: None,
                environment_spec_sha256: None,
            },
            SpecCheck {
                id: "test".into(),
                kind: CheckKind::Test,
                description: "test".into(),
                command: vec!["cargo".into(), "test".into()],
                expect_exit: 0,
                timeout_secs: 120,
                environment_plan_id: None,
                environment_spec_sha256: None,
            },
            SpecCheck {
                id: "functional".into(),
                kind: CheckKind::Functional,
                description: "functional".into(),
                command: vec!["cargo".into(), "test".into(), "intake".into()],
                expect_exit: 0,
                timeout_secs: 120,
                environment_plan_id: None,
                environment_spec_sha256: None,
            },
            SpecCheck {
                id: "negative".into(),
                kind: CheckKind::Negative,
                description: "negative".into(),
                command: vec!["cargo".into(), "test".into(), "--bad".into()],
                expect_exit: 1,
                timeout_secs: 60,
                environment_plan_id: None,
                environment_spec_sha256: None,
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
            .standing_delegation_v2(&request.delegation_id)
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
            dependency_plan: None,
            dependencies_required: false,
            environment_plan_id: None,
            environment_plan_sha256: None,
            environment_spec_sha256: None,
            environment_builder_image: None,
            environment_builder_image_digest: None,
            environment_required: false,

            environment_spec_id: None,
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
        NewCampaignAuthorization {
            authorization_id: format!("intake-{}", context.intake.id),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "operator-token".into(),
            rationale: "reviewed".into(),
            authorized_paths: context.action_spec.allowed_paths.clone(),
            provider: context.profile.roles.implementer.provider.clone(),
            model: context.profile.roles.implementer.model.clone(),
            provider_mode: "build".into(),
            delegation_id: context.grant.id.clone(),
            allowed_tools: Vec::new(),
            monetary_envelope_id: super::authorization::NO_BILLABLE_ENVELOPE.into(),
            progress: context.profile.progress,
            operations: context.profile.operations,
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
                &[],
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
        assert!(policy.delegation.operations.tool_call_timeout_secs.get() >= 1);
        assert_eq!(policy.delegation.progress, progress());
        assert!(policy.delegation.expires_at > Utc::now());

        let (request, created) = db
            .create_intake_request(
                "project",
                "Implement a bounded intake endpoint.",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "request-key",
                "operator-token",
                &[],
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
                &[],
            )
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(replayed.id, request.id);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn autonomous_policy_grants_r0_autonomy_and_accepts_autonomous_request() {
        let (path, db, profile, config) = fixture().await;
        let _ = profile;
        let mut content = profile_content("Autopilot");
        content.auto_start = AutoStartPolicy {
            enabled: true,
            risk_classes: vec![RiskClass::R0],
        };
        let profile = db
            .create_execution_profile(&content, "owner")
            .await
            .unwrap();
        let grant = db
            .create_execution_policy_grant(NewExecutionPolicyGrant {
                project_id: "project",
                profile_id: &profile.id,
                expected_revision: profile.revision,
                expected_revision_hash: &profile.content_hash,
                project_config_hash: &config.config_hash,
                acknowledge: true,
                autonomous: true,
                subscription_confirmed: true,
                idempotency_key: "autopilot-policy",
                principal_id: "operator-token",
                principal_label: None,
            })
            .await
            .unwrap();
        assert!(grant.grant.autonomous);
        assert!(grant.grant.subscription_confirmed);
        assert!(grant.delegation.autonomous_start);
        assert_eq!(
            grant.delegation.autonomous_risk_classes,
            vec![RiskClass::R0]
        );
        let (request, created) = db
            .create_intake_request(
                "project",
                "autonomous R0 request",
                Some(&profile.id),
                IntakeMode::Autonomous,
                "autopilot-request",
                "operator-token",
                &[],
            )
            .await
            .unwrap();
        assert!(created);
        assert_eq!(request.mode, IntakeMode::Autonomous);
        let operation = db.agentic_operation_for_request(&request.id).await.unwrap();
        assert_eq!(operation.priority, 100);
        assert_eq!(operation.status, "QUEUED");
        assert_eq!(operation.worktree_policy, "PER_ATTEMPT");
        let operation = db
            .set_agentic_operation_priority(&request.id, 10)
            .await
            .unwrap();
        assert_eq!(operation.priority, 10);
        let operation = db
            .set_agentic_operation_status(&request.id, "EXECUTING")
            .await
            .unwrap();
        assert_eq!(operation.status, "EXECUTING");
        let now = chrono::Utc::now().to_rfc3339();
        let action_id: (String,) =
            sqlx::query_as::query_as("SELECT action_id FROM intake_requests WHERE id=?1")
                .bind(&request.id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES ('operation-campaign',?1,'CANCELLED',?2)",
        )
        .bind(&action_id.0)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "UPDATE intake_requests SET campaign_id='operation-campaign' WHERE id=?1",
        )
        .bind(&request.id)
        .execute(&db.pool)
        .await
        .unwrap();
        let changed = db.reconcile_agentic_operations().await.unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].status, "CANCELLED");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn work_graph_and_spec_freeze_bind_exact_revisions() {
        let (path, db, profile, config) = fixture().await;
        let _ = config;
        let policy = create_policy(&db, &profile, &config).await;
        let _ = policy;
        let request = create_request(&db, &profile, "spec-request").await;
        let graph_model = crate::agentic::WorkGraphModel {
            units: vec![
                crate::agentic::PlannedWorkUnit {
                    id: "model".into(),
                    title: "Model".into(),
                    objective: "Implement the validator".into(),
                    depends_on: vec![],
                    paths: vec!["src/model.rs".into()],
                    acceptance: vec!["invalid input is rejected".into()],
                },
                crate::agentic::PlannedWorkUnit {
                    id: "api".into(),
                    title: "API".into(),
                    objective: "Expose the validator".into(),
                    depends_on: vec!["model".into()],
                    paths: vec!["src/api.rs".into()],
                    acceptance: vec!["the endpoint returns the result".into()],
                },
            ],
            assumptions: vec![],
            risks: vec![],
        };
        let graph = crate::agentic::validate_work_graph(&graph_model, &["src".into()]).unwrap();
        let saved = db
            .save_agentic_work_graph(&crate::db::NewAgenticWorkGraph {
                request_id: &request.id,
                graph: &graph,
                provider: "opencode",
                model: "deepseek-v4.1-flash",
                schema_sha256: &"a".repeat(64),
                request_sha256: &"b".repeat(64),
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cost_micros: 1,
                usage_known: true,
            })
            .await
            .unwrap();
        assert_eq!(saved.graph_sha256, graph.graph_sha256);
        let stack = crate::agentic::StackProfile {
            languages: vec!["rust".into()],
            ..crate::agentic::StackProfile::default()
        };
        let plan = crate::agentic::participant_plan_for_graph(
            crate::agentic::ChangeRoute::FormalSpec,
            crate::agentic::Assurance::Elevated,
            &graph,
            &stack,
            "opencode",
            "deepseek-v4.1-flash",
            "build",
        );
        let plan_record = db
            .save_agentic_participant_plan(&request.id, Some(&graph.graph_sha256), &plan)
            .await
            .unwrap();
        assert_eq!(plan_record.reviewer_count, 6);
        assert_eq!(plan_record.work_units, 2);
        assert_eq!(
            plan_record.graph_sha256.as_deref(),
            Some(graph.graph_sha256.as_str())
        );
        let draft_model = crate::agentic::SpecDraftModel {
            summary: "Bounded validator".into(),
            requirements: vec![crate::agentic::SpecRequirementDraft {
                statement: "Invalid input is rejected.".into(),
                work_unit_id: graph
                    .units
                    .first()
                    .map(|unit| unit.id.clone())
                    .unwrap_or_default(),
                acceptance: vec!["invalid input returns an error".into()],
                negative: vec!["valid input is accepted".into()],
            }],
            assumptions: vec![],
            risks: vec![],
        };
        let draft_json = serde_json::to_string(&draft_model).unwrap();
        let draft = db
            .save_agentic_spec_draft(&crate::db::NewAgenticSpecDraft {
                request_id: &request.id,
                draft_json: &draft_json,
                graph_sha256: Some(&graph.graph_sha256),
                provider: "opencode",
                model: "deepseek-v4.1-flash",
                schema_sha256: &"a".repeat(64),
                request_sha256: &"b".repeat(64),
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cost_micros: 1,
                usage_known: true,
            })
            .await
            .unwrap();
        assert_eq!(draft.revision, 1);
        let mut changed_graph_model = graph_model.clone();
        changed_graph_model
            .assumptions
            .push("a later plan change".into());
        let changed_graph =
            crate::agentic::validate_work_graph(&changed_graph_model, &["src".into()]).unwrap();
        db.save_agentic_work_graph(&crate::db::NewAgenticWorkGraph {
            request_id: &request.id,
            graph: &changed_graph,
            provider: "opencode",
            model: "deepseek-v4.1-flash",
            schema_sha256: &"a".repeat(64),
            request_sha256: &"b".repeat(64),
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cost_micros: 1,
            usage_known: true,
        })
        .await
        .unwrap();
        let frozen = crate::agentic::compile_spec(&draft_model, draft.revision).unwrap();
        assert!(
            db.freeze_agentic_spec(&draft.id, &frozen, &changed_graph)
                .await
                .is_err(),
            "a changed WorkGraph must not freeze against an older draft"
        );
        db.save_agentic_work_graph(&crate::db::NewAgenticWorkGraph {
            request_id: &request.id,
            graph: &graph,
            provider: "opencode",
            model: "deepseek-v4.1-flash",
            schema_sha256: &"a".repeat(64),
            request_sha256: &"b".repeat(64),
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cost_micros: 1,
            usage_known: true,
        })
        .await
        .unwrap();
        let frozen_record = db
            .freeze_agentic_spec(&draft.id, &frozen, &graph)
            .await
            .unwrap();
        assert_eq!(frozen_record.state, "FROZEN");
        assert_eq!(
            frozen_record.spec_sha256.as_deref(),
            Some(frozen.spec_sha256.as_str())
        );
        assert!(
            db.save_agentic_work_graph(&crate::db::NewAgenticWorkGraph {
                request_id: &request.id,
                graph: &graph,
                provider: "opencode",
                model: "deepseek-v4.1-flash",
                schema_sha256: &"a".repeat(64),
                request_sha256: &"b".repeat(64),
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cost_micros: 1,
                usage_known: true,
            })
            .await
            .is_err()
        );
        let second = db
            .save_agentic_spec_draft(&crate::db::NewAgenticSpecDraft {
                request_id: &request.id,
                draft_json: &draft_json,
                graph_sha256: Some(&graph.graph_sha256),
                provider: "opencode",
                model: "deepseek-v4.1-flash",
                schema_sha256: &"a".repeat(64),
                request_sha256: &"b".repeat(64),
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cost_micros: 1,
                usage_known: true,
            })
            .await
            .unwrap();
        assert_eq!(second.revision, 2);
        assert_eq!(db.agentic_spec(&request.id).await.unwrap().revision, 1);
        let second_frozen = crate::agentic::compile_spec(&draft_model, second.revision).unwrap();
        db.freeze_agentic_spec(&second.id, &second_frozen, &graph)
            .await
            .unwrap();
        let current = db.agentic_spec(&request.id).await.unwrap();
        assert_eq!(current.revision, 2);
        assert_eq!(
            db.agentic_spec_by_id(&draft.id).await.unwrap().state,
            "SUPERSEDED"
        );
        let publication = db
            .upsert_agentic_spec_publication(&crate::db::NewAgenticSpecPublication {
                spec_id: &current.id,
                project_id: "project",
                provider: "GITHUB",
                repository: "acme/widgets",
                status: "PUBLISHED",
                drift_status: "IN_SYNC",
                last_error: None,
            })
            .await
            .unwrap();
        assert_eq!(publication.status, "PUBLISHED");
        let issue = db
            .record_agentic_spec_issue(
                &current.id,
                "api",
                "factory-spec:api",
                Some(42),
                Some("https://github.com/acme/widgets/issues/42"),
                "PUBLISHED",
            )
            .await
            .unwrap();
        assert_eq!(issue.external_number, Some(42));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn knowledge_skill_promotion_follows_the_trust_gate() {
        let (path, db, profile, config) = fixture().await;
        let policy = create_policy(&db, &profile, &config).await;
        let _ = policy;
        let request = create_request(&db, &profile, "skill-request").await;
        let pack = crate::knowledge::KnowledgePack {
            library: "chrono".into(),
            library_id: "/chronotope/chrono".into(),
            query: "timezone-aware ICS export".into(),
            sources: vec![crate::knowledge::KnowledgeSource {
                title: "Chrono — Context7".into(),
                url: "https://context7.com/chronotope/chrono".into(),
                kind: "CONTEXT7_MCP".into(),
                snippet: "Use Utc::now().to_rfc3339().".into(),
                reputation: Some("High".into()),
                benchmark_score: Some(77.3),
            }],
            pack_sha256: String::new(),
            fetched_at: "2026-11-09T00:00:00Z".into(),
        }
        .seal();
        let saved = db
            .save_agentic_knowledge_pack(&crate::db::NewAgenticKnowledgePack {
                request_id: &request.id,
                project_id: "project",
                pack: &pack,
                provider: "CONTEXT7_MCP",
                source_url: "https://context7.com/chronotope/chrono",
            })
            .await
            .unwrap();
        assert_eq!(saved.pack_sha256, pack.pack_sha256);
        let manifest = crate::knowledge::derive_skill_manifest(
            &pack,
            &["rust".into()],
            "2026-11-09T00:00:00Z",
            "2026-11-16T00:00:00Z",
        )
        .unwrap();
        let skill = db
            .create_agentic_skill(&crate::db::NewAgenticSkill {
                project_id: "project",
                request_id: &request.id,
                manifest: &manifest,
                manifest_sha256: &crate::knowledge::skill_manifest_sha256(&manifest),
                status: "ACTIVE",
            })
            .await
            .unwrap();
        assert_eq!(skill.scope, "RUN");
        assert_eq!(skill.status, "ACTIVE");
        let adoption = db
            .adopt_agentic_skill(&request.id, &skill.id, "implementer")
            .await
            .unwrap();
        assert_eq!(adoption.role, "implementer");
        let now = chrono::Utc::now().to_rfc3339();
        let action_id: (String,) =
            sqlx::query_as::query_as("SELECT action_id FROM intake_requests WHERE id=?1")
                .bind(&request.id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES ('skill-campaign',?1,'AUTHORIZED',?2)",
        )
        .bind(&action_id.0)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query("UPDATE intake_requests SET campaign_id='skill-campaign' WHERE id=?1")
            .bind(&request.id)
            .execute(&db.pool)
            .await
            .unwrap();
        let assignment_skills = db
            .adopted_skills_for_campaign("skill-campaign")
            .await
            .unwrap();
        assert_eq!(assignment_skills.len(), 1);
        assert_eq!(assignment_skills[0].name, "context7-chrono");
        assert!(assignment_skills[0].instructions.contains("Context7"));
        assert_eq!(
            assignment_skills[0].tools,
            vec!["context-read", "repository-read"]
        );
        assert!(
            db.record_agentic_skill_use(&skill.id, &request.id, "SUCCESS", "unlicensed")
                .await
                .is_err(),
            "a NOASSERTION skill cannot be used before license review"
        );
        let skill = db
            .review_agentic_skill_license(&skill.id, "owner", "MIT")
            .await
            .unwrap();
        assert_eq!(skill.scope, "RUN");
        let skill = db
            .record_agentic_skill_use(
                &skill.id,
                &request.id,
                "SUCCESS",
                "candidate accepted after using the docs",
            )
            .await
            .unwrap();
        assert_eq!(skill.success_count, 1);
        assert_eq!(skill.scope, "RUN");
        let skill = db
            .record_agentic_skill_use(&skill.id, &request.id, "SUCCESS", "second accepted use")
            .await
            .unwrap();
        assert_eq!(skill.success_count, 2);
        assert_eq!(skill.scope, "PROJECT");
        assert_eq!(skill.status, "ACTIVE");
        let skill = db.review_agentic_skill(&skill.id, "owner").await.unwrap();
        assert!(skill.last_reviewed_at.is_some());
        let skill = db
            .promote_agentic_skill(&skill.id, "GLOBAL", "owner", true)
            .await
            .unwrap();
        assert_eq!(skill.scope, "GLOBAL");
        assert_eq!(skill.owner_approved_by.as_deref(), Some("owner"));
        assert_eq!(db.agentic_skill_uses(&skill.id).await.unwrap().len(), 2);
        let skill = db
            .revoke_agentic_skill(&skill.id, "superseded")
            .await
            .unwrap();
        assert_eq!(skill.status, "REVOKED");
        assert!(
            db.adopt_agentic_skill(&request.id, &skill.id, "implementer")
                .await
                .is_err()
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn owner_charter_and_reserved_decision_are_durable() {
        let (path, db, _profile, _config) = fixture().await;
        let expires_at = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
        let domains = vec!["repository".to_string()];
        let reserved = vec!["credentials".to_string(), "delivery".to_string()];
        let charter = db
            .create_agentic_charter(&crate::db::NewAgenticCharter {
                owner: "owner",
                project_id: "project",
                domains: &domains,
                risk_appetite: crate::lead::RiskClass::R1,
                execution_autonomy: true,
                repair_autonomy: true,
                delivery_autonomy: false,
                reserved_matters: &reserved,
                expires_at: &expires_at,
            })
            .await
            .unwrap();
        assert_eq!(charter.risk_appetite, crate::lead::RiskClass::R1);
        assert!(
            db.active_agentic_charter("project")
                .await
                .unwrap()
                .is_some()
        );
        let deadline = (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
        let options = vec!["APPROVE".to_string(), "DENY".to_string()];
        let evidence = serde_json::json!({"objective": "rotate a credential"});
        let decision = db
            .create_agentic_owner_decision(&crate::db::NewAgenticOwnerDecision {
                project_id: "project",
                request_id: None,
                campaign_id: None,
                matter: "credentials",
                risk: crate::lead::RiskClass::R2,
                options: &options,
                evidence: &evidence,
                recommendation: "DENY",
                deadline: &deadline,
            })
            .await
            .unwrap();
        assert_eq!(decision.status, "PENDING");
        let resolved = db
            .resolve_agentic_owner_decision(&decision.id, "APPROVE", "bounded rotation", "owner")
            .await
            .unwrap();
        assert_eq!(resolved.status, "APPROVED");
        assert_eq!(resolved.selected_option.as_deref(), Some("APPROVE"));
        let revoked = db
            .revoke_agentic_charter(&charter.id, "withdrawn")
            .await
            .unwrap();
        assert!(revoked.revoked_at.is_some());
        assert!(
            db.active_agentic_charter("project")
                .await
                .unwrap()
                .is_none()
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unfinished_autonomous_requests_exclude_review_plan_work() {
        let (path, db, _profile, config) = fixture().await;
        let mut content = profile_content("Autopilot");
        content.auto_start = AutoStartPolicy {
            enabled: true,
            risk_classes: vec![RiskClass::R0],
        };
        let profile = db
            .create_execution_profile(&content, "owner")
            .await
            .unwrap();
        db.create_execution_policy_grant(NewExecutionPolicyGrant {
            project_id: "project",
            profile_id: &profile.id,
            expected_revision: profile.revision,
            expected_revision_hash: &profile.content_hash,
            project_config_hash: &config.config_hash,
            acknowledge: true,
            autonomous: true,
            subscription_confirmed: true,
            idempotency_key: "autopilot-unfinished",
            principal_id: "operator-token",
            principal_label: None,
        })
        .await
        .unwrap();
        let (autonomous, _) = db
            .create_intake_request(
                "project",
                "autonomous work",
                Some(&profile.id),
                IntakeMode::Autonomous,
                "autonomous-unfinished",
                "operator-token",
                &[],
            )
            .await
            .unwrap();
        let _ = create_policy(&db, &profile, &config).await;
        let (review, _) = db
            .create_intake_request(
                "project",
                "review work",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "review-unfinished",
                "operator-token",
                &[],
            )
            .await
            .unwrap();
        let review_operation = db.agentic_operation_for_request(&review.id).await.unwrap();
        assert_eq!(review_operation.status, "OBSERVED");
        let unfinished = db.unfinished_autonomous_intake_requests().await.unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0].id, autonomous.id);
        assert_ne!(unfinished[0].id, review.id);
        let lead_id: (String,) =
            sqlx::query_as::query_as("SELECT lead_request_id FROM intake_requests WHERE id=?1")
                .bind(&autonomous.id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        db.set_agentic_operation_status(&autonomous.id, "EXECUTING")
            .await
            .unwrap();
        sqlx::query::query("UPDATE lead_requests SET state='BLOCKED' WHERE id=?1")
            .bind(&lead_id.0)
            .execute(&db.pool)
            .await
            .unwrap();
        let changed = db.reconcile_agentic_operations().await.unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].status, "BLOCKED");
        let unfinished = db.unfinished_autonomous_intake_requests().await.unwrap();
        assert!(unfinished.is_empty());
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn autonomous_grant_rejects_non_subscription_routes() {
        let (path, db, _profile, config) = fixture().await;
        configure_provider(&db, "opencode", "opencode", "deepseek-v4.1-flash").await;
        let mut content = profile_content("API key route");
        content.auto_start = AutoStartPolicy {
            enabled: true,
            risk_classes: vec![RiskClass::R0],
        };
        content.roles.lead.provider = "opencode".into();
        content.roles.implementer.provider = "opencode".into();
        let profile = db
            .create_execution_profile(&content, "owner")
            .await
            .unwrap();
        let error = db
            .create_execution_policy_grant(NewExecutionPolicyGrant {
                project_id: "project",
                profile_id: &profile.id,
                expected_revision: profile.revision,
                expected_revision_hash: &profile.content_hash,
                project_config_hash: &config.config_hash,
                acknowledge: true,
                autonomous: true,
                subscription_confirmed: true,
                idempotency_key: "api-key-autopilot",
                principal_id: "operator-token",
                principal_label: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "AUTONOMOUS_PROFILE_REQUIRED",
                ..
            }
        ));
        db.close().await;
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
                autonomous: false,
                subscription_confirmed: false,
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
                autonomous: false,
                subscription_confirmed: false,
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
                &[],
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
                autonomous: false,
                subscription_confirmed: false,
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
                &[],
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
                autonomous: false,
                subscription_confirmed: false,
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
    async fn approval_scope_and_start_enforce_frozen_operation_limits() {
        let (path, db, _base_profile, config) = fixture().await;
        let mut limited = profile_content("Operation limited");
        limited.operations.max_generation_tokens = std::num::NonZeroU32::new(4_096).unwrap();
        limited.operations.tool_call_timeout_secs = std::num::NonZeroU32::new(120).unwrap();
        let profile = db
            .create_execution_profile(&limited, "owner")
            .await
            .unwrap();
        db.create_execution_policy_grant(NewExecutionPolicyGrant {
            project_id: "project",
            profile_id: &profile.id,
            expected_revision: profile.revision,
            expected_revision_hash: &profile.content_hash,
            project_config_hash: &config.config_hash,
            acknowledge: true,
            autonomous: false,
            subscription_confirmed: false,
            idempotency_key: "policy-operation-limited",
            principal_id: "operator-token",
            principal_label: None,
        })
        .await
        .unwrap();
        let request = create_request(&db, &profile, "request-operation-limited").await;
        let context = record_fixture_plan(&db, &request).await;
        // The approval scope carries the profile's frozen technical operation
        // limits and no attempts/sessions/turns/tokens/cost counters.
        assert_eq!(context.scope.operations, limited.operations);
        assert_eq!(context.scope.progress, limited.progress);
        assert_eq!(context.scope.max_parallel, 1);

        let approval = |rationale: &'static str| {
            AuthenticatedOperatorApproval::new("operator-token", None, rationale).unwrap()
        };
        let mut inflated_timeout = authorization(&context);
        inflated_timeout.operations.tool_call_timeout_secs =
            std::num::NonZeroU32::new(121).unwrap();
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "operation-timeout",
                StartAuthority::Operator(approval("timeout")),
                Some(&binding(&context)),
                Some(&inflated_timeout),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");

        let mut inflated_tokens = authorization(&context);
        inflated_tokens.operations.max_generation_tokens =
            std::num::NonZeroU32::new(4_097).unwrap();
        let error = db
            .start_lead_campaign(
                &context.lead.id,
                "operation-tokens",
                StartAuthority::Operator(approval("tokens")),
                Some(&binding(&context)),
                Some(&inflated_tokens),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");

        let result = db
            .start_lead_campaign(
                &context.lead.id,
                "operation-ok",
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

    async fn mark_request_completed(db: &Database, request: &IntakeRequestRecord) {
        let now = Utc::now().to_rfc3339();
        let campaign_id = format!("campaign-{}", request.id);
        let run_id = format!("run-{}", request.id);
        let attempt_id = format!("attempt-{}", request.id);
        let candidate_id = format!("candidate-{}", request.id);
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES (?1,?2,'SUCCEEDED',?3)",
        )
        .bind(&campaign_id)
        .bind(&request.action_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO runs(id,campaign_id,status,created_at) VALUES (?1,?2,'SUCCEEDED',?3)",
        )
        .bind(&run_id)
        .bind(&campaign_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,'SUCCEEDED',?3)",
        )
        .bind(&attempt_id)
        .bind(&run_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
             VALUES (?1,?2,?3,?4,?5,'fixture',?6)",
        )
        .bind(&candidate_id)
        .bind(&attempt_id)
        .bind("a".repeat(64))
        .bind("b".repeat(40))
        .bind("c".repeat(40))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO gate_decisions(candidate_id,outcome,reasons_json,decided_at)
             VALUES (?1,'ACCEPTED','[]',?2)",
        )
        .bind(&candidate_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query("UPDATE lead_requests SET campaign_id=?1 WHERE id=?2")
            .bind(&campaign_id)
            .bind(&request.lead_request_id)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query("UPDATE intake_requests SET campaign_id=?1 WHERE id=?2")
            .bind(&campaign_id)
            .bind(&request.id)
            .execute(&db.pool)
            .await
            .unwrap();
    }

    async fn count_for(sql: &'static str, id: &str, db: &Database) -> i64 {
        let (count,): (i64,) = sqlx::query_as::query_as(sql)
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        count
    }

    #[tokio::test]
    async fn terminal_message_creates_one_follow_up_without_reusing_parent_evidence() {
        let (path, db, profile, config) = fixture().await;
        let policy = create_policy(&db, &profile, &config).await;
        let parent = create_request(&db, &profile, "request-parent").await;
        mark_request_completed(&db, &parent).await;

        let outcome = db
            .append_intake_message_or_start_follow_up(
                &parent.id,
                "Add a CSV export endpoint as a follow-up.",
                "follow-up-message-1",
                "operator-token",
            )
            .await
            .unwrap();
        assert!(outcome.created);
        let follow_up = outcome.follow_up.as_ref().expect("follow-up request");
        assert_eq!(
            follow_up.parent_request_id.as_deref(),
            Some(parent.id.as_str())
        );
        assert_eq!(
            follow_up.source_message_id.as_deref(),
            Some(outcome.message.id.as_str())
        );
        assert_eq!(
            outcome.message.follow_up_request_id.as_deref(),
            Some(follow_up.id.as_str())
        );
        assert_eq!(follow_up.objective, outcome.message.body);
        assert_eq!(follow_up.project_id, parent.project_id);
        assert_eq!(follow_up.profile_id, parent.profile_id);
        assert_eq!(follow_up.profile_revision_id, parent.profile_revision_id);
        assert_eq!(follow_up.mode, parent.mode);
        assert_eq!(follow_up.campaign_id, None);
        assert_ne!(follow_up.action_id, parent.action_id);
        assert_ne!(follow_up.lead_request_id, parent.lead_request_id);
        // The unchanged config still matches the same owner-confirmed grant.
        assert_eq!(follow_up.delegation_id, policy.delegation.id);

        // No frozen plan, ActionSpec or Campaign is inherited by the child.
        assert_eq!(
            count_for(
                "SELECT COUNT(*) FROM frozen_lead_plans WHERE request_id=?1",
                &follow_up.lead_request_id,
                &db
            )
            .await,
            0
        );
        assert_eq!(
            count_for(
                "SELECT COUNT(*) FROM action_specs WHERE action_id=?1",
                &follow_up.action_id,
                &db
            )
            .await,
            0
        );
        let child_lead = db.lead_request(&follow_up.lead_request_id).await.unwrap();
        assert_eq!(child_lead.campaign_id, None);
        assert_eq!(
            db.intake_authorization_state(&follow_up.id, crate::intake::IntakeState::Submitted)
                .await
                .reason
                .as_deref(),
            Some("FROZEN_PLAN_REQUIRED")
        );

        // Exact replay returns the same message and child, never a second one.
        let replay = db
            .append_intake_message_or_start_follow_up(
                &parent.id,
                "Add a CSV export endpoint as a follow-up.",
                "follow-up-message-1",
                "operator-token",
            )
            .await
            .unwrap();
        assert!(!replay.created);
        assert_eq!(replay.follow_up.as_ref().unwrap().id, follow_up.id);
        assert_eq!(
            count_for(
                "SELECT COUNT(*) FROM intake_requests WHERE parent_request_id=?1",
                &parent.id,
                &db
            )
            .await,
            1
        );

        // A different message cannot fork a second follow-up from the same
        // terminal parent; the operator continues in the created child.
        let error = db
            .append_intake_message_or_start_follow_up(
                &parent.id,
                "A second unrelated objective.",
                "follow-up-message-2",
                "operator-token",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "INTAKE_FOLLOW_UP_EXISTS",
                ..
            }
        ));
        let messages = db.intake_messages(&parent.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn another_principal_with_a_matching_grant_cannot_continue_the_parent_policy() {
        let (path, db, profile, config) = fixture().await;
        let _parent_policy = create_policy(&db, &profile, &config).await;
        let parent = create_request(&db, &profile, "request-parent-other-principal").await;
        mark_request_completed(&db, &parent).await;
        // A second authenticated principal has its own current grant for the
        // same Project, exact profile revision and scope.
        db.create_execution_policy_grant(NewExecutionPolicyGrant {
            project_id: "project",
            profile_id: &profile.id,
            expected_revision: profile.revision,
            expected_revision_hash: &profile.content_hash,
            project_config_hash: &config.config_hash,
            acknowledge: true,
            autonomous: false,
            subscription_confirmed: false,
            idempotency_key: "policy-other-principal",
            principal_id: "other-operator",
            principal_label: None,
        })
        .await
        .unwrap();

        let error = db
            .append_intake_message_or_start_follow_up(
                &parent.id,
                "Another principal tries to continue this thread.",
                "follow-up-message-other-principal",
                "other-operator",
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::IntakeConflict {
                code: "FOLLOW_UP_POLICY_PRINCIPAL_MISMATCH",
                ..
            }
        ));
        assert_eq!(
            count_for(
                "SELECT COUNT(*) FROM intake_requests WHERE parent_request_id=?1",
                &parent.id,
                &db
            )
            .await,
            0
        );
        assert!(db.intake_messages(&parent.id).await.unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn follow_up_uses_the_merged_base_commit_and_rebinds_the_same_policy() {
        let (path, db, profile, config) = fixture().await;
        let policy = create_policy(&db, &profile, &config).await;
        let parent = create_request(&db, &profile, "request-parent-base").await;
        mark_request_completed(&db, &parent).await;
        let merge_commit = "e".repeat(40);
        let advanced = db
            .advance_project_base_after_merged_delivery(
                "project",
                &merge_commit,
                "main",
                config.revision,
                &config.base_commit,
            )
            .await
            .unwrap();
        assert_eq!(advanced.base_commit, merge_commit);

        let outcome = db
            .append_intake_message_or_start_follow_up(
                &parent.id,
                "Build the follow-up on merged main.",
                "follow-up-message-base",
                "operator-token",
            )
            .await
            .unwrap();
        let follow_up = outcome.follow_up.expect("follow-up request");
        let child_config = db.project_config("project").await.unwrap();
        assert_eq!(child_config.base_commit, merge_commit);
        let child_lead = db.lead_request(&follow_up.lead_request_id).await.unwrap();
        assert_eq!(child_lead.project_config_hash, advanced.config_hash);
        let context = db
            .lead_execution_context(&follow_up.lead_request_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(context.project_config_hash, advanced.config_hash);
        let spec = db.intake_assessment_spec(&follow_up.id).await.unwrap();
        assert_eq!(spec.base_commit, merge_commit);

        // The exact parent grant terms are re-bound to the Core-advanced config;
        // no autonomy, scope or subscription confirmation is widened.
        assert_ne!(follow_up.delegation_id, policy.delegation.id);
        let rebound = db
            .standing_delegation_v2(&follow_up.delegation_id)
            .await
            .unwrap();
        assert_eq!(rebound.principal_id, policy.delegation.principal_id);
        assert_eq!(rebound.allowed_paths, policy.delegation.allowed_paths);
        assert_eq!(rebound.capabilities, policy.delegation.capabilities);
        assert_eq!(rebound.risk_classes, policy.delegation.risk_classes);
        assert_eq!(rebound.autonomous_start, policy.delegation.autonomous_start);
        let rebound_policy = db
            .execution_policy_grant(&follow_up.delegation_id)
            .await
            .unwrap();
        assert_eq!(rebound_policy.project_config_hash, advanced.config_hash);
        assert!(!rebound_policy.autonomous);
        assert!(!rebound_policy.subscription_confirmed);
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
            .standing_delegation_v2(&request.delegation_id)
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
            dependency_plan: None,
            dependencies_required: false,
            environment_plan_id: None,
            environment_plan_sha256: None,
            environment_spec_sha256: None,
            environment_builder_image: None,
            environment_builder_image_digest: None,
            environment_required: false,

            environment_spec_id: None,
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

    async fn seed_issue(
        db: &Database,
        number: i64,
        github_id: i64,
        state: &str,
        is_pull_request: bool,
    ) {
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO github_issues
             (project_id,repo_full_name,number,github_id,title,body,body_truncated,state,
              state_reason,labels_json,author,author_id,author_association,html_url,comments,
              created_at,updated_at,closed_at,synced_at,sync_mark,content_hash,
              repository_github_id,is_pull_request)
             VALUES ('project','acme/widgets',?1,?2,?3,'body',0,?4,NULL,'[]','octo',1,
                     'MEMBER','https://github.com/acme/widgets/issues/'||?1,0,?5,?5,NULL,?5,'sync',
                     'hash',42,?6)",
        )
        .bind(number)
        .bind(github_id)
        .bind(format!("Issue {number}"))
        .bind(state)
        .bind(&now)
        .bind(i64::from(is_pull_request))
        .execute(&db.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn issue_selection_is_frozen_and_rejects_unsupported_foreign_duplicate_and_closed() {
        let (path, db, profile, config) = fixture().await;
        let _policy = create_policy(&db, &profile, &config).await;
        db.bind_project_github(
            "project",
            NewProjectGitHubBinding {
                owner: "acme",
                repo: "widgets",
                default_branch: "main",
                repository_github_id: Some(42),
            },
            "tester",
        )
        .await
        .unwrap();
        seed_issue(&db, 7, 70, "open", false).await;
        seed_issue(&db, 8, 80, "closed", false).await;
        seed_issue(&db, 9, 90, "open", true).await;

        let issue = |number, role| crate::issues::NewIssueSelection {
            owner: "acme".into(),
            repo: "widgets".into(),
            number,
            role,
        };
        let primary = issue(7, crate::issues::IssueRole::Primary);

        let (request, created) = db
            .create_intake_request(
                "project",
                "Deliver selected issue 7",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "issue-selection-request",
                "operator-token",
                std::slice::from_ref(&primary),
            )
            .await
            .unwrap();
        assert!(created);
        let links = db.issue_links_for_request(&request.id).await.unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].issue_number, 7);
        assert_eq!(links[0].repository_full_name, "acme/widgets");
        assert_eq!(links[0].role, crate::issues::IssueRole::Primary);
        assert_eq!(links[0].state, crate::issues::IssueLinkState::Ready);
        assert_eq!(links[0].github_issue_id, Some(70));

        let replay = db
            .create_intake_request(
                "project",
                "Deliver selected issue 7",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "issue-selection-request",
                "operator-token",
                std::slice::from_ref(&primary),
            )
            .await
            .unwrap();
        assert!(!replay.1);
        assert_eq!(replay.0.id, request.id);

        let changed = db
            .create_intake_request(
                "project",
                "Deliver selected issue 7",
                Some(&profile.id),
                IntakeMode::ReviewPlan,
                "issue-selection-request",
                "operator-token",
                &[],
            )
            .await
            .unwrap_err();
        assert!(matches!(
            changed,
            Error::IntakeConflict {
                code: "ISSUE_SELECTION_CHANGED",
                ..
            }
        ));

        for (selection, code) in [
            (vec![primary.clone(), primary.clone()], "ISSUE_DUPLICATE"),
            (
                vec![crate::issues::NewIssueSelection {
                    owner: "other".into(),
                    repo: "widgets".into(),
                    number: 7,
                    role: crate::issues::IssueRole::Primary,
                }],
                "ISSUE_REPOSITORY_FOREIGN",
            ),
            (
                vec![issue(8, crate::issues::IssueRole::Primary)],
                "ISSUE_CLOSED",
            ),
            (
                vec![issue(99, crate::issues::IssueRole::Primary)],
                "ISSUE_UNSUPPORTED",
            ),
            (
                vec![issue(9, crate::issues::IssueRole::Primary)],
                "ISSUE_UNSUPPORTED",
            ),
        ] {
            let error = db
                .create_intake_request(
                    "project",
                    "Reject an invalid issue selection",
                    Some(&profile.id),
                    IntakeMode::ReviewPlan,
                    &format!("issue-selection-{code}"),
                    "operator-token",
                    &selection,
                )
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::IntakeConflict { code: actual, .. } if actual == code),
                "expected {code}, got {error:?}"
            );
        }

        // The link follows the request into its Campaign without becoming DONE.
        sqlx::query::query(
            "INSERT INTO campaigns VALUES ('campaign-links',?1,'BLOCKED_AUTHORIZATION',?2)",
        )
        .bind(&request.action_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query("UPDATE intake_requests SET campaign_id='campaign-links' WHERE id=?1")
            .bind(&request.id)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query(
            "UPDATE lead_requests SET campaign_id='campaign-links',state='RUNNING' WHERE id=?1",
        )
        .bind(&request.lead_request_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let campaign_links = db.issue_links_for_campaign("campaign-links").await.unwrap();
        assert_eq!(campaign_links.len(), 1);
        assert_eq!(
            campaign_links[0].state,
            crate::issues::IssueLinkState::Queued
        );
        sqlx::query::query("UPDATE campaigns SET status='AUTHORIZED' WHERE id='campaign-links'")
            .execute(&db.pool)
            .await
            .unwrap();
        let running = db.issue_links_for_campaign("campaign-links").await.unwrap();
        assert_eq!(running[0].state, crate::issues::IssueLinkState::InProgress);

        let marker = "<!-- factory:issue-link:test:OPENED:1 -->";
        let body = format!("{marker}\n\ncomment");
        let event = NewIssueDeliveryEvent {
            issue_link_id: &links[0].id,
            campaign_id: Some("campaign-links"),
            candidate_id: Some("candidate-links"),
            action: crate::issues::IssueDeliveryAction::Comment,
            event_key: "comment:opened:candidate:1",
            marker: Some(marker),
            body: Some(&body),
            label_name: None,
        };
        let (first, created) = db.record_issue_delivery_event(event.clone()).await.unwrap();
        assert!(created);
        let (replayed, created) = db.record_issue_delivery_event(event).await.unwrap();
        assert!(!created);
        assert_eq!(replayed.id, first.id);
        let conflicting = db
            .record_issue_delivery_event(NewIssueDeliveryEvent {
                issue_link_id: &links[0].id,
                campaign_id: Some("campaign-links"),
                candidate_id: Some("candidate-links"),
                action: crate::issues::IssueDeliveryAction::Comment,
                event_key: "comment:opened:candidate:1",
                marker: Some(marker),
                body: Some(&format!("{marker}\n\ndifferent body")),
                label_name: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(conflicting, Error::Conflict(_)));

        db.close().await;
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
