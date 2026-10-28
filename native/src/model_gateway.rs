//! Core-owned authorized model-call client.
//!
//! The Lead, semantic Reviewer and CAP-014 evaluator all use this path. Core
//! reserves a bounded token/cost envelope before the request, refuses ambiguous
//! retries, and settles measured usage after the response. The provider
//! credential never leaves Core. Model output never grants authority.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::db::{
    Database, ModelCallDecision, ModelCallPolicy, ModelCallResult, ModelCallRole, ModelCallUsage,
    NewModelCallAuthorization,
};

pub const MAX_PROMPT_BYTES: usize = 512 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_UPSTREAM_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct ModelGatewayConfig {
    pub base_url: String,
    pub credential: String,
    pub provider: String,
    pub model: String,
    pub timeout: Duration,
    pub input_micros_per_million: u64,
    pub output_micros_per_million: u64,
    pub lead_policy: ModelCallPolicy,
    pub reviewer_policy: ModelCallPolicy,
    pub evaluation_policy: ModelCallPolicy,
    pub global_policy: ModelCallPolicy,
    pub allow_loopback_http: bool,
    /// Explicit test-only plain-HTTP hosts from
    /// `FACTORY_TEST_UPSTREAM_HOSTS`. Ignored unless
    /// `allow_loopback_http`/`FACTORY_ALLOW_TEST_UPSTREAM=1` is set.
    pub test_upstream_hosts: Vec<String>,
}

impl std::fmt::Debug for ModelGatewayConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelGatewayConfig")
            .field("base_url", &self.base_url)
            .field("credential", &"<redacted>")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Clone)]
pub struct ModelGateway {
    config: ModelGatewayConfig,
    client: reqwest::Client,
}

impl std::fmt::Debug for ModelGateway {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelGateway")
            .field("config", &self.config)
            .finish()
    }
}

