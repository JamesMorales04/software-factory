//! Bounded per-role model gateway construction.
//!
//! A profile role names a provider connection and a model. This module turns
//! that declaration into an actual `ModelGateway` only when Core can substantiate
//! every part of the route:
//!
//! 1. the provider connection exists and is enabled;
//! 2. its encrypted credential decrypts inside Core;
//! 3. the implemented adapter can execute the role transport;
//! 4. the model is advertised by the connection (configured model, tested
//!    registry route or real discovery).
//!
//! The resulting gateway is bound to the role and carries the intersection of
//! the profile route caps, the provider settings caps and Core's deployment
//! caps. Credentials never leave this module. Model output never grants
//! authority.

use std::time::Duration;

use crate::db::{Database, ModelCallPolicy, ModelCallRole, ProviderSettingsRecord};
use crate::execution_profile::{ExecutionRole, ExecutionRoleRoute};
use crate::model_gateway::{ModelGateway, ModelGatewayConfig, ModelGatewayError};
use crate::provider_adapter::ProviderAdapter;
use crate::provider_catalog;
use crate::provider_secrets::ProviderSecretKey;

pub const DEFAULT_MODEL_TIMEOUT_SECS: u64 = 60;
pub const MAX_MODEL_TIMEOUT_SECS: u64 = 90;

/// Deployment-wide Core caps. A profile or provider can narrow these, never
/// widen them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreModelCaps {
    pub lead: ModelCallPolicy,
    pub reviewer: ModelCallPolicy,
    pub evaluation: ModelCallPolicy,
    pub global: ModelCallPolicy,
}

impl CoreModelCaps {
    pub fn new(
        lead: ModelCallPolicy,
        reviewer: ModelCallPolicy,
        evaluation: ModelCallPolicy,
        global: ModelCallPolicy,
    ) -> Self {
        Self {
            lead,
            reviewer,
            evaluation,
            global,
        }
    }

    /// Read the deployment caps from Core environment variables. Invalid values
    /// are an error, never a silent default.
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            lead: policy_from_env("FACTORY_LEAD_MODEL", 128_000, 8_192, 300_000)?,
            reviewer: policy_from_env("FACTORY_REVIEW_MODEL", 400_000, 8_192, 1_000_000)?,
            evaluation: policy_from_env("FACTORY_EVALUATION_MODEL", 400_000, 8_192, 2_000_000)?,
            global: policy_from_env("FACTORY_MODEL_GLOBAL", 2_000_000, 100_000, 10_000_000)?,
        })
    }

    pub fn role_policy(&self, role: ExecutionRole) -> Option<ModelCallPolicy> {
        match role {
            ExecutionRole::Lead => Some(self.lead),
            ExecutionRole::SemanticReviewer => Some(self.reviewer),
            ExecutionRole::Implementer => None,
        }
    }
}

fn policy_from_env(
    prefix: &str,
    input_default: u64,
    output_default: u64,
    cost_default: u64,
) -> Result<ModelCallPolicy, String> {
    Ok(ModelCallPolicy {
        max_input_tokens: env_u64(&format!("{prefix}_MAX_INPUT_TOKENS"), input_default)?,
        max_output_tokens: env_u64(&format!("{prefix}_MAX_OUTPUT_TOKENS"), output_default)?,
        max_cost_micros: env_u64(&format!("{prefix}_MAX_COST_MICROS"), cost_default)?,
    })
}

fn env_u64(name: &str, default: u64) -> Result<u64, String> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be a nonnegative integer")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(format!("{name} is not readable: {error}")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    #[error("no provider connection {provider:?} is configured")]
    ProviderNotConfigured { provider: String },
    #[error("provider {provider:?} is disabled")]
    ProviderDisabled { provider: String },
    #[error("provider {provider:?} has no usable stored credential")]
    CredentialUnavailable { provider: String },
    #[error("provider {provider:?} kind {kind:?} has no supported {role} adapter for this role")]
    AdapterUnsupported {
        provider: String,
        kind: String,
        role: String,
    },
    #[error("provider {provider:?} does not advertise model {model:?}")]
    ModelUnavailable { provider: String, model: String },
    #[error("the {role} role is not a one-shot model gateway role")]
    NotAGatewayRole { role: String },
    #[error("model route lookup failed: {0}")]
    Database(String),
    #[error("model route configuration for {provider}/{model} is invalid: {detail}")]
    Invalid {
        provider: String,
        model: String,
        detail: String,
    },
}

