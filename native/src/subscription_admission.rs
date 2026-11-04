//! Automated real-model acceptance policy and Owner-confirmed subscription
//! admission.
//!
//! This module is deliberately separate from the general provider catalog and
//! execution-profile model selection. The product keeps configurable provider
//! and model selection; only the current automated acceptance suite is limited
//! to the exact routes below.
//!
//! OpenCode Go authenticates with an API key. The API key is an authentication
//! method, not a billing classification: the exact `opencode-go` route is a
//! subscription route. Extra Usage is an Owner-controlled upstream Console
//! setting, recorded as an assumption and never as provider-issued evidence.
//!
//! `codex/gpt-6-luna` remains the Owner-approved Codex pair for the future
//! native App Server transport. It is deliberately not admitted here: the
//! Codex Worker path uses independently configured egress credentials, and
//! Core cannot yet verify that they represent ChatGPT plan authentication
//! rather than an API-key route. Admitting it before that proof exists would
//! turn an API-key call into subscription-funded dispatch.
//!
//! Decision mechanism: `DETERMINISTIC`. No model output grants admission.

use serde::{Deserialize, Serialize};

pub const WAVE3_TEST_PROFILE_ID: &str = "wave3-real-model-2026-11";
pub const MAX_STATEMENT_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TestModelRole {
    Lead,
    Implementer,
    SemanticReviewer,
}

impl TestModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Implementer => "implementer",
            Self::SemanticReviewer => "semanticReviewer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestModelRoute {
    pub provider: &'static str,
    pub model: &'static str,
    pub roles: &'static [TestModelRole],
    pub transport: &'static str,
}

/// The only provider/model pairs admitted by the current automated real-model
/// acceptance suite. This is a test policy, not a permanent product catalog.
pub const TEST_MODEL_ROUTES: &[TestModelRoute] = &[TestModelRoute {
    provider: "opencode-go",
    model: "deepseek-v4.1-flash",
    roles: &[
        TestModelRole::Lead,
        TestModelRole::Implementer,
        TestModelRole::SemanticReviewer,
    ],
    transport: "OPENCODE_GO_SUBSCRIPTION",
}];

pub fn test_route(provider: &str, model: &str) -> Option<&'static TestModelRoute> {
    TEST_MODEL_ROUTES
        .iter()
        .find(|route| route.provider == provider && route.model == model)
}

pub fn role_allowed(route: &TestModelRoute, role: TestModelRole) -> bool {
    route.roles.contains(&role)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestRouteBinding {
    pub role: TestModelRole,
    pub provider: String,
    pub model: String,
    pub settings_revision: String,
    pub transport: String,
}

impl TestRouteBinding {
    pub fn validate(&self) -> Result<(), &'static str> {
        let route = test_route(&self.provider, &self.model)
            .ok_or("provider/model pair is outside the automated acceptance allowlist")?;
        if self.transport != route.transport {
            return Err("test route transport differs from the approved adapter");
        }
        if !role_allowed(route, self.role) {
            return Err("test route does not support this role with the current adapter");
        }
        if self.settings_revision.trim().is_empty() || self.settings_revision.len() > 128 {
            return Err("test route settings revision is invalid");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_contract_admits_only_the_opencode_go_subscription_pair() {
        assert!(test_route("opencode-go", "deepseek-v4.1-flash").is_some());
        assert!(test_route("opencode-go", "gpt-6-luna").is_none());
        assert!(test_route("opencode", "deepseek-v4.1-flash").is_none());
        assert!(test_route("codex", "gpt-6-luna").is_none());
        assert!(test_route("codex", "gpt-5").is_none());
    }

    #[test]
    fn codex_is_not_admitted_until_credential_class_is_verifiable() {
        let codex = TestRouteBinding {
            role: TestModelRole::Implementer,
            provider: "codex".into(),
            model: "gpt-6-luna".into(),
            settings_revision: "7".into(),
            transport: "CODEX_APP_SERVER".into(),
        };
        assert_eq!(
            codex.validate(),
            Err("provider/model pair is outside the automated acceptance allowlist")
        );
        assert!(
            TEST_MODEL_ROUTES
                .iter()
                .all(|route| route.transport != "CODEX_APP_SERVER")
        );
    }

    #[test]
    fn route_bindings_reject_unapproved_transport() {
        let valid = TestRouteBinding {
            role: TestModelRole::Implementer,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: "7".into(),
            transport: "OPENCODE_GO_SUBSCRIPTION".into(),
        };
        assert_eq!(valid.validate(), Ok(()));
        let mut wrong_transport = valid;
        wrong_transport.transport = "OPENAI_CHAT_COMPLETIONS".into();
        assert!(wrong_transport.validate().is_err());
    }
}
