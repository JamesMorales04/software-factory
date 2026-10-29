//! Versioned Execution Profile domain.
//!
//! A profile is operator configuration for the model-backed roles in the
//! Factory. It is not an authorization, budget grant or tool capability: Core
//! still freezes an exact revision into a Campaign authorization and remains
//! the sole authority for risk, delivery and Quality Gate decisions.
//!
//! Decision mechanisms: route support, field bounds, revision identity and
//! executability are `DETERMINISTIC`. No model can create, validate or widen a
//! profile. Executability is derived from Core-owned provider settings, the
//! implemented provider adapter and the models Core can substantiate for that
//! connection (configured model, tested registry routes and real discovery).
//! A profile whose declared roles are not all routable stays a non-executable
//! draft instead of being silently coerced to another model.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::lead::RiskClass;
use crate::provider_adapter::ProviderAdapter;
use crate::provider_catalog;

pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_DESCRIPTION_CHARS: usize = 500;
pub const MAX_ROLE_INPUT_TOKENS: u64 = 1_000_000;
pub const MAX_ROLE_OUTPUT_TOKENS: u64 = 100_000;
pub const MAX_ROLE_COST_MICROS: u64 = 100_000_000;
pub const MIN_CAMPAIGN_ATTEMPTS: u32 = 1;
pub const MAX_CAMPAIGN_ATTEMPTS: u32 = 10;
pub const MIN_CAMPAIGN_SESSIONS: u32 = 1;
pub const MAX_CAMPAIGN_SESSIONS: u32 = 16;
pub const MIN_CAMPAIGN_TURNS: u32 = 1;
pub const MAX_CAMPAIGN_TURNS: u32 = 16;
pub const MAX_CAMPAIGN_INPUT_TOKENS: u64 = 1_000_000;
pub const MAX_CAMPAIGN_OUTPUT_TOKENS: u64 = 100_000;
pub const MAX_CAMPAIGN_COST_MICROS: u64 = 100_000_000;
pub const MIN_CAMPAIGN_DEADLINE_SECS: u64 = 30;
pub const MAX_CAMPAIGN_DEADLINE_SECS: u64 = 86_400;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExecutionProfileError {
    #[error("invalid execution profile: {0}")]
    Invalid(String),
    #[error("unsupported execution profile route: {0}")]
    UnsupportedRoute(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionProfileCategory {
    Budget,
    Balanced,
    Quality,
    Custom,
}

impl ExecutionProfileCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Budget => "BUDGET",
            Self::Balanced => "BALANCED",
            Self::Quality => "QUALITY",
            Self::Custom => "CUSTOM",
        }
    }
}

/// The three model-backed roles that exist as real calls today. The
/// deterministic Verifier is not a profile role. A future assessor split must
/// add a role only when it has a separate, integrated model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionRole {
    Lead,
    Implementer,
    SemanticReviewer,
}

impl ExecutionRole {
    pub const ALL: [Self; 3] = [Self::Lead, Self::Implementer, Self::SemanticReviewer];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Implementer => "implementer",
            Self::SemanticReviewer => "semanticReviewer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedExecutionRoute {
    pub provider: &'static str,
    pub model: &'static str,
}

