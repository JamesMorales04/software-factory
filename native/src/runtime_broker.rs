//! Typed client for the trusted Factory runtime broker.
//!
//! The broker is the only component that may create execution volumes or child
//! containers in the dedicated nested Podman engine. Callers supply typed
//! operations, never Podman flags, images outside the allowlist or host paths.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::execution::{BoundedOutput, ExecutionError};

const MAX_ERROR_BYTES: usize = 4096;
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("runtime broker configuration is invalid: {0}")]
    Invalid(String),
    #[error("runtime broker I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime broker request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("runtime broker refused the operation: {0}")]
    Refused(String),
    #[error("runtime broker returned an invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Clone)]
pub struct RuntimeBrokerClient {
    base: reqwest::Url,
    token: String,
    client: reqwest::Client,
}

impl std::fmt::Debug for RuntimeBrokerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeBrokerClient")
            .field("base", &self.base)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewVolume {
    pub volume: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContainer<'a> {
    pub volume: &'a str,
    pub container: &'a str,
    pub image: &'a str,
    pub argv: &'a [String],
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContainerResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartProvider<'a> {
    pub container: &'a str,
    pub session_subdir: &'a str,
    pub timeout_secs: u64,
    pub env: &'a [(String, String)],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatus {
    pub container: String,
    pub status: String,
    pub exit_code: i32,
}

impl RuntimeBrokerClient {
    pub fn new(base: &str, token: String) -> Result<Self, BrokerError> {
        let base = reqwest::Url::parse(base)
            .map_err(|_| BrokerError::Invalid("runtime broker URL".into()))?;
        if base.scheme() != "http"
            || base.host_str().is_none()
            || base.port().is_none()
            || !matches!(base.path(), "" | "/")
            || base.username() != ""
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(BrokerError::Invalid(
                "runtime broker URL must be an HTTP origin with an explicit port".into(),
            ));
        }
        if token.len() < 32 {
            return Err(BrokerError::Invalid(
                "runtime broker token is too short".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            base,
            token,
            client,
        })
    }

    pub async fn preflight(&self) -> Result<(), BrokerError> {
        let response = self
            .client
            .get(self.join("health")?)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        let body: serde_json::Value = response.json().await?;
        if body.get("healthy").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(BrokerError::Refused(
                "runtime broker did not report healthy".into(),
            ));
        }
        Ok(())
    }

    pub async fn create_volume(&self, input: &NewVolume) -> Result<(), BrokerError> {
        let response = self
            .client
            .post(self.join("v1/volumes")?)
            .bearer_auth(&self.token)
            .json(input)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        self.expect_success(response).await
    }

    pub async fn remove_volume(&self, volume: &str) -> Result<(), BrokerError> {
        let response = self
            .client
            .delete(self.join(&format!("v1/volumes/{volume}"))?)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        self.expect_success(response).await
    }

    pub async fn import_volume(&self, volume: &str, bytes: Vec<u8>) -> Result<(), BrokerError> {
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(BrokerError::Invalid(
                "workspace archive exceeds the broker bound".into(),
            ));
        }
        let response = self
            .client
            .post(self.join(&format!("v1/volumes/{volume}/import"))?)
            .bearer_auth(&self.token)
            .header("content-type", "application/x-tar")
            .body(bytes)
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        self.expect_success(response).await
    }

    pub async fn export_volume(&self, volume: &str) -> Result<Vec<u8>, BrokerError> {
        let response = self
            .client
            .get(self.join(&format!("v1/volumes/{volume}/export"))?)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        if let Some(length) = response.content_length()
            && length > MAX_ARCHIVE_BYTES
        {
            return Err(BrokerError::InvalidResponse(
                "volume export exceeds the broker bound".into(),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(BrokerError::InvalidResponse(
                "volume export exceeds the broker bound".into(),
            ));
        }
        Ok(bytes.to_vec())
    }

    pub async fn run_container(
        &self,
        input: &RunContainer<'_>,
    ) -> Result<RunContainerResult, BrokerError> {
        let response = self
            .client
            .post(self.join("v1/containers/run")?)
            .bearer_auth(&self.token)
            .json(input)
            .timeout(Duration::from_secs(input.timeout_secs + 30))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        response.json().await.map_err(BrokerError::Request)
    }

    pub async fn remove_container(&self, container: &str) -> Result<(), BrokerError> {
        let response = self
            .client
            .delete(self.join(&format!("v1/containers/{container}"))?)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        self.expect_success(response).await
    }

    pub async fn start_provider(
        &self,
        input: &StartProvider<'_>,
    ) -> Result<ContainerStatus, BrokerError> {
        let response = self
            .client
            .post(self.join("v1/providers/start")?)
            .bearer_auth(&self.token)
            .json(input)
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        let body: serde_json::Value = response.json().await?;
        let container = body
            .get("container")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(ContainerStatus {
            container,
            status: "running".into(),
            exit_code: -1,
        })
    }

    pub async fn container_status(&self, container: &str) -> Result<ContainerStatus, BrokerError> {
        let response = self
            .client
            .get(self.join(&format!("v1/containers/{container}/status"))?)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        response.json().await.map_err(BrokerError::Request)
    }

    fn join(&self, path: &str) -> Result<reqwest::Url, BrokerError> {
        self.base
            .join(path)
            .map_err(|_| BrokerError::Invalid("runtime broker operation URL".into()))
    }

    async fn expect_success(&self, response: reqwest::Response) -> Result<(), BrokerError> {
        if response.status().is_success() {
            return Ok(());
        }
        Err(self.refusal(response).await)
    }

    async fn refusal(&self, response: reqwest::Response) -> BrokerError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let body: String = body.chars().take(MAX_ERROR_BYTES).collect();
        BrokerError::Refused(format!("HTTP {status}: {body}"))
    }
}

pub fn run_result(result: RunContainerResult) -> BoundedOutput {
    BoundedOutput {
        exit_code: result.exit_code,
        stdout: result.stdout.into_bytes(),
        stderr: result.stderr.into_bytes(),
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
    }
}

impl From<BrokerError> for ExecutionError {
    fn from(error: BrokerError) -> Self {
        Self::Volume(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_url_and_token_are_strict() {
        assert!(RuntimeBrokerClient::new("http://runtime:8770", "x".repeat(32)).is_ok());
        assert!(RuntimeBrokerClient::new("http://runtime:8770/path", "x".repeat(32)).is_err());
        assert!(RuntimeBrokerClient::new("https://runtime:8770", "x".repeat(32)).is_err());
        assert!(RuntimeBrokerClient::new("http://runtime:8770", "short".into()).is_err());
    }
}
