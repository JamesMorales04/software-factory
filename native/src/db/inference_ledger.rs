//! Durable shared inference-ledger transactions.
//!
//! Every money-changing operation uses `BEGIN IMMEDIATE` so independent SQLite
//! connections serialize admission, settlement, release, reconciliation and
//! revocation. Process-local mutexes are a convenience for in-process callers,
//! never the safety argument.
//!
//! Event rows and authorization revisions are append-only. Reservation
//! identity, request digest, pricing quote and original bound are immutable;
//! only the documented state transition may change a reservation.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

use crate::execution_policy::{
    EnvelopeState, InferenceLedgerError, InferenceReservation, InferenceRole, InferenceSettlement,
    ReservationState, ReserveInferenceInput, SpendingStatus, UsdMicros,
};
use crate::inference_ledger::{
    CampaignEnvelopeBindingInput, CampaignInferenceRequirement, EnvelopeSnapshot,
    InferenceEnvelopeRecord, InferenceLedgerMode, InferencePricingQuote,
    InferenceReservationRecord, MarkInferenceDispatchedInput, NewInferenceAuthorization,
    NewInferenceEnvelope, NewInferencePricingQuote, ReconcileInferenceInput, ReconciliationOutcome,
    ReservationAmounts, RevokeInferenceEnvelopeInput, project_spending, spending_totals,
    validate_reserve_input, validate_settlement,
};

type WorkerDispatchLease = (
    Option<String>,
    Option<i64>,
    Option<i64>,
    String,
    String,
    String,
);
type ProviderPricingSettings = (String, String, String, i64, i64, i64, i64, i64, i64);

#[derive(Debug, Clone)]
struct EnvelopeRow {
    id: String,
    request_id: String,
    project_id: String,
    currency: String,
    state: EnvelopeState,
    authorized_micros: UsdMicros,
    authorization_revision: i64,
    created_at_ms: i64,
    revoked_at_ms: Option<i64>,
}

impl EnvelopeRow {
    fn record(&self) -> InferenceEnvelopeRecord {
        InferenceEnvelopeRecord {
            envelope_id: self.id.clone(),
            request_id: self.request_id.clone(),
            project_id: self.project_id.clone(),
            currency: self.currency.clone(),
            state: self.state,
            authorized_micros: self.authorized_micros,
            authorization_revision: self.authorization_revision,
            created_at_ms: self.created_at_ms,
            revoked_at_ms: self.revoked_at_ms,
        }
    }

    fn snapshot(&self) -> EnvelopeSnapshot {
        EnvelopeSnapshot {
            envelope_id: self.id.clone(),
            currency: self.currency.clone(),
            state: self.state,
            authorized_micros: self.authorized_micros,
            authorization_revision: self.authorization_revision,
        }
    }
}

