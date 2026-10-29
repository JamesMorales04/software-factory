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

use crate::execution_policy::{OperationLimits, ProgressPolicy};
use crate::lead::RiskClass;
use crate::provider_adapter::ProviderAdapter;
use crate::provider_catalog;

pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_DESCRIPTION_CHARS: usize = 500;

/// Technical operation bounds accepted in a V2 profile. These are per-operation
/// and capacity limits, never cumulative Campaign productivity budgets.
pub const MAX_MODEL_REQUEST_TIMEOUT_SECS: u32 = 3_600;
pub const MAX_TOOL_CALL_TIMEOUT_SECS: u32 = 3_600;
pub const MAX_OPERATION_BYTES: u32 = 64 * 1024 * 1024;
pub const MAX_GENERATION_TOKENS: u32 = 1_000_000;
pub const MAX_IN_FLIGHT_OPERATIONS: u32 = 32;
pub const MAX_DIAGNOSE_AFTER_CHECKPOINTS: u32 = 100;
pub const MAX_ESCALATE_AFTER_REPLANS: u32 = 100;
pub const MAX_OBSERVATION_TIMEOUT_SECS: u32 = 3_600;

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

/// Route and reasoning selection for one model-backed role. Request bounds are
/// technical operation limits on the profile, not replenishing role allowances.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionRoleRoute {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningEffort>,
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

/// Legacy V1 route evidence. Cumulative token and cost fields are preserved for
/// historical decode only; they are not execution authority and are never
/// enforced by the V2 path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LegacyExecutionRoleRouteV1 {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningEffort>,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_micros: u64,
}

/// Legacy V1 Campaign budget evidence. Retained byte-compatibly so old
/// revision hashes verify; never decoded as V2 and never enforced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LegacyCampaignBudgetsV1 {
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
pub struct LegacyExecutionProfileRolesV1 {
    pub lead: LegacyExecutionRoleRouteV1,
    pub implementer: LegacyExecutionRoleRouteV1,
    pub semantic_reviewer: LegacyExecutionRoleRouteV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LegacyExecutionProfileContentV1 {
    pub name: String,
    pub description: String,
    pub category: ExecutionProfileCategory,
    pub roles: LegacyExecutionProfileRolesV1,
    pub budgets: LegacyCampaignBudgetsV1,
    pub auto_start: AutoStartPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutoStartPolicy {
    pub enabled: bool,
    #[serde(default)]
    pub risk_classes: Vec<RiskClass>,
}

/// Execution Policy V2 profile content: routes/reasoning plus technical
/// operation limits and deterministic progress supervision. It deliberately
/// carries no turns, attempts, sessions, repair cycles, cumulative role tokens,
/// per-role money or Campaign wall-clock deadline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecutionProfileContent {
    pub name: String,
    pub description: String,
    pub category: ExecutionProfileCategory,
    pub roles: ExecutionProfileRoles,
    pub operations: OperationLimits,
    pub progress: ProgressPolicy,
    pub auto_start: AutoStartPolicy,
}

/// Versioned profile revision content. Legacy V1 revisions are decoded with
/// their original type and hash; they are never reinterpreted using V2
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "schemaVersion")]
pub enum ExecutionProfileRevisionContent {
    #[serde(rename = "1")]
    V1(LegacyExecutionProfileContentV1),
    #[serde(rename = "2")]
    V2(ExecutionProfileContent),
}

impl ExecutionProfileRevisionContent {
    pub fn schema_version(&self) -> u32 {
        match self {
            Self::V1(_) => 1,
            Self::V2(_) => 2,
        }
    }

    pub fn v2(&self) -> Option<&ExecutionProfileContent> {
        match self {
            Self::V2(content) => Some(content),
            Self::V1(_) => None,
        }
    }

    pub fn content_hash(&self) -> String {
        match self {
            Self::V1(content) => content.compute_content_hash(),
            Self::V2(content) => content.compute_content_hash(),
        }
    }

    pub fn validate(&self) -> Result<(), ExecutionProfileError> {
        match self {
            Self::V1(content) => content.validate(),
            Self::V2(content) => content.validate(),
        }
    }
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
        validate_operation_limits(&self.operations)?;
        validate_progress_policy(&self.progress)?;
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
    Ok(())
}