impl RouteError {
    /// Stable HTTP-facing classification. Details stay human-readable.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProviderNotConfigured { .. } => "MODEL_PROVIDER_NOT_CONFIGURED",
            Self::ProviderDisabled { .. } => "MODEL_PROVIDER_DISABLED",
            Self::CredentialUnavailable { .. } => "MODEL_CREDENTIAL_UNAVAILABLE",
            Self::AdapterUnsupported { .. } => "MODEL_ADAPTER_UNSUPPORTED",
            Self::ModelUnavailable { .. } => "MODEL_NOT_AVAILABLE",
            Self::NotAGatewayRole { .. } => "MODEL_ROLE_NOT_GATEWAY",
            Self::Database(_) => "MODEL_ROUTE_DATABASE",
            Self::Invalid { .. } => "MODEL_ROUTE_INVALID",
        }
    }
}

/// Core-owned factory for one bounded role gateway. It holds no gateway of its
/// own; each call decrypts exactly the selected provider credential.
pub struct RoleGatewayFactory<'a> {
    database: &'a Database,
    master_key: Option<&'a ProviderSecretKey>,
    caps: CoreModelCaps,
    timeout: Duration,
    allow_loopback_http: bool,
}

impl<'a> RoleGatewayFactory<'a> {
    pub fn new(
        database: &'a Database,
        master_key: Option<&'a ProviderSecretKey>,
        caps: CoreModelCaps,
        timeout: Duration,
        allow_loopback_http: bool,
    ) -> Self {
        Self {
            database,
            master_key,
            caps,
            timeout: timeout.min(Duration::from_secs(MAX_MODEL_TIMEOUT_SECS)),
            allow_loopback_http,
        }
    }

    /// Construct the exact gateway for one declared profile role route.
    pub async fn for_role(
        &self,
        role: ExecutionRole,
        route: &ExecutionRoleRoute,
    ) -> Result<ModelGateway, RouteError> {
        let (call_role, core_policy) = match role {
            ExecutionRole::Lead => (ModelCallRole::Lead, self.caps.lead),
            ExecutionRole::SemanticReviewer => {
                (ModelCallRole::SemanticReviewer, self.caps.reviewer)
            }
            ExecutionRole::Implementer => {
                return Err(RouteError::NotAGatewayRole {
                    role: role.as_str().into(),
                });
            }
        };
        self.build(call_role, role.as_str(), route, core_policy)
            .await
    }

    /// Construct a bounded Evaluation gateway for an explicit CAP-014 route.
    /// Evaluation has no Execution Profile role; the route is supplied by the
    /// evaluation operation and still intersects provider and Core caps.
    pub async fn for_evaluation(
        &self,
        route: &ExecutionRoleRoute,
    ) -> Result<ModelGateway, RouteError> {
        self.build(
            ModelCallRole::Evaluation,
            "evaluation",
            route,
            self.caps.evaluation,
        )
        .await
    }

    async fn build(
        &self,
        call_role: ModelCallRole,
        role_label: &str,
        route: &ExecutionRoleRoute,
        core_policy: ModelCallPolicy,
    ) -> Result<ModelGateway, RouteError> {
        let settings = self
            .database
            .provider_settings(&route.provider)
            .await
            .map_err(|error| match error {
                crate::db::Error::NotFound => RouteError::ProviderNotConfigured {
                    provider: route.provider.clone(),
                },
                other => RouteError::Database(other.to_string()),
            })?;
        if !settings.enabled {
            return Err(RouteError::ProviderDisabled {
                provider: route.provider.clone(),
            });
        }
        let Some(adapter) = ProviderAdapter::from_kind(&settings.kind) else {
            return Err(RouteError::AdapterUnsupported {
                provider: route.provider.clone(),
                kind: settings.kind.clone(),
                role: role_label.into(),
            });
        };
        if !adapter.supports_chat_completions() {
            return Err(RouteError::AdapterUnsupported {
                provider: route.provider.clone(),
                kind: settings.kind.clone(),
                role: role_label.into(),
            });
        }
        if let Err(message) = provider_catalog::validate_model(&route.model) {
            return Err(RouteError::Invalid {
                provider: route.provider.clone(),
                model: route.model.clone(),
                detail: message,
            });
        }
        let advertised = crate::execution_profile::substantiated_models(
            &settings.provider,
            &settings.model,
            &self.database.provider_model_cache(),
        );
        if !advertised.iter().any(|model| model == &route.model) {
            return Err(RouteError::ModelUnavailable {
                provider: route.provider.clone(),
                model: route.model.clone(),
            });
        }
        let credential = self.decrypt_credential(&settings)?;
        let policy = intersect_policies(
            ModelCallPolicy {
                max_input_tokens: route.max_input_tokens,
                max_output_tokens: route.max_output_tokens,
                max_cost_micros: route.max_cost_micros,
            },
            ModelCallPolicy {
                max_input_tokens: settings.max_input_tokens,
                max_output_tokens: settings.max_output_tokens,
                max_cost_micros: settings.max_cost_micros,
            },
            core_policy,
        );
        let config = ModelGatewayConfig {
            base_url: settings.base_url.clone(),
            credential,
            provider: settings.provider.clone(),
            model: route.model.clone(),
            timeout: self.timeout,
            input_micros_per_million: settings.input_micros_per_million,
            output_micros_per_million: settings.output_micros_per_million,
            lead_policy: policy,
            reviewer_policy: policy,
            evaluation_policy: policy,
            global_policy: self.caps.global,
            allow_loopback_http: self.allow_loopback_http,
            bound_role: Some(call_role),
        };
        ModelGateway::new(config).map_err(|error| match error {
            ModelGatewayError::Invalid(detail) => RouteError::Invalid {
                provider: settings.provider,
                model: route.model.clone(),
                detail,
            },
            other => RouteError::Invalid {
                provider: settings.provider,
                model: route.model.clone(),
                detail: other.to_string(),
            },
        })
    }

