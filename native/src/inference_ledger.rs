//! Shared campaign inference-ledger domain.
//!
//! Decision mechanism: `DETERMINISTIC`. Monetary admission, checked
//! arithmetic, state transitions and projection are mechanical; no model
//! output participates. This module owns the durable value types and pure
//! projections. SQLite transactions live in `db::inference_ledger`.
//!
//! Durable invariants:
//!
//! - One envelope per operator request; campaign attachment never creates a
//!   second allowance.
//! - A reservation holds its full conservative upper bound until it settles,
//!   is released with proven-undispatched evidence, or becomes `AMBIGUOUS`.
//! - `SETTLED` and `RELEASED` are terminal. An ambiguous hold is never
//!   redispatched and requires explicit reconciliation evidence.
//! - Amounts are integer USD micros with checked arithmetic; no float ever
//!   contributes to a balance.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::execution_policy::{
    EnvelopeState, ExecutionPolicyError, InferenceLedgerError, InferenceReservation, InferenceRole,
    InferenceSettlement, ReservationState, ReserveInferenceInput, SpendingBreakdown,
    SpendingStatus, UsdMicros,
};

impl From<ExecutionPolicyError> for InferenceLedgerError {
    fn from(error: ExecutionPolicyError) -> Self {
        match error {
            ExecutionPolicyError::Invalid(message) => Self::Invalid(message),
            ExecutionPolicyError::Overflow => Self::Invalid("monetary amount overflow".into()),
        }
    }
}

/// Durable billing basis recorded for every projection. A subscription login
/// is not evidence of zero cost; unsupported billing bases must fail closed
/// before a reservation is created.
pub const USD_BILLING_BASIS: &str = "USD_MICROS";

/// Explicit billing mode. Legacy money accounting is legal only while Core is
/// explicitly in V1. Once promoted to V2, every billable operation requires a
/// bound envelope; there is no implicit fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InferenceLedgerMode {
    V1,
    V2,
}

impl InferenceLedgerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "V1",
            Self::V2 => "V2",
        }
    }

    pub fn parse(value: &str) -> Result<Self, InferenceLedgerError> {
        match value {
            "V1" => Ok(Self::V1),
            "V2" => Ok(Self::V2),
            _ => Err(InferenceLedgerError::Invalid(
                "stored inference ledger mode".into(),
            )),
        }
    }
}

/// What a Campaign's billing mode requires from the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CampaignInferenceRequirement {
    LegacyV1,
    EnvelopeBound { envelope_id: String },
}

const MAX_TOKEN_LEN: usize = 256;
const MAX_TEXT_LEN: usize = 512;
const MAX_JSON_LEN: usize = 64 * 1024;

/// Trusted adapter billing coverage. This is a Core-owned declaration, not a
/// caller boolean: the HTTP OpenCode/Codex API-key adapters report token usage
/// only, cached and reasoning tokens are token classes billed at the input or
/// output rate, tool use is part of the prompt, each retry is a separate
/// reservation, and there is no fixed request fee. Providers outside this
/// contract fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterBillingCoverage {
    pub input: bool,
    pub output: bool,
    pub cached_input: bool,
    pub reasoning: bool,
    pub tools: bool,
    pub retries: bool,
    pub request_overhead: bool,
}

pub fn adapter_billing_coverage(kind: &str) -> Option<AdapterBillingCoverage> {
    match kind {
        "opencode" | "codex" => Some(AdapterBillingCoverage {
            input: true,
            output: true,
            cached_input: true,
            reasoning: true,
            tools: true,
            retries: true,
            request_overhead: true,
        }),
        _ => None,
    }
}

/// Create a frozen quote from Core-owned provider settings. The caller chooses
/// only the provider/model/revision and supplies operator evidence; the bound,
/// coverage document and snapshot are derived by Core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewInferencePricingQuote {
    pub provider: String,
    pub model: String,
    pub revision: i64,
    pub evidence_sha256: String,
    pub actor: String,
}

