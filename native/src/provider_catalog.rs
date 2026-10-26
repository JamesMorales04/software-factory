//! Explicit developer-preview provider catalog and configuration validation.
//!
//! The catalog states what is implemented and tested. It never converts a
//! subscription or account claim into a supported route. ChatGPT-authenticated
//! Codex remains blocked in this build.

use serde::Serialize;

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

pub fn validate_model(model: &str) -> Result<(), String> {
    if model.trim().is_empty()
        || model.len() > 128
        || model.trim() != model
        || model.chars().any(char::is_whitespace)
    {
        return Err("model must be a bounded non-whitespace value".into());
    }
    Ok(())
}

pub fn validate_base_url(base_url: &str, allow_loopback_http: bool) -> Result<(), String> {
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
        "http"
            if allow_loopback_http && matches!(url.host_str(), Some("127.0.0.1" | "localhost")) => {
        }
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
        assert!(validate_base_url("https://opencode.ai/inference/go/openai/v1", false).is_ok());
        assert!(validate_base_url("http://opencode.ai/v1", false).is_err());
        assert!(validate_base_url("https://user:pass@opencode.ai/v1", false).is_err());
        assert!(validate_base_url("http://127.0.0.1:9999/v1", true).is_ok());
        assert!(validate_model("deepseek-v4.1-flash").is_ok());
        assert!(validate_model("bad model").is_err());
    }
}
