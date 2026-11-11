//! Typed client for the trusted Factory runtime broker.
//!
//! The broker is the only component that may create execution volumes or child
//! containers in the dedicated nested Podman engine. Callers supply typed
//! operations, never Podman flags, images outside the allowlist or host paths.

use std::{collections::BTreeMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::execution::{BoundedOutput, ExecutionError};
use crate::planning::environment::{
    EnvironmentBuildReceipt, EnvironmentPlan, EnvironmentResolution, EnvironmentSpec,
};

const MAX_ERROR_BYTES: usize = 4096;
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENVIRONMENT_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ENVIRONMENT_WORKSPACE_FILES: usize = 128;
pub const MAX_ENVIRONMENT_WORKSPACE_FILE_BYTES: usize = 512 * 1024;
pub const MAX_ENVIRONMENT_WORKSPACE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_RESOURCE_INVENTORY_BYTES: usize = 4 * 1024 * 1024;
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
    Environment,
}

impl ResourceOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supervisor => "supervisor",
            Self::Verifier => "verifier",
            Self::Provider => "provider",
            Self::Environment => "environment",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value {
            "supervisor" => Some(Self::Supervisor),
            "verifier" => Some(Self::Verifier),
            "provider" => Some(Self::Provider),
            "environment" => Some(Self::Environment),
            _ => None,
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

pub fn is_valid_dependency_container_name(name: &str) -> bool {
    valid_prefixed_uuid(name, "factory-deps-")
}

pub fn is_valid_container_name(name: &str) -> bool {
    is_valid_exec_container_name(name) || is_valid_provider_container_name(name)
}

pub fn is_valid_environment_volume_name(name: &str) -> bool {
    valid_prefixed_uuid(name, "factory-env-")
}

pub fn is_valid_environment_container_name(name: &str) -> bool {
    valid_prefixed_uuid(name, "factory-env-")
}

pub const ENVIRONMENT_CACHE_VOLUME: &str = "factory-env-cache";

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
struct PodmanContainerInspect {
    #[serde(rename = "Config", default)]
    config: Option<PodmanContainerConfig>,
}

#[derive(Deserialize)]
struct PodmanContainerConfig {
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

/// Owner derived from the managed and owner labels. Missing, unknown or
/// non-managed labels yield `None`.
pub fn managed_owner(labels: &BTreeMap<String, String>) -> Option<ResourceOwner> {
    if !is_managed(labels) {
        return None;
    }
    labels
        .get(OWNER_LABEL_KEY)
        .and_then(|value| ResourceOwner::from_label(value))
}

/// A resource is removable only when it carries the managed label and the
/// expected owner label. Missing or unknown owner labels fail closed.
pub fn resource_owner_matches(labels: &BTreeMap<String, String>, expected: ResourceOwner) -> bool {
    managed_owner(labels) == Some(expected)
}

/// Volume reuse is allowed only when the existing volume is managed, owned by
/// the same service and labelled with the same workspace identity.
pub fn volume_identity_matches(
    labels: &BTreeMap<String, String>,
    expected: ResourceOwner,
    workspace_id: &str,
) -> bool {
    resource_owner_matches(labels, expected)
        && labels.get(WORKSPACE_LABEL_KEY).map(String::as_str) == Some(workspace_id)
}

/// Legacy resources created before owner labels were introduced are adoptable
/// only when they are managed, carry the exact same non-empty workspace
/// identity and have no owner label (or an empty one). A different workspace,
/// an existing owner or a missing managed label must remain a conflict.
pub fn legacy_workspace_identity_matches(
    labels: &BTreeMap<String, String>,
    workspace_id: &str,
) -> bool {
    !workspace_id.is_empty()
        && is_managed(labels)
        && labels.get(WORKSPACE_LABEL_KEY).map(String::as_str) == Some(workspace_id)
        && labels
            .get(OWNER_LABEL_KEY)
            .is_none_or(|owner| owner.is_empty())
}

/// Extract the label map from `podman volume inspect --format json`.
pub fn parse_volume_labels(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    if bytes.len() > MAX_RESOURCE_INVENTORY_BYTES {
        return Err("volume inspect output exceeds its bound".into());
    }
    let entries: Vec<PodmanVolumeEntry> = serde_json::from_slice(bytes)
        .map_err(|error| format!("volume inspect is invalid JSON: {error}"))?;
    entries
        .into_iter()
        .next()
        .map(|entry| entry.labels.unwrap_or_default())
        .ok_or_else(|| "volume inspect returned no entry".to_owned())
}

/// Extract the label map from `podman container inspect --format json`.
pub fn parse_container_labels(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    if bytes.len() > MAX_RESOURCE_INVENTORY_BYTES {
        return Err("container inspect output exceeds its bound".into());
    }
    let entries: Vec<PodmanContainerInspect> = serde_json::from_slice(bytes)
        .map_err(|error| format!("container inspect is invalid JSON: {error}"))?;
    entries
        .into_iter()
        .next()
        .map(|entry| {
            entry
                .config
                .and_then(|config| config.labels)
                .unwrap_or_default()
        })
        .ok_or_else(|| "container inspect returned no entry".to_owned())
}

fn bounded_label(value: &str) -> Option<String> {
    (value.len() <= MAX_LABEL_BYTES && value.bytes().all(|byte| byte.is_ascii_graphic()))
        .then(|| value.to_owned())
}

/// Append a streamed chunk only while it stays inside `limit`. Returning
/// `false` lets the caller terminate the response/process instead of buffering
/// an unbounded body first.
pub fn extend_bounded(target: &mut Vec<u8>, chunk: &[u8], limit: usize) -> bool {
    if target.len().saturating_add(chunk.len()) > limit {
        return false;
    }
    target.extend_from_slice(chunk);
    true
}

const MAX_DEPENDENCY_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("runtime broker configuration is invalid: {0}")]
    Invalid(String),
    #[error("runtime broker configuration is invalid: {0}")]
    InvalidConfiguration(String),
    #[error("runtime broker I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime broker request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("runtime broker refused the operation: {0}")]
    Refused(String),
    #[error("runtime broker refused the operation ({status} {code}): {message}")]
    RefusedCode {
        status: u16,
        code: String,
        message: String,
    },
    #[error("runtime broker returned an invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Clone)]
pub struct RuntimeBrokerClient {
    base: reqwest::Url,
    token: String,
    owner: ResourceOwner,
    client: reqwest::Client,
}

impl std::fmt::Debug for RuntimeBrokerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeBrokerClient")
            .field("base", &self.base)
            .field("owner", &self.owner)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentBrokerOperation {
    Resolve,
    Build,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentBuildPhase {
    Resolve,
    Fetch,
    Build,
}

/// One project environment file sent by Core. Generated files are carried in
/// the plan itself; this list carries only bounded project files that the
/// builder must stage without a host mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentWorkspaceFile {
    pub path: String,
    pub sha256: String,
    pub content: String,
}

/// One narrow environment-builder request. It contains no argv, Podman flags,
/// host path, socket, network name or environment variable. The broker derives
/// the builder command, network, proxy and mounts from this typed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentBrokerRequest {
    pub schema_version: u32,
    pub operation: EnvironmentBrokerOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<EnvironmentBuildPhase>,
    pub image: String,
    pub digest: String,
    pub plan: EnvironmentPlan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<EnvironmentSpec>,
    #[serde(default)]
    pub workspace_files: Vec<EnvironmentWorkspaceFile>,
}

