//! Explicit Campaign execution authorization.
//!
//! Fixture and trusted-local provider execution are explicit modes. A V2
//! authorization freezes an [`ExecutionPolicyV2`]: exact scope, risk, tools,
//! route, delegation identity, monetary envelope reference, technical operation
//! limits and deterministic progress supervision. It deliberately freezes no
//! attempts, sessions, turns, repair cycles, cumulative tokens, per-role money
//! or universal Campaign deadline.
//!
//! Legacy V1 rows remain immutable historical evidence. They are decoded for
//! display only and are never reinterpreted as V2; continuing a V1 Campaign
//! requires a new linked V2 request.
//!
//! Authorization never moves risk, delivery or quality policy to a model,
//! Worker or container label.

use super::*;

use crate::execution_policy::{
    ExecutionPolicyV2, ExecutionPolicyV3, FrozenExecutionPolicy, FundingPolicy, OperationLimits,
    ProgressPolicy,
};
use crate::workspace::path_covered;

use chrono::Utc;
use sqlx_sqlite::SqliteConnection;

use crate::lead::validate_repository_paths;

/// Semantic reference used by FIXTURE execution, which performs no billable
/// provider inference and therefore has no ledger envelope or balance. It is a
/// URN, never a numeric zero/huge sentinel.
pub const NO_BILLABLE_ENVELOPE: &str = "urn:factory:no-billable-inference";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SubscriptionFundingRecord {
    pub campaign_id: String,
    pub authorization_id: String,
    pub policy: ExecutionPolicyV3,
    pub state: crate::execution_policy::FundingRecoveryState,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewCampaignAuthorization {
    pub authorization_id: String,
    pub risk_class: String,
    pub execution_mode: String,
    pub actor: String,
    pub rationale: String,
    pub authorized_paths: Vec<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider_mode: String,
    #[serde(default)]
    pub delegation_id: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    pub monetary_envelope_id: String,
    pub progress: ProgressPolicy,
    pub operations: OperationLimits,
}

/// Legacy V1 cap evidence. Present only on historical rows and never used for
/// admissions, continuation or spend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LegacyCampaignAuthorizationCaps {
    pub max_attempts: u32,
    pub max_sessions: u32,
    pub max_turns: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
    pub deadline_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CampaignAuthorizationRecord {
    pub campaign_id: String,
    pub authorization_id: String,
    pub risk_class: String,
    pub execution_mode: String,
    pub actor: String,
    pub rationale: String,
    pub authorized_paths: Vec<String>,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
    pub delegation_id: String,
    pub allowed_tools: Vec<String>,
    pub schema_version: u32,
    /// Present exactly for V2 rows.
    pub policy: Option<ExecutionPolicyV2>,
    /// Present exactly for V1 historical rows.
    pub legacy: Option<LegacyCampaignAuthorizationCaps>,
    pub created_at: String,
}

impl CampaignAuthorizationRecord {
    pub fn is_legacy(&self) -> bool {
        self.schema_version == 1
    }

    pub fn v2_policy(&self) -> Result<&ExecutionPolicyV2, Error> {
        self.policy.as_ref().ok_or(Error::Forbidden(
            "legacy V1 authorization cannot authorize V2 execution".into(),
        ))
    }
}

/// Which delegation units an authorization records for diagnosis. A V2 grant
/// has no cumulative ceiling; the counters are telemetry only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthorizationReservationPolicy {
    Campaign,
    LeadStart,
}

