//! Explicit developer-preview provider catalog, model discovery and
//! configuration validation.
//!
//! The catalog states what is implemented and tested. It never converts a
//! subscription or account claim into a supported route. ChatGPT-authenticated
//! Codex remains blocked in this build.
//!
//! Model discovery records three separate facts and never collapses them:
//!
//! - the catalog itself (available, absent, rejected or unreadable);
//! - the stored credential's presence (owned by provider settings) and, only
//!   when the provider explicitly rejects a credentialed request, a rejection
//!   signal from that request;
//! - successful inference, which Core does not perform or infer during
//!   discovery.
//!
//! A provider's inference base URL is not assumed to publish `GET /models`.
//! OpenCode Go serves inference from `/inference/go/openai/v1` but publishes its
//! catalog at the sibling `/inference/go/v1/models`. The catalog URL is derived
//! per connection and recorded on every discovery result. Core never invents
//! model IDs: an unavailable discovery contributes no routable models, and a
//! previous successful catalog is retained only as display provenance.

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use thiserror::Error;

use crate::provider_adapter::ProviderAdapter;

pub const MAX_DISCOVERED_MODELS: usize = 200;
pub const MAX_DISCOVERY_BODY_BYTES: usize = 512 * 1024;
pub const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// How Core locates a connection's model catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelCatalogStrategy {
    /// `GET {base_url}/models`, the standard OpenAI-compatible catalog.
    ProviderBaseUrl,
    /// OpenCode Go publishes its catalog at the sibling of its inference base:
    /// `.../inference/go/openai/v1` -> `.../inference/go/v1/models`.
    OpenCodeGoSibling,
    /// The connection has no supported catalog transport.
    Unsupported,
}

/// Provenance of a catalog observation, returned to Office.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProviderCatalogSource {
    #[serde(rename = "OPENCODE_GO_CATALOG_URL")]
    OpenCodeGoCatalogUrl,
    #[serde(rename = "OPENAI_MODELS_ENDPOINT")]
    OpenAiModelsEndpoint,
    #[serde(rename = "UNSUPPORTED")]
    Unsupported,
}

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
    pub catalog: ModelCatalogStrategy,
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
        note: "OpenCode adapter. Inference uses /openai/v1; the model catalog is published at the sibling /v1/models path.",
        catalog: ModelCatalogStrategy::OpenCodeGoSibling,
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
        catalog: ModelCatalogStrategy::ProviderBaseUrl,
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
        catalog: ModelCatalogStrategy::Unsupported,
    },
];

pub fn entry(provider: &str) -> Option<&'static ProviderCatalogEntry> {
    CATALOG.iter().find(|entry| entry.provider == provider)
}

/// A resolved catalog request. `requires_credential` is false for a public
/// provider-specific catalog; it is true for the generic OpenAI-compatible
/// endpoint, which Core calls with the stored credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEndpoint {
    pub url: String,
    pub source: ProviderCatalogSource,
    pub requires_credential: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogEndpointError {
    #[error("unknown provider connection")]
    UnknownProvider,
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("OpenCode Go model catalog requires an inference base URL ending in /openai/v1")]
    UnexpectedBaseUrl,
}

