//! Durable Lead request, assessment, proposal and frozen-plan identity.
//!
//! Storage does not start a Campaign, grant autonomy, call a model or approve a
//! risk class. Core policy and explicit authorization remain separate.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

use crate::lead::{
    ActionBinding, AssessmentInput, LeadAssessment, LeadPlanProposal, LeadRequestState,
    ProjectConfig, StandingDelegation, plan_hash,
};

use super::delegations::{load_delegation, release_usage_in_tx, reserve_usage_in_tx};
use super::intake::{
    LeadExecutionContextRecord, load_execution_context, load_frozen_profile_content,
};
use super::lifecycle::load_campaign;
use super::project_configs::load_project_config;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadRequestRecord {
    pub id: String,
    pub project_id: String,
    pub action_id: String,
    pub delegation_id: String,
    pub objective: String,
    pub request_hash: String,
    pub project_config_hash: String,
    pub state: LeadRequestState,
    pub assessment_mechanism: Option<String>,
    pub failure_reason: Option<String>,
    pub campaign_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadAssessmentRecord {
    pub id: String,
    pub request_id: String,
    pub mechanism: String,
    pub assessment_json: String,
    pub schema_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadProposalRecord {
    pub id: String,
    pub request_id: String,
    pub assessment_id: String,
    pub proposal_json: String,
    pub proposal_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadTaskRecord {
    pub task_id: String,
    pub campaign_id: String,
    pub request_id: String,
    pub unit_id: String,
    pub sequence: i64,
    pub status: String,
    pub base_commit: String,
    pub depends_on: Vec<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub candidate_id: Option<String>,
    pub candidate_commit: Option<String>,
    pub candidate_tree: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenLeadPlanRecord {
    pub id: String,
    pub request_id: String,
    pub proposal_id: String,
    pub plan_hash: String,
    pub config_hash: String,
    pub delegation_hash: String,
    pub created_at: String,
}

/// Immutable assessment provenance for an Action that has not yet received its
/// first executable ActionSpec. This is not execution authority and is never
/// resolved by a Worker or the Quality Gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDraftRecord {
    pub action_id: String,
    pub assessment_input_json: String,
    pub input_sha256: String,
    pub created_at: String,
}

/// Core-constructed proof that the deployment policy gate for autonomous start
/// was applied. It is never deserialized from HTTP, model or Worker input and
/// grants no other authority; the start transaction still validates the active
/// delegation, autonomous R0 scope, plan, route and budgets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedAutonomyContext {
    _private: (),
}

impl ValidatedAutonomyContext {
    pub fn from_deployment_policy(enabled: bool) -> Result<Self, Error> {
        if !enabled {
            return Err(Error::Forbidden(
                "autonomous start policy is not activated".into(),
            ));
        }
        Ok(Self { _private: () })
    }
}

/// Core-constructed proof of an authenticated operator approval. The principal
/// identity comes from the authenticated request boundary; a display label is
/// audit metadata only. Client-supplied actor fields are never parsed into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedOperatorApproval {
    principal_id: String,
    principal_label: Option<String>,
    rationale: String,
}

impl AuthenticatedOperatorApproval {
    pub fn new(
        principal_id: &str,
        principal_label: Option<&str>,
        rationale: &str,
    ) -> Result<Self, Error> {
        if principal_id.trim().is_empty()
            || principal_id.len() > 200
            || rationale.trim().is_empty()
            || rationale.len() > 2_000
            || principal_label.is_some_and(|label| label.trim().is_empty() || label.len() > 200)
        {
            return Err(Error::Invalid("operator approval identity or rationale"));
        }
        Ok(Self {
            principal_id: principal_id.into(),
            principal_label: principal_label.map(str::to_owned),
            rationale: rationale.into(),
        })
    }

    fn principal_id(&self) -> &str {
        &self.principal_id
    }

    fn principal_label(&self) -> Option<&str> {
        self.principal_label.as_deref()
    }

    fn rationale(&self) -> &str {
        &self.rationale
    }
}

/// Exactly which frozen identities the caller believes it is approving. A
/// changed hash, principal or rationale conflicts instead of silently starting
/// a different Campaign.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedStartBinding {
    pub request_hash: String,
    pub plan_hash: String,
    pub spec_sha256: String,
    pub config_hash: String,
    pub delegation_hash: String,
    pub profile_revision_id: Option<String>,
    pub profile_hash: Option<String>,
}

impl ExpectedStartBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_hash: &str,
        plan_hash: &str,
        spec_sha256: &str,
        config_hash: &str,
        delegation_hash: &str,
        profile_revision_id: Option<&str>,
        profile_hash: Option<&str>,
    ) -> Result<Self, Error> {
        if ![
            request_hash,
            plan_hash,
            spec_sha256,
            config_hash,
            delegation_hash,
        ]
        .iter()
        .all(|value| is_sha256(value))
            || profile_revision_id.is_some() != profile_hash.is_some()
            || profile_revision_id.is_some_and(|value| value.trim().is_empty() || value.len() > 200)
            || profile_hash.is_some_and(|value| !is_sha256(value))
        {
            return Err(Error::Invalid("expected Lead start binding"));
        }
        Ok(Self {
            request_hash: request_hash.into(),
            plan_hash: plan_hash.into(),
            spec_sha256: spec_sha256.into(),
            config_hash: config_hash.into(),
            delegation_hash: delegation_hash.into(),
            profile_revision_id: profile_revision_id.map(str::to_owned),
            profile_hash: profile_hash.map(str::to_owned),
        })
    }

    fn profile_binding(&self) -> Option<(&str, &str)> {
        self.profile_revision_id
            .as_deref()
            .zip(self.profile_hash.as_deref())
    }
}

