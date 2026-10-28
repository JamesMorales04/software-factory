//! Explicit developer-preview provider catalog, model discovery and
//! configuration validation.
//!
//! The catalog states what is implemented and tested. It never converts a
//! subscription or account claim into a supported route. ChatGPT-authenticated
//! Codex remains blocked in this build.
//!
//! Model discovery asks the configured provider connection for its real model
//! catalog. Core never invents model IDs: an unavailable discovery is reported
//! as `UNAVAILABLE` with a detail, and the in-memory catalog keeps only model
//! IDs the provider actually returned. Model IDs are not secrets; credentials
//! never leave Core and are never returned in a discovery response.

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;

use crate::provider_adapter::{ModelDiscoverySupport, ProviderAdapter};

pub const MAX_DISCOVERED_MODELS: usize = 200;
pub const MAX_DISCOVERY_BODY_BYTES: usize = 512 * 1024;
pub const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogEntry {
    pub provider: &'static str,
    pub kind: &'static str,
    pub display_name: &'static str,
    pub default_model: &'static str,
    pub default_base_url: &'static str,
    pub auth_modes: &'static [&'static str],
    pub chatgpt_oauth: &'static str,
    pub status: &'static str,
    pub note: &'static str,
}

pub const CATALOG: &[ProviderCatalogEntry] = &[
    ProviderCatalogEntry {
        provider: "opencode-go",
        kind: "opencode",
        display_name: "OpenCode Go",
        default_model: "deepseek-v4.1-flash",
        default_base_url: "https://opencode.ai/inference/go/openai/v1",
        auth_modes: &["api_key"],
        chatgpt_oauth: "NOT_APPLICABLE",
        status: "SUPPORTED_TRUSTED_LOCAL",
        note: "OpenCode 1.18.32 adapter. Model discovery is provider-dependent.",
    },
    ProviderCatalogEntry {
        provider: "opencode",
        kind: "opencode",
        display_name: "OpenCode",
        default_model: "deepseek-v4.1-flash",
        default_base_url: "",
        auth_modes: &["api_key"],
        chatgpt_oauth: "NOT_APPLICABLE",
        status: "NOT_PRECONFIGURED",
        note: "Distinct operator-supplied OpenAI-compatible chat-completions connection. This build preselects no URL.",
    },
    ProviderCatalogEntry {
        provider: "codex",
        kind: "codex",
        display_name: "Codex App Server",
        default_model: "gpt-6-luna",
        default_base_url: "https://api.openai.com/v1",
        auth_modes: &["api_key"],
        chatgpt_oauth: "BLOCKED",
        status: "DETERMINISTIC_ONLY",
        note: "API-key transport only. Native ChatGPT OAuth is blocked by the credential boundary.",
    },
];

pub fn entry(provider: &str) -> Option<&'static ProviderCatalogEntry> {
    CATALOG.iter().find(|entry| entry.provider == provider)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderDiscoveryStatus {
    Available,
    Unavailable,
}

/// One truthful discovery outcome for a provider connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelCatalogEntry {
    pub provider: String,
    pub status: ProviderDiscoveryStatus,
    pub models: Vec<String>,
    pub detail: String,
    pub discovered_at: String,
}

/// Process-local model catalog cache. It is a cache, never authority: a model
/// is routable only while the provider connection is enabled, credentialed and
/// the adapter exists. Core does not persist model IDs.
#[derive(Debug, Clone, Default)]
pub struct ProviderModelCache {
    entries: BTreeMap<String, ProviderModelCatalogEntry>,
}

impl ProviderModelCache {
    pub fn record(&mut self, entry: ProviderModelCatalogEntry) {
        self.entries.insert(entry.provider.clone(), entry);
    }

    pub fn entry(&self, provider: &str) -> Option<&ProviderModelCatalogEntry> {
        self.entries.get(provider)
    }