impl Database {
    /// Persist a subscription authorization proposal without enabling model
    /// execution. Runtime admission is deliberately a later milestone.
    pub async fn record_subscription_funding_policy(
        &self,
        campaign_id: &str,
        policy: &ExecutionPolicyV3,
        observed_at_ms: i64,
    ) -> Result<SubscriptionFundingRecord, Error> {
        if observed_at_ms <= 0
            || policy.authorization_id.trim().is_empty()
            || policy.policy_id.trim().is_empty()
            || policy.scope_sha256.len() != 64
            || !policy
                .scope_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("subscription funding policy"));
        }
        let FundingPolicy::Subscription {
            grant_id,
            binding_sha256,
            binding,
        } = &policy.funding
        else {
            return Err(Error::Forbidden(
                "subscription policy persistence accepts only subscription funding".into(),
            ));
        };
        if grant_id.trim().is_empty()
            || binding_sha256.len() != 64
            || !binding_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || binding.provider_account_id.trim().is_empty()
            || binding.plan_id.trim().is_empty()
            || binding.model_id.trim().is_empty()
            || binding.settings_revision.trim().is_empty()
        {
            return Err(Error::Invalid("subscription funding provenance"));
        }
        let binding_json = serde_json::to_string(binding)
            .map_err(|_| Error::Invalid("subscription binding encoding"))?;
        if sha256_hex(binding_json.as_bytes()) != *binding_sha256 {
            return Err(Error::Invalid("subscription binding digest mismatch"));
        }
        let policy_json = serde_json::to_string(policy)
            .map_err(|_| Error::Invalid("subscription policy encoding"))?;
        let policy_sha256 = sha256_hex(policy_json.as_bytes());
        let _write = self.write_lock.lock().await;
        let mut tx = self.pool.begin().await?;
        let campaign: Option<(String,)> =
            sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                .bind(campaign_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some((status,)) = campaign else {
            return Err(Error::NotFound);
        };
        if status != "BLOCKED_AUTHORIZATION" {
            return Err(Error::Conflict(
                "subscription policy requires an authorization-pending campaign",
            ));
        }
        let existing: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT authorization_id FROM campaign_authorizations WHERE campaign_id=?1",
        )
        .bind(campaign_id)
        .fetch_optional(&mut *tx)
        .await?;
        if existing.is_some() {
            return Err(Error::Conflict(
                "campaign already has an execution authorization",
            ));
        }
        sqlx::query::query(
            "INSERT INTO subscription_funding_policies
             (campaign_id,authorization_id,policy_json,policy_sha256,state,reason,
              observed_at_ms,resets_at_ms,created_at_ms,updated_at_ms)
             VALUES (?1,?2,?3,?4,'DISABLED',NULL,?5,NULL,?5,?5)",
        )
        .bind(campaign_id)
        .bind(&policy.authorization_id)
        .bind(&policy_json)
        .bind(&policy_sha256)
        .bind(observed_at_ms)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(SubscriptionFundingRecord {
            campaign_id: campaign_id.into(),
            authorization_id: policy.authorization_id.clone(),
            policy: policy.clone(),
            state: crate::execution_policy::FundingRecoveryState::Disabled {
                recorded_at_ms: observed_at_ms,
            },
            enabled: false,
        })
    }

    pub async fn subscription_funding_policy(
        &self,
        campaign_id: &str,
    ) -> Result<SubscriptionFundingRecord, Error> {
        use sqlx::row::Row;
        let row = sqlx::query::query(
            "SELECT authorization_id,policy_json,policy_sha256,state,reason,
                    observed_at_ms,resets_at_ms FROM subscription_funding_policies WHERE campaign_id=?1",
        ).bind(campaign_id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound)?;
        let policy_json: String = row.try_get("policy_json")?;
        let digest: String = row.try_get("policy_sha256")?;
        if sha256_hex(policy_json.as_bytes()) != digest {
            return Err(Error::Invalid("stored subscription policy digest mismatch"));
        }
        let policy: ExecutionPolicyV3 = serde_json::from_str(&policy_json)
            .map_err(|_| Error::Invalid("stored subscription policy is unsupported"))?;
        let FundingPolicy::Subscription {
            binding_sha256,
            binding,
            ..
        } = &policy.funding
        else {
            return Err(Error::Invalid("stored policy is not subscription funded"));
        };
        let binding_json = serde_json::to_string(binding.as_ref())
            .map_err(|_| Error::Invalid("stored subscription binding is invalid"))?;
        if sha256_hex(binding_json.as_bytes()) != *binding_sha256 {
            return Err(Error::Invalid(
                "stored subscription binding digest mismatch",
            ));
        }
        let authorization_id: String = row.try_get("authorization_id")?;
        if authorization_id != policy.authorization_id {
            return Err(Error::Invalid("funding authorization identity mismatch"));
        }
        let state_name: String = row.try_get("state")?;
        let reason: Option<String> = row.try_get("reason")?;
        let observed_at_ms: i64 = row.try_get("observed_at_ms")?;
        let resets_at_ms: Option<i64> = row.try_get("resets_at_ms")?;
        let state = match state_name.as_str() {
            "DISABLED" => crate::execution_policy::FundingRecoveryState::Disabled {
                recorded_at_ms: observed_at_ms,
            },
            "PAUSED_QUOTA" => crate::execution_policy::FundingRecoveryState::PausedQuota {
                observed_at_ms,
                resets_at_ms,
                reason: reason.ok_or(Error::Invalid("paused funding reason is missing"))?,
            },
            "RECOVERABLE" => crate::execution_policy::FundingRecoveryState::Recoverable {
                observed_at_ms,
                reason: reason.ok_or(Error::Invalid("recovery reason is missing"))?,
            },
            _ => return Err(Error::Invalid("unsupported funding recovery state")),
        };
        Ok(SubscriptionFundingRecord {
            campaign_id: campaign_id.into(),
            authorization_id,
            policy,
            state,
            enabled: false,
        })
    }

    pub async fn pause_subscription_funding_for_quota(
        &self,
        campaign_id: &str,
        authorization_id: &str,
        observed_at_ms: i64,
        resets_at_ms: Option<i64>,
        reason: &str,
    ) -> Result<(), Error> {
        if observed_at_ms <= 0
            || resets_at_ms.is_some_and(|reset| reset <= observed_at_ms)
            || reason.trim().is_empty()
            || reason.len() > 512
        {
            return Err(Error::Invalid("quota pause evidence"));
        }
        let _write = self.write_lock.lock().await;
        let changed = sqlx::query::query(
            "UPDATE subscription_funding_policies SET state='PAUSED_QUOTA',reason=?1,
             observed_at_ms=?2,resets_at_ms=?3,updated_at_ms=?2
             WHERE campaign_id=?4 AND authorization_id=?5 AND observed_at_ms < ?2
               AND state IN ('DISABLED','RECOVERABLE')",
        )
        .bind(reason)
        .bind(observed_at_ms)
        .bind(resets_at_ms)
        .bind(campaign_id)
        .bind(authorization_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict(
                "quota pause does not match an active funding record",
            ));
        }
        Ok(())
    }

    pub async fn authorize_campaign(
        &self,
        campaign_id: &str,
        input: &NewCampaignAuthorization,
    ) -> Result<(CampaignAuthorizationRecord, bool), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        if let Some(existing) = load_authorization(&mut transaction, campaign_id).await? {
            if existing.is_legacy() {
                return Err(Error::Conflict(
                    "campaign already has a legacy V1 authorization; create a new linked V2 request instead of reinterpreting it",
                ));
            }
            if authorization_matches(&existing, input)? {
                transaction.commit().await?;
                return Ok((existing, false));
            }
            return Err(Error::Conflict(
                "campaign already has a different authorization",
            ));
        }
        let record = authorize_campaign_in_tx(
            &mut transaction,
            campaign_id,
            input,
            AuthorizationReservationPolicy::Campaign,
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

fn authorization_matches(
    existing: &CampaignAuthorizationRecord,
    input: &NewCampaignAuthorization,
) -> Result<bool, Error> {
    let Some(policy) = &existing.policy else {
        return Ok(false);
    };
    Ok(existing.authorization_id == input.authorization_id
        && existing.risk_class == input.risk_class
        && existing.execution_mode == input.execution_mode
        && existing.actor == input.actor
        && existing.rationale == input.rationale
        && existing.authorized_paths == input.authorized_paths
        && existing.provider == input.provider
        && existing.model == input.model
        && existing.provider_mode == input.provider_mode
        && existing.delegation_id == input.delegation_id
        && existing.allowed_tools == input.allowed_tools
        && policy.progress == input.progress
        && policy.operations == input.operations
        && policy.monetary_envelope_id == input.monetary_envelope_id)
}

/// Validate and insert a Campaign authorization inside the caller's
/// transaction. This is shared by the operator authorization route and Lead
/// start so both enforce exactly the same scope, path, delegation, risk, route
/// and tool policy. Any failure rolls the surrounding transaction back.
pub(super) async fn authorize_campaign_in_tx(
    transaction: &mut SqliteConnection,
    campaign_id: &str,
    input: &NewCampaignAuthorization,
    reservation_policy: AuthorizationReservationPolicy,
) -> Result<CampaignAuthorizationRecord, Error> {
    if input.authorization_id.trim().is_empty()
        || input.authorization_id.len() > 128
        || input.actor.trim().is_empty()
        || input.actor.len() > 128
        || input.rationale.trim().is_empty()
        || input.rationale.len() > 2_000
        || input.monetary_envelope_id.trim().is_empty()
        || input.monetary_envelope_id.len() > 256
    {
        return Err(Error::Invalid("campaign authorization input"));
    }
    validate_operation_limits(&input.operations)?;
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
    let campaign: Option<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT c.status,c.action_id,a.project_id
         FROM campaigns c JOIN actions a ON a.id=c.action_id WHERE c.id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let (status, action_id, project_id) = campaign.ok_or(Error::NotFound)?;
    if status != "BLOCKED_AUTHORIZATION" {
        return Err(Error::Conflict("campaign is not awaiting authorization"));
    }
    let lead_request: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM lead_requests WHERE action_id=?1")
            .bind(&action_id)
            .fetch_optional(&mut *transaction)
            .await?;
    if let Some((request_id,)) = lead_request {
        let withdrawal: Option<(String,)> =
            sqlx::query_as::query_as("SELECT request_id FROM lead_withdrawals WHERE request_id=?1")
                .bind(&request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if withdrawal.is_some() {
            return Err(Error::Withdrawn);
        }
        if reservation_policy != AuthorizationReservationPolicy::LeadStart {
            return Err(Error::Forbidden(
                "Lead Campaign authorizations are recorded only by the shared Lead start boundary"
                    .into(),
            ));
        }
    }
    let duplicate: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT campaign_id FROM campaign_authorizations WHERE authorization_id=?1",
    )
    .bind(&input.authorization_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if duplicate.is_some() {
        return Err(Error::Conflict("authorization id is already in use"));
    }
    let spec: Option<(String, String)> =
        sqlx::query_as::query_as("SELECT action_id,spec_json FROM action_specs WHERE action_id=?1")
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
        let allowed = ["write_file", "run_command"];
        let mut seen = std::collections::BTreeSet::new();
        if input
            .allowed_tools
            .iter()
            .any(|tool| !allowed.contains(&tool.as_str()) || !seen.insert(tool))
        {
            return Err(Error::Forbidden(
                "provider tools must be a unique subset of write_file and run_command".into(),
            ));
        }
        if input.provider.is_empty()
            || input.model.is_empty()
            || input.provider_mode.is_empty()
            || input.delegation_id.is_empty()
            || input.provider != action_spec.provider
            || input.model != action_spec.model
        {
            return Err(Error::Invalid("provider authorization route identity"));
        }
        let grant =
            super::delegations::load_delegation_v2(&mut *transaction, &input.delegation_id).await?;
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
        // Per-operation and capacity bounds must stay inside the grant. There is
        // no attempt/session/turn/wall-clock intersection to perform.
        if !operation_limits_within(&input.operations, &grant.operations) {
            return Err(Error::Forbidden(
                "operation limits exceed the standing delegation".into(),
            ));
        }
        if input.progress != grant.progress {
            return Err(Error::Forbidden(
                "progress policy differs from the standing delegation".into(),
            ));
        }
        let (requests, sessions, attempts) = match reservation_policy {
            AuthorizationReservationPolicy::Campaign => (1, 0, 0),
            AuthorizationReservationPolicy::LeadStart => (0, 0, 0),
        };
        super::delegations::record_usage_v2_in_tx(
            &mut *transaction,
            &grant,
            requests,
            sessions,
            attempts,
        )
        .await?;
    }
    if fixture && !input.allowed_tools.is_empty() {
        return Err(Error::Forbidden(
            "fixture authorization cannot grant provider tools".into(),
        ));
    }
    if fixture && input.monetary_envelope_id != NO_BILLABLE_ENVELOPE {
        return Err(Error::Forbidden(format!(
            "fixture authorization uses {NO_BILLABLE_ENVELOPE:?}, not a billable envelope"
        )));
    }
    let now = Utc::now().to_rfc3339();
    let scope_sha256 = scope_digest(input);
    let policy = ExecutionPolicyV2 {
        policy_id: format!("policy:{}", input.authorization_id),
        authorization_id: input.authorization_id.clone(),
        monetary_envelope_id: input.monetary_envelope_id.clone(),
        scope_sha256,
        progress: input.progress,
        operations: input.operations,
    };
    let policy_json = encode_execution_policy(&policy)?;
    let policy_sha256 = sha256_hex(policy_json.as_bytes());
    sqlx::query::query(
        "INSERT INTO execution_policies
         (policy_id,schema_version,authorization_id,monetary_envelope_id,scope_sha256,
          progress_policy_json,operation_limits_json,policy_json,policy_sha256,created_at)
         VALUES (?1,2,?2,?3,?4,?5,?6,?7,?8,?9)",
    )
    .bind(&policy.policy_id)
    .bind(&policy.authorization_id)
    .bind(&policy.monetary_envelope_id)
    .bind(&policy.scope_sha256)
    .bind(
        serde_json::to_string(&policy.progress)
            .map_err(|_| Error::Invalid("progress policy encoding"))?,
    )
    .bind(
        serde_json::to_string(&policy.operations)
            .map_err(|_| Error::Invalid("operation limits encoding"))?,
    )
    .bind(&policy_json)
    .bind(&policy_sha256)
    .bind(&now)
    .execute(&mut *transaction)
    .await?;
    sqlx::query::query(
        "INSERT INTO campaign_authorizations
         (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
          authorized_paths_json,created_at,provider,model,provider_mode,delegation_id,
          allowed_tools_json,schema_version,policy_id)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,2,?14)",
    )
    .bind(campaign_id)
    .bind(&input.authorization_id)
    .bind(&input.risk_class)
    .bind(&input.execution_mode)
    .bind(&input.actor)
    .bind(&input.rationale)
    .bind(paths_json)
    .bind(&now)
    .bind(&input.provider)
    .bind(&input.model)
    .bind(&input.provider_mode)
    .bind(&input.delegation_id)
    .bind(
        serde_json::to_string(&input.allowed_tools)
            .map_err(|_| Error::Invalid("allowed tools encoding"))?,
    )
    .bind(&policy.policy_id)
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
        &mut *transaction,
        campaign_id,
        "campaign.authorized",
        &format!(
            "authorization={} policy={} risk=R0 mode={}",
            input.authorization_id, policy.policy_id, input.execution_mode
        ),
    )
    .await?;
    Ok(CampaignAuthorizationRecord {
        campaign_id: campaign_id.into(),
        authorization_id: input.authorization_id.clone(),
        risk_class: input.risk_class.clone(),
        execution_mode: input.execution_mode.clone(),
        actor: input.actor.clone(),
        rationale: input.rationale.clone(),
        authorized_paths: input.authorized_paths.clone(),
        provider: input.provider.clone(),
        model: input.model.clone(),
        provider_mode: input.provider_mode.clone(),
        delegation_id: input.delegation_id.clone(),
        allowed_tools: input.allowed_tools.clone(),
        schema_version: 2,
        policy: Some(policy),
        legacy: None,
        created_at: now,
    })
}

