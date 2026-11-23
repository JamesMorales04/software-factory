//! Durable Owner-confirmed subscription admissions.
//!
//! A request-scoped admission covers the pre-Campaign Lead assessment. Approval
//! consumes it into a Campaign-scoped `ENABLED` funding policy in the same
//! transaction that creates the Campaign authorization. No debug feature or
//! wildcard participates in this path.

use super::*;

use crate::execution_policy::{
    ExecutionPolicyV3, FundingPolicy, OwnerBillingAssumption, ProviderEntitlement,
    QuotaObservation, SubscriptionAdmissionBasis, SubscriptionBinding, SubscriptionRoute,
};
use crate::subscription_admission::{TestModelRole, TestRouteBinding, role_allowed, test_route};
use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;
use uuid::Uuid;

const SCOPE_ADMISSION_MAX_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct ScopeAdmissionWrite {
    pub intake_request_id: String,
    pub lead_request_id: String,
    pub test_profile_id: String,
    pub execution_profile_id: String,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
    pub profile_revision_id: String,
    pub profile_hash: String,
    pub assumption: OwnerBillingAssumption,
    pub routes: Vec<TestRouteBinding>,
    /// Exact route-to-declaration bindings backing this admission. Empty for
    /// the legacy per-request confirmation.
    pub owner_extra_usage_declarations: Vec<OwnerExtraUsageBinding>,
    /// Stable fingerprint of the original client request plus the exact frozen
    /// profile identity. Reused for exact replay; it deliberately excludes the
    /// current settings revision, so revocation or unrelated settings changes
    /// do not deny a non-retroactive exact replay.
    pub request_fingerprint: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerExtraUsageBinding {
    pub role: String,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
    pub base_url: String,
    pub credential_fingerprint: String,
    pub declaration_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeAdmissionRecord {
    pub admission_id: String,
    pub request_fingerprint: String,
    pub intake_request_id: String,
    pub lead_request_id: String,
    pub test_profile_id: String,
    pub execution_profile_id: String,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
    pub profile_revision_id: String,
    pub profile_hash: String,
    pub assumption: OwnerBillingAssumption,
    pub routes: Vec<TestRouteBinding>,
    pub owner_extra_usage_declarations: Vec<OwnerExtraUsageBinding>,
    pub state: String,
    pub campaign_id: Option<String>,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub receipt_sha256: String,
}

fn canonical_routes(routes: &[TestRouteBinding]) -> Result<String, Error> {
    serde_json::to_string(routes).map_err(|_| Error::Invalid("test route binding encoding"))
}

/// Stable fingerprint of the original client request and frozen profile
/// identity. It is intentionally independent of the current settings revision,
/// so an exact replay survives revocation or unrelated settings changes while a
/// changed payload under the same idempotency key is refused.
#[allow(clippy::too_many_arguments)]
pub fn subscription_request_fingerprint(
    assumption_id: &str,
    confirmed_by: &str,
    confirmed_at_ms: i64,
    valid_until_ms: i64,
    extra_usage_disabled: bool,
    statement: &str,
    test_profile_id: &str,
    idempotency_key: &str,
    profile_revision_id: &str,
    profile_hash: &str,
    intake_request_id: &str,
    lead_request_id: &str,
    execution_profile_id: &str,
) -> String {
    let value = serde_json::json!({
        "schema": "factory-subscription-request-v1",
        "assumptionId": assumption_id,
        "confirmedBy": confirmed_by,
        "confirmedAtMs": confirmed_at_ms,
        "validUntilMs": valid_until_ms,
        "extraUsageDisabled": extra_usage_disabled,
        "statement": statement,
        "testProfileId": test_profile_id,
        "idempotencyKey": idempotency_key,
        "profileRevisionId": profile_revision_id,
        "profileHash": profile_hash,
        "intakeRequestId": intake_request_id,
        "leadRequestId": lead_request_id,
        "executionProfileId": execution_profile_id,
    });
    sha256_hex(&serde_json::to_vec(&value).expect("subscription request fingerprint"))
}

fn receipt_digest(
    input: &ScopeAdmissionWrite,
    assumption_json: &str,
    routes_json: &str,
) -> Result<String, Error> {
    let mut value = serde_json::json!({
        "schema": "factory-subscription-scope-admission-v1",
        "intakeRequestId": input.intake_request_id,
        "leadRequestId": input.lead_request_id,
        "testProfileId": input.test_profile_id,
        "executionProfileId": input.execution_profile_id,
        "provider": input.provider,
        "model": input.model,
        "settingsRevision": input.settings_revision,
        "profileRevisionId": input.profile_revision_id,
        "profileHash": input.profile_hash,
        "assumption": assumption_json,
        "routes": routes_json,
        "idempotencyKey": input.idempotency_key,
    });
    if !input.owner_extra_usage_declarations.is_empty() {
        value["ownerExtraUsageDeclarations"] =
            serde_json::to_value(&input.owner_extra_usage_declarations)
                .map_err(|_| Error::Invalid("Owner declaration binding encoding"))?;
    }
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| Error::Invalid("subscription admission receipt encoding"))?;
    Ok(sha256_hex(&bytes))
}

fn validate_write(input: &ScopeAdmissionWrite, now_ms: i64) -> Result<(), Error> {
    if input.intake_request_id.trim().is_empty()
        || input.lead_request_id.trim().is_empty()
        || input.test_profile_id.trim().is_empty()
        || input.execution_profile_id.trim().is_empty()
        || input.provider.trim().is_empty()
        || input.model.trim().is_empty()
        || input.settings_revision.trim().is_empty()
        || input.profile_revision_id.trim().is_empty()
        || input.profile_hash.len() != 64
        || !input
            .profile_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || input.idempotency_key.trim().is_empty()
    {
        return Err(Error::Invalid("subscription scope admission"));
    }
    if input.assumption.validate_at(now_ms).is_err() {
        return Err(Error::Forbidden(
            "Owner billing assumption is invalid or expired".into(),
        ));
    }
    if input.routes.is_empty() {
        return Err(Error::Forbidden(
            "subscription admission has no approved test routes".into(),
        ));
    }
    for route in &input.routes {
        route
            .validate()
            .map_err(|reason| Error::Forbidden(reason.into()))?;
    }
    let assessment_allowed = input.routes.iter().any(|route| {
        route.role == TestModelRole::Lead
            && route.provider == input.provider
            && route.model == input.model
    });
    if !assessment_allowed {
        return Err(Error::Forbidden(
            "assessment route is not an approved test route for this admission".into(),
        ));
    }
    Ok(())
}

impl Database {
    /// Persist one request-scoped admission. The caller must already have
    /// authenticated the operator and resolved the exact profile and routes.
    pub async fn create_subscription_scope_admission(
        &self,
        input: &ScopeAdmissionWrite,
    ) -> Result<ScopeAdmissionRecord, Error> {
        let now_ms = Utc::now().timestamp_millis();
        validate_write(input, now_ms)?;
        if input.assumption.valid_until_ms > now_ms + SCOPE_ADMISSION_MAX_LIFETIME_MS {
            return Err(Error::Invalid(
                "subscription admission validity exceeds the bounded test window",
            ));
        }
        let assumption_json = serde_json::to_string(&input.assumption)
            .map_err(|_| Error::Invalid("Owner assumption encoding"))?;
        let assumption_sha256 = sha256_hex(assumption_json.as_bytes());
        let routes_json = canonical_routes(&input.routes)?;
        let receipt_sha256 = receipt_digest(input, &assumption_json, &routes_json)?;
        if input.owner_extra_usage_declarations.len() > 16
            || input.owner_extra_usage_declarations.iter().any(|binding| {
                binding.provider != "opencode-go"
                    || binding.role.trim().is_empty()
                    || binding.model.trim().is_empty()
                    || binding.settings_revision.trim().is_empty()
                    || binding.declaration_id.trim().is_empty()
                    || binding.declaration_id.len() > 64
                    || binding.base_url.len() > 512
                    || binding.credential_fingerprint.len() > 128
            })
        {
            return Err(Error::Invalid(
                "subscription admission Owner declaration list is not bounded",
            ));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<(String, String, String, String, String)> = sqlx::query_as::query_as(
            "SELECT admission_id,receipt_sha256,state,idempotency_key,request_fingerprint
             FROM subscription_scope_admissions WHERE idempotency_key=?1",
        )
        .bind(&input.idempotency_key)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some((admission_id, existing_digest, state, _, existing_fingerprint)) = existing {
            if existing_fingerprint != input.request_fingerprint
                || existing_digest != receipt_sha256
                || state != "ACTIVE"
            {
                return Err(Error::Conflict(
                    "subscription admission idempotency key was used for different evidence",
                ));
            }
            let record = load_scope_admission(&mut transaction, &admission_id)
                .await?
                .ok_or(Error::NotFound)?;
            transaction.commit().await?;
            return Ok(record);
        }
        // Fence revocation for new admissions only: every cited declaration must
        // still be active and belong to the exact route configuration. A replay
        // of an already-recorded admission returns it above and is not cancelled
        // retroactively by a later revocation.
        for binding in &input.owner_extra_usage_declarations {
            let active: Option<(i64,)> = sqlx::query_as::query_as(
                "SELECT 1 FROM provider_extra_usage_declarations
                 WHERE declaration_id=?1 AND provider=?2 AND base_url=?3
                   AND credential_fingerprint=?4 AND revoked_at IS NULL",
            )
            .bind(&binding.declaration_id)
            .bind(&binding.provider)
            .bind(&binding.base_url)
            .bind(&binding.credential_fingerprint)
            .fetch_optional(&mut *transaction)
            .await?;
            if active.is_none() {
                return Err(Error::Conflict(
                    "an Owner Extra Usage declaration was revoked or no longer matches its route",
                ));
            }
            let current: Option<(i64, String, String)> = sqlx::query_as::query_as(
                "SELECT revision, principal_id, statement
                 FROM provider_settings JOIN provider_extra_usage_declarations
                   ON provider_extra_usage_declarations.provider = provider_settings.provider
                 WHERE provider_settings.provider=?1 AND provider_settings.enabled=1
                   AND provider_settings.base_url=?2
                   AND provider_settings.credential_fingerprint=?3
                   AND provider_extra_usage_declarations.declaration_id=?4",
            )
            .bind(&binding.provider)
            .bind(&binding.base_url)
            .bind(&binding.credential_fingerprint)
            .bind(&binding.declaration_id)
            .fetch_optional(&mut *transaction)
            .await?;
            let Some((current_revision, declaration_principal, declaration_statement)) = current
            else {
                return Err(Error::Conflict(
                    "an Owner Extra Usage declaration no longer matches the current provider settings",
                ));
            };
            if binding.settings_revision.parse::<i64>().ok() != Some(current_revision) {
                return Err(Error::Conflict(
                    "an Owner Extra Usage declaration was bound to a stale settings revision",
                ));
            }
            if input.assumption.confirmed_by
                != format!("{declaration_principal}:persisted-owner-declaration")
                || input.assumption.statement != declaration_statement
            {
                return Err(Error::Conflict(
                    "the Owner billing assumption does not match its declaration",
                ));
            }
            if !input.routes.iter().any(|route| {
                route.role.as_str() == binding.role
                    && route.provider == binding.provider
                    && route.model == binding.model
                    && route.settings_revision == binding.settings_revision
            }) {
                return Err(Error::Conflict(
                    "an Owner Extra Usage declaration does not match an admitted route",
                ));
            }
        }
        if !input.owner_extra_usage_declarations.is_empty() {
            for route in input
                .routes
                .iter()
                .filter(|route| route.provider == "opencode-go")
            {
                let covered = input.owner_extra_usage_declarations.iter().any(|binding| {
                    binding.role == route.role.as_str()
                        && binding.provider == route.provider
                        && binding.model == route.model
                        && binding.settings_revision == route.settings_revision
                });
                if !covered {
                    return Err(Error::Conflict(
                        "an OpenCode Go route has no exact Owner Extra Usage binding",
                    ));
                }
            }
        }
        let admission_id = Uuid::new_v4().to_string();
        let expires_at_ms = input
            .assumption
            .valid_until_ms
            .min(now_ms + SCOPE_ADMISSION_MAX_LIFETIME_MS);
        sqlx::query::query(
            "INSERT INTO subscription_scope_admissions
             (admission_id,intake_request_id,lead_request_id,test_profile_id,execution_profile_id,provider,model,
              settings_revision,profile_revision_id,profile_hash,assumption_json,
              assumption_sha256,routes_json,owner_extra_usage_declarations_json,
              request_fingerprint,state,campaign_id,created_at_ms,expires_at_ms,
              consumed_at_ms,idempotency_key,receipt_sha256)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'ACTIVE',NULL,?16,?17,NULL,?18,?19)",
        )
        .bind(&admission_id)
        .bind(&input.intake_request_id)
        .bind(&input.lead_request_id)
        .bind(&input.test_profile_id)
        .bind(&input.execution_profile_id)
        .bind(&input.provider)
        .bind(&input.model)
        .bind(&input.settings_revision)
        .bind(&input.profile_revision_id)
        .bind(&input.profile_hash)
        .bind(&assumption_json)
        .bind(&assumption_sha256)
        .bind(&routes_json)
        .bind(
            serde_json::to_string(&input.owner_extra_usage_declarations)
                .map_err(|_| Error::Invalid("Owner declaration binding encoding"))?,
        )
        .bind(&input.request_fingerprint)
        .bind(now_ms)
        .bind(expires_at_ms)
        .bind(&input.idempotency_key)
        .bind(&receipt_sha256)
        .execute(&mut *transaction)
        .await?;
        let record = load_scope_admission(&mut transaction, &admission_id)
            .await?
            .ok_or(Error::NotFound)?;
        transaction.commit().await?;
        Ok(record)
    }

    /// Look up one admission by idempotency key for an exact replay. The
    /// caller compares the stored request/profile identity before returning it,
    /// so a changed payload with a reused key is still refused.
    pub async fn subscription_scope_admission_by_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<ScopeAdmissionRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let id: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT admission_id FROM subscription_scope_admissions WHERE idempotency_key=?1",
        )
        .bind(idempotency_key)
        .fetch_optional(&mut *connection)
        .await?;
        match id {
            Some((admission_id,)) => load_scope_admission(&mut connection, &admission_id).await,
            None => Ok(None),
        }
    }

    /// True when an exact active admission covers this request/Lead scope and
    /// the configured assessment route. It is used to skip the legacy monetary
    /// envelope/pricing preflight for subscription assessment.
    pub async fn subscription_scope_admitted(
        &self,
        scope: &str,
        provider: &str,
        model: &str,
    ) -> Result<bool, Error> {
        let mut connection = self.pool.acquire().await?;
        match require_scope_admission_in_tx(&mut connection, scope, provider, model).await {
            Ok(()) => Ok(true),
            Err(Error::SubscriptionFunding(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Resolve one still-active admission bound to this intake or Lead request.
    /// Approval uses it only when the operator did not name an admission
    /// explicitly; the start transaction revalidates and consumes the exact
    /// row, so this lookup cannot widen authority.
    pub async fn active_subscription_admission_for_request(
        &self,
        scope: &str,
    ) -> Result<Option<String>, Error> {
        let now_ms = Utc::now().timestamp_millis();
        let row: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT admission_id FROM subscription_scope_admissions
             WHERE (intake_request_id=?1 OR lead_request_id=?1)
               AND state='ACTIVE' AND expires_at_ms>?2
             ORDER BY created_at_ms DESC,admission_id DESC LIMIT 1",
        )
        .bind(scope)
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(id,)| id))
    }

    /// Latest admission for one intake/Lead scope. ACTIVE rows are preferred;
    /// otherwise the newest row is returned so a truthful projection can show
    /// ADMISSION_INACTIVE/ADMISSION_EXPIRED. This is a read-only description;
    /// the dispatch guard still re-evaluates the exact admission.
    pub async fn subscription_scope_admission_for_request(
        &self,
        scope: &str,
    ) -> Result<Option<ScopeAdmissionRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let id: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT admission_id FROM subscription_scope_admissions
             WHERE intake_request_id=?1 OR lead_request_id=?1
             ORDER BY CASE WHEN state='ACTIVE' THEN 0 ELSE 1 END,
                      created_at_ms DESC, admission_id DESC
             LIMIT 1",
        )
        .bind(scope)
        .fetch_optional(&mut *connection)
        .await?;
        match id {
            Some((admission_id,)) => load_scope_admission(&mut connection, &admission_id).await,
            None => Ok(None),
        }
    }

    pub async fn subscription_scope_admission(
        &self,
        admission_id: &str,
    ) -> Result<ScopeAdmissionRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        load_scope_admission(&mut connection, admission_id)
            .await?
            .ok_or(Error::NotFound)
    }
}

