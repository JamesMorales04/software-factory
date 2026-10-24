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
        let mut transaction = self.pool.begin().await?;
        let request = load_lead_request(&mut transaction, request_id).await?;
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
        if let Some(id) = replay(
            &mut transaction,
            idempotency_key,
            "record_lead_assessment",
            &digest,
        )
        .await?
        {
            let existing = load_assessment(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(existing);
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
        let mut transaction = self.pool.begin().await?;
        let request = load_lead_request(&mut transaction, request_id).await?;
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
            transaction.commit().await?;
            return Ok(existing);
        }
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

    pub async fn freeze_lead_plan(
        &self,
        request_id: &str,
        idempotency_key: &str,
    ) -> Result<FrozenLeadPlanRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let request = load_lead_request(&mut transaction, request_id).await?;
        if request.state != LeadRequestState::Planned {
            return Err(Error::Conflict("lead plan is not ready to freeze"));
        }
        let (proposal_id, proposal_json): (String, String) = sqlx::query_as::query_as(
            "SELECT id,proposal_json FROM lead_proposals WHERE request_id=?1",
        )
        .bind(request_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::Conflict("lead plan requires a recorded proposal"))?;
        let grant = load_delegation(&mut transaction, &request.delegation_id).await?;
        let proposal: LeadPlanProposal = serde_json::from_str(&proposal_json)
            .map_err(|_| Error::Invalid("stored proposal encoding"))?;
        let config = load_project_config(&mut transaction, &request.project_id).await?;
        if config.config_hash != request.project_config_hash
            || proposal.base_revision != config.base_commit
        {
            return Err(Error::Forbidden(
                "project configuration or base changed after the Lead request".into(),
            ));
        }
        let action = load_action_binding(&mut transaction, &request.action_id).await?;
        proposal.validate(&grant, &config.binding(), &action, Utc::now())?;
        let extra_sessions = proposal.max_sessions.saturating_sub(1);
        if extra_sessions > 0 {
            reserve_usage_in_tx(&mut transaction, &grant, 0, extra_sessions, 0).await?;
        }
        let hash = plan_hash(&proposal, &grant, &request.project_config_hash);
        if let Some(id) =
            replay(&mut transaction, idempotency_key, "freeze_lead_plan", &hash).await?
        {
            let existing = load_frozen_plan_by_id(&mut transaction, &id).await?;
            transaction.commit().await?;
            return Ok(existing);
        }
        let existing: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM frozen_lead_plans WHERE request_id=?1")
                .bind(request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if existing.is_some() {
            return Err(Error::Conflict("lead plan is already frozen"));
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
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("freeze_lead_plan")
            .bind(&hash)
            .bind(&record.id)
            .execute(&mut *transaction)
            .await?;
        update_lead_state(&mut transaction, &request, LeadRequestState::Frozen).await?;
        record_event(
            &mut transaction,
            &request.id,
            "lead.plan_frozen",
            &record.plan_hash,
        )
        .await?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Start a frozen single-unit Lead plan under an explicit autonomous grant.
    /// This creates the Campaign authorization, Run and Attempt atomically. It
    /// does not enable provider or ISOLATED execution.
    pub async fn start_lead_campaign(
        &self,
        request_id: &str,
        idempotency_key: &str,
    ) -> Result<(Campaign, Run, Attempt), Error> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(Error::Invalid("lead start idempotency key"));
        }
        let digest = sha256_hex(
            serde_json::to_string(&(request_id, "start-lead-campaign-v1"))
                .expect("lead start digest serializes")
                .as_bytes(),
        );
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(existing) = replay(
            &mut transaction,
            idempotency_key,
            "start_lead_campaign",
            &digest,
        )
        .await?
        {
            let campaign = load_campaign(&mut transaction, &existing).await?;
            let run = load_run_for_campaign(&mut transaction, &campaign.id).await?;
            let attempt = load_attempt_for_run(&mut transaction, &run.id).await?;
            transaction.commit().await?;
            return Ok((campaign, run, attempt));
        }
        let request = load_lead_request(&mut transaction, request_id).await?;
        if request.state != LeadRequestState::Ready {
            return Err(Error::Conflict("lead plan is not ready to start"));
        }
        if request.campaign_id.is_some() {
            return Err(Error::Conflict("lead request already started"));
        }
        let grant = load_delegation(&mut transaction, &request.delegation_id).await?;
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
        let (_proposal_id, proposal_json): (String, String) = sqlx::query_as::query_as(
            "SELECT p.id,lp.proposal_json FROM frozen_lead_plans p
             JOIN lead_proposals lp ON lp.id=p.proposal_id
             WHERE p.request_id=?1",
        )
        .bind(request_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::Conflict("lead plan is not frozen"))?;
        let plan_id: (String,) =
            sqlx::query_as::query_as("SELECT id FROM frozen_lead_plans WHERE request_id=?1")
                .bind(request_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(Error::Conflict("lead plan is not frozen"))?;
        let plan = load_frozen_plan_by_id(&mut transaction, &plan_id.0).await?;
        let proposal: LeadPlanProposal = serde_json::from_str(&proposal_json)
            .map_err(|_| Error::Invalid("stored proposal encoding"))?;
        if proposal.work_units.is_empty() {
            return Err(Error::Forbidden("WorkGraph is empty".into()));
        }
        if proposal.risk_class != crate::lead::RiskClass::R0 {
            return Err(Error::Forbidden("autonomous start is limited to R0".into()));
        }
        let action = load_action_binding(&mut transaction, &request.action_id).await?;
        if action.status != crate::lead::ActionStatus::Pending {
            return Err(Error::Forbidden("action is not startable".into()));
        }
        let risk: (String,) = sqlx::query_as::query_as("SELECT risk FROM actions WHERE id=?1")
            .bind(&request.action_id)
            .fetch_one(&mut *transaction)
            .await?;
        if risk.0 != "R0" {
            return Err(Error::Forbidden(
                "action must be explicitly classified R0 before autonomous start".into(),
            ));
        }
        let config = load_project_config(&mut transaction, &request.project_id).await?;
        proposal.validate(&grant, &config.binding(), &action, Utc::now())?;
        let mut paths = std::collections::BTreeSet::new();
        let mut specs = Vec::with_capacity(proposal.work_units.len());
        for unit in &proposal.work_units {
            paths.extend(unit.allowed_paths.iter().cloned());
            specs.push(unit_action_spec(
                &proposal,
                &config,
                &grant,
                unit,
                &config.base_commit,
            )?);
        }
        let action_spec: String = sqlx::query_as::query_as::<_, (String,)>(
            "SELECT spec_json FROM action_specs WHERE action_id=?1",
        )
        .bind(&request.action_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::Conflict("action has no frozen spec"))?
        .0;
        let action_spec: ActionSpec = serde_json::from_str(&action_spec)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        let mut expected = proposal.acceptance_criteria.clone();
        expected.extend(proposal.negative_cases.clone());
        expected.sort_by(|a, b| a.id.cmp(&b.id));
        let mut actual = action_spec.checks.clone();
        actual.sort_by(|a, b| a.id.cmp(&b.id));
        if expected != actual {
            return Err(Error::Forbidden(
                "frozen plan does not match the ActionSpec checks".into(),
            ));
        }
        for unit in &proposal.work_units {
            if unit
                .allowed_paths
                .iter()
                .any(|path| !crate::workspace::path_covered(path, &action_spec.allowed_paths))
            {
                return Err(Error::Forbidden(
                    "a WorkUnit path widens beyond the frozen ActionSpec".into(),
                ));
            }
        }
        let now = Utc::now().to_rfc3339();
        let campaign = Campaign {
            id: Uuid::new_v4().to_string(),
            action_id: request.action_id.clone(),
            status: "AUTHORIZED".into(),
            created_at: now.clone(),
        };
        sqlx::query::query("INSERT INTO campaigns VALUES (?1,?2,?3,?4)")
            .bind(&campaign.id)
            .bind(&campaign.action_id)
            .bind(&campaign.status)
            .bind(&campaign.created_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,authorized_paths_json,max_attempts,created_at)
             VALUES (?1,?2,'R0','FIXTURE',?3,?4,?5,?6,?7)",
        )
        .bind(&campaign.id)
        .bind(format!("lead-{}", request.id))
        .bind(format!("delegation:{}", grant.id))
        .bind(format!("autonomous R0 start for plan {}", plan.plan_hash))
        .bind(serde_json::to_string(&paths.into_iter().collect::<Vec<_>>()).map_err(|_| Error::Invalid("allowed paths encoding"))?)
        .bind(i64::from(proposal.budget.max_attempts))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let mut first_run = None;
        let mut first_attempt = None;
        for (index, (unit, spec)) in proposal.work_units.iter().zip(specs.iter()).enumerate() {
            let spec_json = serde_json::to_string(spec)
                .map_err(|_| Error::Invalid("unit ActionSpec encoding"))?;
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
                    .execute(&mut *transaction)
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
                .execute(&mut *transaction)
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
                .execute(&mut *transaction)
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
            .execute(&mut *transaction)
            .await?;
        }
        let run = first_run.ok_or(Error::Invalid("lead plan has no runnable unit"))?;
        let attempt = first_attempt.ok_or(Error::Invalid("lead plan has no runnable attempt"))?;
        sqlx::query::query(
            "UPDATE lead_requests SET state='RUNNING',campaign_id=?1,updated_at=?2 WHERE id=?3",
        )
        .bind(&campaign.id)
        .bind(&now)
        .bind(&request.id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("INSERT INTO idempotency VALUES (?1,?2,?3,?4)")
            .bind(idempotency_key)
            .bind("start_lead_campaign")
            .bind(&digest)
            .bind(&campaign.id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            &campaign.id,
            "campaign.started",
            "autonomous R0 fixture start",
        )
        .await?;
        record_event(&mut transaction, &request.id, "lead.started", &campaign.id).await?;
        transaction.commit().await?;
        Ok((campaign, run, attempt))
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
        let mut transaction = self.pool.begin().await?;
        let current = load_lead_request(&mut transaction, id).await?;
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

async fn load_lead_request(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<LeadRequestRecord, Error> {
    let row = sqlx::query::query(
        "SELECT id,project_id,action_id,delegation_id,objective,request_hash,
                project_config_hash,state,failure_reason,campaign_id,created_at,updated_at
         FROM lead_requests WHERE id=?1",
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
        failure_reason: row.try_get("failure_reason")?,
        campaign_id: row.try_get("campaign_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
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

async fn load_frozen_plan_by_id(
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

async fn load_action_binding(
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
        let (campaign, run, attempt) = db
            .start_lead_campaign(&request.id, "fixture-start-key")
            .await
            .unwrap();
        assert_eq!(campaign.status, "AUTHORIZED");
        assert_eq!(run.status, "QUEUED");
        assert_eq!(attempt.status, "QUEUED");
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
        let (_campaign, _run, attempt) = db
            .start_lead_campaign(&request.id, "chain-start-key")
            .await
            .unwrap();
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
        db.start_lead_campaign(&request.id, "fanin-start")
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
            "SELECT COUNT(*) FROM worker_leases WHERE attempt_id='attempt-cancel'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(leases, 0);

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
}