#[derive(Debug, Clone)]
struct ReservationRow {
    id: String,
    envelope_id: String,
    operation_id: String,
    idempotency_key: String,
    request_sha256: String,
    authorization_revision: i64,
    scope_sha256: String,
    role: InferenceRole,
    principal_id: String,
    parent_operation_id: Option<String>,
    provider: String,
    model: String,
    pricing_revision: String,
    pricing_snapshot_json: String,
    upper_bound_micros: UsdMicros,
    settled_micros: Option<UsdMicros>,
    state: ReservationState,
    worker_id: Option<String>,
    generation: Option<i64>,
    dispatched_at_ms: Option<i64>,
    provider_request_id: Option<String>,
    expires_at_ms: i64,
    settlement_sha256: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl ReservationRow {
    fn record(&self) -> InferenceReservationRecord {
        InferenceReservationRecord {
            reservation_id: self.id.clone(),
            envelope_id: self.envelope_id.clone(),
            operation_id: self.operation_id.clone(),
            idempotency_key: self.idempotency_key.clone(),
            request_sha256: self.request_sha256.clone(),
            role: self.role,
            principal_id: self.principal_id.clone(),
            parent_operation_id: self.parent_operation_id.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            pricing_revision: self.pricing_revision.clone(),
            pricing_snapshot_json: self.pricing_snapshot_json.clone(),
            authorization_revision: self.authorization_revision,
            scope_sha256: self.scope_sha256.clone(),
            upper_bound_micros: self.upper_bound_micros,
            settled_micros: self.settled_micros,
            state: self.state,
            worker_id: self.worker_id.clone(),
            generation: self.generation,
            dispatched_at_ms: self.dispatched_at_ms,
            provider_request_id: self.provider_request_id.clone(),
            expires_at_ms: self.expires_at_ms,
            settlement_sha256: self.settlement_sha256.clone(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }

    fn amounts(&self) -> ReservationAmounts {
        ReservationAmounts {
            role: self.role,
            provider: self.provider.clone(),
            state: self.state,
            upper_bound_micros: self.upper_bound_micros,
            settled_micros: self.settled_micros,
        }
    }
}

impl Database {
    pub async fn inference_ledger_mode(&self) -> Result<InferenceLedgerMode, Error> {
        let mut connection = self.pool.acquire().await?;
        load_ledger_mode(&mut connection).await
    }

    /// Promote execution from V1 to V2. This is one-way: a V2 cutover cannot be
    /// silently reverted to legacy accounting.
    pub async fn set_inference_ledger_mode_v2(
        &self,
        actor: &str,
        evidence_sha256: &str,
    ) -> Result<InferenceLedgerMode, Error> {
        if actor.trim().is_empty() || actor.len() > 512 || !is_sha256(evidence_sha256) {
            return Err(Error::Validation("inference ledger policy evidence".into()));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current = load_ledger_mode(&mut transaction).await?;
        if current == InferenceLedgerMode::V1 {
            sqlx::query::query(
                "UPDATE inference_ledger_policy
                 SET mode='V2',actor=?1,evidence_sha256=?2,updated_at_ms=?3
                 WHERE id=1 AND mode='V1'",
            )
            .bind(actor)
            .bind(evidence_sha256)
            .bind(Utc::now().timestamp_millis())
            .execute(&mut *transaction)
            .await?;
        }
        let next = load_ledger_mode(&mut transaction).await?;
        transaction.commit().await?;
        Ok(next)
    }

    /// Derive a frozen per-request ceiling from Core-owned provider settings
    /// and the adapter's trusted billing-coverage declaration. A caller cannot
    /// supply a bound, coverage flag or snapshot.
    pub async fn create_inference_pricing_quote(
        &self,
        input: &NewInferencePricingQuote,
    ) -> Result<InferencePricingQuote, Error> {
        input.validate().map_err(ledger_error)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let settings: Option<ProviderPricingSettings> = sqlx::query_as::query_as(
            "SELECT kind,auth_mode,model,max_input_tokens,max_output_tokens,
                    input_micros_per_million,output_micros_per_million,enabled,revision
             FROM provider_settings WHERE provider=?1",
        )
        .bind(&input.provider)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((
            kind,
            auth_mode,
            model,
            max_input_tokens,
            max_output_tokens,
            input_rate,
            output_rate,
            enabled,
            provider_revision,
        )) = settings
        else {
            return Err(Error::Unavailable(
                "no Core provider settings exist for the pricing quote".into(),
            ));
        };
        if model != input.model || enabled != 1 {
            return Err(Error::Unavailable(
                "provider settings are not enabled for the requested model".into(),
            ));
        }
        let Some(coverage) = crate::inference_ledger::adapter_billing_coverage(&kind, &auth_mode)
        else {
            return Err(Error::Unavailable(
                "provider adapter has no enforceable billing coverage".into(),
            ));
        };
        if max_input_tokens <= 0 || max_output_tokens <= 0 || input_rate < 0 || output_rate < 0 {
            return Err(Error::Unavailable(
                "provider settings do not define an enforceable token envelope".into(),
            ));
        }
        let billed_rate = u64::try_from(input_rate.max(output_rate))
            .map_err(|_| Error::Unavailable("provider rate is invalid".into()))?;
        if billed_rate == 0 {
            return Err(Error::Unavailable(
                "provider token rate is unavailable".into(),
            ));
        }
        let total_tokens = u64::try_from(max_input_tokens)
            .ok()
            .and_then(|input| {
                u64::try_from(max_output_tokens)
                    .ok()
                    .and_then(|output| input.checked_add(output))
            })
            .ok_or(Error::Unavailable(
                "provider token envelope overflow".into(),
            ))?;
        let bound = u128::from(total_tokens)
            .checked_mul(u128::from(billed_rate))
            .ok_or(Error::Unavailable("provider price overflow".into()))?
            .div_ceil(1_000_000);
        let upper_bound_micros = UsdMicros::new(
            u64::try_from(bound)
                .map_err(|_| Error::Unavailable("provider price overflow".into()))?,
        )
        .map_err(|_| Error::Unavailable("provider quote bound exceeds range".into()))?;
        if upper_bound_micros == UsdMicros::ZERO {
            return Err(Error::Unavailable(
                "provider token envelope produces a zero price".into(),
            ));
        }
        let coverage_json = serde_json::json!({
            "input": coverage.input,
            "output": coverage.output,
            "cachedInput": coverage.cached_input,
            "reasoning": coverage.reasoning,
            "tools": coverage.tools,
            "retries": coverage.retries,
            "requestOverhead": coverage.request_overhead,
        })
        .to_string();
        let snapshot_json = serde_json::json!({
            "basis": "provider-settings-token-envelope",
            "provider": input.provider,
            "model": input.model,
            "kind": kind,
            "providerRevision": provider_revision,
            "inputMicrosPerMillion": input_rate,
            "outputMicrosPerMillion": output_rate,
            "billedRateMicrosPerMillion": billed_rate,
            "authMode": auth_mode,
            "inputTokens": max_input_tokens,
            "outputTokens": max_output_tokens,
            "upperBoundMicros": upper_bound_micros.get(),
            "coverage": serde_json::from_str::<serde_json::Value>(&coverage_json)
                .map_err(|_| Error::Unavailable("coverage document".into()))?,
        })
        .to_string();
        let coverage_sha256 = sha256_hex(coverage_json.as_bytes());
        if let Some(existing) = load_pricing_quote_by_revision(
            &mut transaction,
            &input.provider,
            &input.model,
            input.revision,
        )
        .await?
        {
            if existing.upper_bound_micros == upper_bound_micros
                && existing.coverage_sha256 == coverage_sha256
                && existing.snapshot_json == snapshot_json
                && existing.evidence_sha256 == input.evidence_sha256
            {
                transaction.commit().await?;
                return Ok(existing);
            }
            return Err(Error::Conflict(
                "pricing quote revision conflicts with its recorded value",
            ));
        }
        let quote = InferencePricingQuote {
            quote_id: Uuid::new_v4().to_string(),
            provider: input.provider.clone(),
            model: input.model.clone(),
            revision: input.revision,
            provider_revision,
            auth_mode,
            input_tokens: u64::try_from(max_input_tokens)
                .map_err(|_| Error::Unavailable("provider input token envelope".into()))?,
            output_tokens: u64::try_from(max_output_tokens)
                .map_err(|_| Error::Unavailable("provider output token envelope".into()))?,
            upper_bound_micros,
            coverage_sha256,
            snapshot_json,
            evidence_sha256: input.evidence_sha256.clone(),
        };
        sqlx::query::query(
            "INSERT INTO inference_pricing_quotes
             (quote_id,provider,model,revision,currency,upper_bound_micros,provider_revision,
              auth_mode,input_tokens,output_tokens,coverage_sha256,coverage_json,snapshot_json,
              evidence_sha256,created_at_ms)
             VALUES (?1,?2,?3,?4,'USD',?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        )
        .bind(&quote.quote_id)
        .bind(&quote.provider)
        .bind(&quote.model)
        .bind(quote.revision)
        .bind(micros_to_i64(quote.upper_bound_micros)?)
        .bind(quote.provider_revision)
        .bind(&quote.auth_mode)
        .bind(i64::try_from(quote.input_tokens).map_err(|_| Error::Unavailable("token".into()))?)
        .bind(i64::try_from(quote.output_tokens).map_err(|_| Error::Unavailable("token".into()))?)
        .bind(&quote.coverage_sha256)
        .bind(&coverage_json)
        .bind(&quote.snapshot_json)
        .bind(&quote.evidence_sha256)
        .bind(Utc::now().timestamp_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(quote)
    }

    pub async fn inference_pricing_quote(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<InferencePricingQuote, Error> {
        let mut connection = self.pool.acquire().await?;
        load_latest_pricing_quote(&mut connection, provider, model)
            .await?
            .ok_or(Error::NotFound)
    }

    /// Resolve the Campaign's explicit V1/V2 requirement. V2 (or a global V2
    /// cutover) without a bound, active envelope fails closed.
    pub async fn require_campaign_inference_envelope(
        &self,
        campaign_id: &str,
    ) -> Result<CampaignInferenceRequirement, Error> {
        let mut connection = self.pool.acquire().await?;
        campaign_requirement_in_tx(&mut connection, campaign_id).await
    }

    /// Create one operator-request envelope and its initial authorization.
    /// Replaying the same authorization idempotency key returns the durable
    /// envelope; a different request or agreement conflicts.
    pub async fn create_inference_envelope(
        &self,
        input: &NewInferenceEnvelope,
    ) -> Result<InferenceEnvelopeRecord, Error> {
        input.validate().map_err(ledger_error)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now_ms = Utc::now().timestamp_millis();
        if let Some(existing) =
            load_authorization_by_key(&mut transaction, &input.idempotency_key).await?
        {
            let envelope = load_envelope(&mut transaction, &existing.envelope_id)
                .await?
                .ok_or(Error::NotFound)?;
            if envelope.request_id != input.request_id
                || envelope.project_id != input.project_id
                || existing.limit_micros != input.authorized_micros
                || existing.evidence_sha256 != input.evidence_sha256
                || existing.scope_sha256 != input.scope_sha256
                || existing.actor != input.actor
            {
                return Err(Error::Conflict(
                    "inference authorization idempotency key was used differently",
                ));
            }
            transaction.commit().await?;
            return Ok(envelope.record());
        }
        if find_envelope_for_request_alias(&mut transaction, &input.request_id)
            .await?
            .is_some()
        {
            return Err(Error::Conflict(
                "operator request alias already has an inference envelope",
            ));
        }
        // If the request identity is already known, its project is durable and
        // must match. Unknown standalone request identities remain admissible
        // only because no canonical request row exists to contradict them.
        let lead_project: Option<(String,)> =
            sqlx::query_as::query_as("SELECT project_id FROM lead_requests WHERE id=?1")
                .bind(&input.request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        let intake_project: Option<(String,)> =
            sqlx::query_as::query_as("SELECT project_id FROM intake_requests WHERE id=?1")
                .bind(&input.request_id)
                .fetch_optional(&mut *transaction)
                .await?;
        if let (Some((lead,)), Some((intake,))) = (&lead_project, &intake_project)
            && lead != intake
        {
            return Err(Error::Conflict(
                "request identity resolves to conflicting projects",
            ));
        }
        if let Some((project,)) = lead_project.or(intake_project)
            && project != input.project_id
        {
            return Err(Error::Forbidden(
                "inference envelope project differs from its request project".into(),
            ));
        }
        let envelope_id = Uuid::new_v4().to_string();
        insert_envelope(&mut transaction, &envelope_id, input, now_ms).await?;
        insert_authorization(
            &mut transaction,
            &envelope_id,
            1,
            input.authorized_micros,
            &input.actor,
            &input.scope_sha256,
            &input.evidence_sha256,
            &input.idempotency_key,
            now_ms,
        )
        .await?;
        insert_ledger_event(
            &mut transaction,
            &envelope_id,
            None,
            "ENVELOPE_CREATED",
            &input.actor,
            &format!("request:{}", input.request_id),
            &input.evidence_sha256,
            now_ms,
        )
        .await?;
        let envelope = load_envelope(&mut transaction, &envelope_id)
            .await?
            .ok_or(Error::NotFound)?;
        transaction.commit().await?;
        Ok(envelope.record())
    }

    /// Append a new authorization revision. This can raise, or re-evaluate, the
    /// request limit; it never resurrects a revoked envelope.
    pub async fn authorize_inference_envelope(
        &self,
        input: &NewInferenceAuthorization,
    ) -> Result<InferenceEnvelopeRecord, Error> {
        input.validate().map_err(ledger_error)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now_ms = Utc::now().timestamp_millis();
        let envelope = load_envelope(&mut transaction, &input.envelope_id)
            .await?
            .ok_or(Error::NotFound)?;
        if matches!(
            envelope.state,
            EnvelopeState::Revoked | EnvelopeState::Closed
        ) {
            return Err(Error::Forbidden("inference envelope is not active".into()));
        }
        if let Some(existing) =
            load_authorization_by_key(&mut transaction, &input.idempotency_key).await?
        {
            if existing.envelope_id == envelope.id
                && existing.limit_micros == input.limit_micros
                && existing.evidence_sha256 == input.evidence_sha256
                && existing.scope_sha256 == input.scope_sha256
                && existing.actor == input.actor
            {
                transaction.commit().await?;
                return Ok(envelope.record());
            }
            return Err(Error::Conflict(
                "inference authorization idempotency key was used differently",
            ));
        }
        let next_revision =
            envelope
                .authorization_revision
                .checked_add(1)
                .ok_or(Error::Validation(
                    "inference authorization revision overflow".into(),
                ))?;
        insert_authorization(
            &mut transaction,
            &envelope.id,
            next_revision,
            input.limit_micros,
            &input.actor,
            &input.scope_sha256,
            &input.evidence_sha256,
            &input.idempotency_key,
            now_ms,
        )
        .await?;
        let amounts = load_envelope_amounts(&mut transaction, &envelope.id).await?;
        let totals = spending_totals(input.limit_micros, &amounts).map_err(ledger_error)?;
        let next_state = if totals.settled_micros > input.limit_micros {
            EnvelopeState::Overdrawn
        } else if envelope.state == EnvelopeState::Overdrawn
            && input.limit_micros >= totals.committed_micros
        {
            EnvelopeState::Active
        } else {
            envelope.state
        };
        sqlx::query::query(
            "UPDATE inference_envelopes
             SET authorized_micros=?1,authorization_revision=?2,state=?3
             WHERE id=?4",
        )
        .bind(micros_to_i64(input.limit_micros)?)
        .bind(next_revision)
        .bind(envelope_state_str(next_state))
        .bind(&envelope.id)
        .execute(&mut *transaction)
        .await?;
        insert_ledger_event(
            &mut transaction,
            &envelope.id,
            None,
            "AUTHORIZED",
            &input.actor,
            &format!("authorization:{next_revision}"),
            &input.evidence_sha256,
            now_ms,
        )
        .await?;
        let envelope = load_envelope(&mut transaction, &envelope.id)
            .await?
            .ok_or(Error::NotFound)?;
        transaction.commit().await?;
        Ok(envelope.record())
    }

    /// Bind a Campaign to the request envelope it inherits. The Campaign must
    /// belong to the envelope's project and to the same operator request; an
    /// envelope can back at most one Campaign. Binding is a one-way V2
    /// classification.
    pub async fn bind_campaign_inference_envelope(
        &self,
        input: &CampaignEnvelopeBindingInput,
    ) -> Result<(), Error> {
        input.validate().map_err(ledger_error)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let envelope = load_envelope(&mut transaction, &input.envelope_id)
            .await?
            .ok_or(Error::NotFound)?;
        let campaign: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT a.project_id FROM campaigns c JOIN actions a ON a.id=c.action_id
             WHERE c.id=?1",
        )
        .bind(&input.campaign_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((project_id,)) = campaign else {
            return Err(Error::NotFound);
        };
        if project_id != envelope.project_id {
            return Err(Error::Forbidden(
                "campaign belongs to a different project than the inference envelope".into(),
            ));
        }
        if !campaign_owns_envelope_request(
            &mut transaction,
            &input.campaign_id,
            &envelope.request_id,
        )
        .await?
        {
            return Err(Error::Forbidden(
                "campaign does not belong to the envelope's operator request".into(),
            ));
        }
        let existing: Option<(String, String)> = sqlx::query_as::query_as(
            "SELECT campaign_id,envelope_id FROM inference_campaign_bindings
             WHERE campaign_id=?1 OR envelope_id=?2",
        )
        .bind(&input.campaign_id)
        .bind(&input.envelope_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((existing_campaign, existing_envelope)) = existing {
            if existing_campaign != input.campaign_id || existing_envelope != input.envelope_id {
                return Err(Error::Conflict(
                    "campaign or inference envelope is already bound elsewhere",
                ));
            }
            classify_campaign_v2(&mut transaction, &input.campaign_id, &input.evidence_sha256)
                .await?;
            transaction.commit().await?;
            return Ok(());
        }
        sqlx::query::query(
            "INSERT INTO inference_campaign_bindings(campaign_id,envelope_id,bound_at_ms)
             VALUES (?1,?2,?3)",
        )
        .bind(&input.campaign_id)
        .bind(&envelope.id)
        .bind(Utc::now().timestamp_millis())
        .execute(&mut *transaction)
        .await?;
        classify_campaign_v2(&mut transaction, &input.campaign_id, &input.evidence_sha256).await?;
        insert_ledger_event(
            &mut transaction,
            &envelope.id,
            None,
            "CAMPAIGN_BOUND",
            &input.actor,
            &format!(
                "campaign:{}:request:{}",
                input.campaign_id, envelope.request_id
            ),
            &input.evidence_sha256,
            Utc::now().timestamp_millis(),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Atomically admit a new hold. The full upper bound is committed before
    /// any upstream effect and remains held until settlement, proven-undispatched
    /// release, or explicit reconciliation.
    pub async fn reserve_inference(
        &self,
        input: &ReserveInferenceInput,
    ) -> Result<InferenceReservation, Error> {
        let now_ms = Utc::now().timestamp_millis();
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        expire_inference_in_tx(&mut transaction, now_ms).await?;
        let reservation = reserve_inference_in_tx(&mut transaction, input, now_ms).await?;
        transaction.commit().await?;
        Ok(reservation.projection())
    }

    /// Record dispatch intent before the upstream request is sent. An
    /// ambiguous reservation is never redispatched.
    pub async fn mark_inference_dispatched(
        &self,
        input: &MarkInferenceDispatchedInput,
    ) -> Result<InferenceReservation, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let reservation =
            mark_inference_dispatched_in_tx(&mut transaction, input, Utc::now().timestamp_millis())
                .await?;
        transaction.commit().await?;
        Ok(reservation.projection())
    }

    /// Replace a hold with the actual charge. Unknown cost remains ambiguous and
    /// is never silently settled. An actual amount above the reservation bound
    /// is recorded truthfully and marks the envelope `OVERDRAWN`.
    pub async fn settle_inference(
        &self,
        settlement: &InferenceSettlement,
    ) -> Result<InferenceReservation, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let reservation =
            settle_inference_in_tx(&mut transaction, settlement, Utc::now().timestamp_millis())
                .await?;
        transaction.commit().await?;
        Ok(reservation.projection())
    }

    /// Release only a reservation with no recorded dispatch intent. A
    /// dispatched or uncertain operation requires reconciliation.
    pub async fn release_inference(
        &self,
        reservation_id: &str,
        actor: &str,
        evidence_ref: &str,
    ) -> Result<InferenceReservation, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let reservation = release_inference_in_tx(
            &mut transaction,
            reservation_id,
            actor,
            evidence_ref,
            Utc::now().timestamp_millis(),
        )
        .await?;
        transaction.commit().await?;
        Ok(reservation.projection())
    }

    /// Resolve an ambiguous hold with explicit operator evidence.
    pub async fn reconcile_inference(
        &self,
        input: &ReconcileInferenceInput,
    ) -> Result<InferenceReservation, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let reservation =
            reconcile_inference_in_tx(&mut transaction, input, Utc::now().timestamp_millis())
                .await?;
        transaction.commit().await?;
        Ok(reservation.projection())
    }

    /// Revoke new spending. Existing holds and unsettled liabilities are
    /// preserved; a late authenticated settlement is still accepted.
    pub async fn revoke_inference_envelope(
        &self,
        input: &RevokeInferenceEnvelopeInput,
    ) -> Result<InferenceEnvelopeRecord, Error> {
        input.validate().map_err(ledger_error)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now_ms = Utc::now().timestamp_millis();
        let envelope = load_envelope(&mut transaction, &input.envelope_id)
            .await?
            .ok_or(Error::NotFound)?;
        match envelope.state {
            EnvelopeState::Active | EnvelopeState::Overdrawn => {
                sqlx::query::query(
                    "UPDATE inference_envelopes SET state='REVOKED',revoked_at_ms=?1 WHERE id=?2",
                )
                .bind(now_ms)
                .bind(&envelope.id)
                .execute(&mut *transaction)
                .await?;
                insert_ledger_event(
                    &mut transaction,
                    &envelope.id,
                    None,
                    "REVOKED",
                    &input.actor,
                    &input.evidence_ref,
                    &input.evidence_sha256,
                    now_ms,
                )
                .await?;
            }
            EnvelopeState::Revoked => {}
            EnvelopeState::Closed => {
                return Err(Error::Conflict("inference envelope is closed"));
            }
        }
        let envelope = load_envelope(&mut transaction, &envelope.id)
            .await?
            .ok_or(Error::NotFound)?;
        transaction.commit().await?;
        Ok(envelope.record())
    }

    /// Mark expired holds ambiguous. Expiry is never proof that no provider
    /// charge occurred, so an expired hold is preserved, not released.
    pub async fn expire_inference_reservations(&self, now_ms: i64) -> Result<(), Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        expire_inference_in_tx(&mut transaction, now_ms).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Aggregate financial status. Campaign totals derive exclusively from this
    /// projection; legacy call/session totals are evidence and are not re-added.
    pub async fn inference_spending_status(
        &self,
        envelope_id: &str,
    ) -> Result<SpendingStatus, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now_ms = Utc::now().timestamp_millis();
        expire_inference_in_tx(&mut transaction, now_ms).await?;
        let status = spending_status_in_tx(&mut transaction, envelope_id, now_ms).await?;
        transaction.commit().await?;
        Ok(status)
    }

    pub async fn inference_envelope_for_request(
        &self,
        request_id: &str,
    ) -> Result<Option<InferenceEnvelopeRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        Ok(load_envelope_by_request(&mut connection, request_id)
            .await?
            .map(|envelope| envelope.record()))
    }

    pub async fn inference_envelope_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<Option<InferenceEnvelopeRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        Ok(load_envelope_for_campaign(&mut connection, campaign_id)
            .await?
            .map(|envelope| envelope.record()))
    }

    pub async fn inference_reservation(
        &self,
        reservation_id: &str,
    ) -> Result<InferenceReservationRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        let row = load_reservation(&mut connection, reservation_id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(row.record())
    }

    pub async fn inference_reservations(
        &self,
        envelope_id: &str,
    ) -> Result<Vec<InferenceReservationRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        Ok(load_envelope_reservations(&mut connection, envelope_id)
            .await?
            .iter()
            .map(ReservationRow::record)
            .collect())
    }
}

/// Admit and insert one hold inside an existing `BEGIN IMMEDIATE` transaction.
pub(crate) async fn reserve_inference_in_tx(
    connection: &mut SqliteConnection,
    input: &ReserveInferenceInput,
    now_ms: i64,
) -> Result<InferenceReservationRecord, Error> {
    validate_reserve_input(input).map_err(ledger_error)?;
    let envelope = load_envelope(connection, &input.envelope_id)
        .await?
        .ok_or(Error::NotFound)?;
    // The monetary bound comes from a frozen adapter quote, never from a
    // caller estimate. Exact revision, snapshot and bound equality make a
    // lowered or invented liability impossible.
    let pricing_quote = load_pricing_quote(
        connection,
        &input.pricing_revision,
        &input.provider,
        &input.model,
    )
    .await?
    .ok_or(Error::Validation(
        "inference reservation requires a frozen pricing quote".into(),
    ))?;
    let settings: Option<(String, String, i64, i64)> = sqlx::query_as::query_as(
        "SELECT model,auth_mode,enabled,revision FROM provider_settings WHERE provider=?1",
    )
    .bind(&input.provider)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((settings_model, auth_mode, enabled, settings_revision)) = settings else {
        return Err(Error::Unavailable(
            "pricing quote has no current provider settings".into(),
        ));
    };
    if enabled != 1
        || auth_mode != "api_key"
        || settings_model != pricing_quote.model
        || settings_revision != pricing_quote.provider_revision
    {
        return Err(Error::Unavailable(
            "pricing quote is stale for the current provider settings".into(),
        ));
    }
    let snapshot: serde_json::Value = serde_json::from_str(&pricing_quote.snapshot_json)
        .map_err(|_| Error::Unavailable("frozen pricing snapshot is invalid".into()))?;
    if snapshot
        .get("upperBoundMicros")
        .and_then(serde_json::Value::as_u64)
        != Some(pricing_quote.upper_bound_micros.get())
        || snapshot.get("provider").and_then(serde_json::Value::as_str)
            != Some(pricing_quote.provider.as_str())
        || snapshot.get("model").and_then(serde_json::Value::as_str)
            != Some(pricing_quote.model.as_str())
        || snapshot
            .get("providerRevision")
            .and_then(serde_json::Value::as_i64)
            != Some(pricing_quote.provider_revision)
        || snapshot.get("authMode").and_then(serde_json::Value::as_str)
            != Some(pricing_quote.auth_mode.as_str())
        || snapshot
            .get("inputTokens")
            .and_then(serde_json::Value::as_u64)
            != Some(pricing_quote.input_tokens)
        || snapshot
            .get("outputTokens")
            .and_then(serde_json::Value::as_u64)
            != Some(pricing_quote.output_tokens)
    {
        return Err(Error::Conflict(
            "frozen pricing snapshot does not encode its stored bound/route",
        ));
    }
    if pricing_quote.snapshot_json != input.pricing_snapshot_json {
        return Err(Error::Conflict(
            "inference pricing snapshot differs from the frozen quote",
        ));
    }
    if pricing_quote.upper_bound_micros != input.upper_bound_micros {
        return Err(Error::Validation(
            "caller-supplied bound differs from the frozen pricing quote".into(),
        ));
    }
    if let Some(existing) = load_reservation_by_identity(
        connection,
        &input.envelope_id,
        &input.idempotency_key,
        &input.operation_id,
    )
    .await?
    {
        if existing.request_sha256 != input.request_sha256
            || existing.operation_id != input.operation_id
            || existing.idempotency_key != input.idempotency_key
            || existing.role != input.role
            || existing.principal_id != input.principal_id
            || existing.parent_operation_id.as_deref() != input.parent_operation_id.as_deref()
            || existing.provider != input.provider
            || existing.model != input.model
            || existing.pricing_revision != input.pricing_revision
            || existing.authorization_revision != input.authorization_revision
            || existing.scope_sha256 != input.scope_sha256
            || existing.upper_bound_micros != input.upper_bound_micros
            || existing.expires_at_ms != input.expires_at_ms
        {
            return Err(Error::Conflict(
                "inference reservation conflicts with its recorded identity",
            ));
        }
        match existing.state {
            ReservationState::Reserved if existing.dispatched_at_ms.is_some() => {
                return Err(Error::Forbidden(
                    "inference operation was already dispatched and is unresolved".into(),
                ));
            }
            ReservationState::Ambiguous => {
                return Err(Error::Forbidden(
                    "inference operation is ambiguous; reconciliation is required".into(),
                ));
            }
            ReservationState::Reserved | ReservationState::Settled | ReservationState::Released => {
                return Ok(existing.record());
            }
        }
    }
    match envelope.state {
        EnvelopeState::Active => {}
        EnvelopeState::Overdrawn => {
            return Err(Error::Forbidden(
                "inference spending authorization is exhausted".into(),
            ));
        }
        EnvelopeState::Revoked | EnvelopeState::Closed => {
            return Err(Error::Forbidden("inference envelope is not active".into()));
        }
    }
    // The caller's expected revision and scope are re-read from durable state.
    // A stale expectation can never create a new hold.
    if input.authorization_revision != envelope.authorization_revision {
        return Err(Error::Conflict("inference authorization revision is stale"));
    }
    let Some(stored_scope) =
        load_authorization_scope(connection, &envelope.id, input.authorization_revision).await?
    else {
        return Err(Error::Conflict(
            "inference authorization revision is not recorded",
        ));
    };
    if stored_scope != input.scope_sha256 {
        return Err(Error::Conflict(
            "inference scope digest does not match the durable authorization",
        ));
    }
    if input.expires_at_ms <= now_ms {
        return Err(Error::Validation(
            "inference reservation expiry must be in the future".into(),
        ));
    }
    if let Some(parent) = &input.parent_operation_id {
        let parent_exists: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT id FROM inference_reservations WHERE envelope_id=?1 AND operation_id=?2",
        )
        .bind(&input.envelope_id)
        .bind(parent)
        .fetch_optional(&mut *connection)
        .await?;
        if parent_exists.is_none() {
            return Err(Error::Validation(
                "parent inference operation does not exist in this envelope".into(),
            ));
        }
    }
    let amounts = load_envelope_amounts(connection, &input.envelope_id).await?;
    let totals = spending_totals(envelope.authorized_micros, &amounts).map_err(ledger_error)?;
    if totals.committed_micros.get() > envelope.authorized_micros.get() {
        return Err(Error::Forbidden(
            "inference spending authorization is exhausted".into(),
        ));
    }
    let required = totals
        .committed_micros
        .checked_add(input.upper_bound_micros)
        .map_err(InferenceLedgerError::from)
        .map_err(ledger_error)?;
    if required > envelope.authorized_micros {
        return Err(Error::Forbidden(
            "inference spending authorization is exhausted".into(),
        ));
    }
    let reservation_id = Uuid::new_v4().to_string();
    sqlx::query::query(
        "INSERT INTO inference_reservations
         (id,envelope_id,operation_id,idempotency_key,request_sha256,role,
          authorization_revision,scope_sha256,principal_id,parent_operation_id,provider,model,
          pricing_revision,pricing_snapshot_json,upper_bound_micros,settled_micros,state,
          worker_id,generation,expires_at_ms,created_at_ms,updated_at_ms)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,NULL,'RESERVED',
                 ?16,?17,?18,?19,?19)",
    )
    .bind(&reservation_id)
    .bind(&input.envelope_id)
    .bind(&input.operation_id)
    .bind(&input.idempotency_key)
    .bind(&input.request_sha256)
    .bind(input.role.as_str())
    .bind(input.authorization_revision)
    .bind(&input.scope_sha256)
    .bind(&input.principal_id)
    .bind(&input.parent_operation_id)
    .bind(&input.provider)
    .bind(&input.model)
    .bind(&input.pricing_revision)
    .bind(&input.pricing_snapshot_json)
    .bind(micros_to_i64(input.upper_bound_micros)?)
    .bind(&input.worker_id)
    .bind(input.generation)
    .bind(input.expires_at_ms)
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    insert_ledger_event(
        connection,
        &input.envelope_id,
        Some(&reservation_id),
        "RESERVED",
        &input.principal_id,
        &format!("reservation:{reservation_id}"),
        &input.request_sha256,
        now_ms,
    )
    .await?;
    load_reservation(connection, &reservation_id)
        .await?
        .map(|row| row.record())
        .ok_or(Error::NotFound)
}

/// Record dispatch intent inside an existing transaction. Idempotent replay of
/// the same identity returns the durable record; a different identity conflicts.
pub(crate) async fn mark_inference_dispatched_in_tx(
    connection: &mut SqliteConnection,
    input: &MarkInferenceDispatchedInput,
    now_ms: i64,
) -> Result<InferenceReservationRecord, Error> {
    let (record, _) = dispatch_inference_in_tx(connection, input, now_ms, false).await?;
    Ok(record)
}

/// Acquire dispatch intent for a new upstream send. Only a newly recorded
/// intent returns success; an already-dispatched reservation is refused so a
/// caller can never authorize a second send from a replayed intent.
pub(crate) async fn acquire_inference_dispatch_in_tx(
    connection: &mut SqliteConnection,
    input: &MarkInferenceDispatchedInput,
    now_ms: i64,
) -> Result<InferenceReservationRecord, Error> {
    let (record, _) = dispatch_inference_in_tx(connection, input, now_ms, true).await?;
    Ok(record)
}

async fn dispatch_inference_in_tx(
    connection: &mut SqliteConnection,
    input: &MarkInferenceDispatchedInput,
    now_ms: i64,
    require_new: bool,
) -> Result<(InferenceReservationRecord, bool), Error> {
    input.validate().map_err(ledger_error)?;
    let reservation = load_reservation(connection, &input.reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Reserved => {}
        ReservationState::Ambiguous => {
            return Err(Error::Forbidden(
                "inference operation is ambiguous; reconciliation is required".into(),
            ));
        }
        ReservationState::Settled | ReservationState::Released => {
            return Err(Error::Conflict("inference reservation is already terminal"));
        }
    }
    if reservation.dispatched_at_ms.is_some() {
        if reservation.worker_id == input.worker_id
            && reservation.generation == input.generation
            && reservation.provider_request_id == input.provider_request_id
        {
            if require_new {
                return Err(Error::Forbidden(
                    "inference dispatch intent was already acquired".into(),
                ));
            }
            return Ok((reservation.record(), false));
        }
        return Err(Error::Conflict(
            "inference dispatch intent conflicts with the recorded dispatch",
        ));
    }
    if reservation.worker_id != input.worker_id || reservation.generation != input.generation {
        return Err(Error::Conflict(
            "inference dispatch fencing differs from the reservation",
        ));
    }
    if reservation.expires_at_ms <= now_ms {
        return Err(Error::Validation(
            "inference reservation expired before dispatch".into(),
        ));
    }
    let envelope = load_envelope(connection, &reservation.envelope_id)
        .await?
        .ok_or(Error::NotFound)?;
    match envelope.state {
        EnvelopeState::Active => {}
        EnvelopeState::Overdrawn => {
            return Err(Error::Forbidden(
                "inference spending authorization is exhausted".into(),
            ));
        }
        EnvelopeState::Revoked | EnvelopeState::Closed => {
            return Err(Error::Forbidden("inference envelope is not active".into()));
        }
    }
    // Obsolete pricing may not authorize a new dispatch. Historical quote rows
    // remain for settlement/audit, but only the quote matching the current
    // enabled provider settings revision and auth mode is eligible.
    let quote = load_pricing_quote(
        connection,
        &reservation.pricing_revision,
        &reservation.provider,
        &reservation.model,
    )
    .await?
    .ok_or(Error::Unavailable(
        "dispatched reservation has no frozen pricing quote".into(),
    ))?;
    let settings: Option<(String, i64, i64)> = sqlx::query_as::query_as(
        "SELECT auth_mode,enabled,revision FROM provider_settings WHERE provider=?1",
    )
    .bind(&reservation.provider)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((auth_mode, enabled, settings_revision)) = settings else {
        return Err(Error::Unavailable(
            "dispatched reservation has no current provider settings".into(),
        ));
    };
    if enabled != 1
        || auth_mode != "api_key"
        || settings_revision != quote.provider_revision
        || quote.auth_mode != auth_mode
    {
        return Err(Error::Unavailable(
            "pricing quote is stale for dispatch".into(),
        ));
    }
    // Frozen authorization context must still resolve to the exact durable
    // revision/scope recorded on the reservation. Append-only reauthorization
    // does not erase older revision evidence.
    let Some(scope) =
        load_authorization_scope(connection, &envelope.id, reservation.authorization_revision)
            .await?
    else {
        return Err(Error::Conflict(
            "reservation authorization revision is missing",
        ));
    };
    if scope != reservation.scope_sha256 {
        return Err(Error::Conflict(
            "reservation scope differs from durable authorization evidence",
        ));
    }
    if reservation.role == InferenceRole::Implementer {
        let Some(attempt_id) = &input.attempt_id else {
            return Err(Error::Validation(
                "worker dispatch requires an attempt id".into(),
            ));
        };
        let lease: Option<WorkerDispatchLease> = sqlx::query_as::query_as(
            "SELECT l.worker_id,l.generation,l.expires_at_ms,a.status,c.status,r.campaign_id
                 FROM attempts a
                 JOIN runs r ON r.id=a.run_id
                 JOIN campaigns c ON c.id=r.campaign_id
                 LEFT JOIN worker_leases l ON l.attempt_id=a.id
                 WHERE a.id=?1",
        )
        .bind(attempt_id)
        .fetch_optional(&mut *connection)
        .await?;
        let Some((
            lease_worker,
            lease_generation,
            lease_expiry,
            attempt_status,
            campaign_status,
            campaign_id,
        )) = lease
        else {
            return Err(Error::Forbidden(
                "worker dispatch has no current attempt lineage".into(),
            ));
        };
        if lease_worker.as_deref() != input.worker_id.as_deref()
            || lease_generation != input.generation
            || lease_expiry.is_none_or(|expiry| expiry <= now_ms)
            || attempt_status != "LEASED"
            || campaign_status != "AUTHORIZED"
        {
            return Err(Error::Forbidden(
                "worker dispatch is not authorized by the current lease".into(),
            ));
        }
        let (billing_mode,): (String,) = sqlx::query_as::query_as(
            "SELECT mode FROM inference_campaign_modes WHERE campaign_id=?1",
        )
        .bind(&campaign_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::Forbidden(
            "worker dispatch campaign is not classified V2".into(),
        ))?;
        if billing_mode != "V2" {
            return Err(Error::Forbidden(
                "worker dispatch campaign is not classified V2".into(),
            ));
        }
    } else if input.attempt_id.is_some() || input.worker_id.is_some() {
        return Err(Error::Validation(
            "non-worker dispatch must not carry worker fencing".into(),
        ));
    }
    sqlx::query::query(
        "UPDATE inference_reservations
         SET worker_id=?1,generation=?2,dispatched_at_ms=?3,provider_request_id=?4,updated_at_ms=?5
         WHERE id=?6 AND state='RESERVED' AND dispatched_at_ms IS NULL",
    )
    .bind(&input.worker_id)
    .bind(input.generation)
    .bind(now_ms)
    .bind(&input.provider_request_id)
    .bind(now_ms)
    .bind(&reservation.id)
    .execute(&mut *connection)
    .await?;
    insert_ledger_event(
        connection,
        &reservation.envelope_id,
        Some(&reservation.id),
        "DISPATCHED",
        &input.actor,
        &format!("reservation:{}", reservation.id),
        &reservation.request_sha256,
        now_ms,
    )
    .await?;
    let record = load_reservation(connection, &reservation.id)
        .await?
        .ok_or(Error::NotFound)?
        .record();
    Ok((record, true))
}

/// Settle a reserved hold inside an existing transaction.
pub(crate) async fn settle_inference_in_tx(
    connection: &mut SqliteConnection,
    settlement: &InferenceSettlement,
    now_ms: i64,
) -> Result<InferenceReservationRecord, Error> {
    validate_settlement(settlement).map_err(ledger_error)?;
    let reservation = load_reservation(connection, &settlement.reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Settled => {
            if reservation.settled_micros == Some(settlement.settled_micros)
                && reservation.settlement_sha256.as_deref()
                    == Some(settlement.usage_evidence_sha256.as_str())
            {
                return Ok(reservation.record());
            }
            return Err(Error::Conflict(
                "inference reservation was already settled differently",
            ));
        }
        ReservationState::Released => {
            return Err(Error::Conflict(
                "inference reservation was released and cannot be settled",
            ));
        }
        ReservationState::Ambiguous => {
            return Err(Error::Forbidden(
                "inference operation is ambiguous; reconciliation is required".into(),
            ));
        }
        ReservationState::Reserved => {}
    }
    sqlx::query::query(
        "UPDATE inference_reservations
         SET state='SETTLED',settled_micros=?1,settlement_sha256=?2,updated_at_ms=?3
         WHERE id=?4 AND state='RESERVED'",
    )
    .bind(micros_to_i64(settlement.settled_micros)?)
    .bind(&settlement.usage_evidence_sha256)
    .bind(now_ms)
    .bind(&reservation.id)
    .execute(&mut *connection)
    .await?;
    insert_ledger_event(
        connection,
        &reservation.envelope_id,
        Some(&reservation.id),
        "SETTLED",
        "core:settlement",
        &format!("reservation:{}", reservation.id),
        &settlement.usage_evidence_sha256,
        now_ms,
    )
    .await?;
    mark_envelope_overdrawn_if_needed(
        connection,
        &reservation,
        settlement.settled_micros,
        &settlement.usage_evidence_sha256,
        now_ms,
    )
    .await?;
    load_reservation(connection, &reservation.id)
        .await?
        .map(|row| row.record())
        .ok_or(Error::NotFound)
}

/// Mark a reservation ambiguous inside an existing transaction. Idempotent.
pub(crate) async fn mark_inference_ambiguous_in_tx(
    connection: &mut SqliteConnection,
    reservation_id: &str,
    actor: &str,
    evidence_ref: &str,
    payload_sha256: &str,
    now_ms: i64,
) -> Result<(), Error> {
    if actor.trim().is_empty() || evidence_ref.trim().is_empty() || evidence_ref.len() > 512 {
        return Err(Error::Validation("inference ambiguity evidence".into()));
    }
    if !is_sha256(payload_sha256) {
        return Err(Error::Validation(
            "inference ambiguity payload digest".into(),
        ));
    }
    let reservation = load_reservation(connection, reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Ambiguous => return Ok(()),
        ReservationState::Settled => return Ok(()),
        ReservationState::Released => {
            return Err(Error::Conflict(
                "inference reservation was released and cannot become ambiguous",
            ));
        }
        ReservationState::Reserved => {}
    }
    sqlx::query::query(
        "UPDATE inference_reservations SET state='AMBIGUOUS',updated_at_ms=?1
         WHERE id=?2 AND state='RESERVED'",
    )
    .bind(now_ms)
    .bind(&reservation.id)
    .execute(&mut *connection)
    .await?;
    insert_ledger_event(
        connection,
        &reservation.envelope_id,
        Some(&reservation.id),
        "AMBIGUOUS",
        actor,
        evidence_ref,
        payload_sha256,
        now_ms,
    )
    .await?;
    Ok(())
}

/// Release a proven-undispatched reservation inside an existing transaction.
pub(crate) async fn release_inference_in_tx(
    connection: &mut SqliteConnection,
    reservation_id: &str,
    actor: &str,
    evidence_ref: &str,
    now_ms: i64,
) -> Result<InferenceReservationRecord, Error> {
    if actor.trim().is_empty() || evidence_ref.trim().is_empty() || evidence_ref.len() > 512 {
        return Err(Error::Validation("inference release evidence".into()));
    }
    let reservation = load_reservation(connection, reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Released => return Ok(reservation.record()),
        ReservationState::Ambiguous => {
            return Err(Error::Forbidden(
                "inference operation is ambiguous; reconciliation is required".into(),
            ));
        }
        ReservationState::Settled => {
            return Err(Error::Conflict(
                "settled inference reservation cannot be released",
            ));
        }
        ReservationState::Reserved => {}
    }
    if reservation.dispatched_at_ms.is_some() || reservation.provider_request_id.is_some() {
        return Err(Error::Conflict(
            "dispatched inference reservation cannot be released without reconciliation",
        ));
    }
    sqlx::query::query(
        "UPDATE inference_reservations SET state='RELEASED',updated_at_ms=?1
         WHERE id=?2 AND state='RESERVED'",
    )
    .bind(now_ms)
    .bind(&reservation.id)
    .execute(&mut *connection)
    .await?;
    let payload_sha256 =
        sha256_hex(format!("release:v1:{}:{actor}:{evidence_ref}", reservation.id).as_bytes());
    insert_ledger_event(
        connection,
        &reservation.envelope_id,
        Some(&reservation.id),
        "RELEASED",
        actor,
        evidence_ref,
        &payload_sha256,
        now_ms,
    )
    .await?;
    load_reservation(connection, &reservation.id)
        .await?
        .map(|row| row.record())
        .ok_or(Error::NotFound)
}

/// Reconcile an ambiguous reservation inside an existing transaction.
pub(crate) async fn reconcile_inference_in_tx(
    connection: &mut SqliteConnection,
    input: &ReconcileInferenceInput,
    now_ms: i64,
) -> Result<InferenceReservationRecord, Error> {
    input.validate().map_err(ledger_error)?;
    let reservation = load_reservation(connection, &input.reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Ambiguous => {}
        ReservationState::Settled => {
            if let ReconciliationOutcome::Settled { settled_micros } = input.outcome
                && reservation.settled_micros == Some(settled_micros)
                && reservation.settlement_sha256.as_deref() == Some(input.evidence_sha256.as_str())
            {
                return Ok(reservation.record());
            }
            return Err(Error::Conflict("inference reservation is already settled"));
        }
        ReservationState::Released => {
            if matches!(input.outcome, ReconciliationOutcome::Released)
                && reservation.settlement_sha256.as_deref() == Some(input.evidence_sha256.as_str())
            {
                return Ok(reservation.record());
            }
            return Err(Error::Conflict("inference reservation is already released"));
        }
        ReservationState::Reserved => {
            return Err(Error::Conflict("inference reservation is not ambiguous"));
        }
    }
    match input.outcome {
        ReconciliationOutcome::Settled { settled_micros } => {
            sqlx::query::query(
                "UPDATE inference_reservations
                 SET state='SETTLED',settled_micros=?1,settlement_sha256=?2,updated_at_ms=?3
                 WHERE id=?4 AND state='AMBIGUOUS'",
            )
            .bind(micros_to_i64(settled_micros)?)
            .bind(&input.evidence_sha256)
            .bind(now_ms)
            .bind(&reservation.id)
            .execute(&mut *connection)
            .await?;
            insert_ledger_event(
                connection,
                &reservation.envelope_id,
                Some(&reservation.id),
                "RECONCILED",
                &input.actor,
                &input.evidence_ref,
                &input.evidence_sha256,
                now_ms,
            )
            .await?;
            mark_envelope_overdrawn_if_needed(
                connection,
                &reservation,
                settled_micros,
                &input.evidence_sha256,
                now_ms,
            )
            .await?;
        }
        ReconciliationOutcome::Released => {
            sqlx::query::query(
                "UPDATE inference_reservations
                 SET state='RELEASED',settlement_sha256=?1,updated_at_ms=?2
                 WHERE id=?3 AND state='AMBIGUOUS'",
            )
            .bind(&input.evidence_sha256)
            .bind(now_ms)
            .bind(&reservation.id)
            .execute(&mut *connection)
            .await?;
            insert_ledger_event(
                connection,
                &reservation.envelope_id,
                Some(&reservation.id),
                "RECONCILED",
                &input.actor,
                &input.evidence_ref,
                &input.evidence_sha256,
                now_ms,
            )
            .await?;
        }
    }
    load_reservation(connection, &reservation.id)
        .await?
        .map(|row| row.record())
        .ok_or(Error::NotFound)
}

/// Settle a reserved hold or reconcile an ambiguous one; used by conservative
/// worst-case operator resolution. Released and conflicting settlements refuse.
pub(crate) async fn settle_or_reconcile_in_tx(
    connection: &mut SqliteConnection,
    reservation_id: &str,
    settled_micros: UsdMicros,
    evidence_sha256: &str,
    actor: &str,
    evidence_ref: &str,
    now_ms: i64,
) -> Result<(), Error> {
    let reservation = load_reservation(connection, reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Reserved => {
            settle_inference_in_tx(
                connection,
                &InferenceSettlement {
                    reservation_id: reservation_id.to_owned(),
                    settled_micros,
                    usage_evidence_sha256: evidence_sha256.to_owned(),
                },
                now_ms,
            )
            .await?;
        }
        ReservationState::Ambiguous => {
            reconcile_inference_in_tx(
                connection,
                &ReconcileInferenceInput {
                    reservation_id: reservation_id.to_owned(),
                    outcome: ReconciliationOutcome::Settled { settled_micros },
                    actor: actor.to_owned(),
                    evidence_ref: evidence_ref.to_owned(),
                    evidence_sha256: evidence_sha256.to_owned(),
                },
                now_ms,
            )
            .await?;
        }
        ReservationState::Settled => {
            if reservation.settled_micros != Some(settled_micros)
                || reservation.settlement_sha256.as_deref() != Some(evidence_sha256)
            {
                return Err(Error::Conflict(
                    "inference reservation was already settled differently",
                ));
            }
        }
        ReservationState::Released => {
            return Err(Error::Conflict(
                "inference reservation was released and cannot be settled",
            ));
        }
    }
    Ok(())
}

/// Release a reserved hold or reconcile an ambiguous one after an operator
/// decision. Used by the legacy model-call resolution path.
pub(crate) async fn release_or_reconcile_in_tx(
    connection: &mut SqliteConnection,
    reservation_id: &str,
    actor: &str,
    evidence_ref: &str,
    evidence_sha256: &str,
    now_ms: i64,
) -> Result<(), Error> {
    let reservation = load_reservation(connection, reservation_id)
        .await?
        .ok_or(Error::NotFound)?;
    match reservation.state {
        ReservationState::Reserved => {
            release_inference_in_tx(connection, reservation_id, actor, evidence_ref, now_ms)
                .await?;
        }
        ReservationState::Ambiguous => {
            reconcile_inference_in_tx(
                connection,
                &ReconcileInferenceInput {
                    reservation_id: reservation_id.to_owned(),
                    outcome: ReconciliationOutcome::Released,
                    actor: actor.to_owned(),
                    evidence_ref: evidence_ref.to_owned(),
                    evidence_sha256: evidence_sha256.to_owned(),
                },
                now_ms,
            )
            .await?;
        }
        ReservationState::Released => {}
        ReservationState::Settled => {
            return Err(Error::Conflict(
                "settled inference reservation cannot be released",
            ));
        }
    }
    Ok(())
}

/// Ledger reservations attached to one provider turn, ordered by egress
/// sequence. A missing row means the legacy V1 path.
pub(crate) async fn provider_turn_ledger_ids_in_tx(
    connection: &mut SqliteConnection,
    turn_reservation_id: &str,
) -> Result<Vec<(i64, String)>, Error> {
    let rows: Vec<(i64, String)> = sqlx::query_as::query_as(
        "SELECT sequence,ledger_reservation_id FROM provider_turn_ledger_requests
         WHERE reservation_id=?1 ORDER BY sequence",
    )
    .bind(turn_reservation_id)
    .fetch_all(&mut *connection)
    .await?;
    if !rows.is_empty() {
        return Ok(rows);
    }
    // Compatibility evidence for a single linked row created before the
    // per-request mapping existed.
    let legacy: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT ledger_reservation_id FROM provider_turn_reservations
         WHERE reservation_id=?1 AND ledger_reservation_id IS NOT NULL",
    )
    .bind(turn_reservation_id)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(legacy
        .map(|(ledger_id,)| vec![(0, ledger_id)])
        .unwrap_or_default())
}