pub(super) async fn load_scope_admission(
    connection: &mut SqliteConnection,
    admission_id: &str,
) -> Result<Option<ScopeAdmissionRecord>, Error> {
    let row = sqlx::query::query(
        "SELECT admission_id,intake_request_id,lead_request_id,test_profile_id,execution_profile_id,provider,model,
                settings_revision,profile_revision_id,profile_hash,assumption_json,
                assumption_sha256,routes_json,owner_extra_usage_declarations_json,
                request_fingerprint,state,campaign_id,created_at_ms,expires_at_ms,
                receipt_sha256
         FROM subscription_scope_admissions WHERE admission_id=?1",
    )
    .bind(admission_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let assumption_json: String = row.try_get("assumption_json")?;
    let assumption_sha256: String = row.try_get("assumption_sha256")?;
    if sha256_hex(assumption_json.as_bytes()) != assumption_sha256 {
        return Err(Error::Invalid("subscription admission assumption digest"));
    }
    let assumption: OwnerBillingAssumption = serde_json::from_str(&assumption_json)
        .map_err(|_| Error::Invalid("subscription admission assumption encoding"))?;
    let routes_json: String = row.try_get("routes_json")?;
    let routes: Vec<TestRouteBinding> = serde_json::from_str(&routes_json)
        .map_err(|_| Error::Invalid("subscription admission route encoding"))?;
    let owner_extra_usage_declarations: Vec<OwnerExtraUsageBinding> =
        serde_json::from_str(&row.try_get::<String, _>("owner_extra_usage_declarations_json")?)
            .map_err(|_| Error::Invalid("subscription admission Owner declaration encoding"))?;
    Ok(Some(ScopeAdmissionRecord {
        admission_id: row.try_get("admission_id")?,
        request_fingerprint: row.try_get("request_fingerprint")?,
        intake_request_id: row.try_get("intake_request_id")?,
        lead_request_id: row.try_get("lead_request_id")?,
        test_profile_id: row.try_get("test_profile_id")?,
        execution_profile_id: row.try_get("execution_profile_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        settings_revision: row.try_get("settings_revision")?,
        profile_revision_id: row.try_get("profile_revision_id")?,
        profile_hash: row.try_get("profile_hash")?,
        assumption,
        routes,
        owner_extra_usage_declarations,
        state: row.try_get("state")?,
        campaign_id: row.try_get("campaign_id")?,
        created_at_ms: row.try_get("created_at_ms")?,
        expires_at_ms: row.try_get("expires_at_ms")?,
        receipt_sha256: row.try_get("receipt_sha256")?,
    }))
}

/// The pre-Campaign Lead assessment gate. It checks the exact scope, route,
/// Owner assumption and expiry. No wildcard or debug feature is consulted.
pub(super) async fn require_scope_admission_in_tx(
    connection: &mut SqliteConnection,
    scope: &str,
    provider: &str,
    model: &str,
) -> Result<(), Error> {
    let now_ms = Utc::now().timestamp_millis();
    let rows = sqlx::query::query(
        "SELECT admission_id,provider,model,assumption_json,assumption_sha256,routes_json,
                state,expires_at_ms
         FROM subscription_scope_admissions
         WHERE (intake_request_id=?1 OR lead_request_id=?1) AND state='ACTIVE'",
    )
    .bind(scope)
    .fetch_all(&mut *connection)
    .await?;
    for row in rows {
        let expires_at_ms: i64 = row.try_get("expires_at_ms")?;
        if expires_at_ms <= now_ms {
            continue;
        }
        let provider_row: String = row.try_get("provider")?;
        let model_row: String = row.try_get("model")?;
        if provider_row != provider || model_row != model {
            continue;
        }
        let assumption_json: String = row.try_get("assumption_json")?;
        let digest: String = row.try_get("assumption_sha256")?;
        if sha256_hex(assumption_json.as_bytes()) != digest {
            return Err(Error::Invalid("subscription admission assumption digest"));
        }
        let assumption: OwnerBillingAssumption = serde_json::from_str(&assumption_json)
            .map_err(|_| Error::Invalid("subscription admission assumption encoding"))?;
        if assumption.validate_at(now_ms).is_err() {
            continue;
        }
        let routes_json: String = row.try_get("routes_json")?;
        let routes: Vec<TestRouteBinding> = serde_json::from_str(&routes_json)
            .map_err(|_| Error::Invalid("subscription admission route encoding"))?;
        let settings: Option<(i64, String)> = sqlx::query_as::query_as(
            "SELECT revision,base_url FROM provider_settings WHERE provider=?1",
        )
        .bind(provider)
        .fetch_optional(&mut *connection)
        .await?;
        let Some((revision, base_url)) = settings else {
            continue;
        };
        if provider == "opencode-go"
            && !crate::provider_catalog::is_canonical_opencode_go_base_url(&base_url)
            && !crate::provider_catalog::is_test_upstream_url(&base_url)
        {
            return Err(Error::Invalid(
                "OpenCode Go admission route is not the canonical subscription endpoint",
            ));
        }
        if routes.iter().any(|route| {
            route.provider == provider
                && route.model == model
                && route.role == TestModelRole::Lead
                && route.settings_revision == revision.to_string()
                && route.validate().is_ok()
        }) {
            return Ok(());
        }
    }
    Err(crate::execution_policy::SubscriptionFundingRefusal::BindingRequired.into())
}

/// Consume a request admission into the Campaign transaction. The caller must
/// have inserted the Campaign row and must not yet have inserted an
/// authorization.
#[allow(clippy::too_many_arguments)]
pub(super) async fn consume_scope_admission_in_tx(
    connection: &mut SqliteConnection,
    admission_id: &str,
    campaign_id: &str,
    intake_request_id: &str,
    lead_request_id: &str,
    execution_profile_id: &str,
    profile_revision_id: &str,
    profile_hash: &str,
    implementer_provider: &str,
    implementer_model: &str,
    implementer_settings_revision: &str,
) -> Result<ScopeAdmissionRecord, Error> {
    let now_ms = Utc::now().timestamp_millis();
    let record = load_scope_admission(connection, admission_id)
        .await?
        .ok_or(Error::NotFound)?;
    if record.state != "ACTIVE" {
        return Err(Error::Conflict(
            "subscription admission is not active or was already consumed",
        ));
    }
    if record.expires_at_ms <= now_ms || record.assumption.validate_at(now_ms).is_err() {
        return Err(Error::Forbidden(
            "subscription admission or Owner assumption is expired".into(),
        ));
    }
    if record.intake_request_id != intake_request_id
        || record.lead_request_id != lead_request_id
        || record.execution_profile_id != execution_profile_id
        || record.profile_revision_id != profile_revision_id
        || record.profile_hash != profile_hash
    {
        return Err(Error::Forbidden(
            "subscription admission does not bind this request and profile".into(),
        ));
    }
    let implementer_allowed = record.routes.iter().any(|route| {
        route.role == TestModelRole::Implementer
            && route.provider == implementer_provider
            && route.model == implementer_model
            && route.settings_revision == implementer_settings_revision
            && route.validate().is_ok()
    });
    if !implementer_allowed {
        return Err(Error::Forbidden(
            "subscription admission does not cover the frozen implementer route".into(),
        ));
    }
    let changed = sqlx::query::query(
        "UPDATE subscription_scope_admissions
         SET state='CONSUMED',campaign_id=?1,consumed_at_ms=?2
         WHERE admission_id=?3 AND state='ACTIVE'",
    )
    .bind(campaign_id)
    .bind(now_ms)
    .bind(admission_id)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(Error::Conflict(
            "subscription admission was consumed concurrently",
        ));
    }
    Ok(record)
}

/// Build and insert the Campaign-scoped ENABLED policy before the Campaign
/// authorization. This is the only path that enables dispatch for an
/// Owner-confirmed admission.
#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_enabled_campaign_admission_in_tx(
    connection: &mut SqliteConnection,
    campaign_id: &str,
    authorization_id: &str,
    admission: &ScopeAdmissionRecord,
    implementer_provider: &str,
    implementer_model: &str,
    settings_revision: &str,
    scope_sha256: &str,
) -> Result<(), Error> {
    let now_ms = Utc::now().timestamp_millis();
    if implementer_provider == "codex" {
        return Err(Error::Forbidden(
            "Codex API-key transport cannot be admitted as ChatGPT subscription funding".into(),
        ));
    }
    let route = test_route(implementer_provider, implementer_model).ok_or_else(|| {
        Error::Forbidden("implementer route is outside the test allowlist".into())
    })?;
    if !role_allowed(route, TestModelRole::Implementer) {
        return Err(Error::Forbidden(
            "implementer route is not executable by the current adapter".into(),
        ));
    }
    let subscription_route = match implementer_provider {
        "opencode-go" => SubscriptionRoute::OpenCodeGo,
        _ => {
            return Err(Error::Forbidden(
                "implementer route has no subscription funding route".into(),
            ));
        }
    };
    let binding = SubscriptionBinding {
        route: subscription_route,
        entitlement: ProviderEntitlement::Unknown {
            reason: "Owner-confirmed configuration assumption; not provider-issued evidence".into(),
        },
        provider_account_id: String::new(),
        plan_id: "OWNER_CONFIRMED".into(),
        model_id: implementer_model.to_owned(),
        settings_revision: settings_revision.to_owned(),
        entitlement_verified: false,
        no_charge_route_verified: false,
        extra_usage_disabled: true,
        api_key_route_disabled: true,
        quota: QuotaObservation::Unknown {
            observed_at_ms: now_ms,
            reason:
                "provider quota not required for Owner-confirmed admission; exhaustion still pauses"
                    .into(),
        },
        admission_basis: SubscriptionAdmissionBasis::OwnerConfirmedConfiguration {
            assumption: admission.assumption.clone(),
        },
    };
    let binding_json = serde_json::to_string(&binding)
        .map_err(|_| Error::Invalid("subscription binding encoding"))?;
    let binding_sha256 = sha256_hex(binding_json.as_bytes());
    let policy = ExecutionPolicyV3 {
        policy_id: format!("subscription:{authorization_id}"),
        authorization_id: authorization_id.to_owned(),
        authorization_revision: 1,
        funding: FundingPolicy::Subscription {
            grant_id: format!("owner-admission:{}", admission.admission_id),
            binding_sha256,
            binding: Box::new(binding),
        },
        scope_sha256: scope_sha256.to_owned(),
        progress: crate::execution_policy::ProgressPolicy {
            diagnose_after_stagnant_checkpoints: std::num::NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: std::num::NonZeroU32::new(2).unwrap(),
            observation_timeout_secs: std::num::NonZeroU32::new(3_600).unwrap(),
            autonomy: Default::default(),
        },
        operations: crate::execution_policy::OperationLimits {
            model_request_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
            tool_call_timeout_secs: std::num::NonZeroU32::new(600).unwrap(),
            max_request_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_response_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_generation_tokens: std::num::NonZeroU32::new(32_768).unwrap(),
            max_in_flight_operations: std::num::NonZeroU32::new(1).unwrap(),
        },
    };
    let policy_json = serde_json::to_string(&policy)
        .map_err(|_| Error::Invalid("subscription policy encoding"))?;
    let policy_sha256 = sha256_hex(policy_json.as_bytes());
    sqlx::query::query(
        "INSERT INTO subscription_funding_policies
         (campaign_id,authorization_id,policy_json,policy_sha256,state,reason,
          observed_at_ms,resets_at_ms,created_at_ms,updated_at_ms)
         VALUES (?1,?2,?3,?4,'ENABLED',?5,?6,NULL,?6,?6)",
    )
    .bind(campaign_id)
    .bind(authorization_id)
    .bind(&policy_json)
    .bind(&policy_sha256)
    .bind(&admission.admission_id)
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    sqlx::query::query(
        "INSERT INTO inference_campaign_modes(campaign_id,mode,classified_at_ms,evidence_sha256)
         VALUES (?1,'SUBSCRIPTION',?2,?3)
         ON CONFLICT(campaign_id) DO UPDATE SET mode='SUBSCRIPTION',
             classified_at_ms=excluded.classified_at_ms,
             evidence_sha256=excluded.evidence_sha256",
    )
    .bind(campaign_id)
    .bind(now_ms)
    .bind(&admission.receipt_sha256)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_policy::{OwnerBillingAssumption, OwnerControlledExtraUsage};
    use crate::subscription_admission::{TestModelRole, TestRouteBinding};

    async fn fixture() -> (std::path::PathBuf, super::super::Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-admission-binding-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = super::super::Database::open(&path).await.unwrap();
        let settings = super::super::ProviderSettingsWrite {
            provider: "opencode-go".into(),
            kind: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            base_url: "https://opencode.ai/inference/go/openai/v1".into(),
            auth_mode: "api_key".into(),
            credential_ciphertext: "ciphertext".into(),
            credential_nonce: "nonce".into(),
            credential_fingerprint: "fp".into(),
            enabled: true,
            is_default: true,
            max_turns: 8,
            max_input_tokens: 128_000,
            max_output_tokens: 8_192,
            max_cost_micros: 1_000_000,
            input_micros_per_million: 1,
            output_micros_per_million: 1,
        };
        db.save_provider_settings(&settings, "owner").await.unwrap();
        let _declaration = db
            .declare_provider_extra_usage(super::super::DeclareProviderExtraUsage {
                provider: "opencode-go",
                base_url: "https://opencode.ai/inference/go/openai/v1",
                credential_fingerprint: "fp",
                principal_id: "operator-token",
                statement:
                    "I confirm OpenCode Go Extra Usage is disabled for this provider configuration.",
                settings_revision: 1,
                idempotency_key: "binding-declaration",
            })
            .await
            .unwrap();
        (path, db)
    }

    fn route(role: TestModelRole) -> TestRouteBinding {
        TestRouteBinding {
            role,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: "1".into(),
            transport: "OPENCODE_GO_SUBSCRIPTION".into(),
        }
    }

    fn write(
        bindings: Vec<OwnerExtraUsageBinding>,
        routes: Vec<TestRouteBinding>,
    ) -> ScopeAdmissionWrite {
        let now = Utc::now().timestamp_millis();
        ScopeAdmissionWrite {
            intake_request_id: "intake".into(),
            lead_request_id: "lead".into(),
            test_profile_id: "opencode-go-subscription".into(),
            execution_profile_id: "profile".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: "model-gateway:opencode-go:deepseek-v4.1-flash".into(),
            profile_revision_id: "revision".into(),
            profile_hash: "a".repeat(64),
            assumption: OwnerBillingAssumption {
                assumption_id: "binding-assumption".into(),
                confirmed_by: "operator-token:persisted-owner-declaration".into(),
                confirmed_at_ms: now - 1_000,
                valid_until_ms: now + 60_000,
                extra_usage: OwnerControlledExtraUsage::Disabled,
                statement:
                    "I confirm OpenCode Go Extra Usage is disabled for this provider configuration."
                        .into(),
            },
            routes,
            owner_extra_usage_declarations: bindings,
            request_fingerprint: "f".repeat(64),
            idempotency_key: "binding-admission".into(),
        }
    }

    #[tokio::test]
    async fn stale_foreign_and_partial_bindings_are_refused() {
        let (path, db) = fixture().await;
        let declaration = db
            .active_provider_extra_usage_declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fp",
            )
            .await
            .unwrap()
            .unwrap();
        let binding = |settings_revision: &str, declaration_id: &str| OwnerExtraUsageBinding {
            role: "lead".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: settings_revision.into(),
            base_url: "https://opencode.ai/inference/go/openai/v1".into(),
            credential_fingerprint: "fp".into(),
            declaration_id: declaration_id.into(),
        };
        let lead = route(TestModelRole::Lead);
        assert!(
            db.create_subscription_scope_admission(&write(
                vec![binding("999", &declaration.declaration_id)],
                vec![lead.clone()],
            ))
            .await
            .is_err(),
            "stale revision"
        );
        assert!(
            db.create_subscription_scope_admission(&write(
                vec![binding("1", "foreign-declaration")],
                vec![lead.clone()],
            ))
            .await
            .is_err(),
            "foreign declaration"
        );
        assert!(
            db.create_subscription_scope_admission(&write(
                vec![binding("1", &declaration.declaration_id)],
                vec![lead, route(TestModelRole::Implementer)],
            ))
            .await
            .is_err(),
            "partial route coverage"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
