//! Typed dependency-acquisition operation for the runtime broker.
//!
//! The broker accepts only a bounded, authenticated request that names a
//! managed workspace volume and one configured dependency image. It launches a
//! dedicated, credential-free, network-enabled ephemeral container with a
//! fixed argv; the check container remains `--network=none --offline`. The
//! container writes the vendor tree and its evidence through the atomic path in
//! [`crate::dependency_broker`], and the broker validates the returned receipt
//! before exposing it to a trusted caller.
//!
//! This module does not own authorization. Core authorizes the operation and
//! the Supervisor binds it to an exact request digest; the broker only enforces
//! its own narrow transport, image, path, command and resource bounds.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

use crate::runtime_broker::{is_valid_dependency_container_name, is_valid_volume_name};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time,
};

use crate::dependencies::registry_allowed;
use crate::dependency_broker::{
    ADVISORY_FILE, EVIDENCE_DIR, LICENSE_FILE, MAX_VENDOR_ENTRIES, SBOM_FILE,
};

pub const DEPENDENCY_ACQUIRE_ROUTE: &str = "/v1/dependencies/acquire";
pub const MAX_DEPENDENCY_BODY_BYTES: usize = 64 * 1024;
pub const MAX_DEPENDENCY_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_DEPENDENCY_TIMEOUT: Duration = Duration::from_secs(900);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum DependencyAcquireError {
    #[error("dependency acquisition failed: {0}")]
    Command(String),
    #[error("dependency acquisition timed out")]
    Timeout,
    #[error("dependency acquisition receipt is invalid: {0}")]
    InvalidReceipt(String),
}

/// Acquisition request fields that are bound by the Core operation grant and
/// sent to the broker. The broker adds the workspace volume, container identity
/// and configured image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyAcquireRequest {
    pub workdir: String,
    pub lockfile_path: String,
    pub lockfile_sha256: String,
    pub vendor_path: String,
    pub registry: String,
    pub timeout_secs: u64,
}