impl EnvironmentBrokerRequest {
    pub fn validate_shape(&self) -> Result<(), String> {
        if self.schema_version != crate::planning::environment::ENVIRONMENT_SCHEMA_VERSION {
            return Err("unsupported environment broker schema version".into());
        }
        if self.workspace_files.len() > MAX_ENVIRONMENT_WORKSPACE_FILES {
            return Err("environment workspace has too many files".into());
        }
        let mut total = 0usize;
        for file in &self.workspace_files {
            if file.path.is_empty()
                || file.path.len() > 512
                || file.path.starts_with('/')
                || file.path.split('/').any(|part| {
                    part.is_empty() || part == "." || part == ".." || part.contains('\\')
                })
            {
                return Err("environment workspace path is unsafe".into());
            }
            if file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err("environment workspace file hash is not lowercase SHA-256".into());
            }
            if file.content.len() > MAX_ENVIRONMENT_WORKSPACE_FILE_BYTES {
                return Err("environment workspace file exceeds its bound".into());
            }
            if crate::planning::environment::sha256_hex(file.content.as_bytes()) != file.sha256 {
                return Err("environment workspace file does not match its hash".into());
            }
            total = total.saturating_add(file.content.len());
        }
        if total > MAX_ENVIRONMENT_WORKSPACE_BYTES {
            return Err("environment workspace exceeds its byte bound".into());
        }
        Ok(())
    }
}