    /// Only an available catalog substantiates model IDs. An unavailable
    /// discovery fails closed and contributes no discovered models.
    pub fn models(&self, provider: &str) -> &[String] {
        match self.entries.get(provider) {
            Some(entry) if entry.status == ProviderDiscoveryStatus::Available => &entry.models,
            _ => &[],
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &ProviderModelCatalogEntry> {
        self.entries.values()
    }
}

/// Query the real provider model endpoint for an OpenAI-compatible connection.
///
/// The request uses the provided credential directly (no proxy), never follows
/// redirects, has a bounded timeout/body and never includes the credential in
/// the returned value or detail.
pub async fn discover_models(
    provider: &str,
    base_url: &str,
    credential: &str,
    allow_loopback_http: bool,
    test_hosts: Vec<String>,
    discovered_at: String,
) -> ProviderModelCatalogEntry {
    let unavailable = |detail: String| ProviderModelCatalogEntry {
        provider: provider.to_owned(),
        status: ProviderDiscoveryStatus::Unavailable,
        models: Vec::new(),
        detail,
        discovered_at: discovered_at.clone(),
    };
    let Some(catalog) = entry(provider) else {
        return unavailable("unknown provider connection".into());
    };
    let Some(adapter) = ProviderAdapter::from_kind(catalog.kind) else {
        return unavailable(format!(
            "provider kind {} has no implemented adapter",
            catalog.kind
        ));
    };
    match adapter.model_discovery() {
        ModelDiscoverySupport::Unsupported(detail) => return unavailable(detail.into()),
        ModelDiscoverySupport::OpenAiModelsEndpoint => {}
    }
    if let Err(error) = validate_base_url(base_url, allow_loopback_http, &test_hosts) {
        return unavailable(format!("provider base URL is not usable: {error}"));
    }
    if credential.len() < 16 {
        return unavailable("provider credential is not usable".into());
    }
    let client = match reqwest::Client::builder()
        .timeout(MODEL_DISCOVERY_TIMEOUT)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => return unavailable("model discovery HTTP client is unavailable".into()),
    };
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = match client.get(&url).bearer_auth(credential).send().await {
        Ok(response) => response,
        Err(error) => {
            return unavailable(bounded_detail(&format!(
                "model discovery request failed: {error}"
            )));
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return unavailable(format!(
            "provider denied the model catalog request (HTTP {status})"
        ));
    }
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        return unavailable(format!(
            "provider does not expose an OpenAI-compatible model catalog (HTTP {status})"
        ));
    }
    if !status.is_success() {
        return unavailable(format!(
            "provider model catalog is unavailable (HTTP {status})"
        ));
    }
    let (body, truncated) = match read_bounded_body(response, MAX_DISCOVERY_BODY_BYTES).await {
        Ok(value) => value,
        Err(_) => return unavailable("provider model catalog body could not be read".into()),
    };
    if truncated {
        return unavailable("provider model catalog exceeded its bounded body limit".into());
    }
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return unavailable("provider model catalog is not JSON".into()),
    };
    let Some(items) = value.get("data").and_then(serde_json::Value::as_array) else {
        return unavailable("provider model catalog has no data array".into());
    };
    let mut models = Vec::new();
    for item in items {
        let Some(model) = item.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if validate_model(model).is_err() {
            continue;
        }
        if !models.iter().any(|seen: &String| seen == model) {
            models.push(model.to_owned());
        }
        if models.len() == MAX_DISCOVERED_MODELS {
            break;
        }
    }
    ProviderModelCatalogEntry {
        provider: provider.to_owned(),
        status: ProviderDiscoveryStatus::Available,
        detail: if models.is_empty() {
            "provider returned an empty model catalog".into()
        } else {
            format!("{} model(s) discovered", models.len())
        },
        models,
        discovered_at,
    }
}

fn bounded_detail(value: &str) -> String {
    value.chars().take(300).collect()
}

async fn read_bounded_body(
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

pub fn validate_model(model: &str) -> Result<(), String> {
    if model.trim().is_empty()
        || model.len() > 128
        || model.trim() != model
        || model.chars().any(char::is_whitespace)
        || model.chars().any(char::is_control)
    {
        return Err("model must be a bounded non-whitespace value".into());
    }
    Ok(())
}

/// Parse the explicit test-only plain-HTTP host allowlist. The caller must have
/// checked `FACTORY_ALLOW_TEST_UPSTREAM=1`; an unset or malformed entry is
/// ignored rather than widening the allowlist.
pub fn parse_test_upstream_hosts(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|host| {
            !host.is_empty()
                && host.len() <= 253
                && host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
        })
        .map(str::to_ascii_lowercase)
        .collect()
}

pub fn test_upstream_host_allowed(
    host: Option<&str>,
    allow_test_http: bool,
    test_hosts: &[String],
) -> bool {
    if !allow_test_http {
        return false;
    }
    let Some(host) = host else {
        return false;
    };
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1")
        || test_hosts.iter().any(|allowed| allowed == &host)
}

pub fn validate_base_url(
    base_url: &str,
    allow_test_http: bool,
    test_hosts: &[String],
) -> Result<(), String> {
    let url = reqwest::Url::parse(base_url).map_err(|_| "base URL is invalid")?;
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err("base URL must be a clean HTTP(S) origin with an optional path".into());
    }
    match url.scheme() {
        "https" => {}
        "http" if test_upstream_host_allowed(url.host_str(), allow_test_http, test_hosts) => {}
        _ => return Err("base URL must use HTTPS".into()),
    }
    if base_url.len() > 512 {
        return Err("base URL is too long".into());
    }
    Ok(())
}

pub fn validate_credential(credential: &str) -> Result<(), String> {
    if credential.len() < 16 || credential.len() > 16 * 1024 || credential.contains('\0') {
        return Err("credential must be between 16 bytes and 16 KiB".into());
    }
    Ok(())
}

