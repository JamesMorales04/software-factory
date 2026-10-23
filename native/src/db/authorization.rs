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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewCampaignAuthorization {
    pub authorization_id: String,
    pub risk_class: String,
    pub execution_mode: String,
    pub actor: String,
    pub rationale: String,
    pub authorized_paths: Vec<String>,
    pub max_attempts: u32,
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
                "only R0 fixture execution is enabled; R1-R3 require protected approval".into(),
            ));
        }
        if input.execution_mode != "FIXTURE" {
            return Err(Error::Forbidden(
                "provider and isolated execution modes are not enabled".into(),
            ));
        }
        validate_repository_paths("authorizedPaths", &input.authorized_paths)?;
        let paths_json = serde_json::to_string(&input.authorized_paths)
            .map_err(|_| Error::Invalid("authorized paths encoding"))?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let campaign: Option<(String, String)> =
            sqlx::query_as::query_as("SELECT status,action_id FROM campaigns WHERE id=?1")
                .bind(campaign_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let (status, action_id) = campaign.ok_or(Error::NotFound)?;
        if let Some(existing) = load_authorization(&mut transaction, campaign_id).await? {
            if existing.authorization_id == input.authorization_id
                && existing.risk_class == input.risk_class
                && existing.execution_mode == input.execution_mode
                && existing.authorized_paths == input.authorized_paths
                && existing.max_attempts == input.max_attempts
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
            created_at: now,
        };
        sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,authorized_paths_json,max_attempts,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        )
        .bind(&record.campaign_id)
        .bind(&record.authorization_id)
        .bind(&record.risk_class)
        .bind(&record.execution_mode)
        .bind(&record.actor)
        .bind(&record.rationale)
        .bind(paths_json)
        .bind(record.max_attempts)
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
                authorized_paths_json,max_attempts,created_at
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
        created_at: row.try_get("created_at")?,
    }))
}