/// One frozen check executed by the environment-builder image. The broker
/// validates every field against the frozen plan/spec/receipt before any
/// container runs; the caller never supplies Podman flags or host paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentCheckSpec {
    pub check_id: String,
    /// The human `SpecCheck` description. It is carried end to end so a failed
    /// frozen check is diagnosable even when the command writes nothing.
    pub description: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub timeout_secs: u32,
    pub expect_exit: i32,
}

pub const MAX_ENVIRONMENT_CHECK_ARGV_ITEMS: usize = 64;
pub const MAX_ENVIRONMENT_CHECK_ARG_BYTES: usize = 2_000;
pub const MAX_ENVIRONMENT_CHECK_ARGV_BYTES: usize = 64 * 1024;
pub const MAX_ENVIRONMENT_CHECK_DESCRIPTION_CHARS: usize = 500;
pub const MAX_ENVIRONMENT_CHECK_DIAGNOSTIC_CHARS: usize = 1_400;
pub const MIN_ENVIRONMENT_CHECK_TIMEOUT_SECS: u32 = 30;
pub const MAX_ENVIRONMENT_CHECK_TIMEOUT_SECS: u32 = 7_200;

impl EnvironmentCheckSpec {
    /// Validate the same bounds enforced by the builder CLI. A malformed check
    /// is refused before the broker creates a container or touches the cache.
    pub fn validate_shape(&self) -> Result<(), String> {
        if self.check_id.is_empty()
            || self.check_id.len() > 200
            || self.check_id.chars().any(char::is_control)
        {
            return Err("environment check id is invalid".into());
        }
        if self.description.trim().is_empty()
            || self.description.chars().count() > MAX_ENVIRONMENT_CHECK_DESCRIPTION_CHARS
            || self.description.chars().any(char::is_control)
        {
            return Err("environment check description is required and bounded".into());
        }
        if self.argv.is_empty() || self.argv.len() > MAX_ENVIRONMENT_CHECK_ARGV_ITEMS {
            return Err("environment check argv is outside its bound".into());
        }
        if self.argv[0].is_empty() {
            return Err("environment check executable is empty".into());
        }
        let mut total = 0usize;
        for argument in &self.argv {
            if argument.len() > MAX_ENVIRONMENT_CHECK_ARG_BYTES || argument.contains('\0') {
                return Err("environment check argument is invalid".into());
            }
            total = total.saturating_add(argument.len());
        }
        if total > MAX_ENVIRONMENT_CHECK_ARGV_BYTES {
            return Err("environment check argv exceeds its byte bound".into());
        }
        if self.cwd != "." && !safe_environment_path(&self.cwd) {
            return Err("environment check working directory is unsafe".into());
        }
        if !(MIN_ENVIRONMENT_CHECK_TIMEOUT_SECS..=MAX_ENVIRONMENT_CHECK_TIMEOUT_SECS)
            .contains(&self.timeout_secs)
        {
            return Err("environment check timeout is outside its bound".into());
        }
        if !(-255..=255).contains(&self.expect_exit) {
            return Err("environment check expected exit is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentCheckRequest {
    pub schema_version: u32,
    pub image: String,
    pub digest: String,
    pub plan: EnvironmentPlan,
    pub spec: EnvironmentSpec,
    pub receipt: EnvironmentBuildReceipt,
    pub check: EnvironmentCheckSpec,
    pub workspace_volume: String,
}

impl EnvironmentCheckRequest {
    pub fn validate_shape(&self) -> Result<(), String> {
        if self.schema_version != crate::planning::environment::ENVIRONMENT_SCHEMA_VERSION {
            return Err("unsupported environment check schema version".into());
        }
        if self.image.is_empty()
            || self.image.len() > 512
            || self.image.contains('@')
            || self.image.chars().any(char::is_whitespace)
            || self.image.chars().any(char::is_control)
        {
            return Err("environment check builder image is invalid".into());
        }
        if !is_environment_digest(&self.digest) {
            return Err("environment check builder digest is invalid".into());
        }
        if !is_valid_volume_name(&self.workspace_volume) {
            return Err("environment check workspace volume is invalid".into());
        }
        self.check.validate_shape()
    }
}

/// The Supervisor-owned proxy request. It carries no workspace volume: the
/// Supervisor resolves the volume from its durable WorkspaceManager record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentCheckExecutionRequest {
    pub image: String,
    pub digest: String,
    pub plan: EnvironmentPlan,
    pub spec: EnvironmentSpec,
    pub receipt: EnvironmentBuildReceipt,
    pub check: EnvironmentCheckSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentCheckStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentCheckResult {
    pub check_id: String,
    pub spec_sha256: String,
    pub description: String,
    pub status: EnvironmentCheckStatus,
    pub exit_code: Option<i32>,
    pub expect_exit: i32,
    pub passed: bool,
    #[serde(default)]
    pub diagnostic: String,
    #[serde(default)]
    pub logs: String,
    #[serde(default)]
    pub logs_truncated: bool,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub stdout_truncated: bool,
    #[serde(default)]
    pub stderr_truncated: bool,
    pub cleanup_confirmed: bool,
    pub duration_ms: u64,
}

impl EnvironmentCheckResult {
    pub fn validate_for(&self, request: &EnvironmentCheckRequest) -> Result<(), String> {
        self.validate_for_check(
            &request.check.check_id,
            &request.check.description,
            &request.spec.spec_sha256,
            request.check.expect_exit,
        )
    }

    pub fn validate_for_check(
        &self,
        check_id: &str,
        description: &str,
        spec_sha256: &str,
        expect_exit: i32,
    ) -> Result<(), String> {
        if self.check_id != check_id
            || self.description != description
            || self.spec_sha256 != spec_sha256
            || self.expect_exit != expect_exit
        {
            return Err("environment check result does not bind the request".into());
        }
        if self.diagnostic.chars().count() > MAX_ENVIRONMENT_CHECK_DIAGNOSTIC_CHARS {
            return Err("environment check diagnostic exceeds its bound".into());
        }
        let Some(exit_code) = self.exit_code else {
            return Err("environment check result has no exit code".into());
        };
        let passed = exit_code == self.expect_exit;
        if self.passed != passed || (self.status == EnvironmentCheckStatus::Passed) != passed {
            return Err("environment check result contradicts its exit code".into());
        }
        if !self.cleanup_confirmed {
            return Err("environment check cleanup was not confirmed".into());
        }
        Ok(())
    }
}

/// Build a bounded, machine-readable diagnostic for a failed frozen check.
/// It carries the human `SpecCheck` description and at most one redacted
/// stderr line, never the full output or a source body.
pub fn bounded_check_diagnostic(
    description: &str,
    exit_code: i32,
    expect_exit: i32,
    stderr_tail: &str,
) -> String {
    let description: String = description
        .chars()
        .take(MAX_ENVIRONMENT_CHECK_DESCRIPTION_CHARS)
        .collect();
    let mut diagnostic = format!("{description} (exit {exit_code}, expected {expect_exit})");
    let tail: String = stderr_tail
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(600)
        .collect();
    if !tail.is_empty() {
        diagnostic.push_str(": ");
        diagnostic.push_str(&tail);
    }
    diagnostic
        .chars()
        .take(MAX_ENVIRONMENT_CHECK_DIAGNOSTIC_CHARS)
        .collect()
}

fn is_environment_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn safe_environment_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains("//")
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentBrokerResult {
    pub schema_version: u32,
    pub operation: EnvironmentBrokerOperation,
    pub phase: EnvironmentBuildPhase,
    #[serde(default)]
    pub spec_sha256: String,
    #[serde(default)]
    pub resolution: Option<EnvironmentResolution>,
    #[serde(default)]
    pub receipt: Option<EnvironmentBuildReceipt>,
    #[serde(default)]
    pub logs: String,
    #[serde(default)]
    pub logs_truncated: bool,
    pub cleanup_confirmed: bool,
    pub duration_ms: u64,
}

/// Read one project environment file without following a symlink or escaping
/// the canonical workspace root. The byte bound is enforced while reading so a
/// special or growing file cannot allocate first and fail later.
fn read_workspace_environment_file(root: &Path, relative: &str) -> Result<String, String> {
    use std::io::Read;
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "project environment file {relative} is not a safe relative path"
        ));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| format!("project environment workspace for {relative} is unavailable"))?;
    let mut current = canonical_root.clone();
    for component in relative_path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(format!(
                "project environment file {relative} is not a safe relative path"
            ));
        };
        current.push(part);
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| format!("project environment file {relative} is unavailable"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "project environment file {relative} must not be a symbolic link"
            ));
        }
    }
    let canonical = current
        .canonicalize()
        .map_err(|_| format!("project environment file {relative} is unavailable"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(format!(
            "project environment file {relative} escapes the workspace"
        ));
    }
    let file = std::fs::File::open(&canonical)
        .map_err(|_| format!("project environment file {relative} is unavailable"))?;
    let mut bytes = Vec::new();
    file.take(MAX_ENVIRONMENT_WORKSPACE_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("project environment file {relative} cannot be read"))?;
    if bytes.len() > MAX_ENVIRONMENT_WORKSPACE_FILE_BYTES {
        return Err(format!(
            "project environment file {relative} exceeds its bound"
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| format!("project environment file {relative} is not UTF-8 text"))
}

/// Absolute workspace root used to read project environment files. Core passes
/// it only to the client implementation; it is never sent to the broker and the
/// broker never receives a host path.
pub fn environment_workspace_files(
    plan: &EnvironmentPlan,
    workspace: Option<&Path>,
) -> Result<Vec<EnvironmentWorkspaceFile>, String> {
    let mut files = Vec::new();
    let mut total = 0usize;
    for file in &plan.files {
        let content = match (&file.content, workspace) {
            (Some(content), _) => content.clone(),
            (None, Some(root)) => read_workspace_environment_file(root, &file.path)?,
            (None, None) => {
                return Err(format!(
                    "project environment file {} requires an explicit workspace",
                    file.path
                ));
            }
        };
        if content.len() > MAX_ENVIRONMENT_WORKSPACE_FILE_BYTES {
            return Err(format!(
                "project environment file {} exceeds its bound",
                file.path
            ));
        }
        if file.content.is_none()
            && crate::planning::environment::sha256_hex(content.as_bytes()) != file.sha256
        {
            return Err(format!(
                "project environment file {} does not match the frozen plan hash",
                file.path
            ));
        }
        total = total.saturating_add(content.len());
        if total > MAX_ENVIRONMENT_WORKSPACE_BYTES {
            return Err("environment workspace exceeds its byte bound".into());
        }
        files.push(EnvironmentWorkspaceFile {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            content,
        });
        if files.len() > MAX_ENVIRONMENT_WORKSPACE_FILES {
            return Err("environment workspace has too many files".into());
        }
    }
    Ok(files)
}

pub const ENVIRONMENT_BUILD_ROUTE: &str = "v1/environments/build";
pub const ENVIRONMENT_RESOLVE_ROUTE: &str = "v1/environments/resolve";
pub const ENVIRONMENT_CHECK_ROUTE: &str = "v1/environments/check";

pub const MAX_ENVIRONMENT_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENVIRONMENT_CHECK_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
impl RuntimeBrokerClient {
    pub fn new(base: &str, token: String, owner: ResourceOwner) -> Result<Self, BrokerError> {
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
            owner,
            client,
        })
    }

    pub fn owner(&self) -> ResourceOwner {
        self.owner
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

    /// Read the broker's authenticated-free health projection. It contains no
    /// token or credential, only capability availability.
    pub async fn health(&self) -> Result<serde_json::Value, BrokerError> {
        let response = self
            .client
            .get(self.join("health")?)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        if let Some(length) = response.content_length()
            && length as usize > MAX_RESOURCE_INVENTORY_BYTES
        {
            return Err(BrokerError::InvalidResponse(
                "runtime broker health exceeds the broker bound".into(),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_RESOURCE_INVENTORY_BYTES {
            return Err(BrokerError::InvalidResponse(
                "runtime broker health exceeds the broker bound".into(),
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| BrokerError::InvalidResponse("runtime broker health is invalid".into()))
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
        let mut response = self
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
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if !extend_bounded(&mut bytes, &chunk, MAX_RESOURCE_INVENTORY_BYTES) {
                return Err(BrokerError::InvalidResponse(
                    "managed resource inventory exceeds the broker bound".into(),
                ));
            }
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

    /// Invoke exactly one typed environment resolve/build phase. The broker
    /// validates the builder image/digest, plan resources, network policy and
    /// staged workspace; Core never supplies a command, mount or proxy.
    pub async fn environment_build(
        &self,
        input: &EnvironmentBrokerRequest,
        timeout: Duration,
    ) -> Result<EnvironmentBrokerResult, BrokerError> {
        if self.owner != ResourceOwner::Environment {
            return Err(BrokerError::Invalid(
                "environment operations require the environment owner token".into(),
            ));
        }
        input.validate_shape().map_err(BrokerError::Invalid)?;
        let route = match input.operation {
            EnvironmentBrokerOperation::Resolve => ENVIRONMENT_RESOLVE_ROUTE,
            EnvironmentBrokerOperation::Build => ENVIRONMENT_BUILD_ROUTE,
        };
        let response = self
            .client
            .post(self.join(route)?)
            .bearer_auth(&self.token)
            .json(input)
            .timeout(timeout)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        if let Some(length) = response.content_length()
            && length as usize > MAX_ENVIRONMENT_RESPONSE_BYTES
        {
            return Err(BrokerError::InvalidResponse(
                "environment builder response exceeds the broker bound".into(),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_ENVIRONMENT_RESPONSE_BYTES {
            return Err(BrokerError::InvalidResponse(
                "environment builder response exceeds the broker bound".into(),
            ));
        }
        let result: EnvironmentBrokerResult = serde_json::from_slice(&bytes).map_err(|_| {
            BrokerError::InvalidResponse("environment builder response is invalid".into())
        })?;
        let expected_phase = match input.operation {
            EnvironmentBrokerOperation::Resolve => EnvironmentBuildPhase::Resolve,
            EnvironmentBrokerOperation::Build => {
                input.phase.unwrap_or(EnvironmentBuildPhase::Build)
            }
        };
        if result.operation != input.operation
            || result.phase != expected_phase
            || !result.cleanup_confirmed
        {
            return Err(BrokerError::InvalidResponse(
                "environment builder response does not bind the requested operation".into(),
            ));
        }
        Ok(result)
    }

    /// Execute exactly one frozen check through the environment-builder image
    /// behind the broker. The broker validates the exact builder identity,
    /// plan/spec/receipt binding, check bounds and the caller-owned workspace
    /// volume, then mounts the candidate workspace read-only and the per-spec
    /// cache read-write with no network.
    pub async fn environment_check(
        &self,
        input: &EnvironmentCheckRequest,
        timeout: Duration,
    ) -> Result<EnvironmentCheckResult, BrokerError> {
        if !matches!(
            self.owner,
            ResourceOwner::Environment | ResourceOwner::Supervisor | ResourceOwner::Verifier
        ) {
            return Err(BrokerError::Invalid(
                "environment checks require an environment, supervisor or verifier owner token"
                    .into(),
            ));
        }
        input.validate_shape().map_err(BrokerError::Invalid)?;
        if timeout < Duration::from_secs(u64::from(input.check.timeout_secs)) {
            return Err(BrokerError::Invalid(
                "environment check client timeout is shorter than the frozen check timeout".into(),
            ));
        }
        let response = self
            .client
            .post(self.join(ENVIRONMENT_CHECK_ROUTE)?)
            .bearer_auth(&self.token)
            .json(input)
            .timeout(timeout)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        if let Some(length) = response.content_length()
            && length as usize > MAX_ENVIRONMENT_CHECK_RESPONSE_BYTES
        {
            return Err(BrokerError::InvalidResponse(
                "environment check response exceeds the broker bound".into(),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_ENVIRONMENT_CHECK_RESPONSE_BYTES {
            return Err(BrokerError::InvalidResponse(
                "environment check response exceeds the broker bound".into(),
            ));
        }
        let result: EnvironmentCheckResult = serde_json::from_slice(&bytes).map_err(|_| {
            BrokerError::InvalidResponse("environment check response is invalid".into())
        })?;
        result
            .validate_for(input)
            .map_err(BrokerError::InvalidResponse)?;
        Ok(result)
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

    /// Request a bounded, credential-free dependency acquisition against a
    /// managed workspace volume. The broker owns the image, argv, network and
    /// timeout bounds; this client only supplies validated plan fields.
    pub async fn acquire_dependencies(
        &self,
        input: &crate::dependency_service::DependencyAcquireWireRequest,
    ) -> Result<crate::dependency_service::DependencyAcquireReceipt, BrokerError> {
        if input.timeout_secs == 0 {
            return Err(BrokerError::Invalid(
                "dependency acquisition timeout must be positive".into(),
            ));
        }
        let response = self
            .client
            .post(self.join(crate::dependency_service::DEPENDENCY_ACQUIRE_ROUTE)?)
            .bearer_auth(&self.token)
            .json(input)
            .timeout(Duration::from_secs(input.timeout_secs + 90))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(self.refusal(response).await);
        }
        if let Some(length) = response.content_length()
            && length as usize > MAX_DEPENDENCY_RESPONSE_BYTES
        {
            return Err(BrokerError::InvalidResponse(
                "dependency receipt exceeds the broker bound".into(),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > MAX_DEPENDENCY_RESPONSE_BYTES {
            return Err(BrokerError::InvalidResponse(
                "dependency receipt exceeds the broker bound".into(),
            ));
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            BrokerError::InvalidResponse("dependency receipt could not be decoded".into())
        })
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
        let bounded: String = body.chars().take(MAX_ERROR_BYTES).collect();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&bounded)
            && let (Some(code), Some(message)) = (
                value.get("code").and_then(serde_json::Value::as_str),
                value.get("error").and_then(serde_json::Value::as_str),
            )
        {
            return BrokerError::RefusedCode {
                status: status.as_u16(),
                code: code.chars().take(128).collect(),
                message: message.chars().take(MAX_ERROR_BYTES).collect(),
            };
        }
        BrokerError::Refused(format!("HTTP {status}: {bounded}"))
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
    fn workspace_file_reads_reject_symlink_escape_and_oversize() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("factory-env-workspace-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("factory-env-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("flake.nix"), b"{}\n").unwrap();
        std::fs::write(outside.join("secret"), b"secret").unwrap();
        assert_eq!(
            read_workspace_environment_file(&root, "flake.nix").unwrap(),
            "{}\n"
        );

        symlink(outside.join("secret"), root.join("escape")).unwrap();
        assert!(read_workspace_environment_file(&root, "escape").is_err());

        symlink(&outside, root.join("outside-dir")).unwrap();
        assert!(read_workspace_environment_file(&root, "outside-dir/secret").is_err());

        std::fs::write(
            root.join("huge"),
            vec![b'a'; MAX_ENVIRONMENT_WORKSPACE_FILE_BYTES + 1],
        )
        .unwrap();
        assert!(read_workspace_environment_file(&root, "huge").is_err());
        assert!(read_workspace_environment_file(&root, "../secret").is_err());
        assert!(read_workspace_environment_file(&root, "/etc/passwd").is_err());

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn broker_url_and_token_are_strict() {
        let client = RuntimeBrokerClient::new(
            "http://runtime:8770",
            "x".repeat(32),
            ResourceOwner::Supervisor,
        )
        .unwrap();
        assert_eq!(client.owner(), ResourceOwner::Supervisor);
        assert!(
            RuntimeBrokerClient::new(
                "http://runtime:8770/path",
                "x".repeat(32),
                ResourceOwner::Supervisor,
            )
            .is_err()
        );
        assert!(
            RuntimeBrokerClient::new(
                "https://runtime:8770",
                "x".repeat(32),
                ResourceOwner::Supervisor,
            )
            .is_err()
        );
        assert!(
            RuntimeBrokerClient::new(
                "http://runtime:8770",
                "short".into(),
                ResourceOwner::Supervisor,
            )
            .is_err()
        );
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
        assert_eq!(
            ResourceOwner::from_label("verifier"),
            Some(ResourceOwner::Verifier)
        );
        assert_eq!(ResourceOwner::from_label("worker"), None);
    }

    #[test]
    fn resource_ownership_requires_managed_and_exact_owner_labels() {
        let mut labels = BTreeMap::new();
        assert!(!resource_owner_matches(&labels, ResourceOwner::Supervisor));
        labels.insert(MANAGED_LABEL_KEY.into(), "true".into());
        assert!(!resource_owner_matches(&labels, ResourceOwner::Supervisor));
        labels.insert(OWNER_LABEL_KEY.into(), "verifier".into());
        assert!(!resource_owner_matches(&labels, ResourceOwner::Supervisor));
        assert!(resource_owner_matches(&labels, ResourceOwner::Verifier));
        labels.insert(OWNER_LABEL_KEY.into(), "worker".into());
        assert!(!resource_owner_matches(&labels, ResourceOwner::Verifier));
        labels.insert(MANAGED_LABEL_KEY.into(), "false".into());
        labels.insert(OWNER_LABEL_KEY.into(), "supervisor".into());
        assert!(!resource_owner_matches(&labels, ResourceOwner::Supervisor));
    }

    #[test]
    fn volume_identity_requires_owner_and_workspace_match() {
        let mut labels = BTreeMap::new();
        labels.insert(MANAGED_LABEL_KEY.into(), "true".into());
        labels.insert(OWNER_LABEL_KEY.into(), "supervisor".into());
        labels.insert(WORKSPACE_LABEL_KEY.into(), "workspace-a".into());
        assert!(volume_identity_matches(
            &labels,
            ResourceOwner::Supervisor,
            "workspace-a"
        ));
        assert!(!volume_identity_matches(
            &labels,
            ResourceOwner::Supervisor,
            "workspace-b"
        ));
        assert!(!volume_identity_matches(
            &labels,
            ResourceOwner::Verifier,
            "workspace-a"
        ));
        labels.remove(OWNER_LABEL_KEY);
        assert!(!volume_identity_matches(
            &labels,
            ResourceOwner::Supervisor,
            "workspace-a"
        ));
        assert!(legacy_workspace_identity_matches(&labels, "workspace-a"));
        assert!(!legacy_workspace_identity_matches(&labels, "workspace-b"));
        labels.insert(OWNER_LABEL_KEY.into(), String::new());
        assert!(legacy_workspace_identity_matches(&labels, "workspace-a"));
        labels.insert(OWNER_LABEL_KEY.into(), "supervisor".into());
        assert!(!legacy_workspace_identity_matches(&labels, "workspace-a"));
        labels.insert(MANAGED_LABEL_KEY.into(), "false".into());
        assert!(!legacy_workspace_identity_matches(&labels, "workspace-a"));
    }

    #[test]
    fn inspect_labels_and_bounded_streams_fail_closed() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let inspect = format!(
            r#"[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]"#
        );
        let labels = parse_volume_labels(inspect.as_bytes()).unwrap();
        assert!(resource_owner_matches(&labels, ResourceOwner::Supervisor));

        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let inspect = format!(
            r#"[{{"Name":"{container}","Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"verifier"}}}}}}]"#
        );
        let labels = parse_container_labels(inspect.as_bytes()).unwrap();
        assert!(resource_owner_matches(&labels, ResourceOwner::Verifier));

        assert!(parse_volume_labels(b"[]").is_err());
        assert!(parse_container_labels(b"[]").is_err());

        let mut bytes = Vec::new();
        assert!(extend_bounded(&mut bytes, b"1234", 4));
        assert!(!extend_bounded(&mut bytes, b"5", 4));
        assert_eq!(bytes, b"1234");
    }
}