impl DependencyAcquireRequest {
    /// Canonical request digest used by the Supervisor operation grant. It
    /// binds every field the broker will execute.
    pub fn digest(&self) -> Result<String, String> {
        crate::attempt_grant::request_digest(
            &serde_json::to_value(self)
                .map_err(|_| "dependency acquisition request encoding failed")?,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyAcquireWireRequest {
    pub volume: String,
    pub container: String,
    pub workdir: String,
    pub lockfile_path: String,
    pub lockfile_sha256: String,
    pub vendor_path: String,
    pub registry: String,
    pub timeout_secs: u64,
}

impl DependencyAcquireWireRequest {
    pub fn request(&self) -> DependencyAcquireRequest {
        DependencyAcquireRequest {
            workdir: self.workdir.clone(),
            lockfile_path: self.lockfile_path.clone(),
            lockfile_sha256: self.lockfile_sha256.clone(),
            vendor_path: self.vendor_path.clone(),
            registry: self.registry.clone(),
            timeout_secs: self.timeout_secs,
        }
    }

    pub fn digest(&self) -> Result<String, String> {
        self.request().digest()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyAcquireReceipt {
    pub image: String,
    pub lockfile_sha256: String,
    pub vendor_sha256: String,
    pub vendor_entries: usize,
    pub sbom_path: String,
    pub sbom_sha256: String,
    pub license_path: String,
    pub license_sha256: String,
    pub advisory_path: String,
    pub advisory_sha256: String,
    pub staging_cleaned: bool,
}

impl DependencyAcquireReceipt {
    /// Convert the broker receipt into the candidate-bound evidence contract.
    /// The paths are workspace-relative because the vendor tree is promoted
    /// under the workspace volume.
    pub fn evidence(&self) -> crate::dependencies::DependencyEvidence {
        crate::dependencies::DependencyEvidence {
            lockfile_sha256: self.lockfile_sha256.clone(),
            vendor_sha256: self.vendor_sha256.clone(),
            sbom_ref: self.sbom_path.clone(),
            sbom_sha256: self.sbom_sha256.clone(),
            license_report_ref: self.license_path.clone(),
            license_report_sha256: self.license_sha256.clone(),
            advisory_report_ref: self.advisory_path.clone(),
            advisory_report_sha256: self.advisory_sha256.clone(),
        }
    }
}

pub trait DependencyAcquirer: Send + Sync {
    fn acquire<'a>(
        &'a self,
        volume: &'a str,
        container: &'a str,
        image: &'a str,
        request: &'a DependencyAcquireRequest,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<DependencyAcquireReceipt, DependencyAcquireError>>
                + Send
                + 'a,
        >,
    >;
}

pub struct DependencyService {
    token: String,
    image: String,
    registries: Vec<String>,
    acquirer: std::sync::Arc<dyn DependencyAcquirer>,
    guard: Option<std::sync::Arc<dyn DependencyGuard>>,
}

/// Broker-owned admission check for one acquisition. The returned value is
/// dropped after the acquisition finishes; a broker implementation uses it to
/// release its active-volume registration. The guard runs after authentication
/// and shape validation so a caller cannot bypass the broker's owner and
/// per-volume exclusion checks by addressing the dependency route directly.
pub trait DependencyGuard: Send + Sync {
    fn enter<'a>(
        &'a self,
        volume: &'a str,
        container: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Box<dyn std::any::Any + Send>, String>> + Send + 'a>>;
}

impl std::fmt::Debug for DependencyService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DependencyService")
            .field("image", &self.image)
            .field("registries", &self.registries)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl DependencyService {
    pub fn new(
        token: String,
        image: String,
        registries: Vec<String>,
        acquirer: std::sync::Arc<dyn DependencyAcquirer>,
    ) -> Result<Self, DependencyAcquireError> {
        if token.len() < 32 {
            return Err(DependencyAcquireError::Command(
                "dependency service token is too short".into(),
            ));
        }
        if !valid_image(&image) {
            return Err(DependencyAcquireError::Command(
                "dependency image is invalid".into(),
            ));
        }
        if registries.is_empty() || registries.iter().any(|registry| registry.trim().is_empty()) {
            return Err(DependencyAcquireError::Command(
                "dependency registry allowlist must not be empty".into(),
            ));
        }
        Ok(Self {
            token,
            image,
            registries,
            acquirer,
            guard: None,
        })
    }

    pub fn with_guard(mut self, guard: std::sync::Arc<dyn DependencyGuard>) -> Self {
        self.guard = Some(guard);
        self
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    pub fn router(self: std::sync::Arc<Self>) -> Router {
        Router::new()
            .route(DEPENDENCY_ACQUIRE_ROUTE, post(acquire))
            .layer(DefaultBodyLimit::max(MAX_DEPENDENCY_BODY_BYTES))
            .with_state(self)
    }

    fn validate(&self, input: &DependencyAcquireWireRequest) -> Result<(), String> {
        if !is_valid_volume_name(&input.volume) {
            return Err("invalid workspace volume identity".into());
        }
        if !is_valid_dependency_container_name(&input.container) {
            return Err("invalid dependency container identity".into());
        }
        validate_request_fields(&self.registries, &input.request())
    }

    fn validate_receipt(
        &self,
        input: &DependencyAcquireWireRequest,
        receipt: &DependencyAcquireReceipt,
    ) -> Result<(), DependencyAcquireError> {
        if receipt.image != self.image {
            return Err(DependencyAcquireError::InvalidReceipt(
                "receipt image does not match the configured dependency image".into(),
            ));
        }
        if receipt.lockfile_sha256 != input.lockfile_sha256 {
            return Err(DependencyAcquireError::InvalidReceipt(
                "receipt lockfile digest does not match the request".into(),
            ));
        }
        if !valid_sha256(&receipt.vendor_sha256) {
            return Err(DependencyAcquireError::InvalidReceipt(
                "receipt vendor digest is not SHA-256".into(),
            ));
        }
        if receipt.vendor_entries > MAX_VENDOR_ENTRIES {
            return Err(DependencyAcquireError::InvalidReceipt(
                "receipt vendor entry count exceeds the bound".into(),
            ));
        }
        if !receipt.staging_cleaned {
            return Err(DependencyAcquireError::InvalidReceipt(
                "receipt reports an unclean staging directory".into(),
            ));
        }
        let prefix = format!("{}/{EVIDENCE_DIR}", input.vendor_path);
        for (label, path, digest, expected_file) in [
            (
                "sbom",
                receipt.sbom_path.as_str(),
                receipt.sbom_sha256.as_str(),
                SBOM_FILE,
            ),
            (
                "license",
                receipt.license_path.as_str(),
                receipt.license_sha256.as_str(),
                LICENSE_FILE,
            ),
            (
                "advisory",
                receipt.advisory_path.as_str(),
                receipt.advisory_sha256.as_str(),
                ADVISORY_FILE,
            ),
        ] {
            if !is_safe_relative_path(path) {
                return Err(DependencyAcquireError::InvalidReceipt(format!(
                    "{label} evidence path is unsafe"
                )));
            }
            if path != format!("{prefix}/{expected_file}") {
                return Err(DependencyAcquireError::InvalidReceipt(format!(
                    "{label} evidence path is outside the vendor evidence directory"
                )));
            }
            if !valid_sha256(digest) {
                return Err(DependencyAcquireError::InvalidReceipt(format!(
                    "{label} evidence digest is not SHA-256"
                )));
            }
        }
        Ok(())
    }
}

async fn acquire(
    State(service): State<std::sync::Arc<DependencyService>>,
    headers: HeaderMap,
    Json(input): Json<DependencyAcquireWireRequest>,
) -> Response {
    if !authorized(&headers, &service.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(message) = service.validate(&input) {
        return bad_request(&message);
    }
    let _guard = match &service.guard {
        Some(guard) => match guard.enter(&input.volume, &input.container).await {
            Ok(guard) => Some(guard),
            Err(message) => return conflict(&message),
        },
        None => None,
    };
    tracing::info!(
        container = %input.container,
        volume = %input.volume,
        "dependency acquisition requested"
    );
    let request = input.request();
    match service
        .acquirer
        .acquire(&input.volume, &input.container, &service.image, &request)
        .await
    {
        Ok(receipt) => match service.validate_receipt(&input, &receipt) {
            Ok(()) => {
                tracing::info!(container = %input.container, "dependency acquisition finished");
                (StatusCode::OK, Json(receipt)).into_response()
            }
            Err(error) => operation_error(error),
        },
        Err(error) => operation_error(error),
    }
}

fn validate_request_fields(
    registries: &[String],
    request: &DependencyAcquireRequest,
) -> Result<(), String> {
    validate_request_shape(request)?;
    if !registry_allowed(&request.registry, registries) {
        return Err("dependency registry is not allowlisted".into());
    }
    Ok(())
}

/// Shape validation shared by the Supervisor and the broker. Registry
/// allowlisting is a broker policy and remains separate.
pub fn validate_request_shape(request: &DependencyAcquireRequest) -> Result<(), String> {
    if !safe_workdir(&request.workdir) {
        return Err("dependency workdir is unsafe".into());
    }
    if !is_safe_relative_path(&request.lockfile_path)
        || !request.lockfile_path.ends_with("Cargo.lock")
    {
        return Err("dependency lockfile path is unsafe".into());
    }
    if !is_safe_relative_path(&request.vendor_path) {
        return Err("dependency vendor path is unsafe".into());
    }
    if !valid_sha256(&request.lockfile_sha256) {
        return Err("dependency lockfile digest is not SHA-256".into());
    }
    if request.registry.trim().is_empty() {
        return Err("dependency registry is required".into());
    }
    if request.timeout_secs == 0
        || Duration::from_secs(request.timeout_secs) > MAX_DEPENDENCY_TIMEOUT
    {
        return Err("dependency timeout is outside the allowed bound".into());
    }
    Ok(())
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    constant_time_eq(value.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn bad_request(message: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": message})),
    )
        .into_response()
}

fn operation_error(error: DependencyAcquireError) -> Response {
    let message: String = error.to_string().chars().take(1024).collect();
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({"error": message})),
    )
        .into_response()
}

fn conflict(message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"error": message})),
    )
        .into_response()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_image(image: &str) -> bool {
    !image.is_empty()
        && !image.starts_with('-')
        && !image.chars().any(char::is_whitespace)
        && !image.contains("://")
}

fn valid_image_id(value: &str) -> bool {
    normalize_image_id(value).is_some()
}

/// Normalize a Podman image identity to `sha256:<64 hex>`. Podman's `{{.Id}}`
/// prints the bare config digest for locally imported images.
pub fn normalize_image_id(value: &str) -> Option<String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("sha256:{hex}"))
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains(':')
        && !value.contains('"')
        && !value.contains('\'')
        && value.chars().all(|character| !character.is_control())
        && value
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn safe_workdir(value: &str) -> bool {
    value.is_empty() || value == "." || is_safe_relative_path(value)
}

#[derive(Clone)]
pub struct DependencyEgressConfig {
    pub network: String,
    pub proxy_url: String,
    pub ready: Arc<AtomicBool>,
}

/// Production acquirer. The dependency image is pinned by its exact image ID
/// and must be reached only through the deny-by-default egress proxy on the
/// dedicated internal network. The container receives no request-controlled
/// environment, no host mount and no credential.
pub struct PodmanDependencyAcquirer {
    podman: PathBuf,
    image_id: String,
    egress: Option<DependencyEgressConfig>,
}

impl PodmanDependencyAcquirer {
    pub fn new(podman: PathBuf, image_id: String) -> Result<Self, DependencyAcquireError> {
        if !podman.is_absolute() || !podman.is_file() {
            return Err(DependencyAcquireError::Command(
                "podman binary must be an absolute existing file".into(),
            ));
        }
        if !valid_image_id(&image_id) {
            return Err(DependencyAcquireError::Command(
                "dependency image ID must be sha256:<64 hex>".into(),
            ));
        }
        Ok(Self {
            podman,
            image_id,
            egress: None,
        })
    }

    pub fn with_egress(
        mut self,
        network: String,
        proxy_url: String,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, DependencyAcquireError> {
        if network.is_empty()
            || network.len() > 64
            || !network
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(DependencyAcquireError::Command(
                "dependency egress network is invalid".into(),
            ));
        }
        let url = reqwest::Url::parse(&proxy_url).map_err(|_| {
            DependencyAcquireError::Command("dependency egress proxy URL is invalid".into())
        })?;
        if url.scheme() != "http"
            || url.host_str().is_none()
            || url.port().is_none()
            || !matches!(url.path(), "" | "/")
            || url.username() != ""
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(DependencyAcquireError::Command(
                "dependency egress proxy must be a plain HTTP origin with an explicit port".into(),
            ));
        }
        self.egress = Some(DependencyEgressConfig {
            network,
            proxy_url,
            ready,
        });
        Ok(self)
    }

    async fn remove_container(&self, container: &str) {
        let _ = time::timeout(
            COMMAND_TIMEOUT,
            self.command()
                .args(["rm", "--force", "--time=0", container])
                .output(),
        )
        .await;
    }

    async fn verify_image(&self, image: &str) -> Result<(), DependencyAcquireError> {
        let output = time::timeout(
            COMMAND_TIMEOUT,
            self.command()
                .args(["image", "inspect", "--format", "{{.Id}}", image])
                .output(),
        )
        .await
        .map_err(|_| {
            DependencyAcquireError::Command("dependency image inspection timed out".into())
        })?
        .map_err(|error| {
            DependencyAcquireError::Command(format!("dependency image inspection failed: {error}"))
        })?;
        if !output.status.success() {
            return Err(DependencyAcquireError::Command(
                "dependency image is not available in the engine".into(),
            ));
        }
        let actual = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let Some(actual) = normalize_image_id(&actual) else {
            return Err(DependencyAcquireError::Command(
                "dependency image returned an invalid identity".into(),
            ));
        };
        if actual != self.image_id {
            return Err(DependencyAcquireError::Command(format!(
                "dependency image identity mismatch: expected {}, observed {actual}",
                self.image_id
            )));
        }
        Ok(())
    }

    fn command(&self) -> Command {
        // Reuse only the non-secret process home/temp paths so the same
        // acquirer works in the runtime container and in a host smoke test.
        // No broker, provider or registry credential is forwarded.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let tmpdir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into());
        let mut command = Command::new(&self.podman);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home)
            .env("TMPDIR", tmpdir)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }
}

