//! Core-owned model-call reservation ledger.
//!
//! Every non-Worker model call (Lead assessment, semantic review, CAP-014
//! evaluation) reserves a bounded token/cost envelope in Core before any
//! upstream effect. Settlement records measured usage. An interrupted call is
//! marked ambiguous and cannot be silently replayed; an operator must resolve
//! it explicitly.

use super::*;

use crate::execution_policy::{
    InferenceRole, InferenceSettlement, ReserveInferenceInput, UsdMicros,
};
use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

/// Deterministic settlement evidence for a model call. The same stored usage
/// always produces the same digest, so an identical settlement replays.
fn model_call_settlement_digest(
    reservation_id: &str,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    cost_micros: u64,
    usage_known: bool,
) -> String {
    sha256_hex(
        format!(
            "model-call-settlement:v1:{reservation_id}:{input_tokens}:{output_tokens}:{total_tokens}:{cost_micros}:{usage_known}"
        )
        .as_bytes(),
    )
}

fn model_call_ledger_role(role: ModelCallRole) -> InferenceRole {
    match role {
        ModelCallRole::Lead => InferenceRole::Lead,
        ModelCallRole::SemanticReviewer => InferenceRole::SemanticReviewer,
        ModelCallRole::Evaluation => InferenceRole::Evaluation,
    }
}

/// Resolve the ledger requirement only after current Owner funding policy has
/// admitted the provider route. The ledger continues to retain V1/V2 billing
/// protections for a future explicitly authorized billable mode.
async fn model_call_ledger_requirement(
    connection: &mut SqliteConnection,
    role: ModelCallRole,
    objective_id: &str,
) -> Result<Option<String>, Error> {
    match role {
        ModelCallRole::Lead => {
            let _ = (connection, objective_id);
            Err(Error::Unavailable(
                "Lead assessment has no verified request-level subscription funding binding".into(),
            ))
        }
        ModelCallRole::SemanticReviewer => {
            let campaign: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT r.campaign_id FROM candidates cd
                 JOIN attempts a ON a.id=cd.attempt_id
                 JOIN runs r ON r.id=a.run_id
                 WHERE cd.id=?1",
            )
            .bind(objective_id)
            .fetch_optional(&mut *connection)
            .await?;
            let Some((campaign_id,)) = campaign else {
                return Err(Error::NotFound);
            };
            campaign_requirement_to_envelope(connection, &campaign_id).await
        }
        ModelCallRole::Evaluation => {
            let campaign: Option<(String,)> = sqlx::query_as::query_as(
                "SELECT r.campaign_id FROM candidates cd
                 JOIN attempts a ON a.id=cd.attempt_id
                 JOIN runs r ON r.id=a.run_id
                 WHERE cd.id=?1",
            )
            .bind(objective_id)
            .fetch_optional(&mut *connection)
            .await?;
            if let Some((campaign_id,)) = campaign {
                return campaign_requirement_to_envelope(connection, &campaign_id).await;
            }
            Err(Error::Unavailable(
                "evaluation inference has no campaign-bound, verified subscription funding".into(),
            ))
        }
    }
}