    fn decrypt_credential(&self, settings: &ProviderSettingsRecord) -> Result<String, RouteError> {
        let unavailable = || RouteError::CredentialUnavailable {
            provider: settings.provider.clone(),
        };
        if settings.credential_ciphertext.is_empty() || settings.credential_nonce.is_empty() {
            return Err(unavailable());
        }
        let Some(master_key) = self.master_key else {
            return Err(unavailable());
        };
        let plaintext = master_key
            .decrypt(&settings.credential_ciphertext, &settings.credential_nonce)
            .map_err(|_| unavailable())?;
        let credential = String::from_utf8(plaintext).map_err(|_| unavailable())?;
        if provider_catalog::validate_credential(&credential).is_err() {
            return Err(unavailable());
        }
        crate::telemetry::register_secret(&credential);
        Ok(credential)
    }
}

/// Intersect three policies. Every limit takes the strictest value; a route can
/// never raise provider or deployment caps.
pub fn intersect_policies(
    route: ModelCallPolicy,
    provider: ModelCallPolicy,
    core: ModelCallPolicy,
) -> ModelCallPolicy {
    ModelCallPolicy {
        max_input_tokens: route
            .max_input_tokens
            .min(provider.max_input_tokens)
            .min(core.max_input_tokens),
        max_output_tokens: route
            .max_output_tokens
            .min(provider.max_output_tokens)
            .min(core.max_output_tokens),
        max_cost_micros: route
            .max_cost_micros
            .min(provider.max_cost_micros)
            .min(core.max_cost_micros),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(input: u64, output: u64, cost: u64) -> ModelCallPolicy {
        ModelCallPolicy {
            max_input_tokens: input,
            max_output_tokens: output,
            max_cost_micros: cost,
        }
    }

    #[test]
    fn budget_intersection_takes_the_strictest_cap() {
        let result = intersect_policies(
            policy(10_000, 4_096, 900_000),
            policy(8_000, 8_192, 500_000),
            policy(128_000, 2_048, 300_000),
        );
        assert_eq!(result, policy(8_000, 2_048, 300_000));
    }

    #[test]
    fn capability_errors_have_stable_codes() {
        assert_eq!(
            RouteError::AdapterUnsupported {
                provider: "codex".into(),
                kind: "codex".into(),
                role: "lead".into(),
            }
            .code(),
            "MODEL_ADAPTER_UNSUPPORTED"
        );
        assert_eq!(
            RouteError::ModelUnavailable {
                provider: "opencode-go".into(),
                model: "unknown".into(),
            }
            .code(),
            "MODEL_NOT_AVAILABLE"
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::db::ProviderSettingsWrite;
    use crate::execution_profile::{ExecutionRoleRoute, ReasoningEffort};
    use std::io::Write;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn policy(input: u64, output: u64, cost: u64) -> ModelCallPolicy {
        ModelCallPolicy {
            max_input_tokens: input,
            max_output_tokens: output,
            max_cost_micros: cost,
        }
    }

    fn master_key() -> ProviderSecretKey {
        master_key_with("22")
    }

    fn master_key_with(seed: &str) -> ProviderSecretKey {
        let path = std::env::temp_dir().join(format!(
            "factory-routing-master-key-{}.txt",
            uuid::Uuid::new_v4()
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(seed.repeat(32).as_bytes()).unwrap();
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let key = ProviderSecretKey::from_file(&path).unwrap();
        let _ = std::fs::remove_file(path);
        key
    }

    fn caps() -> CoreModelCaps {
        CoreModelCaps::new(
            policy(128_000, 8_192, 300_000),
            policy(400_000, 8_192, 1_000_000),
            policy(200_000, 8_192, 2_000_000),
            policy(2_000_000, 100_000, 10_000_000),
        )
    }

    fn route(provider: &str, model: &str, output: u64, cost: u64) -> ExecutionRoleRoute {
        ExecutionRoleRoute {
            provider: provider.into(),
            model: model.into(),
            reasoning: Some(ReasoningEffort::Medium),
            max_input_tokens: 64_000,
            max_output_tokens: output,
            max_cost_micros: cost,
        }
    }

    async fn upstream() -> (String, Arc<Mutex<Vec<String>>>) {
        use axum::{Router, routing::post};
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let state = seen.clone();
        async fn chat(
            axum::extract::State(seen): axum::extract::State<Arc<Mutex<Vec<String>>>>,
            headers: axum::http::HeaderMap,
        ) -> axum::Json<serde_json::Value> {
            let authorization = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            seen.lock().await.push(authorization);
            axum::Json(serde_json::json!({
                "choices": [{"message": {"content": "{\"ok\":true}"}}],
                "usage": {"prompt_tokens": 12, "completion_tokens": 7, "total_tokens": 19}
            }))
        }
        let app = Router::new()
            .route("/chat/completions", post(chat))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    async fn database() -> (std::path::PathBuf, Database) {
        let path =
            std::env::temp_dir().join(format!("factory-routing-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Database::open(&path).await.unwrap();
        (path, db)
    }

    async fn configure(
        db: &Database,
        key: &ProviderSecretKey,
        provider: &str,
        model: &str,
        base_url: &str,
        credential: &str,
        enabled: bool,
    ) {
        let kind = if provider == "codex" {
            "codex"
        } else {
            "opencode"
        };
        let encrypted = key.encrypt(credential.as_bytes()).unwrap();
        db.save_provider_settings(
            &ProviderSettingsWrite {
                provider: provider.into(),
                kind: kind.into(),
                model: model.into(),
                base_url: base_url.into(),
                auth_mode: "api_key".into(),
                credential_ciphertext: encrypted.ciphertext,
                credential_nonce: encrypted.nonce,
                credential_fingerprint: encrypted.fingerprint,
                enabled,
                is_default: provider == "opencode-go",
                max_turns: 4,
                max_input_tokens: 40_000,
                max_output_tokens: 4_096,
                max_cost_micros: 500_000,
                input_micros_per_million: 1_000,
                output_micros_per_million: 2_000,
            },
            "test-operator",
        )
        .await
        .unwrap();
    }

    fn request<'a>(
        role: ModelCallRole,
        scope: &'a str,
    ) -> crate::model_gateway::ModelCallRequest<'a> {
        crate::model_gateway::ModelCallRequest {
            role,
            principal_id: "reviewer",
            project_id: "project",
            objective_id: scope,
            scope_id: scope,
            purpose: "test",
            system_prompt: "Return JSON.",
            user_prompt: "prompt",
            max_output_tokens: 2_000,
        }
    }

    #[tokio::test]
    async fn each_role_uses_its_own_provider_model_and_credential() {
        let key = master_key();
        let (path, db) = database().await;
        let (lead_url, lead_seen) = upstream().await;
        let (review_url, review_seen) = upstream().await;
        configure(
            &db,
            &key,
            "opencode-go",
            "deepseek-v4.1-flash",
            &lead_url,
            "lead-credential-1234567890",
            true,
        )
        .await;
        configure(
            &db,
            &key,
            "opencode",
            "reviewer-model",
            &review_url,
            "review-credential-1234567890",
            true,
        )
        .await;
        let factory =
            RoleGatewayFactory::new(&db, Some(&key), caps(), Duration::from_secs(5), true);

        let lead = factory
            .for_role(
                ExecutionRole::Lead,
                &route("opencode-go", "deepseek-v4.1-flash", 8_192, 300_000),
            )
            .await
            .unwrap();
        assert_eq!(lead.provider(), "opencode-go");
        assert_eq!(lead.model(), "deepseek-v4.1-flash");
        assert_eq!(lead.bound_role(), Some(ModelCallRole::Lead));
        // Route output cap is intersected with the provider's.
        assert_eq!(
            lead.role_policy(ModelCallRole::Lead).max_output_tokens,
            4_096
        );
        assert_eq!(
            lead.role_policy(ModelCallRole::Lead).max_cost_micros,
            300_000
        );
        // The selected credential is exactly the one stored for this provider.
        let lead_settings = db.provider_settings("opencode-go").await.unwrap();
        assert_eq!(
            factory.decrypt_credential(&lead_settings).unwrap(),
            "lead-credential-1234567890"
        );
        let review_settings = db.provider_settings("opencode").await.unwrap();
        assert_eq!(
            factory.decrypt_credential(&review_settings).unwrap(),
            "review-credential-1234567890"
        );

        let reviewer = factory
            .for_role(
                ExecutionRole::SemanticReviewer,
                &route("opencode", "reviewer-model", 8_192, 1_000_000),
            )
            .await
            .unwrap();
        assert_eq!(reviewer.provider(), "opencode");
        assert_eq!(reviewer.model(), "reviewer-model");
        assert_eq!(reviewer.bound_role(), Some(ModelCallRole::SemanticReviewer));
        assert!(matches!(
            reviewer
                .call(&db, request(ModelCallRole::Lead, "wrong-role"))
                .await,
            Err(ModelGatewayError::Invalid(_))
        ));

        // Evaluation is a real Core model role with no profile row. It uses
        // the same factory and the same provider credential boundary.
        let evaluation = factory
            .for_evaluation(&route("opencode", "reviewer-model", 8_192, 1_000_000))
            .await
            .unwrap();
        evaluation
            .call(&db, request(ModelCallRole::Evaluation, "review-route"))
            .await
            .unwrap();
        assert_eq!(
            review_seen.lock().await.as_slice(),
            ["Bearer review-credential-1234567890"]
        );
        assert!(lead_seen.lock().await.is_empty());

        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn routing_fails_closed_for_each_unsubstantiated_axis() {
        let key = master_key();
        let (path, db) = database().await;
        let (url, _) = upstream().await;
        configure(
            &db,
            &key,
            "opencode-go",
            "deepseek-v4.1-flash",
            &url,
            "lead-credential-1234567890",
            false,
        )
        .await;
        configure(
            &db,
            &key,
            "codex",
            "gpt-6-luna",
            &url,
            "codex-credential-1234567890",
            true,
        )
        .await;
        let factory =
            RoleGatewayFactory::new(&db, Some(&key), caps(), Duration::from_secs(5), true);

        assert!(matches!(
            factory
                .for_role(
                    ExecutionRole::Lead,
                    &route("opencode-go", "deepseek-v4.1-flash", 8_192, 300_000),
                )
                .await,
            Err(RouteError::ProviderDisabled { .. })
        ));
        assert!(matches!(
            factory
                .for_role(
                    ExecutionRole::Lead,
                    &route("codex", "gpt-6-luna", 8_192, 300_000),
                )
                .await,
            Err(RouteError::AdapterUnsupported { .. })
        ));

        let (path_two, db_two) = database().await;
        configure(
            &db_two,
            &key,
            "opencode-go",
            "deepseek-v4.1-flash",
            &url,
            "lead-credential-1234567890",
            true,
        )
        .await;
        let factory_two =
            RoleGatewayFactory::new(&db_two, Some(&key), caps(), Duration::from_secs(5), true);
        assert!(matches!(
            factory_two
                .for_role(
                    ExecutionRole::Lead,
                    &route("opencode-go", "not-advertised", 8_192, 300_000),
                )
                .await,
            Err(RouteError::ModelUnavailable { .. })
        ));
        assert!(matches!(
            factory_two
                .for_role(
                    ExecutionRole::Implementer,
                    &route("opencode-go", "deepseek-v4.1-flash", 8_192, 300_000),
                )
                .await,
            Err(RouteError::NotAGatewayRole { .. })
        ));
        assert!(matches!(
            RoleGatewayFactory::new(&db_two, None, caps(), Duration::from_secs(5), true)
                .for_role(
                    ExecutionRole::Lead,
                    &route("opencode-go", "deepseek-v4.1-flash", 8_192, 300_000),
                )
                .await,
            Err(RouteError::CredentialUnavailable { .. })
        ));
        // A different master key cannot decrypt the stored credential. The
        // route fails closed instead of using another provider or the default.
        let wrong_key = master_key_with("55");
        assert!(matches!(
            RoleGatewayFactory::new(
                &db_two,
                Some(&wrong_key),
                caps(),
                Duration::from_secs(5),
                true
            )
            .for_role(
                ExecutionRole::Lead,
                &route("opencode-go", "deepseek-v4.1-flash", 8_192, 300_000),
            )
            .await,
            Err(RouteError::CredentialUnavailable { .. })
        ));
        db_two.close().await;
        let _ = std::fs::remove_file(path_two);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
