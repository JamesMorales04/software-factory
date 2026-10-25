//! Core-owned authorized model-call client.
//!
//! The Lead, semantic Reviewer and CAP-014 evaluator all use this path. Core
//! reserves a bounded token/cost envelope before the request, refuses ambiguous
//! retries, and settles measured usage after the response. The provider
//! credential never leaves Core. Model output never grants authority.

use std::time::Duration;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::db::{
    Database, ModelCallDecision, ModelCallPolicy, ModelCallResult, ModelCallRole, ModelCallUsage,
    NewModelCallAuthorization,
};

pub const MAX_PROMPT_BYTES: usize = 512 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

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
    pub allow_loopback_http: bool,
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
            let message = response.text().await.unwrap_or_default();
            let detail = bounded(&message, 4096);
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
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|_| ModelGatewayError::Upstream("response is not JSON".into()))?;
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let (usage, usage_known) = parse_usage(&self.config, &value);
        let content = if content.len() > MAX_RESPONSE_BYTES {
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
        if content.is_empty() {
            return Err(ModelGatewayError::Upstream(
                "upstream returned no usable assistant content".into(),
            ));
        }
        Ok(settled)
    }
}

fn validate_config(config: &ModelGatewayConfig) -> Result<(), ModelGatewayError> {
    let url = reqwest::Url::parse(&config.base_url)
        .map_err(|_| ModelGatewayError::Invalid("model upstream URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/v1")
    {
        return Err(ModelGatewayError::Invalid(
            "model upstream must be an origin with an optional /v1 path".into(),
        ));
    }
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "https" && !(config.allow_loopback_http && loopback) {
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
        || config.timeout > Duration::from_secs(300)
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
    let bytes = system_prompt.len().saturating_add(user_prompt.len()) as u64;
    bytes.saturating_add(3) / 4
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
            let cost = reported_cost.unwrap_or_else(|| estimate_cost(config, input, output));
            ((input, output, total, cost), true)
        }
        _ => ((0, 0, 0, 0), false),
    }
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
            lead_policy: policy(32_000, 8_192, 200_000),
            reviewer_policy: policy(64_000, 8_192, 300_000),
            evaluation_policy: policy(200_000, 8_192, 2_000_000),
            allow_loopback_http: true,
        }
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
