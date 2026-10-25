//! Explicit Campaign execution authorization.
//!
//! Only deterministic fixture execution is enabled in this slice. Provider and
//! isolated modes fail closed until their credential, sandbox and budget gates
//! exist. Authorization never moves risk, delivery or quality policy to a
//! model, Worker or container label.

use super::*;

use crate::workspace::path_covered;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;

use crate::lead::validate_repository_paths;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewCampaignAuthorization {
    pub authorization_id: String,
    pub risk_class: String,
    pub execution_mode: String,
    pub actor: String,
    pub rationale: String,
    pub authorized_paths: Vec<String>,
    pub max_attempts: u32,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider_mode: String,
    #[serde(default)]
    pub delegation_id: String,
    #[serde(default)]
    pub max_sessions: u32,
    #[serde(default)]
    pub max_turns: u32,
    #[serde(default)]
    pub max_input_tokens: u64,
    #[serde(default)]
    pub max_output_tokens: u64,
    #[serde(default)]
    pub max_cost_micros: u64,
    #[serde(default)]
    pub deadline_secs: u64,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignAuthorizationRecord {
    pub campaign_id: String,
    pub authorization_id: String,
    pub risk_class: String,
    pub execution_mode: String,
    pub actor: String,
    pub rationale: String,
    pub authorized_paths: Vec<String>,
    pub max_attempts: u32,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
    pub delegation_id: String,
    pub max_sessions: u32,
    pub max_turns: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
    pub deadline_secs: u64,
    pub allowed_tools: Vec<String>,
    pub created_at: String,
}

impl Database {
    pub async fn authorize_campaign(
        &self,
        campaign_id: &str,
        input: &NewCampaignAuthorization,
    ) -> Result<(CampaignAuthorizationRecord, bool), Error> {
        if input.authorization_id.trim().is_empty()
            || input.authorization_id.len() > 128
            || input.actor.trim().is_empty()
            || input.actor.len() > 128
            || input.rationale.trim().is_empty()
            || input.rationale.len() > 2_000
            || !(1..=5).contains(&input.max_attempts)
        {
            return Err(Error::Invalid("campaign authorization input"));
        }
        if input.risk_class != "R0" {
            return Err(Error::Forbidden(
                "only R0 execution is enabled; R1-R3 require protected approval".into(),
            ));
        }
        let fixture = input.execution_mode == "FIXTURE";
        let provider = input.execution_mode == "TRUSTED_LOCAL_PROVIDER";
        if !fixture && !provider {
            return Err(Error::Forbidden(
                "only FIXTURE and TRUSTED_LOCAL_PROVIDER execution are enabled; ISOLATED is blocked"
                    .into(),
            ));
        }
        validate_repository_paths("authorizedPaths", &input.authorized_paths)?;
        let paths_json = serde_json::to_string(&input.authorized_paths)
            .map_err(|_| Error::Invalid("authorized paths encoding"))?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let campaign: Option<(String, String, String)> = sqlx::query_as::query_as(
            "SELECT c.status,c.action_id,a.project_id
             FROM campaigns c JOIN actions a ON a.id=c.action_id WHERE c.id=?1",
        )
        .bind(campaign_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (status, action_id, project_id) = campaign.ok_or(Error::NotFound)?;
        if let Some(existing) = load_authorization(&mut transaction, campaign_id).await? {
            if existing.authorization_id == input.authorization_id
                && existing.risk_class == input.risk_class
                && existing.execution_mode == input.execution_mode
                && existing.actor == input.actor
                && existing.rationale == input.rationale
                && existing.authorized_paths == input.authorized_paths
                && existing.max_attempts == input.max_attempts
                && existing.provider == input.provider
                && existing.model == input.model
                && existing.provider_mode == input.provider_mode
                && existing.delegation_id == input.delegation_id
                && existing.max_sessions == input.max_sessions
                && existing.max_turns == input.max_turns
                && existing.max_input_tokens == input.max_input_tokens
                && existing.max_output_tokens == input.max_output_tokens
                && existing.max_cost_micros == input.max_cost_micros
                && existing.deadline_secs == input.deadline_secs
                && existing.allowed_tools == input.allowed_tools
            {
                transaction.commit().await?;
                return Ok((existing, false));
            }
            return Err(Error::Conflict(
                "campaign already has a different authorization",
            ));
        }
        if status != "BLOCKED_AUTHORIZATION" {
            return Err(Error::Conflict("campaign is not awaiting authorization"));
        }
        let spec: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT action_id,spec_json FROM action_specs WHERE action_id=?1",
        )
        .bind(&action_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let (_, spec_json) = spec.ok_or(Error::Conflict("campaign action has no frozen spec"))?;
        let action_spec: ActionSpec = serde_json::from_str(&spec_json)
            .map_err(|_| Error::Invalid("stored ActionSpec encoding"))?;
        if action_spec.allowed_paths.is_empty() {
            return Err(Error::Forbidden(
                "campaign action has no frozen allowed-path scope".into(),
            ));
        }
        if let Some(path) = action_spec
            .allowed_paths
            .iter()
            .find(|path| !path_covered(path, &input.authorized_paths))
        {
            return Err(Error::Forbidden(format!(
                "action path {path:?} is outside the campaign authorized paths"
            )));
        }
        if provider {
            if !input.allowed_tools.is_empty() {
                return Err(Error::Forbidden(
                    "provider tool mediation is not implemented; allowedTools must be empty".into(),
                ));
            }
            if input.provider.is_empty()
                || input.model.is_empty()
                || input.provider_mode.is_empty()
                || input.delegation_id.is_empty()
                || input.provider != action_spec.provider
                || input.model != action_spec.model
                || input.max_sessions == 0
                || input.max_sessions > 2
                || input.max_turns != 1
                || input.max_input_tokens == 0
                || input.max_output_tokens == 0
                || input.max_cost_micros == 0
                || !(30..=600).contains(&input.deadline_secs)
            {
                return Err(Error::Invalid("provider authorization bounds"));
            }
            let grant =
                super::delegations::load_delegation(&mut transaction, &input.delegation_id).await?;
            grant.validate(Utc::now())?;
            if grant.project_id != project_id {
                return Err(Error::Forbidden(
                    "delegation project does not match the Action project".into(),
                ));
            }
            if grant.route.provider != input.provider
                || grant.route.model != input.model
                || grant.route.mode != input.provider_mode
            {
                return Err(Error::Forbidden(
                    "provider route does not match the standing delegation".into(),
                ));
            }
            if !grant.risk_classes.contains(&crate::lead::RiskClass::R0) {
                return Err(Error::Forbidden(
                    "standing delegation does not authorize R0".into(),
                ));
            }
            if action_spec
                .allowed_paths
                .iter()
                .any(|path| !path_covered(path, &grant.allowed_paths))
            {
                return Err(Error::Forbidden(
                    "ActionSpec paths exceed the standing delegation".into(),
                ));
            }
            if input.max_sessions > grant.max_sessions
                || input.max_attempts > grant.max_attempts
                || input.deadline_secs > u64::from(grant.max_timeout_secs)
            {
                return Err(Error::Forbidden(
                    "provider budget exceeds the standing delegation".into(),
                ));
            }
            super::delegations::reserve_usage_in_tx(
                &mut transaction,
                &grant,
                1,
                input.max_sessions,
                input.max_attempts,
            )
            .await?;
        }
        let now = Utc::now().to_rfc3339();
        let record = CampaignAuthorizationRecord {
            campaign_id: campaign_id.into(),
            authorization_id: input.authorization_id.clone(),
            risk_class: input.risk_class.clone(),
            execution_mode: input.execution_mode.clone(),
            actor: input.actor.clone(),
            rationale: input.rationale.clone(),
            authorized_paths: input.authorized_paths.clone(),
            max_attempts: input.max_attempts,
            provider: input.provider.clone(),
            model: input.model.clone(),
            provider_mode: input.provider_mode.clone(),
            delegation_id: input.delegation_id.clone(),
            max_sessions: input.max_sessions,
            max_turns: input.max_turns,
            max_input_tokens: input.max_input_tokens,
            max_output_tokens: input.max_output_tokens,
            max_cost_micros: input.max_cost_micros,
            deadline_secs: input.deadline_secs,
            allowed_tools: input.allowed_tools.clone(),
            created_at: now,
        };
        sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
              authorized_paths_json,max_attempts,provider,model,provider_mode,delegation_id,
              max_sessions,max_turns,max_input_tokens,max_output_tokens,max_cost_micros,
              deadline_secs,allowed_tools_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        )
        .bind(&record.campaign_id)
        .bind(&record.authorization_id)
        .bind(&record.risk_class)
        .bind(&record.execution_mode)
        .bind(&record.actor)
        .bind(&record.rationale)
        .bind(paths_json)
        .bind(record.max_attempts)
        .bind(&record.provider)
        .bind(&record.model)
        .bind(&record.provider_mode)
        .bind(&record.delegation_id)
        .bind(record.max_sessions)
        .bind(record.max_turns)
        .bind(
            i64::try_from(record.max_input_tokens)
                .map_err(|_| Error::Invalid("provider input budget"))?,
        )
        .bind(
            i64::try_from(record.max_output_tokens)
                .map_err(|_| Error::Invalid("provider output budget"))?,
        )
        .bind(
            i64::try_from(record.max_cost_micros)
                .map_err(|_| Error::Invalid("provider cost budget"))?,
        )
        .bind(i64::try_from(record.deadline_secs).map_err(|_| Error::Invalid("provider deadline"))?)
        .bind(
            serde_json::to_string(&record.allowed_tools)
                .map_err(|_| Error::Invalid("allowed tools encoding"))?,
        )
        .bind(&record.created_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query::query("UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1")
            .bind(campaign_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query::query("UPDATE actions SET risk='R0' WHERE id=?1")
            .bind(&action_id)
            .execute(&mut *transaction)
            .await?;
        record_event(
            &mut transaction,
            campaign_id,
            "campaign.authorized",
            &format!(
                "authorization={} risk=R0 mode={} attempts={}",
                record.authorization_id, record.execution_mode, record.max_attempts
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok((record, true))
    }

    pub async fn campaign_authorization(
        &self,
        campaign_id: &str,
    ) -> Result<CampaignAuthorizationRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_authorization(&mut connection, campaign_id)
            .await?
            .ok_or(Error::NotFound)
    }
}

pub(super) async fn load_authorization(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Option<CampaignAuthorizationRecord>, Error> {
    use sqlx::row::Row;

    let row = sqlx::query::query(
        "SELECT campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
                authorized_paths_json,max_attempts,provider,model,provider_mode,delegation_id,
                max_sessions,max_turns,max_input_tokens,max_output_tokens,max_cost_micros,
                deadline_secs,allowed_tools_json,created_at
         FROM campaign_authorizations WHERE campaign_id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let paths: String = row.try_get("authorized_paths_json")?;
    let max_attempts: i64 = row.try_get("max_attempts")?;
    let max_sessions: i64 = row.try_get("max_sessions")?;
    let max_turns: i64 = row.try_get("max_turns")?;
    let max_input_tokens: i64 = row.try_get("max_input_tokens")?;
    let max_output_tokens: i64 = row.try_get("max_output_tokens")?;
    let max_cost_micros: i64 = row.try_get("max_cost_micros")?;
    let deadline_secs: i64 = row.try_get("deadline_secs")?;
    let allowed_tools: String = row.try_get("allowed_tools_json")?;
    Ok(Some(CampaignAuthorizationRecord {
        campaign_id: row.try_get("campaign_id")?,
        authorization_id: row.try_get("authorization_id")?,
        risk_class: row.try_get("risk_class")?,
        execution_mode: row.try_get("execution_mode")?,
        actor: row.try_get("actor")?,
        rationale: row.try_get("rationale")?,
        authorized_paths: serde_json::from_str(&paths)
            .map_err(|_| Error::Invalid("stored authorized paths encoding"))?,
        max_attempts: u32::try_from(max_attempts)
            .map_err(|_| Error::Invalid("stored max attempts"))?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        provider_mode: row.try_get("provider_mode")?,
        delegation_id: row.try_get("delegation_id")?,
        max_sessions: u32::try_from(max_sessions)
            .map_err(|_| Error::Invalid("stored session budget"))?,
        max_turns: u32::try_from(max_turns).map_err(|_| Error::Invalid("stored turn budget"))?,
        max_input_tokens: u64::try_from(max_input_tokens)
            .map_err(|_| Error::Invalid("stored input budget"))?,
        max_output_tokens: u64::try_from(max_output_tokens)
            .map_err(|_| Error::Invalid("stored output budget"))?,
        max_cost_micros: u64::try_from(max_cost_micros)
            .map_err(|_| Error::Invalid("stored cost budget"))?,
        deadline_secs: u64::try_from(deadline_secs)
            .map_err(|_| Error::Invalid("stored provider deadline"))?,
        allowed_tools: serde_json::from_str(&allowed_tools)
            .map_err(|_| Error::Invalid("stored allowed tools encoding"))?,
        created_at: row.try_get("created_at")?,
    }))
}
