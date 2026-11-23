//! Read-only subscription funding readiness.
//!
//! The static provider projection describes one stored provider connection. It
//! is deliberately `UNSCOPED`: it never decides whether a particular request or
//! Campaign may dispatch. The request-scoped projection consumes the durable
//! subscription admission and the authoritative Core scope-admission decision
//! for one exact intake/Lead scope. Neither projection is provider-issued
//! entitlement, no-overage or quota evidence.
//!
//! Decision mechanism: `DETERMINISTIC`. Nothing here calls a provider, trusts a
//! model response or turns an Owner Console observation into verified evidence.

use serde::Serialize;

use crate::db::ScopeAdmissionRecord;
use crate::execution_policy::OwnerControlledExtraUsage;
use crate::subscription_admission::{TestModelRole, role_allowed, test_route};

/// Provider connection facts Core owns. Credential material is never included.
#[derive(Debug, Clone)]
pub struct ProviderFundingFacts {
    pub kind: String,
    pub configured: bool,
    pub enabled: bool,
    pub model: String,
    pub settings_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingQuotaStatus {
    pub state: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionFundingStatus {
    /// This endpoint describes one provider connection, not one request. It
    /// cannot decide whether a request or Campaign may dispatch.
    pub scope: &'static str,
    pub request_scoped: bool,
    pub provider: String,
    pub kind: String,
    pub route: &'static str,
    pub configured: bool,
    pub enabled: bool,
    pub model: String,
    pub settings_revision: u32,
    /// Provider settings only support an API-key credential today. A stored
    /// key is not evidence that the account is billed through a subscription.
    pub authentication_class: &'static str,
    /// Always false: no provider-issued verification contract is implemented.
    pub core_verifiable: bool,
    /// Always false: this unscoped projection cannot grant request dispatch.
    pub admissible: bool,
    pub entitlement: &'static str,
    pub extra_usage: &'static str,
    pub balance_fallback: &'static str,
    pub no_charge_route: &'static str,
    pub quota: FundingQuotaStatus,
    pub missing_evidence: Vec<&'static str>,
    pub note: &'static str,
}

const MISSING_EVIDENCE: &[&str] = &[
    "OWNER_CONFIRMED_EXTRA_USAGE_ASSUMPTION",
    "EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION",
];

const NOTE: &str = "UNSCOPED PROVIDER STATUS: this projection cannot decide a request or Campaign. Core does not require a provider-issued no-overage certificate. OpenCode Go uses API-key authentication on the subscription endpoint; the Owner confirms Extra Usage is disabled in the provider Console. Dispatch remains blocked until an authenticated operator records a request-scoped bounded admission and exact route binding. Readiness for a specific request must be obtained from the request-scoped projection.";

/// Project the supported subscription routes. `None` means the provider is
/// not a subscription route at all, which the HTTP layer reports as 404.
pub fn project(
    provider: &str,
    facts: Option<&ProviderFundingFacts>,
) -> Option<SubscriptionFundingStatus> {
    let route = match provider {
        "opencode-go" => "GO_SUBSCRIPTION_INCLUDED",
        "codex" => "CHATGPT_PLAN_INCLUDED",
        _ => return None,
    };
    let mut missing_evidence = MISSING_EVIDENCE.to_vec();
    if facts.is_none() || !facts.is_some_and(|facts| facts.configured) {
        missing_evidence.insert(0, "PROVIDER_CONNECTION");
    }
    Some(SubscriptionFundingStatus {
        scope: "UNSCOPED_PROVIDER_STATUS",
        request_scoped: false,
        provider: provider.to_owned(),
        kind: facts.map(|facts| facts.kind.clone()).unwrap_or_else(|| {
            if provider == "codex" {
                "codex".into()
            } else {
                "opencode".into()
            }
        }),
        route,
        configured: facts.is_some_and(|facts| facts.configured),
        enabled: facts.is_some_and(|facts| facts.enabled),
        model: facts.map(|facts| facts.model.clone()).unwrap_or_default(),
        settings_revision: facts.map(|facts| facts.settings_revision).unwrap_or(0),
        authentication_class: if facts.is_some_and(|facts| facts.configured) {
            "API_KEY"
        } else {
            "UNKNOWN"
        },
        core_verifiable: false,
        admissible: false,
        entitlement: "NOT_RECORDED",
        extra_usage: "OWNER_ASSUMED_DISABLED_PENDING_ADMISSION",
        balance_fallback: "NOT_RECORDED",
        no_charge_route: "EXACT_SUBSCRIPTION_ROUTE_ONLY",
        quota: FundingQuotaStatus {
            state: "UNKNOWN",
            reason: "NO_PROVIDER_QUOTA_OBSERVATION_REQUIRED",
        },
        missing_evidence,
        note: NOTE,
    })
}

/// Request-scoped funding readiness schema. This projection reports the exact
/// admission evidence that Core records for one intake/Lead scope. It is a
/// read-only description, not a dispatch decision: the model gateway and Lead
/// preflight re-evaluate the durable admission before any provider call.
pub const REQUEST_FUNDING_SCHEMA_VERSION: u32 = 1;

const REQUEST_NOTE: &str = "Request-scoped readiness is derived from the durable Owner-confirmed admission and current provider settings. Core does not treat an API key, a provider Console observation or this projection as provider-issued no-overage evidence. requestScopedAdmissible mirrors the existing Core scope-admission guard and never bypasses it.";

const RECOVERY_SCOPE: &str = "Open this readiness through an intake request so both the intake and Lead request scope are known. An unscoped provider status cannot decide a request.";
const RECOVERY_ADMISSION: &str = "Record a new bounded, request-scoped Owner-confirmed subscription admission for the exact provider, model and current settings revision. A previous admission is never extended or reused.";
const RECOVERY_OWNER: &str = "Record a new Owner Extra Usage assertion that is currently valid and confirms Extra Usage is disabled. Core cannot verify the provider Console observation.";
const RECOVERY_PROVIDER: &str =
    "Configure and enable the exact provider route before requesting a new admission.";
const RECOVERY_ROUTE: &str = "Use the exact canonical subscription route and an admitted Lead route for the current settings revision.";
const RECOVERY_ADMITTED: &str = "No recovery is required for this bounded admission. Provider dispatch still requires Core's dispatch guard and remains outside this projection.";

const MISSING_SCOPE: &str = "EXACT_REQUEST_SCOPE";
const MISSING_ADMISSION: &str = "EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION";
const MISSING_ACTIVE_ADMISSION: &str = "ACTIVE_SUBSCRIPTION_ADMISSION";
const MISSING_ADMISSION_VALIDITY: &str = "SUBSCRIPTION_ADMISSION_VALIDITY";
const MISSING_OWNER_ASSUMPTION: &str = "OWNER_CONFIRMED_EXTRA_USAGE_ASSUMPTION";
const MISSING_EXTRA_USAGE_DISABLED: &str = "OWNER_CONFIRMED_EXTRA_USAGE_DISABLED";
const MISSING_PROVIDER_CONNECTION: &str = "PROVIDER_CONNECTION";
const MISSING_PROVIDER_ENABLED: &str = "PROVIDER_ENABLED";
const MISSING_EXACT_SCOPE: &str = "EXACT_REQUEST_SCOPE_BINDING";
const MISSING_EXACT_ROUTE: &str = "EXACT_PROVIDER_MODEL_ROUTE";
const MISSING_SETTINGS_REVISION: &str = "CURRENT_PROVIDER_SETTINGS_REVISION";
const MISSING_CANONICAL_ROUTE: &str = "CANONICAL_SUBSCRIPTION_ROUTE";
const MISSING_LEAD_ROUTE: &str = "LEAD_ROUTE_ADMISSION";
const MISSING_SCOPE_GUARD: &str = "CORE_SCOPE_ADMISSION_GUARD";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestFundingVerdict {
    AdmittedForRequest,
    ScopeRequired,
    AdmissionMissing,
    AdmissionInactive,
    AdmissionExpired,
    OwnerAssumptionInvalid,
    ProviderNotConfigured,
    ProviderDisabled,
    ScopeMismatch,
    RouteMismatch,
    SettingsRevisionMismatch,
    NonCanonicalRoute,
    RouteNotAdmitted,
    ExtraUsageNotDisabled,
    BindingRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFundingScope {
    pub intake_request_id: String,
    pub lead_request_id: String,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFundingRouteView {
    pub role: String,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
    pub transport: String,
    pub valid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFundingDeclarationView {
    pub declaration_id: String,
    pub role: String,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFundingAdmissionView {
    pub admission_id: String,
    pub state: String,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
    pub valid_until_ms: i64,
    pub campaign_id: Option<String>,
    pub extra_usage: OwnerControlledExtraUsage,
    pub confirmed_by: String,
    pub statement: String,
    pub owner_extra_usage_declarations: Vec<RequestFundingDeclarationView>,
    pub routes: Vec<RequestFundingRouteView>,
    /// The only admission digest exposed. Credential and request fingerprints
    /// are deliberately absent.
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFundingStatus {
    pub schema_version: u32,
    pub scope: RequestFundingScope,
    pub provider: String,
    pub model: String,
    pub request_scoped: bool,
    /// Always false: an Owner Console assumption is not provider-issued
    /// entitlement, no-overage or quota evidence.
    pub core_verifiable: bool,
    /// Mirrors the existing Core scope-admission guard for this exact Lead
    /// route. This is not dispatch authority; the guard remains authoritative.
    pub request_scoped_admissible: bool,
    pub verdict: RequestFundingVerdict,
    pub admission: Option<RequestFundingAdmissionView>,
    pub missing_evidence: Vec<&'static str>,
    pub recovery: &'static str,
    pub provenance: Vec<&'static str>,
    pub note: &'static str,
}

/// Provider connection facts needed by the request-scoped projection. The
/// canonical-route flag is computed by Core; the base URL and credentials are
/// never exposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFundingProviderFacts {
    pub configured: bool,
    pub enabled: bool,
    pub settings_revision: String,
    pub canonical_route: bool,
}

#[derive(Debug, Clone)]
pub struct RequestFundingFacts<'a> {
    pub intake_request_id: &'a str,
    pub lead_request_id: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub provider_facts: Option<&'a RequestFundingProviderFacts>,
    pub admission: Option<&'a ScopeAdmissionRecord>,
    /// Authoritative result of `subscription_scope_admitted` /
    /// `require_scope_admission_in_tx` for this exact intake/Lead route.
    pub scope_admitted: bool,
    pub now_ms: i64,
}

fn admission_view(admission: &ScopeAdmissionRecord) -> RequestFundingAdmissionView {
    RequestFundingAdmissionView {
        admission_id: admission.admission_id.clone(),
        state: admission.state.clone(),
        provider: admission.provider.clone(),
        model: admission.model.clone(),
        settings_revision: admission.settings_revision.clone(),
        valid_until_ms: admission.expires_at_ms,
        campaign_id: admission.campaign_id.clone(),
        extra_usage: admission.assumption.extra_usage,
        confirmed_by: admission.assumption.confirmed_by.clone(),
        statement: admission.assumption.statement.clone(),
        owner_extra_usage_declarations: admission
            .owner_extra_usage_declarations
            .iter()
            .map(|binding| RequestFundingDeclarationView {
                declaration_id: binding.declaration_id.clone(),
                role: binding.role.clone(),
                provider: binding.provider.clone(),
                model: binding.model.clone(),
                settings_revision: binding.settings_revision.clone(),
            })
            .collect(),
        routes: admission
            .routes
            .iter()
            .map(|route| RequestFundingRouteView {
                role: route.role.as_str().to_owned(),
                provider: route.provider.clone(),
                model: route.model.clone(),
                settings_revision: route.settings_revision.clone(),
                transport: route.transport.clone(),
                valid: route.validate().is_ok(),
            })
            .collect(),
        receipt_sha256: admission.receipt_sha256.clone(),
    }
}

/// Project one request-scoped readiness fact set. Every branch except the exact
/// valid admission with an authoritative `scope_admitted` guard returns
/// `request_scoped_admissible: false`. The function reuses the same domain
/// validators as dispatch and never converts provider configuration into
/// provider-issued evidence.
pub fn project_request_scoped(facts: &RequestFundingFacts<'_>) -> RequestFundingStatus {
    let intake_request_id = facts.intake_request_id.trim();
    let lead_request_id = facts.lead_request_id.trim();
    let complete = !intake_request_id.is_empty() && !lead_request_id.is_empty();
    let admission = facts.admission.map(admission_view);
    let mut provenance = Vec::new();
    if facts.provider_facts.is_some() {
        provenance.push("PROVIDER_SETTINGS");
    }
    if facts.admission.is_some() {
        provenance.push("REQUEST_SCOPED_SUBSCRIPTION_ADMISSION");
        provenance.push("OWNER_EXTRA_USAGE_ASSUMPTION");
        provenance.push("OWNER_ASSUMPTION_NOT_PROVIDER_EVIDENCE");
    }
    if facts.scope_admitted {
        provenance.push("CORE_SCOPE_ADMISSION_GUARD");
    }

    let mut status = RequestFundingStatus {
        schema_version: REQUEST_FUNDING_SCHEMA_VERSION,
        scope: RequestFundingScope {
            intake_request_id: intake_request_id.to_owned(),
            lead_request_id: lead_request_id.to_owned(),
            complete,
        },
        provider: facts.provider.to_owned(),
        model: facts.model.to_owned(),
        request_scoped: complete,
        core_verifiable: false,
        request_scoped_admissible: false,
        verdict: RequestFundingVerdict::ScopeRequired,
        admission,
        missing_evidence: Vec::new(),
        recovery: RECOVERY_SCOPE,
        provenance,
        note: REQUEST_NOTE,
    };

    if !complete {
        status.missing_evidence.push(MISSING_SCOPE);
        return status;
    }
    let Some(admission) = facts.admission else {
        status.verdict = RequestFundingVerdict::AdmissionMissing;
        status.missing_evidence.push(MISSING_ADMISSION);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    };
    if admission.state != "ACTIVE" {
        status.verdict = RequestFundingVerdict::AdmissionInactive;
        status.missing_evidence.push(MISSING_ACTIVE_ADMISSION);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    }
    if admission.expires_at_ms <= facts.now_ms {
        status.verdict = RequestFundingVerdict::AdmissionExpired;
        status.missing_evidence.push(MISSING_ADMISSION_VALIDITY);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    }
    if admission.assumption.extra_usage != OwnerControlledExtraUsage::Disabled {
        status.verdict = RequestFundingVerdict::ExtraUsageNotDisabled;
        status.missing_evidence.push(MISSING_EXTRA_USAGE_DISABLED);
        status.recovery = RECOVERY_OWNER;
        return status;
    }
    if admission.assumption.validate_at(facts.now_ms).is_err() {
        status.verdict = RequestFundingVerdict::OwnerAssumptionInvalid;
        status.missing_evidence.push(MISSING_OWNER_ASSUMPTION);
        status.recovery = RECOVERY_OWNER;
        return status;
    }
    if admission.intake_request_id != intake_request_id
        || admission.lead_request_id != lead_request_id
    {
        status.verdict = RequestFundingVerdict::ScopeMismatch;
        status.missing_evidence.push(MISSING_EXACT_SCOPE);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    }
    if admission.provider != facts.provider || admission.model != facts.model {
        status.verdict = RequestFundingVerdict::RouteMismatch;
        status.missing_evidence.push(MISSING_EXACT_ROUTE);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    }
    let Some(provider_facts) = facts.provider_facts else {
        status.verdict = RequestFundingVerdict::ProviderNotConfigured;
        status.missing_evidence.push(MISSING_PROVIDER_CONNECTION);
        status.recovery = RECOVERY_PROVIDER;
        return status;
    };
    if !provider_facts.configured {
        status.verdict = RequestFundingVerdict::ProviderNotConfigured;
        status.missing_evidence.push(MISSING_PROVIDER_CONNECTION);
        status.recovery = RECOVERY_PROVIDER;
        return status;
    }
    if !provider_facts.enabled {
        status.verdict = RequestFundingVerdict::ProviderDisabled;
        status.missing_evidence.push(MISSING_PROVIDER_ENABLED);
        status.recovery = RECOVERY_PROVIDER;
        return status;
    }
    if admission.settings_revision != provider_facts.settings_revision {
        status.verdict = RequestFundingVerdict::SettingsRevisionMismatch;
        status.missing_evidence.push(MISSING_SETTINGS_REVISION);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    }
    if !provider_facts.canonical_route {
        status.verdict = RequestFundingVerdict::NonCanonicalRoute;
        status.missing_evidence.push(MISSING_CANONICAL_ROUTE);
        status.recovery = RECOVERY_ROUTE;
        return status;
    }
    let Some(route) = test_route(facts.provider, facts.model) else {
        status.verdict = RequestFundingVerdict::RouteNotAdmitted;
        status.missing_evidence.push(MISSING_LEAD_ROUTE);
        status.recovery = RECOVERY_ROUTE;
        return status;
    };
    if !role_allowed(route, TestModelRole::Lead) {
        status.verdict = RequestFundingVerdict::RouteNotAdmitted;
        status.missing_evidence.push(MISSING_LEAD_ROUTE);
        status.recovery = RECOVERY_ROUTE;
        return status;
    }
    let lead_route_admitted = admission.routes.iter().any(|binding| {
        binding.role == TestModelRole::Lead
            && binding.provider == facts.provider
            && binding.model == facts.model
            && binding.settings_revision == provider_facts.settings_revision
            && binding.validate().is_ok()
    });
    if !lead_route_admitted {
        status.verdict = RequestFundingVerdict::RouteNotAdmitted;
        status.missing_evidence.push(MISSING_LEAD_ROUTE);
        status.recovery = RECOVERY_ROUTE;
        return status;
    }
    if !facts.scope_admitted {
        status.verdict = RequestFundingVerdict::BindingRequired;
        status.missing_evidence.push(MISSING_SCOPE_GUARD);
        status.recovery = RECOVERY_ADMISSION;
        return status;
    }

    status.verdict = RequestFundingVerdict::AdmittedForRequest;
    status.request_scoped_admissible = true;
    status.recovery = RECOVERY_ADMITTED;
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::OwnerExtraUsageBinding;
    use crate::execution_policy::OwnerBillingAssumption;
    use crate::subscription_admission::TestRouteBinding;

    fn facts() -> ProviderFundingFacts {
        ProviderFundingFacts {
            kind: "opencode".into(),
            configured: true,
            enabled: true,
            model: "deepseek-v4.1-flash".into(),
            settings_revision: 3,
        }
    }

    #[test]
    fn unknown_provider_is_not_a_subscription_route() {
        assert!(project("opencode", Some(&facts())).is_none());
        assert!(project("fixture", None).is_none());
    }

    #[test]
    fn a_stored_api_key_is_never_reported_as_verified_subscription_funding() {
        let status = project("opencode-go", Some(&facts())).unwrap();
        assert_eq!(status.route, "GO_SUBSCRIPTION_INCLUDED");
        assert_eq!(status.authentication_class, "API_KEY");
        assert_eq!(status.model, "deepseek-v4.1-flash");
        assert_eq!(status.settings_revision, 3);
        assert!(!status.core_verifiable);
        assert!(!status.admissible);
        assert_eq!(status.entitlement, "NOT_RECORDED");
        assert_eq!(
            status.extra_usage,
            "OWNER_ASSUMED_DISABLED_PENDING_ADMISSION"
        );
        assert_eq!(status.balance_fallback, "NOT_RECORDED");
        assert_eq!(status.no_charge_route, "EXACT_SUBSCRIPTION_ROUTE_ONLY");
        assert_eq!(status.quota.state, "UNKNOWN");
        assert_eq!(
            status.missing_evidence,
            [
                "OWNER_CONFIRMED_EXTRA_USAGE_ASSUMPTION",
                "EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION",
            ]
        );
    }

    #[test]
    fn an_unconfigured_route_reports_the_missing_connection() {
        let status = project("opencode-go", None).unwrap();
        assert!(!status.configured);
        assert!(!status.enabled);
        assert_eq!(status.authentication_class, "UNKNOWN");
        assert_eq!(status.settings_revision, 0);
        assert_eq!(
            status.missing_evidence.first(),
            Some(&"PROVIDER_CONNECTION")
        );
        assert!(!status.admissible);
    }

    #[test]
    fn a_settings_row_without_a_credential_is_not_called_api_key_authenticated() {
        let facts = ProviderFundingFacts {
            kind: "opencode".into(),
            configured: false,
            enabled: false,
            model: "deepseek-v4.1-flash".into(),
            settings_revision: 4,
        };
        let status = project("opencode-go", Some(&facts)).unwrap();
        assert!(!status.configured);
        assert_eq!(status.authentication_class, "UNKNOWN");
        assert_eq!(
            status.missing_evidence.first(),
            Some(&"PROVIDER_CONNECTION")
        );
    }

    #[test]
    fn codex_is_a_known_but_blocked_subscription_route() {
        let status = project("codex", None).unwrap();
        assert_eq!(status.route, "CHATGPT_PLAN_INCLUDED");
        assert_eq!(status.kind, "codex");
        assert!(!status.admissible);
    }

    #[test]
    fn unscoped_provider_status_is_not_a_request_or_dispatch_verdict() {
        let status = project("opencode-go", Some(&facts())).unwrap();
        assert_eq!(status.scope, "UNSCOPED_PROVIDER_STATUS");
        assert!(!status.request_scoped);
        assert!(!status.admissible);
        assert!(status.note.contains("UNSCOPED PROVIDER STATUS"));
        assert!(status.note.contains("request-scoped"));
    }

    const NOW_MS: i64 = 1_800_000_000_000;

    fn owner_assumption(extra_usage: OwnerControlledExtraUsage) -> OwnerBillingAssumption {
        OwnerBillingAssumption {
            assumption_id: "assumption-1".into(),
            confirmed_by: "Product Owner".into(),
            confirmed_at_ms: NOW_MS - 1_000,
            valid_until_ms: NOW_MS + 60_000,
            extra_usage,
            statement: "I confirm OpenCode Go Extra Usage is disabled for this exact route.".into(),
        }
    }

    fn route(settings_revision: &str) -> TestRouteBinding {
        TestRouteBinding {
            role: TestModelRole::Lead,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: settings_revision.into(),
            transport: "OPENCODE_GO_SUBSCRIPTION".into(),
        }
    }

    fn declaration() -> OwnerExtraUsageBinding {
        OwnerExtraUsageBinding {
            role: "lead".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: "7".into(),
            base_url: "https://opencode.ai/inference/go/openai/v1".into(),
            credential_fingerprint: "credential-fingerprint".into(),
            declaration_id: "declaration-1".into(),
        }
    }

    fn admission_record() -> ScopeAdmissionRecord {
        ScopeAdmissionRecord {
            admission_id: "admission-1".into(),
            request_fingerprint: "request-fingerprint".into(),
            intake_request_id: "intake-1".into(),
            lead_request_id: "lead-1".into(),
            test_profile_id: "test-profile-1".into(),
            execution_profile_id: "profile-1".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: "7".into(),
            profile_revision_id: "profile-revision-1".into(),
            profile_hash: "b".repeat(64),
            assumption: owner_assumption(OwnerControlledExtraUsage::Disabled),
            routes: vec![route("7")],
            owner_extra_usage_declarations: vec![declaration()],
            state: "ACTIVE".into(),
            campaign_id: None,
            created_at_ms: NOW_MS - 2_000,
            expires_at_ms: NOW_MS + 60_000,
            receipt_sha256: "a".repeat(64),
        }
    }

    fn provider_facts() -> RequestFundingProviderFacts {
        RequestFundingProviderFacts {
            configured: true,
            enabled: true,
            settings_revision: "7".into(),
            canonical_route: true,
        }
    }

    fn request_facts<'a>(
        provider: &'a RequestFundingProviderFacts,
        admission: Option<&'a ScopeAdmissionRecord>,
    ) -> RequestFundingFacts<'a> {
        RequestFundingFacts {
            intake_request_id: "intake-1",
            lead_request_id: "lead-1",
            provider: "opencode-go",
            model: "deepseek-v4.1-flash",
            provider_facts: Some(provider),
            admission,
            scope_admitted: true,
            now_ms: NOW_MS,
        }
    }

    #[test]
    fn exact_request_admission_is_truthful_but_not_provider_verified() {
        let provider = provider_facts();
        let admission = admission_record();
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::AdmittedForRequest);
        assert!(status.request_scoped_admissible);
        assert!(!status.core_verifiable);
        assert!(status.request_scoped);
        assert_eq!(status.scope.intake_request_id, "intake-1");
        assert_eq!(status.scope.lead_request_id, "lead-1");
        assert!(status.missing_evidence.is_empty());
        assert_eq!(
            status
                .admission
                .as_ref()
                .map(|admission| admission.admission_id.as_str()),
            Some("admission-1")
        );
        assert!(
            status
                .provenance
                .contains(&"OWNER_ASSUMPTION_NOT_PROVIDER_EVIDENCE")
        );
        assert!(status.provenance.contains(&"CORE_SCOPE_ADMISSION_GUARD"));
        assert!(status.note.contains("never bypasses it"));
    }

    #[test]
    fn unscoped_request_status_is_never_admissible() {
        let provider = provider_facts();
        let admission = admission_record();
        let mut facts = request_facts(&provider, Some(&admission));
        facts.lead_request_id = "";
        let status = project_request_scoped(&facts);
        assert_eq!(status.verdict, RequestFundingVerdict::ScopeRequired);
        assert!(!status.request_scoped);
        assert!(!status.request_scoped_admissible);
        assert_eq!(status.missing_evidence, vec!["EXACT_REQUEST_SCOPE"]);
    }

    #[test]
    fn missing_admission_reports_the_exact_blocking_evidence() {
        let provider = provider_facts();
        let status = project_request_scoped(&request_facts(&provider, None));
        assert_eq!(status.verdict, RequestFundingVerdict::AdmissionMissing);
        assert!(!status.request_scoped_admissible);
        assert_eq!(
            status.missing_evidence,
            vec!["EXPLICIT_BOUNDED_SUBSCRIPTION_ADMISSION"]
        );
    }

    #[test]
    fn inactive_revoked_or_consumed_admission_is_never_admissible() {
        let provider = provider_facts();
        for state in ["CONSUMED", "REVOKED"] {
            let mut admission = admission_record();
            admission.state = state.into();
            let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
            assert_eq!(status.verdict, RequestFundingVerdict::AdmissionInactive);
            assert!(!status.request_scoped_admissible);
        }
    }

    #[test]
    fn expired_admission_or_owner_assumption_is_never_admissible() {
        let provider = provider_facts();
        let mut admission = admission_record();
        admission.expires_at_ms = NOW_MS;
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::AdmissionExpired);
        assert!(!status.request_scoped_admissible);

        let mut admission = admission_record();
        admission.assumption.valid_until_ms = NOW_MS;
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(
            status.verdict,
            RequestFundingVerdict::OwnerAssumptionInvalid
        );
        assert!(!status.request_scoped_admissible);
    }

    #[test]
    fn extra_usage_unknown_or_enabled_is_never_admissible() {
        let provider = provider_facts();
        let mut admission = admission_record();
        admission.assumption = owner_assumption(OwnerControlledExtraUsage::Unknown);
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::ExtraUsageNotDisabled);
        assert!(!status.request_scoped_admissible);
        assert_eq!(
            status.missing_evidence,
            vec!["OWNER_CONFIRMED_EXTRA_USAGE_DISABLED"]
        );
    }