/// Authority mode constructed by Core after authentication and policy checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartAuthority {
    Autonomous(ValidatedAutonomyContext),
    Operator(AuthenticatedOperatorApproval),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadWithdrawalRecord {
    pub request_id: String,
    pub principal_id: String,
    pub principal_label: Option<String>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadStartEvidenceRecord {
    pub request_id: String,
    pub campaign_id: String,
    pub mode: String,
    pub principal_id: String,
    pub principal_label: Option<String>,
    pub rationale: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub plan_hash: String,
    pub profile_revision_id: Option<String>,
    pub profile_hash: Option<String>,
    pub spec_sha256: String,
    pub config_hash: String,
    pub delegation_hash: String,
    pub policy_hash: String,
    pub authorization_digest: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignExecutionBindingRecord {
    pub campaign_id: String,
    pub request_id: String,
    pub plan_id: String,
    pub spec_sha256: String,
    pub envelope_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct LeadStartResult {
    pub campaign: Campaign,
    pub run: Run,
    pub attempt: Attempt,
    pub replayed: bool,
}

/// Inputs for the shared one-transaction intake creation boundary. The Action
/// remains `HUMAN_REQUIRED`/`UNASSESSED` with an immutable draft and no
/// executable spec until the frozen plan is validated.
#[derive(Debug, Clone)]
pub struct NewDraftLeadRequest<'a> {
    pub project_id: &'a str,
    pub delegation_id: &'a str,
    pub objective: &'a str,
    pub project_config_hash: &'a str,
    pub assessment_input_json: &'a str,
    pub idempotency_key: &'a str,
}

/// Validated, canonicalized draft-request inputs shared by the public method
/// and transaction-scoped intake creation.
pub(super) struct PreparedDraftLeadRequest {
    assessment_input_json: String,
    assessment_sha256: String,
    submission_digest: String,
}

pub(super) fn prepare_draft_lead_request(
    input: &NewDraftLeadRequest<'_>,
) -> Result<PreparedDraftLeadRequest, Error> {
    if input.objective.trim().is_empty()
        || input.objective.len() > 20_000
        || input.idempotency_key.trim().is_empty()
        || input.idempotency_key.len() > 256
        || !is_sha256(input.project_config_hash)
    {
        return Err(Error::Invalid("draft lead request input"));
    }
    let assessment_value: serde_json::Value = serde_json::from_str(input.assessment_input_json)
        .map_err(|_| Error::Invalid("assessment input JSON"))?;
    if !assessment_value.is_object() {
        return Err(Error::Invalid("assessment input must be a JSON object"));
    }
    let assessment_input_json = serde_json::to_string(&assessment_value)
        .map_err(|_| Error::Invalid("assessment input encoding"))?;
    if assessment_input_json.len() > 65_536 {
        return Err(Error::Invalid("assessment input is too large"));
    }
    let assessment_sha256 = sha256_hex(assessment_input_json.as_bytes());
    let submission_digest = sha256_hex(
        serde_json::to_string(&(
            "intake-lead-v1",
            input.project_id,
            input.delegation_id,
            input.objective,
            input.project_config_hash,
            &assessment_sha256,
        ))
        .expect("intake lead digest serializes")
        .as_bytes(),
    );
    Ok(PreparedDraftLeadRequest {
        assessment_input_json,
        assessment_sha256,
        submission_digest,
    })
}

/// Transaction-scoped draft Lead creation. Callers own the write transaction
/// and must never nest the public database methods.
pub(super) async fn create_draft_lead_request_in_tx(
    connection: &mut SqliteConnection,
    input: &NewDraftLeadRequest<'_>,
    prepared: &PreparedDraftLeadRequest,
) -> Result<(LeadRequestRecord, bool), Error> {
    if let Some(id) = replay(
        connection,
        input.idempotency_key,
        "create_draft_lead_request",
        &prepared.submission_digest,
    )
    .await?
    {
        let existing = load_lead_request(connection, &id).await?;
        return Ok((existing, false));
    }
    let project_config = load_project_config(connection, input.project_id).await?;
    if !project_config.active || project_config.config_hash != input.project_config_hash {
        return Err(Error::Forbidden(
            "project configuration is inactive or the supplied config hash is stale".into(),
        ));
    }
    let grant = load_delegation(connection, input.delegation_id).await?;
    grant.validate(Utc::now())?;
    if grant.project_id != input.project_id {
        return Err(Error::Forbidden(
            "delegation belongs to another project".into(),
        ));
    }
    let action_id = Uuid::new_v4().to_string();
    let objective_sha256 = sha256_hex(input.objective.as_bytes());
    let created_at = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO actions
         (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
         VALUES (?1,?2,?3,?4,?5,'UNASSESSED','HUMAN_REQUIRED',?6,?7)",
    )
    .bind(&action_id)
    .bind(input.project_id)
    .bind(input.objective)
    .bind(&grant.route.provider)
    .bind(&grant.route.model)
    .bind(&objective_sha256)
    .bind(&created_at)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "INSERT INTO action_drafts(action_id,assessment_input_json,input_sha256,created_at)
         VALUES (?1,?2,?3,?4)",
    )
    .bind(&action_id)
    .bind(&prepared.assessment_input_json)
    .bind(&prepared.assessment_sha256)
    .bind(&created_at)
    .execute(&mut *connection)
    .await?;
    reserve_usage_in_tx(connection, &grant, 1, 1, 0).await?;
    let request_hash = sha256_hex(
        serde_json::to_string(&(
            input.project_id,
            &action_id,
            input.delegation_id,
            input.objective,
            input.project_config_hash,
        ))
        .expect("lead request digest serializes")
        .as_bytes(),
    );
    let request = LeadRequestRecord {
        id: Uuid::new_v4().to_string(),
        project_id: input.project_id.into(),
        action_id: action_id.clone(),
        delegation_id: input.delegation_id.into(),
        objective: input.objective.into(),
        request_hash,
        project_config_hash: input.project_config_hash.into(),
        state: LeadRequestState::Submitted,
        assessment_mechanism: None,
        failure_reason: None,
        campaign_id: None,
        created_at: created_at.clone(),
        updated_at: created_at,
    };
    sqlx::query::query(
        "INSERT INTO lead_requests
         (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,state,failure_reason,campaign_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,'SUBMITTED',NULL,NULL,?8,?8)",
    )
    .bind(&request.id)
    .bind(&request.project_id)
    .bind(&request.action_id)
    .bind(&request.delegation_id)
    .bind(&request.objective)
    .bind(&request.request_hash)
    .bind(&request.project_config_hash)
    .bind(&request.created_at)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
        .bind(input.idempotency_key)
        .bind("create_draft_lead_request")
        .bind(&prepared.submission_digest)
        .bind(&request.id)
        .execute(&mut *connection)
        .await?;
    record_event(
        connection,
        &action_id,
        "action.created",
        "assessment draft recorded; no executable ActionSpec",
    )
    .await?;
    record_event(
        connection,
        &request.id,
        "lead.request_created",
        "assessment session reserved; no executable ActionSpec or Campaign start",
    )
    .await?;
    Ok((request, true))
}

impl Database {
    pub async fn create_lead_request(
        &self,
        project_id: &str,
        action_id: &str,
        delegation_id: &str,
        objective: &str,
        project_config_hash: &str,
        idempotency_key: &str,
    ) -> Result<(LeadRequestRecord, bool), Error> {
        if objective.trim().is_empty()
            || objective.len() > 20_000
            || idempotency_key.trim().is_empty()
            || !is_sha256(project_config_hash)
        {
            return Err(Error::Invalid("lead request input"));
        }
        let request_hash = sha256_hex(
            serde_json::to_string(&(
                project_id,
                action_id,
                delegation_id,
                objective,
                project_config_hash,
            ))
            .expect("lead request digest serializes")
            .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "create_lead_request",
            &request_hash,
        )
        .await?
        {
            let existing = load_lead_request(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok((existing, false));
        }
        let action: Option<(String, String)> =
            sqlx::query_as::query_as("SELECT project_id,status FROM actions WHERE id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let (action_project, action_status) = action.ok_or(Error::NotFound)?;
        if action_project != project_id {
            return Err(Error::Forbidden(
                "lead request action belongs to another project".into(),
            ));
        }
        if matches!(action_status.as_str(), "CANCELLED" | "COMPLETED" | "FAILED") {
            return Err(Error::Conflict("lead request action is terminal"));
        }
        let project_config = load_project_config(&mut transaction, project_id).await?;
        if !project_config.active || project_config.config_hash != project_config_hash {
            return Err(Error::Forbidden(
                "project configuration is inactive or the supplied config hash is stale".into(),
            ));
        }
        let grant = load_delegation(&mut transaction, delegation_id).await?;
        if grant.project_id != project_id {
            return Err(Error::Forbidden(
                "delegation belongs to another project".into(),
            ));
        }
        if grant.revoked_at.is_some() {
            return Err(Error::Forbidden("delegation is revoked".into()));
        }
        if grant.expires_at <= Utc::now() {
            return Err(Error::Forbidden("delegation is expired".into()));
        }
        let existing_action: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM lead_requests WHERE action_id=?1")
                .bind(action_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing_action.is_some() {
            return Err(Error::Conflict(
                "action already has a lead request; use its original request",
            ));
        }
        reserve_usage_in_tx(&mut transaction, &grant, 1, 1, 0).await?;
        let now = Utc::now().to_rfc3339();
        let request = LeadRequestRecord {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            action_id: action_id.into(),
            delegation_id: delegation_id.into(),
            objective: objective.into(),
            request_hash: request_hash.clone(),
            project_config_hash: project_config_hash.into(),
            state: LeadRequestState::Submitted,
            assessment_mechanism: None,
            failure_reason: None,
            campaign_id: None,
            created_at: now.clone(),
            updated_at: now,
        };
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,state,failure_reason,campaign_id,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9,?9)",
        )
        .bind(&request.id)
        .bind(&request.project_id)
        .bind(&request.action_id)
        .bind(&request.delegation_id)
        .bind(&request.objective)
        .bind(&request.request_hash)
        .bind(&request.project_config_hash)
        .bind(state_str(request.state))
        .bind(&request.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("create_lead_request")
            .bind(&request_hash)
            .bind(&request.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &request.id,
            "lead.request_created",
            "assessment session reserved; no model call or Campaign start",
        )
        .await?;
        transaction.commit().await?;
        Ok((request, true))
    }

    /// Create an intake Action with an immutable assessment draft and its Lead
    /// request in one transaction. No executable ActionSpec exists until freeze.
    ///
    /// Replay is resolved against the original submitted arguments before any
    /// current project, delegation or lifecycle check. This helper is the only
    /// supported way to create a draft-backed Lead request; callers cannot pair
    /// a draft Action with a second Campaign creation path.
    pub async fn create_draft_lead_request(
        &self,
        input: NewDraftLeadRequest<'_>,
    ) -> Result<(LeadRequestRecord, bool), Error> {
        let prepared = prepare_draft_lead_request(&input)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let result = create_draft_lead_request_in_tx(&mut transaction, &input, &prepared).await?;
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn action_draft(&self, action_id: &str) -> Result<ActionDraftRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_action_draft(&mut connection, action_id).await
    }

    pub async fn lead_requests(&self) -> Result<Vec<LeadRequestRecord>, Error> {
        let ids: Vec<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM lead_requests ORDER BY created_at,id")
                .fetch_all(&self.pool)
                .await?;
        let mut requests = Vec::with_capacity(ids.len());
        let mut connection = self.pool.acquire().await?;
        for (id,) in ids {
            requests.push(load_lead_request(&mut connection, &id).await?);
        }
        Ok(requests)
    }

    pub async fn lead_request(&self, id: &str) -> Result<LeadRequestRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_lead_request(&mut connection, id).await
    }

    /// Derived Office disposition with Campaign/gate precedence over the
    /// withdrawal fact and the Lead request's own progress.
    pub async fn lead_request_projection(
        &self,
        id: &str,
    ) -> Result<crate::lead::LeadRequestProjectionState, Error> {
        let request = self.lead_request(id).await?;
        let campaign_status = match &request.campaign_id {
            Some(campaign_id) => {
                let row: Option<(String,)> =
                    sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                        .bind(campaign_id)
                        .fetch_optional(&self.pool)
                        .await?;
                row.map(|(status,)| status)
            }
            None => None,
        };
        let gate_outcome = match &request.campaign_id {
            Some(campaign_id) => {
                let row: Option<(String,)> = sqlx::query_as::query_as(
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
                row.map(|(outcome,)| outcome)
            }
            None => None,
        };
        let withdrawn = self.lead_withdrawal(id).await?.is_some();
        Ok(crate::lead::project_lead_request_state(
            request.state,
            campaign_status.as_deref(),
            gate_outcome.as_deref(),
            withdrawn,
        ))
    }

    pub async fn begin_lead_assessment(&self, id: &str) -> Result<LeadRequestRecord, Error> {
        self.transition_lead_request(id, LeadRequestState::Submitted, LeadRequestState::Assessing)
            .await
    }

    pub async fn mark_lead_ready(&self, id: &str) -> Result<LeadRequestRecord, Error> {
        self.transition_lead_request(id, LeadRequestState::Frozen, LeadRequestState::Ready)
            .await
    }

    pub async fn block_lead_request(
        &self,
        id: &str,
        reason: &str,
    ) -> Result<LeadRequestRecord, Error> {
        if reason.trim().is_empty() || reason.len() > 1_000 {
            return Err(Error::Invalid("lead block reason"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let current = load_lead_request(&mut transaction, id).await?;
        if matches!(
            current.state,
            LeadRequestState::Blocked | LeadRequestState::Failed
        ) {
            transaction.commit().await?;
            return Ok(current);
        }
        let assessment: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM lead_assessments WHERE request_id=?1")
                .bind(id)
                .fetch_optional(&mut *transaction)
                .await?;
        if assessment.is_none() && current.state == LeadRequestState::Submitted {
            let grant = load_delegation(&mut transaction, &current.delegation_id).await?;
            release_usage_in_tx(&mut transaction, &grant, 1, 1, 0).await?;
        }
        sqlx::query::query(
            "UPDATE lead_requests SET state='BLOCKED',failure_reason=?1,updated_at=?2 WHERE id=?3",
        )
        .bind(reason)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        record_event(&mut transaction, id, "lead.request_blocked", reason).await?;
        let blocked = load_lead_request(&mut transaction, id).await?;
        transaction.commit().await?;
        Ok(blocked)
    }

    pub async fn record_lead_assessment(
        &self,
        request_id: &str,
        mechanism: &str,
        assessment: &LeadAssessment,
        input: &AssessmentInput,
        idempotency_key: &str,
    ) -> Result<LeadAssessmentRecord, Error> {
        if mechanism.trim().is_empty() || mechanism.len() > 128 {
            return Err(Error::Invalid("assessment mechanism"));
        }
        assessment.validate(input)?;
        let assessment_json =
            serde_json::to_string(assessment).map_err(|_| Error::Invalid("assessment encoding"))?;
        crate::model_output::validate_lead_assessment_schema(assessment_json.as_bytes())
            .map_err(|error| Error::Delegation(error.to_string()))?;
        let schema_sha256 = crate::model_output::lead_assessment_schema_sha256();
        let digest = sha256_hex(assessment_json.as_bytes());
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let request = load_lead_request(&mut transaction, request_id).await?;
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "record_lead_assessment",
            &digest,
        )
        .await?
        {
            let existing = load_assessment(&mut transaction, &id).await?;
            if existing.request_id != request.id {
                return Err(Error::Conflict(
                    "assessment idempotency key identifies another request",
                ));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        if withdrawal_in_tx(&mut transaction, request_id).await? {
            return Err(Error::Withdrawn);
        }
        if request.state != LeadRequestState::Assessing {
            return Err(Error::Conflict(
                "lead request is not in the assessing state",
            ));
        }
        if assessment.project_id != request.project_id
            || assessment.action_id != request.action_id
            || assessment.request_id != request.id
        {
            return Err(Error::Forbidden(
                "assessment identities do not match the lead request".into(),
            ));
        }
        let existing: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM lead_assessments WHERE request_id=?1")
                .bind(request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing.is_some() {
            return Err(Error::Conflict("lead assessment is write-once"));
        }
        let record = LeadAssessmentRecord {
            id: Uuid::new_v4().to_string(),
            request_id: request.id.clone(),
            mechanism: mechanism.into(),
            assessment_json,
            schema_sha256,
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query(
            "INSERT INTO lead_assessments
             (id,request_id,mechanism,assessment_json,schema_sha256,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(&record.id)
        .bind(&record.request_id)
        .bind(&record.mechanism)
        .bind(&record.assessment_json)
        .bind(&record.schema_sha256)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("record_lead_assessment")
            .bind(&digest)
            .bind(&record.id)
            .execute(&mut *transaction)
            .await?;
        update_lead_state(&mut transaction, &request, LeadRequestState::Planned).await?;
        record_event(
            &mut transaction,
            &request.id,
            "lead.assessment_recorded",
            &record.id,
        )
        .await?;
        transaction.commit().await?;
        Ok(record)
    }

    pub async fn record_lead_proposal(
        &self,
        request_id: &str,
        proposal: &LeadPlanProposal,
        idempotency_key: &str,
    ) -> Result<LeadProposalRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let request = load_lead_request(&mut transaction, request_id).await?;
        let proposal_json =
            serde_json::to_string(proposal).map_err(|_| Error::Invalid("proposal encoding"))?;
        let proposal_hash = proposal.content_hash();
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "record_lead_proposal",
            &proposal_hash,
        )
        .await?
        {
            let existing = load_proposal(&mut transaction, &id).await?;
            if existing.request_id != request.id {
                return Err(Error::Conflict(
                    "proposal idempotency key identifies another request",
                ));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        if withdrawal_in_tx(&mut transaction, request_id).await? {
            return Err(Error::Withdrawn);
        }
        if request.state != LeadRequestState::Planned {
            return Err(Error::Conflict("lead request is not ready for a proposal"));
        }
        let grant = load_delegation(&mut transaction, &request.delegation_id).await?;
        if proposal.project_id != request.project_id
            || proposal.action_id != request.action_id
            || proposal.project_config_hash != request.project_config_hash
        {
            return Err(Error::Forbidden(
                "proposal binding does not match the lead request".into(),
            ));
        }
        let config = load_project_config(&mut transaction, &request.project_id).await?;
        if proposal.base_revision != config.base_commit {
            return Err(Error::Forbidden(
                "proposal base revision is not the current registered base".into(),
            ));
        }
        let action = load_action_binding(&mut transaction, &request.action_id).await?;
        proposal.validate(&grant, &config.binding(), &action, Utc::now())?;
        let assessment: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM lead_assessments WHERE request_id=?1")
                .bind(request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let (assessment_id,) = assessment.ok_or(Error::Conflict(
            "lead proposal requires a recorded assessment",
        ))?;
        let existing: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM lead_proposals WHERE request_id=?1")
                .bind(request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing.is_some() {
            return Err(Error::Conflict("lead proposal is write-once"));
        }
        let record = LeadProposalRecord {
            id: Uuid::new_v4().to_string(),
            request_id: request.id.clone(),
            assessment_id,
            proposal_json,
            proposal_hash: proposal_hash.clone(),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query(
            "INSERT INTO lead_proposals
             (id,request_id,assessment_id,proposal_json,proposal_hash,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(&record.id)
        .bind(&record.request_id)
        .bind(&record.assessment_id)
        .bind(&record.proposal_json)
        .bind(&record.proposal_hash)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("record_lead_proposal")
            .bind(&proposal_hash)
            .bind(&record.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &request.id,
            "lead.proposal_recorded",
            &record.id,
        )
        .await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Freeze the plan and, when the Action still has only an assessment draft,
    /// derive and insert its first executable ActionSpec in the same
    /// transaction. Existing specs are never replaced or mutated.
    pub async fn freeze_lead_plan(
        &self,
        request_id: &str,
        idempotency_key: &str,
    ) -> Result<FrozenLeadPlanRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let record = freeze_lead_plan_in_tx(&mut transaction, request_id, idempotency_key).await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Recoverable freeze/READY boundary. A committed freeze is replayed before
    /// any lifecycle check, and a FROZEN request can be advanced to READY by a
    /// retry without reserving sessions or deriving a second spec.
    pub async fn freeze_lead_plan_to_ready(
        &self,
        request_id: &str,
        idempotency_key: &str,
    ) -> Result<(FrozenLeadPlanRecord, LeadRequestRecord), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let record = freeze_lead_plan_in_tx(&mut transaction, request_id, idempotency_key).await?;
        if withdrawal_in_tx(&mut transaction, request_id).await? {
            return Err(Error::Withdrawn);
        }
        let current = load_lead_request(&mut transaction, request_id).await?;
        let request = match current.state {
            LeadRequestState::Frozen => {
                update_lead_state(&mut transaction, &current, LeadRequestState::Ready).await?;
                record_event(
                    &mut transaction,
                    request_id,
                    "lead.state_changed",
                    "Frozen->Ready",
                )
                .await?;
                load_lead_request(&mut transaction, request_id).await?
            }
            LeadRequestState::Ready => current,
            _ => return Err(Error::Conflict("lead plan is not ready to freeze")),
        };
        transaction.commit().await?;
        Ok((record, request))
    }

    /// Start a frozen Lead plan under Core-constructed authority.
    ///
    /// Autonomous starts retain the deployment flag, active delegation,
    /// autonomous-start permission and R0 scope. Operator starts permit a
    /// non-autonomous delegation after an authenticated request-scoped
    /// approval but retain every common scope, risk, expiry, route, budget,
    /// execution-mode and frozen-identity check. Evidence, authorization,
    /// Campaign, Run and Attempt commit in one transaction.
    pub async fn start_lead_campaign(
        &self,
        request_id: &str,
        idempotency_key: &str,
        authority: StartAuthority,
        expected: Option<&ExpectedStartBinding>,
        authorization: Option<&NewCampaignAuthorization>,
    ) -> Result<LeadStartResult, Error> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(Error::Invalid("lead start idempotency key"));
        }
        if matches!(authority, StartAuthority::Operator(_)) && expected.is_none() {
            return Err(Error::Invalid(
                "operator approval requires an exact expected start binding",
            ));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = start_lead_campaign_in_tx(
            &mut transaction,
            &self.provider_model_cache(),
            request_id,
            idempotency_key,
            &authority,
            expected,
            authorization,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn cancel_campaign(
        &self,
        campaign_id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<Campaign, Error> {
        if actor.trim().is_empty() || actor.len() > 128 || reason.len() > 500 {
            return Err(Error::Invalid("campaign cancellation identity or reason"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT id,action_id,status,created_at FROM campaigns WHERE id=?1",
        )
        .bind(campaign_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (id, action_id, status, created_at) = row.ok_or(Error::NotFound)?;
        if status == "CANCELLED" {
            transaction.commit().await?;
            return Ok(Campaign {
                id,
                action_id,
                status,
                created_at,
            });
        }
        if matches!(status.as_str(), "COMPLETED" | "FAILED") {
            return Err(Error::Conflict("campaign is already terminal"));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query::query("UPDATE campaigns SET status='CANCELLED' WHERE id=?1")
            .bind(&id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query(
            "UPDATE runs SET status='CANCELLED'
             WHERE campaign_id=?1 AND status IN ('QUEUED','RUNNING')",
        )
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE attempts SET status='CANCELLED'
             WHERE status IN ('QUEUED','LEASED')
               AND run_id IN (SELECT id FROM runs WHERE campaign_id=?1)",
        )
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE lead_tasks SET status='CANCELLED',updated_at=?2
             WHERE campaign_id=?1 AND status IN ('PENDING','RUNNING')",
        )
        .bind(&id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE lead_integrations SET status='CANCELLED',updated_at=?2
             WHERE campaign_id=?1 AND status IN ('QUEUED','RUNNING')",
        )
        .bind(&id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE lead_requests SET state='BLOCKED',failure_reason=?2,updated_at=?3
             WHERE campaign_id=?1 AND state='RUNNING'",
        )
        .bind(&id)
        .bind(format!("campaign cancelled: {}", reason.trim()))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "UPDATE provider_sessions
             SET state='CANCEL_REQUESTED',
                 error='campaign cancelled; external termination unconfirmed',
                 updated_at=?2
             WHERE state IN ('INTENT','ACTIVE','ORPHANED')
               AND attempt_id IN (
                 SELECT a.id FROM attempts a
                 JOIN runs r ON r.id=a.run_id
                 WHERE r.campaign_id=?1
               )",
        )
        .bind(&id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query(
            "DELETE FROM worker_leases
             WHERE attempt_id IN (
               SELECT a.id FROM attempts a
               JOIN runs r ON r.id=a.run_id
               WHERE r.campaign_id=?1
             )",
        )
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &id,
            "campaign.cancelled",
            &format!("cancelled by {}: {}", actor, reason.trim()),
        )
        .await?;
        transaction.commit().await?;
        Ok(Campaign {
            id,
            action_id,
            status: "CANCELLED".into(),
            created_at,
        })
    }

    pub async fn campaign_tasks(&self, campaign_id: &str) -> Result<Vec<LeadTaskRecord>, Error> {
        use sqlx::row::Row;
        let rows = sqlx::query::query(
            "SELECT t.task_id,t.campaign_id,t.request_id,t.unit_id,t.sequence,t.status,
                    t.base_commit,t.depends_on_json,t.run_id,t.attempt_id,t.candidate_id,
                    c.commit_sha,c.tree_sha,t.updated_at
             FROM lead_tasks t
             LEFT JOIN candidates c ON c.id=t.candidate_id
             WHERE t.campaign_id=?1 ORDER BY t.sequence",
        )
        .bind(campaign_id)
        .fetch_all(&self.pool)
        .await?;
        let mut tasks = Vec::with_capacity(rows.len());
        for row in rows {
            let depends: String = row.try_get("depends_on_json")?;
            tasks.push(LeadTaskRecord {
                task_id: row.try_get("task_id")?,
                campaign_id: row.try_get("campaign_id")?,
                request_id: row.try_get("request_id")?,
                unit_id: row.try_get("unit_id")?,
                sequence: row.try_get("sequence")?,
                status: row.try_get("status")?,
                base_commit: row.try_get("base_commit")?,
                depends_on: serde_json::from_str(&depends)
                    .map_err(|_| Error::Invalid("stored task dependencies"))?,
                run_id: row.try_get("run_id")?,
                attempt_id: row.try_get("attempt_id")?,
                candidate_id: row.try_get("candidate_id")?,
                candidate_commit: row.try_get("commit_sha")?,
                candidate_tree: row.try_get("tree_sha")?,
                updated_at: row.try_get("updated_at")?,
            });
        }
        Ok(tasks)
    }

    /// Record the authoritative operator withdrawal fact. This is not a
    /// lifecycle transition and never cancels an already-authorized Campaign.
    /// Exact replay by the same authenticated principal and reason is
    /// idempotent; a start that committed first returns a typed conflict with
    /// the Campaign identity.
    pub async fn withdraw_lead_request(
        &self,
        request_id: &str,
        principal_id: &str,
        principal_label: Option<&str>,
        reason: &str,
    ) -> Result<LeadWithdrawalRecord, Error> {
        if principal_id.trim().is_empty()
            || principal_id.len() > 200
            || reason.trim().is_empty()
            || reason.len() > 1_000
            || principal_label.is_some_and(|label| label.trim().is_empty() || label.len() > 200)
        {
            return Err(Error::Invalid("lead withdrawal identity or reason"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let request = load_lead_request(&mut transaction, request_id).await?;
        if let Some(existing) = load_withdrawal(&mut transaction, request_id).await? {
            if existing.principal_id == principal_id
                && existing.principal_label.as_deref() == principal_label
                && existing.reason == reason
            {
                transaction.commit().await?;
                return Ok(existing);
            }
            return Err(Error::Conflict(
                "lead request already has a different withdrawal",
            ));
        }
        if let Some(evidence) = load_start_evidence(&mut transaction, request_id).await? {
            let status: (String,) =
                sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                    .bind(&evidence.campaign_id)
                    .fetch_one(&mut *transaction)
                    .await?;
            return Err(Error::AlreadyStarted {
                campaign_id: evidence.campaign_id,
                campaign_status: status.0,
            });
        }
        if let Some(campaign_id) = &request.campaign_id {
            let status: (String,) =
                sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                    .bind(campaign_id)
                    .fetch_one(&mut *transaction)
                    .await?;
            return Err(Error::AlreadyStarted {
                campaign_id: campaign_id.clone(),
                campaign_status: status.0,
            });
        }
        let record = LeadWithdrawalRecord {
            request_id: request.id.clone(),
            principal_id: principal_id.into(),
            principal_label: principal_label.map(str::to_owned),
            reason: reason.into(),
            created_at: Utc::now().to_rfc3339(),
        };
        sqlx::query::query(
            "INSERT INTO lead_withdrawals
             (request_id,principal_id,principal_label,reason,created_at)
             VALUES (?1,?2,?3,?4,?5)",
        )
        .bind(&record.request_id)
        .bind(&record.principal_id)
        .bind(&record.principal_label)
        .bind(&record.reason)
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            request_id,
            "lead.withdrawn",
            &format!("principal={} reason={}", record.principal_id, record.reason),
        )
        .await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Exact execution-spec identity for one Attempt. Lead WorkUnits have a
    /// unit-specific spec while the Campaign has a separate root spec; readers
    /// must compare against this exact row, not only the root ActionSpec.
    pub async fn attempt_spec_identity(&self, attempt_id: &str) -> Result<(String, String), Error> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query_as::query_as(
            "SELECT action_id,spec_sha256 FROM attempt_specs WHERE attempt_id=?1",
        )
        .bind(attempt_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or_else(|| Error::Forbidden("Attempt has no exact execution spec".into()))
    }

    /// Revalidate the frozen WorkGraph/spec lineage for a Lead Campaign
    /// Attempt. Non-Lead/legacy Attempts without a Campaign execution binding
    /// remain valid.
    pub async fn validate_attempt_lineage(&self, attempt_id: &str) -> Result<(), Error> {
        let mut connection = self.pool.acquire().await?;
        validate_lead_attempt_lineage(&mut connection, attempt_id).await
    }

    pub async fn lead_withdrawal(
        &self,
        request_id: &str,
    ) -> Result<Option<LeadWithdrawalRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_withdrawal(&mut connection, request_id).await
    }

    pub async fn lead_start_evidence(
        &self,
        request_id: &str,
    ) -> Result<Option<LeadStartEvidenceRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_start_evidence(&mut connection, request_id).await
    }

    pub async fn campaign_execution_binding(
        &self,
        campaign_id: &str,
    ) -> Result<Option<CampaignExecutionBindingRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        load_campaign_execution_binding(&mut connection, campaign_id).await
    }

    pub async fn lead_assessment(&self, request_id: &str) -> Result<LeadAssessmentRecord, Error> {
        let row: Option<(String, String, String, String, String, String)> =
            sqlx::query_as::query_as(
                "SELECT id,request_id,mechanism,assessment_json,schema_sha256,created_at
                 FROM lead_assessments WHERE request_id=?1",
            )
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(
            |(id, request_id, mechanism, assessment_json, schema_sha256, created_at)| {
                LeadAssessmentRecord {
                    id,
                    request_id,
                    mechanism,
                    assessment_json,
                    schema_sha256,
                    created_at,
                }
            },
        )
        .ok_or(Error::NotFound)
    }

    pub async fn lead_proposal(&self, request_id: &str) -> Result<LeadProposalRecord, Error> {
        let row: Option<(String, String, String, String, String, String)> =
            sqlx::query_as::query_as(
                "SELECT id,request_id,assessment_id,proposal_json,proposal_hash,created_at
                 FROM lead_proposals WHERE request_id=?1",
            )
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(
            |(id, request_id, assessment_id, proposal_json, proposal_hash, created_at)| {
                LeadProposalRecord {
                    id,
                    request_id,
                    assessment_id,
                    proposal_json,
                    proposal_hash,
                    created_at,
                }
            },
        )
        .ok_or(Error::NotFound)
    }

    pub async fn frozen_lead_plan(&self, request_id: &str) -> Result<FrozenLeadPlanRecord, Error> {
        let row: Option<(String, String, String, String, String, String, String)> =
            sqlx::query_as::query_as(
                "SELECT id,request_id,proposal_id,plan_hash,config_hash,delegation_hash,created_at
                 FROM frozen_lead_plans WHERE request_id=?1",
            )
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(
            |(id, request_id, proposal_id, plan_hash, config_hash, delegation_hash, created_at)| {
                FrozenLeadPlanRecord {
                    id,
                    request_id,
                    proposal_id,
                    plan_hash,
                    config_hash,
                    delegation_hash,
                    created_at,
                }
            },
        )
        .ok_or(Error::NotFound)
    }

    async fn transition_lead_request(
        &self,
        id: &str,
        from: LeadRequestState,
        to: LeadRequestState,
    ) -> Result<LeadRequestRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_lead_request(&mut transaction, id).await?;
        if withdrawal_in_tx(&mut transaction, id).await? {
            return Err(Error::Withdrawn);
        }
        if current.state != from {
            return Err(Error::Conflict("lead request is in another state"));
        }
        update_lead_state(&mut transaction, &current, to).await?;
        record_event(
            &mut transaction,
            id,
            "lead.state_changed",
            &format!("{:?}->{:?}", from, to),
        )
        .await?;
        let updated = load_lead_request(&mut transaction, id).await?;
        transaction.commit().await?;
        Ok(updated)
    }
}

struct StartIdentity {
    plan: FrozenLeadPlanRecord,
    proposal: LeadPlanProposal,
    grant: StandingDelegation,
    action_spec: ActionSpec,
    spec_sha256: String,
    request_hash: String,
    plan_hash: String,
    config_hash: String,
    delegation_hash: String,
    profile: Option<LeadExecutionContextRecord>,
    profile_content: Option<crate::execution_profile::ExecutionProfileContent>,
}

async fn load_start_identity(
    connection: &mut SqliteConnection,
    request: &LeadRequestRecord,
) -> Result<StartIdentity, Error> {
    let plan = sqlx::query_as::query_as::<_, (String,)>(
        "SELECT id FROM frozen_lead_plans WHERE request_id=?1",
    )
    .bind(&request.id)
    .fetch_optional(&mut *connection)
    .await?
    .map(|(id,)| id)
    .ok_or(Error::Conflict("lead plan is not frozen"))?;
    let plan = load_frozen_plan_by_id(connection, &plan).await?;
    let proposal: (String,) =
        sqlx::query_as::query_as("SELECT proposal_json FROM lead_proposals WHERE id=?1")
            .bind(&plan.proposal_id)
            .fetch_one(&mut *connection)
            .await?;
    let proposal: LeadPlanProposal = serde_json::from_str(&proposal.0)
        .map_err(|_| Error::Invalid("stored proposal encoding"))?;
    let grant = load_delegation(connection, &request.delegation_id).await?;
    if plan.config_hash != request.project_config_hash || plan.delegation_hash != grant.content_hash
    {
        return Err(Error::Invalid(
            "frozen plan binding does not match the Lead request",
        ));
    }
    let profile = load_execution_context(connection, &request.id).await?;
    let profile_content = match &profile {
        Some(context) => {
            if context.project_id != request.project_id
                || context.project_config_hash != request.project_config_hash
                || context.delegation_hash != grant.content_hash
                || sha256_hex(context.context_json.as_bytes()) != context.context_hash
            {
                return Err(Error::Invalid(
                    "frozen execution context does not match the Lead request",
                ));
            }
            Some(load_frozen_profile_content(connection, context).await?)
        }
        None => None,
    };
    let request_hash = sha256_hex(
        serde_json::to_string(&(
            &request.project_id,
            &request.action_id,
            &request.delegation_id,
            &request.objective,
            &request.project_config_hash,
        ))
        .expect("lead request digest serializes")
        .as_bytes(),
    );
    if request_hash != request.request_hash {
        return Err(Error::Invalid("Lead request hash mismatch"));
    }
    let recomputed_plan_hash = plan_hash(&proposal, &grant, &request.project_config_hash);
    if recomputed_plan_hash != plan.plan_hash {
        return Err(Error::Invalid("frozen Lead plan hash mismatch"));
    }
    let (spec_json, spec_sha256): (String, String) = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
    )
    .bind(&request.action_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(Error::Conflict("action has no frozen spec"))?;
    if sha256_hex(spec_json.as_bytes()) != spec_sha256 {
        return Err(Error::Invalid("frozen ActionSpec digest mismatch"));
    }
    let action_spec: ActionSpec = serde_json::from_str(&spec_json)
        .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
    let delegation_hash = grant.content_hash.clone();
    Ok(StartIdentity {
        plan,
        proposal,
        grant,
        action_spec,
        spec_sha256,
        request_hash,
        plan_hash: recomputed_plan_hash,
        config_hash: request.project_config_hash.clone(),
        delegation_hash,
        profile,
        profile_content,
    })
}

fn start_submission_digest(
    request_id: &str,
    authority: &StartAuthority,
    expected: Option<&ExpectedStartBinding>,
    authorization: Option<&NewCampaignAuthorization>,
) -> String {
    let authority = match authority {
        StartAuthority::Autonomous(_) => serde_json::json!({"mode":"AUTONOMOUS"}),
        StartAuthority::Operator(approval) => serde_json::json!({
            "mode":"OPERATOR",
            "principalId":approval.principal_id(),
            "principalLabel":approval.principal_label(),
            "rationale":approval.rationale(),
        }),
    };
    sha256_hex(
        serde_json::to_string(&serde_json::json!({
            "schema":"s4d-lead-start-submission-v3",
            "requestId":request_id,
            "authority":authority,
            "expected":expected,
            "authorization":authorization,
        }))
        .expect("lead start submission serializes")
        .as_bytes(),
    )
}

fn expected_binding_matches(expected: &ExpectedStartBinding, identity: &StartIdentity) -> bool {
    expected.request_hash == identity.request_hash
        && expected.plan_hash == identity.plan_hash
        && expected.spec_sha256 == identity.spec_sha256
        && expected.config_hash == identity.config_hash
        && expected.delegation_hash == identity.delegation_hash
        && match expected.profile_binding() {
            None => identity.profile.is_none(),
            Some((revision_id, hash)) => identity.profile.as_ref().is_some_and(|profile| {
                profile.profile_revision_id == revision_id && profile.profile_hash == hash
            }),
        }
}

fn validate_profile_authorization(
    input: &NewCampaignAuthorization,
    profile: &crate::execution_profile::ExecutionProfileContent,
    proposal: &LeadPlanProposal,
    grant: &StandingDelegation,
    action_spec: &ActionSpec,
    mechanism: &str,
) -> Result<(), Error> {
    let expected_mode = match mechanism {
        "MODEL" => "TRUSTED_LOCAL_PROVIDER",
        "FIXTURE" => "FIXTURE",
        _ => return Err(Error::Invalid("stored lead assessment mechanism")),
    };
    if input.execution_mode != expected_mode {
        return Err(Error::Forbidden(
            "authorization execution mode differs from the frozen assessment mechanism".into(),
        ));
    }
    if input.risk_class != "R0" {
        return Err(Error::Forbidden(
            "operator approval is limited to R0 execution".into(),
        ));
    }
    if input.delegation_id != grant.id {
        return Err(Error::Forbidden(
            "authorization delegation differs from the frozen request".into(),
        ));
    }
    if grant.risk_classes != vec![crate::lead::RiskClass::R0]
        || grant.autonomous_start
        || !grant.autonomous_risk_classes.is_empty()
        || grant.max_attempts > profile.budgets.max_attempts
        || grant.max_sessions > profile.budgets.max_sessions
        || u64::from(grant.max_timeout_secs) > profile.budgets.deadline_secs
        || u64::from(grant.max_wall_clock_secs) > profile.budgets.deadline_secs
        || grant.max_parallel != 1
        || grant.max_repair_cycles != 0
    {
        return Err(Error::Forbidden(
            "execution policy delegation exceeds the frozen profile authority".into(),
        ));
    }
    if input.provider != profile.roles.implementer.provider
        || input.model != profile.roles.implementer.model
        || input.provider_mode != "build"
    {
        return Err(Error::Forbidden(
            "authorization route differs from the frozen profile implementer route".into(),
        ));
    }
    if input.max_attempts == 0
        || input.max_attempts > profile.budgets.max_attempts
        || input.max_attempts > proposal.budget.max_attempts
        || input.max_attempts > grant.max_attempts
        || input.max_attempts > 5
    {
        return Err(Error::Forbidden(
            "authorization attempt budget exceeds the frozen profile, plan or delegation".into(),
        ));
    }
    if input.max_sessions == 0
        || input.max_sessions > profile.budgets.max_sessions
        || input.max_sessions > proposal.max_sessions.saturating_sub(1)
        || input.max_sessions > grant.max_sessions
        || input.max_sessions > 2
    {
        return Err(Error::Forbidden(
            "authorization session budget exceeds the frozen profile, plan or delegation".into(),
        ));
    }
    if input.max_turns == 0 || input.max_turns > profile.budgets.max_turns || input.max_turns > 4 {
        return Err(Error::Forbidden(
            "authorization turn budget exceeds the frozen profile".into(),
        ));
    }
    if input.max_input_tokens == 0
        || input.max_input_tokens > profile.budgets.max_input_tokens
        || input.max_input_tokens > profile.roles.implementer.max_input_tokens
    {
        return Err(Error::Forbidden(
            "authorization input-token budget exceeds the frozen profile or implementer route"
                .into(),
        ));
    }
    if input.max_output_tokens == 0
        || input.max_output_tokens > profile.budgets.max_output_tokens
        || input.max_output_tokens > profile.roles.implementer.max_output_tokens
    {
        return Err(Error::Forbidden(
            "authorization output-token budget exceeds the frozen profile or implementer route"
                .into(),
        ));
    }
    if input.max_cost_micros == 0
        || input.max_cost_micros > profile.budgets.max_cost_micros
        || input.max_cost_micros > profile.roles.implementer.max_cost_micros
    {
        return Err(Error::Forbidden(
            "authorization cost budget exceeds the frozen profile or implementer route".into(),
        ));
    }
    if input.deadline_secs < 30
        || input.deadline_secs > profile.budgets.deadline_secs
        || input.deadline_secs > u64::from(proposal.max_wall_clock_secs)
        || input.deadline_secs > u64::from(grant.max_wall_clock_secs)
        || input.deadline_secs > 600
    {
        return Err(Error::Forbidden(
            "authorization deadline exceeds the frozen profile, plan or delegation".into(),
        ));
    }
    if input.authorized_paths.is_empty()
        || input.authorized_paths.iter().any(|path| {
            !crate::workspace::path_covered(path, &action_spec.allowed_paths)
                || !crate::workspace::path_covered(path, &grant.allowed_paths)
        })
    {
        return Err(Error::Forbidden(
            "authorization paths exceed the frozen ActionSpec or delegation".into(),
        ));
    }
    if input.execution_mode == "FIXTURE" && !input.allowed_tools.is_empty() {
        return Err(Error::Forbidden(
            "fixture authorization cannot grant provider tools".into(),
        ));
    }
    if input.execution_mode == "TRUSTED_LOCAL_PROVIDER" && input.allowed_tools.is_empty() {
        return Err(Error::Forbidden(
            "provider authorization requires explicit bounded tools".into(),
        ));
    }
    Ok(())
}

fn evidence_matches_identity(evidence: &LeadStartEvidenceRecord, identity: &StartIdentity) -> bool {
    evidence.request_hash == identity.request_hash
        && evidence.plan_hash == identity.plan_hash
        && evidence.spec_sha256 == identity.spec_sha256
        && evidence.config_hash == identity.config_hash
        && evidence.delegation_hash == identity.delegation_hash
        && match &identity.profile {
            None => evidence.profile_revision_id.is_none() && evidence.profile_hash.is_none(),
            Some(profile) => {
                evidence.profile_revision_id.as_deref()
                    == Some(profile.profile_revision_id.as_str())
                    && evidence.profile_hash.as_deref() == Some(profile.profile_hash.as_str())
            }
        }
}

fn evidence_authority_matches(
    evidence: &LeadStartEvidenceRecord,
    mode: &str,
    principal_id: &str,
    principal_label: Option<&str>,
    rationale: Option<&str>,
    submission_digest: &str,
) -> bool {
    evidence.mode == mode
        && evidence.principal_id == principal_id
        && evidence.principal_label.as_deref() == principal_label
        && rationale.is_none_or(|rationale| evidence.rationale == rationale)
        && evidence.authorization_digest == submission_digest
}

async fn start_lead_campaign_in_tx(
    connection: &mut SqliteConnection,
    cache: &crate::provider_catalog::ProviderModelCache,
    request_id: &str,
    idempotency_key: &str,
    authority: &StartAuthority,
    expected: Option<&ExpectedStartBinding>,
    authorization: Option<&NewCampaignAuthorization>,
) -> Result<LeadStartResult, Error> {
    let request = load_lead_request(connection, request_id).await?;
    let submission_digest = start_submission_digest(request_id, authority, expected, authorization);

    if let Some(evidence) = load_start_evidence_by_key(connection, idempotency_key).await? {
        let identity = load_start_identity(connection, &request).await?;
        let (mode, principal_id, principal_label, rationale) = match authority {
            StartAuthority::Autonomous(_) => (
                "AUTONOMOUS",
                identity.grant.principal_id.as_str(),
                None,
                None,
            ),
            StartAuthority::Operator(approval) => (
                "OPERATOR",
                approval.principal_id(),
                approval.principal_label(),
                Some(approval.rationale()),
            ),
        };
        if evidence.request_id != request.id
            || !evidence_authority_matches(
                &evidence,
                mode,
                principal_id,
                principal_label,
                rationale,
                &submission_digest,
            )
            || !evidence_matches_identity(&evidence, &identity)
            || expected.is_some_and(|binding| !expected_binding_matches(binding, &identity))
        {
            return Err(Error::Conflict(
                "start idempotency key was used with different approval or binding",
            ));
        }
        let campaign = load_campaign(connection, &evidence.campaign_id).await?;
        let run = load_run_for_campaign(connection, &campaign.id).await?;
        let attempt = load_attempt_for_run(connection, &run.id).await?;
        return Ok(LeadStartResult {
            campaign,
            run,
            attempt,
            replayed: true,
        });
    }

    if let Some(evidence) = load_start_evidence(connection, request_id).await? {
        let status: (String,) =
            sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                .bind(&evidence.campaign_id)
                .fetch_one(&mut *connection)
                .await?;
        return Err(Error::AlreadyStarted {
            campaign_id: evidence.campaign_id,
            campaign_status: status.0,
        });
    }
    if let Some(campaign_id) = &request.campaign_id {
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
    if withdrawal_in_tx(connection, request_id).await? {
        return Err(Error::Withdrawn);
    }
    if request.state != LeadRequestState::Ready {
        return Err(Error::Conflict("lead plan is not ready to start"));
    }

    let identity = load_start_identity(connection, &request).await?;
    let proposal = &identity.proposal;
    let grant = &identity.grant;
    let config = load_project_config(connection, &request.project_id).await?;
    let mechanism: String =
        sqlx::query_as::query_as("SELECT mechanism FROM lead_assessments WHERE request_id=?1")
            .bind(request_id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|(mechanism,): (String,)| mechanism)
            .ok_or(Error::Conflict("lead request has no assessment"))?;
    if mechanism == "MODEL"
        && let Some(profile) = &identity.profile_content
    {
        let availability =
            super::execution_profiles::load_provider_availability(connection, cache).await?;
        let (executable, warnings) =
            crate::execution_profile::evaluate_executability(profile, &availability);
        if !executable {
            return Err(Error::Forbidden(format!(
                "frozen execution profile is not executable: {}",
                warnings.join("; ")
            )));
        }
    }
    if !config.active
        || config.config_hash != request.project_config_hash
        || proposal.base_revision != config.base_commit
    {
        return Err(Error::Forbidden(
            "project configuration or base changed after the Lead request".into(),
        ));
    }
    if proposal.work_units.is_empty() {
        return Err(Error::Forbidden("WorkGraph is empty".into()));
    }
    if proposal.risk_class != crate::lead::RiskClass::R0 {
        return Err(Error::Forbidden(
            "explicit start is limited to a deterministically eligible R0 plan".into(),
        ));
    }
    let action = load_action_binding(connection, &request.action_id).await?;
    if action.status != crate::lead::ActionStatus::Pending {
        return Err(Error::Forbidden("action is not startable".into()));
    }
    let action_classification: (String, String) =
        sqlx::query_as::query_as("SELECT risk,status FROM actions WHERE id=?1")
            .bind(&request.action_id)
            .fetch_one(&mut *connection)
            .await?;
    match authority {
        StartAuthority::Autonomous(_) => {
            grant.validate(Utc::now())?;
            if !grant.autonomous_start
                || !grant
                    .autonomous_risk_classes
                    .contains(&crate::lead::RiskClass::R0)
            {
                return Err(Error::Forbidden(
                    "delegation does not authorize autonomous R0 start".into(),
                ));
            }
            if action_classification.0 != "R0" {
                return Err(Error::Forbidden(
                    "action must be explicitly classified R0 before autonomous start".into(),
                ));
            }
        }
        StartAuthority::Operator(_) => {
            grant.validate(Utc::now())?;
            if !matches!(action_classification.0.as_str(), "R0" | "UNASSESSED") {
                return Err(Error::Forbidden(
                    "action risk is incompatible with operator R0 approval".into(),
                ));
            }
        }
    }
    proposal.validate(grant, &config.binding(), &action, Utc::now())?;
    if proposal.objective != request.objective {
        return Err(Error::Forbidden(
            "proposal objective differs from the original Lead request".into(),
        ));
    }
    match (
        &identity.profile,
        expected.and_then(ExpectedStartBinding::profile_binding),
    ) {
        (Some(_), None) => {
            return Err(Error::Forbidden(
                "this Lead request requires an exact frozen profile binding".into(),
            ));
        }
        (None, Some(_)) => {
            return Err(Error::Forbidden(
                "this Lead request has no frozen execution profile context".into(),
            ));
        }
        _ => {}
    }
    if matches!(authority, StartAuthority::Autonomous(_)) && identity.profile.is_some() {
        return Err(Error::Forbidden(
            "a unified-intake request can start only through explicit operator approval".into(),
        ));
    }
    if let Some(expected) = expected
        && !expected_binding_matches(expected, &identity)
    {
        return Err(Error::Conflict(
            "expected Lead start binding is stale or changed",
        ));
    }
    if let (Some(profile), Some(binding)) = (&identity.profile, expected) {
        let Some((revision_id, hash)) = binding.profile_binding() else {
            return Err(Error::Forbidden(
                "this Lead request requires an exact frozen profile binding".into(),
            ));
        };
        if profile.profile_revision_id != revision_id || profile.profile_hash != hash {
            return Err(Error::Conflict(
                "expected execution profile revision is stale or changed",
            ));
        }
    }
    let derived_from_draft: Option<(String,)> =
        sqlx::query_as::query_as("SELECT action_id FROM action_drafts WHERE action_id=?1")
            .bind(&request.action_id)
            .fetch_optional(&mut *connection)
            .await?;
    let mut expected_checks = if derived_from_draft.is_some() {
        complete_plan_checks(proposal)?
    } else {
        proposal.acceptance_criteria.clone()
    };
    if derived_from_draft.is_none() {
        expected_checks.extend(proposal.negative_cases.clone());
        expected_checks.sort_by(|left, right| left.id.cmp(&right.id));
    }
    let mut actual_checks = identity.action_spec.checks.clone();
    actual_checks.sort_by(|left, right| left.id.cmp(&right.id));
    if expected_checks != actual_checks {
        return Err(Error::Forbidden(
            "frozen plan does not match the ActionSpec checks".into(),
        ));
    }
    for unit in &proposal.work_units {
        if unit
            .allowed_paths
            .iter()
            .any(|path| !crate::workspace::path_covered(path, &identity.action_spec.allowed_paths))
        {
            return Err(Error::Forbidden(
                "a WorkUnit path widens beyond the frozen ActionSpec".into(),
            ));
        }
    }
    let mut specs = Vec::with_capacity(proposal.work_units.len());
    for unit in &proposal.work_units {
        specs.push(unit_action_spec(
            proposal,
            &config,
            grant,
            unit,
            &config.base_commit,
        )?);
    }
    let mut authorization_input = match mechanism.as_str() {
        "MODEL" => {
            let input = authorization.ok_or(Error::Invalid(
                "MODEL Lead start requires an explicit TRUSTED_LOCAL_PROVIDER authorization payload",
            ))?;
            if input.execution_mode != "TRUSTED_LOCAL_PROVIDER" {
                return Err(Error::Invalid(
                    "MODEL Lead start requires TRUSTED_LOCAL_PROVIDER authorization",
                ));
            }
            if input.delegation_id != request.delegation_id {
                return Err(Error::Forbidden(
                    "authorization delegation differs from the frozen Lead request".into(),
                ));
            }
            if input.provider != proposal.route.provider
                || input.model != proposal.route.model
                || input.provider_mode != proposal.route.mode
            {
                return Err(Error::Forbidden(
                    "authorization route differs from the frozen proposal route".into(),
                ));
            }
            if input.max_attempts > proposal.budget.max_attempts {
                return Err(Error::Forbidden(
                    "authorization attempt budget exceeds the frozen plan".into(),
                ));
            }
            if input.max_sessions > proposal.max_sessions.saturating_sub(1) {
                return Err(Error::Forbidden(
                    "authorization session budget exceeds the frozen plan".into(),
                ));
            }
            if input.authorized_paths.iter().any(|path| {
                !crate::workspace::path_covered(path, &identity.action_spec.allowed_paths)
            }) {
                return Err(Error::Forbidden(
                    "authorization paths exceed the frozen ActionSpec".into(),
                ));
            }
            input.clone()
        }
        "FIXTURE" => {
            if let Some(input) = authorization {
                if identity.profile.is_none() {
                    return Err(Error::Invalid(
                        "FIXTURE Lead start without a frozen profile does not accept an authorization payload",
                    ));
                }
                input.clone()
            } else {
                NewCampaignAuthorization {
                    authorization_id: format!("lead-{}", request.id),
                    risk_class: "R0".into(),
                    execution_mode: "FIXTURE".into(),
                    actor: "lead-autonomous-start".into(),
                    rationale: format!(
                        "autonomous R0 FIXTURE start for plan {} by delegation {}",
                        identity.plan.plan_hash, grant.id
                    ),
                    authorized_paths: identity.action_spec.allowed_paths.clone(),
                    max_attempts: proposal.budget.max_attempts,
                    provider: proposal.route.provider.clone(),
                    model: proposal.route.model.clone(),
                    provider_mode: proposal.route.mode.clone(),
                    delegation_id: grant.id.clone(),
                    ..Default::default()
                }
            }
        }
        _ => return Err(Error::Invalid("stored lead assessment mechanism")),
    };
    if let (Some(profile), Some(_context)) = (&identity.profile_content, &identity.profile) {
        validate_profile_authorization(
            &authorization_input,
            profile,
            proposal,
            grant,
            &identity.action_spec,
            &mechanism,
        )?;
    }
    let (mode, principal_id, principal_label, rationale) = match authority {
        StartAuthority::Autonomous(_) => (
            "AUTONOMOUS",
            grant.principal_id.clone(),
            None,
            authorization_input.rationale.clone(),
        ),
        StartAuthority::Operator(approval) => {
            let actor = approval
                .principal_label()
                .unwrap_or(approval.principal_id())
                .to_owned();
            authorization_input.actor = actor;
            authorization_input.rationale = format!(
                "operator {}: {}",
                approval.principal_id(),
                approval.rationale()
            );
            (
                "OPERATOR",
                approval.principal_id().to_owned(),
                approval.principal_label().map(str::to_owned),
                approval.rationale().to_owned(),
            )
        }
    };
    let policy_hash = sha256_hex(
        serde_json::to_string(&serde_json::json!({
            "schema":"s4d-lead-start-policy-v1",
            "mode":mode,
            "riskClass":"R0",
            "executionMode":authorization_input.execution_mode,
            "requestHash":identity.request_hash,
            "planHash":identity.plan_hash,
            "configHash":identity.config_hash,
            "delegationHash":identity.delegation_hash,
            "specSha256":sha256_hex(
                serde_json::to_string(&identity.action_spec)
                    .expect("ActionSpec serializes")
                    .as_bytes()
            ),
            "profileRevisionId":identity.profile.as_ref().map(|profile| profile.profile_revision_id.clone()),
            "profileHash":identity.profile.as_ref().map(|profile| profile.profile_hash.clone()),
        }))
        .expect("lead start policy serializes")
        .as_bytes(),
    );
    let envelope_hash = campaign_execution_envelope_hash(&CampaignExecutionEnvelope {
        request_id: &request.id,
        plan_id: &identity.plan.id,
        plan_hash: &identity.plan_hash,
        spec_sha256: &identity.spec_sha256,
        config_hash: &identity.config_hash,
        delegation_hash: &identity.delegation_hash,
        profile_revision_id: identity
            .profile
            .as_ref()
            .map(|profile| profile.profile_revision_id.as_str()),
        profile_hash: identity
            .profile
            .as_ref()
            .map(|profile| profile.profile_hash.as_str()),
        policy_hash: &policy_hash,
        authorization_digest: &submission_digest,
    });

    let now = Utc::now().to_rfc3339();
    let mut campaign = Campaign {
        id: Uuid::new_v4().to_string(),
        action_id: request.action_id.clone(),
        status: "BLOCKED_AUTHORIZATION".into(),
        created_at: now.clone(),
    };
    sqlx::query::query("INSERT INTO campaigns VALUES (?1,?2,?3,?4)")
        .bind(&campaign.id)
        .bind(&campaign.action_id)
        .bind(&campaign.status)
        .bind(&campaign.created_at)
        .execute(&mut *connection)
        .await?;
    super::authorization::authorize_campaign_in_tx(
        connection,
        &campaign.id,
        &authorization_input,
        super::authorization::AuthorizationReservationPolicy::LeadStart,
    )
    .await?;
    campaign.status = "AUTHORIZED".into();
    if matches!(authority, StartAuthority::Operator(_)) {
        sqlx::query::query("UPDATE actions SET risk='R0',status='AUTHORIZED' WHERE id=?1")
            .bind(&request.action_id)
            .execute(&mut *connection)
            .await?;
    }
    let mut first_run = None;
    let mut first_attempt = None;
    for (index, (unit, spec)) in proposal.work_units.iter().zip(specs.iter()).enumerate() {
        let spec_json =
            serde_json::to_string(spec).map_err(|_| Error::Invalid("unit ActionSpec encoding"))?;
        let spec_sha256 = sha256_hex(spec_json.as_bytes());
        let task_id = Uuid::new_v4().to_string();
        let mut run_id = None;
        let mut attempt_id = None;
        let is_root = unit.depends_on.is_empty();
        if is_root {
            let run = Run {
                id: Uuid::new_v4().to_string(),
                campaign_id: campaign.id.clone(),
                status: "QUEUED".into(),
                created_at: now.clone(),
            };
            sqlx::query::query("INSERT INTO runs VALUES (?1,?2,?3,?4)")
                .bind(&run.id)
                .bind(&run.campaign_id)
                .bind(&run.status)
                .bind(&run.created_at)
                .execute(&mut *connection)
                .await?;
            let attempt = Attempt {
                id: Uuid::new_v4().to_string(),
                run_id: run.id.clone(),
                status: "QUEUED".into(),
                created_at: now.clone(),
            };
            sqlx::query::query(
                "INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)",
            )
            .bind(&attempt.id)
            .bind(&attempt.run_id)
            .bind(&attempt.status)
            .bind(&attempt.created_at)
            .execute(&mut *connection)
            .await?;
            sqlx::query::query(
                "INSERT INTO attempt_specs(attempt_id,action_id,base_commit,spec_json,spec_sha256)
                 VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(&attempt.id)
            .bind(&request.action_id)
            .bind(&spec.base_commit)
            .bind(&spec_json)
            .bind(&spec_sha256)
            .execute(&mut *connection)
            .await?;
            run_id = Some(run.id.clone());
            attempt_id = Some(attempt.id.clone());
            first_run = Some(run);
            first_attempt = Some(attempt);
        }
        sqlx::query::query(
            "INSERT INTO lead_tasks
             (task_id,campaign_id,request_id,unit_id,sequence,status,base_commit,spec_json,
              spec_sha256,depends_on_json,run_id,attempt_id,candidate_id,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,NULL,?13,?13)",
        )
        .bind(&task_id)
        .bind(&campaign.id)
        .bind(&request.id)
        .bind(&unit.id)
        .bind(i64::try_from(index).map_err(|_| Error::Invalid("task sequence"))?)
        .bind(if is_root { "RUNNING" } else { "PENDING" })
        .bind(&spec.base_commit)
        .bind(spec_json)
        .bind(spec_sha256)
        .bind(
            serde_json::to_string(&unit.depends_on)
                .map_err(|_| Error::Invalid("dependencies encoding"))?,
        )
        .bind(run_id)
        .bind(attempt_id)
        .bind(&now)
        .execute(&mut *connection)
        .await?;
    }
    let run = first_run.ok_or(Error::Invalid("lead plan has no runnable unit"))?;
    let attempt = first_attempt.ok_or(Error::Invalid("lead plan has no runnable attempt"))?;
    sqlx::query::query(
        "INSERT INTO campaign_execution_bindings
         (campaign_id,request_id,plan_id,spec_sha256,envelope_hash,created_at)
         VALUES (?1,?2,?3,?4,?5,?6)",
    )
    .bind(&campaign.id)
    .bind(&request.id)
    .bind(&identity.plan.id)
    .bind(sha256_hex(
        serde_json::to_string(&identity.action_spec)
            .expect("ActionSpec serializes")
            .as_bytes(),
    ))
    .bind(&envelope_hash)
    .bind(&now)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "INSERT INTO lead_start_evidence
         (request_id,campaign_id,mode,principal_id,principal_label,rationale,idempotency_key,
          request_hash,plan_hash,profile_revision_id,profile_hash,spec_sha256,config_hash,
          delegation_hash,policy_hash,authorization_digest,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
    )
    .bind(&request.id)
    .bind(&campaign.id)
    .bind(mode)
    .bind(&principal_id)
    .bind(&principal_label)
    .bind(&rationale)
    .bind(idempotency_key)
    .bind(&identity.request_hash)
    .bind(&identity.plan_hash)
    .bind(
        identity
            .profile
            .as_ref()
            .map(|profile| profile.profile_revision_id.clone()),
    )
    .bind(
        identity
            .profile
            .as_ref()
            .map(|profile| profile.profile_hash.clone()),
    )
    .bind(sha256_hex(
        serde_json::to_string(&identity.action_spec)
            .expect("ActionSpec serializes")
            .as_bytes(),
    ))
    .bind(&identity.config_hash)
    .bind(&identity.delegation_hash)
    .bind(&policy_hash)
    .bind(&submission_digest)
    .bind(&now)
    .execute(&mut *connection)
    .await?;
    if let Some(profile) = &identity.profile {
        sqlx::query::query(
            "INSERT INTO campaign_execution_profile_bindings
             (campaign_id,request_id,profile_revision_id,profile_revision,profile_hash,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(&campaign.id)
        .bind(&request.id)
        .bind(&profile.profile_revision_id)
        .bind(i64::from(profile.profile_revision))
        .bind(&profile.profile_hash)
        .bind(&now)
        .execute(&mut *connection)
        .await?;
    }
    sqlx::query::query(
        "UPDATE intake_requests SET campaign_id=?1,updated_at=?2 WHERE lead_request_id=?3",
    )
    .bind(&campaign.id)
    .bind(&now)
    .bind(&request.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE lead_requests SET state='RUNNING',campaign_id=?1,updated_at=?2 WHERE id=?3",
    )
    .bind(&campaign.id)
    .bind(&now)
    .bind(&request.id)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
        .bind(idempotency_key)
        .bind("start_lead_campaign")
        .bind(&submission_digest)
        .bind(&campaign.id)
        .execute(&mut *connection)
        .await?;
    record_event(
        connection,
        &campaign.id,
        "campaign.started",
        &format!(
            "{} {} start for plan {} authorization={}",
            mode,
            authorization_input.execution_mode,
            identity.plan.plan_hash,
            authorization_input.authorization_id
        ),
    )
    .await?;
    record_event(
        connection,
        &request.id,
        "lead.approved",
        &format!(
            "{} principal={} authorization={}",
            mode, principal_id, authorization_input.authorization_id
        ),
    )
    .await?;
    record_event(
        connection,
        &request.id,
        "lead.started",
        &format!(
            "{} campaign={} authorization={}",
            authorization_input.execution_mode, campaign.id, authorization_input.authorization_id
        ),
    )
    .await?;
    Ok(LeadStartResult {
        campaign,
        run,
        attempt,
        replayed: false,
    })
}

async fn load_start_evidence_by_key(
    connection: &mut SqliteConnection,
    idempotency_key: &str,
) -> Result<Option<LeadStartEvidenceRecord>, Error> {
    let row: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT request_id FROM lead_start_evidence WHERE idempotency_key=?1",
    )
    .bind(idempotency_key)
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some((request_id,)) => load_start_evidence(connection, &request_id).await,
        None => Ok(None),
    }
}

async fn load_start_evidence(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Option<LeadStartEvidenceRecord>, Error> {
    use sqlx::row::Row;
    let row = sqlx::query::query(
        "SELECT request_id,campaign_id,mode,principal_id,principal_label,rationale,
                idempotency_key,request_hash,plan_hash,profile_revision_id,profile_hash,
                spec_sha256,config_hash,delegation_hash,policy_hash,authorization_digest,created_at
         FROM lead_start_evidence WHERE request_id=?1",
    )
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(LeadStartEvidenceRecord {
        request_id: row.try_get("request_id")?,
        campaign_id: row.try_get("campaign_id")?,
        mode: row.try_get("mode")?,
        principal_id: row.try_get("principal_id")?,
        principal_label: row.try_get("principal_label")?,
        rationale: row.try_get("rationale")?,
        idempotency_key: row.try_get("idempotency_key")?,
        request_hash: row.try_get("request_hash")?,
        plan_hash: row.try_get("plan_hash")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_hash: row.try_get("profile_hash")?,
        spec_sha256: row.try_get("spec_sha256")?,
        config_hash: row.try_get("config_hash")?,
        delegation_hash: row.try_get("delegation_hash")?,
        policy_hash: row.try_get("policy_hash")?,
        authorization_digest: row.try_get("authorization_digest")?,
        created_at: row.try_get("created_at")?,
    }))
}

async fn load_withdrawal(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Option<LeadWithdrawalRecord>, Error> {
    use sqlx::row::Row;
    let row = sqlx::query::query(
        "SELECT request_id,principal_id,principal_label,reason,created_at
         FROM lead_withdrawals WHERE request_id=?1",
    )
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(LeadWithdrawalRecord {
        request_id: row.try_get("request_id")?,
        principal_id: row.try_get("principal_id")?,
        principal_label: row.try_get("principal_label")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
    }))
}

struct CampaignExecutionEnvelope<'a> {
    request_id: &'a str,
    plan_id: &'a str,
    plan_hash: &'a str,
    spec_sha256: &'a str,
    config_hash: &'a str,
    delegation_hash: &'a str,
    profile_revision_id: Option<&'a str>,
    profile_hash: Option<&'a str>,
    policy_hash: &'a str,
    authorization_digest: &'a str,
}

fn campaign_execution_envelope_hash(envelope: &CampaignExecutionEnvelope<'_>) -> String {
    let mut value = serde_json::json!({
        "schema": if envelope.profile_revision_id.is_some() {
            "s4d-campaign-execution-binding-v2"
        } else {
            "s4d-campaign-execution-binding-v1"
        },
        "requestId":envelope.request_id,
        "planId":envelope.plan_id,
        "planHash":envelope.plan_hash,
        "specSha256":envelope.spec_sha256,
        "configHash":envelope.config_hash,
        "delegationHash":envelope.delegation_hash,
        "policyHash":envelope.policy_hash,
        "authorizationDigest":envelope.authorization_digest,
    });
    if let (Some(revision_id), Some(profile_hash)) =
        (envelope.profile_revision_id, envelope.profile_hash)
    {
        value["profileRevisionId"] = serde_json::json!(revision_id);
        value["profileHash"] = serde_json::json!(profile_hash);
    }
    sha256_hex(
        serde_json::to_string(&value)
            .expect("campaign execution binding serializes")
            .as_bytes(),
    )
}

/// Validate the immutable Campaign → request → frozen plan → root ActionSpec
/// envelope for every Campaign that has an execution binding. Campaigns without
/// a binding (legacy or non-Lead flows) are unchanged.
async fn validate_campaign_execution_binding_in_tx(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<(), Error> {
    let Some(binding) = load_campaign_execution_binding(connection, campaign_id).await? else {
        return Ok(());
    };
    let evidence = load_start_evidence(connection, &binding.request_id)
        .await?
        .ok_or_else(|| {
            Error::Forbidden("Campaign execution binding has no start evidence".into())
        })?;
    if evidence.campaign_id != campaign_id {
        return Err(Error::Forbidden(
            "Campaign execution binding evidence names another Campaign".into(),
        ));
    }
    let plan = load_frozen_plan_by_id(connection, &binding.plan_id).await?;
    if plan.request_id != binding.request_id
        || evidence.plan_hash != plan.plan_hash
        || plan.plan_hash.is_empty()
        || plan.config_hash != evidence.config_hash
        || plan.delegation_hash != evidence.delegation_hash
    {
        return Err(Error::Forbidden(
            "Campaign execution binding differs from its frozen plan".into(),
        ));
    }
    let lead: Option<(String, String, Option<String>)> = sqlx::query_as::query_as(
        "SELECT lr.id,lr.action_id,lr.campaign_id
         FROM campaigns c JOIN lead_requests lr ON lr.action_id=c.action_id
         WHERE c.id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((request_id, action_id, bound_campaign)) = lead else {
        return Err(Error::Forbidden(
            "Campaign execution binding has no Lead request".into(),
        ));
    };
    if request_id != binding.request_id || bound_campaign.as_deref() != Some(campaign_id) {
        return Err(Error::Forbidden(
            "Campaign execution binding differs from the Lead request".into(),
        ));
    }
    let root: Option<(String, String)> = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
    )
    .bind(&action_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((root_json, root_sha)) = root else {
        return Err(Error::Forbidden(
            "Campaign has no frozen root ActionSpec".into(),
        ));
    };
    if sha256_hex(root_json.as_bytes()) != root_sha
        || binding.spec_sha256 != root_sha
        || evidence.spec_sha256 != root_sha
    {
        return Err(Error::Forbidden(
            "Campaign root ActionSpec lineage mismatch".into(),
        ));
    }
    let expected = campaign_execution_envelope_hash(&CampaignExecutionEnvelope {
        request_id: &binding.request_id,
        plan_id: &binding.plan_id,
        plan_hash: &plan.plan_hash,
        spec_sha256: &root_sha,
        config_hash: &evidence.config_hash,
        delegation_hash: &evidence.delegation_hash,
        profile_revision_id: evidence.profile_revision_id.as_deref(),
        profile_hash: evidence.profile_hash.as_deref(),
        policy_hash: &evidence.policy_hash,
        authorization_digest: &evidence.authorization_digest,
    });
    if binding.envelope_hash != expected {
        return Err(Error::Forbidden(
            "Campaign execution envelope hash mismatch".into(),
        ));
    }
    Ok(())
}

/// A bound Lead Campaign may dispatch only attempts created by the shared
/// WorkGraph scheduler and linked to a Lead task or integration. Legacy and
/// non-Lead Campaigns without a binding keep their existing dispatch path.
pub(super) async fn validate_lead_attempt_lineage(
    connection: &mut SqliteConnection,
    attempt_id: &str,
) -> Result<(), Error> {
    let row: Option<(String, String)> = sqlx::query_as::query_as(
        "SELECT r.campaign_id,c.action_id
         FROM attempts a
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         WHERE a.id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(&mut *connection)
    .await?;
    let (campaign_id, campaign_action_id) = row.ok_or(Error::NotFound)?;
    let binding: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT campaign_id FROM campaign_execution_bindings WHERE campaign_id=?1",
    )
    .bind(&campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    if binding.is_none() {
        return Ok(());
    }
    validate_campaign_execution_binding_in_tx(connection, &campaign_id).await?;
    let linked: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT attempt_id FROM lead_tasks WHERE attempt_id=?1 AND campaign_id=?2
         UNION ALL
         SELECT attempt_id FROM lead_integrations WHERE attempt_id=?1 AND campaign_id=?2
         LIMIT 1",
    )
    .bind(attempt_id)
    .bind(&campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    if linked.is_none() {
        return Err(Error::Forbidden(
            "Lead Campaign Attempt is not part of the frozen WorkGraph".into(),
        ));
    }
    let spec: Option<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT action_id,spec_json,spec_sha256 FROM attempt_specs WHERE attempt_id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((action_id, spec_json, spec_sha256)) = spec else {
        return Err(Error::Forbidden(
            "Lead Campaign Attempt has no exact execution spec".into(),
        ));
    };
    if action_id != campaign_action_id || sha256_hex(spec_json.as_bytes()) != spec_sha256 {
        return Err(Error::Forbidden(
            "Lead Campaign Attempt spec lineage mismatch".into(),
        ));
    }
    Ok(())
}

async fn load_campaign_execution_binding(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Option<CampaignExecutionBindingRecord>, Error> {
    use sqlx::row::Row;
    let row = sqlx::query::query(
        "SELECT campaign_id,request_id,plan_id,spec_sha256,envelope_hash,created_at
         FROM campaign_execution_bindings WHERE campaign_id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(CampaignExecutionBindingRecord {
        campaign_id: row.try_get("campaign_id")?,
        request_id: row.try_get("request_id")?,
        plan_id: row.try_get("plan_id")?,
        spec_sha256: row.try_get("spec_sha256")?,
        envelope_hash: row.try_get("envelope_hash")?,
        created_at: row.try_get("created_at")?,
    }))
}

async fn freeze_lead_plan_in_tx(
    connection: &mut SqliteConnection,
    request_id: &str,
    idempotency_key: &str,
) -> Result<FrozenLeadPlanRecord, Error> {
    let request = load_lead_request(connection, request_id).await?;
    if withdrawal_in_tx(connection, request_id).await? {
        return Err(Error::Withdrawn);
    }
    let (proposal_id, proposal_json): (String, String) =
        sqlx::query_as::query_as("SELECT id,proposal_json FROM lead_proposals WHERE request_id=?1")
            .bind(request_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::Conflict("lead plan requires a recorded proposal"))?;
    let proposal: LeadPlanProposal = serde_json::from_str(&proposal_json)
        .map_err(|_| Error::Invalid("stored proposal encoding"))?;
    let grant = load_delegation(connection, &request.delegation_id).await?;
    let hash = plan_hash(&proposal, &grant, &request.project_config_hash);

    // Recovery replay is resolved against the original frozen identity before
    // current state, expiry, config availability or withdrawal checks.
    if let Some(id) = replay(connection, idempotency_key, "freeze_lead_plan", &hash).await? {
        let existing = load_frozen_plan_by_id(connection, &id).await?;
        if existing.request_id != request.id {
            return Err(Error::Conflict(
                "freeze idempotency key identifies another request",
            ));
        }
        return Ok(existing);
    }
    if request.state != LeadRequestState::Planned {
        return Err(Error::Conflict("lead plan is not ready to freeze"));
    }
    let existing: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM frozen_lead_plans WHERE request_id=?1")
            .bind(request_id)
            .fetch_optional(&mut *connection)
            .await?;
    if existing.is_some() {
        return Err(Error::Conflict("lead plan is already frozen"));
    }
    let config = load_project_config(connection, &request.project_id).await?;
    if config.config_hash != request.project_config_hash
        || proposal.base_revision != config.base_commit
    {
        return Err(Error::Forbidden(
            "project configuration or base changed after the Lead request".into(),
        ));
    }
    let action = load_action_binding(connection, &request.action_id).await?;
    proposal.validate(&grant, &config.binding(), &action, Utc::now())?;
    if proposal.objective != request.objective {
        return Err(Error::Forbidden(
            "proposal objective differs from the original Lead request".into(),
        ));
    }
    ensure_action_spec_for_frozen_plan(connection, &request, &proposal, &config, &grant).await?;
    let extra_sessions = proposal.max_sessions.saturating_sub(1);
    if extra_sessions > 0 {
        reserve_usage_in_tx(connection, &grant, 0, extra_sessions, 0).await?;
    }
    let record = FrozenLeadPlanRecord {
        id: Uuid::new_v4().to_string(),
        request_id: request.id.clone(),
        proposal_id,
        plan_hash: hash.clone(),
        config_hash: request.project_config_hash.clone(),
        delegation_hash: grant.content_hash.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    sqlx::query::query(
        "INSERT INTO frozen_lead_plans
         (id,request_id,proposal_id,plan_hash,config_hash,delegation_hash,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
    )
    .bind(&record.id)
    .bind(&record.request_id)
    .bind(&record.proposal_id)
    .bind(&record.plan_hash)
    .bind(&record.config_hash)
    .bind(&record.delegation_hash)
    .bind(&record.created_at)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
        .bind(idempotency_key)
        .bind("freeze_lead_plan")
        .bind(&hash)
        .bind(&record.id)
        .execute(&mut *connection)
        .await?;
    update_lead_state(connection, &request, LeadRequestState::Frozen).await?;
    record_event(
        connection,
        &request.id,
        "lead.plan_frozen",
        &record.plan_hash,
    )
    .await?;
    Ok(record)
}

async fn ensure_action_spec_for_frozen_plan(
    connection: &mut SqliteConnection,
    request: &LeadRequestRecord,
    proposal: &LeadPlanProposal,
    config: &ProjectConfig,
    grant: &StandingDelegation,
) -> Result<(), Error> {
    let existing: Option<(String, String)> = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
    )
    .bind(&request.action_id)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some((spec_json, saved_sha256)) = existing {
        if sha256_hex(spec_json.as_bytes()) != saved_sha256 {
            return Err(Error::Invalid("frozen ActionSpec digest mismatch"));
        }
        return Ok(());
    }
    let draft = match load_action_draft(connection, &request.action_id).await {
        Ok(draft) => draft,
        Err(Error::NotFound) => {
            return Err(Error::Conflict(
                "action has neither an executable spec nor an assessment draft",
            ));
        }
        Err(error) => return Err(error),
    };
    if sha256_hex(draft.assessment_input_json.as_bytes()) != draft.input_sha256 {
        return Err(Error::Invalid("action draft digest mismatch"));
    }
    let action: (String, String, String, String) = sqlx::query_as::query_as(
        "SELECT provider,model,objective,objective_sha256 FROM actions WHERE id=?1",
    )
    .bind(&request.action_id)
    .fetch_one(&mut *connection)
    .await?;
    if action.0 != grant.route.provider
        || action.1 != grant.route.model
        || action.2 != request.objective
        || action.3 != sha256_hex(request.objective.as_bytes())
    {
        return Err(Error::Forbidden(
            "draft Action identity does not match the registered implementer route".into(),
        ));
    }
    let spec = derive_root_action_spec(request, proposal, config, grant)?;
    let spec_json =
        serde_json::to_string(&spec).map_err(|_| Error::Invalid("ActionSpec encoding"))?;
    let spec_sha256 = sha256_hex(spec_json.as_bytes());
    sqlx::query::query(
        "INSERT INTO action_specs(action_id,spec_json,spec_sha256) VALUES (?1,?2,?3)",
    )
    .bind(&request.action_id)
    .bind(&spec_json)
    .bind(&spec_sha256)
    .execute(&mut *connection)
    .await?;
    record_event(
        connection,
        &request.action_id,
        "action.spec_frozen",
        &spec_sha256,
    )
    .await?;
    Ok(())
}

fn derive_root_action_spec(
    request: &LeadRequestRecord,
    proposal: &LeadPlanProposal,
    config: &ProjectConfig,
    grant: &StandingDelegation,
) -> Result<ActionSpec, Error> {
    let mut allowed_paths = std::collections::BTreeSet::new();
    for unit in &proposal.work_units {
        allowed_paths.extend(unit.allowed_paths.iter().cloned());
    }
    if allowed_paths.is_empty() {
        return Err(Error::Forbidden(
            "frozen plan has no validated work-unit path scope".into(),
        ));
    }
    let checks = complete_plan_checks(proposal)?;
    let allowed_paths: Vec<String> = allowed_paths.into_iter().collect();
    if allowed_paths
        .iter()
        .any(|path| !crate::workspace::path_covered(path, &grant.allowed_paths))
    {
        return Err(Error::Forbidden(
            "derived ActionSpec paths exceed the standing delegation".into(),
        ));
    }
    Ok(ActionSpec {
        schema_version: 1,
        project_id: config.project_id.clone(),
        repository: config.repository.clone(),
        base_commit: config.base_commit.clone(),
        objective: request.objective.clone(),
        objective_sha256: sha256_hex(request.objective.as_bytes()),
        provider: grant.route.provider.clone(),
        model: grant.route.model.clone(),
        allowed_paths,
        checks,
    })
}

fn complete_plan_checks(proposal: &LeadPlanProposal) -> Result<Vec<SpecCheck>, Error> {
    let mut checks = std::collections::BTreeMap::new();
    for check in proposal
        .acceptance_criteria
        .iter()
        .chain(proposal.negative_cases.iter())
    {
        insert_unique_check(&mut checks, check)?;
    }
    for unit in &proposal.work_units {
        for check in unit
            .acceptance_criteria
            .iter()
            .chain(unit.negative_cases.iter())
        {
            insert_unique_check(&mut checks, check)?;
        }
    }
    let checks: Vec<SpecCheck> = checks.into_values().collect();
    crate::lead::validate_mixed_checks(&checks)
        .map_err(|error| Error::Forbidden(error.to_string()))?;
    Ok(checks)
}

fn insert_unique_check(
    checks: &mut std::collections::BTreeMap<String, SpecCheck>,
    check: &SpecCheck,
) -> Result<(), Error> {
    match checks.get(&check.id) {
        Some(existing) if existing == check => Ok(()),
        Some(_) => Err(Error::Forbidden(
            "frozen plan contains conflicting check identities".into(),
        )),
        None => {
            checks.insert(check.id.clone(), check.clone());
            Ok(())
        }
    }
}

async fn update_lead_state(
    connection: &mut SqliteConnection,
    request: &LeadRequestRecord,
    next: LeadRequestState,
) -> Result<(), Error> {
    request.state.transition(next)?;
    sqlx::query::query("UPDATE lead_requests SET state=?1,updated_at=?2 WHERE id=?3")
        .bind(state_str(next))
        .bind(Utc::now().to_rfc3339())
        .bind(&request.id)
        .execute(connection)
        .await?;
    Ok(())
}

fn state_str(state: LeadRequestState) -> &'static str {
    match state {
        LeadRequestState::Submitted => "SUBMITTED",
        LeadRequestState::Assessing => "ASSESSING",
        LeadRequestState::Planned => "PLANNED",
        LeadRequestState::Frozen => "FROZEN",
        LeadRequestState::Ready => "READY",
        LeadRequestState::Running => "RUNNING",
        LeadRequestState::Blocked => "BLOCKED",
        LeadRequestState::Failed => "FAILED",
    }
}

fn parse_state(value: &str) -> Option<LeadRequestState> {
    match value {
        "SUBMITTED" => Some(LeadRequestState::Submitted),
        "ASSESSING" => Some(LeadRequestState::Assessing),
        "PLANNED" => Some(LeadRequestState::Planned),
        "FROZEN" => Some(LeadRequestState::Frozen),
        "READY" => Some(LeadRequestState::Ready),
        "RUNNING" => Some(LeadRequestState::Running),
        "BLOCKED" => Some(LeadRequestState::Blocked),
        "FAILED" => Some(LeadRequestState::Failed),
        _ => None,
    }
}

pub(super) async fn load_lead_request(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<LeadRequestRecord, Error> {
    let row = sqlx::query::query(
        "SELECT lr.id,lr.project_id,lr.action_id,lr.delegation_id,lr.objective,lr.request_hash,
                lr.project_config_hash,lr.state,la.mechanism AS assessment_mechanism,
                lr.failure_reason,lr.campaign_id,lr.created_at,lr.updated_at
         FROM lead_requests lr
         LEFT JOIN lead_assessments la ON la.request_id=lr.id
         WHERE lr.id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    let state: String = row.try_get("state")?;
    Ok(LeadRequestRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        action_id: row.try_get("action_id")?,
        delegation_id: row.try_get("delegation_id")?,
        objective: row.try_get("objective")?,
        request_hash: row.try_get("request_hash")?,
        project_config_hash: row.try_get("project_config_hash")?,
        state: parse_state(&state).ok_or(Error::Invalid("stored lead request state"))?,
        assessment_mechanism: row.try_get("assessment_mechanism")?,
        failure_reason: row.try_get("failure_reason")?,
        campaign_id: row.try_get("campaign_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(super) async fn withdrawal_in_tx(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<bool, Error> {
    let row: Option<(String,)> =
        sqlx::query_as::query_as("SELECT request_id FROM lead_withdrawals WHERE request_id=?1")
            .bind(request_id)
            .fetch_optional(connection)
            .await?;
    Ok(row.is_some())
}

async fn load_action_draft(
    connection: &mut SqliteConnection,
    action_id: &str,
) -> Result<ActionDraftRecord, Error> {
    let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT action_id,assessment_input_json,input_sha256,created_at
         FROM action_drafts WHERE action_id=?1",
    )
    .bind(action_id)
    .fetch_optional(connection)
    .await?;
    let (action_id, assessment_input_json, input_sha256, created_at) =
        row.ok_or(Error::NotFound)?;
    if sha256_hex(assessment_input_json.as_bytes()) != input_sha256 {
        return Err(Error::Invalid("action draft digest mismatch"));
    }
    Ok(ActionDraftRecord {
        action_id,
        assessment_input_json,
        input_sha256,
        created_at,
    })
}

async fn load_assessment(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<LeadAssessmentRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,mechanism,assessment_json,schema_sha256,created_at
         FROM lead_assessments WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(LeadAssessmentRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        mechanism: row.try_get("mechanism")?,
        assessment_json: row.try_get("assessment_json")?,
        schema_sha256: row.try_get("schema_sha256")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn load_proposal(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<LeadProposalRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,assessment_id,proposal_json,proposal_hash,created_at
         FROM lead_proposals WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(LeadProposalRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        assessment_id: row.try_get("assessment_id")?,
        proposal_json: row.try_get("proposal_json")?,
        proposal_hash: row.try_get("proposal_hash")?,
        created_at: row.try_get("created_at")?,
    })
}

pub(super) async fn load_frozen_plan_by_id(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<FrozenLeadPlanRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,proposal_id,plan_hash,config_hash,delegation_hash,created_at
         FROM frozen_lead_plans WHERE id=?1",
    )
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(FrozenLeadPlanRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        proposal_id: row.try_get("proposal_id")?,
        plan_hash: row.try_get("plan_hash")?,
        config_hash: row.try_get("config_hash")?,
        delegation_hash: row.try_get("delegation_hash")?,
        created_at: row.try_get("created_at")?,
    })
}

fn unit_action_spec(
    proposal: &LeadPlanProposal,
    config: &ProjectConfig,
    grant: &StandingDelegation,
    unit: &crate::lead::LeadWorkUnit,
    base_commit: &str,
) -> Result<ActionSpec, Error> {
    let mut checks = unit.acceptance_criteria.clone();
    checks.extend(unit.negative_cases.clone());
    if checks.is_empty() {
        checks = proposal.acceptance_criteria.clone();
        checks.extend(proposal.negative_cases.clone());
    }
    if !checks
        .iter()
        .any(|check| !matches!(check.kind, crate::lead::CheckKind::Negative))
        || !checks
            .iter()
            .any(|check| matches!(check.kind, crate::lead::CheckKind::Negative))
    {
        return Err(Error::Forbidden(
            "each WorkUnit requires acceptance and negative checks".into(),
        ));
    }
    let objective = if unit.description.trim().is_empty() {
        proposal.objective.clone()
    } else {
        unit.description.clone()
    };
    Ok(ActionSpec {
        schema_version: 1,
        project_id: config.project_id.clone(),
        repository: config.repository.clone(),
        base_commit: base_commit.into(),
        objective_sha256: sha256_hex(objective.as_bytes()),
        objective,
        provider: grant.route.provider.clone(),
        model: grant.route.model.clone(),
        allowed_paths: unit.allowed_paths.clone(),
        checks,
    })
}

pub(super) async fn advance_lead_task(
    transaction: &mut SqliteConnection,
    attempt_id: &str,
    candidate_commit: &str,
) -> Result<(), Error> {
    use sqlx::row::Row;
    let campaign_id = if let Some(task) =
        sqlx::query::query("SELECT task_id,campaign_id FROM lead_tasks WHERE attempt_id=?1")
            .bind(attempt_id)
            .fetch_optional(&mut *transaction)
            .await?
    {
        let task_id: String = task.try_get("task_id")?;
        let campaign_id: String = task.try_get("campaign_id")?;
        sqlx::query::query(
            "UPDATE lead_tasks SET status='SUCCEEDED',candidate_id=(
               SELECT id FROM candidates WHERE attempt_id=?1
             ),updated_at=?2 WHERE task_id=?3",
        )
        .bind(attempt_id)
        .bind(Utc::now().to_rfc3339())
        .bind(&task_id)
        .execute(&mut *transaction)
        .await?;
        campaign_id
    } else if let Some(integration) = sqlx::query::query(
        "SELECT campaign_id,next_task_id FROM lead_integrations WHERE attempt_id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        let campaign_id: String = integration.try_get("campaign_id")?;
        let next_task_id: String = integration.try_get("next_task_id")?;
        sqlx::query::query(
            "UPDATE lead_integrations SET status='SUCCEEDED',candidate_id=(
               SELECT id FROM candidates WHERE attempt_id=?1
             ),updated_at=?2 WHERE attempt_id=?1",
        )
        .bind(attempt_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        if !next_task_id.is_empty() {
            schedule_unit_attempt(transaction, &next_task_id, candidate_commit).await?;
        } else {
            let request_id: (String,) = sqlx::query_as::query_as(
                "SELECT lr.id FROM lead_requests lr WHERE lr.campaign_id=?1",
            )
            .bind(&campaign_id)
            .fetch_one(&mut *transaction)
            .await?;
            record_event(
                transaction,
                &request_id.0,
                "lead.plan_completed",
                "final integrated candidate recorded",
            )
            .await?;
        }
        campaign_id
    } else {
        return Ok(());
    };
    schedule_ready_tasks(transaction, &campaign_id).await?;
    Ok(())
}

pub(super) async fn fail_lead_work(
    transaction: &mut SqliteConnection,
    attempt_id: &str,
    reason: &str,
) -> Result<(), Error> {
    use sqlx::row::Row;
    let task: Option<(String, String)> =
        sqlx::query_as::query_as("SELECT task_id,campaign_id FROM lead_tasks WHERE attempt_id=?1")
            .bind(attempt_id)
            .fetch_optional(&mut *transaction)
            .await?;
    let integration: Option<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT integration_id,campaign_id,next_task_id FROM lead_integrations WHERE attempt_id=?1",
    )
    .bind(attempt_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let (task_id, campaign_id, next_task) = if let Some((task_id, campaign_id)) = task {
        (Some(task_id), campaign_id, None)
    } else if let Some((integration_id, campaign_id, next_task_id)) = integration {
        sqlx::query::query(
            "UPDATE lead_integrations SET status='FAILED',updated_at=?1 WHERE integration_id=?2",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(&integration_id)
        .execute(&mut *transaction)
        .await?;
        (
            None,
            campaign_id,
            if next_task_id.is_empty() {
                None
            } else {
                Some(next_task_id)
            },
        )
    } else {
        return Ok(());
    };
    if let Some(task_id) = &task_id {
        sqlx::query::query("UPDATE lead_tasks SET status='FAILED',updated_at=?1 WHERE task_id=?2")
            .bind(Utc::now().to_rfc3339())
            .bind(task_id)
            .execute(&mut *transaction)
            .await?;
    }
    let mut failed_units: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if let Some(task_id) = &task_id {
        let unit: (String,) =
            sqlx::query_as::query_as("SELECT unit_id FROM lead_tasks WHERE task_id=?1")
                .bind(task_id)
                .fetch_one(&mut *transaction)
                .await?;
        failed_units.insert(unit.0);
    }
    if let Some(next_task) = &next_task {
        let unit: (String,) =
            sqlx::query_as::query_as("SELECT unit_id FROM lead_tasks WHERE task_id=?1")
                .bind(next_task)
                .fetch_one(&mut *transaction)
                .await?;
        failed_units.insert(unit.0);
        sqlx::query::query(
            "UPDATE lead_tasks SET status='FAILED',updated_at=?1 WHERE task_id=?2 AND status='PENDING'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(next_task)
        .execute(&mut *transaction)
        .await?;
    }
    let rows = sqlx::query::query(
        "SELECT task_id,unit_id,status,depends_on_json FROM lead_tasks WHERE campaign_id=?1",
    )
    .bind(&campaign_id)
    .fetch_all(&mut *transaction)
    .await?;
    loop {
        let mut changed = false;
        for row in &rows {
            let task_id: String = row.try_get("task_id")?;
            let unit: String = row.try_get("unit_id")?;
            let status: String = row.try_get("status")?;
            let depends: String = row.try_get("depends_on_json")?;
            let dependencies: Vec<String> = serde_json::from_str(&depends)
                .map_err(|_| Error::Invalid("stored task dependencies"))?;
            if status == "SUCCEEDED"
                || status == "FAILED"
                || failed_units.contains(&unit)
                || !dependencies.iter().any(|dep| failed_units.contains(dep))
            {
                continue;
            }
            sqlx::query::query(
                "UPDATE lead_tasks SET status='FAILED',updated_at=?1 WHERE task_id=?2",
            )
            .bind(Utc::now().to_rfc3339())
            .bind(&task_id)
            .execute(&mut *transaction)
            .await?;
            failed_units.insert(unit);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    let request: (String,) =
        sqlx::query_as::query_as("SELECT id FROM lead_requests WHERE campaign_id=?1")
            .bind(&campaign_id)
            .fetch_one(&mut *transaction)
            .await?;
    sqlx::query::query(
        "UPDATE lead_requests SET state='FAILED',failure_reason=?1,updated_at=?2 WHERE id=?3",
    )
    .bind(reason)
    .bind(Utc::now().to_rfc3339())
    .bind(&request.0)
    .execute(&mut *transaction)
    .await?;
    sqlx::query::query("UPDATE campaigns SET status='FAILED' WHERE id=?1")
        .bind(&campaign_id)
        .execute(&mut *transaction)
        .await?;
    record_event(transaction, &request.0, "lead.work_failed", reason).await?;
    Ok(())
}

async fn schedule_ready_tasks(
    transaction: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<(), Error> {
    use sqlx::row::Row;
    let rows = sqlx::query::query(
        "SELECT t.task_id,t.unit_id,t.status,t.depends_on_json,c.commit_sha
         FROM lead_tasks t
         LEFT JOIN candidates c ON c.id=t.candidate_id
         WHERE t.campaign_id=?1 ORDER BY t.sequence",
    )
    .bind(campaign_id)
    .fetch_all(&mut *transaction)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    let mut candidates: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut all_units = Vec::new();
    let mut depended_on = std::collections::BTreeSet::new();
    let mut running_or_pending = false;
    for row in &rows {
        let unit: String = row.try_get("unit_id")?;
        let status: String = row.try_get("status")?;
        let candidate: Option<String> = row.try_get("commit_sha")?;
        let depends: String = row.try_get("depends_on_json")?;
        let dependencies: Vec<String> = serde_json::from_str(&depends)
            .map_err(|_| Error::Invalid("stored task dependencies"))?;
        depended_on.extend(dependencies);
        if let Some(candidate) = candidate {
            candidates.insert(unit.clone(), candidate);
        }
        all_units.push(unit.clone());
        if matches!(status.as_str(), "PENDING" | "RUNNING") {
            running_or_pending = true;
        }
    }
    for row in &rows {
        let status: String = row.try_get("status")?;
        if status != "PENDING" {
            continue;
        }
        let task_id: String = row.try_get("task_id")?;
        let unit: String = row.try_get("unit_id")?;
        let depends: String = row.try_get("depends_on_json")?;
        let dependencies: Vec<String> = serde_json::from_str(&depends)
            .map_err(|_| Error::Invalid("stored task dependencies"))?;
        if dependencies.iter().all(|dep| candidates.contains_key(dep)) {
            let commits: Vec<String> = dependencies
                .iter()
                .map(|dep| candidates[dep].clone())
                .collect();
            match commits.len() {
                0 => {}
                1 => schedule_unit_attempt(transaction, &task_id, &commits[0]).await?,
                _ => {
                    ensure_integration(
                        transaction,
                        campaign_id,
                        "FANIN",
                        &task_id,
                        &dependencies,
                        &commits,
                    )
                    .await?;
                }
            }
            let _ = unit;
        }
    }
    if !running_or_pending {
        let terminals: Vec<&String> = all_units
            .iter()
            .filter(|unit| !depended_on.contains(*unit))
            .collect();
        if terminals.len() > 1 {
            let commits: Vec<String> = terminals
                .iter()
                .filter_map(|unit| candidates.get(*unit).cloned())
                .collect();
            if commits.len() > 1 {
                ensure_integration(transaction, campaign_id, "FINAL", "", &[], &commits).await?;
            }
        } else {
            let request_id: (String,) =
                sqlx::query_as::query_as("SELECT id FROM lead_requests WHERE campaign_id=?1")
                    .bind(campaign_id)
                    .fetch_one(&mut *transaction)
                    .await?;
            record_event(
                transaction,
                &request_id.0,
                "lead.plan_completed",
                "all WorkGraph tasks succeeded",
            )
            .await?;
        }
    }
    Ok(())
}

async fn schedule_unit_attempt(
    transaction: &mut SqliteConnection,
    task_id: &str,
    base_commit: &str,
) -> Result<(), Error> {
    use sqlx::row::Row;
    let task =
        sqlx::query::query("SELECT campaign_id,status,spec_json FROM lead_tasks WHERE task_id=?1")
            .bind(task_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(Error::NotFound)?;
    let status: String = task.try_get("status")?;
    if status != "PENDING" {
        return Ok(());
    }
    let campaign_id: String = task.try_get("campaign_id")?;
    let spec_json: String = task.try_get("spec_json")?;
    let mut spec: ActionSpec =
        serde_json::from_str(&spec_json).map_err(|_| Error::Invalid("stored unit ActionSpec"))?;
    spec.base_commit = base_commit.into();
    let spec_json =
        serde_json::to_string(&spec).map_err(|_| Error::Invalid("unit ActionSpec encoding"))?;
    let spec_sha256 = sha256_hex(spec_json.as_bytes());
    let now = Utc::now().to_rfc3339();
    let run = Run {
        id: Uuid::new_v4().to_string(),
        campaign_id: campaign_id.clone(),
        status: "QUEUED".into(),
        created_at: now.clone(),
    };
    sqlx::query::query("INSERT INTO runs VALUES (?1,?2,?3,?4)")
        .bind(&run.id)
        .bind(&run.campaign_id)
        .bind(&run.status)
        .bind(&run.created_at)
        .execute(&mut *transaction)
        .await?;
    let attempt = Attempt {
        id: Uuid::new_v4().to_string(),
        run_id: run.id.clone(),
        status: "QUEUED".into(),
        created_at: now.clone(),
    };
    sqlx::query::query("INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)")
        .bind(&attempt.id)
        .bind(&attempt.run_id)
        .bind(&attempt.status)
        .bind(&attempt.created_at)
        .execute(&mut *transaction)
        .await?;
    sqlx::query::query(
        "INSERT INTO attempt_specs
         (attempt_id,action_id,base_commit,spec_json,spec_sha256,kind)
         VALUES (?1,(SELECT action_id FROM campaigns WHERE id=?2),?3,?4,?5,'UNIT')",
    )
    .bind(&attempt.id)
    .bind(&campaign_id)
    .bind(&spec.base_commit)
    .bind(&spec_json)
    .bind(&spec_sha256)
    .execute(&mut *transaction)
    .await?;
    sqlx::query::query(
        "UPDATE lead_tasks SET status='RUNNING',base_commit=?1,spec_json=?2,spec_sha256=?3,
         run_id=?4,attempt_id=?5,updated_at=?6 WHERE task_id=?7",
    )
    .bind(&spec.base_commit)
    .bind(&spec_json)
    .bind(&spec_sha256)
    .bind(&run.id)
    .bind(&attempt.id)
    .bind(&now)
    .bind(task_id)
    .execute(&mut *transaction)
    .await?;
    Ok(())
}

async fn ensure_integration(
    transaction: &mut SqliteConnection,
    campaign_id: &str,
    purpose: &str,
    next_task_id: &str,
    dependencies: &[String],
    commits: &[String],
) -> Result<(), Error> {
    use sqlx::row::Row;
    let mut commits = commits.to_vec();
    commits.sort();
    commits.dedup();
    if commits.len() < 2 {
        return Err(Error::Invalid(
            "integration requires at least two candidates",
        ));
    }
    let existing = sqlx::query::query(
        "SELECT candidate_id FROM lead_integrations
         WHERE campaign_id=?1 AND purpose=?2 AND next_task_id=?3",
    )
    .bind(campaign_id)
    .bind(purpose)
    .bind(next_task_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some(existing) = existing {
        let candidate: Option<String> = existing.try_get("candidate_id")?;
        if let Some(candidate) = candidate
            && purpose == "FANIN"
        {
            schedule_unit_attempt(transaction, next_task_id, &candidate).await?;
        }
        return Ok(());
    }
    let context = sqlx::query::query(
        "SELECT lr.id AS request_id,lr.project_id,lr.delegation_id,lp.proposal_json
         FROM campaigns c
         JOIN lead_requests lr ON lr.campaign_id=c.id
         JOIN lead_proposals lp ON lp.request_id=lr.id
         WHERE c.id=?1",
    )
    .bind(campaign_id)
    .fetch_one(&mut *transaction)
    .await?;
    let _request_id: String = context.try_get("request_id")?;
    let project_id: String = context.try_get("project_id")?;
    let delegation_id: String = context.try_get("delegation_id")?;
    let proposal_json: String = context.try_get("proposal_json")?;
    let proposal: LeadPlanProposal = serde_json::from_str(&proposal_json)
        .map_err(|_| Error::Invalid("stored proposal encoding"))?;
    let config = load_project_config(transaction, &project_id).await?;
    let grant = load_delegation(transaction, &delegation_id).await?;
    let mut allowed_paths = std::collections::BTreeSet::new();
    let checks = if purpose == "FANIN" {
        let task = sqlx::query::query("SELECT spec_json FROM lead_tasks WHERE task_id=?1")
            .bind(next_task_id)
            .fetch_one(&mut *transaction)
            .await?;
        let spec_json: String = task.try_get("spec_json")?;
        let spec: ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored unit ActionSpec"))?;
        allowed_paths.extend(spec.allowed_paths);
        for dependency in dependencies {
            let dep_spec: (String,) = sqlx::query_as::query_as(
                "SELECT spec_json FROM lead_tasks WHERE campaign_id=?1 AND unit_id=?2",
            )
            .bind(campaign_id)
            .bind(dependency)
            .fetch_one(&mut *transaction)
            .await?;
            let dep_spec: ActionSpec = serde_json::from_str(&dep_spec.0)
                .map_err(|_| Error::Invalid("stored dependency ActionSpec"))?;
            allowed_paths.extend(dep_spec.allowed_paths);
        }
        spec.checks
    } else {
        let mut checks = proposal.acceptance_criteria.clone();
        checks.extend(proposal.negative_cases.clone());
        for unit in proposal.terminal_units() {
            allowed_paths.extend(unit.allowed_paths.iter().cloned());
        }
        checks
    };
    let objective = if purpose == "FINAL" {
        "Integrate final WorkGraph candidates".to_string()
    } else {
        format!("Integrate fan-in candidates for task {next_task_id}")
    };
    let spec = ActionSpec {
        schema_version: 1,
        project_id: config.project_id.clone(),
        repository: config.repository.clone(),
        base_commit: config.base_commit.clone(),
        objective_sha256: sha256_hex(objective.as_bytes()),
        objective,
        provider: grant.route.provider.clone(),
        model: grant.route.model.clone(),
        allowed_paths: allowed_paths.into_iter().collect(),
        checks,
    };
    let spec_json = serde_json::to_string(&spec)
        .map_err(|_| Error::Invalid("integration ActionSpec encoding"))?;
    let spec_sha256 = sha256_hex(spec_json.as_bytes());
    let now = Utc::now().to_rfc3339();
    let run = Run {
        id: Uuid::new_v4().to_string(),
        campaign_id: campaign_id.into(),
        status: "QUEUED".into(),
        created_at: now.clone(),
    };
    sqlx::query::query("INSERT INTO runs VALUES (?1,?2,?3,?4)")
        .bind(&run.id)
        .bind(&run.campaign_id)
        .bind(&run.status)
        .bind(&run.created_at)
        .execute(&mut *transaction)
        .await?;
    let attempt = Attempt {
        id: Uuid::new_v4().to_string(),
        run_id: run.id.clone(),
        status: "QUEUED".into(),
        created_at: now.clone(),
    };
    sqlx::query::query("INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)")
        .bind(&attempt.id)
        .bind(&attempt.run_id)
        .bind(&attempt.status)
        .bind(&attempt.created_at)
        .execute(&mut *transaction)
        .await?;
    let candidates_json = serde_json::to_string(&commits)
        .map_err(|_| Error::Invalid("integration candidates encoding"))?;
    sqlx::query::query(
        "INSERT INTO attempt_specs
         (attempt_id,action_id,base_commit,spec_json,spec_sha256,kind,
          integration_candidates_json,next_task_id,final_integration)
         VALUES (?1,(SELECT action_id FROM campaigns WHERE id=?2),?3,?4,?5,'INTEGRATION',?6,?7,?8)",
    )
    .bind(&attempt.id)
    .bind(campaign_id)
    .bind(&spec.base_commit)
    .bind(&spec_json)
    .bind(&spec_sha256)
    .bind(&candidates_json)
    .bind(next_task_id)
    .bind(i64::from(purpose == "FINAL"))
    .execute(&mut *transaction)
    .await?;
    sqlx::query::query(
        "INSERT INTO lead_integrations
         (integration_id,campaign_id,purpose,next_task_id,attempt_id,status,candidates_json,candidate_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,'QUEUED',?6,NULL,?7,?7)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(campaign_id)
    .bind(purpose)
    .bind(next_task_id)
    .bind(&attempt.id)
    .bind(candidates_json)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    record_event(
        transaction,
        campaign_id,
        "work_integration.started",
        &format!("{purpose} next_task={next_task_id}"),
    )
    .await?;
    Ok(())
}

pub(super) async fn load_action_binding(
    connection: &mut SqliteConnection,
    action_id: &str,
) -> Result<ActionBinding, Error> {
    let row: Option<(String, String)> =
        sqlx::query_as::query_as("SELECT project_id,status FROM actions WHERE id=?1")
            .bind(action_id)
            .fetch_optional(&mut *connection)
            .await?;
    let (project_id, status) = row.ok_or(Error::NotFound)?;
    let status = match status.as_str() {
        "HUMAN_REQUIRED" | "PENDING" | "BLOCKED_AUTHORIZATION" | "AUTHORIZED" => {
            crate::lead::ActionStatus::Pending
        }
        "IN_PROGRESS" | "RUNNING" => crate::lead::ActionStatus::InProgress,
        "COMPLETED" | "FAILED" | "CANCELLED" => crate::lead::ActionStatus::Terminal,
        _ => return Err(Error::Invalid("stored action status")),
    };
    Ok(ActionBinding {
        id: action_id.into(),
        project_id,
        status,
    })
}

async fn load_run_for_campaign(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Run, Error> {
    let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT id,campaign_id,status,created_at FROM runs WHERE campaign_id=?1 ORDER BY created_at LIMIT 1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|(id, campaign_id, status, created_at)| Run {
        id,
        campaign_id,
        status,
        created_at,
    })
    .ok_or(Error::NotFound)
}

async fn load_attempt_for_run(
    connection: &mut SqliteConnection,
    run_id: &str,
) -> Result<Attempt, Error> {
    let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
        "SELECT id,run_id,status,created_at FROM attempts WHERE run_id=?1 ORDER BY created_at LIMIT 1",
    )
    .bind(run_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|(id, run_id, status, created_at)| Attempt {
        id,
        run_id,
        status,
        created_at,
    })
    .ok_or(Error::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead::{
        Capability, CheckKind, Complexity, EffortEstimate, EffortStatus, Intent, LeadRequestState,
        LeadWorkUnit, ObjectiveClass, ProjectConfig, ProviderRoute, QualityProfile, RiskClass,
        SpecBudget, SpecCheck, StandingDelegation, WorkUnitKind,
    };

    async fn fixture() -> (std::path::PathBuf, Database, StandingDelegation) {
        let path = std::env::temp_dir().join(format!(
            "factory-lead-request-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query("INSERT INTO projects(id,repository,base_commit,created_at,identity) VALUES (?1,?2,?3,?4,'')")
            .bind("project")
            .bind("/fixture")
            .bind("b".repeat(40))
            .bind(Utc::now().to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();
        let config = project_config();
        sqlx::query::query(
            "INSERT INTO project_configs
             (project_id,revision,config_hash,repository,base_commit,allowed_paths_json,
              capabilities_json,quality_profile,active,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        )
        .bind(&config.project_id)
        .bind(i64::from(config.revision))
        .bind(&config.config_hash)
        .bind(&config.repository)
        .bind(&config.base_commit)
        .bind(serde_json::to_string(&config.allowed_paths).unwrap())
        .bind(serde_json::to_string(&config.capabilities).unwrap())
        .bind("generic")
        .bind(1_i64)
        .bind(&config.updated_at)
        .execute(&db.pool)
        .await
        .unwrap();
        let mut grant = grant();
        grant.autonomous_start = true;
        grant.autonomous_risk_classes = vec![RiskClass::R0];
        grant.content_hash = grant.compute_content_hash();
        db.create_standing_delegation(&grant, "grant-key")
            .await
            .unwrap();
        (path, db, grant)
    }

    fn project_config() -> ProjectConfig {
        let mut config = ProjectConfig {
            project_id: "project".into(),
            revision: 1,
            config_hash: String::new(),
            repository: "/fixture".into(),
            base_commit: "b".repeat(40),
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            quality_profile: QualityProfile::Generic,
            active: true,
            updated_at: "2026-10-24T00:00:00Z".into(),
        };
        config.config_hash = config.compute_hash();
        config
    }

    fn config_hash() -> String {
        project_config().config_hash
    }

    fn grant() -> StandingDelegation {
        let mut grant = StandingDelegation {
            id: "grant".into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
            risk_classes: vec![RiskClass::R0, RiskClass::R1],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_attempts: 2,
            max_timeout_secs: 300,
            max_repair_cycles: 1,
            max_wall_clock_secs: 1_800,
            max_parallel: 1,
            max_sessions: 4,
            max_requests: 2,
            max_cumulative_sessions: 8,
            max_cumulative_attempts: 16,
            expires_at: Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        grant
    }

    fn checks() -> (Vec<SpecCheck>, Vec<SpecCheck>) {
        (
            vec![
                SpecCheck {
                    id: "format".into(),
                    kind: CheckKind::Format,
                    description: "format".into(),
                    command: vec!["cargo".into(), "fmt".into(), "--check".into()],
                    expect_exit: 0,
                    timeout_secs: 60,
                },
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
                    command: vec!["cargo".into(), "run".into()],
                    expect_exit: 0,
                    timeout_secs: 120,
                },
            ],
            vec![SpecCheck {
                id: "negative".into(),
                kind: CheckKind::Negative,
                description: "invalid input rejected".into(),
                command: vec!["cargo".into(), "run".into(), "--invalid".into()],
                expect_exit: 1,
                timeout_secs: 60,
            }],
        )
    }

    fn assessment() -> LeadAssessment {
        let (acceptance_criteria, negative_cases) = checks();
        let proposal = LeadPlanProposal {
            project_id: "project".into(),
            action_id: "action".into(),
            base_revision: "b".repeat(40),
            project_config_hash: config_hash(),
            objective: "Implement the feature.".into(),
            class: ObjectiveClass::Feature,
            assumptions: vec!["Bounded change.".into()],
            risks: vec!["Scope drift.".into()],
            acceptance_criteria,
            negative_cases,
            quality_profile: QualityProfile::Generic,
            risk_class: RiskClass::R1,
            budget: SpecBudget {
                max_attempts: 1,
                timeout_secs: 300,
            },
            route: grant().route,
            max_repair_cycles: 0,
            max_wall_clock_secs: 1_800,
            max_parallel: 1,
            max_sessions: 2,
            work_units: vec![LeadWorkUnit {
                id: "api".into(),
                kind: WorkUnitKind::Implementation,
                description: "Implement the endpoint.".into(),
                depends_on: Vec::new(),
                allowed_paths: vec!["src/api".into()],
                capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            }],
        };
        LeadAssessment {
            schema_revision: 1,
            project_id: "project".into(),
            action_id: "action".into(),
            request_id: "request".into(),
            evidence_refs: vec!["repository-files".into()],
            intent: Intent::Change,
            complexity: Complexity::Small,
            effort_estimate: EffortEstimate {
                status: EffortStatus::Estimated,
                unit: "person-hours".into(),
                min_person_hours: 2,
                max_person_hours: 4,
                assumptions: vec!["One component.".into()],
                unknown_reason: String::new(),
            },
            knowledge_gaps: Vec::new(),
            change_impact: Vec::new(),
            validation_needs: Vec::new(),
            assumptions: Vec::new(),
            uncertainty: Vec::new(),
            suggested_next_action: "Freeze the plan.".into(),
            proposal: Some(proposal),
        }
    }

    async fn action(db: &Database) -> String {
        db.create_action(
            "project",
            "Implement the feature.",
            "opencode",
            "deepseek-v4.1-flash",
            "action-key",
        )
        .await
        .unwrap()
        .id
    }

    struct ReadyLead {
        request: LeadRequestRecord,
        proposal: LeadPlanProposal,
    }

    async fn ready_lead(db: &Database, mechanism: &str) -> ReadyLead {
        let (acceptance, negative) = checks();
        let checks: Vec<SpecCheck> = acceptance.into_iter().chain(negative).collect();
        let allowed = vec!["src".to_string()];
        let unique = Uuid::new_v4().to_string();
        let action = db
            .create_action_with_spec(
                "project",
                "Implement the feature.",
                "opencode",
                "deepseek-v4.1-flash",
                &format!("{mechanism}-action-{unique}"),
                ActionSpecInput {
                    allowed_paths: &allowed,
                    checks: &checks,
                },
            )
            .await
            .unwrap();
        db.classify_action(
            &action.id,
            "R0",
            "test-owner",
            "deterministic R0 classification",
            &format!("{mechanism}-classification-{unique}"),
        )
        .await
        .unwrap();
        let config = db.project_config("project").await.unwrap();
        let (request, _) = db
            .create_lead_request(
                "project",
                &action.id,
                "grant",
                "Implement the feature.",
                &config.config_hash,
                &format!("{mechanism}-request-{unique}"),
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let grant = db.standing_delegation("grant").await.unwrap();
        let binding = ActionBinding {
            id: action.id.clone(),
            project_id: "project".into(),
            status: crate::lead::ActionStatus::Pending,
        };
        let (assessment, proposal) = crate::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &binding,
            &action.spec,
            &request.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action.id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(
            &request.id,
            mechanism,
            &assessment,
            &input,
            &format!("{mechanism}-assessment-{unique}"),
        )
        .await
        .unwrap();
        db.record_lead_proposal(
            &request.id,
            &proposal,
            &format!("{mechanism}-proposal-{unique}"),
        )
        .await
        .unwrap();
        db.freeze_lead_plan(&request.id, &format!("{mechanism}-freeze-{unique}"))
            .await
            .unwrap();
        db.mark_lead_ready(&request.id).await.unwrap();
        ReadyLead { request, proposal }
    }

    fn model_authorization(proposal: &LeadPlanProposal) -> NewCampaignAuthorization {
        NewCampaignAuthorization {
            authorization_id: "explicit-model-lead-start".into(),
            risk_class: "R0".into(),
            execution_mode: "TRUSTED_LOCAL_PROVIDER".into(),
            actor: "test-operator".into(),
            rationale: "explicit provider authorization for a MODEL Lead plan".into(),
            authorized_paths: vec!["src".into()],
            max_attempts: 1,
            provider: proposal.route.provider.clone(),
            model: proposal.route.model.clone(),
            provider_mode: proposal.route.mode.clone(),
            delegation_id: "grant".into(),
            max_sessions: 1,
            max_turns: 4,
            max_input_tokens: 1_000,
            max_output_tokens: 1_000,
            max_cost_micros: 10_000,
            deadline_secs: 300,
            allowed_tools: vec!["write_file".into()],
        }
    }

    struct DraftReady {
        request: LeadRequestRecord,
        plan: FrozenLeadPlanRecord,
        spec_sha256: String,
        config_hash: String,
        grant: StandingDelegation,
        proposal: LeadPlanProposal,
    }

    async fn operator_grant(db: &Database, id: &str) -> StandingDelegation {
        let mut grant = grant();
        grant.id = id.into();
        grant.autonomous_start = false;
        grant.autonomous_risk_classes.clear();
        grant.content_hash = grant.compute_content_hash();
        db.create_standing_delegation(&grant, &format!("{id}-key"))
            .await
            .unwrap();
        grant
    }

    async fn draft_ready_lead(db: &Database, label: &str) -> DraftReady {
        draft_ready_lead_with_mechanism(db, label, "FIXTURE").await
    }

    async fn draft_ready_lead_with_mechanism(
        db: &Database,
        label: &str,
        mechanism: &str,
    ) -> DraftReady {
        draft_lead_with_mechanism(db, label, mechanism, true).await
    }

    async fn draft_frozen_lead_with_mechanism(
        db: &Database,
        label: &str,
        mechanism: &str,
    ) -> DraftReady {
        draft_lead_with_mechanism(db, label, mechanism, false).await
    }

    async fn draft_lead_with_mechanism(
        db: &Database,
        label: &str,
        mechanism: &str,
        make_ready: bool,
    ) -> DraftReady {
        let grant = operator_grant(db, &format!("operator-grant-{label}")).await;
        let config = db.project_config("project").await.unwrap();
        let (request, created) = db
            .create_draft_lead_request(NewDraftLeadRequest {
                project_id: "project",
                delegation_id: &grant.id,
                objective: "Implement the feature.",
                project_config_hash: &config.config_hash,
                assessment_input_json: &serde_json::json!({
                    "objective": "Implement the feature.",
                    "projectId": "project",
                })
                .to_string(),
                idempotency_key: &format!("draft-request-{label}"),
            })
            .await
            .unwrap();
        assert!(created);
        db.begin_lead_assessment(&request.id).await.unwrap();
        let (acceptance, negative) = checks();
        let action_spec = ActionSpec {
            schema_version: 1,
            project_id: "project".into(),
            repository: config.repository.clone(),
            base_commit: config.base_commit.clone(),
            objective: request.objective.clone(),
            objective_sha256: sha256_hex(request.objective.as_bytes()),
            provider: grant.route.provider.clone(),
            model: grant.route.model.clone(),
            allowed_paths: config.allowed_paths.clone(),
            checks: acceptance.into_iter().chain(negative).collect(),
        };
        let binding = ActionBinding {
            id: request.action_id.clone(),
            project_id: "project".into(),
            status: crate::lead::ActionStatus::Pending,
        };
        let (assessment, proposal) = crate::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &binding,
            &action_spec,
            &request.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: request.action_id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(
            &request.id,
            mechanism,
            &assessment,
            &input,
            &format!("draft-assessment-{label}"),
        )
        .await
        .unwrap();
        db.record_lead_proposal(&request.id, &proposal, &format!("draft-proposal-{label}"))
            .await
            .unwrap();
        let (plan, request) = if make_ready {
            db.freeze_lead_plan_to_ready(&request.id, &format!("draft-freeze-{label}"))
                .await
                .unwrap()
        } else {
            let plan = db
                .freeze_lead_plan(&request.id, &format!("draft-freeze-{label}"))
                .await
                .unwrap();
            let request = db.lead_request(&request.id).await.unwrap();
            (plan, request)
        };
        let (spec_sha256,): (String,) =
            sqlx::query_as::query_as("SELECT spec_sha256 FROM action_specs WHERE action_id=?1")
                .bind(&request.action_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        DraftReady {
            request,
            plan,
            spec_sha256,
            config_hash: config.config_hash,
            grant,
            proposal,
        }
    }

    fn expected_binding(ready: &DraftReady) -> ExpectedStartBinding {
        ExpectedStartBinding::new(
            &ready.request.request_hash,
            &ready.plan.plan_hash,
            &ready.spec_sha256,
            &ready.config_hash,
            &ready.grant.content_hash,
            None,
            None,
        )
        .unwrap()
    }

    fn operator_authority(principal: &str, label: Option<&str>, rationale: &str) -> StartAuthority {
        StartAuthority::Operator(
            AuthenticatedOperatorApproval::new(principal, label, rationale).unwrap(),
        )
    }

    fn autonomous_authority() -> StartAuthority {
        StartAuthority::Autonomous(
            ValidatedAutonomyContext::from_deployment_policy(true)
                .expect("test autonomy policy is enabled"),
        )
    }

    fn lead_model_call_authorization(
        request_id: &str,
        grant: &StandingDelegation,
    ) -> NewModelCallAuthorization {
        NewModelCallAuthorization {
            role: ModelCallRole::Lead,
            principal_id: grant.principal_id.clone(),
            project_id: grant.project_id.clone(),
            objective_id: request_id.into(),
            scope_id: grant.id.clone(),
            provider: grant.route.provider.clone(),
            model: grant.route.model.clone(),
            purpose: "lead_assessment".into(),
            request_sha256: "a".repeat(64),
            estimated_input_tokens: 100,
            requested_output_tokens: 100,
            estimated_cost_micros: 100,
            policy: ModelCallPolicy {
                max_input_tokens: 1_000,
                max_output_tokens: 1_000,
                max_cost_micros: 10_000,
            },
            global_policy: None,
        }
    }

    async fn count_rows(db: &Database, sql: &str) -> i64 {
        let (value,): (i64,) = sqlx::query_as::query_as(sql)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        value
    }

    async fn lead_start_row_counts(db: &Database) -> (i64, i64, i64, i64) {
        let campaigns = count_rows(db, "SELECT COUNT(*) FROM campaigns").await;
        let runs = count_rows(db, "SELECT COUNT(*) FROM runs").await;
        let attempts = count_rows(db, "SELECT COUNT(*) FROM attempts").await;
        let authorizations = count_rows(db, "SELECT COUNT(*) FROM campaign_authorizations").await;
        (campaigns, runs, attempts, authorizations)
    }

    #[tokio::test]
    async fn full_request_assessment_proposal_and_freeze_flow_is_durable() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let (request, created) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "request-key",
            )
            .await
            .unwrap();
        assert!(created);
        assert_eq!(request.state, LeadRequestState::Submitted);
        assert_eq!(db.delegation_usage("grant").await.unwrap().sessions, 1);

        let assessing = db.begin_lead_assessment(&request.id).await.unwrap();
        assert_eq!(assessing.state, LeadRequestState::Assessing);

        let mut value = assessment();
        value.request_id = request.id.clone();
        value.action_id = action_id.clone();
        value.proposal.as_mut().expect("proposal").action_id = action_id.clone();
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action_id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(&request.id, "MODEL", &value, &input, "assessment-key")
            .await
            .unwrap();
        assert_eq!(
            db.lead_request(&request.id).await.unwrap().state,
            LeadRequestState::Planned
        );

        let proposal = value.proposal.clone().unwrap();
        db.record_lead_proposal(&request.id, &proposal, "proposal-key")
            .await
            .unwrap();
        let frozen = db
            .freeze_lead_plan(&request.id, "freeze-key")
            .await
            .unwrap();
        assert_eq!(
            db.lead_request(&request.id).await.unwrap().state,
            LeadRequestState::Frozen
        );
        assert_eq!(frozen.plan_hash.len(), 64);
        let ready = db.mark_lead_ready(&request.id).await.unwrap();
        assert_eq!(ready.state, LeadRequestState::Ready);
        assert_eq!(
            db.frozen_lead_plan(&request.id).await.unwrap().plan_hash,
            frozen.plan_hash
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn lead_replay_is_idempotent_and_state_order_is_enforced() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let (first, _) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "request-key",
            )
            .await
            .unwrap();
        let (replay, created) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "request-key",
            )
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(replay.id, first.id);
        let value = assessment();
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action_id.clone(),
            request_id: first.id.clone(),
            objective: first.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        assert!(
            db.record_lead_assessment(&first.id, "MODEL", &value, &input, "assessment-key")
                .await
                .is_err()
        );
        assert!(db.freeze_lead_plan(&first.id, "freeze-key").await.is_err());
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn fixture_assessment_and_freeze_flow_is_reachable_through_storage() {
        let (path, db, grant) = fixture().await;
        let (acceptance, negative) = checks();
        let checks: Vec<SpecCheck> = acceptance.into_iter().chain(negative).collect();
        let action = db
            .create_action_with_spec(
                "project",
                "Implement the feature.",
                "opencode",
                "deepseek-v4.1-flash",
                "fixture-action-key",
                ActionSpecInput {
                    allowed_paths: &["src".to_string()],
                    checks: &checks,
                },
            )
            .await
            .unwrap();
        db.classify_action(
            &action.id,
            "R0",
            "test-owner",
            "deterministic R0 classification",
            "fixture-action-classification",
        )
        .await
        .unwrap();
        let config = db.project_config("project").await.unwrap();
        let (request, _) = db
            .create_lead_request(
                "project",
                &action.id,
                "grant",
                "Implement the feature.",
                &config.config_hash,
                "fixture-request-key",
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let binding = ActionBinding {
            id: action.id.clone(),
            project_id: "project".into(),
            status: crate::lead::ActionStatus::Pending,
        };
        let (assessment, proposal) = crate::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &binding,
            &action.spec,
            &request.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action.id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(
            &request.id,
            "FIXTURE",
            &assessment,
            &input,
            "fixture-assessment-key",
        )
        .await
        .unwrap();
        db.record_lead_proposal(&request.id, &proposal, "fixture-proposal-key")
            .await
            .unwrap();
        db.freeze_lead_plan(&request.id, "fixture-freeze-key")
            .await
            .unwrap();
        assert_eq!(
            db.mark_lead_ready(&request.id).await.unwrap().state,
            LeadRequestState::Ready
        );
        let usage_before = db.delegation_usage("grant").await.unwrap();
        let started = db
            .start_lead_campaign(
                &request.id,
                "fixture-start-key",
                autonomous_authority(),
                None,
                None,
            )
            .await
            .unwrap();
        let (campaign, run, attempt) = (started.campaign, started.run, started.attempt);
        assert_eq!(campaign.status, "AUTHORIZED");
        assert_eq!(run.status, "QUEUED");
        assert_eq!(attempt.status, "QUEUED");
        assert_eq!(db.delegation_usage("grant").await.unwrap(), usage_before);
        let authorization = db.campaign_authorization(&campaign.id).await.unwrap();
        assert_eq!(authorization.execution_mode, "FIXTURE");
        assert_eq!(authorization.actor, "lead-autonomous-start");
        assert!(authorization.rationale.contains("delegation grant"));
        assert_eq!(authorization.authorized_paths, vec!["src".to_string()]);
        assert_eq!(authorization.delegation_id, "grant");
        assert_eq!(authorization.provider, "opencode");
        assert_eq!(authorization.model, "deepseek-v4.1-flash");
        assert_eq!(authorization.provider_mode, "build");
        assert_eq!(authorization.allowed_tools, Vec::<String>::new());
        let running = db.lead_request(&request.id).await.unwrap();
        assert_eq!(running.state, LeadRequestState::Running);
        assert_eq!(running.campaign_id.as_deref(), Some(campaign.id.as_str()));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sequential_workgraph_start_advances_to_the_next_unit() {
        let (path, db, grant) = fixture().await;
        let (acceptance, negative) = checks();
        let checks: Vec<SpecCheck> = acceptance.into_iter().chain(negative).collect();
        let action = db
            .create_action_with_spec(
                "project",
                "Implement two units.",
                "opencode",
                "deepseek-v4.1-flash",
                "chain-action-key",
                ActionSpecInput {
                    allowed_paths: &["src".to_string()],
                    checks: &checks,
                },
            )
            .await
            .unwrap();
        db.classify_action(
            &action.id,
            "R0",
            "test-owner",
            "deterministic R0 classification",
            "fixture-action-classification",
        )
        .await
        .unwrap();
        let config = db.project_config("project").await.unwrap();
        let (request, _) = db
            .create_lead_request(
                "project",
                &action.id,
                "grant",
                "Implement two units.",
                &config.config_hash,
                "chain-request-key",
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let binding = ActionBinding {
            id: action.id.clone(),
            project_id: "project".into(),
            status: crate::lead::ActionStatus::Pending,
        };
        let (assessment, mut proposal) = crate::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &binding,
            &action.spec,
            &request.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        proposal.budget.max_attempts = 1;
        proposal.max_sessions = 3;
        proposal.work_units = vec![
            crate::lead::LeadWorkUnit {
                id: "first".into(),
                kind: crate::lead::WorkUnitKind::Implementation,
                description: "First unit.".into(),
                depends_on: Vec::new(),
                allowed_paths: vec!["src".into()],
                capabilities: config.capabilities.clone(),
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            },
            crate::lead::LeadWorkUnit {
                id: "second".into(),
                kind: crate::lead::WorkUnitKind::Implementation,
                description: "Second unit.".into(),
                depends_on: vec!["first".into()],
                allowed_paths: vec!["src".into()],
                capabilities: config.capabilities.clone(),
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            },
        ];
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action.id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(
            &request.id,
            "FIXTURE",
            &assessment,
            &input,
            "chain-assessment-key",
        )
        .await
        .unwrap();
        db.record_lead_proposal(&request.id, &proposal, "chain-proposal-key")
            .await
            .unwrap();
        db.freeze_lead_plan(&request.id, "chain-freeze-key")
            .await
            .unwrap();
        db.mark_lead_ready(&request.id).await.unwrap();
        let attempt = db
            .start_lead_campaign(
                &request.id,
                "chain-start-key",
                autonomous_authority(),
                None,
                None,
            )
            .await
            .unwrap()
            .attempt;
        let count: (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM lead_tasks WHERE request_id=?1")
                .bind(&request.id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(count.0, 2);
        let candidate = Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,integration,lineage_json)
             VALUES (?1,?2,?3,?4,?5,'test-worker',?6,0,'[]')",
        )
        .bind(&candidate)
        .bind(&attempt.id)
        .bind("e".repeat(64))
        .bind("c".repeat(40))
        .bind("d".repeat(40))
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        let mut transaction = db.pool.begin().await.unwrap();
        advance_lead_task(&mut transaction, &attempt.id, &"c".repeat(40))
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let second: (String, Option<String>) = sqlx::query_as::query_as(
            "SELECT status,attempt_id FROM lead_tasks WHERE request_id=?1 AND unit_id='second'",
        )
        .bind(&request.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(second.0, "RUNNING");
        assert!(second.1.is_some());
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn fanin_creates_an_integration_then_schedules_the_dependent_unit() {
        let (path, db, grant) = fixture().await;
        let (acceptance, negative) = checks();
        let checks: Vec<SpecCheck> = acceptance.into_iter().chain(negative).collect();
        let action = db
            .create_action_with_spec(
                "project",
                "Fan-in graph.",
                "opencode",
                "deepseek-v4.1-flash",
                "fanin-action-key",
                ActionSpecInput {
                    allowed_paths: &["src".to_string()],
                    checks: &checks,
                },
            )
            .await
            .unwrap();
        db.classify_action(&action.id, "R0", "test-owner", "R0", "fanin-classification")
            .await
            .unwrap();
        let config = db.project_config("project").await.unwrap();
        let (request, _) = db
            .create_lead_request(
                "project",
                &action.id,
                "grant",
                "Fan-in graph.",
                &config.config_hash,
                "fanin-request-key",
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let binding = ActionBinding {
            id: action.id.clone(),
            project_id: "project".into(),
            status: crate::lead::ActionStatus::Pending,
        };
        let (mut assessment, mut proposal) = crate::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &binding,
            &action.spec,
            &request.objective,
            &grant,
            Utc::now(),
        )
        .unwrap();
        proposal.budget.max_attempts = 1;
        proposal.max_sessions = 4;
        proposal.work_units = vec![
            crate::lead::LeadWorkUnit {
                id: "root-a".into(),
                kind: crate::lead::WorkUnitKind::Implementation,
                description: "Root A.".into(),
                depends_on: Vec::new(),
                allowed_paths: vec!["src/a".into()],
                capabilities: config.capabilities.clone(),
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            },
            crate::lead::LeadWorkUnit {
                id: "root-b".into(),
                kind: crate::lead::WorkUnitKind::Implementation,
                description: "Root B.".into(),
                depends_on: Vec::new(),
                allowed_paths: vec!["src/b".into()],
                capabilities: config.capabilities.clone(),
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            },
            crate::lead::LeadWorkUnit {
                id: "fanin".into(),
                kind: crate::lead::WorkUnitKind::Implementation,
                description: "Fan in.".into(),
                depends_on: vec!["root-a".into(), "root-b".into()],
                allowed_paths: vec!["src/c".into()],
                capabilities: config.capabilities.clone(),
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            },
        ];
        assessment.proposal = Some(proposal.clone());
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action.id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(
            &request.id,
            "FIXTURE",
            &assessment,
            &input,
            "fanin-assessment",
        )
        .await
        .unwrap();
        db.record_lead_proposal(&request.id, &proposal, "fanin-proposal")
            .await
            .unwrap();
        db.freeze_lead_plan(&request.id, "fanin-freeze")
            .await
            .unwrap();
        db.mark_lead_ready(&request.id).await.unwrap();
        db.start_lead_campaign(
            &request.id,
            "fanin-start",
            autonomous_authority(),
            None,
            None,
        )
        .await
        .unwrap();
        let tasks: Vec<(String, String, Option<String>)> = sqlx::query_as::query_as(
            "SELECT unit_id,status,attempt_id FROM lead_tasks WHERE request_id=?1 ORDER BY sequence",
        )
        .bind(&request.id)
        .fetch_all(&db.pool)
        .await
        .unwrap();
        let root_a_attempt = tasks
            .iter()
            .find(|(unit, _, _)| unit == "root-a")
            .and_then(|(_, _, attempt)| attempt.clone())
            .unwrap();
        let root_b_attempt = tasks
            .iter()
            .find(|(unit, _, _)| unit == "root-b")
            .and_then(|(_, _, attempt)| attempt.clone())
            .unwrap();
        for (attempt, commit) in [
            (&root_a_attempt, "a".repeat(40)),
            (&root_b_attempt, "b".repeat(40)),
        ] {
            sqlx::query::query(
                "INSERT INTO candidates
                 (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,integration,lineage_json)
                 VALUES (?1,?2,?3,?4,?5,'test-worker',?6,0,'[]')",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(attempt)
            .bind("e".repeat(64))
            .bind(&commit)
            .bind("d".repeat(40))
            .bind(Utc::now().to_rfc3339())
            .execute(&db.pool)
            .await
            .unwrap();
            let mut transaction = db.pool.begin().await.unwrap();
            advance_lead_task(&mut transaction, attempt, &commit)
                .await
                .unwrap();
            transaction.commit().await.unwrap();
        }
        let integration: (String, String, Option<String>) = sqlx::query_as::query_as(
            "SELECT purpose,status,attempt_id FROM lead_integrations WHERE campaign_id=(
               SELECT campaign_id FROM lead_requests WHERE id=?1
             )",
        )
        .bind(&request.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(integration.0, "FANIN");
        assert_eq!(integration.1, "QUEUED");
        let integration_attempt = integration.2.unwrap();
        let integrated = Uuid::new_v4().to_string();
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,integration,lineage_json)
             VALUES (?1,?2,?3,?4,?5,'test-worker',?6,1,?7)",
        )
        .bind(&integrated)
        .bind(&integration_attempt)
        .bind("f".repeat(64))
        .bind("c".repeat(40))
        .bind("d".repeat(40))
        .bind(Utc::now().to_rfc3339())
        .bind(serde_json::json!(["a".repeat(40), "b".repeat(40)]).to_string())
        .execute(&db.pool)
        .await
        .unwrap();
        let mut transaction = db.pool.begin().await.unwrap();
        advance_lead_task(&mut transaction, &integration_attempt, &"c".repeat(40))
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let fanin: (String, Option<String>) = sqlx::query_as::query_as(
            "SELECT status,attempt_id FROM lead_tasks WHERE request_id=?1 AND unit_id='fanin'",
        )
        .bind(&request.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(fanin.0, "RUNNING");
        assert!(fanin.1.is_some());
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn pre_model_block_releases_the_reserved_assessment_session() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let (request, _) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "release-request",
            )
            .await
            .unwrap();
        assert_eq!(db.delegation_usage("grant").await.unwrap().sessions, 1);
        db.block_lead_request(&request.id, "no provider authorization")
            .await
            .unwrap();
        assert_eq!(db.delegation_usage("grant").await.unwrap().sessions, 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revocation_blocks_an_open_lead_request() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let (request, _) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "revoke-open-request",
            )
            .await
            .unwrap();
        db.revoke_standing_delegation("grant", Utc::now())
            .await
            .unwrap();
        let blocked = db.lead_request(&request.id).await.unwrap();
        assert_eq!(blocked.state, LeadRequestState::Blocked);
        assert!(
            blocked
                .failure_reason
                .as_deref()
                .is_some_and(|value| value.contains("revoked"))
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn freeze_rejects_revocation_after_proposal() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let (request, _) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "revoke-freeze-request",
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let mut value = assessment();
        value.action_id = action_id.clone();
        value.request_id = request.id.clone();
        if let Some(proposal) = value.proposal.as_mut() {
            proposal.action_id = action_id.clone();
        }
        let input = AssessmentInput {
            project_id: "project".into(),
            action_id: action_id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from([
                "repository-files".to_string()
            ]),
        };
        db.record_lead_assessment(
            &request.id,
            "MODEL",
            &value,
            &input,
            "revoke-freeze-assessment",
        )
        .await
        .unwrap();
        let proposal = value.proposal.clone().unwrap();
        db.record_lead_proposal(&request.id, &proposal, "revoke-freeze-proposal")
            .await
            .unwrap();
        db.revoke_standing_delegation("grant", Utc::now())
            .await
            .unwrap();
        assert!(matches!(
            db.freeze_lead_plan(&request.id, "revoke-freeze-freeze")
                .await,
            Err(Error::Forbidden(_) | Error::Conflict(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn campaign_cancellation_fences_open_work_and_is_idempotent() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query("INSERT INTO campaigns VALUES ('campaign-cancel',?1,'AUTHORIZED',?2)")
            .bind(&action_id)
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query("INSERT INTO runs VALUES ('run-cancel','campaign-cancel','QUEUED',?1)")
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES ('attempt-cancel','run-cancel','LEASED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO worker_leases VALUES ('attempt-cancel','worker-cancel',1,9999999999999)",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES ('integration-cancel','run-cancel','LEASED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO worker_leases VALUES ('integration-cancel','worker-cancel',1,9999999999999)",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_integrations
             (integration_id,campaign_id,purpose,next_task_id,attempt_id,status,candidates_json,
              candidate_id,created_at,updated_at)
             VALUES ('integration-cancel','campaign-cancel','FANIN','','integration-cancel',
                     'QUEUED','[]',NULL,?1,?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,
              state,failure_reason,created_at,updated_at,campaign_id)
             VALUES ('request-cancel','project',?1,'grant','objective','request-hash','config-hash',
                     'RUNNING',NULL,?2,?2,'campaign-cancel')",
        )
        .bind(&action_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_tasks
             (task_id,campaign_id,request_id,unit_id,sequence,status,base_commit,spec_json,
              spec_sha256,depends_on_json,run_id,attempt_id,candidate_id,created_at,updated_at)
             VALUES ('task-cancel','campaign-cancel','request-cancel','unit',0,'RUNNING',?1,'{}',
                     'spec-hash','[]','run-cancel','attempt-cancel',NULL,?2,?2)",
        )
        .bind("b".repeat(40))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();

        sqlx::query::query(
            "INSERT INTO provider_sessions
             (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,state,
              provider_session_id,provider_directory,created_at,updated_at)
             VALUES ('provider-cancel','attempt-cancel','worker-cancel',1,'opencode',
                     'deepseek-v4.1-flash','build','ACTIVE','ses_cancel','/tmp/provider',?1,?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();

        let cancelled = db
            .cancel_campaign("campaign-cancel", "operator", "owner request")
            .await
            .unwrap();
        assert_eq!(cancelled.status, "CANCELLED");

        let (run_status,): (String,) =
            sqlx::query_as::query_as("SELECT status FROM runs WHERE id='run-cancel'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(run_status, "CANCELLED");
        let (attempt_status,): (String,) =
            sqlx::query_as::query_as("SELECT status FROM attempts WHERE id='attempt-cancel'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(attempt_status, "CANCELLED");
        let (task_status,): (String,) =
            sqlx::query_as::query_as("SELECT status FROM lead_tasks WHERE task_id='task-cancel'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(task_status, "CANCELLED");
        let (request_state, failure_reason): (String, Option<String>) = sqlx::query_as::query_as(
            "SELECT state,failure_reason FROM lead_requests WHERE id='request-cancel'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(request_state, "BLOCKED");
        assert!(
            failure_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("owner request"))
        );
        let (leases,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM worker_leases WHERE attempt_id IN ('attempt-cancel','integration-cancel')",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(leases, 0);
        let (integration_status,): (String,) = sqlx::query_as::query_as(
            "SELECT status FROM lead_integrations WHERE integration_id='integration-cancel'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(integration_status, "CANCELLED");
        let (provider_state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM provider_sessions WHERE session_record_id='provider-cancel'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(provider_state, "CANCEL_REQUESTED");

        let again = db
            .cancel_campaign("campaign-cancel", "operator", "repeat")
            .await
            .unwrap();
        assert_eq!(again.status, "CANCELLED");
        let (event_count,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM events WHERE subject_id='campaign-cancel' AND kind='campaign.cancelled'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1);

        let (audit,): (String,) =
            sqlx::query_as::query_as("SELECT detail FROM events WHERE subject_id='campaign-cancel' AND kind='campaign.cancelled'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(audit.contains("owner request"));
        assert_eq!(
            db.cancel_campaign("missing-campaign", "operator", "x")
                .await
                .unwrap_err()
                .to_string(),
            "record not found"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revoked_delegation_and_duplicate_action_fail_closed() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        db.revoke_standing_delegation("grant", Utc::now())
            .await
            .unwrap();
        assert!(matches!(
            db.create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config_hash(),
                "request-key"
            )
            .await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn lead_model_calls_are_scoped_to_the_request_and_role() {
        let (path, db, _) = fixture().await;
        let ready = ready_lead(&db, "MODEL").await;
        let now = Utc::now().to_rfc3339();
        for (reservation, role, provider) in [
            ("lead-call", "LEAD", "opencode"),
            ("reviewer-call", "SEMANTIC_REVIEWER", "opencode-go"),
        ] {
            sqlx::query::query(
                "INSERT INTO model_call_reservations
                 (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
                  purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
                  max_cost_micros,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                  response_text,state,error,expires_at_ms,created_at,updated_at)
                 VALUES (?1,?2,'owner','project',?3,'grant',?4,'deepseek-v4.1-flash',
                         'lead_assessment',?5,10,20,30,100,10,20,30,25,1,
                         NULL,'SETTLED',NULL,0,?6,?6)",
            )
            .bind(reservation)
            .bind(role)
            .bind(&ready.request.id)
            .bind(provider)
            .bind("a".repeat(64))
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        }
        let calls = db.lead_model_calls(&ready.request.id).await.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].reservation_id, "lead-call");
        assert_eq!(calls[0].role, ModelCallRole::Lead);
        assert_eq!(calls[0].provider, "opencode");
        assert_eq!(calls[0].model, "deepseek-v4.1-flash");
        assert_eq!(calls[0].input_tokens, 10);
        assert_eq!(calls[0].output_tokens, 20);
        assert_eq!(calls[0].total_tokens, 30);
        assert_eq!(calls[0].cost_micros, 25);
        assert_eq!(calls[0].state, "SETTLED");
        assert!(calls[0].usage_known);
        assert_eq!(calls[0].objective_id, ready.request.id);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn model_lead_start_requires_explicit_authorization_and_leaves_no_rows() {
        let (path, db, _) = fixture().await;
        let ready = ready_lead(&db, "MODEL").await;
        let error = db
            .start_lead_campaign(
                &ready.request.id,
                "model-start-missing",
                autonomous_authority(),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Invalid(_)));
        assert_eq!(lead_start_row_counts(&db).await, (0, 0, 0, 0));
        assert_eq!(
            db.lead_request(&ready.request.id).await.unwrap().state,
            LeadRequestState::Ready
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn model_lead_start_records_exact_authorization_without_double_reservation() {
        let (path, db, _) = fixture().await;
        let ready = ready_lead(&db, "MODEL").await;
        assert_eq!(ready.proposal.budget.max_attempts, 2);
        assert_eq!(ready.proposal.max_sessions, 3);
        let usage_before = db.delegation_usage("grant").await.unwrap();
        assert_eq!(usage_before.requests, 1);
        assert_eq!(usage_before.sessions, 3);
        assert_eq!(usage_before.attempts, 0);
        let authorization = model_authorization(&ready.proposal);
        let started = db
            .start_lead_campaign(
                &ready.request.id,
                "model-start-valid",
                autonomous_authority(),
                None,
                Some(&authorization),
            )
            .await
            .unwrap();
        let (campaign, run, attempt) = (started.campaign, started.run, started.attempt);
        assert_eq!(campaign.status, "AUTHORIZED");
        assert_eq!(run.campaign_id, campaign.id);
        assert_eq!(attempt.run_id, run.id);
        let stored = db.campaign_authorization(&campaign.id).await.unwrap();
        assert_eq!(stored.authorization_id, authorization.authorization_id);
        assert_eq!(stored.risk_class, "R0");
        assert_eq!(stored.execution_mode, "TRUSTED_LOCAL_PROVIDER");
        assert_eq!(stored.actor, authorization.actor);
        assert_eq!(stored.rationale, authorization.rationale);
        assert_eq!(stored.authorized_paths, authorization.authorized_paths);
        assert_eq!(stored.max_attempts, authorization.max_attempts);
        assert_eq!(stored.provider, authorization.provider);
        assert_eq!(stored.model, authorization.model);
        assert_eq!(stored.provider_mode, authorization.provider_mode);
        assert_eq!(stored.delegation_id, authorization.delegation_id);
        assert_eq!(stored.max_sessions, authorization.max_sessions);
        assert_eq!(stored.max_turns, authorization.max_turns);
        assert_eq!(stored.max_input_tokens, authorization.max_input_tokens);
        assert_eq!(stored.max_output_tokens, authorization.max_output_tokens);
        assert_eq!(stored.max_cost_micros, authorization.max_cost_micros);
        assert_eq!(stored.deadline_secs, authorization.deadline_secs);
        assert_eq!(stored.allowed_tools, authorization.allowed_tools);
        assert_eq!(lead_start_row_counts(&db).await, (1, 1, 1, 1));
        let usage_after = db.delegation_usage("grant").await.unwrap();
        assert_eq!(usage_after.requests, usage_before.requests);
        assert_eq!(usage_after.sessions, usage_before.sessions);
        assert_eq!(
            usage_after.attempts,
            usage_before.attempts + authorization.max_attempts
        );
        assert_eq!(
            db.lead_request(&ready.request.id).await.unwrap().state,
            LeadRequestState::Running
        );

        let replayed = db
            .start_lead_campaign(
                &ready.request.id,
                "model-start-valid",
                autonomous_authority(),
                None,
                Some(&authorization),
            )
            .await
            .unwrap();
        assert!(replayed.replayed);
        assert_eq!(replayed.campaign.id, campaign.id);
        assert_eq!(replayed.run.id, run.id);
        assert_eq!(replayed.attempt.id, attempt.id);
        assert_eq!(db.delegation_usage("grant").await.unwrap(), usage_after);

        let mut different = authorization.clone();
        different.max_attempts = 2;
        assert!(matches!(
            db.start_lead_campaign(
                &ready.request.id,
                "model-start-valid",
                autonomous_authority(),
                None,
                Some(&different)
            )
            .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(db.delegation_usage("grant").await.unwrap(), usage_after);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn model_lead_start_rejects_a_reused_authorization_id_without_partial_rows() {
        let (path, db, _) = fixture().await;
        let first = ready_lead(&db, "MODEL").await;
        let second = ready_lead(&db, "MODEL").await;
        let authorization = model_authorization(&first.proposal);
        db.start_lead_campaign(
            &first.request.id,
            "duplicate-first",
            autonomous_authority(),
            None,
            Some(&authorization),
        )
        .await
        .unwrap();
        let error = db
            .start_lead_campaign(
                &second.request.id,
                "duplicate-second",
                autonomous_authority(),
                None,
                Some(&authorization),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Conflict(_)));
        assert_eq!(lead_start_row_counts(&db).await, (1, 1, 1, 1));
        assert_eq!(
            db.lead_request(&second.request.id).await.unwrap().state,
            LeadRequestState::Ready
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn model_lead_start_rejects_invalid_route_budget_delegation_and_paths() {
        let (path, db, _) = fixture().await;
        let ready = ready_lead(&db, "MODEL").await;
        let usage_before = db.delegation_usage("grant").await.unwrap();
        let base = model_authorization(&ready.proposal);
        let mut fixtures = Vec::new();
        let mut wrong_route = base.clone();
        wrong_route.provider = "codex".into();
        fixtures.push(("route", wrong_route));
        let mut wrong_delegation = base.clone();
        wrong_delegation.delegation_id = "another-grant".into();
        fixtures.push(("delegation", wrong_delegation));
        let mut attempts = base.clone();
        attempts.max_attempts = base.max_attempts + 2;
        fixtures.push(("attempts", attempts));
        let mut sessions = base.clone();
        sessions.max_sessions = ready.proposal.max_sessions;
        fixtures.push(("sessions", sessions));
        let mut paths = base.clone();
        paths.authorized_paths = vec!["src".into(), "tests".into()];
        fixtures.push(("paths", paths));
        let mut fixture_mode = base.clone();
        fixture_mode.execution_mode = "FIXTURE".into();
        fixtures.push(("fixture-mode", fixture_mode));
        for (label, authorization) in fixtures {
            let error = db
                .start_lead_campaign(
                    &ready.request.id,
                    &format!("model-start-invalid-{label}"),
                    autonomous_authority(),
                    None,
                    Some(&authorization),
                )
                .await
                .unwrap_err();
            assert!(
                !matches!(error, Error::Sqlx(_) | Error::Migrate(_) | Error::File(_)),
                "invalid {label} start returned a server error: {error}"
            );
            assert_eq!(
                lead_start_row_counts(&db).await,
                (0, 0, 0, 0),
                "invalid {label} start left rows"
            );
            assert_eq!(
                db.delegation_usage("grant").await.unwrap(),
                usage_before,
                "invalid {label} start changed delegation usage"
            );
            assert_eq!(
                db.lead_request(&ready.request.id).await.unwrap().state,
                LeadRequestState::Ready
            );
        }
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn draft_action_spec_is_created_once_at_freeze_and_binds_campaign() {
        let (path, db, _) = fixture().await;
        let config = db.project_config("project").await.unwrap();
        let grant = operator_grant(&db, "binding-grant").await;
        let (draft, created) = db
            .create_draft_lead_request(NewDraftLeadRequest {
                project_id: "project",
                delegation_id: &grant.id,
                objective: "Implement the feature.",
                project_config_hash: &config.config_hash,
                assessment_input_json: r#"{"objective":"Implement the feature.","projectId":"project"}"#,
                idempotency_key: "binding-draft-request",
            })
            .await
            .unwrap();
        assert!(created);
        let (spec_count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM action_specs WHERE action_id=?1")
                .bind(&draft.action_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(spec_count, 0);
        let stored_draft = db.action_draft(&draft.action_id).await.unwrap();
        assert_eq!(
            stored_draft.input_sha256,
            sha256_hex(stored_draft.assessment_input_json.as_bytes())
        );

        let classified = db
            .create_action_with_spec(
                "project",
                "A separately classified action",
                "opencode",
                "deepseek-v4.1-flash",
                "binding-classified-action",
                ActionSpecInput {
                    allowed_paths: &["src".to_string()],
                    checks: &[],
                },
            )
            .await
            .unwrap();
        let coexistence = sqlx::query::query(
            "INSERT INTO action_drafts(action_id,assessment_input_json,input_sha256,created_at)
             VALUES (?1,'{}',?2,?3)",
        )
        .bind(&classified.id)
        .bind("a".repeat(64))
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await;
        assert!(
            coexistence.is_err(),
            "draft/spec coexistence must be rejected"
        );

        let ready = draft_ready_lead(&db, "binding").await;
        let (spec_json_before, spec_sha_before): (String, String) = sqlx::query_as::query_as(
            "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
        )
        .bind(&ready.request.action_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(spec_sha_before, ready.spec_sha256);
        let replayed_plan = db
            .freeze_lead_plan(&ready.request.id, "draft-freeze-binding")
            .await
            .unwrap();
        assert_eq!(replayed_plan, ready.plan);
        let (spec_json_after, spec_sha_after): (String, String) = sqlx::query_as::query_as(
            "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id=?1",
        )
        .bind(&ready.request.action_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(spec_json_before, spec_json_after);
        assert_eq!(spec_sha_before, spec_sha_after);
        let draft_after = db.action_draft(&ready.request.action_id).await.unwrap();
        assert_eq!(draft_after.input_sha256, stored_draft.input_sha256);

        let expected = expected_binding(&ready);
        let started = db
            .start_lead_campaign(
                &ready.request.id,
                "binding-operator-start",
                operator_authority("device:operator", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap();
        let binding = db
            .campaign_execution_binding(&started.campaign.id)
            .await
            .unwrap()
            .expect("Campaign execution binding");
        assert_eq!(binding.request_id, ready.request.id);
        assert_eq!(binding.plan_id, ready.plan.id);
        assert_eq!(binding.spec_sha256, spec_sha_before);
        assert_eq!(binding.envelope_hash.len(), 64);
        let evidence = db
            .lead_start_evidence(&ready.request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.mode, "OPERATOR");
        assert_eq!(evidence.spec_sha256, spec_sha_before);

        assert!(
            sqlx::query::query("UPDATE action_specs SET spec_json='{}' WHERE action_id=?1")
                .bind(&ready.request.action_id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query::query("DELETE FROM action_specs WHERE action_id=?1")
                .bind(&ready.request.action_id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query::query("UPDATE attempt_specs SET spec_json='{}' WHERE attempt_id=?1")
                .bind(&started.attempt.id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query::query("DELETE FROM attempt_specs WHERE attempt_id=?1")
                .bind(&started.attempt.id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query::query(
                "UPDATE campaign_execution_bindings SET envelope_hash='bad' WHERE campaign_id=?1"
            )
            .bind(&started.campaign.id)
            .execute(&db.pool)
            .await
            .is_err()
        );
        assert!(
            sqlx::query::query("DELETE FROM lead_start_evidence WHERE request_id=?1")
                .bind(&ready.request.id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn operator_start_requires_authenticated_approval_and_exact_binding() {
        let (path, db, _) = fixture().await;
        let ready = draft_ready_lead(&db, "operator").await;
        let usage_before = db.delegation_usage(&ready.grant.id).await.unwrap();
        let expected = expected_binding(&ready);

        let autonomous = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-autonomous-rejected",
                autonomous_authority(),
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(autonomous, Error::Forbidden(_)));
        assert_eq!(lead_start_row_counts(&db).await, (0, 0, 0, 0));

        let wrong_plan = ExpectedStartBinding::new(
            &ready.request.request_hash,
            &"f".repeat(64),
            &ready.spec_sha256,
            &ready.config_hash,
            &ready.grant.content_hash,
            None,
            None,
        )
        .unwrap();
        let stale = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-stale-plan",
                operator_authority("device:operator", Some("Alice"), "stale plan"),
                Some(&wrong_plan),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(stale, Error::Conflict(_)));
        assert_eq!(lead_start_row_counts(&db).await, (0, 0, 0, 0));

        let profile_bound = ExpectedStartBinding::new(
            &ready.request.request_hash,
            &ready.plan.plan_hash,
            &ready.spec_sha256,
            &ready.config_hash,
            &ready.grant.content_hash,
            Some("profile-revision-1"),
            Some(&"a".repeat(64)),
        )
        .unwrap();
        let unavailable = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-profile-unavailable",
                operator_authority("device:operator", Some("Alice"), "profile bound"),
                Some(&profile_bound),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(unavailable, Error::Forbidden(_)));
        assert_eq!(lead_start_row_counts(&db).await, (0, 0, 0, 0));

        let started = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-start",
                operator_authority("device:operator", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap();
        assert!(!started.replayed);
        let evidence = db
            .lead_start_evidence(&ready.request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.campaign_id, started.campaign.id);
        assert_eq!(evidence.mode, "OPERATOR");
        assert_eq!(evidence.principal_id, "device:operator");
        assert_eq!(evidence.principal_label.as_deref(), Some("Alice"));
        assert_eq!(evidence.rationale, "reviewed the plan");
        assert_eq!(evidence.request_hash, ready.request.request_hash);
        assert_eq!(evidence.plan_hash, ready.plan.plan_hash);
        assert_eq!(evidence.spec_sha256, ready.spec_sha256);
        assert_eq!(evidence.config_hash, ready.config_hash);
        assert_eq!(evidence.delegation_hash, ready.grant.content_hash);
        assert!(evidence.profile_revision_id.is_none());
        assert!(evidence.profile_hash.is_none());
        assert_eq!(evidence.policy_hash.len(), 64);
        assert_eq!(evidence.authorization_digest.len(), 64);
        let authorization = db
            .campaign_authorization(&started.campaign.id)
            .await
            .unwrap();
        assert_eq!(authorization.actor, "Alice");
        assert_eq!(authorization.execution_mode, "FIXTURE");
        let action = db.action(&ready.request.action_id).await.unwrap();
        assert_eq!(action.risk, "R0");
        assert_eq!(action.status, "AUTHORIZED");
        assert_eq!(
            db.lead_request(&ready.request.id).await.unwrap().state,
            LeadRequestState::Running
        );
        assert_eq!(
            db.lead_request_projection(&ready.request.id).await.unwrap(),
            crate::lead::LeadRequestProjectionState::Executing
        );
        assert_eq!(
            db.delegation_usage(&ready.grant.id).await.unwrap(),
            usage_before
        );

        let replay = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-start",
                operator_authority("device:operator", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.campaign.id, started.campaign.id);
        assert_eq!(replay.run.id, started.run.id);
        assert_eq!(replay.attempt.id, started.attempt.id);
        assert_eq!(
            db.delegation_usage(&ready.grant.id).await.unwrap(),
            usage_before
        );

        let changed_principal = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-start",
                operator_authority("device:intruder", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(changed_principal, Error::Conflict(_)));

        let changed_rationale = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-start",
                operator_authority("device:operator", Some("Alice"), "different rationale"),
                Some(&expected),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(changed_rationale, Error::Conflict(_)));

        let second_key = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-second-key",
                operator_authority("device:operator", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap_err();
        match second_key {
            Error::AlreadyStarted {
                campaign_id,
                campaign_status,
            } => {
                assert_eq!(campaign_id, started.campaign.id);
                assert_eq!(campaign_status, "AUTHORIZED");
            }
            other => panic!("expected typed already-started conflict, got {other:?}"),
        }
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM lead_start_evidence").await,
            1
        );
        assert_eq!(count_rows(&db, "SELECT COUNT(*) FROM campaigns").await, 1);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn withdrawal_is_authoritative_idempotent_and_fences_start_and_freeze() {
        let (path, db, _) = fixture().await;
        let ready = draft_ready_lead(&db, "withdraw").await;
        let withdrawal = db
            .withdraw_lead_request(
                &ready.request.id,
                "device:operator",
                Some("Alice"),
                "objective no longer approved",
            )
            .await
            .unwrap();
        let replay = db
            .withdraw_lead_request(
                &ready.request.id,
                "device:operator",
                Some("Alice"),
                "objective no longer approved",
            )
            .await
            .unwrap();
        assert_eq!(replay, withdrawal);
        let changed = db
            .withdraw_lead_request(
                &ready.request.id,
                "device:operator",
                Some("Alice"),
                "different reason",
            )
            .await
            .unwrap_err();
        assert!(matches!(changed, Error::Conflict(_)));
        let expected = expected_binding(&ready);
        let fenced = db
            .start_lead_campaign(
                &ready.request.id,
                "withdrawn-start",
                operator_authority("device:operator", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(fenced, Error::Withdrawn));
        assert_eq!(lead_start_row_counts(&db).await, (0, 0, 0, 0));
        assert_eq!(
            db.lead_request_projection(&ready.request.id).await.unwrap(),
            crate::lead::LeadRequestProjectionState::Withdrawn
        );
        assert!(
            sqlx::query::query("UPDATE lead_withdrawals SET reason='tampered' WHERE request_id=?1")
                .bind(&ready.request.id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query::query("DELETE FROM lead_withdrawals WHERE request_id=?1")
                .bind(&ready.request.id)
                .execute(&db.pool)
                .await
                .is_err()
        );

        let (unfrozen, _) = db
            .create_draft_lead_request(NewDraftLeadRequest {
                project_id: "project",
                delegation_id: &ready.grant.id,
                objective: "Implement a second feature.",
                project_config_hash: &ready.config_hash,
                assessment_input_json: r#"{"objective":"Implement a second feature."}"#,
                idempotency_key: "withdraw-unfrozen",
            })
            .await
            .unwrap();
        db.withdraw_lead_request(&unfrozen.id, "device:operator", None, "stop")
            .await
            .unwrap();
        assert!(matches!(
            db.begin_lead_assessment(&unfrozen.id).await,
            Err(Error::Withdrawn)
        ));
        assert!(matches!(
            db.freeze_lead_plan_to_ready(&unfrozen.id, "withdraw-freeze")
                .await,
            Err(Error::Withdrawn)
        ));

        let started_ready = draft_ready_lead(&db, "withdraw-after-start").await;
        let expected = expected_binding(&started_ready);
        let started = db
            .start_lead_campaign(
                &started_ready.request.id,
                "start-before-withdraw",
                operator_authority("device:operator", Some("Alice"), "reviewed the plan"),
                Some(&expected),
                None,
            )
            .await
            .unwrap();
        let late = db
            .withdraw_lead_request(
                &started_ready.request.id,
                "device:operator",
                Some("Alice"),
                "too late",
            )
            .await
            .unwrap_err();
        match late {
            Error::AlreadyStarted {
                campaign_id,
                campaign_status,
            } => {
                assert_eq!(campaign_id, started.campaign.id);
                assert_eq!(campaign_status, "AUTHORIZED");
            }
            other => panic!("expected typed Campaign conflict, got {other:?}"),
        }
        assert!(
            db.lead_withdrawal(&started_ready.request.id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db.lead_request_projection(&started_ready.request.id)
                .await
                .unwrap(),
            crate::lead::LeadRequestProjectionState::Executing
        );
        db.cancel_campaign(
            &started.campaign.id,
            "device:operator",
            "operator cancelled",
        )
        .await
        .unwrap();
        assert_eq!(
            db.lead_request_projection(&started_ready.request.id)
                .await
                .unwrap(),
            crate::lead::LeadRequestProjectionState::Cancelled
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn concurrent_operator_starts_and_withdrawal_create_one_outcome_across_databases() {
        let (path, db, _) = fixture().await;
        let second = Database::open(&path).await.unwrap();
        let ready = draft_ready_lead(&db, "race").await;
        let expected = expected_binding(&ready);
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let first_barrier = barrier.clone();
        let first_db = db.clone();
        let first_request = ready.request.id.clone();
        let first_expected = expected.clone();
        let first_key = "race-operator-a".to_string();
        let first = tokio::spawn(async move {
            first_barrier.wait().await;
            first_db
                .start_lead_campaign(
                    &first_request,
                    &first_key,
                    operator_authority("device:operator", Some("Alice"), "approve A"),
                    Some(&first_expected),
                    None,
                )
                .await
        });
        let second_barrier = barrier.clone();
        let second_db = second.clone();
        let second_request = ready.request.id.clone();
        let second_expected = expected.clone();
        let second_key = "race-operator-b".to_string();
        let other = tokio::spawn(async move {
            second_barrier.wait().await;
            second_db
                .start_lead_campaign(
                    &second_request,
                    &second_key,
                    operator_authority("device:operator", Some("Alice"), "approve B"),
                    Some(&second_expected),
                    None,
                )
                .await
        });
        let first = first.await.unwrap();
        let other = other.await.unwrap();
        let successes = usize::from(first.is_ok()) + usize::from(other.is_ok());
        assert_eq!(successes, 1, "exactly one start must win");
        let loser = if let Err(error) = first {
            error
        } else {
            other.unwrap_err()
        };
        match loser {
            Error::AlreadyStarted { .. } => {}
            other => panic!("expected typed already-started conflict, got {other:?}"),
        }
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM lead_start_evidence").await,
            1
        );
        assert_eq!(count_rows(&db, "SELECT COUNT(*) FROM campaigns").await, 1);

        let ready = draft_ready_lead(&db, "race-withdraw").await;
        let expected = expected_binding(&ready);
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let start_barrier = barrier.clone();
        let start_db = db.clone();
        let start_request = ready.request.id.clone();
        let start_expected = expected.clone();
        let start = tokio::spawn(async move {
            start_barrier.wait().await;
            start_db
                .start_lead_campaign(
                    &start_request,
                    "race-start-vs-withdraw",
                    operator_authority("device:operator", Some("Alice"), "race approval"),
                    Some(&start_expected),
                    None,
                )
                .await
        });
        let withdraw_barrier = barrier.clone();
        let withdraw_db = second.clone();
        let withdraw_request = ready.request.id.clone();
        let withdraw = tokio::spawn(async move {
            withdraw_barrier.wait().await;
            withdraw_db
                .withdraw_lead_request(
                    &withdraw_request,
                    "device:operator",
                    Some("Alice"),
                    "race withdrawal",
                )
                .await
        });
        let start = start.await.unwrap();
        let withdraw = withdraw.await.unwrap();
        match (&start, &withdraw) {
            (Ok(started), Err(Error::AlreadyStarted { campaign_id, .. })) => {
                assert_eq!(campaign_id, &started.campaign.id);
            }
            (Err(Error::Withdrawn), Ok(_)) => {}
            other => panic!("start/withdraw race was inconsistent: {other:?}"),
        }
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM campaigns").await,
            if start.is_ok() { 2 } else { 1 }
        );
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM lead_withdrawals").await,
            if withdraw.is_ok() { 1 } else { 0 }
        );
        second.close().await;
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn operator_start_rejects_stale_config_revoked_grant_and_alternate_campaign_paths() {
        let (path, db, _) = fixture().await;
        let revoked = draft_ready_lead(&db, "revoked").await;
        db.revoke_standing_delegation(&revoked.grant.id, Utc::now())
            .await
            .unwrap();
        let expected = expected_binding(&revoked);
        let error = db
            .start_lead_campaign(
                &revoked.request.id,
                "revoked-start",
                operator_authority("device:operator", Some("Alice"), "approval"),
                Some(&expected),
                None,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Forbidden(_) | Error::Conflict(_)),
            "revoked start error: {error:?}"
        );
        assert_eq!(lead_start_row_counts(&db).await, (0, 0, 0, 0));

        let withdrawn = draft_ready_lead(&db, "alternate-withdraw").await;
        db.withdraw_lead_request(
            &withdrawn.request.id,
            "device:operator",
            None,
            "no longer wanted",
        )
        .await
        .unwrap();
        let error = db
            .create_campaign(&withdrawn.request.action_id, "alternate-campaign")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Withdrawn));

        let ready = draft_ready_lead(&db, "alternate-ready").await;
        let error = db
            .create_campaign(&ready.request.action_id, "alternate-ready-campaign")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)));
        let created_at = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO campaigns VALUES ('manual-lead-campaign',?1,'BLOCKED_AUTHORIZATION',?2)",
        )
        .bind(&ready.request.action_id)
        .bind(&created_at)
        .execute(&db.pool)
        .await
        .unwrap();
        let authorization = NewCampaignAuthorization {
            authorization_id: "manual-lead-authorization".into(),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "forged".into(),
            rationale: "bypass attempt".into(),
            authorized_paths: vec!["src".into()],
            max_attempts: 1,
            ..Default::default()
        };
        let error = db
            .authorize_campaign("manual-lead-campaign", &authorization)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)));
        assert_eq!(lead_start_row_counts(&db).await, (1, 0, 0, 0));

        let stale = draft_ready_lead(&db, "stale-config").await;
        let expected = expected_binding(&stale);
        db.update_project_config(
            "project",
            &crate::db::NewProjectConfig {
                expected_revision: 1,
                allowed_paths: vec!["src".into(), "docs".into()],
                capabilities: vec![
                    crate::lead::Capability::RepositoryEdit,
                    crate::lead::Capability::GenericToolchain,
                ],
                quality_profile: crate::lead::QualityProfile::Generic,
                active: true,
            },
        )
        .await
        .unwrap();
        let error = db
            .start_lead_campaign(
                &stale.request.id,
                "stale-config-start",
                operator_authority("device:operator", Some("Alice"), "approval"),
                Some(&expected),
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)));
        assert_eq!(lead_start_row_counts(&db).await, (1, 0, 0, 0));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn operator_start_ignores_forged_provider_actor_and_records_principal_identity() {
        let (path, db, _) = fixture().await;
        let ready = draft_ready_lead_with_mechanism(&db, "forged", "MODEL").await;
        let expected = expected_binding(&ready);
        let authorization = NewCampaignAuthorization {
            authorization_id: "operator-model-authorization".into(),
            risk_class: "R0".into(),
            execution_mode: "TRUSTED_LOCAL_PROVIDER".into(),
            actor: "forged-model-worker".into(),
            rationale: "forged worker rationale".into(),
            authorized_paths: vec!["src".into()],
            max_attempts: 1,
            provider: ready.proposal.route.provider.clone(),
            model: ready.proposal.route.model.clone(),
            provider_mode: ready.proposal.route.mode.clone(),
            delegation_id: ready.grant.id.clone(),
            max_sessions: 1,
            max_turns: 4,
            max_input_tokens: 1_000,
            max_output_tokens: 1_000,
            max_cost_micros: 10_000,
            deadline_secs: 300,
            allowed_tools: vec!["write_file".into()],
        };
        let started = db
            .start_lead_campaign(
                &ready.request.id,
                "operator-model-start",
                operator_authority("device:operator", Some("Alice"), "reviewed provider plan"),
                Some(&expected),
                Some(&authorization),
            )
            .await
            .unwrap();
        let stored = db
            .campaign_authorization(&started.campaign.id)
            .await
            .unwrap();
        assert_eq!(stored.actor, "Alice");
        assert_ne!(stored.actor, "forged-model-worker");
        assert!(stored.rationale.contains("device:operator"));
        assert_eq!(stored.execution_mode, "TRUSTED_LOCAL_PROVIDER");
        let evidence = db
            .lead_start_evidence(&ready.request.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(evidence.principal_id, "device:operator");
        assert_eq!(evidence.rationale, "reviewed provider plan");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn freeze_and_ready_are_recoverable_without_double_reservation() {
        let (path, db, _) = fixture().await;
        let frozen = draft_frozen_lead_with_mechanism(&db, "recover", "FIXTURE").await;
        assert_eq!(frozen.request.state, LeadRequestState::Frozen);
        let usage_before = db.delegation_usage(&frozen.grant.id).await.unwrap();
        let (plan, request) = db
            .freeze_lead_plan_to_ready(&frozen.request.id, "draft-freeze-recover")
            .await
            .unwrap();
        assert_eq!(plan, frozen.plan);
        assert_eq!(request.state, LeadRequestState::Ready);
        assert_eq!(
            db.delegation_usage(&frozen.grant.id).await.unwrap(),
            usage_before
        );
        let (replayed_plan, replayed_request) = db
            .freeze_lead_plan_to_ready(&frozen.request.id, "draft-freeze-recover")
            .await
            .unwrap();
        assert_eq!(replayed_plan, plan);
        assert_eq!(replayed_request.state, LeadRequestState::Ready);
        assert_eq!(
            db.delegation_usage(&frozen.grant.id).await.unwrap(),
            usage_before
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn start_authority_types_fail_closed_and_require_exact_hashes() {
        assert!(matches!(
            ValidatedAutonomyContext::from_deployment_policy(false),
            Err(Error::Forbidden(_))
        ));
        assert!(ValidatedAutonomyContext::from_deployment_policy(true).is_ok());
        assert!(matches!(
            AuthenticatedOperatorApproval::new("", None, "rationale"),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            AuthenticatedOperatorApproval::new("device:operator", None, ""),
            Err(Error::Invalid(_))
        ));
        assert!(
            AuthenticatedOperatorApproval::new("device:operator", Some("Alice"), "rationale")
                .is_ok()
        );
        let hash = "a".repeat(64);
        assert!(ExpectedStartBinding::new(&hash, &hash, &hash, &hash, &hash, None, None).is_ok());
        assert!(
            ExpectedStartBinding::new(&hash, "short", &hash, &hash, &hash, None, None).is_err()
        );
        assert!(
            ExpectedStartBinding::new(&hash, &hash, &hash, &hash, &hash, Some("revision"), None)
                .is_err()
        );
    }

    #[tokio::test]
    async fn autonomous_start_and_withdrawal_race_is_fenced_across_databases() {
        let (path, db, _) = fixture().await;
        let second = Database::open(&path).await.unwrap();
        let ready = ready_lead(&db, "FIXTURE").await;
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let start_barrier = barrier.clone();
        let start_db = db.clone();
        let start_request = ready.request.id.clone();
        let start = tokio::spawn(async move {
            start_barrier.wait().await;
            start_db
                .start_lead_campaign(
                    &start_request,
                    "autonomous-race-start",
                    autonomous_authority(),
                    None,
                    None,
                )
                .await
        });
        let withdraw_barrier = barrier.clone();
        let withdraw_db = second.clone();
        let withdraw_request = ready.request.id.clone();
        let withdraw = tokio::spawn(async move {
            withdraw_barrier.wait().await;
            withdraw_db
                .withdraw_lead_request(
                    &withdraw_request,
                    "device:operator",
                    Some("Alice"),
                    "stop autonomous work",
                )
                .await
        });
        let start = start.await.unwrap();
        let withdraw = withdraw.await.unwrap();
        match (&start, &withdraw) {
            (Ok(started), Err(Error::AlreadyStarted { campaign_id, .. })) => {
                assert_eq!(campaign_id, &started.campaign.id);
            }
            (Err(Error::Withdrawn), Ok(_)) => {}
            other => panic!("autonomous start/withdraw race was inconsistent: {other:?}"),
        }
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM campaigns").await,
            if start.is_ok() { 1 } else { 0 }
        );
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM lead_withdrawals").await,
            if withdraw.is_ok() { 1 } else { 0 }
        );
        second.close().await;
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn withdrawn_assessing_request_cannot_authorize_or_send_a_paid_lead_call() {
        let (path, db, _) = fixture().await;
        let action_id = action(&db).await;
        let config = db.project_config("project").await.unwrap();
        let (request, _) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config.config_hash,
                "withdrawn-assessing-request",
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        db.withdraw_lead_request(
            &request.id,
            "device:operator",
            Some("Alice"),
            "objective withdrawn during assessment",
        )
        .await
        .unwrap();
        assert_eq!(
            db.lead_request(&request.id).await.unwrap().state,
            LeadRequestState::Assessing
        );

        let grant = db.standing_delegation("grant").await.unwrap();
        let input = lead_model_call_authorization(&request.id, &grant);
        let error = db.authorize_model_call(&input).await.unwrap_err();
        assert!(matches!(error, Error::Withdrawn), "{error:?}");
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM model_call_reservations").await,
            0
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let server = tokio::spawn(async move {
            if listener.accept().await.is_ok() {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        });
        let policy = input.policy;
        let gateway =
            crate::model_gateway::ModelGateway::new(crate::model_gateway::ModelGatewayConfig {
                base_url: format!("http://{addr}"),
                credential: "test-credential-1234567890".into(),
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                timeout: std::time::Duration::from_millis(1_000),
                input_micros_per_million: 100,
                output_micros_per_million: 100,
                lead_policy: policy,
                reviewer_policy: policy,
                evaluation_policy: policy,
                global_policy: policy,
                allow_loopback_http: true,
                bound_role: None,
                test_upstream_hosts: Vec::new(),
            })
            .unwrap();
        let action = db.action(&action_id).await.unwrap();
        let binding = ActionBinding {
            id: action.id.clone(),
            project_id: "project".into(),
            status: crate::lead::ActionStatus::Pending,
        };
        let assessed = crate::lead_model::assess(
            &gateway,
            &db,
            crate::lead_model::AssessmentRequest {
                request_id: &request.id,
                project: &config,
                action: &binding,
                action_spec: &action.spec,
                objective: &request.objective,
                grant: &grant,
                now: Utc::now(),
            },
        )
        .await;
        assert!(assessed.is_err(), "withdrawn assessment must not succeed");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "withdrawn assessment made an upstream request"
        );
        assert_eq!(
            count_rows(&db, "SELECT COUNT(*) FROM model_call_reservations").await,
            0
        );
        server.abort();
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn lead_model_authorization_and_withdrawal_serialize_across_databases() {
        let (path, db, _) = fixture().await;
        let second = Database::open(&path).await.unwrap();
        let action_id = action(&db).await;
        let config = db.project_config("project").await.unwrap();
        let (request, _) = db
            .create_lead_request(
                "project",
                &action_id,
                "grant",
                "Implement the feature.",
                &config.config_hash,
                "racing-assessing-request",
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let grant = db.standing_delegation("grant").await.unwrap();
        let input = lead_model_call_authorization(&request.id, &grant);
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let auth_barrier = barrier.clone();
        let auth_db = db.clone();
        let auth_input = input.clone();
        let authorize = tokio::spawn(async move {
            auth_barrier.wait().await;
            auth_db.authorize_model_call(&auth_input).await
        });
        let withdraw_barrier = barrier.clone();
        let withdraw_db = second.clone();
        let withdraw_request = request.id.clone();
        let withdraw = tokio::spawn(async move {
            withdraw_barrier.wait().await;
            withdraw_db
                .withdraw_lead_request(
                    &withdraw_request,
                    "device:operator",
                    Some("Alice"),
                    "withdraw during authorization race",
                )
                .await
        });
        let authorize = authorize.await.unwrap();
        let withdraw = withdraw.await.unwrap();
        assert!(
            withdraw.is_ok(),
            "withdrawal after authorization is valid: {withdraw:?}"
        );
        let reservations = count_rows(&db, "SELECT COUNT(*) FROM model_call_reservations").await;
        match authorize {
            Ok(ModelCallDecision::Reserved(_)) => {
                assert_eq!(
                    reservations, 1,
                    "authorization that committed first owns a reservation"
                );
            }
            Err(Error::Withdrawn) => {
                assert_eq!(
                    reservations, 0,
                    "a committed withdrawal must create no reservation"
                );
            }
            other => panic!("unexpected serialized authorization result: {other:?}"),
        }
        second.close().await;
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn generic_run_attempt_routes_and_unlinked_worker_attempts_are_rejected_for_lead_campaigns()
     {
        let (path, db, _) = fixture().await;
        let ready = draft_ready_lead(&db, "generic-paths").await;
        let expected = expected_binding(&ready);
        let started = db
            .start_lead_campaign(
                &ready.request.id,
                "generic-paths-start",
                operator_authority("device:operator", Some("Alice"), "approved WorkGraph"),
                Some(&expected),
                None,
            )
            .await
            .unwrap();
        let (runs_before,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM runs WHERE campaign_id=?1")
                .bind(&started.campaign.id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        let (attempts_before,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM attempts WHERE run_id IN
             (SELECT id FROM runs WHERE campaign_id=?1)",
        )
        .bind(&started.campaign.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert!(matches!(
            db.create_run(&started.campaign.id, "generic-run-key").await,
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.create_attempt(&started.run.id, "generic-attempt-key")
                .await,
            Err(Error::Forbidden(_))
        ));
        let (runs_after,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM runs WHERE campaign_id=?1")
                .bind(&started.campaign.id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        let (attempts_after,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM attempts WHERE run_id IN
             (SELECT id FROM runs WHERE campaign_id=?1)",
        )
        .bind(&started.campaign.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(runs_after, runs_before);
        assert_eq!(attempts_after, attempts_before);
        assert_eq!(runs_before, 1);
        assert_eq!(attempts_before, 1);

        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE attempts SET status='COMPLETED' WHERE run_id IN (
            SELECT id FROM runs WHERE campaign_id=?1)",
        )
        .bind(&started.campaign.id)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO runs(id,campaign_id,status,created_at)
             VALUES ('forged-generic-run',?1,'QUEUED',?2)",
        )
        .bind(&started.campaign.id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at)
             VALUES ('forged-generic-attempt','forged-generic-run','QUEUED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        let (root_spec_sha,): (String,) =
            sqlx::query_as::query_as("SELECT spec_sha256 FROM action_specs WHERE action_id=?1")
                .bind(&ready.request.action_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        sqlx::query::query(
            "INSERT INTO attempt_specs(attempt_id,action_id,base_commit,spec_json,spec_sha256)
             SELECT 'forged-generic-attempt',s.action_id,p.base_commit,s.spec_json,s.spec_sha256
             FROM action_specs s
             JOIN actions a ON a.id=s.action_id
             JOIN projects p ON p.id=a.project_id
             WHERE s.action_id=?1",
        )
        .bind(&ready.request.action_id)
        .execute(&db.pool)
        .await
        .unwrap();
        let now_ms = Utc::now().timestamp_millis().saturating_add(1_000);
        let error = db
            .claim_assignment("forged-worker", now_ms, 5_000)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
        assert_eq!(
            count_rows(
                &db,
                "SELECT COUNT(*) FROM worker_leases WHERE attempt_id='forged-generic-attempt'"
            )
            .await,
            0
        );
        assert_eq!(
            count_rows(
                &db,
                "SELECT COUNT(*) FROM attempts WHERE id='forged-generic-attempt' AND status='QUEUED'"
            )
            .await,
            1
        );
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
             VALUES ('forged-generic-candidate','forged-generic-attempt',?1,?2,?3,'worker',?4)",
        )
        .bind(&root_spec_sha)
        .bind("1".repeat(40))
        .bind("2".repeat(40))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        let error = db
            .candidate_inspection("forged-generic-candidate")
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