pub struct ModelCallRequest<'a> {
    pub role: ModelCallRole,
    pub principal_id: &'a str,
    pub project_id: &'a str,
    pub objective_id: &'a str,
    pub scope_id: &'a str,
    pub purpose: &'a str,
    pub system_prompt: &'a str,
    pub user_prompt: &'a str,
    pub max_output_tokens: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelGatewayError {
    #[error("model gateway configuration is invalid: {0}")]
    Invalid(String),
    #[error("model call database error: {0}")]
    Database(#[from] crate::db::Error),
    #[error("model upstream failed: {0}")]
    Upstream(String),
    #[error("model upstream outcome is ambiguous: {0}")]
    Ambiguous(String),
    #[error("model usage is unknown: {0}")]
    Usage(String),
}

impl ModelGateway {
    pub fn new(config: ModelGatewayConfig) -> Result<Self, ModelGatewayError> {
        validate_config(&config)?;
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ModelGatewayError::Invalid("model HTTP client".into()))?;
        Ok(Self { config, client })
    }

    pub fn provider(&self) -> &str {
        &self.config.provider
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }

    pub async fn call(
        &self,
        database: &Database,
        request: ModelCallRequest<'_>,
    ) -> Result<ModelCallResult, ModelGatewayError> {
        validate_request(&request)?;
        let request_sha256 = request_digest(&self.config, &request);
        let estimated_input_tokens = estimate_tokens(request.system_prompt, request.user_prompt);
        let estimated_cost_micros = estimate_cost(
            &self.config,
            estimated_input_tokens,
            request.max_output_tokens,
        );
        let policy = match request.role {
            ModelCallRole::Lead => self.config.lead_policy,
            ModelCallRole::SemanticReviewer => self.config.reviewer_policy,
            ModelCallRole::Evaluation => self.config.evaluation_policy,
        };
        let decision = database
            .authorize_model_call(&NewModelCallAuthorization {
                role: request.role,
                principal_id: request.principal_id.into(),
                project_id: request.project_id.into(),
                objective_id: request.objective_id.into(),
                scope_id: request.scope_id.into(),
                provider: self.config.provider.clone(),
                model: self.config.model.clone(),
                purpose: request.purpose.into(),
                request_sha256,
                estimated_input_tokens,
                requested_output_tokens: request.max_output_tokens,
                estimated_cost_micros,
                policy,
                global_policy: Some(self.config.global_policy),
            })
            .await?;
        let grant = match decision {
            ModelCallDecision::Replayed(result) => return Ok(result),
            ModelCallDecision::Reserved(grant) => grant,
        };
        let body = json!({
            "model": self.config.model,
            "temperature": 0,
            "max_tokens": grant.max_output_tokens,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": request.system_prompt},
                {"role": "user", "content": request.user_prompt}
            ]
        });
        let response = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.config.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.config.credential)
            .header(
                "x-opencode-session",
                format!("factory-model-{}", grant.reservation_id),
            )
            .json(&body)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                if error.is_connect() {
                    let _ = database
                        .release_model_call(
                            &grant.reservation_id,
                            "model upstream connection failed before a request was sent",
                            false,
                        )
                        .await;
                    return Err(ModelGatewayError::Upstream(
                        "connection failed before request".into(),
                    ));
                }
                let _ = database
                    .release_model_call(
                        &grant.reservation_id,
                        "model upstream transport outcome is ambiguous",
                        true,
                    )
                    .await;
                return Err(ModelGatewayError::Ambiguous(
                    "upstream transport outcome".into(),
                ));
            }
        };
        let status = response.status();
        if !status.is_success() {
            let (message, _) = read_body(response, 4096).await.unwrap_or_default();
            let detail = bounded(
                &crate::telemetry::redact(&String::from_utf8_lossy(&message)),
                4096,
            );
            if status.is_client_error() {
                let _ = database
                    .release_model_call(
                        &grant.reservation_id,
                        &format!("model upstream refused the request with {status}"),
                        false,
                    )
                    .await;
                return Err(ModelGatewayError::Upstream(format!(
                    "HTTP {status}: {detail}"
                )));
            }
            let _ = database
                .release_model_call(
                    &grant.reservation_id,
                    &format!("model upstream returned ambiguous {status}"),
                    true,
                )
                .await;
            return Err(ModelGatewayError::Ambiguous(format!("HTTP {status}")));
        }
        let (body, truncated) = match read_body(response, MAX_UPSTREAM_BYTES).await {
            Ok(value) => value,
            Err(_) => {
                let _ = database
                    .release_model_call(
                        &grant.reservation_id,
                        "model upstream response stream failed",
                        true,
                    )
                    .await;
                return Err(ModelGatewayError::Ambiguous(
                    "upstream response stream failed".into(),
                ));
            }
        };
        if truncated {
            let _ = database
                .release_model_call(
                    &grant.reservation_id,
                    "model upstream response exceeded the bounded body limit",
                    true,
                )
                .await;
            return Err(ModelGatewayError::Ambiguous(
                "upstream response exceeded its body limit".into(),
            ));
        }
        let value: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                let _ = database
                    .release_model_call(
                        &grant.reservation_id,
                        "model upstream response is not JSON",
                        true,
                    )
                    .await;
                return Err(ModelGatewayError::Ambiguous(
                    "upstream response is not JSON".into(),
                ));
            }
        };
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let (usage, usage_known) = parse_usage(&self.config, &value);
        let content = if content.is_empty() || content.len() > MAX_RESPONSE_BYTES {
            String::new()
        } else {
            content.to_owned()
        };
        let settled = database
            .settle_model_call(&ModelCallUsage {
                reservation_id: grant.reservation_id.clone(),
                input_tokens: usage.0,
                output_tokens: usage.1,
                total_tokens: usage.2,
                cost_micros: usage.3,
                usage_known,
                content: content.clone(),
            })
            .await?;
        if !usage_known {
            return Err(ModelGatewayError::Usage(
                "upstream did not report measurable usage".into(),
            ));
        }
        Ok(settled)
    }
}