impl NewInferencePricingQuote {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("provider", &self.provider)?;
        validate_token("model", &self.model)?;
        if self.revision <= 0 {
            return Err(InferenceLedgerError::Invalid(
                "pricing revision must be positive".into(),
            ));
        }
        validate_sha256("evidence_sha256", &self.evidence_sha256)?;
        validate_text("actor", &self.actor)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InferencePricingQuote {
    pub quote_id: String,
    pub provider: String,
    pub model: String,
    pub revision: i64,
    pub upper_bound_micros: UsdMicros,
    pub coverage_sha256: String,
    pub snapshot_json: String,
    pub evidence_sha256: String,
}

/// Create one operator-request envelope and its first monetary authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewInferenceEnvelope {
    pub request_id: String,
    pub project_id: String,
    pub authorized_micros: UsdMicros,
    pub actor: String,
    pub scope_sha256: String,
    pub evidence_sha256: String,
    pub idempotency_key: String,
}

impl NewInferenceEnvelope {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("request_id", &self.request_id)?;
        validate_token("project_id", &self.project_id)?;
        validate_text("actor", &self.actor)?;
        validate_sha256("scope_sha256", &self.scope_sha256)?;
        validate_sha256("evidence_sha256", &self.evidence_sha256)?;
        validate_token("idempotency_key", &self.idempotency_key)?;
        Ok(())
    }
}

/// Append a new monetary authorization revision. Revisions only ever add rows;
/// the envelope's current limit and revision are a projection of the latest
/// append-only authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NewInferenceAuthorization {
    pub envelope_id: String,
    pub limit_micros: UsdMicros,
    pub actor: String,
    pub scope_sha256: String,
    pub evidence_sha256: String,
    pub idempotency_key: String,
}

impl NewInferenceAuthorization {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("envelope_id", &self.envelope_id)?;
        validate_text("actor", &self.actor)?;
        validate_sha256("scope_sha256", &self.scope_sha256)?;
        validate_sha256("evidence_sha256", &self.evidence_sha256)?;
        validate_token("idempotency_key", &self.idempotency_key)?;
        Ok(())
    }
}

/// Attach a Campaign to the single request envelope it inherits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CampaignEnvelopeBindingInput {
    pub campaign_id: String,
    pub envelope_id: String,
    pub actor: String,
    pub evidence_sha256: String,
}

impl CampaignEnvelopeBindingInput {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("campaign_id", &self.campaign_id)?;
        validate_token("envelope_id", &self.envelope_id)?;
        validate_text("actor", &self.actor)?;
        validate_sha256("evidence_sha256", &self.evidence_sha256)?;
        Ok(())
    }
}

/// Record dispatch intent before the upstream request is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MarkInferenceDispatchedInput {
    pub reservation_id: String,
    pub worker_id: Option<String>,
    pub generation: Option<i64>,
    pub provider_request_id: Option<String>,
    /// Worker attempt that owns the current lease. Required for Implementer
    /// reservations and rejected for Core-only assessment/review roles.
    pub attempt_id: Option<String>,
    pub actor: String,
}

impl MarkInferenceDispatchedInput {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("reservation_id", &self.reservation_id)?;
        validate_text("actor", &self.actor)?;
        match (&self.worker_id, self.generation) {
            (Some(worker_id), Some(generation)) => {
                validate_token("worker_id", worker_id)?;
                if generation <= 0 {
                    return Err(InferenceLedgerError::Invalid(
                        "generation must be positive".into(),
                    ));
                }
            }
            (None, None) => {}
            _ => {
                return Err(InferenceLedgerError::Invalid(
                    "worker_id and generation must be supplied together".into(),
                ));
            }
        }
        if let Some(provider_request_id) = &self.provider_request_id {
            validate_token("provider_request_id", provider_request_id)?;
        }
        if let Some(attempt_id) = &self.attempt_id {
            validate_token("attempt_id", attempt_id)?;
        }
        if self.worker_id.is_some() && self.attempt_id.is_none() {
            return Err(InferenceLedgerError::Invalid(
                "worker dispatch requires an attempt id".into(),
            ));
        }
        Ok(())
    }
}

/// Operator reconciliation of an `AMBIGUOUS` reservation. Evidence is required
/// for both outcomes; reconciliation never deletes the original hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReconcileInferenceInput {
    pub reservation_id: String,
    pub outcome: ReconciliationOutcome,
    pub actor: String,
    pub evidence_ref: String,
    pub evidence_sha256: String,
}