/// Settle a total receipt across the turn's per-request holds, allocating the
/// actual cost once with checked arithmetic. A remainder beyond all holds is
/// recorded on the last hold so the overrun is truthful and visible.
pub(crate) async fn settle_provider_turn_ledger_in_tx(
    connection: &mut SqliteConnection,
    turn_reservation_id: &str,
    total_cost_micros: UsdMicros,
    actor: &str,
    evidence_ref: &str,
    now_ms: i64,
) -> Result<(), Error> {
    let ledger_ids = provider_turn_ledger_ids_in_tx(connection, turn_reservation_id).await?;
    let mut remaining = total_cost_micros;
    let last_index = ledger_ids.len().saturating_sub(1);
    for (index, (_, ledger_id)) in ledger_ids.iter().enumerate() {
        let reservation = load_reservation(connection, ledger_id)
            .await?
            .ok_or(Error::NotFound)?;
        let portion = if index == last_index {
            remaining
        } else {
            remaining.min(reservation.upper_bound_micros)
        };
        remaining = remaining
            .checked_sub(portion)
            .map_err(InferenceLedgerError::from)
            .map_err(ledger_error)?;
        let digest = sha256_hex(
            format!(
                "provider-turn-settlement:v2:{turn_reservation_id}:{ledger_id}:{}",
                portion.get()
            )
            .as_bytes(),
        );
        settle_or_reconcile_in_tx(
            connection,
            ledger_id,
            portion,
            &digest,
            actor,
            evidence_ref,
            now_ms,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn mark_provider_turn_ledger_ambiguous_in_tx(
    connection: &mut SqliteConnection,
    turn_reservation_id: &str,
    actor: &str,
    evidence_ref: &str,
    payload_sha256: &str,
    now_ms: i64,
) -> Result<(), Error> {
    for (_, ledger_id) in provider_turn_ledger_ids_in_tx(connection, turn_reservation_id).await? {
        mark_inference_ambiguous_in_tx(
            connection,
            &ledger_id,
            actor,
            evidence_ref,
            payload_sha256,
            now_ms,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn settle_provider_turn_ledger_at_bounds_in_tx(
    connection: &mut SqliteConnection,
    turn_reservation_id: &str,
    actor: &str,
    evidence_ref: &str,
    now_ms: i64,
) -> Result<(), Error> {
    let ledger_ids = provider_turn_ledger_ids_in_tx(connection, turn_reservation_id).await?;
    for (_, ledger_id) in ledger_ids {
        let reservation = load_reservation(connection, &ledger_id)
            .await?
            .ok_or(Error::NotFound)?;
        let digest = sha256_hex(
            format!(
                "provider-turn-resolution:v2:{turn_reservation_id}:{ledger_id}:{}",
                reservation.upper_bound_micros.get()
            )
            .as_bytes(),
        );
        settle_or_reconcile_in_tx(
            connection,
            &ledger_id,
            reservation.upper_bound_micros,
            &digest,
            actor,
            evidence_ref,
            now_ms,
        )
        .await?;
    }
    Ok(())
}

/// Envelope lookup for a Lead request. The operator request identity may be the
/// Lead request itself or the unified-intake request that owns it.
pub(crate) async fn envelope_for_lead_in_tx(
    connection: &mut SqliteConnection,
    lead_request_id: &str,
) -> Result<Option<String>, Error> {
    let direct: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM inference_envelopes WHERE request_id=?1")
            .bind(lead_request_id)
            .fetch_optional(&mut *connection)
            .await?;
    let intake: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT e.id FROM intake_requests ir
         JOIN inference_envelopes e ON e.request_id=ir.id
         WHERE ir.lead_request_id=?1",
    )
    .bind(lead_request_id)
    .fetch_optional(&mut *connection)
    .await?;
    match (direct, intake) {
        (Some((direct,)), Some((intake,))) if direct != intake => Err(Error::Conflict(
            "lead request resolves to more than one inference envelope",
        )),
        (Some((id,)), _) | (_, Some((id,))) => Ok(Some(id)),
        (None, None) => Ok(None),
    }
}

pub(crate) async fn envelope_for_campaign_in_tx(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Option<String>, Error> {
    let row: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT envelope_id FROM inference_campaign_bindings WHERE campaign_id=?1",
    )
    .bind(campaign_id)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(row.map(|(id,)| id))
}

pub(crate) async fn envelope_for_request_in_tx(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Option<String>, Error> {
    Ok(load_envelope_by_request(connection, request_id)
        .await?
        .map(|envelope| envelope.id))
}

async fn insert_envelope(
    connection: &mut SqliteConnection,
    envelope_id: &str,
    input: &NewInferenceEnvelope,
    now_ms: i64,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO inference_envelopes
         (id,request_id,project_id,currency,state,authorized_micros,authorization_revision,
          created_at_ms,revoked_at_ms)
         VALUES (?1,?2,?3,'USD','ACTIVE',?4,1,?5,NULL)",
    )
    .bind(envelope_id)
    .bind(&input.request_id)
    .bind(&input.project_id)
    .bind(micros_to_i64(input.authorized_micros)?)
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_authorization(
    connection: &mut SqliteConnection,
    envelope_id: &str,
    revision: i64,
    limit_micros: UsdMicros,
    actor: &str,
    scope_sha256: &str,
    evidence_sha256: &str,
    idempotency_key: &str,
    now_ms: i64,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO inference_authorizations
         (id,envelope_id,revision,limit_micros,actor,scope_sha256,evidence_sha256,
          idempotency_key,created_at_ms)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(envelope_id)
    .bind(revision)
    .bind(micros_to_i64(limit_micros)?)
    .bind(actor)
    .bind(scope_sha256)
    .bind(evidence_sha256)
    .bind(idempotency_key)
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_ledger_event(
    connection: &mut SqliteConnection,
    envelope_id: &str,
    reservation_id: Option<&str>,
    kind: &str,
    actor: &str,
    evidence_ref: &str,
    payload_sha256: &str,
    now_ms: i64,
) -> Result<(), Error> {
    if !is_sha256(payload_sha256) {
        return Err(Error::Validation(
            "inference ledger event payload digest".into(),
        ));
    }
    if evidence_ref.trim().is_empty() || evidence_ref.len() > 512 || actor.trim().is_empty() {
        return Err(Error::Validation("inference ledger event evidence".into()));
    }
    sqlx::query::query(
        "INSERT INTO inference_ledger_events
         (id,envelope_id,reservation_id,kind,actor,evidence_ref,payload_sha256,created_at_ms)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(envelope_id)
    .bind(reservation_id)
    .bind(kind)
    .bind(actor)
    .bind(evidence_ref)
    .bind(payload_sha256)
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn mark_envelope_overdrawn_if_needed(
    connection: &mut SqliteConnection,
    reservation: &ReservationRow,
    settled_micros: UsdMicros,
    evidence_sha256: &str,
    now_ms: i64,
) -> Result<(), Error> {
    let envelope = load_envelope(connection, &reservation.envelope_id)
        .await?
        .ok_or(Error::NotFound)?;
    if !matches!(envelope.state, EnvelopeState::Active) {
        return Ok(());
    }
    let amounts = load_envelope_amounts(connection, &envelope.id).await?;
    let totals = spending_totals(envelope.authorized_micros, &amounts).map_err(ledger_error)?;
    if settled_micros <= reservation.upper_bound_micros
        && totals.settled_micros <= envelope.authorized_micros
    {
        return Ok(());
    }
    sqlx::query::query("UPDATE inference_envelopes SET state='OVERDRAWN' WHERE id=?1")
        .bind(&envelope.id)
        .execute(&mut *connection)
        .await?;
    insert_ledger_event(
        connection,
        &envelope.id,
        Some(&reservation.id),
        "OVERDRAWN",
        "core:settlement",
        &format!("reservation:{}", reservation.id),
        evidence_sha256,
        now_ms,
    )
    .await?;
    Ok(())
}

async fn expire_inference_in_tx(
    connection: &mut SqliteConnection,
    now_ms: i64,
) -> Result<(), Error> {
    let expired: Vec<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT id,envelope_id,request_sha256 FROM inference_reservations
         WHERE state='RESERVED' AND expires_at_ms<=?1",
    )
    .bind(now_ms)
    .fetch_all(&mut *connection)
    .await?;
    for (reservation_id, envelope_id, request_sha256) in expired {
        let changed = sqlx::query::query(
            "UPDATE inference_reservations SET state='AMBIGUOUS',updated_at_ms=?1
             WHERE id=?2 AND state='RESERVED'",
        )
        .bind(now_ms)
        .bind(&reservation_id)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if changed == 1 {
            insert_ledger_event(
                connection,
                &envelope_id,
                Some(&reservation_id),
                "AMBIGUOUS",
                "core:expiry",
                &format!("expiry:{reservation_id}"),
                &request_sha256,
                now_ms,
            )
            .await?;
        }
    }
    Ok(())
}

async fn spending_status_in_tx(
    connection: &mut SqliteConnection,
    envelope_id: &str,
    now_ms: i64,
) -> Result<SpendingStatus, Error> {
    let envelope = load_envelope(connection, envelope_id)
        .await?
        .ok_or(Error::NotFound)?;
    let rows = load_envelope_reservations(connection, envelope_id).await?;
    let amounts: Vec<ReservationAmounts> = rows.iter().map(ReservationRow::amounts).collect();
    let pricing_revisions = load_pricing_revisions(connection, envelope_id).await?;
    let as_of = chrono::DateTime::<Utc>::from_timestamp_millis(now_ms)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    project_spending(&envelope.snapshot(), &amounts, &pricing_revisions, &as_of)
        .map_err(ledger_error)
}

async fn load_pricing_revisions(
    connection: &mut SqliteConnection,
    envelope_id: &str,
) -> Result<Vec<String>, Error> {
    let revisions: Vec<(String,)> = sqlx::query_as::query_as(
        "SELECT DISTINCT pricing_revision FROM inference_reservations
         WHERE envelope_id=?1 ORDER BY pricing_revision",
    )
    .bind(envelope_id)
    .fetch_all(&mut *connection)
    .await?;
    Ok(revisions.into_iter().map(|(value,)| value).collect())
}

async fn load_envelope_amounts(
    connection: &mut SqliteConnection,
    envelope_id: &str,
) -> Result<Vec<ReservationAmounts>, Error> {
    Ok(load_envelope_reservations(connection, envelope_id)
        .await?
        .iter()
        .map(ReservationRow::amounts)
        .collect())
}

async fn load_envelope_reservations(
    connection: &mut SqliteConnection,
    envelope_id: &str,
) -> Result<Vec<ReservationRow>, Error> {
    let rows = sqlx::query::query(
        "SELECT id,envelope_id,operation_id,idempotency_key,request_sha256,role,
                authorization_revision,scope_sha256,principal_id,parent_operation_id,provider,model,
                pricing_revision,pricing_snapshot_json,upper_bound_micros,settled_micros,state,
                worker_id,generation,dispatched_at_ms,provider_request_id,expires_at_ms,
                settlement_sha256,created_at_ms,updated_at_ms
         FROM inference_reservations WHERE envelope_id=?1 ORDER BY created_at_ms,id",
    )
    .bind(envelope_id)
    .fetch_all(&mut *connection)
    .await?;
    rows.into_iter()
        .map(|row| reservation_from_row(&row))
        .collect()
}

async fn load_envelope(
    connection: &mut SqliteConnection,
    envelope_id: &str,
) -> Result<Option<EnvelopeRow>, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,project_id,currency,state,authorized_micros,
                authorization_revision,created_at_ms,revoked_at_ms
         FROM inference_envelopes WHERE id=?1",
    )
    .bind(envelope_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| envelope_from_row(&row)).transpose()
}

async fn find_envelope_for_request_alias(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Option<String>, Error> {
    let direct: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM inference_envelopes WHERE request_id=?1")
            .bind(request_id)
            .fetch_optional(&mut *connection)
            .await?;
    let via_lead: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT e.id FROM lead_requests lr
         LEFT JOIN intake_requests ir ON ir.lead_request_id=lr.id
         JOIN inference_envelopes e ON e.request_id IN (lr.id, COALESCE(ir.id,''))
         WHERE lr.id=?1 LIMIT 1",
    )
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let via_intake: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT e.id FROM intake_requests ir
         JOIN inference_envelopes e ON e.request_id IN (ir.id, ir.lead_request_id)
         WHERE ir.id=?1 LIMIT 1",
    )
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let mut found: Vec<String> = [direct, via_lead, via_intake]
        .into_iter()
        .flatten()
        .map(|(id,)| id)
        .collect();
    found.sort();
    found.dedup();
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(Error::Conflict(
            "request identity aliases resolve to more than one inference envelope",
        )),
    }
}

async fn load_envelope_by_request(
    connection: &mut SqliteConnection,
    request_id: &str,
) -> Result<Option<EnvelopeRow>, Error> {
    let row = sqlx::query::query(
        "SELECT id,request_id,project_id,currency,state,authorized_micros,
                authorization_revision,created_at_ms,revoked_at_ms
         FROM inference_envelopes WHERE request_id=?1",
    )
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| envelope_from_row(&row)).transpose()
}

async fn load_envelope_for_campaign(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<Option<EnvelopeRow>, Error> {
    let Some(envelope_id) = envelope_for_campaign_in_tx(connection, campaign_id).await? else {
        return Ok(None);
    };
    load_envelope(connection, &envelope_id).await
}

async fn load_reservation(
    connection: &mut SqliteConnection,
    reservation_id: &str,
) -> Result<Option<ReservationRow>, Error> {
    let row = sqlx::query::query(
        "SELECT id,envelope_id,operation_id,idempotency_key,request_sha256,role,
                authorization_revision,scope_sha256,principal_id,parent_operation_id,provider,model,
                pricing_revision,pricing_snapshot_json,upper_bound_micros,settled_micros,state,
                worker_id,generation,dispatched_at_ms,provider_request_id,expires_at_ms,
                settlement_sha256,created_at_ms,updated_at_ms
         FROM inference_reservations WHERE id=?1",
    )
    .bind(reservation_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| reservation_from_row(&row)).transpose()
}

async fn load_reservation_by_identity(
    connection: &mut SqliteConnection,
    envelope_id: &str,
    idempotency_key: &str,
    operation_id: &str,
) -> Result<Option<ReservationRow>, Error> {
    let row = sqlx::query::query(
        "SELECT id,envelope_id,operation_id,idempotency_key,request_sha256,role,
                authorization_revision,scope_sha256,principal_id,parent_operation_id,provider,model,
                pricing_revision,pricing_snapshot_json,upper_bound_micros,settled_micros,state,
                worker_id,generation,dispatched_at_ms,provider_request_id,expires_at_ms,
                settlement_sha256,created_at_ms,updated_at_ms
         FROM inference_reservations
         WHERE envelope_id=?1 AND (idempotency_key=?2 OR operation_id=?3)
         LIMIT 1",
    )
    .bind(envelope_id)
    .bind(idempotency_key)
    .bind(operation_id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| reservation_from_row(&row)).transpose()
}

#[derive(Debug)]
struct AuthorizationRow {
    envelope_id: String,
    limit_micros: UsdMicros,
    actor: String,
    scope_sha256: String,
    evidence_sha256: String,
}

pub(crate) async fn load_ledger_mode(
    connection: &mut SqliteConnection,
) -> Result<InferenceLedgerMode, Error> {
    let (mode,): (String,) =
        sqlx::query_as::query_as("SELECT mode FROM inference_ledger_policy WHERE id=1")
            .fetch_one(&mut *connection)
            .await?;
    InferenceLedgerMode::parse(&mode).map_err(ledger_error)
}

pub(crate) async fn campaign_requirement_in_tx(
    connection: &mut SqliteConnection,
    campaign_id: &str,
) -> Result<CampaignInferenceRequirement, Error> {
    let row: Option<(String,)> =
        sqlx::query_as::query_as("SELECT mode FROM inference_campaign_modes WHERE campaign_id=?1")
            .bind(campaign_id)
            .fetch_optional(&mut *connection)
            .await?;
    if row.as_ref().is_some_and(|(mode,)| mode == "V2") {
        let binding: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT envelope_id FROM inference_campaign_bindings WHERE campaign_id=?1",
        )
        .bind(campaign_id)
        .fetch_optional(&mut *connection)
        .await?;
        let Some((envelope_id,)) = binding else {
            return Err(Error::Forbidden(
                "V2 campaign has no inference envelope".into(),
            ));
        };
        let envelope = load_envelope(connection, &envelope_id)
            .await?
            .ok_or(Error::NotFound)?;
        if envelope.state != EnvelopeState::Active {
            return Err(Error::Forbidden(
                "inference envelope is not active for new campaign spending".into(),
            ));
        }
        return Ok(CampaignInferenceRequirement::EnvelopeBound { envelope_id });
    }
    if load_ledger_mode(connection).await? == InferenceLedgerMode::V2 {
        return Err(Error::Forbidden(
            "V2 execution policy requires a bound inference envelope".into(),
        ));
    }
    Ok(CampaignInferenceRequirement::LegacyV1)
}

async fn classify_campaign_v2(
    connection: &mut SqliteConnection,
    campaign_id: &str,
    evidence_sha256: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO inference_campaign_modes(campaign_id,mode,classified_at_ms,evidence_sha256)
         VALUES (?1,'V2',?2,?3)
         ON CONFLICT(campaign_id) DO UPDATE SET mode='V2',
             classified_at_ms=excluded.classified_at_ms,
             evidence_sha256=excluded.evidence_sha256
         WHERE inference_campaign_modes.mode='V1'",
    )
    .bind(campaign_id)
    .bind(Utc::now().timestamp_millis())
    .bind(evidence_sha256)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn campaign_owns_envelope_request(
    connection: &mut SqliteConnection,
    campaign_id: &str,
    envelope_request_id: &str,
) -> Result<bool, Error> {
    let lead_request: Option<(String,)> =
        sqlx::query_as::query_as("SELECT request_id FROM lead_start_evidence WHERE campaign_id=?1")
            .bind(campaign_id)
            .fetch_optional(&mut *connection)
            .await?;
    let intake_request: Option<(String,)> =
        sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE campaign_id=?1")
            .bind(campaign_id)
            .fetch_optional(&mut *connection)
            .await?;
    if let Some((lead_request_id,)) = lead_request {
        if envelope_request_id == lead_request_id {
            return Ok(true);
        }
        let linked: Option<(String,)> =
            sqlx::query_as::query_as("SELECT id FROM intake_requests WHERE lead_request_id=?1")
                .bind(&lead_request_id)
                .fetch_optional(&mut *connection)
                .await?;
        if linked
            .as_ref()
            .is_some_and(|(id,)| id == envelope_request_id)
        {
            return Ok(true);
        }
    }
    if let Some((intake_request_id,)) = intake_request
        && envelope_request_id == intake_request_id
    {
        return Ok(true);
    }
    Ok(false)
}

async fn load_pricing_quote_by_revision(
    connection: &mut SqliteConnection,
    provider: &str,
    model: &str,
    revision: i64,
) -> Result<Option<InferencePricingQuote>, Error> {
    let row = sqlx::query::query(
        "SELECT quote_id,provider,model,revision,provider_revision,auth_mode,input_tokens,
                output_tokens,upper_bound_micros,coverage_sha256,snapshot_json,evidence_sha256
         FROM inference_pricing_quotes
         WHERE provider=?1 AND model=?2 AND revision=?3",
    )
    .bind(provider)
    .bind(model)
    .bind(revision)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| pricing_quote_from_row(&row)).transpose()
}

async fn load_pricing_quote(
    connection: &mut SqliteConnection,
    quote_id: &str,
    provider: &str,
    model: &str,
) -> Result<Option<InferencePricingQuote>, Error> {
    let row = sqlx::query::query(
        "SELECT quote_id,provider,model,revision,provider_revision,auth_mode,input_tokens,
                output_tokens,upper_bound_micros,coverage_sha256,snapshot_json,evidence_sha256
         FROM inference_pricing_quotes
         WHERE quote_id=?1 AND provider=?2 AND model=?3",
    )
    .bind(quote_id)
    .bind(provider)
    .bind(model)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| pricing_quote_from_row(&row)).transpose()
}

pub(crate) async fn latest_pricing_quote_in_tx(
    connection: &mut SqliteConnection,
    provider: &str,
    model: &str,
) -> Result<InferencePricingQuote, Error> {
    load_latest_pricing_quote(connection, provider, model)
        .await?
        .ok_or(Error::NotFound)
}

async fn load_latest_pricing_quote(
    connection: &mut SqliteConnection,
    provider: &str,
    model: &str,
) -> Result<Option<InferencePricingQuote>, Error> {
    let row = sqlx::query::query(
        "SELECT q.quote_id,q.provider,q.model,q.revision,q.provider_revision,q.auth_mode,
                q.input_tokens,q.output_tokens,q.upper_bound_micros,q.coverage_sha256,
                q.snapshot_json,q.evidence_sha256
         FROM inference_pricing_quotes q
         JOIN provider_settings ps ON ps.provider=q.provider AND ps.model=q.model
         WHERE q.provider=?1 AND q.model=?2 AND ps.enabled=1 AND ps.auth_mode='api_key'
           AND q.provider_revision=ps.revision
         ORDER BY q.revision DESC LIMIT 1",
    )
    .bind(provider)
    .bind(model)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(|row| pricing_quote_from_row(&row)).transpose()
}

fn pricing_quote_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<InferencePricingQuote, Error> {
    Ok(InferencePricingQuote {
        quote_id: row.try_get("quote_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        revision: row.try_get("revision")?,
        provider_revision: row.try_get("provider_revision")?,
        auth_mode: row.try_get("auth_mode")?,
        input_tokens: u64::try_from(row.try_get::<i64, _>("input_tokens")?)
            .map_err(|_| Error::Validation("stored input token envelope".into()))?,
        output_tokens: u64::try_from(row.try_get::<i64, _>("output_tokens")?)
            .map_err(|_| Error::Validation("stored output token envelope".into()))?,
        upper_bound_micros: micros_from_i64(row.try_get("upper_bound_micros")?)?,
        coverage_sha256: row.try_get("coverage_sha256")?,
        snapshot_json: row.try_get("snapshot_json")?,
        evidence_sha256: row.try_get("evidence_sha256")?,
    })
}

async fn load_authorization_scope(
    connection: &mut SqliteConnection,
    envelope_id: &str,
    revision: i64,
) -> Result<Option<String>, Error> {
    let row: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT scope_sha256 FROM inference_authorizations
         WHERE envelope_id=?1 AND revision=?2",
    )
    .bind(envelope_id)
    .bind(revision)
    .fetch_optional(&mut *connection)
    .await?;
    Ok(row.map(|(scope,)| scope))
}

/// Durable authorization revision and frozen scope for a request envelope.
pub(crate) async fn envelope_authorization_context_in_tx(
    connection: &mut SqliteConnection,
    envelope_id: &str,
) -> Result<Option<(i64, String)>, Error> {
    let envelope = load_envelope(connection, envelope_id).await?;
    let Some(envelope) = envelope else {
        return Ok(None);
    };
    let Some(scope) =
        load_authorization_scope(connection, envelope_id, envelope.authorization_revision).await?
    else {
        return Ok(None);
    };
    Ok(Some((envelope.authorization_revision, scope)))
}

async fn load_authorization_by_key(
    connection: &mut SqliteConnection,
    idempotency_key: &str,
) -> Result<Option<AuthorizationRow>, Error> {
    let row = sqlx::query::query(
        "SELECT envelope_id,limit_micros,actor,scope_sha256,evidence_sha256
         FROM inference_authorizations WHERE idempotency_key=?1",
    )
    .bind(idempotency_key)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(AuthorizationRow {
        envelope_id: row.try_get("envelope_id")?,
        limit_micros: micros_from_i64(row.try_get("limit_micros")?)?,
        actor: row.try_get("actor")?,
        scope_sha256: row.try_get("scope_sha256")?,
        evidence_sha256: row.try_get("evidence_sha256")?,
    }))
}

