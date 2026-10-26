//! Core-owned model-call reservation ledger.
//!
//! Every non-Worker model call (Lead assessment, semantic review, CAP-014
//! evaluation) reserves a bounded token/cost envelope in Core before any
//! upstream effect. Settlement records measured usage. An interrupted call is
//! marked ambiguous and cannot be silently replayed; an operator must resolve
//! it explicitly.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCallRole {
    Lead,
    SemanticReviewer,
    Evaluation,
}

impl ModelCallRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "LEAD",
            Self::SemanticReviewer => "SEMANTIC_REVIEWER",
            Self::Evaluation => "EVALUATION",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "LEAD" => Some(Self::Lead),
            "SEMANTIC_REVIEWER" => Some(Self::SemanticReviewer),
            "EVALUATION" => Some(Self::Evaluation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCallPolicy {
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewModelCallAuthorization {
    pub role: ModelCallRole,
    pub principal_id: String,
    pub project_id: String,
    pub objective_id: String,
    pub scope_id: String,
    pub provider: String,
    pub model: String,
    pub purpose: String,
    pub request_sha256: String,
    pub estimated_input_tokens: u64,
    pub requested_output_tokens: u64,
    pub estimated_cost_micros: u64,
    pub policy: ModelCallPolicy,
    #[serde(default)]
    pub global_policy: Option<ModelCallPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallGrant {
    pub reservation_id: String,
    pub role: ModelCallRole,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_total_tokens: u64,
    pub max_cost_micros: u64,
    pub request_sha256: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallResult {
    pub reservation_id: String,
    pub role: ModelCallRole,
    pub provider: String,
    pub model: String,
    pub request_sha256: String,
    pub content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelCallDecision {
    Reserved(ModelCallGrant),
    Replayed(ModelCallResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallUsage {
    pub reservation_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallRecord {
    pub reservation_id: String,
    pub role: ModelCallRole,
    pub principal_id: String,
    pub project_id: String,
    pub objective_id: String,
    pub scope_id: String,
    pub provider: String,
    pub model: String,
    pub purpose: String,
    pub request_sha256: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub state: String,
    pub error: String,
    pub created_at: String,
    pub updated_at: String,
}

type SettleRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
);
type IdentityRow = (
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    String,
    String,
);
type ReplayRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
);

const MAX_CALL_OUTPUT_TOKENS: u64 = 8192;
const RESERVATION_LIFETIME_MS: i64 = 120_000;

impl Database {
    pub async fn authorize_model_call(
        &self,
        input: &NewModelCallAuthorization,
    ) -> Result<ModelCallDecision, Error> {
        validate_model_call_authorization(input)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        self.validate_model_call_scope(&mut transaction, input)
            .await?;
        let now = Utc::now();
        let now_ms = now.timestamp_millis();
        expire_reserved_model_calls(&mut transaction, now_ms).await?;
        if let Some(existing) = load_model_call_identity(&mut transaction, input).await? {
            match existing.state.as_str() {
                "SETTLED" => {
                    let result = replay_model_call(&mut transaction, &existing).await?;
                    transaction.commit().await?;
                    return Ok(ModelCallDecision::Replayed(result));
                }
                "AMBIGUOUS" | "UNKNOWN" => {
                    transaction.commit().await?;
                    return Err(Error::Forbidden(
                        "model call has an unresolved upstream outcome; explicit operator release is required"
                            .into(),
                    ));
                }
                "RESERVED" => {
                    return Err(Error::Conflict("model call is already in flight"));
                }
                "RELEASED" => {}
                _ => return Err(Error::Invalid("stored model call state")),
            }
        }
        let (used_input, used_output, used_cost) =
            model_call_usage(&mut transaction, &input.scope_id).await?;
        let (reserved_input, reserved_output, reserved_cost) =
            model_call_reservations(&mut transaction, &input.scope_id).await?;
        let scope_remaining_input = input
            .policy
            .max_input_tokens
            .saturating_sub(used_input)
            .saturating_sub(reserved_input);
        let scope_remaining_output = input
            .policy
            .max_output_tokens
            .saturating_sub(used_output)
            .saturating_sub(reserved_output);
        let scope_remaining_cost = input
            .policy
            .max_cost_micros
            .saturating_sub(used_cost)
            .saturating_sub(reserved_cost);
        let (remaining_input, remaining_output, remaining_cost) = match input.global_policy {
            Some(global) => {
                let (global_used_input, global_used_output, global_used_cost) =
                    model_call_role_usage(&mut transaction, input.role).await?;
                let (global_reserved_input, global_reserved_output, global_reserved_cost) =
                    model_call_role_reservations(&mut transaction, input.role).await?;
                (
                    scope_remaining_input.min(
                        global
                            .max_input_tokens
                            .saturating_sub(global_used_input)
                            .saturating_sub(global_reserved_input),
                    ),
                    scope_remaining_output.min(
                        global
                            .max_output_tokens
                            .saturating_sub(global_used_output)
                            .saturating_sub(global_reserved_output),
                    ),
                    scope_remaining_cost.min(
                        global
                            .max_cost_micros
                            .saturating_sub(global_used_cost)
                            .saturating_sub(global_reserved_cost),
                    ),
                )
            }
            None => (
                scope_remaining_input,
                scope_remaining_output,
                scope_remaining_cost,
            ),
        };
        // Reserve the whole remaining input/cost envelope rather than a
        // tokenizer estimate. A measured response therefore cannot exceed the
        // reservation merely because the estimate was low.
        if input.estimated_input_tokens > remaining_input
            || input.estimated_cost_micros > remaining_cost
            || remaining_input == 0
            || remaining_cost == 0
        {
            return Err(Error::Forbidden("model call budget is exhausted".into()));
        }
        let max_input = remaining_input;
        let max_output = input.requested_output_tokens.min(remaining_output);
        if max_output == 0 {
            return Err(Error::Forbidden("model call budget is exhausted".into()));
        }
        let max_total = max_input.saturating_add(max_output);
        let max_cost = remaining_cost;
        let request_sha256 = input.request_sha256.to_ascii_lowercase();
        let reservation_id = Uuid::new_v4().to_string();
        let expires_at_ms = now_ms.saturating_add(RESERVATION_LIFETIME_MS);
        let now_text = now.to_rfc3339();
        sqlx::query::query(
            "INSERT INTO model_call_reservations
             (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
              purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
              max_cost_micros,state,expires_at_ms,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'RESERVED',?15,?16,?16)",
        )
        .bind(&reservation_id)
        .bind(input.role.as_str())
        .bind(&input.principal_id)
        .bind(&input.project_id)
        .bind(&input.objective_id)
        .bind(&input.scope_id)
        .bind(&input.provider)
        .bind(&input.model)
        .bind(&input.purpose)
        .bind(&request_sha256)
        .bind(
            i64::try_from(max_input)
                .map_err(|_| Error::Invalid("model call input token reservation"))?,
        )
        .bind(
            i64::try_from(max_output)
                .map_err(|_| Error::Invalid("model call output token reservation"))?,
        )
        .bind(
            i64::try_from(max_total)
                .map_err(|_| Error::Invalid("model call total token reservation"))?,
        )
        .bind(i64::try_from(max_cost).map_err(|_| Error::Invalid("model call cost reservation"))?)
        .bind(expires_at_ms)
        .bind(&now_text)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &input.objective_id,
            "model.call_authorized",
            &format!(
                "role={} reservation={reservation_id} provider={} model={} input={max_input} output={max_output} cost={max_cost}",
                input.role.as_str(),
                input.provider,
                input.model
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(ModelCallDecision::Reserved(ModelCallGrant {
            reservation_id,
            role: input.role,
            max_input_tokens: max_input,
            max_output_tokens: max_output,
            max_total_tokens: max_total,
            max_cost_micros: max_cost,
            request_sha256,
            expires_at_ms,
        }))
    }

    pub async fn settle_model_call(
        &self,
        usage: &ModelCallUsage,
    ) -> Result<ModelCallResult, Error> {
        if usage.content.len() > 256 * 1024 {
            return Err(Error::Invalid("model call response is too large"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row: Option<SettleRow> = sqlx::query_as::query_as(
            "SELECT state,role,provider,model,request_sha256,max_input_tokens,
                    max_output_tokens,max_total_tokens,max_cost_micros,
                    COALESCE(response_text,''),COALESCE(error,'')
             FROM model_call_reservations WHERE reservation_id=?1",
        )
        .bind(&usage.reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((
            state,
            role,
            provider,
            model,
            request_sha256,
            max_input,
            max_output,
            max_total,
            max_cost,
            prior_content,
            _,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        let role = ModelCallRole::parse(&role).ok_or(Error::Invalid("stored model call role"))?;
        if state == "SETTLED" {
            let result = replay_model_call_by_id(&mut transaction, &usage.reservation_id).await?;
            if result.input_tokens == usage.input_tokens
                && result.output_tokens == usage.output_tokens
                && result.total_tokens == usage.total_tokens
                && result.cost_micros == usage.cost_micros
                && result.usage_known == usage.usage_known
                && prior_content == usage.content
            {
                transaction.commit().await?;
                return Ok(result);
            }
            return Err(Error::Conflict("model call already settled differently"));
        }
        if state != "RESERVED" {
            return Err(Error::Conflict("model call is not reserved"));
        }
        let input = i64::try_from(usage.input_tokens)
            .map_err(|_| Error::Invalid("model call input usage"))?;
        let output = i64::try_from(usage.output_tokens)
            .map_err(|_| Error::Invalid("model call output usage"))?;
        let total = i64::try_from(usage.total_tokens)
            .map_err(|_| Error::Invalid("model call total usage"))?;
        let cost = i64::try_from(usage.cost_micros)
            .map_err(|_| Error::Invalid("model call cost usage"))?;
        let exceeds = input > max_input
            || output > max_output
            || total > max_total
            || cost > max_cost
            || total < input.saturating_add(output);
        let (next_state, error) = if !usage.usage_known {
            (
                "UNKNOWN",
                "model call usage is unknown; budget remains reserved",
            )
        } else if usage.content.trim().is_empty() {
            (
                "UNKNOWN",
                "model call returned no usable response; budget remains reserved",
            )
        } else if exceeds {
            (
                "UNKNOWN",
                "model call exceeded its authorized budget; budget remains reserved",
            )
        } else {
            ("SETTLED", "")
        };
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE model_call_reservations
             SET state=?1,input_tokens=?2,output_tokens=?3,total_tokens=?4,cost_micros=?5,
                 usage_known=?6,response_text=?7,error=?8,updated_at=?9
             WHERE reservation_id=?10 AND state='RESERVED'",
        )
        .bind(next_state)
        .bind(input)
        .bind(output)
        .bind(total)
        .bind(cost)
        .bind(i64::from(usage.usage_known))
        .bind(&usage.content)
        .bind(if error.is_empty() { None } else { Some(error) })
        .bind(&now)
        .bind(&usage.reservation_id)
        .execute(&mut *transaction)
        .await?;
        let objective_id: String = sqlx::query::query(
            "SELECT objective_id FROM model_call_reservations WHERE reservation_id=?1",
        )
        .bind(&usage.reservation_id)
        .fetch_one(&mut *transaction)
        .await?
        .try_get("objective_id")?;
        record_event(
            &mut transaction,
            &objective_id,
            "model.call_settled",
            &format!(
                "role={} reservation={} state={next_state} input={input} output={output} total={total} cost={cost}",
                role.as_str(),
                usage.reservation_id
            ),
        )
        .await?;
        transaction.commit().await?;
        if next_state == "UNKNOWN" {
            return Err(Error::Forbidden(error.into()));
        }
        Ok(ModelCallResult {
            reservation_id: usage.reservation_id.clone(),
            role,
            provider,
            model,
            request_sha256,
            content: usage.content.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cost_micros: usage.cost_micros,
            usage_known: usage.usage_known,
        })
    }

    pub async fn release_model_call(
        &self,
        reservation_id: &str,
        reason: &str,
        ambiguous: bool,
    ) -> Result<(), Error> {
        if reason.trim().is_empty() || reason.len() > 500 {
            return Err(Error::Invalid("model call release reason"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let state = if ambiguous { "AMBIGUOUS" } else { "RELEASED" };
        let now = Utc::now().to_rfc3339();
        let objective_id: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT objective_id FROM model_call_reservations WHERE reservation_id=?1 AND state='RESERVED'",
        )
        .bind(reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((objective_id,)) = objective_id else {
            return Err(Error::Conflict("model call is not reserved"));
        };
        sqlx::query::query(
            "UPDATE model_call_reservations SET state=?1,error=?2,updated_at=?3
             WHERE reservation_id=?4 AND state='RESERVED'",
        )
        .bind(state)
        .bind(reason)
        .bind(&now)
        .bind(reservation_id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &objective_id,
            "model.call_released",
            &format!("reservation={reservation_id} state={state} reason={reason}"),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn resolve_ambiguous_model_call(
        &self,
        reservation_id: &str,
        actor: &str,
        reason: &str,
    ) -> Result<(), Error> {
        if actor.trim().is_empty()
            || actor.len() > 128
            || reason.trim().is_empty()
            || reason.len() > 500
        {
            return Err(Error::Invalid("model call resolution identity"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let objective_id: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT objective_id FROM model_call_reservations
             WHERE reservation_id=?1 AND state IN ('AMBIGUOUS','UNKNOWN')",
        )
        .bind(reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((objective_id,)) = objective_id else {
            return Err(Error::Conflict("model call is not ambiguous"));
        };
        sqlx::query::query(
            "UPDATE model_call_reservations SET state='RELEASED',error=?1,updated_at=?2
             WHERE reservation_id=?3 AND state IN ('AMBIGUOUS','UNKNOWN')",
        )
        .bind(format!("resolved by {actor}: {reason}"))
        .bind(Utc::now().to_rfc3339())
        .bind(reservation_id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &objective_id,
            "model.call_resolved",
            &format!("reservation={reservation_id} actor={actor} reason={reason}"),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn model_calls_for_scope(
        &self,
        scope_id: &str,
    ) -> Result<Vec<ModelCallRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,
                    model,purpose,request_sha256,input_tokens,output_tokens,total_tokens,
                    cost_micros,usage_known,state,COALESCE(error,'') AS error,created_at,updated_at
             FROM model_call_reservations WHERE scope_id=?1 ORDER BY created_at,reservation_id",
        )
        .bind(scope_id)
        .fetch_all(&self.pool)
        .await?;
        model_call_records(rows)
    }

    /// Lead model-call provenance bound to one Lead request. This exposes the
    /// Core reservation identity, route and measured usage without returning
    /// prompts, responses or credentials.
    pub async fn lead_model_calls(&self, request_id: &str) -> Result<Vec<ModelCallRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,
                    model,purpose,request_sha256,input_tokens,output_tokens,total_tokens,
                    cost_micros,usage_known,state,COALESCE(error,'') AS error,created_at,updated_at
             FROM model_call_reservations
             WHERE role='LEAD' AND objective_id=?1
             ORDER BY created_at,reservation_id",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        model_call_records(rows)
    }

    pub async fn model_calls(&self, limit: usize) -> Result<Vec<ModelCallRecord>, Error> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query::query(
            "SELECT reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,
                    model,purpose,request_sha256,input_tokens,output_tokens,total_tokens,
                    cost_micros,usage_known,state,COALESCE(error,'') AS error,created_at,updated_at
             FROM model_call_reservations ORDER BY created_at DESC,reservation_id LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        model_call_records(rows)
    }

    async fn validate_model_call_scope(
        &self,
        transaction: &mut SqliteConnection,
        input: &NewModelCallAuthorization,
    ) -> Result<(), Error> {
        match input.role {
            ModelCallRole::Lead => {
                let row: Option<(String, String, String, String)> = sqlx::query_as::query_as(
                    "SELECT project_id,delegation_id,state,action_id FROM lead_requests WHERE id=?1",
                )
                .bind(&input.objective_id)
                .fetch_optional(&mut *transaction)
                .await?;
                let Some((project_id, delegation_id, state, action_id)) = row else {
                    return Err(Error::NotFound);
                };
                if project_id != input.project_id || scope_id_mismatch(&delegation_id, input) {
                    return Err(Error::Forbidden(
                        "Lead model call is not bound to the request delegation".into(),
                    ));
                }
                if state != "ASSESSING" {
                    return Err(Error::Forbidden(
                        "Lead model call requires an assessing request".into(),
                    ));
                }
                let action: Option<(String, String)> =
                    sqlx::query_as::query_as("SELECT provider,model FROM actions WHERE id=?1")
                        .bind(&action_id)
                        .fetch_optional(&mut *transaction)
                        .await?;
                let Some((provider, model)) = action else {
                    return Err(Error::NotFound);
                };
                if provider != input.provider || model != input.model {
                    return Err(Error::Forbidden(
                        "Lead model call route differs from the Action".into(),
                    ));
                }
                let grant =
                    super::delegations::load_delegation(transaction, &delegation_id).await?;
                grant.validate(Utc::now())?;
                if grant.principal_id != input.principal_id
                    || grant.project_id != input.project_id
                    || grant.route.provider != input.provider
                    || grant.route.model != input.model
                {
                    return Err(Error::Forbidden(
                        "Lead model call differs from the active delegation".into(),
                    ));
                }
            }
            ModelCallRole::SemanticReviewer => {
                let row: Option<(String, String, String, String, String, String)> =
                    sqlx::query_as::query_as(
                        "SELECT ac.project_id,cd.implementer_id,
                                COALESCE(ca.provider,''),COALESCE(ca.model,''),r.campaign_id,c.status
                         FROM candidates cd
                         JOIN attempts a ON a.id=cd.attempt_id
                         JOIN runs r ON r.id=a.run_id
                         JOIN campaigns c ON c.id=r.campaign_id
                         JOIN actions ac ON ac.id=c.action_id
                         LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
                         WHERE cd.id=?1",
                    )
                    .bind(&input.objective_id)
                    .fetch_optional(&mut *transaction)
                    .await?;
                let Some((
                    project_id,
                    implementer_id,
                    provider,
                    model,
                    _campaign_id,
                    campaign_status,
                )) = row
                else {
                    return Err(Error::NotFound);
                };
                if project_id != input.project_id
                    || input.scope_id != input.objective_id
                    || campaign_status != "AUTHORIZED"
                {
                    return Err(Error::Forbidden(
                        "review model call is not bound to an authorized candidate campaign".into(),
                    ));
                }
                if input.principal_id == implementer_id {
                    return Err(Error::Forbidden(
                        "implementer cannot request an independent review".into(),
                    ));
                }
                if !provider.is_empty() && (provider != input.provider || model != input.model) {
                    return Err(Error::Forbidden(
                        "review model call route differs from the campaign authorization".into(),
                    ));
                }
            }
            ModelCallRole::Evaluation => {
                if input.scope_id != input.objective_id {
                    return Err(Error::Forbidden(
                        "evaluation model call scope must identify the evaluation".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn validate_model_call_authorization(input: &NewModelCallAuthorization) -> Result<(), Error> {
    for (value, max) in [
        (input.principal_id.as_str(), 128),
        (input.project_id.as_str(), 128),
        (input.objective_id.as_str(), 128),
        (input.scope_id.as_str(), 128),
        (input.provider.as_str(), 200),
        (input.model.as_str(), 200),
        (input.purpose.as_str(), 64),
    ] {
        if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
            return Err(Error::Invalid("model call identity"));
        }
        if value.chars().any(char::is_whitespace) {
            return Err(Error::Invalid("model call identity"));
        }
    }
    if !is_sha256(&input.request_sha256) {
        return Err(Error::Invalid("model call request digest"));
    }
    if input.estimated_input_tokens == 0
        || input.estimated_input_tokens > 1_000_000
        || input.requested_output_tokens == 0
        || input.requested_output_tokens > MAX_CALL_OUTPUT_TOKENS
        || input.estimated_cost_micros == 0
        || input.estimated_cost_micros > 10_000_000
    {
        return Err(Error::Invalid("model call budget request"));
    }
    if input.policy.max_input_tokens == 0
        || input.policy.max_output_tokens == 0
        || input.policy.max_cost_micros == 0
        || input.policy.max_input_tokens > 10_000_000
        || input.policy.max_output_tokens > 1_000_000
        || input.policy.max_cost_micros > 100_000_000
    {
        return Err(Error::Invalid("model call policy"));
    }
    Ok(())
}

fn scope_id_mismatch(expected: &str, input: &NewModelCallAuthorization) -> bool {
    expected != input.scope_id
}

async fn expire_reserved_model_calls(
    connection: &mut SqliteConnection,
    now_ms: i64,
) -> Result<(), Error> {
    let expired: Vec<(String, String)> = sqlx::query_as::query_as(
        "SELECT reservation_id,objective_id FROM model_call_reservations
         WHERE state='RESERVED' AND expires_at_ms<=?1",
    )
    .bind(now_ms)
    .fetch_all(&mut *connection)
    .await?;
    for (reservation_id, objective_id) in expired {
        sqlx::query::query(
            "UPDATE model_call_reservations
             SET state='AMBIGUOUS',error='reservation expired before settlement',updated_at=?1
             WHERE reservation_id=?2 AND state='RESERVED'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(&reservation_id)
        .execute(&mut *connection)
        .await?;
        record_event(
            connection,
            &objective_id,
            "model.call_ambiguous",
            &format!("reservation={reservation_id} expired"),
        )
        .await?;
    }
    Ok(())
}

async fn load_model_call_identity(
    connection: &mut SqliteConnection,
    input: &NewModelCallAuthorization,
) -> Result<Option<ModelCallRecord>, Error> {
    let row: Option<IdentityRow> = sqlx::query_as::query_as(
        "SELECT reservation_id,role,state,input_tokens,output_tokens,total_tokens,cost_micros,
                    usage_known,provider,model,created_at,updated_at
             FROM model_call_reservations
             WHERE role=?1 AND objective_id=?2 AND request_sha256=?3
             ORDER BY created_at DESC LIMIT 1",
    )
    .bind(input.role.as_str())
    .bind(&input.objective_id)
    .bind(input.request_sha256.to_ascii_lowercase())
    .fetch_optional(&mut *connection)
    .await?;
    row.map(
        |(
            reservation_id,
            role,
            state,
            input_tokens,
            output_tokens,
            total_tokens,
            cost_micros,
            usage_known,
            provider,
            model,
            created_at,
            updated_at,
        )| {
            Ok(ModelCallRecord {
                reservation_id,
                role: ModelCallRole::parse(&role)
                    .ok_or(Error::Invalid("stored model call role"))?,
                principal_id: input.principal_id.clone(),
                project_id: input.project_id.clone(),
                objective_id: input.objective_id.clone(),
                scope_id: input.scope_id.clone(),
                provider,
                model,
                purpose: input.purpose.clone(),
                request_sha256: input.request_sha256.to_ascii_lowercase(),
                input_tokens: u64::try_from(input_tokens)
                    .map_err(|_| Error::Invalid("stored model call input"))?,
                output_tokens: u64::try_from(output_tokens)
                    .map_err(|_| Error::Invalid("stored model call output"))?,
                total_tokens: u64::try_from(total_tokens)
                    .map_err(|_| Error::Invalid("stored model call total"))?,
                cost_micros: u64::try_from(cost_micros)
                    .map_err(|_| Error::Invalid("stored model call cost"))?,
                usage_known: usage_known != 0,
                state,
                error: String::new(),
                created_at,
                updated_at,
            })
        },
    )
    .transpose()
}

async fn replay_model_call(
    connection: &mut SqliteConnection,
    record: &ModelCallRecord,
) -> Result<ModelCallResult, Error> {
    replay_model_call_by_id(connection, &record.reservation_id).await
}

async fn replay_model_call_by_id(
    connection: &mut SqliteConnection,
    reservation_id: &str,
) -> Result<ModelCallResult, Error> {
    let row: Option<ReplayRow> = sqlx::query_as::query_as(
        "SELECT role,provider,model,request_sha256,COALESCE(response_text,''),state,
                    input_tokens,output_tokens,total_tokens,cost_micros,usage_known
             FROM model_call_reservations WHERE reservation_id=?1",
    )
    .bind(reservation_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((
        role,
        provider,
        model,
        request_sha256,
        content,
        state,
        input_tokens,
        output_tokens,
        total_tokens,
        cost_micros,
        usage_known,
    )) = row
    else {
        return Err(Error::NotFound);
    };
    if state != "SETTLED" {
        return Err(Error::Conflict("model call is not settled"));
    }
    Ok(ModelCallResult {
        reservation_id: reservation_id.to_owned(),
        role: ModelCallRole::parse(&role).ok_or(Error::Invalid("stored model call role"))?,
        provider,
        model,
        request_sha256,
        content,
        input_tokens: u64::try_from(input_tokens)
            .map_err(|_| Error::Invalid("stored model call input"))?,
        output_tokens: u64::try_from(output_tokens)
            .map_err(|_| Error::Invalid("stored model call output"))?,
        total_tokens: u64::try_from(total_tokens)
            .map_err(|_| Error::Invalid("stored model call total"))?,
        cost_micros: u64::try_from(cost_micros)
            .map_err(|_| Error::Invalid("stored model call cost"))?,
        usage_known: usage_known != 0,
    })
}

fn model_call_records(rows: Vec<sqlx_sqlite::SqliteRow>) -> Result<Vec<ModelCallRecord>, Error> {
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        let role: String = row.try_get("role")?;
        let input_tokens: i64 = row.try_get("input_tokens")?;
        let output_tokens: i64 = row.try_get("output_tokens")?;
        let total_tokens: i64 = row.try_get("total_tokens")?;
        let cost_micros: i64 = row.try_get("cost_micros")?;
        let usage_known: i64 = row.try_get("usage_known")?;
        records.push(ModelCallRecord {
            reservation_id: row.try_get("reservation_id")?,
            role: ModelCallRole::parse(&role).ok_or(Error::Invalid("stored model call role"))?,
            principal_id: row.try_get("principal_id")?,
            project_id: row.try_get("project_id")?,
            objective_id: row.try_get("objective_id")?,
            scope_id: row.try_get("scope_id")?,
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            purpose: row.try_get("purpose")?,
            request_sha256: row.try_get("request_sha256")?,
            input_tokens: u64::try_from(input_tokens)
                .map_err(|_| Error::Invalid("stored model call input"))?,
            output_tokens: u64::try_from(output_tokens)
                .map_err(|_| Error::Invalid("stored model call output"))?,
            total_tokens: u64::try_from(total_tokens)
                .map_err(|_| Error::Invalid("stored model call total"))?,
            cost_micros: u64::try_from(cost_micros)
                .map_err(|_| Error::Invalid("stored model call cost"))?,
            usage_known: usage_known != 0,
            state: row.try_get("state")?,
            error: row.try_get("error")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        });
    }
    Ok(records)
}

async fn model_call_role_usage(
    connection: &mut SqliteConnection,
    role: ModelCallRole,
) -> Result<(u64, u64, u64), Error> {
    let row: (i64, i64, i64) = sqlx::query_as::query_as(
        "SELECT COALESCE(SUM(input_tokens),0),COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cost_micros),0)
         FROM model_call_reservations WHERE role=?1 AND state='SETTLED'",
    )
    .bind(role.as_str())
    .fetch_one(&mut *connection)
    .await?;
    Ok((
        u64::try_from(row.0).map_err(|_| Error::Invalid("model usage"))?,
        u64::try_from(row.1).map_err(|_| Error::Invalid("model usage"))?,
        u64::try_from(row.2).map_err(|_| Error::Invalid("model usage"))?,
    ))
}

async fn model_call_role_reservations(
    connection: &mut SqliteConnection,
    role: ModelCallRole,
) -> Result<(u64, u64, u64), Error> {
    let row: (i64, i64, i64) = sqlx::query_as::query_as(
        "SELECT COALESCE(SUM(max_input_tokens),0),COALESCE(SUM(max_output_tokens),0),
                COALESCE(SUM(max_cost_micros),0)
         FROM model_call_reservations
         WHERE role=?1 AND state IN ('RESERVED','AMBIGUOUS','UNKNOWN')",
    )
    .bind(role.as_str())
    .fetch_one(&mut *connection)
    .await?;
    Ok((
        u64::try_from(row.0).map_err(|_| Error::Invalid("model reservation"))?,
        u64::try_from(row.1).map_err(|_| Error::Invalid("model reservation"))?,
        u64::try_from(row.2).map_err(|_| Error::Invalid("model reservation"))?,
    ))
}

async fn model_call_usage(
    connection: &mut SqliteConnection,
    scope_id: &str,
) -> Result<(u64, u64, u64), Error> {
    let row: (i64, i64, i64) = sqlx::query_as::query_as(
        "SELECT COALESCE(SUM(input_tokens),0),COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(cost_micros),0)
         FROM model_call_reservations
         WHERE scope_id=?1 AND state='SETTLED'",
    )
    .bind(scope_id)
    .fetch_one(&mut *connection)
    .await?;
    Ok((
        u64::try_from(row.0).map_err(|_| Error::Invalid("model usage"))?,
        u64::try_from(row.1).map_err(|_| Error::Invalid("model usage"))?,
        u64::try_from(row.2).map_err(|_| Error::Invalid("model usage"))?,
    ))
}

async fn model_call_reservations(
    connection: &mut SqliteConnection,
    scope_id: &str,
) -> Result<(u64, u64, u64), Error> {
    let row: (i64, i64, i64) = sqlx::query_as::query_as(
        "SELECT COALESCE(SUM(max_input_tokens),0),COALESCE(SUM(max_output_tokens),0),
                COALESCE(SUM(max_cost_micros),0)
         FROM model_call_reservations
         WHERE scope_id=?1 AND state IN ('RESERVED','AMBIGUOUS','UNKNOWN')",
    )
    .bind(scope_id)
    .fetch_one(&mut *connection)
    .await?;
    Ok((
        u64::try_from(row.0).map_err(|_| Error::Invalid("model reservation"))?,
        u64::try_from(row.1).map_err(|_| Error::Invalid("model reservation"))?,
        u64::try_from(row.2).map_err(|_| Error::Invalid("model reservation"))?,
    ))
}