/// One canonical persisted representation for a V2 policy. The envelope carries
/// the explicit `version` discriminator so every Core and Worker decoder uses
/// the same bytes.
pub(super) fn encode_execution_policy(policy: &ExecutionPolicyV2) -> Result<String, Error> {
    serde_json::to_string(&FrozenExecutionPolicy::V2(policy.clone()))
        .map_err(|_| Error::Invalid("execution policy encoding"))
}

/// Decode the canonical persisted policy. A V1 envelope is historical evidence
/// and is never reinterpreted as V2.
pub(super) fn decode_execution_policy(json: &str) -> Result<ExecutionPolicyV2, Error> {
    match serde_json::from_str::<FrozenExecutionPolicy>(json)
        .map_err(|_| Error::Invalid("stored execution policy encoding"))?
    {
        FrozenExecutionPolicy::V2(policy) => Ok(policy),
        FrozenExecutionPolicy::LegacyV1(_) => Err(Error::Invalid(
            "legacy V1 policy cannot be decoded as V2 execution policy",
        )),
        FrozenExecutionPolicy::V3(_) => Err(Error::Invalid(
            "V3 policy cannot be decoded as V2 execution policy",
        )),
    }
}

fn scope_digest(input: &NewCampaignAuthorization) -> String {
    let canonical = serde_json::json!({
        "authorizedPaths": input.authorized_paths,
        "provider": input.provider,
        "model": input.model,
        "providerMode": input.provider_mode,
        "delegationId": input.delegation_id,
        "allowedTools": input.allowed_tools,
        "executionMode": input.execution_mode,
    });
    sha256_hex(
        serde_json::to_vec(&canonical)
            .expect("authorization scope serializes")
            .as_slice(),
    )
}