/// Baseline tested-route registry. `opencode` and `opencode-go` are distinct
/// provider connections. These routes are always substantiated for their
/// provider; additional model IDs become routable only through Core-owned
/// provider model discovery. Extend the registry only with a tested adapter.
pub const SUPPORTED_ROUTES: &[SupportedExecutionRoute] = &[
    SupportedExecutionRoute {
        provider: "opencode",
        model: "deepseek-v4.1-flash",
    },
    SupportedExecutionRoute {
        provider: "opencode-go",
        model: "deepseek-v4.1-flash",
    },
    SupportedExecutionRoute {
        provider: "codex",
        model: "gpt-6-luna",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionRoleRoute {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningEffort>,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionProfileRoles {
    pub lead: ExecutionRoleRoute,
    pub implementer: ExecutionRoleRoute,
    pub semantic_reviewer: ExecutionRoleRoute,
}

impl ExecutionProfileRoles {
    pub fn entries(&self) -> [(ExecutionRole, &ExecutionRoleRoute); 3] {
        [
            (ExecutionRole::Lead, &self.lead),
            (ExecutionRole::Implementer, &self.implementer),
            (ExecutionRole::SemanticReviewer, &self.semantic_reviewer),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CampaignBudgets {
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
pub struct AutoStartPolicy {
    pub enabled: bool,
    #[serde(default)]
    pub risk_classes: Vec<RiskClass>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionProfileContent {
    pub name: String,
    pub description: String,
    pub category: ExecutionProfileCategory,
    pub roles: ExecutionProfileRoles,
    pub budgets: CampaignBudgets,
    pub auto_start: AutoStartPolicy,
}

impl ExecutionProfileContent {
    /// Validate every bounded value and reject unsupported provider/model
    /// routes. An unconfigured provider is not an unsupported route: it makes
    /// the profile a clearly marked non-executable draft.
    pub fn validate(&self) -> Result<(), ExecutionProfileError> {
        validate_text("name", &self.name, MAX_NAME_CHARS, true)?;
        validate_text(
            "description",
            &self.description,
            MAX_DESCRIPTION_CHARS,
            false,
        )?;
        for (role, route) in self.roles.entries() {
            validate_route(role, route)?;
        }
        validate_budgets(&self.budgets)?;
        validate_auto_start(&self.auto_start)?;
        Ok(())
    }

    /// SHA-256 over the canonical content JSON. A new revision keeps its own
    /// identity even when the content is unchanged; the hash identifies the
    /// exact content, not the row.
    pub fn compute_content_hash(&self) -> String {
        let mut canonical = self.clone();
        canonical.auto_start.risk_classes.sort();
        let raw =
            serde_json::to_vec(&canonical).expect("execution profile content is serializable");
        sha256_hex(&raw)
    }

    pub fn validate_and_hash(&self) -> Result<String, ExecutionProfileError> {
        self.validate()?;
        Ok(self.compute_content_hash())
    }
}

/// Core-owned provider facts used to derive executability. The DB layer reads
/// these from `provider_settings` and the process-local discovery cache; the
/// domain only interprets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRouteAvailability {
    pub provider: String,
    pub kind: String,
    /// The model selected for worker provider sessions and the legacy default
    /// gateway. It is one substantiated model, not the only one.
    pub model: String,
    /// Every model ID Core can currently substantiate for this connection:
    /// the configured model, baseline tested registry routes and real discovery
    /// results. A model outside this set is not routable.
    pub models: Vec<String>,
    pub enabled: bool,
    pub has_credential: bool,
    /// `provider_settings.is_default = 1`.
    pub is_default: bool,
}

/// Merge the configured model, tested registry routes and discovered models for
/// one provider connection, preserving first-seen order.
pub fn substantiated_models(
    provider: &str,
    configured_model: &str,
    cache: &provider_catalog::ProviderModelCache,
) -> Vec<String> {
    let mut models = Vec::new();
    let mut push = |model: &str| {
        if !model.is_empty() && !models.iter().any(|seen: &String| seen == model) {
            models.push(model.to_owned());
        }
    };
    push(configured_model);
    for route in SUPPORTED_ROUTES
        .iter()
        .filter(|route| route.provider == provider)
    {
        push(route.model);
    }
    for model in cache.models(provider) {
        push(model);
    }
    models
}

/// Derive executability and truthful warnings. Every false state is explained.
///
/// A role is routable only when its provider connection exists, is enabled,
/// has a stored credential, the adapter implements the role transport, and the
/// model is substantiated for that connection. Lead assessment and semantic
/// review need the OpenAI-compatible chat adapter; implementation needs the
/// Worker provider-session adapter. A Codex App Server connection is executable
/// for implementation but not for one-shot assessment or review in this build.
pub fn evaluate_executability(
    content: &ExecutionProfileContent,
    availability: &[ProviderRouteAvailability],
) -> (bool, Vec<String>) {
    let mut warnings = Vec::new();
    for (role, route) in content.roles.entries() {
        let Some(provider) = availability
            .iter()
            .find(|entry| entry.provider == route.provider)
        else {
            warnings.push(format!(
                "{} route {}/{} is declared but {} is not configured",
                role.as_str(),
                route.provider,
                route.model,
                route.provider
            ));
            continue;
        };
        if !provider.has_credential {
            warnings.push(format!(
                "{} route {}/{} cannot run because {} has no stored credential",
                role.as_str(),
                route.provider,
                route.model,
                route.provider
            ));
        }
        if !provider.enabled {
            warnings.push(format!(
                "{} route {}/{} cannot run because {} is disabled",
                role.as_str(),
                route.provider,
                route.model,
                route.provider
            ));
        }
        match ProviderAdapter::from_kind(&provider.kind) {
            None => warnings.push(format!(
                "{} route {}/{} cannot run because provider kind {} has no implemented adapter",
                role.as_str(),
                route.provider,
                route.model,
                provider.kind
            )),
            Some(adapter) if role == ExecutionRole::Implementer => {
                if !adapter.supports_provider_session() {
                    warnings.push(format!(
                        "implementer route {}/{} cannot run because the {} adapter has no provider session",
                        route.provider,
                        route.model,
                        provider.kind
                    ));
                }
            }
            Some(adapter) => {
                if !adapter.supports_chat_completions() {
                    warnings.push(format!(
                        "{} route {}/{} cannot execute a one-shot model call in this build: the {} adapter is a Worker App Server, not an OpenAI-compatible model gateway",
                        role.as_str(),
                        route.provider,
                        route.model,
                        provider.kind
                    ));
                }
            }
        }
        if !provider.models.iter().any(|model| model == &route.model) {
            warnings.push(format!(
                "{} route {}/{} model is not advertised by {}; run provider model discovery or select a supported model",
                role.as_str(),
                route.provider,
                route.model,
                route.provider
            ));
        }
    }
    (warnings.is_empty(), warnings)
}

fn validate_text(
    field: &str,
    value: &str,
    max_chars: usize,
    required: bool,
) -> Result<(), ExecutionProfileError> {
    let chars = value.chars().count();
    if required && chars == 0 {
        return Err(ExecutionProfileError::Invalid(format!(
            "{field} must not be empty"
        )));
    }
    if chars > max_chars {
        return Err(ExecutionProfileError::Invalid(format!(
            "{field} must be at most {max_chars} characters"
        )));
    }
    if value.trim() != value {
        return Err(ExecutionProfileError::Invalid(format!(
            "{field} must not have surrounding whitespace"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(ExecutionProfileError::Invalid(format!(
            "{field} must not contain control characters"
        )));
    }
    Ok(())
}

fn validate_route(
    role: ExecutionRole,
    route: &ExecutionRoleRoute,
) -> Result<(), ExecutionProfileError> {
    if provider_catalog::entry(&route.provider).is_none() {
        return Err(ExecutionProfileError::UnsupportedRoute(format!(
            "{} route provider {:?} is not a configured Core provider connection",
            role.as_str(),
            route.provider
        )));
    }
    provider_catalog::validate_model(&route.model).map_err(|message| {
        ExecutionProfileError::UnsupportedRoute(format!(
            "{} route {}/{} model is invalid: {message}",
            role.as_str(),
            route.provider,
            route.model
        ))
    })?;
    if !(1..=MAX_ROLE_INPUT_TOKENS).contains(&route.max_input_tokens) {
        return Err(ExecutionProfileError::Invalid(format!(
            "{}.maxInputTokens must be between 1 and {MAX_ROLE_INPUT_TOKENS}",
            role.as_str()
        )));
    }
    if !(1..=MAX_ROLE_OUTPUT_TOKENS).contains(&route.max_output_tokens) {
        return Err(ExecutionProfileError::Invalid(format!(
            "{}.maxOutputTokens must be between 1 and {MAX_ROLE_OUTPUT_TOKENS}",
            role.as_str()
        )));
    }
    if !(1..=MAX_ROLE_COST_MICROS).contains(&route.max_cost_micros) {
        return Err(ExecutionProfileError::Invalid(format!(
            "{}.maxCostMicros must be between 1 and {MAX_ROLE_COST_MICROS}",
            role.as_str()
        )));
    }
    Ok(())
}

fn validate_budgets(budgets: &CampaignBudgets) -> Result<(), ExecutionProfileError> {
    if !(MIN_CAMPAIGN_ATTEMPTS..=MAX_CAMPAIGN_ATTEMPTS).contains(&budgets.max_attempts) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.maxAttempts must be between {MIN_CAMPAIGN_ATTEMPTS} and {MAX_CAMPAIGN_ATTEMPTS}"
        )));
    }
    if !(MIN_CAMPAIGN_SESSIONS..=MAX_CAMPAIGN_SESSIONS).contains(&budgets.max_sessions) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.maxSessions must be between {MIN_CAMPAIGN_SESSIONS} and {MAX_CAMPAIGN_SESSIONS}"
        )));
    }
    if !(MIN_CAMPAIGN_TURNS..=MAX_CAMPAIGN_TURNS).contains(&budgets.max_turns) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.maxTurns must be between {MIN_CAMPAIGN_TURNS} and {MAX_CAMPAIGN_TURNS}"
        )));
    }
    if !(1..=MAX_CAMPAIGN_INPUT_TOKENS).contains(&budgets.max_input_tokens) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.maxInputTokens must be between 1 and {MAX_CAMPAIGN_INPUT_TOKENS}"
        )));
    }
    if !(1..=MAX_CAMPAIGN_OUTPUT_TOKENS).contains(&budgets.max_output_tokens) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.maxOutputTokens must be between 1 and {MAX_CAMPAIGN_OUTPUT_TOKENS}"
        )));
    }
    if !(1..=MAX_CAMPAIGN_COST_MICROS).contains(&budgets.max_cost_micros) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.maxCostMicros must be between 1 and {MAX_CAMPAIGN_COST_MICROS}"
        )));
    }
    if !(MIN_CAMPAIGN_DEADLINE_SECS..=MAX_CAMPAIGN_DEADLINE_SECS).contains(&budgets.deadline_secs) {
        return Err(ExecutionProfileError::Invalid(format!(
            "budgets.deadlineSecs must be between {MIN_CAMPAIGN_DEADLINE_SECS} and {MAX_CAMPAIGN_DEADLINE_SECS}"
        )));
    }
    Ok(())
}