impl ReconcileInferenceInput {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("reservation_id", &self.reservation_id)?;
        validate_text("actor", &self.actor)?;
        validate_text("evidence_ref", &self.evidence_ref)?;
        validate_sha256("evidence_sha256", &self.evidence_sha256)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReconciliationOutcome {
    Settled { settled_micros: UsdMicros },
    Released,
}

/// Revoke new spending while preserving every in-flight liability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RevokeInferenceEnvelopeInput {
    pub envelope_id: String,
    pub actor: String,
    pub evidence_ref: String,
    pub evidence_sha256: String,
}

impl RevokeInferenceEnvelopeInput {
    pub fn validate(&self) -> Result<(), InferenceLedgerError> {
        validate_token("envelope_id", &self.envelope_id)?;
        validate_text("actor", &self.actor)?;
        validate_text("evidence_ref", &self.evidence_ref)?;
        validate_sha256("evidence_sha256", &self.evidence_sha256)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InferenceEnvelopeRecord {
    pub envelope_id: String,
    pub request_id: String,
    pub project_id: String,
    pub currency: String,
    pub state: EnvelopeState,
    pub authorized_micros: UsdMicros,
    pub authorization_revision: i64,
    pub created_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InferenceReservationRecord {
    pub reservation_id: String,
    pub envelope_id: String,
    pub operation_id: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub role: InferenceRole,
    pub principal_id: String,
    pub parent_operation_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub pricing_revision: String,
    pub pricing_snapshot_json: String,
    pub authorization_revision: i64,
    pub scope_sha256: String,
    pub upper_bound_micros: UsdMicros,
    pub settled_micros: Option<UsdMicros>,
    pub state: ReservationState,
    pub worker_id: Option<String>,
    pub generation: Option<i64>,
    pub dispatched_at_ms: Option<i64>,
    pub provider_request_id: Option<String>,
    pub expires_at_ms: i64,
    pub settlement_sha256: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl InferenceReservationRecord {
    pub fn projection(&self) -> InferenceReservation {
        InferenceReservation {
            reservation_id: self.reservation_id.clone(),
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
            worker_id: self.worker_id.clone(),
            generation: self.generation,
            state: self.state,
            upper_bound_micros: self.upper_bound_micros,
            settled_micros: self.settled_micros,
            expires_at_ms: self.expires_at_ms,
        }
    }
}

/// Read-only snapshot of one envelope used by the deterministic projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeSnapshot {
    pub envelope_id: String,
    pub currency: String,
    pub state: EnvelopeState,
    pub authorized_micros: UsdMicros,
    pub authorization_revision: i64,
}

/// The financial fields of one reservation that participate in a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservationAmounts {
    pub role: InferenceRole,
    pub provider: String,
    pub state: ReservationState,
    pub upper_bound_micros: UsdMicros,
    pub settled_micros: Option<UsdMicros>,
}

/// Checked aggregate before it is shaped into `SpendingStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendingTotals {
    pub settled_micros: UsdMicros,
    pub reserved_micros: UsdMicros,
    pub ambiguous_micros: UsdMicros,
    pub committed_micros: UsdMicros,
    pub available_micros: UsdMicros,
    pub overrun_micros: UsdMicros,
    pub unresolved_count: u64,
}

/// Mechanical validation shared by the persistence boundary.
pub fn validate_reserve_input(input: &ReserveInferenceInput) -> Result<(), InferenceLedgerError> {
    input.validate()?;
    validate_token("envelope_id", &input.envelope_id)?;
    validate_token("operation_id", &input.operation_id)?;
    validate_token("idempotency_key", &input.idempotency_key)?;
    validate_token("principal_id", &input.principal_id)?;
    validate_token("provider", &input.provider)?;
    validate_token("model", &input.model)?;
    validate_token("pricing_revision", &input.pricing_revision)?;
    if let Some(parent) = &input.parent_operation_id {
        validate_token("parent_operation_id", parent)?;
    }
    if let Some(worker_id) = &input.worker_id {
        validate_token("worker_id", worker_id)?;
    }
    if matches!(
        input.role,
        InferenceRole::Lead | InferenceRole::SemanticReviewer
    ) && input.worker_id.is_some()
    {
        return Err(InferenceLedgerError::Invalid(
            "assessment and review reservations must not carry Worker fencing".into(),
        ));
    }
    if input.authorization_revision <= 0 {
        return Err(InferenceLedgerError::Invalid(
            "authorization_revision must be positive".into(),
        ));
    }
    validate_sha256("scope_sha256", &input.scope_sha256)?;
    validate_sha256("request_sha256", &input.request_sha256)?;
    validate_json("pricing_snapshot_json", &input.pricing_snapshot_json)?;
    if input.upper_bound_micros == UsdMicros::ZERO {
        return Err(InferenceLedgerError::Invalid(
            "upper_bound_micros must be positive; an invented or zero bound is not admissible"
                .into(),
        ));
    }
    if input.expires_at_ms <= 0 {
        return Err(InferenceLedgerError::Invalid(
            "reservation expiry must be positive".into(),
        ));
    }
    Ok(())
}

/// Checked committed-amount calculation.
pub fn spending_totals(
    authorized_micros: UsdMicros,
    reservations: &[ReservationAmounts],
) -> Result<SpendingTotals, InferenceLedgerError> {
    let mut settled = UsdMicros::ZERO;
    let mut reserved = UsdMicros::ZERO;
    let mut ambiguous = UsdMicros::ZERO;
    let mut bound_overrun = UsdMicros::ZERO;
    let mut unresolved_count = 0u64;
    for reservation in reservations {
        match reservation.state {
            ReservationState::Settled => {
                let amount = reservation.settled_micros.ok_or_else(|| {
                    InferenceLedgerError::Invalid(
                        "settled reservation has no settled amount".into(),
                    )
                })?;
                settled = settled.checked_add(amount)?;
                if amount > reservation.upper_bound_micros {
                    bound_overrun = bound_overrun
                        .checked_add(amount.checked_sub(reservation.upper_bound_micros)?)?;
                }
            }
            ReservationState::Reserved => {
                reserved = reserved.checked_add(reservation.upper_bound_micros)?;
                unresolved_count = unresolved_count.saturating_add(1);
            }
            ReservationState::Ambiguous => {
                ambiguous = ambiguous.checked_add(reservation.upper_bound_micros)?;
                unresolved_count = unresolved_count.saturating_add(1);
            }
            ReservationState::Released => {}
        }
    }
    let committed = settled.checked_add(reserved)?.checked_add(ambiguous)?;
    let available = if committed > authorized_micros {
        UsdMicros::ZERO
    } else {
        authorized_micros.checked_sub(committed)?
    };
    let authorization_overrun = settled
        .checked_sub(authorized_micros)
        .unwrap_or(UsdMicros::ZERO);
    let overrun = authorization_overrun.max(bound_overrun);
    Ok(SpendingTotals {
        settled_micros: settled,
        reserved_micros: reserved,
        ambiguous_micros: ambiguous,
        committed_micros: committed,
        available_micros: available,
        overrun_micros: overrun,
        unresolved_count,
    })
}

/// Deterministic projection used by Core and Office. Campaign totals come
/// exclusively from this ledger; legacy model-call and session rows are
/// evidence and are never added again here.
pub fn project_spending(
    envelope: &EnvelopeSnapshot,
    reservations: &[ReservationAmounts],
    pricing_revisions: &[String],
    as_of: &str,
) -> Result<SpendingStatus, InferenceLedgerError> {
    validate_text("as_of", as_of)?;
    let totals = spending_totals(envelope.authorized_micros, reservations)?;
    Ok(SpendingStatus {
        envelope_id: envelope.envelope_id.clone(),
        currency: envelope.currency.clone(),
        state: envelope.state,
        authorized_micros: envelope.authorized_micros,
        settled_micros: totals.settled_micros,
        reserved_micros: totals.reserved_micros,
        ambiguous_micros: totals.ambiguous_micros,
        available_micros: totals.available_micros,
        overrun_micros: totals.overrun_micros,
        authorization_revision: envelope.authorization_revision,
        billing_basis: USD_BILLING_BASIS.into(),
        pricing_provenance: pricing_provenance(pricing_revisions),
        unresolved_count: totals.unresolved_count,
        by_role: role_breakdowns(reservations)?,
        by_provider: provider_breakdowns(reservations)?,
        as_of: as_of.into(),
    })
}

struct BreakdownTotals {
    settled: UsdMicros,
    reserved: UsdMicros,
    ambiguous: UsdMicros,
}

impl BreakdownTotals {
    fn zero() -> Self {
        Self {
            settled: UsdMicros::ZERO,
            reserved: UsdMicros::ZERO,
            ambiguous: UsdMicros::ZERO,
        }
    }