fn validate_operation_limits(limits: &OperationLimits) -> Result<(), ExecutionProfileError> {
    let checks = [
        (
            "operations.modelRequestTimeoutSecs",
            limits.model_request_timeout_secs.get(),
            MAX_MODEL_REQUEST_TIMEOUT_SECS,
        ),
        (
            "operations.toolCallTimeoutSecs",
            limits.tool_call_timeout_secs.get(),
            MAX_TOOL_CALL_TIMEOUT_SECS,
        ),
        (
            "operations.maxRequestBytes",
            limits.max_request_bytes.get(),
            MAX_OPERATION_BYTES,
        ),
        (
            "operations.maxResponseBytes",
            limits.max_response_bytes.get(),
            MAX_OPERATION_BYTES,
        ),
        (
            "operations.maxGenerationTokens",
            limits.max_generation_tokens.get(),
            MAX_GENERATION_TOKENS,
        ),
        (
            "operations.maxInFlightOperations",
            limits.max_in_flight_operations.get(),
            MAX_IN_FLIGHT_OPERATIONS,
        ),
    ];
    for (field, value, maximum) in checks {
        if value == 0 || value > maximum {
            return Err(ExecutionProfileError::Invalid(format!(
                "{field} must be between 1 and {maximum}"
            )));
        }
    }
    Ok(())
}

fn validate_progress_policy(policy: &ProgressPolicy) -> Result<(), ExecutionProfileError> {
    if !(1..=MAX_DIAGNOSE_AFTER_CHECKPOINTS)
        .contains(&policy.diagnose_after_stagnant_checkpoints.get())
    {
        return Err(ExecutionProfileError::Invalid(format!(
            "progress.diagnoseAfterStagnantCheckpoints must be between 1 and {MAX_DIAGNOSE_AFTER_CHECKPOINTS}"
        )));
    }
    if !(1..=MAX_ESCALATE_AFTER_REPLANS).contains(&policy.escalate_after_unsuccessful_replans.get())
    {
        return Err(ExecutionProfileError::Invalid(format!(
            "progress.escalateAfterUnsuccessfulReplans must be between 1 and {MAX_ESCALATE_AFTER_REPLANS}"
        )));
    }
    if !(1..=MAX_OBSERVATION_TIMEOUT_SECS).contains(&policy.observation_timeout_secs.get()) {
        return Err(ExecutionProfileError::Invalid(format!(
            "progress.observationTimeoutSecs must be between 1 and {MAX_OBSERVATION_TIMEOUT_SECS}"
        )));
    }
    Ok(())
}

fn validate_budgets(_budgets: &LegacyCampaignBudgetsV1) -> Result<(), ExecutionProfileError> {
    Ok(())
}

impl LegacyExecutionProfileContentV1 {
    /// Preserve the exact V1 content-hash computation so historical evidence
    /// keeps verifying. No cap is treated as authority by this method.
    pub fn compute_content_hash(&self) -> String {
        let mut canonical = self.clone();
        canonical.auto_start.risk_classes.sort();
        let raw = serde_json::to_vec(&canonical).expect("legacy execution profile is serializable");
        sha256_hex(&raw)
    }

    pub fn validate(&self) -> Result<(), ExecutionProfileError> {
        validate_text("name", &self.name, MAX_NAME_CHARS, true)?;
        validate_text(
            "description",
            &self.description,
            MAX_DESCRIPTION_CHARS,
            false,
        )?;
        for (role, route) in [
            (ExecutionRole::Lead, &self.roles.lead),
            (ExecutionRole::Implementer, &self.roles.implementer),
            (
                ExecutionRole::SemanticReviewer,
                &self.roles.semantic_reviewer,
            ),
        ] {
            if provider_catalog::entry(&route.provider).is_none() {
                return Err(ExecutionProfileError::UnsupportedRoute(format!(
                    "{} legacy route provider {:?} is not a configured Core provider connection",
                    role.as_str(),
                    route.provider
                )));
            }
            provider_catalog::validate_model(&route.model).map_err(|message| {
                ExecutionProfileError::UnsupportedRoute(format!(
                    "{} legacy route {}/{} model is invalid: {message}",
                    role.as_str(),
                    route.provider,
                    route.model
                ))
            })?;
        }
        validate_budgets(&self.budgets)?;
        validate_auto_start(&self.auto_start)?;
        Ok(())
    }
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
    use crate::execution_policy::ProgressPolicy;
    use std::num::NonZeroU32;