fn validate_auto_start(policy: &AutoStartPolicy) -> Result<(), ExecutionProfileError> {
    let mut seen = BTreeSet::new();
    if policy.risk_classes.iter().any(|risk| !seen.insert(*risk)) {
        return Err(ExecutionProfileError::Invalid(
            "autoStart.riskClasses must not contain duplicates".into(),
        ));
    }
    if policy.enabled && policy.risk_classes.is_empty() {
        return Err(ExecutionProfileError::Invalid(
            "autoStart.riskClasses must be explicit when auto start is enabled".into(),
        ));
    }
    if !policy.enabled && !policy.risk_classes.is_empty() {
        return Err(ExecutionProfileError::Invalid(
            "autoStart.riskClasses require auto start to be enabled".into(),
        ));
    }
    Ok(())
}

fn sha256_hex(raw: &[u8]) -> String {
    let digest = Sha256::digest(raw);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(provider: &str, model: &str) -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: provider.into(),
            model: model.into(),
            reasoning: Some(ReasoningEffort::Medium),
            max_input_tokens: 128_000,
            max_output_tokens: 8_192,
            max_cost_micros: 300_000,
        }
    }

    fn valid_content() -> ExecutionProfileContent {
        ExecutionProfileContent {
            name: "Balanced".into(),
            description: "Default supported routes".into(),
            category: ExecutionProfileCategory::Balanced,
            roles: ExecutionProfileRoles {
                lead: route("opencode-go", "deepseek-v4.1-flash"),
                implementer: route("opencode-go", "deepseek-v4.1-flash"),
                semantic_reviewer: route("codex", "gpt-6-luna"),
            },
            budgets: CampaignBudgets {
                max_attempts: 3,
                max_sessions: 2,
                max_turns: 4,
                max_input_tokens: 40_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                deadline_secs: 600,
            },
            auto_start: AutoStartPolicy {
                enabled: false,
                risk_classes: Vec::new(),
            },
        }
    }

    #[test]
    fn only_model_backed_roles_exist() {
        assert_eq!(
            ExecutionRole::ALL.map(ExecutionRole::as_str),
            ["lead", "implementer", "semanticReviewer"]
        );
    }

    #[test]
    fn content_hash_is_canonical_and_content_bound() {
        let content = valid_content();
        let hash = content.validate_and_hash().unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, content.compute_content_hash());

        let mut reordered = content.clone();
        reordered.auto_start = AutoStartPolicy {
            enabled: true,
            risk_classes: vec![RiskClass::R1, RiskClass::R0],
        };
        let mut sorted = reordered.clone();
        sorted.auto_start.risk_classes = vec![RiskClass::R0, RiskClass::R1];
        assert_eq!(
            reordered.compute_content_hash(),
            sorted.compute_content_hash()
        );

        let mut changed = content;
        changed.name = "Quality".into();
        assert_ne!(hash, changed.compute_content_hash());
    }

    #[test]
    fn unknown_provider_is_rejected_but_an_unadvertised_model_stays_a_draft() {
        let mut content = valid_content();
        content.roles.lead = route("unknown-provider", "deepseek-v4.1-flash");
        assert!(matches!(
            content.validate(),
            Err(ExecutionProfileError::UnsupportedRoute(_))
        ));

        let mut content = valid_content();
        content.roles.lead = route("opencode-go", "not advertised");
        assert!(content.validate().is_err());

        let mut content = valid_content();
        content.roles.semantic_reviewer = route("codex", "gpt-6-luna-typo");
        content.validate().unwrap();
        let (executable, warnings) = evaluate_executability(
            &content,
            &[
                availability(
                    "opencode-go",
                    "opencode",
                    "deepseek-v4.1-flash",
                    &["deepseek-v4.1-flash"],
                ),
                availability("codex", "codex", "gpt-6-luna", &["gpt-6-luna"]),
            ],
        );
        assert!(!executable);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("is not advertised by codex")),
            "{warnings:?}"
        );
    }

    #[test]
    fn bounds_and_auto_start_policy_are_strict() {
        let mut content = valid_content();
        content.budgets.max_attempts = 0;
        assert!(content.validate().is_err());

        let mut content = valid_content();
        content.roles.implementer.max_output_tokens = MAX_ROLE_OUTPUT_TOKENS + 1;
        assert!(content.validate().is_err());

        let mut content = valid_content();
        content.auto_start = AutoStartPolicy {
            enabled: true,
            risk_classes: Vec::new(),
        };
        assert!(content.validate().is_err());

        let mut content = valid_content();
        content.auto_start = AutoStartPolicy {
            enabled: false,
            risk_classes: vec![RiskClass::R0],
        };
        assert!(content.validate().is_err());
    }

    #[test]
    fn distinct_role_routes_are_executable_with_their_own_adapters() {
        let mut content = valid_content();
        content.roles.semantic_reviewer = route("opencode", "deepseek-v4.1-flash");
        let (executable, warnings) = evaluate_executability(
            &content,
            &[
                availability(
                    "opencode-go",
                    "opencode",
                    "deepseek-v4.1-flash",
                    &["deepseek-v4.1-flash"],
                ),
                availability(
                    "opencode",
                    "opencode",
                    "deepseek-v4.1-flash",
                    &["deepseek-v4.1-flash"],
                ),
            ],
        );
        assert!(executable, "{warnings:?}");
        assert!(warnings.is_empty());
    }

    #[test]
    fn one_provider_can_route_different_discovered_models_per_role() {
        let mut content = valid_content();
        content.roles.semantic_reviewer = route("opencode-go", "reviewer-model");
        let (executable, warnings) = evaluate_executability(
            &content,
            &[availability(
                "opencode-go",
                "opencode",
                "deepseek-v4.1-flash",
                &["deepseek-v4.1-flash", "reviewer-model"],
            )],
        );
        assert!(executable, "{warnings:?}");
        assert!(warnings.is_empty());
    }

    #[test]
    fn codex_is_not_a_one_shot_gateway_but_can_implement() {
        let mut content = valid_content();
        content.roles.lead = route("codex", "gpt-6-luna");
        let (executable, warnings) = evaluate_executability(
            &content,
            &[
                availability(
                    "opencode-go",
                    "opencode",
                    "deepseek-v4.1-flash",
                    &["deepseek-v4.1-flash"],
                ),
                availability("codex", "codex", "gpt-6-luna", &["gpt-6-luna"]),
            ],
        );
        assert!(!executable);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("not an OpenAI-compatible model gateway")),
            "{warnings:?}"
        );

        let mut permitted = valid_content();
        permitted.roles.semantic_reviewer = route("opencode-go", "deepseek-v4.1-flash");
        permitted.roles.implementer = route("codex", "gpt-6-luna");
        let (executable, warnings) = evaluate_executability(
            &permitted,
            &[
                availability(
                    "opencode-go",
                    "opencode",
                    "deepseek-v4.1-flash",
                    &["deepseek-v4.1-flash"],
                ),
                availability("codex", "codex", "gpt-6-luna", &["gpt-6-luna"]),
            ],
        );
        assert!(executable, "{warnings:?}");
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_disabled_or_credentialless_providers_are_not_executable() {
        let mut content = valid_content();
        content.roles.semantic_reviewer = route("opencode-go", "deepseek-v4.1-flash");

        let (executable, warnings) = evaluate_executability(
            &content,
            &[availability(
                "opencode-go",
                "opencode",
                "deepseek-v4.1-flash",
                &["deepseek-v4.1-flash"],
            )],
        );
        assert!(executable, "{warnings:?}");

        let (executable, warnings) = evaluate_executability(&content, &[]);
        assert!(!executable);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(
            warnings
                .iter()
                .all(|warning| warning.contains("is not configured"))
        );

        let mut provider = availability(
            "opencode-go",
            "opencode",
            "deepseek-v4.1-flash",
            &["deepseek-v4.1-flash"],
        );
        provider.enabled = false;
        provider.has_credential = false;
        let (executable, warnings) = evaluate_executability(&content, &[provider]);
        assert!(!executable);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("is disabled"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("has no stored credential"))
        );
    }

    #[test]
    fn substantiated_models_merge_the_registry_with_discovery() {
        let mut cache = provider_catalog::ProviderModelCache::default();
        cache.record(provider_catalog::ProviderModelCatalogEntry {
            provider: "opencode-go".into(),
            status: provider_catalog::ProviderDiscoveryStatus::Available,
            source: provider_catalog::ProviderCatalogSource::OpenCodeGoCatalogUrl,
            catalog_url: Some("https://opencode.ai/inference/go/v1/models".into()),
            models: vec!["discovered-model".into(), "deepseek-v4.1-flash".into()],
            detail: "ok".into(),
            discovered_at: "2026-10-28T00:00:00Z".into(),
            credential_signal: provider_catalog::ProviderCredentialSignal::NotObserved,
            last_known: None,
        });
        let models = substantiated_models("opencode-go", "configured-model", &cache);
        assert_eq!(
            models,
            [
                "configured-model",
                "deepseek-v4.1-flash",
                "discovered-model"
            ]
        );
        assert_eq!(
            substantiated_models("opencode", "deepseek-v4.1-flash", &cache),
            ["deepseek-v4.1-flash"]
        );
    }

    fn availability(
        provider: &str,
        kind: &str,
        model: &str,
        models: &[&str],
    ) -> ProviderRouteAvailability {
        ProviderRouteAvailability {
            provider: provider.into(),
            kind: kind.into(),
            model: model.into(),
            models: models.iter().map(|model| (*model).to_owned()).collect(),
            enabled: true,
            has_credential: true,
            is_default: false,
        }
    }
}