    fn add(&mut self, reservation: &ReservationAmounts) -> Result<(), InferenceLedgerError> {
        match reservation.state {
            ReservationState::Settled => {
                let amount = reservation.settled_micros.ok_or_else(|| {
                    InferenceLedgerError::Invalid(
                        "settled reservation has no settled amount".into(),
                    )
                })?;
                self.settled = self.settled.checked_add(amount)?;
            }
            ReservationState::Reserved => {
                self.reserved = self.reserved.checked_add(reservation.upper_bound_micros)?;
            }
            ReservationState::Ambiguous => {
                self.ambiguous = self.ambiguous.checked_add(reservation.upper_bound_micros)?;
            }
            ReservationState::Released => {}
        }
        Ok(())
    }
}

fn role_breakdowns(
    reservations: &[ReservationAmounts],
) -> Result<Vec<SpendingBreakdown>, InferenceLedgerError> {
    let mut totals: BTreeMap<InferenceRole, BreakdownTotals> = BTreeMap::new();
    for reservation in reservations {
        totals
            .entry(reservation.role)
            .or_insert_with(BreakdownTotals::zero)
            .add(reservation)?;
    }
    InferenceRole::ALL
        .into_iter()
        .map(|role| {
            let totals = totals.get(&role);
            Ok(SpendingBreakdown {
                key: role.as_str().into(),
                settled_micros: totals.map_or(UsdMicros::ZERO, |value| value.settled),
                reserved_micros: totals.map_or(UsdMicros::ZERO, |value| value.reserved),
                ambiguous_micros: totals.map_or(UsdMicros::ZERO, |value| value.ambiguous),
            })
        })
        .collect()
}

fn provider_breakdowns(
    reservations: &[ReservationAmounts],
) -> Result<Vec<SpendingBreakdown>, InferenceLedgerError> {
    let mut totals: BTreeMap<String, BreakdownTotals> = BTreeMap::new();
    for reservation in reservations {
        totals
            .entry(reservation.provider.clone())
            .or_insert_with(BreakdownTotals::zero)
            .add(reservation)?;
    }
    totals
        .into_iter()
        .map(|(provider, totals)| {
            Ok(SpendingBreakdown {
                key: provider,
                settled_micros: totals.settled,
                reserved_micros: totals.reserved,
                ambiguous_micros: totals.ambiguous,
            })
        })
        .collect()
}

/// Admission is the only point at which new liability may be created.
pub fn admit(
    committed_micros: UsdMicros,
    requested_micros: UsdMicros,
    authorized_micros: UsdMicros,
) -> Result<(), InferenceLedgerError> {
    if requested_micros == UsdMicros::ZERO {
        return Err(InferenceLedgerError::Invalid(
            "requested upper bound must be positive".into(),
        ));
    }
    let required = committed_micros.checked_add(requested_micros)?;
    if required > authorized_micros {
        return Err(InferenceLedgerError::Exhausted);
    }
    Ok(())
}

pub fn validate_settlement(settlement: &InferenceSettlement) -> Result<(), InferenceLedgerError> {
    settlement.validate()?;
    validate_sha256("usage_evidence_sha256", &settlement.usage_evidence_sha256)
}

pub fn parse_role(value: &str) -> Result<InferenceRole, InferenceLedgerError> {
    match value {
        "LEAD" => Ok(InferenceRole::Lead),
        "IMPLEMENTER" => Ok(InferenceRole::Implementer),
        "SEMANTIC_REVIEWER" => Ok(InferenceRole::SemanticReviewer),
        "EVALUATION" => Ok(InferenceRole::Evaluation),
        "AUXILIARY" => Ok(InferenceRole::Auxiliary),
        _ => Err(InferenceLedgerError::Invalid(
            "stored inference role".into(),
        )),
    }
}

pub fn parse_envelope_state(value: &str) -> Result<EnvelopeState, InferenceLedgerError> {
    match value {
        "ACTIVE" => Ok(EnvelopeState::Active),
        "REVOKED" => Ok(EnvelopeState::Revoked),
        "CLOSED" => Ok(EnvelopeState::Closed),
        "OVERDRAWN" => Ok(EnvelopeState::Overdrawn),
        _ => Err(InferenceLedgerError::Invalid(
            "stored inference envelope state".into(),
        )),
    }
}

pub fn parse_reservation_state(value: &str) -> Result<ReservationState, InferenceLedgerError> {
    match value {
        "RESERVED" => Ok(ReservationState::Reserved),
        "SETTLED" => Ok(ReservationState::Settled),
        "RELEASED" => Ok(ReservationState::Released),
        "AMBIGUOUS" => Ok(ReservationState::Ambiguous),
        _ => Err(InferenceLedgerError::Invalid(
            "stored inference reservation state".into(),
        )),
    }
}

pub fn envelope_state_str(state: EnvelopeState) -> &'static str {
    match state {
        EnvelopeState::Active => "ACTIVE",
        EnvelopeState::Revoked => "REVOKED",
        EnvelopeState::Closed => "CLOSED",
        EnvelopeState::Overdrawn => "OVERDRAWN",
    }
}