impl DependencyAcquirer for PodmanDependencyAcquirer {
    fn acquire<'a>(
        &'a self,
        volume: &'a str,
        container: &'a str,
        image: &'a str,
        request: &'a DependencyAcquireRequest,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<DependencyAcquireReceipt, DependencyAcquireError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            if !is_valid_volume_name(volume)
                || !is_valid_dependency_container_name(container)
                || !valid_image(image)
            {
                return Err(DependencyAcquireError::Command(
                    "dependency acquisition identity is invalid".into(),
                ));
            }
            let Some(egress) = &self.egress else {
                return Err(DependencyAcquireError::Command(
                    "deny-by-default dependency egress proxy is not configured".into(),
                ));
            };
            if !egress.ready.load(Ordering::SeqCst) {
                return Err(DependencyAcquireError::Command(
                    "dependency egress proxy is unavailable".into(),
                ));
            }
            self.verify_image(image).await?;
            let args = dependency_command_args(
                volume,
                container,
                image,
                request,
                &egress.network,
                &egress.proxy_url,
            );
            let mut child = self
                .command()
                .args(args.iter().map(String::as_str))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    DependencyAcquireError::Command(format!(
                        "dependency container could not start: {error}"
                    ))
                })?;
            let Some(stdout) = child.stdout.take() else {
                self.remove_container(container).await;
                return Err(DependencyAcquireError::Command(
                    "dependency container stdout unavailable".into(),
                ));
            };
            let Some(stderr) = child.stderr.take() else {
                self.remove_container(container).await;
                return Err(DependencyAcquireError::Command(
                    "dependency container stderr unavailable".into(),
                ));
            };
            let stdout_task = tokio::spawn(read_bounded(stdout, MAX_DEPENDENCY_OUTPUT_BYTES));
            let stderr_task = tokio::spawn(read_bounded(stderr, MAX_DEPENDENCY_OUTPUT_BYTES));
            let status = match time::timeout(
                Duration::from_secs(request.timeout_secs) + Duration::from_secs(30),
                child.wait(),
            )
            .await
            {
                Ok(Ok(status)) => status,
                Ok(Err(error)) => {
                    self.remove_container(container).await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Err(DependencyAcquireError::Command(format!(
                        "dependency container wait failed: {error}"
                    )));
                }
                Err(_) => {
                    let _ = child.kill().await;
                    self.remove_container(container).await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Err(DependencyAcquireError::Timeout);
                }
            };
            self.remove_container(container).await;
            let (stdout, stdout_truncated) = stdout_task
                .await
                .map_err(|error| {
                    DependencyAcquireError::Command(format!("dependency stdout reader: {error}"))
                })?
                .map_err(|error| {
                    DependencyAcquireError::Command(format!("dependency stdout reader: {error}"))
                })?;
            let (stderr, stderr_truncated) = stderr_task
                .await
                .map_err(|error| {
                    DependencyAcquireError::Command(format!("dependency stderr reader: {error}"))
                })?
                .map_err(|error| {
                    DependencyAcquireError::Command(format!("dependency stderr reader: {error}"))
                })?;
            if !status.success() {
                let message = crate::telemetry::redact(&String::from_utf8_lossy(&stderr));
                return Err(DependencyAcquireError::Command(format!(
                    "dependency container exited with {status}: {}",
                    message.chars().take(2_000).collect::<String>()
                )));
            }
            if stdout_truncated || stderr_truncated {
                return Err(DependencyAcquireError::InvalidReceipt(
                    "dependency container output exceeded the broker bound".into(),
                ));
            }
            serde_json::from_slice(&stdout).map_err(|error| {
                DependencyAcquireError::InvalidReceipt(format!(
                    "dependency receipt could not be decoded: {error}"
                ))
            })
        })
    }
}