fn valid_base_path(path: &str) -> bool {
    matches!(path, "" | "/")
        || (path.starts_with('/')
            && path.len() <= 200
            && !path.ends_with('/')
            && !path.contains("//")
            && !path.contains("..")
            && path
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.')))
}

fn validate_config(config: &ModelGatewayConfig) -> Result<(), ModelGatewayError> {
    let url = reqwest::Url::parse(&config.base_url)
        .map_err(|_| ModelGatewayError::Invalid("model upstream URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !valid_base_path(url.path())
    {
        return Err(ModelGatewayError::Invalid(
            "model upstream must be an origin with a bounded fixed base path".into(),
        ));
    }
    if url.scheme() != "https"
        && !crate::provider_catalog::test_upstream_host_allowed(
            url.host_str(),
            config.allow_loopback_http,
            &config.test_upstream_hosts,
        )
    {
        return Err(ModelGatewayError::Invalid(
            "model upstream must use HTTPS outside loopback tests".into(),
        ));
    }
    if config.credential.len() < 16
        || config.provider.trim().is_empty()
        || config.model.trim().is_empty()
        || config.input_micros_per_million == 0
        || config.output_micros_per_million == 0
        || config.timeout.is_zero()
        || config.timeout > Duration::from_secs(90)
    {
        return Err(ModelGatewayError::Invalid(
            "provider, model, credential, rates or timeout".into(),
        ));
    }
    for policy in [
        config.lead_policy,
        config.reviewer_policy,
        config.evaluation_policy,
    ] {
        if policy.max_input_tokens == 0
            || policy.max_output_tokens == 0
            || policy.max_cost_micros == 0
        {
            return Err(ModelGatewayError::Invalid("model call policy".into()));
        }
    }
    Ok(())
}

fn validate_request(request: &ModelCallRequest<'_>) -> Result<(), ModelGatewayError> {
    if request.principal_id.trim().is_empty()
        || request.project_id.trim().is_empty()
        || request.objective_id.trim().is_empty()
        || request.scope_id.trim().is_empty()
        || request.purpose.trim().is_empty()
        || request.max_output_tokens == 0
        || request.max_output_tokens > 8192
    {
        return Err(ModelGatewayError::Invalid("model call request".into()));
    }
    if request.system_prompt.is_empty()
        || request.user_prompt.is_empty()
        || request
            .system_prompt
            .len()
            .saturating_add(request.user_prompt.len())
            > MAX_PROMPT_BYTES
    {
        return Err(ModelGatewayError::Invalid("model prompt".into()));
    }
    Ok(())
}

fn request_digest(config: &ModelGatewayConfig, request: &ModelCallRequest<'_>) -> String {
    let canonical = serde_json::to_vec(&json!({
        "role": request.role,
        "principal": request.principal_id,
        "project": request.project_id,
        "objective": request.objective_id,
        "scope": request.scope_id,
        "purpose": request.purpose,
        "provider": config.provider,
        "model": config.model,
        "maxOutputTokens": request.max_output_tokens,
        "system": request.system_prompt,
        "user": request.user_prompt
    }))
    .expect("model call digest is serializable");
    Sha256::digest(canonical)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn estimate_tokens(system_prompt: &str, user_prompt: &str) -> u64 {
    // UTF-8 tokenizers cannot emit more tokens than input bytes, and a model
    // call also has a small fixed conversation framing overhead. Reserving up
    // to the byte count is conservative but prevents a measured input from
    // exceeding its pre-request reservation.
    let bytes = system_prompt.len().saturating_add(user_prompt.len()) as u64;
    bytes.max(64)
}

fn estimate_cost(config: &ModelGatewayConfig, input: u64, output: u64) -> u64 {
    let input_cost =
        (u128::from(input) * u128::from(config.input_micros_per_million)).div_ceil(1_000_000);
    let output_cost =
        (u128::from(output) * u128::from(config.output_micros_per_million)).div_ceil(1_000_000);
    u64::try_from(input_cost.saturating_add(output_cost))
        .unwrap_or(u64::MAX)
        .max(1)
}

fn parse_usage(
    config: &ModelGatewayConfig,
    value: &serde_json::Value,
) -> ((u64, u64, u64, u64), bool) {
    let usage = value.get("usage");
    let input = usage
        .and_then(|usage| {
            usage
                .get("prompt_tokens")
                .or_else(|| usage.get("input_tokens"))
        })
        .and_then(serde_json::Value::as_u64);
    let output = usage
        .and_then(|usage| {
            usage
                .get("completion_tokens")
                .or_else(|| usage.get("output_tokens"))
        })
        .and_then(serde_json::Value::as_u64);
    let total = usage
        .and_then(|usage| usage.get("total_tokens"))
        .and_then(serde_json::Value::as_u64);
    let reported_cost = usage
        .and_then(|usage| usage.get("cost_micros"))
        .and_then(serde_json::Value::as_u64);
    match (input, output) {
        (Some(input), Some(output)) => {
            let total = total.unwrap_or_else(|| input.saturating_add(output));
            let local = estimate_cost(config, input, output);
            let cost = reported_cost.unwrap_or(0).max(local);
            ((input, output, total, cost), true)
        }
        _ => ((0, 0, 0, 0), false),
    }
}

async fn read_body(
    response: reqwest::Response,
    limit: usize,
) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len() >= limit {
            return Ok((body, true));
        }
        let take = limit.saturating_sub(body.len()).min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            return Ok((body, true));
        }
    }
    Ok((body, false))
}

fn bounded(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
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

    fn config(url: &str) -> ModelGatewayConfig {
        ModelGatewayConfig {
            base_url: url.into(),
            credential: "test-credential-1234567890".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            timeout: Duration::from_secs(10),
            input_micros_per_million: 150_000,
            output_micros_per_million: 600_000,
            lead_policy: policy(128_000, 8_192, 300_000),
            reviewer_policy: policy(400_000, 8_192, 1_000_000),
            evaluation_policy: policy(200_000, 8_192, 2_000_000),
            global_policy: policy(2_000_000, 100_000, 10_000_000),
            allow_loopback_http: true,
            test_upstream_hosts: Vec::new(),
        }
    }

    #[test]
    fn accepts_only_explicitly_allowed_test_upstream_hosts() {
        let mut allowed = config("http://model-fixture:8080/v1");
        allowed.test_upstream_hosts = vec!["model-fixture".into()];
        assert!(ModelGateway::new(allowed).is_ok());

        let mut other = config("http://other-service:8080/v1");
        other.test_upstream_hosts = vec!["model-fixture".into()];
        assert!(ModelGateway::new(other).is_err());

        let mut disabled = config("http://model-fixture:8080/v1");
        disabled.allow_loopback_http = false;
        disabled.test_upstream_hosts = vec!["model-fixture".into()];
        assert!(ModelGateway::new(disabled).is_err());
    }

    #[test]
    fn rejects_non_loopback_plain_http_and_unbounded_prompts() {
        let gateway = ModelGateway::new(config("http://example.com/v1"));
        assert!(gateway.is_err());
        let gateway = ModelGateway::new(config("http://127.0.0.1:9000/v1")).unwrap();
        let request = ModelCallRequest {
            role: ModelCallRole::Evaluation,
            principal_id: "operator",
            project_id: "evaluation",
            objective_id: "eval",
            scope_id: "eval",
            purpose: "evaluation",
            system_prompt: "",
            user_prompt: "prompt",
            max_output_tokens: 10,
        };
        assert!(validate_request(&request).is_err());
        assert!(!gateway.config.credential.is_empty());
    }

    #[test]
    fn estimates_cost_with_ceiling_arithmetic() {
        let gateway = ModelGateway::new(config("http://127.0.0.1:9000/v1")).unwrap();
        assert_eq!(estimate_cost(&gateway.config, 1_000_000, 0), 150_000);
        assert_eq!(estimate_cost(&gateway.config, 0, 1_000_000), 600_000);
        assert_eq!(estimate_cost(&gateway.config, 1, 1), 2);
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Clone, Copy)]
    enum Behavior {
        Success,
        NoUsage,
        Empty,
        Oversized,
        Timeout,
        Reject,
    }

    #[derive(Clone)]
    struct Upstream {
        behavior: Behavior,
        calls: Arc<Mutex<usize>>,
    }

    async fn chat(
        axum::extract::State(state): axum::extract::State<Upstream>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        *state.calls.lock().await += 1;
        match state.behavior {
            Behavior::Success => axum::Json(json!({
                "choices":[{"message":{"content":"{\"ok\":true}"}}],
                "usage":{"prompt_tokens":12,"completion_tokens":7,"total_tokens":19,"cost_micros":11}
            }))
            .into_response(),
            Behavior::NoUsage => axum::Json(json!({
                "choices":[{"message":{"content":"{\"ok\":true}"}}]
            }))
            .into_response(),
            Behavior::Empty => axum::Json(json!({
                "choices":[{"message":{"content":""}}],
                "usage":{"prompt_tokens":12,"completion_tokens":0,"total_tokens":12}
            }))
            .into_response(),
            Behavior::Oversized => axum::Json(json!({
                "choices":[{"message":{"content":"x".repeat(MAX_RESPONSE_BYTES + 1)}}],
                "usage":{"prompt_tokens":12,"completion_tokens":7,"total_tokens":19}
            }))
            .into_response(),
            Behavior::Timeout => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                axum::Json(json!({"choices":[{"message":{"content":"late"}}]})).into_response()
            }
            Behavior::Reject => (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(json!({"error":"invalid request"})),
            )
                .into_response(),
        }
    }

    async fn upstream(
        behavior: Behavior,
    ) -> (String, Arc<Mutex<usize>>, tokio::task::JoinHandle<()>) {
        use axum::{Router, routing::post};
        let calls = Arc::new(Mutex::new(0));
        let app = Router::new()
            .route("/chat/completions", post(chat))
            .with_state(Upstream {
                behavior,
                calls: calls.clone(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, calls, server)
    }

    fn gateway(url: &str, timeout_ms: u64) -> ModelGateway {
        ModelGateway::new(ModelGatewayConfig {
            base_url: url.into(),
            credential: "test-credential-1234567890".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            timeout: Duration::from_millis(timeout_ms),
            input_micros_per_million: 150_000,
            output_micros_per_million: 600_000,
            lead_policy: ModelCallPolicy {
                max_input_tokens: 128_000,
                max_output_tokens: 8_192,
                max_cost_micros: 300_000,
            },
            reviewer_policy: ModelCallPolicy {
                max_input_tokens: 400_000,
                max_output_tokens: 8_192,
                max_cost_micros: 1_000_000,
            },
            evaluation_policy: ModelCallPolicy {
                max_input_tokens: 200_000,
                max_output_tokens: 8_192,
                max_cost_micros: 2_000_000,
            },
            global_policy: ModelCallPolicy {
                max_input_tokens: 2_000_000,
                max_output_tokens: 100_000,
                max_cost_micros: 10_000_000,
            },
            allow_loopback_http: true,
            test_upstream_hosts: Vec::new(),
        })
        .unwrap()
    }

    fn request<'a>(scope: &'a str, prompt: &'a str) -> ModelCallRequest<'a> {
        ModelCallRequest {
            role: ModelCallRole::Evaluation,
            principal_id: "evaluator",
            project_id: "evaluation",
            objective_id: scope,
            scope_id: scope,
            purpose: "evaluation",
            system_prompt: "Return JSON.",
            user_prompt: prompt,
            max_output_tokens: 100,
        }
    }

    async fn database() -> (std::path::PathBuf, Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-gateway-{}-{}.sqlite",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let database = Database::open(&path).await.unwrap();
        (path, database)
    }

    #[tokio::test]
    async fn success_settles_and_replays_without_a_second_upstream_call() {
        let (path, database) = database().await;
        let (url, calls, server) = upstream(Behavior::Success).await;
        let gateway = gateway(&url, 5_000);
        let first = gateway
            .call(&database, request("gateway-success", "prompt"))
            .await
            .unwrap();
        assert_eq!(first.content, "{\"ok\":true}");
        assert_eq!(first.total_tokens, 19);
        assert_eq!(*calls.lock().await, 1);
        let second = gateway
            .call(&database, request("gateway-success", "prompt"))
            .await
            .unwrap();
        assert_eq!(second.reservation_id, first.reservation_id);
        assert_eq!(*calls.lock().await, 1);
        database.close().await;
        let _ = std::fs::remove_file(path);
        server.abort();
    }

    #[tokio::test]
    async fn missing_usage_marks_the_call_unknown_and_blocks_retry() {
        let (path, database) = database().await;
        let (url, calls, server) = upstream(Behavior::NoUsage).await;
        let gateway = gateway(&url, 5_000);
        assert!(
            gateway
                .call(&database, request("gateway-unknown", "prompt"))
                .await
                .is_err()
        );
        let records = database
            .model_calls_for_scope("gateway-unknown")
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, "UNKNOWN");
        assert!(
            gateway
                .call(&database, request("gateway-unknown", "prompt"))
                .await
                .is_err()
        );
        assert_eq!(*calls.lock().await, 1);
        database.close().await;
        let _ = std::fs::remove_file(path);
        server.abort();
    }

    #[tokio::test]
    async fn empty_and_oversized_content_never_settle_as_success() {
        for (scope, behavior) in [
            ("gateway-empty", Behavior::Empty),
            ("gateway-oversized", Behavior::Oversized),
        ] {
            let (path, database) = database().await;
            let (url, _, server) = upstream(behavior).await;
            let gateway = gateway(&url, 5_000);
            assert!(
                gateway
                    .call(&database, request(scope, "prompt"))
                    .await
                    .is_err()
            );
            let records = database.model_calls_for_scope(scope).await.unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].state, "UNKNOWN");
            assert!(
                gateway
                    .call(&database, request(scope, "prompt"))
                    .await
                    .is_err()
            );
            database.close().await;
            let _ = std::fs::remove_file(path);
            server.abort();
        }
    }

    #[tokio::test]
    async fn timeout_is_ambiguous_and_never_retries_automatically() {
        let (path, database) = database().await;
        let (url, calls, server) = upstream(Behavior::Timeout).await;
        let gateway = gateway(&url, 100);
        assert!(matches!(
            gateway
                .call(&database, request("gateway-timeout", "prompt"))
                .await,
            Err(ModelGatewayError::Ambiguous(_))
        ));
        let records = database
            .model_calls_for_scope("gateway-timeout")
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, "AMBIGUOUS");
        assert!(
            gateway
                .call(&database, request("gateway-timeout", "prompt"))
                .await
                .is_err()
        );
        assert_eq!(*calls.lock().await, 1);
        database.close().await;
        let _ = std::fs::remove_file(path);
        server.abort();
    }

    #[tokio::test]
    async fn client_error_releases_the_reservation() {
        let (path, database) = database().await;
        let (url, _, server) = upstream(Behavior::Reject).await;
        let gateway = gateway(&url, 5_000);
        assert!(
            gateway
                .call(&database, request("gateway-rejected", "prompt"))
                .await
                .is_err()
        );
        let records = database
            .model_calls_for_scope("gateway-rejected")
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, "RELEASED");
        database.close().await;
        let _ = std::fs::remove_file(path);
        server.abort();
    }
}