/// Resolve the catalog URL for one connection without performing I/O.
///
/// `opencode-go` never falls back to `{base_url}/models`: the verified inference
/// base returns HTTP 404 for that path. A base URL that does not match the
/// documented `/openai/v1` shape fails closed rather than guessing.
pub fn catalog_endpoint(
    provider: &str,
    base_url: &str,
) -> Result<ModelCatalogEndpoint, CatalogEndpointError> {
    let catalog = entry(provider).ok_or(CatalogEndpointError::UnknownProvider)?;
    let base_url = base_url.trim_end_matches('/');
    match catalog.catalog {
        ModelCatalogStrategy::ProviderBaseUrl => Ok(ModelCatalogEndpoint {
            url: format!("{base_url}/models"),
            source: ProviderCatalogSource::OpenAiModelsEndpoint,
            requires_credential: true,
        }),
        ModelCatalogStrategy::OpenCodeGoSibling => {
            let prefix = base_url
                .strip_suffix("/openai/v1")
                .ok_or(CatalogEndpointError::UnexpectedBaseUrl)?;
            Ok(ModelCatalogEndpoint {
                url: format!("{prefix}/v1/models"),
                source: ProviderCatalogSource::OpenCodeGoCatalogUrl,
                requires_credential: false,
            })
        }
        ModelCatalogStrategy::Unsupported => Err(CatalogEndpointError::Unsupported(
            "the Codex App Server adapter exposes no model catalog",
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderDiscoveryStatus {
    Available,
    /// The provider answered but publishes no catalog at the requested URL.
    /// This says nothing about credential validity.
    EndpointUnsupported,
    /// The credentialed catalog request was explicitly rejected.
    CredentialRejected,
    /// Transport, timeout or provider-server failure.
    Unreachable,
    /// The provider answered with a malformed or oversized body.
    InvalidResponse,
    /// No catalog transport exists for this connection.
    Unsupported,
}

impl ProviderDiscoveryStatus {
    pub fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// What a catalog request observed about the credential. Only an explicit
/// rejection by a request that carried the credential sets `Rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderCredentialSignal {
    NotObserved,
    /// A catalog requires a credential but none was usable locally.
    NotProvided,
    Rejected,
}

/// A previously successful catalog observation. It is display provenance only:
/// [`ProviderModelCache::models`] never returns it as routable while the current
/// observation is unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogSnapshot {
    pub source: ProviderCatalogSource,
    pub models: Vec<String>,
    pub observed_at: String,
}

/// One truthful discovery outcome for a provider connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelCatalogEntry {
    pub provider: String,
    pub status: ProviderDiscoveryStatus,
    pub source: ProviderCatalogSource,
    pub catalog_url: Option<String>,
    pub models: Vec<String>,
    pub detail: String,
    pub discovered_at: String,
    pub credential_signal: ProviderCredentialSignal,
    /// Last successful catalog in this Core process, kept for display when the
    /// current refresh fails. It never substantiates routing.
    pub last_known: Option<ProviderCatalogSnapshot>,
}

/// Process-local model catalog cache. It is a cache, never authority: a model
/// is routable only while the provider connection is enabled, credentialed and
/// the current discovery is available. Core does not persist discovery across
/// restarts.
#[derive(Debug, Clone, Default)]
pub struct ProviderModelCache {
    entries: BTreeMap<String, ProviderModelCatalogEntry>,
}

impl ProviderModelCache {
    pub fn record(&mut self, mut entry: ProviderModelCatalogEntry) {
        if !entry.status.is_available() {
            entry.last_known = self
                .entries
                .get(&entry.provider)
                .and_then(ProviderModelCatalogEntry::last_known_snapshot);
        }
        self.entries.insert(entry.provider.clone(), entry);
    }

    pub fn entry(&self, provider: &str) -> Option<&ProviderModelCatalogEntry> {
        self.entries.get(provider)
    }

    /// Only an available catalog substantiates model IDs. A failed refresh fails
    /// closed even when `last_known` preserves earlier provenance.
    pub fn models(&self, provider: &str) -> &[String] {
        match self.entries.get(provider) {
            Some(entry) if entry.status.is_available() => &entry.models,
            _ => &[],
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &ProviderModelCatalogEntry> {
        self.entries.values()
    }
}

impl ProviderModelCatalogEntry {
    fn last_known_snapshot(&self) -> Option<ProviderCatalogSnapshot> {
        if self.status.is_available() {
            Some(ProviderCatalogSnapshot {
                source: self.source,
                models: self.models.clone(),
                observed_at: self.discovered_at.clone(),
            })
        } else {
            self.last_known.clone()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn failure(
        provider: &str,
        status: ProviderDiscoveryStatus,
        source: ProviderCatalogSource,
        catalog_url: Option<String>,
        detail: String,
        credential_signal: ProviderCredentialSignal,
        discovered_at: String,
    ) -> Self {
        Self {
            provider: provider.to_owned(),
            status,
            source,
            catalog_url,
            models: Vec::new(),
            detail: bounded_detail(&detail),
            discovered_at,
            credential_signal,
            last_known: None,
        }
    }
}

/// Read one provider model catalog.
///
/// The request is bounded by [`MODEL_DISCOVERY_TIMEOUT`] and
/// [`MAX_DISCOVERY_BODY_BYTES`], never follows redirects and never includes the
/// credential in the returned value or detail. A provider-specific public
/// catalog is fetched without sending the credential; the generic
/// `{base_url}/models` endpoint is fetched with it.
pub async fn discover_models(
    provider: &str,
    base_url: &str,
    credential: &str,
    allow_loopback_http: bool,
    test_hosts: Vec<String>,
    discovered_at: String,
) -> ProviderModelCatalogEntry {
    let Some(catalog) = entry(provider) else {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unsupported,
            ProviderCatalogSource::Unsupported,
            None,
            "unknown provider connection".into(),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    };
    let Some(adapter) = ProviderAdapter::from_kind(catalog.kind) else {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unsupported,
            ProviderCatalogSource::Unsupported,
            None,
            format!("provider kind {} has no implemented adapter", catalog.kind),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    };
    if adapter == ProviderAdapter::CodexAppServer {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unsupported,
            ProviderCatalogSource::Unsupported,
            None,
            "the Codex App Server adapter exposes no model catalog".into(),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    }
    let endpoint = match catalog_endpoint(provider, base_url) {
        Ok(endpoint) => endpoint,
        Err(CatalogEndpointError::Unsupported(detail)) => {
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::Unsupported,
                ProviderCatalogSource::Unsupported,
                None,
                detail.into(),
                ProviderCredentialSignal::NotObserved,
                discovered_at,
            );
        }
        Err(error) => {
            let source = match catalog.catalog {
                ModelCatalogStrategy::OpenCodeGoSibling => {
                    ProviderCatalogSource::OpenCodeGoCatalogUrl
                }
                _ => ProviderCatalogSource::OpenAiModelsEndpoint,
            };
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::Unreachable,
                source,
                None,
                error.to_string(),
                ProviderCredentialSignal::NotObserved,
                discovered_at,
            );
        }
    };
    if let Err(error) = validate_base_url(&endpoint.url, allow_loopback_http, &test_hosts) {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unreachable,
            endpoint.source,
            Some(endpoint.url),
            format!("provider catalog URL is not usable: {error}"),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    }
    if endpoint.requires_credential && credential.len() < 16 {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unreachable,
            endpoint.source,
            Some(endpoint.url),
            "a stored provider credential is required to read this catalog".into(),
            ProviderCredentialSignal::NotProvided,
            discovered_at,
        );
    }
    let client = match reqwest::Client::builder()
        .timeout(MODEL_DISCOVERY_TIMEOUT)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::Unreachable,
                endpoint.source,
                Some(endpoint.url),
                "model discovery HTTP client is unavailable".into(),
                ProviderCredentialSignal::NotObserved,
                discovered_at,
            );
        }
    };
    let mut request = client.get(&endpoint.url);
    if endpoint.requires_credential {
        request = request.bearer_auth(credential);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::Unreachable,
                endpoint.source,
                Some(endpoint.url),
                format!("model discovery request failed: {error}"),
                ProviderCredentialSignal::NotObserved,
                discovered_at,
            );
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        if endpoint.requires_credential {
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::CredentialRejected,
                endpoint.source,
                Some(endpoint.url),
                format!(
                    "provider rejected the model catalog request (HTTP {status}); this is a credential or permission signal for the catalog endpoint, not proof about inference"
                ),
                ProviderCredentialSignal::Rejected,
                discovered_at,
            );
        }
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unreachable,
            endpoint.source,
            Some(endpoint.url),
            format!(
                "public model catalog requires authorization (HTTP {status}); Core did not send the provider credential"
            ),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    }
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::EndpointUnsupported,
            endpoint.source,
            Some(endpoint.url),
            format!(
                "provider does not expose a model catalog at the configured catalog URL (HTTP {status}); the endpoint is absent and this response does not evaluate the credential"
            ),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    }
    if !status.is_success() {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::Unreachable,
            endpoint.source,
            Some(endpoint.url),
            format!("provider model catalog is unavailable (HTTP {status})"),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    }
    let (body, truncated) = match read_bounded_body(response, MAX_DISCOVERY_BODY_BYTES).await {
        Ok(value) => value,
        Err(_) => {
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::InvalidResponse,
                endpoint.source,
                Some(endpoint.url),
                "provider model catalog body could not be read".into(),
                ProviderCredentialSignal::NotObserved,
                discovered_at,
            );
        }
    };
    if truncated {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::InvalidResponse,
            endpoint.source,
            Some(endpoint.url),
            "provider model catalog exceeded its bounded body limit".into(),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
    }
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return ProviderModelCatalogEntry::failure(
                provider,
                ProviderDiscoveryStatus::InvalidResponse,
                endpoint.source,
                Some(endpoint.url),
                "provider model catalog is not valid JSON".into(),
                ProviderCredentialSignal::NotObserved,
                discovered_at,
            );
        }
    };
    let Some(items) = value.get("data").and_then(serde_json::Value::as_array) else {
        return ProviderModelCatalogEntry::failure(
            provider,
            ProviderDiscoveryStatus::InvalidResponse,
            endpoint.source,
            Some(endpoint.url),
            "provider model catalog has no data array".into(),
            ProviderCredentialSignal::NotObserved,
            discovered_at,
        );
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
        source: endpoint.source,
        catalog_url: Some(endpoint.url),
        detail: if models.is_empty() {
            "provider returned an empty model catalog".into()
        } else {
            format!("{} model(s) discovered", models.len())
        },
        models,
        discovered_at,
        credential_signal: ProviderCredentialSignal::NotObserved,
        last_known: None,
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

    fn available(models: &[&str], discovered_at: &str) -> ProviderModelCatalogEntry {
        ProviderModelCatalogEntry {
            provider: "opencode-go".into(),
            status: ProviderDiscoveryStatus::Available,
            source: ProviderCatalogSource::OpenCodeGoCatalogUrl,
            catalog_url: Some("https://opencode.ai/inference/go/v1/models".into()),
            models: models.iter().map(|model| (*model).to_owned()).collect(),
            detail: "ok".into(),
            discovered_at: discovered_at.into(),
            credential_signal: ProviderCredentialSignal::NotObserved,
            last_known: None,
        }
    }

    fn unavailable(
        status: ProviderDiscoveryStatus,
        detail: &str,
        discovered_at: &str,
    ) -> ProviderModelCatalogEntry {
        ProviderModelCatalogEntry {
            provider: "opencode-go".into(),
            status,
            source: ProviderCatalogSource::OpenCodeGoCatalogUrl,
            catalog_url: Some("https://opencode.ai/inference/go/v1/models".into()),
            models: Vec::new(),
            detail: detail.into(),
            discovered_at: discovered_at.into(),
            credential_signal: ProviderCredentialSignal::NotObserved,
            last_known: None,
        }
    }

    #[test]
    fn catalog_is_explicit_about_codex_oauth_and_catalog_sources() {
        assert_eq!(entry("codex").unwrap().chatgpt_oauth, "BLOCKED");
        assert_eq!(entry("opencode-go").unwrap().kind, "opencode");
        assert_eq!(
            entry("opencode-go").unwrap().catalog,
            ModelCatalogStrategy::OpenCodeGoSibling
        );
        assert_eq!(
            entry("opencode").unwrap().catalog,
            ModelCatalogStrategy::ProviderBaseUrl
        );
        assert_eq!(
            entry("codex").unwrap().catalog,
            ModelCatalogStrategy::Unsupported
        );
        assert!(entry("unknown").is_none());
    }

    #[test]
    fn opencode_go_resolves_the_sibling_catalog_and_never_its_inference_models_path() {
        let resolved =
            catalog_endpoint("opencode-go", "https://opencode.ai/inference/go/openai/v1").unwrap();
        assert_eq!(resolved.url, "https://opencode.ai/inference/go/v1/models");
        assert_eq!(resolved.source, ProviderCatalogSource::OpenCodeGoCatalogUrl);
        assert!(!resolved.requires_credential);

        let generic = catalog_endpoint("opencode", "https://gateway.example/v1/").unwrap();
        assert_eq!(generic.url, "https://gateway.example/v1/models");
        assert_eq!(generic.source, ProviderCatalogSource::OpenAiModelsEndpoint);
        assert!(generic.requires_credential);

        assert_eq!(
            catalog_endpoint("opencode-go", "https://gateway.example/v1"),
            Err(CatalogEndpointError::UnexpectedBaseUrl)
        );
        assert!(matches!(
            catalog_endpoint("codex", "https://api.openai.com/v1"),
            Err(CatalogEndpointError::Unsupported(_))
        ));
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
    fn a_failed_refresh_keeps_last_known_provenance_but_no_routable_models() {
        let mut cache = ProviderModelCache::default();
        cache.record(available(&["model-a"], "2026-10-28T00:00:00Z"));
        assert_eq!(cache.models("opencode-go"), ["model-a"]);

        cache.record(unavailable(
            ProviderDiscoveryStatus::EndpointUnsupported,
            "HTTP 404",
            "2026-10-28T00:01:00Z",
        ));
        assert!(cache.models("opencode-go").is_empty());
        let entry = cache.entry("opencode-go").unwrap();
        assert_eq!(entry.status, ProviderDiscoveryStatus::EndpointUnsupported);
        let last_known = entry.last_known.as_ref().unwrap();
        assert_eq!(last_known.models, ["model-a"]);
        assert_eq!(last_known.observed_at, "2026-10-28T00:00:00Z");
    }

    async fn serve(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, server)
    }

    #[tokio::test]
    async fn opencode_go_discovery_uses_the_sibling_catalog_without_the_credential() {
        use axum::{Router, routing::get};

        async fn sibling(headers: axum::http::HeaderMap) -> axum::Json<serde_json::Value> {
            assert!(
                headers.get("authorization").is_none(),
                "the public sibling catalog must not receive the provider credential"
            );
            axum::Json(serde_json::json!({
                "object": "list",
                "data": [
                    {"id": "deepseek-v4.1-flash"},
                    {"id": "deepseek-v4-pro"},
                    {"id": "deepseek-v4.1-flash"}
                ]
            }))
        }
        async fn inference_models() -> axum::http::StatusCode {
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        }
        let app = Router::new()
            .route("/inference/go/v1/models", get(sibling))
            .route("/inference/go/openai/v1/models", get(inference_models));
        let (base, server) = serve(app).await;
        let discovery = discover_models(
            "opencode-go",
            &format!("{base}/inference/go/openai/v1"),
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(discovery.status, ProviderDiscoveryStatus::Available);
        assert_eq!(
            discovery.source,
            ProviderCatalogSource::OpenCodeGoCatalogUrl
        );
        assert_eq!(
            discovery.catalog_url.as_deref(),
            Some(format!("{base}/inference/go/v1/models").as_str())
        );
        assert_eq!(discovery.models, ["deepseek-v4.1-flash", "deepseek-v4-pro"]);
        assert_eq!(
            discovery.credential_signal,
            ProviderCredentialSignal::NotObserved
        );
        let serialized = serde_json::to_string(&discovery).unwrap();
        assert!(!serialized.contains("test-credential"));
        server.abort();
    }

    #[tokio::test]
    async fn generic_provider_discovery_reads_the_models_endpoint_with_the_credential() {
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
        let (base, server) = serve(app).await;
        let discovery = discover_models(
            "opencode",
            &format!("{base}/v1"),
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(discovery.status, ProviderDiscoveryStatus::Available);
        assert_eq!(
            discovery.source,
            ProviderCatalogSource::OpenAiModelsEndpoint
        );
        assert_eq!(discovery.models, ["model-a", "model-b"]);
        assert!(!discovery.detail.contains("test-credential"));
        let serialized = serde_json::to_string(&discovery).unwrap();
        assert!(!serialized.contains("test-credential"));
        server.abort();
    }

    #[tokio::test]
    async fn a_404_is_an_absent_catalog_not_a_credential_finding() {
        use axum::{Router, routing::get};

        async fn missing() -> axum::http::StatusCode {
            axum::http::StatusCode::NOT_FOUND
        }
        let app = Router::new().route("/v1/models", get(missing));
        let (base, server) = serve(app).await;
        let discovery = discover_models(
            "opencode",
            &format!("{base}/v1"),
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(
            discovery.status,
            ProviderDiscoveryStatus::EndpointUnsupported
        );
        assert_eq!(
            discovery.credential_signal,
            ProviderCredentialSignal::NotObserved
        );
        assert!(discovery.detail.contains("HTTP 404"));
        assert!(!discovery.detail.contains("rejected"));
        assert!(
            !discovery
                .detail
                .to_lowercase()
                .contains("invalid credential")
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_401_is_a_credential_rejection_distinct_from_an_absent_catalog() {
        use axum::{Router, routing::get};

        async fn denied() -> axum::http::StatusCode {
            axum::http::StatusCode::UNAUTHORIZED
        }
        let app = Router::new().route("/v1/models", get(denied));
        let (base, server) = serve(app).await;
        let discovery = discover_models(
            "opencode",
            &format!("{base}/v1"),
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(
            discovery.status,
            ProviderDiscoveryStatus::CredentialRejected
        );
        assert_eq!(
            discovery.credential_signal,
            ProviderCredentialSignal::Rejected
        );
        assert!(
            discovery.detail.contains("HTTP 401"),
            "{}",
            discovery.detail
        );
        assert!(discovery.models.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn malformed_and_oversized_catalogs_are_bounded_and_rejected() {
        use axum::{Router, routing::get};

        async fn malformed() -> &'static str {
            "not json"
        }
        async fn oversized() -> String {
            "a".repeat(MAX_DISCOVERY_BODY_BYTES + 1)
        }
        let app = Router::new()
            .route("/v1/models", get(malformed))
            .route("/big/v1/models", get(oversized));
        let (base, server) = serve(app).await;

        let malformed = discover_models(
            "opencode",
            &format!("{base}/v1"),
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(malformed.status, ProviderDiscoveryStatus::InvalidResponse);
        assert!(malformed.models.is_empty());

        let oversized = discover_models(
            "opencode",
            &format!("{base}/big/v1"),
            "test-credential-1234567890",
            true,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(oversized.status, ProviderDiscoveryStatus::InvalidResponse);
        assert!(oversized.models.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn codex_and_unknown_catalogs_are_typed_unsupported() {
        let codex = discover_models(
            "codex",
            "https://api.openai.com/v1",
            "test-credential-1234567890",
            false,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(codex.status, ProviderDiscoveryStatus::Unsupported);
        assert!(codex.detail.contains("no model catalog"));
        assert!(codex.models.is_empty());

        let unknown = discover_models(
            "unknown",
            "https://api.openai.com/v1",
            "test-credential-1234567890",
            false,
            Vec::new(),
            "2026-10-28T00:00:00Z".into(),
        )
        .await;
        assert_eq!(unknown.status, ProviderDiscoveryStatus::Unsupported);
        assert!(unknown.models.is_empty());
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