fn validate_operation_limits(limits: &OperationLimits) -> Result<(), Error> {
    if limits.model_request_timeout_secs.get() == 0
        || limits.tool_call_timeout_secs.get() == 0
        || limits.max_request_bytes.get() == 0
        || limits.max_response_bytes.get() == 0
        || limits.max_generation_tokens.get() == 0
        || limits.max_in_flight_operations.get() == 0
        || limits.max_in_flight_operations.get() > 32
    {
        return Err(Error::Invalid("campaign authorization operation limits"));
    }
    Ok(())
}

fn operation_limits_within(requested: &OperationLimits, granted: &OperationLimits) -> bool {
    requested.model_request_timeout_secs <= granted.model_request_timeout_secs
        && requested.tool_call_timeout_secs <= granted.tool_call_timeout_secs
        && requested.max_request_bytes <= granted.max_request_bytes
        && requested.max_response_bytes <= granted.max_response_bytes
        && requested.max_generation_tokens <= granted.max_generation_tokens
        && requested.max_in_flight_operations <= granted.max_in_flight_operations
}

pub(super) async fn load_authorization(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Option<CampaignAuthorizationRecord>, Error> {
    use sqlx::row::Row;

    let row = sqlx::query::query(
        "SELECT ca.campaign_id,ca.authorization_id,ca.risk_class,ca.execution_mode,ca.actor,
                ca.rationale,ca.authorized_paths_json,ca.created_at,ca.provider,ca.model,
                ca.provider_mode,ca.delegation_id,ca.allowed_tools_json,ca.schema_version,
                ca.max_attempts,ca.max_sessions,ca.max_turns,ca.max_input_tokens,
                ca.max_output_tokens,ca.max_cost_micros,ca.deadline_secs,ca.policy_id,
                p.policy_json
         FROM campaign_authorizations ca
         LEFT JOIN execution_policies p ON p.policy_id=ca.policy_id
         WHERE ca.campaign_id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let paths: String = row.try_get("authorized_paths_json")?;
    let allowed_tools: String = row.try_get("allowed_tools_json")?;
    let schema_version: i64 = row.try_get("schema_version")?;
    let base = CampaignAuthorizationRecord {
        campaign_id: row.try_get("campaign_id")?,
        authorization_id: row.try_get("authorization_id")?,
        risk_class: row.try_get("risk_class")?,
        execution_mode: row.try_get("execution_mode")?,
        actor: row.try_get("actor")?,
        rationale: row.try_get("rationale")?,
        authorized_paths: serde_json::from_str(&paths)
            .map_err(|_| Error::Invalid("stored authorized paths encoding"))?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        provider_mode: row.try_get("provider_mode")?,
        delegation_id: row.try_get("delegation_id")?,
        allowed_tools: serde_json::from_str(&allowed_tools)
            .map_err(|_| Error::Invalid("stored allowed tools encoding"))?,
        schema_version: u32::try_from(schema_version)
            .map_err(|_| Error::Invalid("stored authorization version"))?,
        policy: None,
        legacy: None,
        created_at: row.try_get("created_at")?,
    };
    match base.schema_version {
        1 => {
            let max_attempts: i64 = required_cap(&row, "max_attempts")?;
            let max_sessions: i64 = required_cap(&row, "max_sessions")?;
            let max_turns: i64 = required_cap(&row, "max_turns")?;
            let max_input_tokens: i64 = required_cap(&row, "max_input_tokens")?;
            let max_output_tokens: i64 = required_cap(&row, "max_output_tokens")?;
            let max_cost_micros: i64 = required_cap(&row, "max_cost_micros")?;
            let deadline_secs: i64 = required_cap(&row, "deadline_secs")?;
            let policy_id: Option<String> = row.try_get("policy_id")?;
            if policy_id.is_some() {
                return Err(Error::Invalid(
                    "legacy authorization unexpectedly references a V2 policy",
                ));
            }
            Ok(Some(CampaignAuthorizationRecord {
                legacy: Some(LegacyCampaignAuthorizationCaps {
                    max_attempts: u32::try_from(max_attempts)
                        .map_err(|_| Error::Invalid("stored max attempts"))?,
                    max_sessions: u32::try_from(max_sessions)
                        .map_err(|_| Error::Invalid("stored session budget"))?,
                    max_turns: u32::try_from(max_turns)
                        .map_err(|_| Error::Invalid("stored turn budget"))?,
                    max_input_tokens: u64::try_from(max_input_tokens)
                        .map_err(|_| Error::Invalid("stored input budget"))?,
                    max_output_tokens: u64::try_from(max_output_tokens)
                        .map_err(|_| Error::Invalid("stored output budget"))?,
                    max_cost_micros: u64::try_from(max_cost_micros)
                        .map_err(|_| Error::Invalid("stored cost budget"))?,
                    deadline_secs: u64::try_from(deadline_secs)
                        .map_err(|_| Error::Invalid("stored provider deadline"))?,
                }),
                ..base
            }))
        }
        2 => {
            for column in [
                "max_attempts",
                "max_sessions",
                "max_turns",
                "max_input_tokens",
                "max_output_tokens",
                "max_cost_micros",
                "deadline_secs",
            ] {
                if row.try_get::<Option<i64>, _>(column)?.is_some() {
                    return Err(Error::Invalid(
                        "V2 authorization carries an obsolete cap column",
                    ));
                }
            }
            let policy_id: Option<String> = row.try_get("policy_id")?;
            let policy_json: Option<String> = row.try_get("policy_json")?;
            let (Some(policy_id), Some(policy_json)) = (policy_id, policy_json) else {
                return Err(Error::Invalid("V2 authorization has no frozen policy"));
            };
            let policy = decode_execution_policy(&policy_json)?;
            if policy.policy_id != policy_id || policy.authorization_id != base.authorization_id {
                return Err(Error::Invalid("V2 authorization policy identity mismatch"));
            }
            Ok(Some(CampaignAuthorizationRecord {
                policy: Some(policy),
                ..base
            }))
        }
        _ => Err(Error::Invalid("unsupported authorization schema version")),
    }
}

fn required_cap(row: &sqlx_sqlite::SqliteRow, column: &str) -> Result<i64, Error> {
    use sqlx::row::Row;
    row.try_get::<Option<i64>, _>(column)?
        .ok_or_else(|| Error::Invalid("legacy authorization is missing a cap column"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_policy::{FundingPolicy, ProgressPolicy};
    use crate::lead::{
        Capability, CheckKind, ObjectiveClass, ProviderRoute, RiskClass, SpecCheck,
        StandingDelegationV2,
    };
    use std::num::NonZeroU32;
    use std::path::{Path, PathBuf};

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
        }
    }

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.sqlite", uuid::Uuid::new_v4()))
    }

    async fn fixture() -> (PathBuf, Database, StandingDelegationV2) {
        let path = temp_path("factory-authorization-v2");
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture',?1,?2,'')",
        )
        .bind("a".repeat(40))
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        let spec = ActionSpec {
            schema_version: 1,
            project_id: "project".into(),
            repository: "/fixture".into(),
            base_commit: "a".repeat(40),
            objective: "Implement the feature.".into(),
            objective_sha256: "b".repeat(64),
            provider: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            allowed_paths: vec!["src".into()],
            checks: vec![SpecCheck {
                id: "test".into(),
                kind: CheckKind::Test,
                description: "test".into(),
                command: vec!["cargo".into(), "test".into()],
                expect_exit: 0,
                timeout_secs: 60,
            }],
        };
        let _action = db
            .create_action_with_spec(
                "project",
                "Implement the feature.",
                "opencode",
                "deepseek-v4.1-flash",
                "action-key",
                ActionSpecInput {
                    allowed_paths: &spec.allowed_paths,
                    checks: &spec.checks,
                },
            )
            .await
            .unwrap();
        let mut grant = StandingDelegationV2 {
            id: "grant".into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: "project".into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![Capability::RepositoryEdit],
            risk_classes: vec![RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
            route: ProviderRoute {
                provider: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                mode: "build".into(),
            },
            max_timeout_secs: 600,
            max_parallel: 1,
            operations: operations(),
            progress: progress(),
            expires_at: Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        db.create_standing_delegation_v2(&grant, "grant-key")
            .await
            .unwrap();
        (path, db, grant)
    }

    fn authorization() -> NewCampaignAuthorization {
        NewCampaignAuthorization {
            authorization_id: "auth-v2".into(),
            risk_class: "R0".into(),
            execution_mode: "TRUSTED_LOCAL_PROVIDER".into(),
            actor: "owner".into(),
            rationale: "explicit R0 execution approval".into(),
            authorized_paths: vec!["src".into()],
            provider: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            provider_mode: "build".into(),
            delegation_id: "grant".into(),
            allowed_tools: vec!["write_file".into()],
            monetary_envelope_id: "envelope-1".into(),
            progress: progress(),
            operations: operations(),
        }
    }

    fn subscription_policy() -> ExecutionPolicyV3 {
        let binding = crate::execution_policy::SubscriptionBinding {
            route: crate::execution_policy::SubscriptionRoute::OpenCodeGo,
            entitlement: crate::execution_policy::ProviderEntitlement::Unknown {
                reason: "not verified in milestone one".into(),
            },
            provider_account_id: "account".into(),
            plan_id: "go".into(),
            model_id: "model".into(),
            settings_revision: "revision-1".into(),
            entitlement_verified: false,
            no_charge_route_verified: false,
            extra_usage_disabled: false,
            api_key_route_disabled: false,
            quota: crate::execution_policy::QuotaObservation::Unknown {
                observed_at_ms: 10,
                reason: "not observed".into(),
            },
        };
        let binding_json = serde_json::to_string(&binding).unwrap();
        ExecutionPolicyV3 {
            policy_id: "policy-subscription-1".into(),
            authorization_id: "auth-subscription-1".into(),
            funding: FundingPolicy::Subscription {
                grant_id: "grant".into(),
                binding_sha256: sha256_hex(binding_json.as_bytes()),
                binding: Box::new(binding),
            },
            scope_sha256: "d".repeat(64),
            progress: progress(),
            operations: operations(),
        }
    }

    #[tokio::test]
    async fn subscription_policy_persists_without_monetary_envelope_and_pauses_across_reopen() {
        let (path, db, _) = fixture().await;
        let action_id: (String,) = sqlx::query_as::query_as("SELECT id FROM actions LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let campaign = db
            .create_campaign(&action_id.0, "subscription-campaign")
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO inference_envelopes
             (id,request_id,project_id,currency,state,authorized_micros,authorization_revision,created_at_ms)
             VALUES ('legacy-envelope','legacy-request','project','USD','ACTIVE',100,1,1)",
        ).execute(&db.pool).await.unwrap();
        let policy = subscription_policy();
        let record = db
            .record_subscription_funding_policy(&campaign.id, &policy, 10)
            .await
            .unwrap();
        assert!(!record.enabled);
        assert!(matches!(
            record.policy.funding,
            FundingPolicy::Subscription { .. }
        ));
        assert!(
            !serde_json::to_string(&record.policy)
                .unwrap()
                .contains("monetaryEnvelopeId")
        );
        db.pause_subscription_funding_for_quota(
            &campaign.id,
            &policy.authorization_id,
            20,
            Some(30),
            "provider quota exhausted",
        )
        .await
        .unwrap();
        db.close().await;
        let reopened = Database::open(&path).await.unwrap();
        let loaded = reopened
            .subscription_funding_policy(&campaign.id)
            .await
            .unwrap();
        assert!(!loaded.enabled);
        assert!(matches!(
            loaded.state,
            crate::execution_policy::FundingRecoveryState::PausedQuota {
                observed_at_ms: 20,
                resets_at_ms: Some(30),
                ..
            }
        ));
        let versions: (i64, i64) = sqlx::query_as::query_as(
            "SELECT (SELECT version FROM _sqlx_migrations WHERE version=41),
                    (SELECT count(*) FROM inference_envelopes)",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
        assert_eq!(versions.0, 41);
        assert_eq!(
            versions.1, 1,
            "existing monetary ledger evidence must survive migration and reopen"
        );
        reopened.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn subscription_persistence_rejects_paid_unknown_and_authorized_transitions() {
        let (path, db, _) = fixture().await;
        let action_id: (String,) = sqlx::query_as::query_as("SELECT id FROM actions LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let campaign = db
            .create_campaign(&action_id.0, "subscription-campaign-negative")
            .await
            .unwrap();
        let mut paid = subscription_policy();
        paid.funding = FundingPolicy::Billable {
            monetary_envelope_id: "env".into(),
        };
        assert!(
            db.record_subscription_funding_policy(&campaign.id, &paid, 10)
                .await
                .is_err()
        );
        assert!(
            serde_json::from_value::<ExecutionPolicyV3>(serde_json::json!({
                "policyId":"p", "authorizationId":"a", "funding":{"kind":"FUTURE_MODE"},
                "scopeSha256":"d", "progress":{}, "operations":{}
            }))
            .is_err()
        );
        let policy = subscription_policy();
        db.record_subscription_funding_policy(&campaign.id, &policy, 10)
            .await
            .unwrap();
        let mut execution_auth = authorization();
        execution_auth.authorization_id = "execution-after-subscription-proposal".into();
        assert!(
            db.authorize_campaign(&campaign.id, &execution_auth)
                .await
                .is_err()
        );
        assert!(
            db.record_subscription_funding_policy(&campaign.id, &policy, 11)
                .await
                .is_err()
        );
        db.pause_subscription_funding_for_quota(&campaign.id, "wrong-auth", 20, None, "quota")
            .await
            .unwrap_err();
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn v2_authorization_freezes_policy_without_legacy_caps() {
        let (path, db, _grant) = fixture().await;
        let action_id: (String,) = sqlx::query_as::query_as("SELECT id FROM actions LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let campaign = db
            .create_campaign(&action_id.0, "campaign-key")
            .await
            .unwrap();
        let (record, created) = db
            .authorize_campaign(&campaign.id, &authorization())
            .await
            .unwrap();
        assert!(created);
        assert_eq!(record.schema_version, 2);
        assert!(record.legacy.is_none());
        let policy = record.policy.as_ref().unwrap();
        assert_eq!(policy.authorization_id, "auth-v2");
        assert_eq!(policy.monetary_envelope_id, "envelope-1");
        assert_eq!(policy.progress, progress());
        assert_eq!(policy.operations, operations());

        let (caps, policy_id, policy_json): (i64, String, String) =
            sqlx::query_as::query_as(
                "SELECT (max_attempts IS NULL) + (max_sessions IS NULL) + (max_turns IS NULL)
                      + (max_input_tokens IS NULL) + (max_output_tokens IS NULL)
                      + (max_cost_micros IS NULL) + (deadline_secs IS NULL),
                        policy_id, (SELECT policy_json FROM execution_policies WHERE policy_id=ca.policy_id)
                 FROM campaign_authorizations ca WHERE ca.campaign_id=?1",
            )
            .bind(&campaign.id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(caps, 7, "every obsolete cap column must be NULL for V2");
        let decoded = decode_execution_policy(&policy_json).unwrap();
        assert_eq!(decoded.policy_id, policy_id);

        let (replayed, created) = db
            .authorize_campaign(&campaign.id, &authorization())
            .await
            .unwrap();
        assert!(!created);
        assert_eq!(replayed.policy, record.policy);
        let loaded = db.campaign_authorization(&campaign.id).await.unwrap();
        assert_eq!(loaded, record);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn v2_authorization_keeps_scope_tools_risk_and_operation_identity() {
        let (path, db, _grant) = fixture().await;
        let action_id: (String,) = sqlx::query_as::query_as("SELECT id FROM actions LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let action_id = action_id.0;
        let mut attempts = Vec::new();

        let mut risk = authorization();
        risk.risk_class = "R1".into();
        attempts.push(("risk", risk, "R0"));
        let mut path_escape = authorization();
        path_escape.authorized_paths = vec!["docs".into()];
        attempts.push(("paths", path_escape, "path"));
        let mut tool = authorization();
        tool.allowed_tools = vec!["read_file".into()];
        attempts.push(("tools", tool, "tool"));
        let mut operations = authorization();
        operations.operations.max_generation_tokens = NonZeroU32::new(16_384).unwrap();
        attempts.push(("operations", operations, "limit"));
        let mut envelope = authorization();
        envelope.monetary_envelope_id = String::new();
        attempts.push(("envelope", envelope, "input"));

        for (index, (label, input, _)) in attempts.into_iter().enumerate() {
            let campaign = db
                .create_campaign(&action_id, &format!("campaign-{index}"))
                .await
                .unwrap();
            let error = db
                .authorize_campaign(&campaign.id, &input)
                .await
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    Error::Forbidden(_) | Error::Invalid(_) | Error::Validation(_)
                ),
                "{label} accepted: {error:?}"
            );
            assert!(
                sqlx::query_as::query_as::<_, (i64,)>(
                    "SELECT COUNT(*) FROM campaign_authorizations WHERE campaign_id=?1"
                )
                .bind(&campaign.id)
                .fetch_one(&db.pool)
                .await
                .unwrap()
                .0 == 0,
                "{label} left an authorization row"
            );
        }
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn legacy_rows_keep_their_original_values_and_are_never_reinterpreted() {
        let (path, db, _grant) = fixture().await;
        let action_id: (String,) = sqlx::query_as::query_as("SELECT id FROM actions LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let campaign = db
            .create_campaign(&action_id.0, "legacy-campaign")
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
              authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
              delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
              max_cost_micros,deadline_secs,allowed_tools_json,schema_version)
             VALUES (?1,'legacy-auth','R0','FIXTURE','owner','historical','[\"src\"]',3,
                     ?2,'','','','',0,0,0,0,0,0,'[]',1)",
        )
        .bind(&campaign.id)
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        let loaded = db.campaign_authorization(&campaign.id).await.unwrap();
        assert!(loaded.is_legacy());
        assert!(loaded.policy.is_none());
        let legacy = loaded.legacy.as_ref().unwrap();
        assert_eq!(legacy.max_attempts, 3);
        assert_eq!(legacy.max_sessions, 0);
        assert_eq!(legacy.deadline_secs, 0);
        assert!(loaded.v2_policy().is_err());

        let error = db
            .authorize_campaign(&campaign.id, &authorization())
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Conflict(_)), "{error:?}");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn production_authorization_round_trips_to_a_worker_assignment() {
        let (path, db, _grant) = fixture().await;
        let action_id: (String,) = sqlx::query_as::query_as("SELECT id FROM actions LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let campaign = db
            .create_campaign(&action_id.0, "assignment-campaign")
            .await
            .unwrap();
        let (record, created) = db
            .authorize_campaign(&campaign.id, &authorization())
            .await
            .unwrap();
        assert!(created);
        let (policy_json,): (String,) = sqlx::query_as::query_as(
            "SELECT p.policy_json FROM campaign_authorizations ca
             JOIN execution_policies p ON p.policy_id=ca.policy_id
             WHERE ca.campaign_id=?1",
        )
        .bind(&campaign.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let envelope: serde_json::Value = serde_json::from_str(&policy_json).unwrap();
        assert_eq!(envelope["version"], "V2");
        assert_eq!(
            envelope["policyId"],
            record.policy.as_ref().unwrap().policy_id
        );

        let run = db.create_run(&campaign.id, "assignment-run").await.unwrap();
        db.create_attempt(&run.id, "assignment-attempt")
            .await
            .unwrap();
        let assignment = db
            .claim_assignment("assignment-worker", Utc::now().timestamp_millis(), 30_000)
            .await
            .unwrap()
            .expect("V2 assignment");
        assert_eq!(assignment.campaign_id, campaign.id);
        assert_eq!(assignment.work_item_id, campaign.id);
        let policy = assignment
            .execution_policy
            .as_ref()
            .and_then(crate::execution_policy::FrozenExecutionPolicy::v2)
            .expect("V2 policy");
        assert_eq!(policy, record.policy.as_ref().unwrap());
        assert_eq!(policy.operations, operations());
        assert_eq!(policy.progress, progress());
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    async fn apply_migrations_up_to(pool: &sqlx_sqlite::SqlitePool, limit: &str) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension().and_then(|ext| ext.to_str()) == Some("sql")
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name <= limit)
            })
            .collect();
        files.sort();
        for file in files {
            let sql = std::fs::read_to_string(&file).unwrap();
            sqlx::raw_sql::raw_sql(&sql)
                .execute(pool)
                .await
                .unwrap_or_else(|error| panic!("migration {:?} failed: {error}", file));
        }
    }

    #[tokio::test]
    async fn populated_legacy_migration_rebuild_preserves_rows_and_fk_integrity() {
        let path = temp_path("factory-authorization-migration");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = sqlx_sqlite::SqlitePool::connect(&url).await.unwrap();
        sqlx::query::query("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        apply_migrations_up_to(&pool, "0033").await;
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture','base','2026-10-28T00:00:00Z','')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO actions(id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES ('action','project','obj','opencode','deepseek-v4.1-flash','R0','PENDING','h','2026-10-28T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES ('campaign','action','AUTHORIZED','2026-10-28T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO standing_delegations(id,principal_id,audit_id,project_id,revision,
               content_hash,content_json,revoked_at,created_at)
             VALUES ('grant','owner','audit','project',1,'hash','{\"legacy\":true}',NULL,
                     '2026-10-28T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
              authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
              delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
              max_cost_micros,deadline_secs,allowed_tools_json)
             VALUES ('campaign','legacy-auth','R0','TRUSTED_LOCAL_PROVIDER','owner','why',
                     '[\"src\"]',3,'2026-10-28T00:00:00Z','opencode','deepseek-v4.1-flash',
                     'build','grant',2,4,40000,12000,500000,600,'[\"write_file\"]')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Active V1 execution evidence must survive the rebuild.
        sqlx::query::query(
            "INSERT INTO runs(id,campaign_id,status,created_at)
             VALUES ('legacy-run','campaign','RUNNING','2026-10-28T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at)
             VALUES ('legacy-attempt','legacy-run','LEASED','2026-10-28T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let migration = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../migrations/0034_execution_policy_v2.sql"),
        )
        .unwrap();

        // Production-shaped interruption: the migration runs in a transaction,
        // an injected failure rolls it back, and the old schema/rows survive.
        let mut interrupted = pool.begin().await.unwrap();
        sqlx::raw_sql::raw_sql(&migration)
            .execute(&mut *interrupted)
            .await
            .unwrap();
        assert!(
            sqlx::raw_sql::raw_sql("INSERT INTO missing_table VALUES (1)")
                .execute(&mut *interrupted)
                .await
                .is_err()
        );
        interrupted.rollback().await.unwrap();
        let (pre_id, pre_attempts): (String, i64) = sqlx::query_as::query_as(
            "SELECT authorization_id,max_attempts FROM campaign_authorizations
             WHERE campaign_id='campaign'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((pre_id.as_str(), pre_attempts), ("legacy-auth", 3));
        assert!(
            sqlx::query_as::query_as::<_, (i64,)>(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='execution_policies'"
            )
            .fetch_one(&pool)
            .await
            .unwrap()
            .0 == 0,
            "rolled-back migration left a new table"
        );

        let mut retry = pool.begin().await.unwrap();
        sqlx::raw_sql::raw_sql(&migration)
            .execute(&mut *retry)
            .await
            .unwrap();
        retry.commit().await.unwrap();

        let (authorization_id, max_attempts, max_sessions, max_cost, deadline, version, policy): (
            String,
            i64,
            i64,
            i64,
            i64,
            i64,
            Option<String>,
        ) = sqlx::query_as::query_as(
            "SELECT authorization_id,max_attempts,max_sessions,max_cost_micros,deadline_secs,
                    schema_version,policy_id
             FROM campaign_authorizations WHERE campaign_id='campaign'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(authorization_id, "legacy-auth");
        assert_eq!(
            (max_attempts, max_sessions, max_cost, deadline),
            (3, 2, 500_000, 600)
        );
        assert_eq!(version, 1);
        assert_eq!(policy, None);

        let (delegation_version, content_json, authority): (i64, String, Option<String>) =
            sqlx::query_as::query_as(
                "SELECT schema_version,content_json,authority_json
                 FROM standing_delegations WHERE id='grant'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(delegation_version, 1);
        assert_eq!(content_json, "{\"legacy\":true}");
        assert_eq!(authority, None);

        let violations: Vec<(String, i64, String, i64)> =
            sqlx::query_as::query_as("PRAGMA foreign_key_check")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(
            violations.is_empty(),
            "foreign key violations: {violations:?}"
        );
        let integrity: Vec<(String,)> = sqlx::query_as::query_as("PRAGMA integrity_check")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(integrity.first().map(|(value,)| value.as_str()), Some("ok"));

        // Reopen the production file and verify the committed rebuild.
        let reopened = sqlx_sqlite::SqlitePool::connect(&url).await.unwrap();
        let (reopened_auth, reopened_attempts): (String, i64) = sqlx::query_as::query_as(
            "SELECT authorization_id,max_attempts FROM campaign_authorizations
             WHERE campaign_id='campaign'",
        )
        .fetch_one(&reopened)
        .await
        .unwrap();
        assert_eq!(
            (reopened_auth.as_str(), reopened_attempts),
            ("legacy-auth", 3)
        );
        for (table, id) in [("runs", "legacy-run"), ("attempts", "legacy-attempt")] {
            let count: (i64,) =
                sqlx::query_as::query_as(&format!("SELECT COUNT(*) FROM {table} WHERE id=?1"))
                    .bind(id)
                    .fetch_one(&reopened)
                    .await
                    .unwrap();
            assert_eq!(count.0, 1, "{table} row was lost");
        }
        let reopened_violations: Vec<(String, i64, String, i64)> =
            sqlx::query_as::query_as("PRAGMA foreign_key_check")
                .fetch_all(&reopened)
                .await
                .unwrap();
        assert!(reopened_violations.is_empty(), "{reopened_violations:?}");
        reopened.close().await;

        // A V2 authorization cannot carry a legacy cap column.
        sqlx::query::query(
            "INSERT INTO execution_policies(policy_id,schema_version,authorization_id,
               monetary_envelope_id,scope_sha256,progress_policy_json,operation_limits_json,
               policy_json,policy_sha256,created_at)
             VALUES ('policy-v2',2,'auth-v2','envelope',?1,'{}','{}','{}',?2,
                     '2026-10-28T00:00:00Z')",
        )
        .bind("c".repeat(64))
        .bind("d".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
        let rejected = sqlx::query::query(
            "INSERT INTO campaign_authorizations
             (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
              authorized_paths_json,max_turns,created_at,provider,model,provider_mode,
              delegation_id,allowed_tools_json,schema_version,policy_id)
             VALUES ('campaign','auth-v2','R0','TRUSTED_LOCAL_PROVIDER','owner','why',
                     '[\"src\"]',4,'2026-10-28T00:00:00Z','opencode','deepseek-v4.1-flash',
                     'build','grant','[]',2,'policy-v2')",
        )
        .execute(&pool)
        .await;
        assert!(rejected.is_err(), "V2 row with a cap must fail");
        pool.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[allow(dead_code)]
    fn _assert_policy_type(_policy: &ExecutionPolicyV2) {}
}