/// Exact argv passed to the dependency image. It is a fixed command with
/// validated positional values; no Podman flag, image or environment comes
/// from the request.
pub fn dependency_command_args(
    volume: &str,
    container: &str,
    image: &str,
    request: &DependencyAcquireRequest,
    egress_network: &str,
    egress_proxy_url: &str,
) -> Vec<String> {
    let workdir = if request.workdir.is_empty() || request.workdir == "." {
        "."
    } else {
        request.workdir.as_str()
    };
    vec![
        "run".into(),
        "--rm".into(),
        format!("--name={container}"),
        "--label=io.software-factory.managed=true".into(),
        format!("--label=io.software-factory.workspace={volume}"),
        "--label=io.software-factory.dependency=acquire".into(),
        "--pull=never".into(),
        format!("--network={egress_network}"),
        "--read-only".into(),
        "--tmpfs=/tmp:rw,noexec,nosuid,size=1g".into(),
        "--tmpfs=/cargo-home:rw,nodev,nosuid,size=4g".into(),
        "--cap-drop=all".into(),
        "--security-opt=no-new-privileges".into(),
        "--ulimit=fsize=67108864".into(),
        "--pids-limit=256".into(),
        "--memory=4g".into(),
        "--cpus=2".into(),
        "--workdir=/workspace".into(),
        "--env=HOME=/tmp".into(),
        "--env=TMPDIR=/tmp".into(),
        "--env=CARGO_HOME=/cargo-home".into(),
        "--env=CARGO_NET_OFFLINE=false".into(),
        "--env=CARGO_TERM_COLOR=never".into(),
        "--env=RUSTUP_HOME=/usr/local/rustup".into(),
        format!("--env=FACTORY_DEPENDENCY_IMAGE={image}"),
        format!("--env=HTTPS_PROXY={egress_proxy_url}"),
        format!("--env=HTTP_PROXY={egress_proxy_url}"),
        format!("--env=ALL_PROXY={egress_proxy_url}"),
        format!("--env=CARGO_HTTP_PROXY={egress_proxy_url}"),
        "--env=NO_PROXY=".into(),
        format!("--env=FACTORY_DEPENDENCY_EGRESS_PROXY={egress_proxy_url}"),
        format!("--volume={volume}:/workspace:rw"),
        image.into(),
        "--workdir".into(),
        workdir.into(),
        "--lockfile".into(),
        request.lockfile_path.clone(),
        "--lockfile-sha256".into(),
        request.lockfile_sha256.clone(),
        "--vendor".into(),
        request.vendor_path.clone(),
        "--registry".into(),
        request.registry.clone(),
    ]
}

