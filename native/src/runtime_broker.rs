//! Typed client for the trusted Factory runtime broker.
//!
//! The broker is the only component that may create execution volumes or child
//! containers in the dedicated nested Podman engine. Callers supply typed
//! operations, never Podman flags, images outside the allowlist or host paths.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

use crate::execution::{BoundedOutput, ExecutionError};

const MAX_ERROR_BYTES: usize = 4096;
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RESOURCE_INVENTORY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANAGED_RESOURCES: usize = 4096;
const MAX_LABEL_BYTES: usize = 128;

const MANAGED_LABEL_KEY: &str = "io.software-factory.managed";
const WORKSPACE_LABEL_KEY: &str = "io.software-factory.workspace";
const OWNER_LABEL_KEY: &str = "io.software-factory.owner";

/// The trusted service that owns a managed execution resource. Reconciliation
/// may remove only resources carrying its own owner label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceOwner {
    Supervisor,
    Verifier,
    Provider,
}

impl ResourceOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supervisor => "supervisor",
            Self::Verifier => "verifier",
            Self::Provider => "provider",
        }
    }
}

/// One labelled volume or container returned by the runtime broker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedResource {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// Read-only inventory of resources that carry the managed-execution label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedResources {
    #[serde(default)]
    pub volumes: Vec<ManagedResource>,
    #[serde(default)]
    pub containers: Vec<ManagedResource>,
}

pub fn is_valid_volume_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("factory-ws-") else {
        return false;
    };
    rest.len() == 36
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

pub fn is_valid_exec_container_name(name: &str) -> bool {
    valid_prefixed_uuid(name, "factory-exec-")
}

pub fn is_valid_provider_container_name(name: &str) -> bool {
    valid_prefixed_uuid(name, "factory-provider-")
}

pub fn is_valid_container_name(name: &str) -> bool {
    is_valid_exec_container_name(name) || is_valid_provider_container_name(name)
}