async fn campaign_requirement_to_envelope(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Option<String>, Error> {
    super::inference_ledger::require_enabled_subscription_funding_in_tx(connection, campaign_id)
        .await?;
    match super::inference_ledger::campaign_requirement_in_tx(connection, campaign_id).await? {
        crate::inference_ledger::CampaignInferenceRequirement::LegacyV1 => Ok(None),
        crate::inference_ledger::CampaignInferenceRequirement::EnvelopeBound { envelope_id } => {
            Ok(Some(envelope_id))
        }
    }
}

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

/// Result of one dispatch-intent acquisition. `acquired=false` means an earlier
/// send already owns this reservation; the caller must not send again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallDispatch {
    pub reservation_id: String,
    pub dispatched_at_ms: i64,
    pub provider_request_id: Option<String>,
    pub acquired: bool,
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
    pub max_cost_micros: u64,
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
    Option<String>,
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
type DispatchReservationRow = (String, String, Option<i64>, Option<String>, String, String);
const MAX_CALL_OUTPUT_TOKENS: u64 = 8192;
const RESERVATION_LIFETIME_MS: i64 = 120_000;

impl Database {
    pub async fn authorize_model_call(
        &self,
        input: &NewModelCallAuthorization,
    ) -> Result<ModelCallDecision, Error> {
        validate_model_call_authorization(input)?;
        let _write = self.write_lock.lock().await;
        // Withdrawal uses the same immediate write transaction. Whichever
        // commits first defines the outcome: a committed withdrawal makes this
        // authorization fail before any reservation or upstream effect.
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
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
        // A campaign-bound call inherits the request envelope. The envelope is
        // the single monetary authority for this operation; the legacy scope
        // and role columns below remain evidence projections.
        let ledger_envelope_id =
            model_call_ledger_requirement(&mut transaction, input.role, &input.objective_id)
                .await?;
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
        // Reserve the whole remaining input envelope rather than a tokenizer
        // estimate. For a ledger-bound call the monetary bound comes only from
        // a trusted frozen adapter quote; the caller estimate is never used for
        // authorization. Legacy V1 cost columns are not intersected again.
        let pricing_quote = if ledger_envelope_id.is_some() {
            Some(
                super::inference_ledger::latest_pricing_quote_in_tx(
                    &mut transaction,
                    &input.provider,
                    &input.model,
                )
                .await
                .map_err(|_| {
                    Error::Forbidden(
                        "invoice-bound model call has no enforceable pricing quote".into(),
                    )
                })?,
            )
        } else {
            None
        };
        let (max_input, max_output) = if let Some(quote) = &pricing_quote {
            // V2 uses a per-request technical token bound, never cumulative
            // lifetime role/scope allowances. The bound must fit the frozen
            // quote envelope or the request is refused before any effect.
            if input.estimated_input_tokens > input.policy.max_input_tokens
                || input.requested_output_tokens > input.policy.max_output_tokens
            {
                return Err(Error::Forbidden(
                    "model call request exceeds its technical policy".into(),
                ));
            }
            if input.estimated_input_tokens > quote.input_tokens
                || input.requested_output_tokens > quote.output_tokens
            {
                return Err(Error::Forbidden(
                    "model call request exceeds its frozen token quote".into(),
                ));
            }
            (input.estimated_input_tokens, input.requested_output_tokens)
        } else {
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
            (max_input, max_output)
        };
        let max_total = max_input.saturating_add(max_output);
        let max_cost = match &pricing_quote {
            Some(quote) => quote.upper_bound_micros,
            None => UsdMicros::new(remaining_cost)
                .map_err(|_| Error::Invalid("model call cost reservation"))?,
        };
        let request_sha256 = input.request_sha256.to_ascii_lowercase();
        let reservation_id = Uuid::new_v4().to_string();
        let expires_at_ms = now_ms.saturating_add(RESERVATION_LIFETIME_MS);
        let now_text = now.to_rfc3339();
        let ledger_reservation_id =
            if let (Some(envelope_id), Some(quote)) = (&ledger_envelope_id, &pricing_quote) {
                let (authorization_revision, scope_sha256) =
                    super::inference_ledger::envelope_authorization_context_in_tx(
                        &mut transaction,
                        envelope_id,
                    )
                    .await?
                    .ok_or(Error::Conflict(
                        "inference envelope has no durable authorization",
                    ))?;
                let ledger_input = ReserveInferenceInput {
                    envelope_id: envelope_id.clone(),
                    operation_id: reservation_id.clone(),
                    // The legacy reservation id is the operation identity. A
                    // released attempt legitimately creates a new operation; an
                    // identical replay keeps this key and returns the durable row.
                    idempotency_key: format!("model_call:{reservation_id}"),
                    request_sha256: request_sha256.clone(),
                    role: model_call_ledger_role(input.role),
                    principal_id: input.principal_id.clone(),
                    parent_operation_id: None,
                    provider: input.provider.clone(),
                    model: input.model.clone(),
                    pricing_revision: quote.quote_id.clone(),
                    pricing_snapshot_json: quote.snapshot_json.clone(),
                    upper_bound_micros: quote.upper_bound_micros,
                    expires_at_ms,
                    authorization_revision,
                    scope_sha256,
                    worker_id: None,
                    generation: None,
                };
                let reserved = super::inference_ledger::reserve_inference_in_tx(
                    &mut transaction,
                    &ledger_input,
                    now_ms,
                )
                .await?;
                Some(reserved.reservation_id)
            } else {
                None
            };
        sqlx::query::query(
            "INSERT INTO model_call_reservations
             (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
              purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
              max_cost_micros,state,expires_at_ms,created_at,updated_at,ledger_reservation_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'RESERVED',?15,?16,?16,?17)",
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
        .bind(
            i64::try_from(max_cost.get())
                .map_err(|_| Error::Invalid("model call cost reservation"))?,
        )
        .bind(expires_at_ms)
        .bind(&now_text)
        .bind(&ledger_reservation_id)
        .execute(&mut *transaction)
        .await?;
        record_event(
            &mut transaction,
            &input.objective_id,
            "model.call_authorized",
            &format!(
                "role={} reservation={reservation_id} provider={} model={} input={max_input} output={max_output} cost={}",
                input.role.as_str(),
                input.provider,
                input.model,
                max_cost.get()
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
            max_cost_micros: max_cost.get(),
            request_sha256,
            expires_at_ms,
        }))
    }

    /// Acquire durable dispatch intent before the model HTTP request is sent.
    /// Only the first acquisition returns `acquired=true`; a replay of the
    /// intent returns `acquired=false` and must not authorize another send.
    pub async fn acquire_model_call_dispatch(
        &self,
        reservation_id: &str,
        provider_request_id: &str,
    ) -> Result<ModelCallDispatch, Error> {
        if reservation_id.trim().is_empty()
            || provider_request_id.trim().is_empty()
            || provider_request_id.len() > 256
        {
            return Err(Error::Invalid("model call dispatch identity"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now_ms = Utc::now().timestamp_millis();
        let row: Option<DispatchReservationRow> = sqlx::query_as::query_as(
            "SELECT role,state,dispatched_at_ms,ledger_reservation_id,request_sha256,objective_id
                 FROM model_call_reservations WHERE reservation_id=?1",
        )
        .bind(reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((
            role,
            state,
            dispatched_at_ms,
            ledger_reservation_id,
            _request_sha256,
            objective_id,
        )) = row
        else {
            return Err(Error::NotFound);
        };
        // Candidate-bound Core calls share the same campaign funding check as
        // Worker turns. Re-read it at dispatch so a funding proposal recorded
        // after reservation cannot be bypassed by a stale reservation.
        if let Some((campaign_id,)) = sqlx::query_as::query_as::<_, (String,)>(
            "SELECT r.campaign_id FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id WHERE cd.id=?1",
        )
        .bind(&objective_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            super::inference_ledger::require_enabled_subscription_funding_in_tx(
                &mut transaction,
                &campaign_id,
            )
            .await?;
        } else {
            let reason = match role.as_str() {
                "LEAD" => "Lead dispatch has no request-level subscription funding binding",
                "SEMANTIC_REVIEWER" => {
                    "reviewer dispatch has no campaign-bound subscription funding"
                }
                "EVALUATION" => "evaluation dispatch has no campaign-bound subscription funding",
                _ => "model dispatch role is invalid",
            };
            return Err(Error::Unavailable(reason.into()));
        }
        if dispatched_at_ms.is_some() {
            if let Some(ledger_id) = &ledger_reservation_id {
                let (ledger_dispatched,): (Option<i64>,) = sqlx::query_as::query_as(
                    "SELECT dispatched_at_ms FROM inference_reservations WHERE id=?1",
                )
                .bind(ledger_id)
                .fetch_one(&mut *transaction)
                .await?;
                if ledger_dispatched.is_none() {
                    return Err(Error::Conflict(
                        "model dispatch record is inconsistent with the inference ledger",
                    ));
                }
            }
            transaction.commit().await?;
            return Ok(ModelCallDispatch {
                reservation_id: reservation_id.to_owned(),
                dispatched_at_ms: dispatched_at_ms.unwrap_or(now_ms),
                provider_request_id: Some(provider_request_id.to_owned()),
                acquired: false,
            });
        }
        if state != "RESERVED" {
            return Err(Error::Conflict("model call is not reserved"));
        }
        if let Some(ledger_id) = &ledger_reservation_id {
            super::inference_ledger::acquire_inference_dispatch_in_tx(
                &mut transaction,
                &MarkInferenceDispatchedInput {
                    reservation_id: ledger_id.clone(),
                    worker_id: None,
                    generation: None,
                    provider_request_id: Some(provider_request_id.to_owned()),
                    attempt_id: None,
                    actor: "core:model_gateway".into(),
                },
                now_ms,
            )
            .await?;
        }
        let changed = sqlx::query::query(
            "UPDATE model_call_reservations
             SET dispatched_at_ms=?1,provider_request_id=?2,updated_at=?3
             WHERE reservation_id=?4 AND state='RESERVED' AND dispatched_at_ms IS NULL",
        )
        .bind(now_ms)
        .bind(provider_request_id)
        .bind(Utc::now().to_rfc3339())
        .bind(reservation_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(Error::Conflict("model call dispatch intent raced"));
        }

        transaction.commit().await?;
        Ok(ModelCallDispatch {
            reservation_id: reservation_id.to_owned(),
            dispatched_at_ms: now_ms,
            provider_request_id: Some(provider_request_id.to_owned()),
            acquired: true,
        })
    }

    pub async fn settle_model_call(
        &self,
        usage: &ModelCallUsage,
    ) -> Result<ModelCallResult, Error> {
        if usage.content.len() > 256 * 1024 {
            return Err(Error::Invalid("model call response is too large"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row: Option<SettleRow> = sqlx::query_as::query_as(
            "SELECT state,role,provider,model,request_sha256,max_input_tokens,
                    max_output_tokens,max_total_tokens,max_cost_micros,
                    COALESCE(response_text,''),COALESCE(error,''),ledger_reservation_id
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
            ledger_reservation_id,
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
                if let Some(ledger_id) = &ledger_reservation_id {
                    let digest = model_call_settlement_digest(
                        ledger_id,
                        result.input_tokens,
                        result.output_tokens,
                        result.total_tokens,
                        result.cost_micros,
                        result.usage_known,
                    );
                    super::inference_ledger::settle_inference_in_tx(
                        &mut transaction,
                        &InferenceSettlement {
                            reservation_id: ledger_id.clone(),
                            settled_micros: UsdMicros::new(result.cost_micros)
                                .map_err(|_| Error::Invalid("stored model call cost"))?,
                            usage_evidence_sha256: digest,
                        },
                        Utc::now().timestamp_millis(),
                    )
                    .await?;
                }
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
        if let Some(ledger_id) = &ledger_reservation_id {
            let ledger_now_ms = Utc::now().timestamp_millis();
            if usage.usage_known {
                let digest = model_call_settlement_digest(
                    ledger_id,
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.total_tokens,
                    usage.cost_micros,
                    usage.usage_known,
                );
                super::inference_ledger::settle_inference_in_tx(
                    &mut transaction,
                    &InferenceSettlement {
                        reservation_id: ledger_id.clone(),
                        settled_micros: UsdMicros::new(usage.cost_micros)
                            .map_err(|_| Error::Invalid("model call cost usage"))?,
                        usage_evidence_sha256: digest,
                    },
                    ledger_now_ms,
                )
                .await?;
            } else {
                super::inference_ledger::mark_inference_ambiguous_in_tx(
                    &mut transaction,
                    ledger_id,
                    "core:model_call",
                    &format!("model_call:{}:ambiguous", usage.reservation_id),
                    &request_sha256,
                    ledger_now_ms,
                )
                .await?;
            }
        }
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

    /// Authenticated late settlement for a model call whose legacy row became
    /// ambiguous/unknown or whose envelope was revoked after dispatch. This is
    /// financial reconciliation only; it never grants permission to continue
    /// work.
    pub async fn settle_late_model_call(
        &self,
        usage: &ModelCallUsage,
        actor: &str,
        evidence_ref: &str,
    ) -> Result<ModelCallResult, Error> {
        if !usage.usage_known {
            return Err(Error::Invalid("late model settlement requires known usage"));
        }
        if actor.trim().is_empty() || evidence_ref.trim().is_empty() {
            return Err(Error::Invalid("late model settlement evidence"));
        }
        if usage.content.len() > 256 * 1024 {
            return Err(Error::Invalid("model call response is too large"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row: Option<(String, String, String, String, String, Option<String>)> =
            sqlx::query_as::query_as(
                "SELECT state,role,provider,model,request_sha256,ledger_reservation_id
                 FROM model_call_reservations WHERE reservation_id=?1",
            )
            .bind(&usage.reservation_id)
            .fetch_optional(&mut *transaction)
            .await?;
        let Some((state, role, provider, model, request_sha256, ledger_reservation_id)) = row
        else {
            return Err(Error::NotFound);
        };
        if state == "SETTLED" {
            return Err(Error::Conflict("model call is already settled"));
        }
        if !matches!(state.as_str(), "AMBIGUOUS" | "UNKNOWN") {
            return Err(Error::Conflict("model call is not ambiguous or unknown"));
        }
        let role = ModelCallRole::parse(&role).ok_or(Error::Invalid("stored model call role"))?;
        let input = i64::try_from(usage.input_tokens)
            .map_err(|_| Error::Invalid("model call input usage"))?;
        let output = i64::try_from(usage.output_tokens)
            .map_err(|_| Error::Invalid("model call output usage"))?;
        let total = i64::try_from(usage.total_tokens)
            .map_err(|_| Error::Invalid("model call total usage"))?;
        let cost = i64::try_from(usage.cost_micros)
            .map_err(|_| Error::Invalid("model call cost usage"))?;
        let now = Utc::now();
        sqlx::query::query(
            "UPDATE model_call_reservations
             SET state='SETTLED',input_tokens=?1,output_tokens=?2,total_tokens=?3,cost_micros=?4,
                 usage_known=1,response_text=?5,error=NULL,updated_at=?6
             WHERE reservation_id=?7 AND state IN ('AMBIGUOUS','UNKNOWN')",
        )
        .bind(input)
        .bind(output)
        .bind(total)
        .bind(cost)
        .bind(&usage.content)
        .bind(now.to_rfc3339())
        .bind(&usage.reservation_id)
        .execute(&mut *transaction)
        .await?;
        if let Some(ledger_id) = &ledger_reservation_id {
            let digest = model_call_settlement_digest(
                ledger_id,
                usage.input_tokens,
                usage.output_tokens,
                usage.total_tokens,
                usage.cost_micros,
                usage.usage_known,
            );
            super::inference_ledger::settle_or_reconcile_in_tx(
                &mut transaction,
                ledger_id,
                UsdMicros::new(usage.cost_micros)
                    .map_err(|_| Error::Invalid("model call cost usage"))?,
                &digest,
                actor,
                evidence_ref,
                now.timestamp_millis(),
            )
            .await?;
        }
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
            "model.call_resolved",
            &format!(
                "reservation={} actor={actor} late_settlement cost={}",
                usage.reservation_id, usage.cost_micros
            ),
        )
        .await?;
        transaction.commit().await?;
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
            usage_known: true,
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
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let state = if ambiguous { "AMBIGUOUS" } else { "RELEASED" };
        let now = Utc::now().to_rfc3339();
        let objective_id: Option<(String, Option<String>, String, Option<i64>)> =
            sqlx::query_as::query_as(
                "SELECT objective_id,ledger_reservation_id,request_sha256,dispatched_at_ms
                 FROM model_call_reservations WHERE reservation_id=?1 AND state='RESERVED'",
            )
            .bind(reservation_id)
            .fetch_optional(&mut *transaction)
            .await?;
        let Some((objective_id, ledger_reservation_id, request_sha256, dispatched_at_ms)) =
            objective_id
        else {
            return Err(Error::Conflict("model call is not reserved"));
        };
        if !ambiguous && dispatched_at_ms.is_some() {
            return Err(Error::Conflict(
                "a dispatched model call cannot be released; reconciliation is required",
            ));
        }
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
        if let Some(ledger_id) = &ledger_reservation_id {
            let ledger_now_ms = Utc::now().timestamp_millis();
            if ambiguous {
                super::inference_ledger::mark_inference_ambiguous_in_tx(
                    &mut transaction,
                    ledger_id,
                    "core:model_call",
                    &format!("model_call:{reservation_id}:release"),
                    &request_sha256,
                    ledger_now_ms,
                )
                .await?;
            } else {
                super::inference_ledger::release_inference_in_tx(
                    &mut transaction,
                    ledger_id,
                    "core:model_call",
                    &format!("model_call:{reservation_id}:release"),
                    ledger_now_ms,
                )
                .await?;
            }
        }
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
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let objective_id: Option<(String, Option<String>)> = sqlx::query_as::query_as(
            "SELECT objective_id,ledger_reservation_id FROM model_call_reservations
             WHERE reservation_id=?1 AND state IN ('AMBIGUOUS','UNKNOWN')",
        )
        .bind(reservation_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((objective_id, ledger_reservation_id)) = objective_id else {
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
        if let Some(ledger_id) = &ledger_reservation_id {
            let evidence_sha256 = sha256_hex(
                format!("model-call-resolution:v1:{reservation_id}:{actor}:{reason}").as_bytes(),
            );
            super::inference_ledger::release_or_reconcile_in_tx(
                &mut transaction,
                ledger_id,
                actor,
                &format!("model_call:{reservation_id}:resolve"),
                &evidence_sha256,
                Utc::now().timestamp_millis(),
            )
            .await?;
        }
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
                    cost_micros,max_cost_micros,usage_known,state,COALESCE(error,'') AS error,
                    created_at,updated_at
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
                    cost_micros,max_cost_micros,usage_known,state,COALESCE(error,'') AS error,
                    created_at,updated_at
             FROM model_call_reservations
             WHERE role='LEAD' AND objective_id=?1
             ORDER BY created_at,reservation_id",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        model_call_records(rows)
    }

    /// Every unresolved reservation regardless of age. Unlike the bounded
    /// recent-call listing, this projection must not hide an old ambiguous
    /// reservation that still holds budget.
    /// Deterministically expire reservations whose deadline passed. The
    /// transition and audit event are committed before callers read the full
    /// unresolved ledger, so an expired-but-unswept reservation cannot hide.
    pub async fn expire_model_calls(&self, now_ms: i64) -> Result<(), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        expire_reserved_model_calls(&mut transaction, now_ms).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn unresolved_model_calls(&self) -> Result<Vec<ModelCallRecord>, Error> {
        self.expire_model_calls(Utc::now().timestamp_millis())
            .await?;
        let rows = sqlx::query::query(
            "SELECT reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,
                    model,purpose,request_sha256,input_tokens,output_tokens,total_tokens,
                    cost_micros,max_cost_micros,usage_known,state,COALESCE(error,'') AS error,
                    created_at,updated_at
             FROM model_call_reservations
             WHERE state IN ('AMBIGUOUS','UNKNOWN')
             ORDER BY updated_at DESC,reservation_id",
        )
        .fetch_all(&self.pool)
        .await?;
        model_call_records(rows)
    }

    /// Count and reservation ceiling for unresolved model calls whose usage is
    /// unknown. The ceiling comes from `max_cost_micros`, not the observed
    /// `cost_micros` column, because an expired reservation may never have
    /// settled any observed cost.
    pub async fn unresolved_model_call_budget(&self) -> Result<(u64, u64), Error> {
        self.expire_model_calls(Utc::now().timestamp_millis())
            .await?;
        let row: (i64, i64) = sqlx::query_as::query_as(
            "SELECT COUNT(*),COALESCE(SUM(max_cost_micros),0)
             FROM model_call_reservations
             WHERE state IN ('AMBIGUOUS','UNKNOWN') AND usage_known=0",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((
            u64::try_from(row.0).map_err(|_| Error::Invalid("model call count"))?,
            u64::try_from(row.1).map_err(|_| Error::Invalid("model reservation"))?,
        ))
    }

    pub async fn model_calls(&self, limit: usize) -> Result<Vec<ModelCallRecord>, Error> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query::query(
            "SELECT reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,
                    model,purpose,request_sha256,input_tokens,output_tokens,total_tokens,
                    cost_micros,max_cost_micros,usage_known,state,COALESCE(error,'') AS error,
                    created_at,updated_at
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
                if super::lead::withdrawal_in_tx(transaction, &input.objective_id).await? {
                    return Err(Error::Withdrawn);
                }
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
                // An intake request freezes an execution-profile revision whose
                // Lead route may differ from the Implementer route used by the
                // Action and the delegation. The old Action/delegation route
                // equality check made such profiles executable but unusable.
                let frozen_lead_route =
                    match super::intake::load_execution_context(transaction, &input.objective_id)
                        .await?
                    {
                        Some(context) => Some(
                            // The context is immutable; loading the exact
                            // revision here also rechecks its stored hash.
                            super::intake::load_frozen_profile_content(transaction, &context)
                                .await?,
                        ),
                        None => None,
                    };
                if let Some(content) = &frozen_lead_route {
                    let route = &content.roles.lead;
                    if route.provider != input.provider || route.model != input.model {
                        return Err(Error::Forbidden(
                            "Lead model call differs from the frozen Lead route".into(),
                        ));
                    }
                } else {
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
                }
                let delegation_version: i64 = sqlx::query_scalar::query_scalar(
                    "SELECT schema_version FROM standing_delegations WHERE id=?1",
                )
                .bind(&delegation_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(Error::NotFound)?;
                if delegation_version == 2 {
                    let grant =
                        super::delegations::load_delegation_v2(transaction, &delegation_id).await?;
                    grant.validate(Utc::now())?;
                    if grant.principal_id != input.principal_id
                        || grant.project_id != input.project_id
                    {
                        return Err(Error::Forbidden(
                            "Lead model call differs from the active V2 delegation".into(),
                        ));
                    }
                    if frozen_lead_route.is_none()
                        && (grant.route.provider != input.provider
                            || grant.route.model != input.model)
                    {
                        return Err(Error::Forbidden(
                            "Lead model call differs from the active V2 delegation".into(),
                        ));
                    }
                } else {
                    let grant =
                        super::delegations::load_delegation(transaction, &delegation_id).await?;
                    grant.validate(Utc::now())?;
                    if grant.principal_id != input.principal_id
                        || grant.project_id != input.project_id
                    {
                        return Err(Error::Forbidden(
                            "Lead model call differs from the active delegation".into(),
                        ));
                    }
                    if frozen_lead_route.is_none()
                        && (grant.route.provider != input.provider
                            || grant.route.model != input.model)
                    {
                        return Err(Error::Forbidden(
                            "Lead model call differs from the active delegation".into(),
                        ));
                    }
                }
            }
            ModelCallRole::SemanticReviewer => {
                let row: Option<(String, String, String, Option<String>, String)> =
                    sqlx::query_as::query_as(
                        "SELECT ac.project_id,cd.implementer_id,c.status,e.profile_revision_id,
                                COALESCE(ca.provider,'') || '|' || COALESCE(ca.model,'')
                         FROM candidates cd
                         JOIN attempts a ON a.id=cd.attempt_id
                         JOIN runs r ON r.id=a.run_id
                         JOIN campaigns c ON c.id=r.campaign_id
                         JOIN actions ac ON ac.id=c.action_id
                         LEFT JOIN campaign_authorizations ca ON ca.campaign_id=c.id
                         LEFT JOIN lead_start_evidence e ON e.campaign_id=c.id
                         WHERE cd.id=?1",
                    )
                    .bind(&input.objective_id)
                    .fetch_optional(&mut *transaction)
                    .await?;
                let Some((
                    project_id,
                    implementer_id,
                    campaign_status,
                    profile_revision_id,
                    legacy_route,
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
                // A Campaign started from a frozen Execution Profile must use
                // the exact semanticReviewer route declared by that immutable
                // revision. The Campaign authorization only carries the
                // implementer route and is a legacy fallback, never a
                // substitute for a frozen profile.
                let route = match profile_revision_id {
                    Some(revision_id) => {
                        let row: Option<(String,)> = sqlx::query_as::query_as(
                            "SELECT content_json FROM execution_profile_revisions
                             WHERE revision_id=?1",
                        )
                        .bind(&revision_id)
                        .fetch_optional(&mut *transaction)
                        .await?;
                        let Some((content_json,)) = row else {
                            return Err(Error::Forbidden(
                                "frozen execution profile revision is missing".into(),
                            ));
                        };
                        let content: crate::execution_profile::ExecutionProfileContent =
                            serde_json::from_str(&content_json).map_err(|_| {
                                Error::Conflict("stored execution profile content is invalid")
                            })?;
                        (
                            content.roles.semantic_reviewer.provider,
                            content.roles.semantic_reviewer.model,
                        )
                    }
                    None => {
                        let (provider, model) = legacy_route.split_once('|').ok_or(
                            Error::Forbidden("review model call has no frozen route".into()),
                        )?;
                        (provider.to_owned(), model.to_owned())
                    }
                };
                if !route.0.is_empty() && (route.0 != input.provider || route.1 != input.model) {
                    return Err(Error::Forbidden(
                        "review model call route differs from the frozen reviewer route".into(),
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
    let expired: Vec<(String, String, String, Option<String>)> = sqlx::query_as::query_as(
        "SELECT reservation_id,objective_id,request_sha256,ledger_reservation_id
         FROM model_call_reservations
         WHERE state='RESERVED' AND expires_at_ms<=?1",
    )
    .bind(now_ms)
    .fetch_all(&mut *connection)
    .await?;
    for (reservation_id, objective_id, request_sha256, ledger_reservation_id) in expired {
        sqlx::query::query(
            "UPDATE model_call_reservations
             SET state='AMBIGUOUS',error='reservation expired before settlement',updated_at=?1
             WHERE reservation_id=?2 AND state='RESERVED'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(&reservation_id)
        .execute(&mut *connection)
        .await?;
        if let Some(ledger_id) = &ledger_reservation_id {
            super::inference_ledger::mark_inference_ambiguous_in_tx(
                connection,
                ledger_id,
                "core:expiry",
                &format!("model_call:{reservation_id}:expiry"),
                &request_sha256,
                now_ms,
            )
            .await?;
        }
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
                    max_cost_micros,usage_known,provider,model,created_at,updated_at
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
            max_cost_micros,
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
                max_cost_micros: u64::try_from(max_cost_micros)
                    .map_err(|_| Error::Invalid("stored model call reservation"))?,
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
        let max_cost_micros: i64 = row.try_get("max_cost_micros")?;
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
            max_cost_micros: u64::try_from(max_cost_micros)
                .map_err(|_| Error::Invalid("stored model call reservation"))?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference_ledger::NewInferenceEnvelope;

    fn evaluation_input(scope: &str) -> NewModelCallAuthorization {
        NewModelCallAuthorization {
            role: ModelCallRole::Evaluation,
            principal_id: "evaluator".into(),
            project_id: "project".into(),
            objective_id: scope.into(),
            scope_id: scope.into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            purpose: "evaluate".into(),
            request_sha256: "c".repeat(64),
            estimated_input_tokens: 100,
            requested_output_tokens: 50,
            estimated_cost_micros: 500,
            policy: ModelCallPolicy {
                max_input_tokens: 1_000,
                max_output_tokens: 1_000,
                max_cost_micros: 1_000,
            },
            global_policy: None,
        }
    }

    async fn project_db(prefix: &str) -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()));
        let db = Database::open(&path).await.unwrap();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture','base',?1,'')",
        )
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        (path, db)
    }

    async fn envelope_for(db: &Database, request_id: &str) -> String {
        db.create_inference_envelope(&NewInferenceEnvelope {
            request_id: request_id.into(),
            project_id: "project".into(),
            authorized_micros: UsdMicros::new(5_000).unwrap(),
            actor: "operator".into(),
            scope_sha256: "a".repeat(64),
            evidence_sha256: "b".repeat(64),
            idempotency_key: format!("envelope-{request_id}"),
        })
        .await
        .unwrap()
        .envelope_id
    }

    async fn quote_with(db: &Database, input_tokens: u64, output_tokens: u64) {
        sqlx::query::query(
            "INSERT INTO provider_settings
             (provider,kind,model,base_url,auth_mode,credential_ciphertext,credential_nonce,
              credential_fingerprint,enabled,is_default,max_turns,max_input_tokens,
              max_output_tokens,max_cost_micros,input_micros_per_million,
              output_micros_per_million,revision,updated_at)
             VALUES ('opencode-go','opencode','deepseek-v4.1-flash','http://127.0.0.1:9',
                     'api_key','cipher','nonce','fingerprint',1,0,1,?1,?2,?3,1000000,0,1,?4)
             ON CONFLICT(provider) DO UPDATE SET
               max_input_tokens=excluded.max_input_tokens,
               max_output_tokens=excluded.max_output_tokens,
               max_cost_micros=excluded.max_cost_micros,
               input_micros_per_million=excluded.input_micros_per_million,
               output_micros_per_million=excluded.output_micros_per_million,
               revision=excluded.revision,updated_at=excluded.updated_at",
        )
        .bind(i64::try_from(input_tokens).unwrap())
        .bind(i64::try_from(output_tokens).unwrap())
        .bind(i64::try_from(input_tokens + output_tokens).unwrap())
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        db.create_inference_pricing_quote(&crate::inference_ledger::NewInferencePricingQuote {
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            revision: 1,
            evidence_sha256: "f".repeat(64),
            actor: "operator".into(),
        })
        .await
        .unwrap();
    }

    async fn quote(db: &Database) {
        quote_with(db, 250, 250).await;
    }

    #[tokio::test]
    async fn model_call_cannot_reach_dispatch_without_subscription_funding() {
        let (path, db) = project_db("factory-model-call-dispatch").await;
        quote(&db).await;
        let scope = "evaluation-dispatch";
        envelope_for(&db, scope).await;
        assert!(matches!(
            db.authorize_model_call(&evaluation_input(scope)).await,
            Err(Error::Unavailable(_))
        ));
        let (reservations,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM model_call_reservations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(reservations, 0, "no dispatch reservation may be created");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn existing_monetary_envelope_does_not_substitute_for_subscription_evidence() {
        let (path, db) = project_db("factory-model-call-dispatch-revoke").await;
        quote(&db).await;
        let scope = "evaluation-revoked";
        envelope_for(&db, scope).await;
        assert!(matches!(
            db.authorize_model_call(&evaluation_input(scope)).await,
            Err(Error::Unavailable(_))
        ));
        let (reservations,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM model_call_reservations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(reservations, 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_dispatch_refuses_missing_subscription_funding_after_reservation() {
        let (path, db) = project_db("factory-model-call-subscription-fence").await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES ('action-subscription','project','objective','opencode','model',
                     'R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES ('campaign-subscription','action-subscription','AUTHORIZED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO runs VALUES ('run-subscription','campaign-subscription','QUEUED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at)
             VALUES ('attempt-subscription','run-subscription','ACCEPTED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO candidates(id,attempt_id,action_spec_sha256,commit_sha,tree_sha,
                                    implementer_id,created_at)
             VALUES ('candidate-subscription','attempt-subscription','hash','commit','tree',
                     'worker',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        let request = evaluation_input("candidate-subscription");
        sqlx::query::query(
            "INSERT INTO model_call_reservations
             (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
              purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
              max_cost_micros,state,expires_at_ms,created_at,updated_at)
             VALUES ('reservation-subscription','EVALUATION','evaluator','project',?1,?1,
                     'opencode-go','deepseek-v4.1-flash','evaluate',?2,100,100,200,1000,
                     'RESERVED',?3,?4,?4)",
        )
        .bind(&request.objective_id)
        .bind(&request.request_sha256)
        .bind(Utc::now().timestamp_millis() + 60_000)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();

        let error = db
            .acquire_model_call_dispatch("reservation-subscription", "provider-request")
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Unavailable(_)),
            "Core dispatch must refuse missing funding before writing dispatch intent: {error:?}"
        );
        let dispatched: (Option<i64>,) = sqlx::query_as::query_as(
            "SELECT dispatched_at_ms FROM model_call_reservations
             WHERE reservation_id='reservation-subscription'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert!(
            dispatched.0.is_none(),
            "refusal must precede dispatch intent"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn lead_model_call_refuses_without_request_level_subscription_evidence() {
        let (path, db) = project_db("factory-lead-subscription-funding").await;
        let mut connection = db.pool.acquire().await.unwrap();
        let error = model_call_ledger_requirement(
            &mut connection,
            ModelCallRole::Lead,
            "lead-request-without-subscription-binding",
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, Error::Unavailable(_)),
            "Lead inference must not use a monetary request envelope under subscription-only policy: {error:?}"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_campaign_model_admission_refuses_missing_subscription_binding() {
        let (path, db) = project_db("factory-core-subscription-funding-missing").await;
        let mut connection = db.pool.acquire().await.unwrap();
        let error = campaign_requirement_to_envelope(&mut connection, "campaign-without-funding")
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Unavailable(_)),
            "Core campaign inference must refuse before reservation without subscription evidence: {error:?}"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn v2_monetary_quote_does_not_bypass_subscription_only_policy() {
        let (path, db) = project_db("factory-model-call-token-quote").await;
        quote_with(&db, 50, 25).await;
        let scope = "evaluation-token-bound";
        envelope_for(&db, scope).await;
        let mut input = evaluation_input(scope);
        input.estimated_input_tokens = 100;
        assert!(matches!(
            db.authorize_model_call(&input).await,
            Err(Error::Unavailable(_))
        ));
        let mut fitting = evaluation_input(scope);
        fitting.estimated_input_tokens = 40;
        fitting.requested_output_tokens = 20;
        assert!(matches!(
            db.authorize_model_call(&fitting).await,
            Err(Error::Unavailable(_))
        ));
        let (reservations,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM model_call_reservations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(reservations, 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn historical_usage_does_not_substitute_for_subscription_evidence() {
        let (path, db) = project_db("factory-model-call-history").await;
        quote(&db).await;
        let scope = "evaluation-history";
        envelope_for(&db, scope).await;
        for digest in ["a".repeat(64), "b".repeat(64)] {
            sqlx::query::query(
                "INSERT INTO model_call_reservations
                 (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
                  purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
                  max_cost_micros,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                  state,expires_at_ms,created_at,updated_at)
                 VALUES (?1,'EVALUATION','evaluator','project',?2,?2,'opencode-go',
                         'deepseek-v4.1-flash','evaluate',?3,1000000,1000000,2000000,1000000,
                         1000000,1000000,2000000,1000000,1,'SETTLED',1,'now','now')",
            )
            .bind(format!("history-{}", digest))
            .bind(scope)
            .bind(digest)
            .execute(&db.pool)
            .await
            .unwrap();
        }
        assert!(matches!(
            db.authorize_model_call(&evaluation_input(scope)).await,
            Err(Error::Unavailable(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn v2_without_envelope_fails_before_a_billable_model_call() {
        let (path, db) = project_db("factory-model-call-v2").await;
        db.set_inference_ledger_mode_v2("operator", &"e".repeat(64))
            .await
            .unwrap();
        assert!(matches!(
            db.authorize_model_call(&evaluation_input("evaluation-v2"))
                .await,
            Err(Error::Unavailable(_))
        ));
        let (count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM model_call_reservations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn v1_without_subscription_binding_is_not_legacy_dispatch_authority() {
        let (path, db) = project_db("factory-model-call-v1").await;
        assert!(matches!(
            db.authorize_model_call(&evaluation_input("evaluation-v1"))
                .await,
            Err(Error::Unavailable(_))
        ));
        let (count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM model_call_reservations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(count, 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn envelope_bound_model_call_cannot_use_billable_fallback() {
        let (path, db) = project_db("factory-model-call-ledger").await;
        let scope = "evaluation-ledger";
        envelope_for(&db, scope).await;
        assert!(matches!(
            db.authorize_model_call(&evaluation_input(scope)).await,
            Err(Error::Unavailable(_))
        ));
        let (reservations,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM model_call_reservations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(reservations, 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