pub fn reservation_state_str(state: ReservationState) -> &'static str {
    match state {
        ReservationState::Reserved => "RESERVED",
        ReservationState::Settled => "SETTLED",
        ReservationState::Released => "RELEASED",
        ReservationState::Ambiguous => "AMBIGUOUS",
    }
}

fn pricing_provenance(revisions: &[String]) -> String {
    let unique: BTreeSet<&str> = revisions
        .iter()
        .map(String::as_str)
        .filter(|revision| !revision.is_empty())
        .collect();
    if unique.is_empty() {
        "NONE".into()
    } else {
        unique.into_iter().collect::<Vec<_>>().join(",")
    }
}

fn validate_token(name: &str, value: &str) -> Result<(), InferenceLedgerError> {
    if value.trim().is_empty()
        || value.len() > MAX_TOKEN_LEN
        || value.chars().any(char::is_control)
        || value.chars().any(char::is_whitespace)
    {
        return Err(InferenceLedgerError::Invalid(format!("{name} is invalid")));
    }
    Ok(())
}

fn validate_text(name: &str, value: &str) -> Result<(), InferenceLedgerError> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_LEN || value.chars().any(char::is_control)
    {
        return Err(InferenceLedgerError::Invalid(format!("{name} is invalid")));
    }
    Ok(())
}