async fn read_bounded<R>(mut reader: R, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        if captured.len() < limit {
            let remaining = limit - captured.len();
            captured.extend_from_slice(&chunk[..read.min(remaining)]);
            if read > remaining {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }
    Ok((captured, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies::DEFAULT_ALLOWED_REGISTRIES;

    struct NoopAcquirer;

    impl DependencyAcquirer for NoopAcquirer {
        fn acquire<'a>(
            &'a self,
            _volume: &'a str,
            _container: &'a str,
            _image: &'a str,
            _request: &'a DependencyAcquireRequest,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<DependencyAcquireReceipt, DependencyAcquireError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                Err(DependencyAcquireError::Command(
                    "fixture acquirer is not configured".into(),
                ))
            })
        }
    }

    fn service() -> DependencyService {
        DependencyService::new(
            "dependency-service-token-0123456789abcdef".into(),
            "localhost/software-factory-dependency:dev".into(),
            DEFAULT_ALLOWED_REGISTRIES
                .iter()
                .map(|registry| (*registry).to_string())
                .collect(),
            std::sync::Arc::new(NoopAcquirer),
        )
        .unwrap()
    }

    fn wire_request() -> DependencyAcquireWireRequest {
        DependencyAcquireWireRequest {
            volume: "factory-ws-123e4567-e89b-12d3-a456-426614174000".into(),
            container: "factory-deps-123e4567-e89b-12d3-a456-426614174000".into(),
            workdir: "project".into(),
            lockfile_path: "Cargo.lock".into(),
            lockfile_sha256: "a".repeat(64),
            vendor_path: "vendor".into(),
            registry: "sparse+https://index.crates.io/".into(),
            timeout_secs: 600,
        }
    }

    fn receipt() -> DependencyAcquireReceipt {
        DependencyAcquireReceipt {
            image: "localhost/software-factory-dependency:dev".into(),
            lockfile_sha256: "a".repeat(64),
            vendor_sha256: "b".repeat(64),
            vendor_entries: 4,
            sbom_path: format!("vendor/{EVIDENCE_DIR}/{SBOM_FILE}"),
            sbom_sha256: "c".repeat(64),
            license_path: format!("vendor/{EVIDENCE_DIR}/{LICENSE_FILE}"),
            license_sha256: "d".repeat(64),
            advisory_path: format!("vendor/{EVIDENCE_DIR}/{ADVISORY_FILE}"),
            advisory_sha256: "e".repeat(64),
            staging_cleaned: true,
        }
    }

    #[test]
    fn wire_request_rejects_unknown_and_credential_fields() {
        let value = serde_json::to_value(wire_request()).unwrap();
        let mut object = value.as_object().unwrap().clone();
        object.insert(
            "token".into(),
            serde_json::Value::String("provider-secret".into()),
        );
        assert!(serde_json::from_value::<DependencyAcquireWireRequest>(object.into()).is_err());
    }

    #[test]
    fn validation_rejects_unsafe_paths_registry_and_bounds() {
        let service = service();
        assert!(service.validate(&wire_request()).is_ok());

        for workdir in ["../escape", "/abs", "a/../b", "a//b"] {
            let mut input = wire_request();
            input.workdir = workdir.into();
            assert!(service.validate(&input).is_err(), "workdir {workdir}");
        }
        let mut input = wire_request();
        input.lockfile_path = "../Cargo.lock".into();
        assert!(service.validate(&input).is_err());
        let mut input = wire_request();
        input.vendor_path = "vendor/../../escape".into();
        assert!(service.validate(&input).is_err());
        let mut input = wire_request();
        input.lockfile_sha256 = "z".repeat(64);
        assert!(service.validate(&input).is_err());
        let mut input = wire_request();
        input.registry = "https://example.invalid/index".into();
        assert!(service.validate(&input).is_err());
        let mut input = wire_request();
        input.timeout_secs = MAX_DEPENDENCY_TIMEOUT.as_secs() + 1;
        assert!(service.validate(&input).is_err());
        let mut input = wire_request();
        input.volume = "host-volume".into();
        assert!(service.validate(&input).is_err());
        let mut input = wire_request();
        input.container = "factory-exec-123e4567-e89b-12d3-a456-426614174000".into();
        assert!(service.validate(&input).is_err());
    }

    #[test]
    fn receipt_validation_binds_request_image_and_evidence_paths() {
        let service = service();
        let input = wire_request();
        assert!(service.validate_receipt(&input, &receipt()).is_ok());

        let mut mismatch = receipt();
        mismatch.lockfile_sha256 = "f".repeat(64);
        assert!(service.validate_receipt(&input, &mismatch).is_err());

        let mut escaped = receipt();
        escaped.sbom_path = "../sbom.json".into();
        assert!(service.validate_receipt(&input, &escaped).is_err());

        let mut wrong_directory = receipt();
        wrong_directory.sbom_path = format!("other/{EVIDENCE_DIR}/{SBOM_FILE}");
        assert!(service.validate_receipt(&input, &wrong_directory).is_err());

        let mut bad_digest = receipt();
        bad_digest.advisory_sha256 = "short".into();
        assert!(service.validate_receipt(&input, &bad_digest).is_err());

        let mut dirty = receipt();
        dirty.staging_cleaned = false;
        assert!(service.validate_receipt(&input, &dirty).is_err());

        let mut wrong_image = receipt();
        wrong_image.image = "docker.io/evil:latest".into();
        assert!(service.validate_receipt(&input, &wrong_image).is_err());
    }

    #[test]
    fn podman_command_is_fixed_bounded_and_credential_free() {
        let args = dependency_command_args(
            "factory-ws-123e4567-e89b-12d3-a456-426614174000",
            "factory-deps-123e4567-e89b-12d3-a456-426614174000",
            "localhost/software-factory-dependency:dev",
            &wire_request().request(),
            "factory-deps-egress",
            "http://10.90.0.1:18080",
        );
        let joined = args.join(" ");
        assert!(joined.contains("--network=factory-deps-egress"));
        assert!(!joined.contains("--network=bridge"));
        assert!(joined.contains("--env=HTTPS_PROXY=http://10.90.0.1:18080"));
        assert!(joined.contains("--env=ALL_PROXY=http://10.90.0.1:18080"));
        assert!(joined.contains("--env=CARGO_HTTP_PROXY=http://10.90.0.1:18080"));
        assert!(joined.contains("--env=NO_PROXY="));
        for forbidden in [
            "--privileged",
            "--network=host",
            "--userns=host",
            "--volume=/",
            "CONTAINER_HOST",
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "API_KEY",
        ] {
            assert!(!joined.contains(forbidden), "forbidden token {forbidden}");
        }
        assert_eq!(
            args.iter()
                .filter(|argument| argument.starts_with("--volume="))
                .cloned()
                .collect::<Vec<_>>(),
            vec![
                "--volume=factory-ws-123e4567-e89b-12d3-a456-426614174000:/workspace:rw"
                    .to_string()
            ]
        );
        let env: Vec<_> = args
            .iter()
            .filter(|argument| argument.starts_with("--env="))
            .collect();
        assert_eq!(env.len(), 13);
        assert!(env.iter().all(|entry| !entry.contains("TOKEN")));
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--lockfile" && pair[1] == "Cargo.lock")
        );
        assert!(args.iter().any(|argument| {
            argument == "--name=factory-deps-123e4567-e89b-12d3-a456-426614174000"
        }));
    }

    #[test]
    fn image_identity_and_egress_configuration_are_strict() {
        let podman = std::env::current_exe().unwrap();
        let image_id = format!("sha256:{}", "a".repeat(64));
        let acquirer = PodmanDependencyAcquirer::new(podman.clone(), image_id.clone()).unwrap();
        assert!(
            acquirer
                .with_egress(
                    "factory-deps-egress".into(),
                    "http://10.90.0.1:18080".into(),
                    std::sync::Arc::new(AtomicBool::new(true)),
                )
                .is_ok()
        );
        assert!(PodmanDependencyAcquirer::new(podman.clone(), "not-a-digest".into()).is_err());
        assert!(
            PodmanDependencyAcquirer::new(podman.clone(), image_id.clone())
                .unwrap()
                .with_egress(
                    "bad/network".into(),
                    "http://10.90.0.1:18080".into(),
                    std::sync::Arc::new(AtomicBool::new(true)),
                )
                .is_err()
        );
        assert!(
            PodmanDependencyAcquirer::new(podman, image_id)
                .unwrap()
                .with_egress(
                    "factory-deps-egress".into(),
                    "https://10.90.0.1:18080".into(),
                    std::sync::Arc::new(AtomicBool::new(true)),
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn unauthorized_requests_are_rejected_before_body_authority() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let router = std::sync::Arc::new(service()).router();
        let body = serde_json::to_vec(&wire_request()).unwrap();
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEPENDENCY_ACQUIRE_ROUTE)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn oversized_bodies_are_bounded() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let router = std::sync::Arc::new(service()).router();
        let body = vec![b'x'; MAX_DEPENDENCY_BODY_BYTES + 1];
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(DEPENDENCY_ACQUIRE_ROUTE)
                    .header("content-type", "application/json")
                    .header(
                        "authorization",
                        "Bearer dependency-service-token-0123456789abcdef",
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