    #[test]
    fn request_scope_provider_or_model_mismatch_is_never_admissible() {
        let provider = provider_facts();
        let mut admission = admission_record();
        admission.lead_request_id = "another-lead".into();
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::ScopeMismatch);
        assert!(!status.request_scoped_admissible);

        let mut admission = admission_record();
        admission.model = "another-model".into();
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::RouteMismatch);
        assert!(!status.request_scoped_admissible);
    }

    #[test]
    fn settings_revision_or_route_binding_mismatch_is_never_admissible() {
        let provider = provider_facts();
        let mut admission = admission_record();
        admission.settings_revision = "6".into();
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(
            status.verdict,
            RequestFundingVerdict::SettingsRevisionMismatch
        );
        assert!(!status.request_scoped_admissible);

        let mut admission = admission_record();
        admission.routes = vec![TestRouteBinding {
            role: TestModelRole::Implementer,
            ..route("7")
        }];
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::RouteNotAdmitted);
        assert!(!status.request_scoped_admissible);
    }

    #[test]
    fn non_canonical_route_is_never_admissible() {
        let mut provider = provider_facts();
        provider.canonical_route = false;
        let admission = admission_record();
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::NonCanonicalRoute);
        assert!(!status.request_scoped_admissible);
        assert_eq!(
            status.missing_evidence,
            vec!["CANONICAL_SUBSCRIPTION_ROUTE"]
        );
    }

    #[test]
    fn missing_or_disabled_provider_connection_is_never_admissible() {
        let admission = admission_record();
        let status = project_request_scoped(&RequestFundingFacts {
            intake_request_id: "intake-1",
            lead_request_id: "lead-1",
            provider: "opencode-go",
            model: "deepseek-v4.1-flash",
            provider_facts: None,
            admission: Some(&admission),
            scope_admitted: true,
            now_ms: NOW_MS,
        });
        assert_eq!(status.verdict, RequestFundingVerdict::ProviderNotConfigured);
        assert!(!status.request_scoped_admissible);

        let mut provider = provider_facts();
        provider.enabled = false;
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        assert_eq!(status.verdict, RequestFundingVerdict::ProviderDisabled);
        assert!(!status.request_scoped_admissible);
    }

    #[test]
    fn authoritative_guard_must_also_pass() {
        let provider = provider_facts();
        let admission = admission_record();
        let mut facts = request_facts(&provider, Some(&admission));
        facts.scope_admitted = false;
        let status = project_request_scoped(&facts);
        assert_eq!(status.verdict, RequestFundingVerdict::BindingRequired);
        assert!(!status.request_scoped_admissible);
        assert_eq!(status.missing_evidence, vec!["CORE_SCOPE_ADMISSION_GUARD"]);
    }

    #[test]
    fn request_projection_serializes_receipt_hash_but_no_credentials_or_base_url() {
        let provider = provider_facts();
        let admission = admission_record();
        let status = project_request_scoped(&request_facts(&provider, Some(&admission)));
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["requestScoped"], true);
        assert_eq!(value["coreVerifiable"], false);
        assert_eq!(value["requestScopedAdmissible"], true);
        assert_eq!(value["scope"]["intakeRequestId"], "intake-1");
        assert_eq!(value["admission"]["extraUsage"], "DISABLED");
        assert_eq!(value["admission"]["receiptSha256"], "a".repeat(64));
        assert_eq!(value["verdict"], "ADMITTED_FOR_REQUEST");
        let serialized = value.to_string();
        assert!(!serialized.contains("base_url"));
        assert!(!serialized.contains("baseUrl"));
        assert!(!serialized.contains("credential_fingerprint"));
        assert!(!serialized.contains("credentialFingerprint"));
        assert!(!serialized.contains("requestFingerprint"));
        assert!(!serialized.contains("profileHash"));
    }
}
