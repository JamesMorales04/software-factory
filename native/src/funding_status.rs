//! Read-only subscription funding readiness.
//!
//! Core can describe the provider connection it has stored, but it cannot
//! verify a provider-issued subscription entitlement, a no-overage guarantee
//! or a provider quota observation. This projection is therefore always
//! `core_verifiable: false` and `admissible: false`; it exists so operators and
//! the bounded real-provider harness can see the exact missing evidence instead
//! of inferring it from a missing route.
//!
//! Decision mechanism: `DETERMINISTIC`. Nothing here calls a provider, trusts
//! a model response or turns an Owner Console observation into verified
//! evidence.

use serde::Serialize;

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
    /// Always false: dispatch stays fail-closed.
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

const NOTE: &str = "Core does not require a provider-issued no-overage certificate. OpenCode Go uses API-key authentication on the subscription endpoint; the Owner confirms Extra Usage is disabled in the provider Console. Dispatch remains blocked until an authenticated operator records the bounded test admission and exact route binding.";

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
