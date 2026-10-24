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
    ActionBinding, AssessmentInput, LeadAssessment, LeadPlanProposal, LeadRequestState, plan_hash,
};

use super::delegations::{load_delegation, release_usage_in_tx, reserve_usage_in_tx};
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
            created_at: now.clone(),
            updated_at: now,
        };
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,state,failure_reason,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,?9)",
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
                project_config_hash,state,failure_reason,created_at,updated_at
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
        "HUMAN_REQUIRED" | "PENDING" | "BLOCKED_AUTHORIZATION" => {
            crate::lead::ActionStatus::Pending
        }
        "IN_PROGRESS" | "AUTHORIZED" | "RUNNING" => crate::lead::ActionStatus::InProgress,
        "COMPLETED" | "FAILED" | "CANCELLED" => crate::lead::ActionStatus::Terminal,
        _ => return Err(Error::Invalid("stored action status")),
    };
    Ok(ActionBinding {
        id: action_id.into(),
        project_id,
        status,
    })
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
        sqlx::query::query("INSERT INTO projects VALUES (?1,?2,?3,?4)")
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
        let grant = grant();
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
            Err(Error::Forbidden(_))
        ));
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
