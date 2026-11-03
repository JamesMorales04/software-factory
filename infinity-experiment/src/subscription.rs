//! Subscription-only provider feasibility and enforcement primitives.
//!
//! This module records what official documentation actually says, separates
//! completion adapters from external coding-agent integrations, and encodes
//! the Owner's funding constraints as fail-closed preflight checks.
//!
//! Nothing here authenticates to a real provider or makes an inference call.
//! Response-time billing fields are detectors, never preventive controls: a
//! `balance` response means a charge may already have happened. Prevention is
//! the preflight attestation and the absence of fallback credentials.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    /// Speaks the provider protocol and returns model completions.
    CompletionAdapter,
    /// A complete coding agent with its own tools, context, and approvals.
    ExternalCodingAgent,
    /// Not permitted under the mission constraints.
    Forbidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode {
    SubscriptionIncluded,
    PayAsYouGo,
    CreditsOrBalanceFallback,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationConfidence {
    /// Directly stated by a first-party page fetched during this experiment.
    OfficialDocumentation,
    /// Inferred from official documentation but not exercised live.
    PartiallyVerified,
    /// Not verified; must not be used for inference.
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRoute {
    pub id: String,
    pub display_name: String,
    pub kind: RouteKind,
    pub provider: String,
    pub model: String,
    pub authentication: String,
    pub official_permission: String,
    pub capabilities: Vec<String>,
    pub streaming: bool,
    pub tool_call_compatible: bool,
    pub quota_behavior: String,
    pub no_overage_controls: Vec<String>,
    pub known_limitations: Vec<String>,
    pub billing_mode: BillingMode,
    pub allowed_without_further_authorization: bool,
    pub decision: String,
    pub confidence: VerificationConfidence,
    pub sources: Vec<String>,
}

pub fn feasibility_matrix() -> Vec<SubscriptionRoute> {
    vec![
        SubscriptionRoute {
            id: "opencode-go-deepseek-v4.1-flash".to_owned(),
            display_name: "OpenCode Go — DeepSeek V4.1 Flash".to_owned(),
            kind: RouteKind::CompletionAdapter,
            provider: "opencode-go".to_owned(),
            model: "deepseek-v4.1-flash".to_owned(),
            authentication: "OpenCode Go subscription API key from the Console; Bearer token to https://opencode.ai/inference/go/openai/v1/chat/completions; stable x-opencode-session header for routing/caching.".to_owned(),
            official_permission: "Official OpenCode Go documentation exposes Go models to OpenCode and other coding agents, subject to abuse monitoring and privacy-region settings.".to_owned(),
            capabilities: vec![
                "Chat Completions compatible".to_owned(),
                "streaming with final usage/cost metadata".to_owned(),
                "tool calling supported by the model family".to_owned(),
                "1M context / 384K max output per third-party catalog; verify from /v1/models".to_owned(),
            ],
            streaming: true,
            tool_call_compatible: true,
            quota_behavior: "Per-model monthly dollar allowance, with 5-hour (20%) and weekly (50%) sub-limits; each response reports opencode.allowance. DeepSeek V4.1 Flash has a $100 monthly limit on the Go plan.".to_owned(),
            no_overage_controls: vec![
                "Owner must disable Extra Usage in the OpenCode Console Go page.".to_owned(),
                "Owner must disable credits/balance fallback; only the Go subscription key may be present.".to_owned(),
                "Guard requires an explicit owner attestation before a request is dispatched.".to_owned(),
                "Post-response billing_source must be go/go-plus and cost must be zero; otherwise halt the run and surface a funding violation.".to_owned(),
            ],
            known_limitations: vec![
                "Extra Usage is an account setting; response metadata cannot undo a charge that already occurred.".to_owned(),
                "DeepSeek models may require Global region selection in workspace privacy settings.".to_owned(),
                "The API key is a bearer credential; the Factory must keep it inside its provider boundary.".to_owned(),
                "Model roster, prices and limits change; re-verify before production.".to_owned(),
            ],
            billing_mode: BillingMode::SubscriptionIncluded,
            allowed_without_further_authorization: false,
            decision: "ALLOWED ONLY behind owner attestation, no Extra Usage/credits, a deterministic fake first, and a separate live-test authorization.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec![
                "https://opencode.ai/v2/docs/console/go/".to_owned(),
                "https://opencode.ai/v2/docs/console/api/inference".to_owned(),
                "https://opencode.ai/docs/providers".to_owned(),
            ],
        },
        SubscriptionRoute {
            id: "codex-app-server-chatgpt".to_owned(),
            display_name: "Codex App Server — ChatGPT subscription".to_owned(),
            kind: RouteKind::ExternalCodingAgent,
            provider: "openai-codex".to_owned(),
            model: "account-selected (e.g. GPT-6 Luna / GPT-6.1 Sol)".to_owned(),
            authentication: "Local Codex sign-in with ChatGPT (`~/.codex/auth.json` or OS keyring); app-server JSON-RPC over stdio/WebSocket/Unix socket. API-key sign-in is a different billing mode and is refused.".to_owned(),
            official_permission: "OpenAI documents app-server as the supported way to embed Codex in a product, including authentication, history, approvals and streamed events. It is an agent integration, not a completion API.".to_owned(),
            capabilities: vec![
                "thread/start, thread/resume, thread/fork".to_owned(),
                "turn/start, turn/steer, turn/interrupt".to_owned(),
                "streamed item/turn notifications and approvals".to_owned(),
                "its own tools, sandbox, skills, MCP servers and context".to_owned(),
            ],
            streaming: true,
            tool_call_compatible: false,
            quota_behavior: "Uses the signed-in ChatGPT plan's shared Work/Codex usage and workspace limits; /status and the usage dashboard expose remaining capacity.".to_owned(),
            no_overage_controls: vec![
                "Refuse API-key auth (`forced_login_method = chatgpt` where policy permits).".to_owned(),
                "Do not enable credits or extra usage for the Factory workspace.".to_owned(),
                "Do not expose app-server to untrusted clients; keep it loopback/stdio and behind Factory authorization.".to_owned(),
                "Do not silently switch provider or account when limits are reached; fail closed.".to_owned(),
            ],
            known_limitations: vec![
                "It is a complete coding agent: wrapping it as a `ModelProvider` loses approvals, tool semantics, context and cancellation boundaries.".to_owned(),
                "Factory is not a registered Sign in with ChatGPT client; this route relies on local Codex authentication, not Factory OAuth.".to_owned(),
                "Usage exhaustion and credits behavior are account/workspace settings, not a stable API contract.".to_owned(),
                "App-server API surface evolves; pin the Codex version and generated schema.".to_owned(),
            ],
            billing_mode: BillingMode::SubscriptionIncluded,
            allowed_without_further_authorization: false,
            decision: "ALLOWED ONLY as an external coding-agent integration, after a separate owner authorization and with ChatGPT auth pinned.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec![
                "https://learn.chatgpt.com/docs/app-server".to_owned(),
                "https://learn.chatgpt.com/docs/auth".to_owned(),
                "https://learn.chatgpt.com/docs/pricing".to_owned(),
            ],
        },
        SubscriptionRoute {
            id: "codex-sdk-chatgpt".to_owned(),
            display_name: "Codex SDK — ChatGPT subscription".to_owned(),
            kind: RouteKind::ExternalCodingAgent,
            provider: "openai-codex".to_owned(),
            model: "account-selected Codex model".to_owned(),
            authentication: "Codex SDK controls the local Codex app-server. It inherits the local ChatGPT sign-in; an API key selects API billing instead.".to_owned(),
            official_permission: "OpenAI documents the TypeScript/Python SDK for programmatic coding tasks and internal tools; the Python SDK explicitly controls app-server over JSON-RPC.".to_owned(),
            capabilities: vec![
                "thread start/resume, run, streaming events".to_owned(),
                "sandbox presets read_only/workspace_write/full_access".to_owned(),
                "external tool messages and structured final response".to_owned(),
            ],
            streaming: true,
            tool_call_compatible: false,
            quota_behavior: "Same ChatGPT plan/Codex usage as app-server; the SDK itself does not create an allowance.".to_owned(),
            no_overage_controls: vec![
                "Use a ChatGPT-authenticated Codex home; never pass an API key.".to_owned(),
                "Run with workspace_write or read_only, not full_access, inside Factory's wider confinement.".to_owned(),
                "Fail closed on authentication-mode mismatch.".to_owned(),
            ],
            known_limitations: vec![
                "SDK adds a Node.js/Python runtime dependency that conflicts with Factory's container-native preference unless packaged in the image.".to_owned(),
                "It is an agent, not a completion adapter.".to_owned(),
                "SDK/Codex version compatibility must be pinned.".to_owned(),
            ],
            billing_mode: BillingMode::SubscriptionIncluded,
            allowed_without_further_authorization: false,
            decision: "ALLOWED ONLY as a specialized coding-agent integration; prefer app-server for a Rust host.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec![
                "https://learn.chatgpt.com/docs/codex-sdk".to_owned(),
                "https://learn.chatgpt.com/docs/auth".to_owned(),
            ],
        },
        SubscriptionRoute {
            id: "sign-in-with-chatgpt-third-party".to_owned(),
            display_name: "Sign in with ChatGPT — third-party app OAuth".to_owned(),
            kind: RouteKind::Forbidden,
            provider: "openai".to_owned(),
            model: "n/a".to_owned(),
            authentication: "OAuth client registered with OpenAI; the user grants the app permission to use their ChatGPT plan.".to_owned(),
            official_permission: "OpenAI requires a registered client ID and lists participating apps. Factory is not registered in this experiment.".to_owned(),
            capabilities: vec!["identity sign-in".to_owned(), "optionally plan usage for eligible apps".to_owned()],
            streaming: false,
            tool_call_compatible: false,
            quota_behavior: "Counts toward the user's existing plan; app limits are caps, not a new allowance.".to_owned(),
            no_overage_controls: vec![
                "Do not invent an entitlement endpoint.".to_owned(),
                "Do not use an unregistered OAuth client.".to_owned(),
            ],
            known_limitations: vec![
                "No client ID or owner approval in this experiment.".to_owned(),
                "Sign-in permission and plan-usage permission are separate consents.".to_owned(),
            ],
            billing_mode: BillingMode::SubscriptionIncluded,
            allowed_without_further_authorization: false,
            decision: "BLOCKED: requires OpenAI client registration and a separate product/legal review.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec![
                "https://learn.chatgpt.com/docs/sign-in-with-chatgpt".to_owned(),
                "https://developers.openai.com/siwc".to_owned(),
            ],
        },
        SubscriptionRoute {
            id: "openai-chatgpt-oauth-completion".to_owned(),
            display_name: "ChatGPT OAuth as an OpenAI-compatible completion endpoint".to_owned(),
            kind: RouteKind::Forbidden,
            provider: "openai".to_owned(),
            model: "n/a".to_owned(),
            authentication: "ChatGPT browser OAuth token used directly against a completion endpoint.".to_owned(),
            official_permission: "No official public completion API with ChatGPT subscription OAuth is documented for third parties. OpenCode's bundled integration is an OpenCode implementation, not a Factory entitlement.".to_owned(),
            capabilities: vec![],
            streaming: false,
            tool_call_compatible: false,
            quota_behavior: "Undocumented for this use.".to_owned(),
            no_overage_controls: vec!["Forbidden: do not scrape tokens or reverse-engineer private endpoints.".to_owned()],
            known_limitations: vec![
                "Would violate the Owner's no-unapproved-auth rule.".to_owned(),
                "Unstable and outside official support.".to_owned(),
            ],
            billing_mode: BillingMode::Unknown,
            allowed_without_further_authorization: false,
            decision: "FORBIDDEN.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec!["https://learn.chatgpt.com/docs/auth".to_owned()],
        },
        SubscriptionRoute {
            id: "openai-api-key".to_owned(),
            display_name: "OpenAI API key".to_owned(),
            kind: RouteKind::Forbidden,
            provider: "openai".to_owned(),
            model: "n/a".to_owned(),
            authentication: "Platform API key.".to_owned(),
            official_permission: "Official API, but billed pay-as-you-go.".to_owned(),
            capabilities: vec!["completion API".to_owned()],
            streaming: true,
            tool_call_compatible: true,
            quota_behavior: "Standard API pricing per token.".to_owned(),
            no_overage_controls: vec!["Forbidden by the Owner funding envelope.".to_owned()],
            known_limitations: vec!["Pay-as-you-go is explicitly forbidden.".to_owned()],
            billing_mode: BillingMode::PayAsYouGo,
            allowed_without_further_authorization: false,
            decision: "FORBIDDEN.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec!["https://learn.chatgpt.com/docs/auth".to_owned()],
        },
        SubscriptionRoute {
            id: "aws-bedrock".to_owned(),
            display_name: "AWS Bedrock inference".to_owned(),
            kind: RouteKind::Forbidden,
            provider: "aws-bedrock".to_owned(),
            model: "n/a".to_owned(),
            authentication: "AWS credentials / SigV4.".to_owned(),
            official_permission: "Official AWS service, but outside the allowed funding modes.".to_owned(),
            capabilities: vec!["provider-specific inference".to_owned()],
            streaming: true,
            tool_call_compatible: true,
            quota_behavior: "AWS billing.".to_owned(),
            no_overage_controls: vec!["Forbidden by the Owner funding envelope.".to_owned()],
            known_limitations: vec!["Explicitly forbidden; Infinity's Bedrock provider must not be enabled.".to_owned()],
            billing_mode: BillingMode::PayAsYouGo,
            allowed_without_further_authorization: false,
            decision: "FORBIDDEN.".to_owned(),
            confidence: VerificationConfidence::OfficialDocumentation,
            sources: vec!["Infinity repository provider-bedrock crate".to_owned()],
        },
    ]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OwnerAttestation {
    pub opencode_go_subscription_active: bool,
    pub opencode_go_extra_usage_disabled: bool,
    pub opencode_go_credit_fallback_disabled: bool,
    pub no_api_key_billing_credentials: bool,
    pub chatgpt_subscription_active: bool,
    pub codex_app_server_authorized: bool,
    pub workspace_confinement_verified: bool,
    pub live_test_authorized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreflightDecision {
    Allow,
    Deny(Vec<String>),
}

impl PreflightDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BillingObservation {
    pub billing_source: Option<String>,
    pub cost_usd: Option<f64>,
    pub allowance_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FundingViolation {
    NonSubscriptionBillingSource(String),
    ChargedAmount,
    MissingBillingMetadata,
    ExplicitExtraUsage,
}

#[derive(Clone)]
pub struct SubscriptionGuard {
    attestation: OwnerAttestation,
    allowed_models: HashSet<String>,
}

impl SubscriptionGuard {
    pub fn new(attestation: OwnerAttestation) -> Self {
        Self {
            attestation,
            allowed_models: HashSet::from(["deepseek-v4.1-flash".to_owned()]),
        }
    }

    pub fn preflight(&self, route_id: &str) -> PreflightDecision {
        let mut reasons = Vec::new();
        match route_id {
            "opencode-go-deepseek-v4.1-flash" => {
                if !self.attestation.opencode_go_subscription_active {
                    reasons.push("OpenCode Go subscription is not attested active".to_owned());
                }
                if !self.attestation.opencode_go_extra_usage_disabled {
                    reasons.push("OpenCode Go Extra Usage is not attested disabled".to_owned());
                }
                if !self.attestation.opencode_go_credit_fallback_disabled {
                    reasons.push(
                        "OpenCode Go credit/balance fallback is not attested disabled".to_owned(),
                    );
                }
                if !self.attestation.no_api_key_billing_credentials {
                    reasons.push("API-key billing credentials may be present".to_owned());
                }
                if !self.attestation.live_test_authorized {
                    reasons.push("no separate live-inference authorization envelope".to_owned());
                }
                if !self.allowed_models.contains("deepseek-v4.1-flash") {
                    reasons.push("model is not in the subscription allowlist".to_owned());
                }
            }
            "codex-app-server-chatgpt" | "codex-sdk-chatgpt" => {
                if !self.attestation.chatgpt_subscription_active {
                    reasons.push("ChatGPT subscription is not attested active".to_owned());
                }
                if !self.attestation.codex_app_server_authorized {
                    reasons.push(
                        "Codex app-server integration is not separately authorized".to_owned(),
                    );
                }
                if !self.attestation.workspace_confinement_verified {
                    reasons.push("workspace confinement is not verified".to_owned());
                }
                if !self.attestation.no_api_key_billing_credentials {
                    reasons.push("API-key billing credentials may be present".to_owned());
                }
                if !self.attestation.live_test_authorized {
                    reasons.push("no separate live-inference authorization envelope".to_owned());
                }
            }
            "sign-in-with-chatgpt-third-party" => {
                reasons.push("Factory has no registered Sign in with ChatGPT client ID".to_owned());
            }
            "openai-chatgpt-oauth-completion" => {
                reasons
                    .push("no official public ChatGPT-subscription completion endpoint".to_owned());
            }
            "openai-api-key" | "aws-bedrock" => {
                reasons.push("pay-as-you-go inference is forbidden".to_owned());
            }
            other => reasons.push(format!("unknown route '{other}'")),
        }
        if reasons.is_empty() {
            PreflightDecision::Allow
        } else {
            PreflightDecision::Deny(reasons)
        }
    }

    /// Check post-response metadata. This detects a violation; it cannot
    /// prevent or reverse a charge, so a violation must halt the run and be
    /// reported to the Owner.
    pub fn observe(
        &self,
        route_id: &str,
        observation: &BillingObservation,
    ) -> Result<(), FundingViolation> {
        match route_id {
            "opencode-go-deepseek-v4.1-flash" => {
                if observation.cost_usd.unwrap_or(0.0) > 0.0 {
                    return Err(FundingViolation::ChargedAmount);
                }
                match observation.billing_source.as_deref() {
                    Some("go" | "go-plus") => Ok(()),
                    Some(other) => Err(FundingViolation::NonSubscriptionBillingSource(
                        other.to_owned(),
                    )),
                    None => Err(FundingViolation::MissingBillingMetadata),
                }
            }
            _ => Err(FundingViolation::MissingBillingMetadata),
        }
    }
}

/// Deterministic fake completion provider used to exercise the subscription
/// guard without network inference. It reports a caller-chosen billing source
/// so a test can prove that a balance/byok response is rejected.
#[derive(Clone)]
pub struct FakeSubscriptionProvider {
    route_id: String,
    observation: BillingObservation,
    calls: Arc<Mutex<Vec<BillingObservation>>>,
}

impl FakeSubscriptionProvider {
    pub fn new(route_id: impl Into<String>, billing_source: &str, cost_usd: f64) -> Self {
        Self {
            route_id: route_id.into(),
            observation: BillingObservation {
                billing_source: Some(billing_source.to_owned()),
                cost_usd: Some(cost_usd),
                allowance_usd: Some(cost_usd),
            },
            calls: Default::default(),
        }
    }

    pub fn route_id(&self) -> &str {
        &self.route_id
    }

    pub fn last_observation(&self) -> Option<BillingObservation> {
        self.calls.lock().expect("bug: calls lock").last().cloned()
    }
}

#[async_trait]
impl infinity_provider_protocol::ModelProvider for FakeSubscriptionProvider {
    async fn list_models(
        &self,
    ) -> Result<Vec<infinity_provider_protocol::ModelEntry>, Box<dyn std::error::Error + Send + Sync>>
    {
        Ok(vec![infinity_provider_protocol::ModelEntry {
            model_id: "deepseek-v4.1-flash".to_owned(),
            display_name: "DeepSeek V4.1 Flash (fake subscription)".to_owned(),
            context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            supports_image_input: false,
        }])
    }

    async fn invoke_model(
        &self,
        _model_id: &str,
        _request: infinity_provider_protocol::CompletionRequest,
    ) -> Result<infinity_provider_protocol::ModelStream, infinity_provider_protocol::CompletionError>
    {
        self.calls
            .lock()
            .expect("bug: calls lock")
            .push(self.observation.clone());
        let chunks = vec![
            Ok(infinity_provider_protocol::StreamChunk::Text(
                "deterministic fake subscription response".to_owned(),
            )),
            Ok(infinity_provider_protocol::StreamChunk::Final(
                infinity_provider_protocol::FinalResponse {
                    usage: Some(infinity_provider_protocol::Usage {
                        input_tokens: 3,
                        output_tokens: 4,
                        total_tokens: 7,
                        cached_input_tokens: 0,
                    }),
                },
            )),
        ];
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

/// One frame of a deliberately small Codex app-server state machine. This is
/// not a Codex implementation; it documents the integration boundary that a
/// future adapter must preserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAuthMode {
    ChatGptSubscription,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexTurnEvent {
    ThreadStarted { thread_id: String },
    TurnStarted { turn_id: String },
    AgentMessageDelta { text: String },
    TurnCompleted { status: String },
    Interrupted,
}

#[derive(Debug, Clone)]
pub struct FakeCodexAppServer {
    auth_mode: CodexAuthMode,
    initialized: bool,
    events: Vec<CodexTurnEvent>,
}

impl FakeCodexAppServer {
    pub fn new(auth_mode: CodexAuthMode) -> Self {
        Self {
            auth_mode,
            initialized: false,
            events: Vec::new(),
        }
    }

    pub fn initialize(&mut self) -> Result<(), &'static str> {
        if self.auth_mode != CodexAuthMode::ChatGptSubscription {
            return Err("API-key auth is forbidden; sign in with ChatGPT");
        }
        self.initialized = true;
        Ok(())
    }

    pub fn start_thread_and_turn(
        &mut self,
        prompt: &str,
    ) -> Result<Vec<CodexTurnEvent>, &'static str> {
        if !self.initialized {
            return Err("initialize/initialized handshake is required first");
        }
        let events = vec![
            CodexTurnEvent::ThreadStarted {
                thread_id: "thread-fixture".to_owned(),
            },
            CodexTurnEvent::TurnStarted {
                turn_id: "turn-fixture".to_owned(),
            },
            CodexTurnEvent::AgentMessageDelta {
                text: format!("reviewed: {prompt}"),
            },
            CodexTurnEvent::TurnCompleted {
                status: "completed".to_owned(),
            },
        ];
        self.events.extend(events.clone());
        Ok(events)
    }

    pub fn interrupt(&mut self) -> CodexTurnEvent {
        CodexTurnEvent::Interrupted
    }

    pub fn events(&self) -> &[CodexTurnEvent] {
        &self.events
    }
}