fn envelope_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<EnvelopeRow, Error> {
    let state: String = row.try_get("state")?;
    Ok(EnvelopeRow {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        project_id: row.try_get("project_id")?,
        currency: row.try_get("currency")?,
        state: crate::inference_ledger::parse_envelope_state(&state).map_err(ledger_error)?,
        authorized_micros: micros_from_i64(row.try_get("authorized_micros")?)?,
        authorization_revision: row.try_get("authorization_revision")?,
        created_at_ms: row.try_get("created_at_ms")?,
        revoked_at_ms: row.try_get("revoked_at_ms")?,
    })
}

fn reservation_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<ReservationRow, Error> {
    let role: String = row.try_get("role")?;
    let state: String = row.try_get("state")?;
    Ok(ReservationRow {
        id: row.try_get("id")?,
        envelope_id: row.try_get("envelope_id")?,
        operation_id: row.try_get("operation_id")?,
        idempotency_key: row.try_get("idempotency_key")?,
        request_sha256: row.try_get("request_sha256")?,
        authorization_revision: row.try_get("authorization_revision")?,
        scope_sha256: row.try_get("scope_sha256")?,
        role: crate::inference_ledger::parse_role(&role).map_err(ledger_error)?,
        principal_id: row.try_get("principal_id")?,
        parent_operation_id: row.try_get("parent_operation_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        pricing_revision: row.try_get("pricing_revision")?,
        pricing_snapshot_json: row.try_get("pricing_snapshot_json")?,
        upper_bound_micros: micros_from_i64(row.try_get("upper_bound_micros")?)?,
        settled_micros: row
            .try_get::<Option<i64>, _>("settled_micros")?
            .map(micros_from_i64)
            .transpose()?,
        state: crate::inference_ledger::parse_reservation_state(&state).map_err(ledger_error)?,
        worker_id: row.try_get("worker_id")?,
        generation: row.try_get("generation")?,
        dispatched_at_ms: row.try_get("dispatched_at_ms")?,
        provider_request_id: row.try_get("provider_request_id")?,
        expires_at_ms: row.try_get("expires_at_ms")?,
        settlement_sha256: row.try_get("settlement_sha256")?,
        created_at_ms: row.try_get("created_at_ms")?,
        updated_at_ms: row.try_get("updated_at_ms")?,
    })
}

fn micros_to_i64(value: UsdMicros) -> Result<i64, Error> {
    i64::try_from(value.get())
        .map_err(|_| Error::Validation("inference amount exceeds SQLite INTEGER".into()))
}

fn micros_from_i64(value: i64) -> Result<UsdMicros, Error> {
    u64::try_from(value)
        .map_err(|_| Error::Validation("stored inference amount is negative".into()))
        .and_then(|value| {
            UsdMicros::new(value)
                .map_err(|_| Error::Validation("stored inference amount overflows".into()))
        })
}

fn ledger_error(error: InferenceLedgerError) -> Error {
    match error {
        InferenceLedgerError::NotFound => Error::NotFound,
        InferenceLedgerError::NotActive => {
            Error::Forbidden("inference envelope is not active".into())
        }
        InferenceLedgerError::Exhausted => {
            Error::Forbidden("inference spending authorization is exhausted".into())
        }
        InferenceLedgerError::Conflict => {
            Error::Conflict("inference ledger operation conflicts with its recorded identity")
        }
        InferenceLedgerError::Ambiguous => {
            Error::Forbidden("inference outcome is ambiguous; reconciliation is required".into())
        }
        InferenceLedgerError::Invalid(message) => Error::Validation(message),
    }
}

fn envelope_state_str(state: EnvelopeState) -> &'static str {
    crate::inference_ledger::envelope_state_str(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference_ledger::{
        InferencePricingQuote, NewInferenceAuthorization, NewInferenceEnvelope,
        NewInferencePricingQuote, ReconcileInferenceInput, ReconciliationOutcome,
        RevokeInferenceEnvelopeInput,
    };
    use sqlx_sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::path::{Path, PathBuf};

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()))
    }

    async fn seed_project(db: &Database, project_id: &str) {
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES (?1,?2,?3,?4,'')",
        )
        .bind(project_id)
        .bind(format!("/{project_id}"))
        .bind("base")
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
    }

    async fn seed_worker_campaign(db: &Database) {
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES ('action-fence','project','objective','opencode-go',
                     'deepseek-v4.1-flash','R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES ('campaign-fence','action-fence','AUTHORIZED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO inference_campaign_modes(campaign_id,mode,classified_at_ms,evidence_sha256)
             VALUES ('campaign-fence','V2',?1,?2)",
        )
        .bind(Utc::now().timestamp_millis())
        .bind("c".repeat(64))
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query("INSERT INTO runs VALUES ('run-fence','campaign-fence','QUEUED',?1)")
            .bind(&now)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at)
             VALUES ('attempt-fence','run-fence','LEASED',?1)",
        )
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO worker_leases(attempt_id,worker_id,generation,expires_at_ms)
             VALUES ('attempt-fence','worker-1',3,?1)",
        )
        .bind(Utc::now().timestamp_millis().saturating_add(60_000))
        .execute(&db.pool)
        .await
        .unwrap();
    }

    async fn create_envelope(
        db: &Database,
        request_id: &str,
        authorized_micros: u64,
    ) -> (String, String) {
        let key = format!("envelope-{request_id}");
        let envelope = db
            .create_inference_envelope(&NewInferenceEnvelope {
                request_id: request_id.into(),
                project_id: "project".into(),
                authorized_micros: UsdMicros::new(authorized_micros).unwrap(),
                actor: "operator".into(),
                scope_sha256: "a".repeat(64),
                evidence_sha256: "b".repeat(64),
                idempotency_key: key.clone(),
            })
            .await
            .unwrap();
        (envelope.envelope_id, key)
    }

    async fn seed_campaign_lineage(
        db: &Database,
        project_id: &str,
        action_id: &str,
        campaign_id: &str,
        request_id: &str,
    ) {
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT OR IGNORE INTO projects(id,repository,base_commit,created_at,identity)
             VALUES (?1,?2,'base',?3,'')",
        )
        .bind(project_id)
        .bind(format!("/{project_id}"))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT OR IGNORE INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES (?1,?2,'objective','opencode-go','deepseek-v4.1-flash','R0','AUTHORIZED',
                     'hash',?3)",
        )
        .bind(action_id)
        .bind(project_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT OR IGNORE INTO campaigns(id,action_id,status,created_at)
             VALUES (?1,?2,'AUTHORIZED',?3)",
        )
        .bind(campaign_id)
        .bind(action_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        let delegation_id = format!("delegation-{request_id}");
        sqlx::query::query(
            "INSERT OR IGNORE INTO standing_delegations
             (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
             VALUES (?1,'worker','audit',?2,1,'hash','{}',?3)",
        )
        .bind(&delegation_id)
        .bind(project_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT OR IGNORE INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,
              state,created_at,updated_at)
             VALUES (?1,?2,?3,?4,'objective','hash','hash','FROZEN',?5,?5)",
        )
        .bind(request_id)
        .bind(project_id)
        .bind(action_id)
        .bind(&delegation_id)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT OR IGNORE INTO lead_start_evidence
             (request_id,campaign_id,mode,principal_id,principal_label,rationale,
              idempotency_key,request_hash,plan_hash,profile_revision_id,profile_hash,
              spec_sha256,config_hash,delegation_hash,policy_hash,authorization_digest,created_at)
             VALUES (?1,?2,'OPERATOR','operator',NULL,'test',?3,?4,?4,NULL,NULL,?4,?4,?4,?4,?4,?5)",
        )
        .bind(request_id)
        .bind(campaign_id)
        .bind(format!("lead-start-{campaign_id}"))
        .bind("a".repeat(64))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    async fn test_quote(db: &Database, bound: u64) -> InferencePricingQuote {
        assert_eq!(bound % 2, 0, "test quote bounds must be even");
        let now = Utc::now().to_rfc3339();
        let (revision,): (i64,) = sqlx::query_as::query_as(
            "SELECT COALESCE(MAX(revision),0)+1 FROM inference_pricing_quotes",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO provider_settings
             (provider,kind,model,base_url,auth_mode,credential_ciphertext,credential_nonce,
              credential_fingerprint,enabled,is_default,max_turns,max_input_tokens,
              max_output_tokens,max_cost_micros,input_micros_per_million,
              output_micros_per_million,revision,updated_at)
             VALUES ('opencode-go','opencode','deepseek-v4.1-flash','http://127.0.0.1:9','api_key',
                     'cipher','nonce','fingerprint',1,0,1,?1,?2,?3,1000000,0,?4,?5)
             ON CONFLICT(provider) DO UPDATE SET
               kind=excluded.kind,model=excluded.model,max_input_tokens=excluded.max_input_tokens,
               max_output_tokens=excluded.max_output_tokens,max_cost_micros=excluded.max_cost_micros,
               input_micros_per_million=excluded.input_micros_per_million,
               output_micros_per_million=excluded.output_micros_per_million,
               revision=excluded.revision,updated_at=excluded.updated_at",
        )
        .bind(i64::try_from(bound / 2).unwrap())
        .bind(i64::try_from(bound / 2).unwrap())
        .bind(i64::try_from(bound).unwrap())
        .bind(1_i64)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        db.create_inference_pricing_quote(&NewInferencePricingQuote {
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            revision,
            evidence_sha256: "e".repeat(64),
            actor: "operator".into(),
        })
        .await
        .unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn reserve_input(
        quote: &InferencePricingQuote,
        envelope_id: &str,
        operation_id: &str,
        idempotency_key: &str,
        digest: &str,
        bound: u64,
        role: InferenceRole,
        expires_at_ms: i64,
    ) -> ReserveInferenceInput {
        assert_eq!(
            quote.upper_bound_micros,
            UsdMicros::new(bound).expect("test quote bound")
        );
        ReserveInferenceInput {
            envelope_id: envelope_id.into(),
            operation_id: operation_id.into(),
            idempotency_key: idempotency_key.into(),
            request_sha256: digest.into(),
            role,
            principal_id: "principal".into(),
            parent_operation_id: None,
            provider: quote.provider.clone(),
            model: quote.model.clone(),
            pricing_revision: quote.quote_id.clone(),
            pricing_snapshot_json: quote.snapshot_json.clone(),
            upper_bound_micros: quote.upper_bound_micros,
            expires_at_ms,
            authorization_revision: 1,
            scope_sha256: "a".repeat(64),
            worker_id: None,
            generation: None,
        }
    }

    fn future_ms(offset: i64) -> i64 {
        Utc::now().timestamp_millis().saturating_add(offset)
    }

    #[tokio::test]
    async fn independent_connections_never_oversubscribe() {
        let path = temp_path("factory-ledger-race");
        let first = Database::open(&path).await.unwrap();
        let second = Database::open(&path).await.unwrap();
        seed_project(&first, "project").await;
        let (envelope_id, _) = create_envelope(&first, "request-race", 10_000).await;
        let quote = test_quote(&first, 6000).await;
        let expires = future_ms(60_000);
        let left = reserve_input(
            &quote,
            &envelope_id,
            "operation-left",
            "idempotency-left",
            &"c".repeat(64),
            6_000,
            InferenceRole::Lead,
            expires,
        );
        let right = reserve_input(
            &quote,
            &envelope_id,
            "operation-right",
            "idempotency-right",
            &"d".repeat(64),
            6_000,
            InferenceRole::Implementer,
            expires,
        );
        let (left_result, right_result) = tokio::join!(
            first.reserve_inference(&left),
            second.reserve_inference(&right)
        );
        let admitted = [&left_result, &right_result]
            .into_iter()
            .filter(|result| result.is_ok())
            .count();
        let refused = [&left_result, &right_result]
            .into_iter()
            .filter(|result| result.is_err())
            .count();
        assert_eq!(admitted, 1, "{left_result:?} {right_result:?}");
        assert_eq!(refused, 1, "{left_result:?} {right_result:?}");
        let status = first.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.reserved_micros.get(), 6_000);
        assert_eq!(status.available_micros.get(), 4_000);
        first.close().await;
        second.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn operation_replay_is_durable_and_digest_conflicts() {
        let path = temp_path("factory-ledger-idempotency");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-idempotency", 10_000).await;
        let quote = test_quote(&db, 2000).await;
        let input = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            2_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        let first = db.reserve_inference(&input).await.unwrap();
        let replay = db.reserve_inference(&input).await.unwrap();
        assert_eq!(replay.reservation_id, first.reservation_id);
        assert_eq!(replay.state, ReservationState::Reserved);
        assert_eq!(replay.idempotency_key, input.idempotency_key);
        assert_eq!(replay.request_sha256, input.request_sha256);
        assert_eq!(replay.authorization_revision, 1);
        let detail = db
            .inference_reservation(&replay.reservation_id)
            .await
            .unwrap();
        assert_eq!(detail.scope_sha256, "a".repeat(64));

        let mut conflicting_digest = input.clone();
        conflicting_digest.request_sha256 = "b".repeat(64);
        assert!(matches!(
            db.reserve_inference(&conflicting_digest).await,
            Err(Error::Conflict(_))
        ));
        let mut conflicting_identity = input.clone();
        conflicting_identity.operation_id = "other-operation".into();
        conflicting_identity.authorization_revision = 2;
        assert!(matches!(
            db.reserve_inference(&conflicting_identity).await,
            Err(Error::Conflict(_))
        ));
        let quote_alt = test_quote(&db, 1_000).await;
        let mut wrong_scope = reserve_input(
            &quote_alt,
            &envelope_id,
            "operation-scope",
            "idempotency-scope",
            &"d".repeat(64),
            1_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        wrong_scope.scope_sha256 = "e".repeat(64);
        assert!(matches!(
            db.reserve_inference(&wrong_scope).await,
            Err(Error::Conflict(_))
        ));
        let mut stale_revision = wrong_scope;
        stale_revision.operation_id = "operation-revision".into();
        stale_revision.idempotency_key = "idempotency-revision".into();
        stale_revision.authorization_revision = 2;
        assert!(matches!(
            db.reserve_inference(&stale_revision).await,
            Err(Error::Conflict(_))
        ));
        let status = db.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.reserved_micros.get(), 2_000);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn settlement_is_idempotent_and_conflicts_on_different_values() {
        let path = temp_path("factory-ledger-settlement");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-settlement", 10_000).await;
        let quote = test_quote(&db, 2000).await;
        let reservation = db
            .reserve_inference(&reserve_input(
                &quote,
                &envelope_id,
                "operation",
                "idempotency",
                &"a".repeat(64),
                2_000,
                InferenceRole::SemanticReviewer,
                future_ms(60_000),
            ))
            .await
            .unwrap();
        let settlement = InferenceSettlement {
            reservation_id: reservation.reservation_id.clone(),
            settled_micros: UsdMicros::new(1_250).unwrap(),
            usage_evidence_sha256: "e".repeat(64),
        };
        let settled = db.settle_inference(&settlement).await.unwrap();
        assert_eq!(settled.state, ReservationState::Settled);
        assert_eq!(settled.settled_micros, Some(UsdMicros::new(1_250).unwrap()));
        let replay = db.settle_inference(&settlement).await.unwrap();
        assert_eq!(replay.settled_micros, settled.settled_micros);

        let mut different_amount = settlement.clone();
        different_amount.settled_micros = UsdMicros::new(1_000).unwrap();
        assert!(matches!(
            db.settle_inference(&different_amount).await,
            Err(Error::Conflict(_))
        ));
        let mut different_evidence = settlement.clone();
        different_evidence.usage_evidence_sha256 = "f".repeat(64);
        assert!(matches!(
            db.settle_inference(&different_evidence).await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            db.release_inference(&reservation.reservation_id, "operator", "attempted release")
                .await,
            Err(Error::Conflict(_))
        ));
        let status = db.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.settled_micros.get(), 1_250);
        assert_eq!(status.reserved_micros.get(), 0);
        assert!(
            sqlx::query::query("UPDATE inference_reservations SET state='RESERVED' WHERE id=?1")
                .bind(&settled.reservation_id)
                .execute(&db.pool)
                .await
                .is_err(),
            "terminal settlement cannot be rewound"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn over_bound_charge_overdraws_and_blocks_further_spending() {
        let path = temp_path("factory-ledger-overdrawn");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-overdrawn", 10_000).await;
        let quote = test_quote(&db, 1000).await;
        let quote_alt = test_quote(&db, 100).await;
        let reservation = db
            .reserve_inference(&reserve_input(
                &quote,
                &envelope_id,
                "operation",
                "idempotency",
                &"a".repeat(64),
                1_000,
                InferenceRole::Evaluation,
                future_ms(60_000),
            ))
            .await
            .unwrap();
        db.settle_inference(&InferenceSettlement {
            reservation_id: reservation.reservation_id,
            settled_micros: UsdMicros::new(1_500).unwrap(),
            usage_evidence_sha256: "e".repeat(64),
        })
        .await
        .unwrap();
        let status = db.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.state, EnvelopeState::Overdrawn);
        assert_eq!(status.settled_micros.get(), 1_500);
        assert_eq!(status.overrun_micros.get(), 500);
        assert_eq!(
            db.reserve_inference(&reserve_input(
                &quote_alt,
                &envelope_id,
                "operation-two",
                "idempotency-two",
                &"b".repeat(64),
                100,
                InferenceRole::Auxiliary,
                future_ms(60_000),
            ))
            .await
            .unwrap_err()
            .to_string(),
            "forbidden: inference spending authorization is exhausted"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revocation_blocks_new_reservations_and_preserves_late_settlement() {
        let path = temp_path("factory-ledger-revocation");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-revocation", 10_000).await;
        let quote = test_quote(&db, 4000).await;
        let quote_alt = test_quote(&db, 100).await;
        let mut evidence_turn = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            4_000,
            InferenceRole::Implementer,
            future_ms(60_000),
        );
        evidence_turn.worker_id = Some("worker".into());
        evidence_turn.generation = Some(1);
        let reservation = db.reserve_inference(&evidence_turn).await.unwrap();
        let revoke = RevokeInferenceEnvelopeInput {
            envelope_id: envelope_id.clone(),
            actor: "operator".into(),
            evidence_ref: "revoke-1".into(),
            evidence_sha256: "b".repeat(64),
        };
        let revoked = db.revoke_inference_envelope(&revoke).await.unwrap();
        assert_eq!(revoked.state, EnvelopeState::Revoked);
        assert!(matches!(
            db.mark_inference_dispatched(&MarkInferenceDispatchedInput {
                reservation_id: reservation.reservation_id.clone(),
                worker_id: Some("worker".into()),
                generation: Some(1),
                provider_request_id: None,
                attempt_id: Some("attempt".into()),
                actor: "core:test".into(),
            })
            .await,
            Err(Error::Forbidden(_))
        ));
        assert!(matches!(
            db.reserve_inference(&reserve_input(
                &quote_alt,
                &envelope_id,
                "operation-two",
                "idempotency-two",
                &"c".repeat(64),
                100,
                InferenceRole::Lead,
                future_ms(60_000),
            ))
            .await,
            Err(Error::Forbidden(_))
        ));
        db.settle_inference(&InferenceSettlement {
            reservation_id: reservation.reservation_id,
            settled_micros: UsdMicros::new(3_500).unwrap(),
            usage_evidence_sha256: "d".repeat(64),
        })
        .await
        .unwrap();
        let status = db.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.state, EnvelopeState::Revoked);
        assert_eq!(status.settled_micros.get(), 3_500);
        assert_eq!(status.available_micros.get(), 6_500);
        // Revocation is idempotent.
        assert_eq!(
            db.revoke_inference_envelope(&revoke).await.unwrap().state,
            EnvelopeState::Revoked
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revocation_race_never_admits_after_revocation_commits() {
        let path = temp_path("factory-ledger-revocation-race");
        let first = Database::open(&path).await.unwrap();
        let second = Database::open(&path).await.unwrap();
        seed_project(&first, "project").await;
        let (envelope_id, _) = create_envelope(&first, "request-revocation-race", 10_000).await;
        let quote = test_quote(&first, 1000).await;
        let quote_alt = test_quote(&first, 100).await;
        let input = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            1_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        let revoke_input = RevokeInferenceEnvelopeInput {
            envelope_id: envelope_id.clone(),
            actor: "operator".into(),
            evidence_ref: "revoke-race".into(),
            evidence_sha256: "b".repeat(64),
        };
        let (reserve_result, revoke_result) = tokio::join!(
            first.reserve_inference(&input),
            second.revoke_inference_envelope(&revoke_input)
        );
        assert!(revoke_result.is_ok());
        if reserve_result.is_ok() {
            let status = first.inference_spending_status(&envelope_id).await.unwrap();
            assert_eq!(status.reserved_micros.get(), 1_000);
        }
        let status = first.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.state, EnvelopeState::Revoked);
        assert!(matches!(
            first
                .reserve_inference(&reserve_input(
                    &quote_alt,
                    &envelope_id,
                    "operation-late",
                    "idempotency-late",
                    &"c".repeat(64),
                    100,
                    InferenceRole::Auxiliary,
                    future_ms(60_000),
                ))
                .await,
            Err(Error::Forbidden(_))
        ));
        first.close().await;
        second.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn ambiguous_dispatch_survives_restart_and_blocks_redispatch() {
        let path = temp_path("factory-ledger-ambiguous");
        let expires = future_ms(60_000);
        let reservation_id = {
            let db = Database::open(&path).await.unwrap();
            seed_project(&db, "project").await;
            let (envelope_id, _) = create_envelope(&db, "request-ambiguous", 10_000).await;
            let quote = test_quote(&db, 3000).await;
            let mut uncertain = reserve_input(
                &quote,
                &envelope_id,
                "operation",
                "idempotency",
                &"a".repeat(64),
                3_000,
                InferenceRole::Auxiliary,
                expires,
            );
            uncertain.worker_id = None;
            uncertain.generation = None;
            let reservation = db.reserve_inference(&uncertain).await.unwrap();
            db.mark_inference_dispatched(&MarkInferenceDispatchedInput {
                reservation_id: reservation.reservation_id.clone(),
                worker_id: None,
                generation: None,
                provider_request_id: Some("provider-request".into()),
                attempt_id: None,
                actor: "core:test".into(),
            })
            .await
            .unwrap();
            assert!(matches!(
                db.release_inference(
                    &reservation.reservation_id,
                    "operator",
                    "claimed undispatched without evidence",
                )
                .await,
                Err(Error::Conflict(_))
            ));
            // An uncertain dispatch is never released by elapsed time.
            db.expire_inference_reservations(expires.saturating_add(1))
                .await
                .unwrap();
            let status = db.inference_spending_status(&envelope_id).await.unwrap();
            assert_eq!(status.ambiguous_micros.get(), 3_000);
            assert_eq!(status.reserved_micros.get(), 0);
            db.close().await;
            reservation.reservation_id
        };
        let reopened = Database::open(&path).await.unwrap();
        let quote = test_quote(&reopened, 3000).await;
        let quote_alt = test_quote(&reopened, 1000).await;
        let record = reopened
            .inference_reservation(&reservation_id)
            .await
            .unwrap();
        assert_eq!(record.state, ReservationState::Ambiguous);
        let envelope_id = record.envelope_id.clone();
        let mut replay = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            3_000,
            InferenceRole::Auxiliary,
            expires,
        );
        replay.worker_id = None;
        replay.generation = None;
        assert!(matches!(
            reopened.reserve_inference(&replay).await,
            Err(Error::Conflict(_) | Error::Forbidden(_))
        ));
        assert!(matches!(
            reopened
                .mark_inference_dispatched(&MarkInferenceDispatchedInput {
                    reservation_id: reservation_id.clone(),
                    worker_id: None,
                    generation: None,
                    provider_request_id: Some("provider-request".into()),
                    attempt_id: None,
                    actor: "core:test".into(),
                })
                .await,
            Err(Error::Forbidden(_))
        ));
        let reconcile = ReconcileInferenceInput {
            reservation_id: reservation_id.clone(),
            outcome: ReconciliationOutcome::Settled {
                settled_micros: UsdMicros::new(3_000).unwrap(),
            },
            actor: "operator".into(),
            evidence_ref: "provider-usage-export".into(),
            evidence_sha256: "e".repeat(64),
        };
        reopened.reconcile_inference(&reconcile).await.unwrap();
        reopened.reconcile_inference(&reconcile).await.unwrap();
        let mut conflicting_reconcile = reconcile.clone();
        conflicting_reconcile.evidence_sha256 = "f".repeat(64);
        assert!(matches!(
            reopened.reconcile_inference(&conflicting_reconcile).await,
            Err(Error::Conflict(_))
        ));
        let status = reopened
            .inference_spending_status(&envelope_id)
            .await
            .unwrap();
        assert_eq!(status.settled_micros.get(), 3_000);
        assert_eq!(status.ambiguous_micros.get(), 0);
        // After reconciliation a new operation has authority again.
        reopened
            .reserve_inference(&reserve_input(
                &quote_alt,
                &envelope_id,
                "operation-next",
                "idempotency-next",
                &"b".repeat(64),
                1_000,
                InferenceRole::SemanticReviewer,
                future_ms(60_000),
            ))
            .await
            .unwrap();
        reopened.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn undispatched_release_and_ambiguous_release_preserve_no_liability() {
        let path = temp_path("factory-ledger-release");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-release", 10_000).await;
        let quote = test_quote(&db, 2000).await;
        let quote_alt = test_quote(&db, 3000).await;
        let undispatched = db
            .reserve_inference(&reserve_input(
                &quote,
                &envelope_id,
                "operation-ready",
                "idempotency-ready",
                &"a".repeat(64),
                2_000,
                InferenceRole::Lead,
                future_ms(60_000),
            ))
            .await
            .unwrap();
        let released = db
            .release_inference(
                &undispatched.reservation_id,
                "operator",
                "provider connection was never opened",
            )
            .await
            .unwrap();
        assert_eq!(released.state, ReservationState::Released);
        assert_eq!(
            db.release_inference(
                &undispatched.reservation_id,
                "operator",
                "provider connection was never opened",
            )
            .await
            .unwrap()
            .state,
            ReservationState::Released
        );
        assert!(matches!(
            db.settle_inference(&InferenceSettlement {
                reservation_id: undispatched.reservation_id,
                settled_micros: UsdMicros::new(100).unwrap(),
                usage_evidence_sha256: "b".repeat(64),
            })
            .await,
            Err(Error::Conflict(_))
        ));

        let mut uncertain_input = reserve_input(
            &quote_alt,
            &envelope_id,
            "operation-uncertain",
            "idempotency-uncertain",
            &"c".repeat(64),
            3_000,
            InferenceRole::Auxiliary,
            future_ms(60_000),
        );
        uncertain_input.worker_id = None;
        uncertain_input.generation = None;
        let uncertain = db.reserve_inference(&uncertain_input).await.unwrap();
        db.mark_inference_dispatched(&MarkInferenceDispatchedInput {
            reservation_id: uncertain.reservation_id.clone(),
            worker_id: None,
            generation: None,
            provider_request_id: Some("request-uncertain".into()),
            attempt_id: None,
            actor: "core:test".into(),
        })
        .await
        .unwrap();
        db.expire_inference_reservations(future_ms(120_000))
            .await
            .unwrap();
        let reconciled = db
            .reconcile_inference(&ReconcileInferenceInput {
                reservation_id: uncertain.reservation_id,
                outcome: ReconciliationOutcome::Released,
                actor: "operator".into(),
                evidence_ref: "provider-access-log-shows-no-request".into(),
                evidence_sha256: "d".repeat(64),
            })
            .await
            .unwrap();
        assert_eq!(reconciled.state, ReservationState::Released);
        let status = db.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.reserved_micros.get(), 0);
        assert_eq!(status.ambiguous_micros.get(), 0);
        assert_eq!(
            status.settled_micros.get()
                + status.reserved_micros.get()
                + status.ambiguous_micros.get(),
            0
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn dispatch_fencing_must_match_the_recorded_reservation() {
        let path = temp_path("factory-ledger-fencing");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        seed_worker_campaign(&db).await;
        let (envelope_id, _) = create_envelope(&db, "request-fencing", 10_000).await;
        let quote = test_quote(&db, 1000).await;
        let mut input = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            1_000,
            InferenceRole::Implementer,
            future_ms(60_000),
        );
        input.worker_id = Some("worker-1".into());
        input.generation = Some(3);
        let reservation = db.reserve_inference(&input).await.unwrap();
        assert!(matches!(
            db.mark_inference_dispatched(&MarkInferenceDispatchedInput {
                reservation_id: reservation.reservation_id.clone(),
                worker_id: Some("worker-2".into()),
                generation: Some(3),
                provider_request_id: None,
                attempt_id: Some("attempt-fence".into()),
                actor: "core:test".into(),
            })
            .await,
            Err(Error::Conflict(_))
        ));
        let dispatched = db
            .mark_inference_dispatched(&MarkInferenceDispatchedInput {
                reservation_id: reservation.reservation_id.clone(),
                worker_id: Some("worker-1".into()),
                generation: Some(3),
                provider_request_id: Some("request-1".into()),
                attempt_id: Some("attempt-fence".into()),
                actor: "core:test".into(),
            })
            .await
            .unwrap();
        assert_eq!(dispatched.state, ReservationState::Reserved);
        assert_eq!(dispatched.worker_id.as_deref(), Some("worker-1"));
        assert_eq!(dispatched.generation, Some(3));
        // Dispatch identity is immutable and terminal states cannot be rewound.
        assert!(
            sqlx::query::query(
                "UPDATE inference_reservations SET worker_id='worker-2' WHERE id=?1"
            )
            .bind(&reservation.reservation_id)
            .execute(&db.pool)
            .await
            .is_err()
        );
        assert!(
            sqlx::query::query("UPDATE inference_reservations SET dispatched_at_ms=1 WHERE id=?1")
                .bind(&reservation.reservation_id)
                .execute(&db.pool)
                .await
                .is_err()
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn dispatch_requires_a_current_worker_lease() {
        let path = temp_path("factory-ledger-stale-lease");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        seed_worker_campaign(&db).await;
        let (envelope_id, _) = create_envelope(&db, "request-stale-lease", 10_000).await;
        let quote = test_quote(&db, 1_000).await;
        let mut input = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            1_000,
            InferenceRole::Implementer,
            future_ms(60_000),
        );
        input.worker_id = Some("worker-1".into());
        input.generation = Some(3);
        let reservation = db.reserve_inference(&input).await.unwrap();
        sqlx::query::query(
            "UPDATE worker_leases SET expires_at_ms=1 WHERE attempt_id='attempt-fence'",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        assert!(matches!(
            db.mark_inference_dispatched(&MarkInferenceDispatchedInput {
                reservation_id: reservation.reservation_id,
                worker_id: Some("worker-1".into()),
                generation: Some(3),
                provider_request_id: Some("request".into()),
                attempt_id: Some("attempt-fence".into()),
                actor: "core:test".into(),
            })
            .await,
            Err(Error::Forbidden(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn stale_authorization_revision_and_scope_are_rejected() {
        let path = temp_path("factory-ledger-revision");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-revision", 10_000).await;
        let quote = test_quote(&db, 1000).await;
        db.authorize_inference_envelope(&NewInferenceAuthorization {
            envelope_id: envelope_id.clone(),
            limit_micros: UsdMicros::new(20_000).unwrap(),
            actor: "operator".into(),
            scope_sha256: "c".repeat(64),
            evidence_sha256: "d".repeat(64),
            idempotency_key: "revision-two".into(),
        })
        .await
        .unwrap();
        let mut stale = reserve_input(
            &quote,
            &envelope_id,
            "operation-stale",
            "idempotency-stale",
            &"a".repeat(64),
            1_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        assert!(matches!(
            db.reserve_inference(&stale).await,
            Err(Error::Conflict(_))
        ));
        stale.authorization_revision = 2;
        stale.scope_sha256 = "a".repeat(64);
        assert!(matches!(
            db.reserve_inference(&stale).await,
            Err(Error::Conflict(_))
        ));
        stale.scope_sha256 = "c".repeat(64);
        stale.operation_id = "operation-current".into();
        stale.idempotency_key = "idempotency-current".into();
        db.reserve_inference(&stale).await.unwrap();
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn campaign_binding_requires_project_and_request_ownership() {
        let path = temp_path("factory-ledger-binding-ownership");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-owner", 10_000).await;
        // Wrong project.
        seed_campaign_lineage(
            &db,
            "other-project",
            "action-other",
            "campaign-other",
            "request-other",
        )
        .await;
        assert!(matches!(
            db.bind_campaign_inference_envelope(&CampaignEnvelopeBindingInput {
                campaign_id: "campaign-other".into(),
                envelope_id: envelope_id.clone(),
                actor: "operator".into(),
                evidence_sha256: "c".repeat(64),
            })
            .await,
            Err(Error::Forbidden(_))
        ));
        // Correct project, foreign request.
        seed_campaign_lineage(
            &db,
            "project",
            "action-owner-foreign",
            "campaign-owner",
            "request-foreign",
        )
        .await;
        assert!(matches!(
            db.bind_campaign_inference_envelope(&CampaignEnvelopeBindingInput {
                campaign_id: "campaign-owner".into(),
                envelope_id: envelope_id.clone(),
                actor: "operator".into(),
                evidence_sha256: "c".repeat(64),
            })
            .await,
            Err(Error::Forbidden(_))
        ));
        // Matching lineage succeeds; an identical rebind is idempotent.
        seed_campaign_lineage(
            &db,
            "project",
            "action-owner-ok",
            "campaign-owner-ok",
            "request-owner",
        )
        .await;
        db.bind_campaign_inference_envelope(&CampaignEnvelopeBindingInput {
            campaign_id: "campaign-owner-ok".into(),
            envelope_id: envelope_id.clone(),
            actor: "operator".into(),
            evidence_sha256: "c".repeat(64),
        })
        .await
        .unwrap();
        db.bind_campaign_inference_envelope(&CampaignEnvelopeBindingInput {
            campaign_id: "campaign-owner-ok".into(),
            envelope_id: envelope_id.clone(),
            actor: "operator".into(),
            evidence_sha256: "c".repeat(64),
        })
        .await
        .unwrap();
        // Schema uniqueness also prevents a second binding row for the envelope.
        sqlx::query::query(
            "INSERT INTO inference_campaign_bindings(campaign_id,envelope_id,bound_at_ms)
             VALUES ('campaign-other',?1,1)",
        )
        .bind(&envelope_id)
        .execute(&db.pool)
        .await
        .expect_err("one envelope may back only one campaign");
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn settlement_helpers_require_identical_receipts_and_terminal_conflicts() {
        let path = temp_path("factory-ledger-settlement-helpers");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-helpers", 10_000).await;
        let quote = test_quote(&db, 1_000).await;
        let reservation = db
            .reserve_inference(&reserve_input(
                &quote,
                &envelope_id,
                "operation",
                "idempotency",
                &"a".repeat(64),
                1_000,
                InferenceRole::Evaluation,
                future_ms(60_000),
            ))
            .await
            .unwrap();
        let ledger_id = reservation.reservation_id;
        db.settle_inference(&InferenceSettlement {
            reservation_id: ledger_id.clone(),
            settled_micros: UsdMicros::new(700).unwrap(),
            usage_evidence_sha256: "b".repeat(64),
        })
        .await
        .unwrap();
        let mut transaction = db.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        assert!(matches!(
            settle_or_reconcile_in_tx(
                &mut transaction,
                &ledger_id,
                UsdMicros::new(700).unwrap(),
                &"c".repeat(64),
                "operator",
                "different receipt",
                Utc::now().timestamp_millis(),
            )
            .await,
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            release_or_reconcile_in_tx(
                &mut transaction,
                &ledger_id,
                "operator",
                "release settled",
                &"d".repeat(64),
                Utc::now().timestamp_millis(),
            )
            .await,
            Err(Error::Conflict(_))
        ));
        transaction.rollback().await.unwrap();
        let mut transaction = db.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        settle_or_reconcile_in_tx(
            &mut transaction,
            &ledger_id,
            UsdMicros::new(700).unwrap(),
            &"b".repeat(64),
            "operator",
            "identical receipt",
            Utc::now().timestamp_millis(),
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn quote_creation_requires_trusted_provider_settings() {
        let path = temp_path("factory-ledger-quote-trust");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        assert!(matches!(
            db.create_inference_pricing_quote(&NewInferencePricingQuote {
                provider: "unknown-provider".into(),
                model: "unknown-model".into(),
                revision: 1,
                evidence_sha256: "e".repeat(64),
                actor: "operator".into(),
            })
            .await,
            Err(Error::Unavailable(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn forged_quote_snapshot_bound_is_rejected() {
        let path = temp_path("factory-ledger-quote-forged");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-forged", 10_000).await;
        let quote_id = "forged-quote";
        sqlx::query::query(
            "INSERT INTO inference_pricing_quotes
             (quote_id,provider,model,revision,currency,upper_bound_micros,provider_revision,
              auth_mode,input_tokens,output_tokens,coverage_sha256,coverage_json,snapshot_json,
              evidence_sha256,created_at_ms)
             VALUES (?1,'opencode-go','deepseek-v4.1-flash',1,'USD',1,1,'api_key',1,1,?2,?3,?4,?5,1)",
        )
        .bind(quote_id)
        .bind("a".repeat(64))
        .bind(r#"{"input":true,"output":true,"cachedInput":true,"reasoning":true,"tools":true,"retries":true,"requestOverhead":true}"#)
        .bind(r#"{"provider":"opencode-go","model":"deepseek-v4.1-flash","providerRevision":1,"authMode":"api_key","inputTokens":1,"outputTokens":1,"upperBoundMicros":2}"#)
        .bind("b".repeat(64))
        .execute(&db.pool)
        .await
        .unwrap();
        let mut input = reserve_input(
            &test_quote(&db, 1_000).await,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            1_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        input.pricing_revision = quote_id.into();
        input.pricing_snapshot_json =
            r#"{"provider":"opencode-go","model":"deepseek-v4.1-flash","upperBoundMicros":2}"#
                .into();
        input.upper_bound_micros = UsdMicros::new(1).unwrap();
        assert!(matches!(
            db.reserve_inference(&input).await,
            Err(Error::Conflict(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn stale_provider_settings_revision_invalidates_old_quotes() {
        let path = temp_path("factory-ledger-quote-stale");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-stale-quote", 10_000).await;
        let quote = test_quote(&db, 1_000).await;
        let reservation = db
            .reserve_inference(&reserve_input(
                &quote,
                &envelope_id,
                "operation-old",
                "idempotency-old",
                &"a".repeat(64),
                1_000,
                InferenceRole::Lead,
                future_ms(60_000),
            ))
            .await
            .unwrap();
        sqlx::query::query(
            "UPDATE provider_settings SET revision=revision+1,updated_at=?1
             WHERE provider='opencode-go'",
        )
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
        let input = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            1_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        assert!(matches!(
            db.reserve_inference(&input).await,
            Err(Error::Unavailable(_))
        ));
        assert!(matches!(
            db.mark_inference_dispatched(&MarkInferenceDispatchedInput {
                reservation_id: reservation.reservation_id,
                worker_id: None,
                generation: None,
                provider_request_id: Some("request".into()),
                attempt_id: None,
                actor: "core:test".into(),
            })
            .await,
            Err(Error::Unavailable(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn quote_bound_cannot_be_lowered_by_the_caller() {
        let path = temp_path("factory-ledger-quote-bound");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-quote", 10_000).await;
        let quote = test_quote(&db, 2_000).await;
        let mut input = reserve_input(
            &quote,
            &envelope_id,
            "operation",
            "idempotency",
            &"a".repeat(64),
            2_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        input.upper_bound_micros = UsdMicros::new(1).unwrap();
        assert!(matches!(
            db.reserve_inference(&input).await,
            Err(Error::Validation(_))
        ));
        let mut estimated = reserve_input(
            &quote,
            &envelope_id,
            "operation-estimate",
            "idempotency-estimate",
            &"b".repeat(64),
            2_000,
            InferenceRole::Lead,
            future_ms(60_000),
        );
        estimated.pricing_snapshot_json = "{\"estimate\":1}".into();
        assert!(matches!(
            db.reserve_inference(&estimated).await,
            Err(Error::Conflict(_))
        ));
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn aggregation_covers_every_inference_role() {
        let path = temp_path("factory-ledger-aggregation");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, _) = create_envelope(&db, "request-aggregation", 100_000).await;
        let quote = test_quote(&db, 1000).await;
        for (index, role) in InferenceRole::ALL.into_iter().enumerate() {
            let reservation = db
                .reserve_inference(&reserve_input(
                    &quote,
                    &envelope_id,
                    &format!("operation-{index}"),
                    &format!("idempotency-{index}"),
                    &format!("{index:064x}"),
                    1_000,
                    role,
                    future_ms(60_000),
                ))
                .await
                .unwrap();
            db.settle_inference(&InferenceSettlement {
                reservation_id: reservation.reservation_id,
                settled_micros: UsdMicros::new(500).unwrap(),
                usage_evidence_sha256: format!("{index:064x}"),
            })
            .await
            .unwrap();
        }
        let status = db.inference_spending_status(&envelope_id).await.unwrap();
        assert_eq!(status.settled_micros.get(), 2_500);
        assert_eq!(status.by_role.len(), 5);
        let settled_by_role: u64 = status
            .by_role
            .iter()
            .map(|role| role.settled_micros.get())
            .sum();
        assert_eq!(settled_by_role, 2_500);
        assert_eq!(status.by_provider.len(), 1);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn envelope_authorization_increments_append_only_revisions() {
        let path = temp_path("factory-ledger-authorization");
        let db = Database::open(&path).await.unwrap();
        seed_project(&db, "project").await;
        let (envelope_id, envelope_key) =
            create_envelope(&db, "request-authorization", 1_000).await;
        let replay = db
            .create_inference_envelope(&NewInferenceEnvelope {
                request_id: "request-authorization".into(),
                project_id: "project".into(),
                authorized_micros: UsdMicros::new(1_000).unwrap(),
                actor: "operator".into(),
                scope_sha256: "a".repeat(64),
                evidence_sha256: "b".repeat(64),
                idempotency_key: envelope_key,
            })
            .await
            .unwrap();
        assert_eq!(replay.envelope_id, envelope_id);
        let updated = db
            .authorize_inference_envelope(&NewInferenceAuthorization {
                envelope_id: envelope_id.clone(),
                limit_micros: UsdMicros::new(2_000).unwrap(),
                actor: "operator".into(),
                scope_sha256: "c".repeat(64),
                evidence_sha256: "d".repeat(64),
                idempotency_key: "authorization-two".into(),
            })
            .await
            .unwrap();
        assert_eq!(updated.authorization_revision, 2);
        assert_eq!(updated.authorized_micros.get(), 2_000);
        let count: (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM inference_authorizations WHERE envelope_id=?1",
        )
        .bind(&envelope_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(count.0, 2);
        assert!(
            sqlx::query::query(
                "UPDATE inference_authorizations SET limit_micros=1 WHERE envelope_id=?1"
            )
            .bind(&envelope_id)
            .execute(&db.pool)
            .await
            .is_err(),
            "authorization rows are append-only"
        );
        assert!(
            sqlx::query::query("DELETE FROM inference_ledger_events WHERE envelope_id=?1")
                .bind(&envelope_id)
                .execute(&db.pool)
                .await
                .is_err(),
            "ledger events are append-only"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn upgrade_from_released_0035_applies_0036_and_preserves_rows() {
        let repo_migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
        let subset =
            std::env::temp_dir().join(format!("factory-migrations-0035-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&subset).unwrap();
        let mut applied = 0u32;
        for entry in std::fs::read_dir(&repo_migrations).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(version) = name
                .split_once('_')
                .and_then(|(prefix, _)| prefix.parse::<u32>().ok())
            else {
                continue;
            };
            if version <= 35 && name.ends_with(".sql") {
                std::fs::copy(entry.path(), subset.join(name.as_ref())).unwrap();
                applied += 1;
            }
        }
        assert!(applied >= 33);
        let path = temp_path("factory-ledger-upgrade-0036");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate::Migrator::new(subset.as_path())
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
        let now = Utc::now().to_rfc3339();
        let now_ms = Utc::now().timestamp_millis();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture','base',?1,'')",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO inference_envelopes
             (id,request_id,project_id,currency,state,authorized_micros,authorization_revision,
              created_at_ms,revoked_at_ms)
             VALUES ('envelope','request','project','USD','ACTIVE',10000,1,?1,NULL)",
        )
        .bind(now_ms)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO inference_authorizations
             (id,envelope_id,revision,limit_micros,actor,scope_sha256,evidence_sha256,
              idempotency_key,created_at_ms)
             VALUES ('auth','envelope',1,10000,'operator',?1,?2,'auth-key',?3)",
        )
        .bind("a".repeat(64))
        .bind("b".repeat(64))
        .bind(now_ms)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO inference_reservations
             (id,envelope_id,operation_id,idempotency_key,request_sha256,role,
              authorization_revision,scope_sha256,principal_id,parent_operation_id,provider,model,
              pricing_revision,pricing_snapshot_json,upper_bound_micros,settled_micros,state,
              worker_id,generation,dispatched_at_ms,provider_request_id,expires_at_ms,
              settlement_sha256,created_at_ms,updated_at_ms)
             VALUES ('reservation','envelope','operation','idempotency',?1,'LEAD',1,?2,
                     'principal',NULL,'opencode-go','deepseek-v4.1-flash','old-quote','{}',1000,
                     NULL,'RESERVED',NULL,NULL,NULL,NULL,?3,NULL,?3,?3)",
        )
        .bind("c".repeat(64))
        .bind("a".repeat(64))
        .bind(now_ms + 60_000)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO model_call_reservations
             (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
              purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
              max_cost_micros,state,expires_at_ms,created_at,updated_at)
             VALUES ('legacy-call','LEAD','principal','project','objective','scope','opencode-go',
                     'deepseek-v4.1-flash','assess',?1,100,100,200,300,'SETTLED',?2,?3,?3)",
        )
        .bind("d".repeat(64))
        .bind(now_ms + 60_000)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES ('action-upgrade','project','objective','opencode-go',
                     'deepseek-v4.1-flash','R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO campaigns(id,action_id,status,created_at)
             VALUES ('campaign-upgrade','action-upgrade','AUTHORIZED',?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO inference_campaign_bindings(campaign_id,envelope_id,bound_at_ms)
             VALUES ('campaign-upgrade','envelope',?1)",
        )
        .bind(now_ms)
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let db = Database::open(&path).await.unwrap();
        let (migration_count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(migration_count, i64::from(applied) + 1);
        let (model_state, dispatch, provider_request): (String, Option<i64>, Option<String>) =
            sqlx::query_as::query_as(
                "SELECT state,dispatched_at_ms,provider_request_id FROM model_call_reservations
                 WHERE reservation_id='legacy-call'",
            )
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(model_state, "SETTLED");
        assert!(dispatch.is_none() && provider_request.is_none());
        let (reservation_state,): (String,) = sqlx::query_as::query_as(
            "SELECT state FROM inference_reservations WHERE id='reservation'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(reservation_state, "RESERVED");
        assert!(matches!(
            db.require_campaign_inference_envelope("campaign-upgrade")
                .await
                .unwrap(),
            CampaignInferenceRequirement::EnvelopeBound { .. }
        ));
        let (mode,): (String,) = sqlx::query_as::query_as(
            "SELECT mode FROM inference_campaign_modes WHERE campaign_id='campaign-upgrade'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(mode, "V2");
        for table in [
            "inference_ledger_policy",
            "inference_campaign_modes",
            "inference_pricing_quotes",
            "provider_turn_ledger_requests",
        ] {
            let (count,): (i64,) = sqlx::query_as::query_as(&format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{table}'"
            ))
            .fetch_one(&db.pool)
            .await
            .unwrap();
            assert_eq!(count, 1, "missing {table}");
        }
        sqlx::query::query(
            "UPDATE inference_reservations
             SET worker_id='worker',generation=1,dispatched_at_ms=1,provider_request_id='request'
             WHERE id='reservation'",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        assert!(
            sqlx::query::query(
                "UPDATE inference_reservations SET worker_id='other' WHERE id='reservation'"
            )
            .execute(&db.pool)
            .await
            .is_err(),
            "0036 dispatch immutability trigger must be installed on upgrade"
        );
        db.close().await;
        std::fs::remove_dir_all(subset).unwrap();
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn migration_from_populated_0033_preserves_legacy_rows() {
        let repo_migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
        let subset =
            std::env::temp_dir().join(format!("factory-migrations-0033-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&subset).unwrap();
        let mut applied = 0u32;
        for entry in std::fs::read_dir(&repo_migrations).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(version) = name
                .split_once('_')
                .and_then(|(prefix, _)| prefix.parse::<u32>().ok())
            else {
                continue;
            };
            if version <= 33 && name.ends_with(".sql") {
                std::fs::copy(entry.path(), subset.join(name.as_ref())).unwrap();
                applied += 1;
            }
        }
        assert!(applied >= 32);
        let path = temp_path("factory-ledger-migration");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate::Migrator::new(subset.as_path())
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO projects(id,repository,base_commit,created_at,identity)
             VALUES ('project','/fixture','base',?1,'')",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO model_call_reservations
             (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
              purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
              max_cost_micros,state,expires_at_ms,created_at,updated_at)
             VALUES ('legacy-call','LEAD','principal','project','objective','scope','opencode-go',
                     'deepseek-v4.1-flash','assess',?1,100,100,200,300,'SETTLED',?2,?3,?3)",
        )
        .bind("a".repeat(64))
        .bind(future_ms(60_000))
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO actions VALUES ('action','project','objective','opencode',
             'deepseek-v4.1-flash','R0','AUTHORIZED','hash',?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query("INSERT INTO campaigns VALUES ('campaign','action','AUTHORIZED',?1)")
            .bind(&now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query::query("INSERT INTO runs VALUES ('run','campaign','QUEUED',?1)")
            .bind(&now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES ('attempt','run','LEASED',?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO provider_sessions
             (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,
              state,created_at,updated_at)
             VALUES ('legacy-session','attempt','worker',1,'opencode','deepseek-v4.1-flash',
                     'build','ACTIVE',?1,?1)",
        )
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO provider_turn_reservations
             (reservation_id,session_record_id,attempt_id,generation,provider,model,
              max_input_tokens,max_output_tokens,max_total_tokens,max_cost_micros,request_sha256,
              state,expires_at_ms,created_at,updated_at,max_requests,requests_bound)
             VALUES ('legacy-turn','legacy-session','attempt',1,'opencode','deepseek-v4.1-flash',
                     100,100,200,300,?1,'SETTLED',?2,?3,?3,1,1)",
        )
        .bind("b".repeat(64))
        .bind(future_ms(60_000))
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let db = Database::open(&path).await.unwrap();
        let (migration_count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(migration_count, i64::from(applied) + 2);
        let model_call: (String, i64, Option<String>) = sqlx::query_as::query_as(
            "SELECT state,cost_micros,ledger_reservation_id FROM model_call_reservations
             WHERE reservation_id='legacy-call'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(model_call, ("SETTLED".into(), 0, None));
        let provider_turn: (String, Option<String>) = sqlx::query_as::query_as(
            "SELECT state,ledger_reservation_id FROM provider_turn_reservations
             WHERE reservation_id='legacy-turn'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(provider_turn, ("SETTLED".into(), None));
        let (fk_violations,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(fk_violations, 0);
        let trigger_count: (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger'
             AND name IN ('inference_reservations_identity_immutable',
                          'inference_reservations_dispatch_immutable',
                          'inference_authorizations_immutable_update',
                          'inference_ledger_events_immutable_update')",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(trigger_count.0, 4);
        db.close().await;
        let reopened = Database::open(&path).await.unwrap();
        let (reopened_count,): (i64,) =
            sqlx::query_as::query_as("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&reopened.pool)
                .await
                .unwrap();
        assert_eq!(reopened_count, migration_count);
        reopened.close().await;
        std::fs::remove_dir_all(subset).unwrap();
        let _ = std::fs::remove_file(path);
    }
}