fn validate_json(name: &str, value: &str) -> Result<(), InferenceLedgerError> {
    if value.trim().is_empty() || value.len() > MAX_JSON_LEN {
        return Err(InferenceLedgerError::Invalid(format!("{name} is invalid")));
    }
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|_| InferenceLedgerError::Invalid(format!("{name} is not valid JSON")))
}

fn validate_sha256(name: &str, value: &str) -> Result<(), InferenceLedgerError> {
    if !is_sha256(value) {
        return Err(InferenceLedgerError::Invalid(format!(
            "{name} is not a lowercase SHA-256"
        )));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(authorized: u64, state: EnvelopeState) -> EnvelopeSnapshot {
        EnvelopeSnapshot {
            envelope_id: "envelope".into(),
            currency: "USD".into(),
            state,
            authorized_micros: UsdMicros::new(authorized).unwrap(),
            authorization_revision: 1,
        }
    }

    fn amounts(state: ReservationState, upper: u64, settled: Option<u64>) -> ReservationAmounts {
        ReservationAmounts {
            role: InferenceRole::Lead,
            provider: "opencode-go".into(),
            state,
            upper_bound_micros: UsdMicros::new(upper).unwrap(),
            settled_micros: settled.map(|value| UsdMicros::new(value).unwrap()),
        }
    }

    #[test]
    fn committed_holds_include_reserved_and_ambiguous_bounds() {
        let totals = spending_totals(
            UsdMicros::new(10_000).unwrap(),
            &[
                amounts(ReservationState::Settled, 5_000, Some(1_200)),
                amounts(ReservationState::Reserved, 2_000, None),
                amounts(ReservationState::Ambiguous, 3_000, None),
                amounts(ReservationState::Released, 9_999, None),
            ],
        )
        .unwrap();
        assert_eq!(totals.settled_micros.get(), 1_200);
        assert_eq!(totals.reserved_micros.get(), 2_000);
        assert_eq!(totals.ambiguous_micros.get(), 3_000);
        assert_eq!(totals.committed_micros.get(), 6_200);
        assert_eq!(totals.available_micros.get(), 3_800);
        assert_eq!(totals.unresolved_count, 2);
        assert_eq!(totals.overrun_micros.get(), 0);
    }

    #[test]
    fn overrun_is_visible_and_available_never_underflows() {
        let totals = spending_totals(
            UsdMicros::new(1_000).unwrap(),
            &[amounts(ReservationState::Settled, 1_000, Some(1_500))],
        )
        .unwrap();
        assert_eq!(totals.available_micros.get(), 0);
        assert_eq!(totals.overrun_micros.get(), 500);
    }

    #[test]
    fn admission_is_checked_and_zero_bounds_are_rejected() {
        let authorized = UsdMicros::new(1_000).unwrap();
        assert!(
            admit(
                UsdMicros::new(900).unwrap(),
                UsdMicros::new(100).unwrap(),
                authorized
            )
            .is_ok()
        );
        assert_eq!(
            admit(
                UsdMicros::new(900).unwrap(),
                UsdMicros::new(101).unwrap(),
                authorized
            )
            .unwrap_err(),
            InferenceLedgerError::Exhausted
        );
        assert!(matches!(
            admit(UsdMicros::ZERO, UsdMicros::ZERO, authorized),
            Err(InferenceLedgerError::Invalid(_))
        ));
        assert!(matches!(
            admit(
                UsdMicros::new(crate::execution_policy::MAX_USD_MICROS).unwrap(),
                UsdMicros::new(1).unwrap(),
                authorized
            ),
            Err(InferenceLedgerError::Invalid(_))
        ));
    }

    #[test]
    fn projection_publishes_basis_and_pricing_provenance() {
        let status = project_spending(
            &snapshot(10_000, EnvelopeState::Active),
            &[
                amounts(ReservationState::Reserved, 1_000, None),
                amounts(ReservationState::Settled, 1_000, Some(750)),
            ],
            &[
                "models.dev@2".into(),
                "models.dev@1".into(),
                "models.dev@2".into(),
            ],
            "2026-10-28T00:00:00Z",
        )
        .unwrap();
        assert_eq!(status.billing_basis, USD_BILLING_BASIS);
        assert_eq!(status.pricing_provenance, "models.dev@1,models.dev@2");
        assert_eq!(status.available_micros.get(), 8_250);
        assert_eq!(status.unresolved_count, 1);
    }

    #[test]
    fn role_and_provider_breakdown_cover_every_role() {
        let mut reservations = Vec::new();
        for (index, role) in InferenceRole::ALL.into_iter().enumerate() {
            reservations.push(ReservationAmounts {
                role,
                provider: if index == 0 { "opencode-go" } else { "codex" }.into(),
                state: ReservationState::Settled,
                upper_bound_micros: UsdMicros::new(100).unwrap(),
                settled_micros: Some(UsdMicros::new(50).unwrap()),
            });
        }
        let status = project_spending(
            &snapshot(10_000, EnvelopeState::Active),
            &reservations,
            &[],
            "2026-10-28T00:00:00Z",
        )
        .unwrap();
        assert_eq!(status.by_role.len(), 5);
        assert_eq!(status.by_provider.len(), 2);
        assert_eq!(
            status
                .by_role
                .iter()
                .map(|role| role.settled_micros.get())
                .sum::<u64>(),
            250
        );
        assert_eq!(status.by_role[0].key, "LEAD");
    }

    #[test]
    fn validators_reject_ambiguous_identity_and_invented_bounds() {
        let mut input = ReserveInferenceInput {
            envelope_id: "envelope".into(),
            operation_id: "operation".into(),
            idempotency_key: "idempotency".into(),
            request_sha256: "a".repeat(64),
            role: InferenceRole::Implementer,
            principal_id: "worker".into(),
            parent_operation_id: None,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            pricing_revision: "models.dev@1".into(),
            pricing_snapshot_json: "{}".into(),
            upper_bound_micros: UsdMicros::new(1_000).unwrap(),
            expires_at_ms: 2_000,
            authorization_revision: 1,
            scope_sha256: "b".repeat(64),
            worker_id: None,
            generation: None,
        };
        assert!(validate_reserve_input(&input).is_ok());
        input.upper_bound_micros = UsdMicros::ZERO;
        assert!(validate_reserve_input(&input).is_err());
        input.upper_bound_micros = UsdMicros::new(1_000).unwrap();
        input.expires_at_ms = 0;
        assert!(validate_reserve_input(&input).is_err());
        input.expires_at_ms = 2_000;
        input.operation_id = "two operations".into();
        assert!(validate_reserve_input(&input).is_err());
    }
}