fn valid_prefixed_uuid(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == 36
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

/// Parse the bounded JSON emitted by `podman volume ls --format json` for the
/// managed label. Entries without a valid managed/name identity are ignored so
/// a foreign resource can never become a deletion candidate.
pub fn parse_volume_inventory(bytes: &[u8]) -> Result<Vec<ManagedResource>, String> {
    if bytes.len() > MAX_RESOURCE_INVENTORY_BYTES {
        return Err("volume inventory exceeds its bound".into());
    }
    let entries: Vec<PodmanVolumeEntry> = serde_json::from_slice(bytes)
        .map_err(|error| format!("volume inventory is invalid JSON: {error}"))?;
    if entries.len() > MAX_MANAGED_RESOURCES {
        return Err("volume inventory has too many entries".into());
    }
    let mut resources = BTreeMap::new();
    for entry in entries {
        let labels = entry.labels.unwrap_or_default();
        if !is_managed(&labels) || !is_valid_volume_name(&entry.name) {
            continue;
        }
        resources.insert(
            entry.name.clone(),
            ManagedResource {
                name: entry.name,
                workspace: labels
                    .get(WORKSPACE_LABEL_KEY)
                    .and_then(|value| bounded_label(value)),
                owner: labels
                    .get(OWNER_LABEL_KEY)
                    .and_then(|value| bounded_label(value)),
            },
        );
    }
    Ok(resources.into_values().collect())
}

/// Parse the bounded JSON emitted by `podman ps -a --format json` for the
/// managed label.
pub fn parse_container_inventory(bytes: &[u8]) -> Result<Vec<ManagedResource>, String> {
    if bytes.len() > MAX_RESOURCE_INVENTORY_BYTES {
        return Err("container inventory exceeds its bound".into());
    }
    let entries: Vec<PodmanContainerEntry> = serde_json::from_slice(bytes)
        .map_err(|error| format!("container inventory is invalid JSON: {error}"))?;
    if entries.len() > MAX_MANAGED_RESOURCES {
        return Err("container inventory has too many entries".into());
    }
    let mut resources = BTreeMap::new();
    for entry in entries {
        let labels = entry.labels.unwrap_or_default();
        let name = match entry.names {
            PodmanNames::List(names) => names.into_iter().next(),
            PodmanNames::Single(name) => Some(name),
        };
        let Some(name) = name else {
            continue;
        };
        if !is_managed(&labels) || !is_valid_container_name(&name) {
            continue;
        }
        resources.insert(
            name.clone(),
            ManagedResource {
                name,
                workspace: labels
                    .get(WORKSPACE_LABEL_KEY)
                    .and_then(|value| bounded_label(value)),
                owner: labels
                    .get(OWNER_LABEL_KEY)
                    .and_then(|value| bounded_label(value)),
            },
        );
    }
    Ok(resources.into_values().collect())
}

#[derive(Deserialize)]
struct PodmanVolumeEntry {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Labels", default)]
    labels: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct PodmanContainerEntry {
    #[serde(rename = "Names")]
    names: PodmanNames,
    #[serde(rename = "Labels", default)]
    labels: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PodmanNames {
    List(Vec<String>),
    Single(String),
}

fn is_managed(labels: &BTreeMap<String, String>) -> bool {
    labels.get(MANAGED_LABEL_KEY).map(String::as_str) == Some("true")
}

fn bounded_label(value: &str) -> Option<String> {
    (value.len() <= MAX_LABEL_BYTES && value.bytes().all(|byte| byte.is_ascii_graphic()))
        .then(|| value.to_owned())
}

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
    pub owner: ResourceOwner,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContainer<'a> {
    pub volume: &'a str,
    pub container: &'a str,
    pub image: &'a str,
    pub argv: &'a [String],
    pub timeout_secs: u64,
    pub owner: ResourceOwner,
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

    /// Read-only enumeration of managed execution resources. Used by the
    /// Supervisor to remove orphaned volumes/containers that no durable record
    /// references.
    pub async fn list_managed_resources(&self) -> Result<ManagedResources, BrokerError> {
        let response = self
            .client
            .get(self.join("v1/resources")?)
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        if let Some(length) = response.content_length()
            && length as usize > MAX_RESOURCE_INVENTORY_BYTES
        {
            return Err(BrokerError::InvalidResponse(
                "managed resource inventory exceeds the broker bound".into(),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_RESOURCE_INVENTORY_BYTES {
            return Err(BrokerError::InvalidResponse(
                "managed resource inventory exceeds the broker bound".into(),
            ));
        }
        let resources: ManagedResources = serde_json::from_slice(&bytes)
            .map_err(|error| BrokerError::InvalidResponse(format!("inventory JSON: {error}")))?;
        if resources.volumes.len() > MAX_MANAGED_RESOURCES
            || resources.containers.len() > MAX_MANAGED_RESOURCES
        {
            return Err(BrokerError::InvalidResponse(
                "managed resource inventory has too many entries".into(),
            ));
        }
        Ok(resources)
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

    #[test]
    fn volume_inventory_keeps_only_managed_valid_volumes() {
        let managed = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let foreign = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let json = format!(
            r#"[
              {{"Name":"{managed}","Labels":{{"io.software-factory.managed":"true","io.software-factory.workspace":"attempt-1","io.software-factory.owner":"supervisor"}}}},
              {{"Name":"{foreign}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"verifier"}}}},
              {{"Name":"unmanaged","Labels":{{"io.software-factory.workspace":"attempt-2"}}}},
              {{"Name":"not-a-factory-volume","Labels":{{"io.software-factory.managed":"true"}}}},
              {{"Name":"../escape","Labels":{{"io.software-factory.managed":"true"}}}}
            ]"#
        );
        let resources = parse_volume_inventory(json.as_bytes()).unwrap();
        assert_eq!(resources.len(), 2);
        let foreign_resource = resources
            .iter()
            .find(|resource| resource.name == foreign)
            .unwrap();
        assert_eq!(foreign_resource.owner.as_deref(), Some("verifier"));
        assert!(foreign_resource.workspace.is_none());
        let managed_resource = resources
            .iter()
            .find(|resource| resource.name == managed)
            .unwrap();
        assert_eq!(managed_resource.workspace.as_deref(), Some("attempt-1"));
        assert_eq!(managed_resource.owner.as_deref(), Some("supervisor"));
    }

    #[test]
    fn container_inventory_keeps_only_managed_exec_and_provider_containers() {
        let exec = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let provider = format!("factory-provider-{}", uuid::Uuid::new_v4());
        let json = format!(
            r#"[
              {{"Names":["{exec}"],"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}},
              {{"Names":["{provider}"],"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"provider"}}}},
              {{"Names":["factory-exec-not-a-uuid"],"Labels":{{"io.software-factory.managed":"true"}}}},
              {{"Names":["other"],"Labels":{{"io.software-factory.managed":"true"}}}},
              {{"Names":["unmanaged"],"Labels":{{}}}}
            ]"#
        );
        let resources = parse_container_inventory(json.as_bytes()).unwrap();
        assert_eq!(resources.len(), 2);
        assert!(resources.iter().any(|resource| resource.name == exec));
        assert!(resources.iter().any(|resource| resource.name == provider));
    }

    #[test]
    fn inventories_reject_invalid_and_oversized_json() {
        assert!(parse_volume_inventory(b"not json").is_err());
        assert!(parse_container_inventory(b"{}").is_err());
        let oversized = format!("[{}]", " ".repeat(MAX_RESOURCE_INVENTORY_BYTES));
        assert!(parse_volume_inventory(oversized.as_bytes()).is_err());
    }

    #[test]
    fn owner_labels_are_strict() {
        assert!(is_valid_volume_name(&format!(
            "factory-ws-{}",
            uuid::Uuid::new_v4()
        )));
        assert!(!is_valid_volume_name("factory-ws-../../etc"));
        assert!(is_valid_exec_container_name(&format!(
            "factory-exec-{}",
            uuid::Uuid::new_v4()
        )));
        assert!(is_valid_provider_container_name(&format!(
            "factory-provider-{}",
            uuid::Uuid::new_v4()
        )));
        assert!(!is_valid_container_name("factory-exec-123"));
        assert_eq!(ResourceOwner::Supervisor.as_str(), "supervisor");
    }
}