pub fn validate_budget(
    max_turns: u32,
    max_input_tokens: u64,
    max_output_tokens: u64,
    max_cost_micros: u64,
) -> Result<(), String> {
    if !(1..=8).contains(&max_turns)
        || !(1..=1_000_000).contains(&max_input_tokens)
        || !(1..=100_000).contains(&max_output_tokens)
        || !(1..=100_000_000).contains(&max_cost_micros)
    {
        return Err("provider budget is outside the supported bound".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_explicit_about_codex_oauth() {
        assert_eq!(entry("codex").unwrap().chatgpt_oauth, "BLOCKED");
        assert_eq!(entry("opencode-go").unwrap().kind, "opencode");
        assert!(entry("unknown").is_none());
    }

    #[test]
    fn urls_and_models_are_strict() {
        assert!(
            validate_base_url("https://opencode.ai/inference/go/openai/v1", false, &[]).is_ok()
        );
        assert!(validate_base_url("http://opencode.ai/v1", false, &[]).is_err());
        assert!(validate_base_url("https://user:pass@opencode.ai/v1", false, &[]).is_err());
        assert!(validate_base_url("http://127.0.0.1:9999/v1", true, &[]).is_ok());
        assert!(validate_model("deepseek-v4.1-flash").is_ok());
        assert!(validate_model("bad model").is_err());
        assert!(validate_model("bad\u{7f}model").is_err());
    }

    #[test]
    fn an_unavailable_catalog_never_substantiates_models() {
        let mut cache = ProviderModelCache::default();
        cache.record(ProviderModelCatalogEntry {
            provider: "opencode-go".into(),
            status: ProviderDiscoveryStatus::Available,
            models: vec!["model-a".into()],
            detail: "ok".into(),
            discovered_at: "now".into(),
        });
        assert_eq!(cache.models("opencode-go"), ["model-a"]);
        cache.record(ProviderModelCatalogEntry {
            provider: "opencode-go".into(),
            status: ProviderDiscoveryStatus::Unavailable,
            models: Vec::new(),
            detail: "HTTP 500".into(),
            discovered_at: "later".into(),
        });
        assert!(cache.models("opencode-go").is_empty());
    }

    #[tokio::test]
    async fn discovery_reads_the_real_catalog_and_never_returns_the_credential() {
        use axum::{Router, routing::get};
        async fn models(headers: axum::http::HeaderMap) -> axum::Json<serde_json::Value> {
            assert_eq!(
                headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer test-credential-1234567890")
            );
            axum::Json(serde_json::json!({
                "data": [
                    {"id": "model-a"},
                    {"id": "model-b"},
                    {"id": "model-a"},
                    {"id": "bad model"},
                    {"id": 7}
                ]
            }))
        }
        let app = Router::new().route("/v1/models", get(models));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let discovery = discover_models(
            "opencode-go",
            &url,
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(discovery.status, ProviderDiscoveryStatus::Available);
        assert_eq!(discovery.models, ["model-a", "model-b"]);
        assert!(!discovery.detail.contains("test-credential"));
        server.abort();
    }

    #[tokio::test]
    async fn discovery_is_typed_unavailable_for_codex_and_denied_credentials() {
        use axum::{Router, routing::get};
        let codex = discover_models(
            "codex",
            "https://api.openai.com/v1",
            "test-credential-1234567890",
            false,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(codex.status, ProviderDiscoveryStatus::Unavailable);
        assert!(codex.detail.contains("App Server"));
        assert!(codex.models.is_empty());

        async fn denied() -> axum::http::StatusCode {
            axum::http::StatusCode::UNAUTHORIZED
        }
        let app = Router::new().route("/v1/models", get(denied));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let denied = discover_models(
            "opencode",
            &url,
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(denied.status, ProviderDiscoveryStatus::Unavailable);
        assert!(denied.detail.contains("HTTP 401"), "{}", denied.detail);
        assert!(denied.models.is_empty());
        server.abort();
    }

    #[test]
    fn test_upstream_allowlist_is_narrow_and_requires_the_flag() {
        let hosts = parse_test_upstream_hosts(Some("model-fixture, other-service"));
        assert_eq!(hosts, vec!["model-fixture", "other-service"]);
        assert!(validate_base_url("http://model-fixture:8080/v1", true, &hosts).is_ok());
        assert!(validate_base_url("http://model-fixture:8080/v1", false, &hosts).is_err());
        assert!(validate_base_url("http://unlisted:8080/v1", true, &hosts).is_err());
        assert!(validate_base_url("http://model-fixture:8080/v1", true, &[]).is_err());
        assert!(validate_base_url("http://127.0.0.1:8080/v1", true, &[]).is_ok());
        assert!(validate_base_url("https://model-fixture:8080/v1", false, &[]).is_ok());
        assert!(parse_test_upstream_hosts(Some("bad host,also bad")).is_empty());
    }
}