    fn route(provider: &str, model: &str) -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: provider.into(),
            model: model.into(),
            reasoning: Some(ReasoningEffort::Medium),
        }
    }

    pub(super) fn operations() -> OperationLimits {
        OperationLimits {
            model_request_timeout_secs: NonZeroU32::new(300).unwrap(),
            tool_call_timeout_secs: NonZeroU32::new(600).unwrap(),
            max_request_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_response_bytes: NonZeroU32::new(2 * 1024 * 1024).unwrap(),
            max_generation_tokens: NonZeroU32::new(8_192).unwrap(),
            max_in_flight_operations: NonZeroU32::new(4).unwrap(),
        }
    }

    pub(super) fn progress_policy() -> ProgressPolicy {
        ProgressPolicy {
            diagnose_after_stagnant_checkpoints: NonZeroU32::new(2).unwrap(),
            escalate_after_unsuccessful_replans: NonZeroU32::new(1).unwrap(),
            observation_timeout_secs: NonZeroU32::new(300).unwrap(),
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
            operations: operations(),
            progress: progress_policy(),
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
        // NonZeroU32 cannot represent zero; prove the maximum bound instead.
        content.operations.max_in_flight_operations =
            NonZeroU32::new(MAX_IN_FLIGHT_OPERATIONS + 1).unwrap();
        assert!(content.validate().is_err());

        let mut content = valid_content();
        content.operations.max_generation_tokens =
            NonZeroU32::new(MAX_GENERATION_TOKENS + 1).unwrap();
        assert!(content.validate().is_err());

        let mut content = valid_content();
        content.progress.diagnose_after_stagnant_checkpoints =
            NonZeroU32::new(MAX_DIAGNOSE_AFTER_CHECKPOINTS + 1).unwrap();
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
    fn v2_profile_content_has_no_productivity_caps() {
        let value = serde_json::to_value(valid_content()).unwrap();
        for forbidden in [
            "budgets",
            "maxTurns",
            "maxAttempts",
            "maxSessions",
            "maxRepairCycles",
            "maxWallClockSecs",
            "maxInputTokens",
            "maxOutputTokens",
            "maxCostMicros",
        ] {
            assert!(value.get(forbidden).is_none(), "{forbidden} must be absent");
        }
        assert!(value["operations"]["toolCallTimeoutSecs"].is_number());
        assert!(value["progress"]["diagnoseAfterStagnantCheckpoints"].is_number());
        assert!(value.get("schemaVersion").is_none());
    }

    #[test]
    fn legacy_v1_profile_keeps_its_original_hash_shape() {
        let legacy = LegacyExecutionProfileContentV1 {
            name: "Legacy".into(),
            description: "historical".into(),
            category: ExecutionProfileCategory::Balanced,
            roles: LegacyExecutionProfileRolesV1 {
                lead: legacy_route("opencode-go", "deepseek-v4.1-flash"),
                implementer: legacy_route("opencode-go", "deepseek-v4.1-flash"),
                semantic_reviewer: legacy_route("opencode-go", "deepseek-v4.1-flash"),
            },
            budgets: LegacyCampaignBudgetsV1 {
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
        };
        legacy.validate().unwrap();
        let hash = legacy.compute_content_hash();
        assert_eq!(hash.len(), 64);
        assert_eq!(
            ExecutionProfileRevisionContent::V1(legacy).content_hash(),
            hash
        );
    }

    fn legacy_route(provider: &str, model: &str) -> LegacyExecutionRoleRouteV1 {
        LegacyExecutionRoleRouteV1 {
            provider: provider.into(),
            model: model.into(),
            reasoning: Some(ReasoningEffort::Medium),
            max_input_tokens: 128_000,
            max_output_tokens: 8_192,
            max_cost_micros: 300_000,
        }
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
