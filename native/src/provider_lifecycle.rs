//! Worker-side HTTP lifecycle for Core-owned provider turns and sessions.
//!
//! This module builds the exact Worker-to-Core requests for turn settlement,
//! release, terminal evidence and unresolved-session cleanup. Core remains the
//! authority; these calls record evidence and never grant budget, candidate or
//! acceptance rights on their own.
//!
//! The unresolved-session loop takes an injected cleanup-receipt provider so
//! the HTTP reconciliation path is testable without a real container or
//! provider sandbox. Production passes the sandbox-specific receipt function.

use std::future::Future;

use serde_json::{Value, json};

use crate::{db::ProviderTerminal, provider_patch::ProviderUsage};

#[derive(Debug, thiserror::Error)]
pub enum ProviderLifecycleError {
    #[error("provider lifecycle request is invalid: {0}")]
    Invalid(String),
    #[error("provider lifecycle HTTP request failed: {0}")]
    Http(String),
    #[error("Core rejected provider lifecycle request: {0}")]
    Refused(String),
    #[error("unresolved provider session is malformed: {0}")]
    Session(String),
    #[error("unresolved provider cleanup failed: {0}")]
    Cleanup(String),
}

fn endpoint(core: &reqwest::Url, path: &str) -> Result<reqwest::Url, ProviderLifecycleError> {
    core.join(path)
        .map_err(|error| ProviderLifecycleError::Invalid(error.to_string()))
}

fn require(value: &str, label: &str) -> Result<(), ProviderLifecycleError> {
    if value.trim().is_empty() {
        return Err(ProviderLifecycleError::Invalid(label.into()));
    }
    Ok(())
}

/// Settle one authorized turn exactly once with the observed usage.
pub async fn settle_provider_turn(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    worker_id: &str,
    reservation_id: &str,
    usage: &ProviderUsage,
) -> Result<(), ProviderLifecycleError> {
    require(session_token, "worker session token")?;
    require(worker_id, "worker id")?;
    require(reservation_id, "reservation id")?;
    let response = client
        .post(endpoint(core, "worker/provider_turn_settle")?)
        .bearer_auth(session_token)
        .json(&json!({
            "usage": {
                "reservation_id": reservation_id,
                "worker_id": worker_id,
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "total_tokens": usage.total_tokens,
                "cost_micros": usage.cost_micros,
                "usage_known": usage.known
            }
        }))
        .send()
        .await
        .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
    if !response.status().is_success() {
        return Err(ProviderLifecycleError::Refused(format!(
            "settle HTTP {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }
    Ok(())
}

/// Release one turn reservation that never bound a provider request.
pub async fn release_provider_turn(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    reservation_id: &str,
    session_record_id: &str,
    worker_id: &str,
    generation: i64,
) -> Result<(), ProviderLifecycleError> {
    require(session_token, "worker session token")?;
    require(reservation_id, "reservation id")?;
    require(session_record_id, "session record id")?;
    require(worker_id, "worker id")?;
    if generation <= 0 {
        return Err(ProviderLifecycleError::Invalid("generation".into()));
    }
    let response = client
        .post(endpoint(core, "worker/provider_turn_release")?)
        .bearer_auth(session_token)
        .json(&json!({
            "reservation_id": reservation_id,
            "session_record_id": session_record_id,
            "worker_id": worker_id,
            "generation": generation
        }))
        .send()
        .await
        .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
    if !response.status().is_success() {
        return Err(ProviderLifecycleError::Refused(format!(
            "release HTTP {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }
    Ok(())
}

/// Report terminal provider-session evidence, including the cleanup receipt.
pub async fn post_provider_terminal(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    terminal: &ProviderTerminal,
) -> Result<(), ProviderLifecycleError> {
    require(session_token, "worker session token")?;
    let response = client
        .post(endpoint(core, "worker/provider_terminal")?)
        .bearer_auth(session_token)
        .json(terminal)
        .send()
        .await
        .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
    if !response.status().is_success() {
        return Err(ProviderLifecycleError::Refused(format!(
            "terminal HTTP {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedProviderSession {
    pub session_record_id: String,
    pub generation: i64,
    pub provider_session_id: String,
    pub provider_directory: String,
}

impl UnresolvedProviderSession {
    fn parse(value: &Value) -> Result<Self, ProviderLifecycleError> {
        let session_record_id = value
            .get("session_record_id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ProviderLifecycleError::Session("missing session record id".into()))?
            .to_owned();
        let generation = value
            .get("generation")
            .and_then(Value::as_i64)
            .filter(|generation| *generation > 0)
            .ok_or_else(|| ProviderLifecycleError::Session("missing generation".into()))?;
        Ok(Self {
            session_record_id,
            generation,
            provider_session_id: value
                .get("provider_session_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            provider_directory: value
                .get("provider_directory")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }
}

/// Fetch every unresolved session owned by this Worker, obtain an external
/// cleanup receipt for each, and submit it to Core exactly once.
///
/// A failed receipt is collected and reported without submitting a cleanup
/// claim, so Core never records an unverified orphan as cleaned.
pub async fn recover_unresolved_provider_sessions<F, Fut>(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    worker_id: &str,
    mut receipt: F,
) -> Result<(), ProviderLifecycleError>
where
    F: FnMut(&UnresolvedProviderSession) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    require(session_token, "worker session token")?;
    require(worker_id, "worker id")?;
    let response = client
        .get(endpoint(core, "worker/provider_unresolved")?)
        .bearer_auth(session_token)
        .query(&[("worker_id", worker_id)])
        .send()
        .await
        .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
    let response = response
        .error_for_status()
        .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
    let sessions: Vec<Value> = response
        .json()
        .await
        .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
    let mut failures = Vec::new();
    for value in &sessions {
        let session = match UnresolvedProviderSession::parse(value) {
            Ok(session) => session,
            Err(error) => {
                failures.push(error.to_string());
                continue;
            }
        };
        let receipt = match receipt(&session).await {
            Ok(receipt) if !receipt.trim().is_empty() => receipt,
            Ok(_) => {
                failures.push(format!(
                    "{}: cleanup receipt is empty",
                    session.session_record_id
                ));
                continue;
            }
            Err(error) => {
                failures.push(format!("{}: {error}", session.session_record_id));
                continue;
            }
        };
        let response = client
            .post(endpoint(core, "worker/provider_cleanup")?)
            .bearer_auth(session_token)
            .json(&json!({
                "session_record_id": session.session_record_id,
                "worker_id": worker_id,
                "generation": session.generation,
                "cleanup_receipt": receipt
            }))
            .send()
            .await
            .map_err(|error| ProviderLifecycleError::Http(error.to_string()))?;
        if !response.status().is_success() {
            failures.push(format!(
                "{}: cleanup rejected with {}",
                session.session_record_id,
                response.status()
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ProviderLifecycleError::Cleanup(failures.join("; ")))
    }
}
