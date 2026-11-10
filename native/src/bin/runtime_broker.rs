//! Narrow trusted runtime broker for the dedicated Factory execution engine.
//!
//! The broker owns the only interface to the nested Podman engine. It accepts
//! typed, policy-bounded operations from the trusted Supervisor, verifier and
//! provider runtime: create/remove named volumes, import/export workspace
//! archives, run an ephemeral network-disabled child container, and manage a
//! provider session container. It never exposes the Podman socket or CLI and
//! never accepts arbitrary Podman arguments, host paths or images.
//!
//! The nested engine runs inside the rootless outer container's user
//! namespace. It is a candidate TRUSTED_LOCAL boundary, not certified
//! isolation. See docs/security/container-runtime.md.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, RwLock},
    time::Duration,
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use factory_native::dependency_egress::{self, EgressAllowlist, PeerSubnet};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time,
};

use factory_native::runtime_broker::{
    MAX_RESOURCE_INVENTORY_BYTES, ManagedResources, ResourceOwner, is_valid_container_name,
    is_valid_exec_container_name, is_valid_provider_container_name, is_valid_volume_name,
    legacy_workspace_identity_matches, parse_container_inventory, parse_container_labels,
    parse_volume_inventory, parse_volume_labels, resource_owner_matches, volume_identity_matches,
};

const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_ARGV_ITEMS: usize = 256;
const MAX_ARG_BYTES: usize = 16 * 1024;
const MAX_ARGV_BYTES: usize = 64 * 1024;
const MAX_ENV_ITEMS: usize = 32;
const MAX_ENV_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_EXECUTION_RUNTIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_PROVIDER_RUNTIME: Duration = Duration::from_secs(60 * 60);
const MANAGED_LABEL: &str = "io.software-factory.managed=true";
const WORKSPACE_LABEL: &str = "io.software-factory.workspace";
const OWNER_LABEL: &str = "io.software-factory.owner";
const RESOURCE_LOCK_ENTRIES: usize = 4096;
const MAX_ACTIVE_EXECUTIONS: usize = 4096;
const DEPENDENCY_EGRESS_UNAVAILABLE: &str = "DEPENDENCY_EGRESS_UNAVAILABLE";
const DEPENDENCY_RUNTIME_UNAVAILABLE: &str = "DEPENDENCY_RUNTIME_UNAVAILABLE";
const MAX_DEPENDENCY_HEALTH_REASON: usize = 512;
const OWNER_TOKEN_FILES: [(ResourceOwner, &str); 3] = [
    (ResourceOwner::Supervisor, "runtime-broker-supervisor.token"),
    (ResourceOwner::Verifier, "runtime-broker-verifier.token"),
    (ResourceOwner::Provider, "runtime-broker-provider.token"),
];

const PROVIDER_ENV_KEYS: [&str; 19] = [
    "FACTORY_PROVIDER_SOCKET",
    "FACTORY_PROVIDER_TCP",
    "FACTORY_EGRESS_LISTEN_PORT",
    "FACTORY_EGRESS_SOCKET",
    "OPENAI_API_KEY",
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_SERVER_PASSWORD",
    "CODEX_HOME",
    "HOME",
    "TMPDIR",
    "PATH",
    "FACTORY_PROVIDER_KIND",
    "FACTORY_PROVIDER_MODEL",
    "FACTORY_FIXTURE_PROVIDER_DIR",
    "FACTORY_FIXTURE_FILE",
    "FACTORY_FIXTURE_CONTENT",
    "FACTORY_FIXTURE_PROVIDER_MODEL",
    "FACTORY_FIXTURE_PROVIDER_USE_EGRESS",
    "LANG",
];

#[derive(Clone)]
struct OwnerTokens {
    entries: Vec<(ResourceOwner, String)>,
}

impl OwnerTokens {
    fn from_directory(directory: &Path) -> Result<Self, String> {
        let mut entries: Vec<(ResourceOwner, String)> = Vec::new();
        for (owner, file) in OWNER_TOKEN_FILES {
            let path = directory.join(file);
            let token = std::fs::read_to_string(&path)
                .map_err(|_| format!("runtime broker owner token {} is unavailable", file))?
                .trim_end_matches(['\r', '\n'])
                .to_owned();
            if token.len() < 32 || token.len() > 4096 || token.contains('\0') {
                return Err(format!("runtime broker owner token {file} is invalid"));
            }
            if entries
                .iter()
                .any(|(_, existing)| constant_time_eq(existing.as_bytes(), token.as_bytes()))
            {
                return Err("runtime broker owner tokens must be distinct".into());
            }
            entries.push((owner, token));
        }
        Ok(Self { entries })
    }

    fn authenticate(&self, headers: &HeaderMap) -> Option<ResourceOwner> {
        let value = headers
            .get("authorization")?
            .to_str()
            .ok()?
            .strip_prefix("Bearer ")?;
        let mut matched = None;
        for (owner, token) in &self.entries {
            if constant_time_eq(value.as_bytes(), token.as_bytes()) {
                matched = Some(*owner);
            }
        }
        matched
    }

    fn token(&self, owner: ResourceOwner) -> Option<&str> {
        self.entries
            .iter()
            .find(|(candidate, _)| *candidate == owner)
            .map(|(_, token)| token.as_str())
    }
}

enum MutationError {
    Conflict(&'static str),
    Failed(String),
}

/// Active execution identity, registered under the volume lock for the short
/// validation window and removed only after confirmed container cleanup. The
/// registry reserves both the volume and the container name atomically. Volume
/// delete/reset/import/export checks this registry instead of holding a lock
/// across the execution lifetime.
struct ActiveExecutions {
    entries: std::sync::Mutex<ActiveEntries>,
}

#[derive(Default)]
struct ActiveEntries {
    volumes: HashMap<String, String>,
    containers: std::collections::HashSet<String>,
}

impl ActiveExecutions {
    fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(ActiveEntries::default()),
        }
    }

    fn register(&self, volume: &str, container: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        if entries.volumes.len() >= MAX_ACTIVE_EXECUTIONS
            || entries.volumes.contains_key(volume)
            || entries.containers.contains(container)
        {
            return false;
        }
        entries
            .volumes
            .insert(volume.to_owned(), container.to_owned());
        entries.containers.insert(container.to_owned());
        true
    }

    fn remove(&self, volume: &str, container: &str) {
        let mut entries = self.entries.lock().unwrap();
        if entries.volumes.get(volume).map(String::as_str) == Some(container) {
            entries.volumes.remove(volume);
            entries.containers.remove(container);
        }
    }

    fn is_active(&self, volume: &str) -> bool {
        self.entries.lock().unwrap().volumes.contains_key(volume)
    }
}

/// Exact per-resource-name async locks. The exact table is permanently bounded.
/// Entries are never reclaimed: a name that once fell back to the overflow lock
/// must keep that identity forever, because reclaiming an idle exact entry could
/// later create a second lock for the same overflowed name. Names beyond the
/// table capacity intentionally serialize on one overflow lock. Locks are never
/// held across a long execution; the active-execution registry provides volume
/// exclusion for the run lifetime.
struct ResourceLocks {
    entries: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    overflow: Arc<tokio::sync::Mutex<()>>,
}

impl ResourceLocks {
    fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(HashMap::new()),
            overflow: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn acquire_with_limit(&self, name: &str, limit: usize) -> Arc<tokio::sync::Mutex<()>> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(lock) = entries.get(name) {
            return Arc::clone(lock);
        }
        if entries.len() >= limit {
            return Arc::clone(&self.overflow);
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        entries.insert(name.to_owned(), Arc::clone(&lock));
        lock
    }
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict(message) => formatter.write_str(message),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DependencyAcquisitionState {
    Enabled {
        network: String,
        proxy_url: String,
        binding: &'static str,
    },
    Disabled {
        code: &'static str,
        reason: String,
    },
}

impl DependencyAcquisitionState {
    fn enabled(network: String, proxy_url: String, binding: &'static str) -> Self {
        Self::Enabled {
            network,
            proxy_url,
            binding,
        }
    }

    fn disabled(code: &'static str, reason: impl Into<String>) -> Self {
        Self::Disabled {
            code,
            reason: bounded_dependency_reason(&reason.into()),
        }
    }

    fn health_projection(&self) -> serde_json::Value {
        match self {
            Self::Enabled {
                network,
                proxy_url,
                binding,
            } => json!({
                "enabled": true,
                "network": network,
                "proxy": proxy_url,
                "binding": binding,
            }),
            Self::Disabled { code, reason } => json!({
                "enabled": false,
                "code": code,
                "reason": reason,
            }),
        }
    }
}

struct DependencySetup {
    state: DependencyAcquisitionState,
    service: Option<Arc<factory_native::dependency_service::DependencyService>>,
    proxy: Option<dependency_egress::RunningProxy>,
}

struct DependencySetupFailure {
    code: &'static str,
    reason: String,
}

impl DependencySetupFailure {
    fn new(code: &'static str, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

#[derive(Clone)]
struct App {
    tokens: Arc<OwnerTokens>,
    resource_locks: Arc<ResourceLocks>,
    lock_limit: usize,
    active: Arc<ActiveExecutions>,
    podman: PathBuf,
    images: Arc<BTreeSet<String>>,
    provider_images: Arc<BTreeSet<String>>,
    provider_kind: String,
    provider_session_root: PathBuf,
    import_images: Vec<(String, PathBuf)>,
    dependency_acquisition: Arc<RwLock<DependencyAcquisitionState>>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "factory_native=info,factory_runtime_broker=info",
                )
            }),
        )
        .with_writer(factory_native::telemetry::RedactingMakeWriter)
        .init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let token_dir = PathBuf::from(
        env::var("FACTORY_RUNTIME_BROKER_TOKEN_DIR").unwrap_or_else(|_| "/config".into()),
    );
    if !token_dir.is_absolute() {
        return Err("FACTORY_RUNTIME_BROKER_TOKEN_DIR must be absolute".into());
    }
    let tokens = OwnerTokens::from_directory(&token_dir)?;
    for (_, token) in &tokens.entries {
        factory_native::telemetry::register_secret(token);
    }
    let podman = env::var("FACTORY_PODMAN_BIN").unwrap_or_else(|_| "/usr/bin/podman".into());
    let podman = PathBuf::from(podman);
    if !podman.is_absolute() || !podman.is_file() {
        return Err("FACTORY_PODMAN_BIN must be an absolute existing file".into());
    }
    let images = parse_images(
        env::var("FACTORY_RUNTIME_IMAGES").map_err(|_| "FACTORY_RUNTIME_IMAGES required")?,
    )?;
    let provider_images = match env::var("FACTORY_RUNTIME_PROVIDER_IMAGES") {
        Ok(value) if !value.trim().is_empty() => parse_images(value)?,
        _ => BTreeSet::new(),
    };
    let provider_kind = env::var("FACTORY_RUNTIME_PROVIDER_KIND").unwrap_or_default();
    if !matches!(
        provider_kind.as_str(),
        "" | "opencode" | "codex" | "fixture"
    ) {
        return Err("FACTORY_RUNTIME_PROVIDER_KIND is unsupported".into());
    }
    if provider_kind.is_empty() != provider_images.is_empty() {
        return Err(
            "FACTORY_RUNTIME_PROVIDER_IMAGES and FACTORY_RUNTIME_PROVIDER_KIND must be set together"
                .into(),
        );
    }
    let provider_session_root = PathBuf::from(
        env::var("FACTORY_RUNTIME_PROVIDER_SESSION_ROOT")
            .unwrap_or_else(|_| "/provider-sessions".into()),
    );
    if !provider_session_root.is_absolute() {
        return Err("FACTORY_RUNTIME_PROVIDER_SESSION_ROOT must be absolute".into());
    }
    let import_images =
        parse_import_images(env::var("FACTORY_RUNTIME_IMPORT_IMAGES").unwrap_or_default())?;
    let bind = env::var("FACTORY_BIND").unwrap_or_else(|_| "0.0.0.0:8770".into());
    let addr: std::net::SocketAddr = bind.parse()?;
    let container_mode = env::var("FACTORY_CONTAINER_MODE").as_deref() == Ok("1");
    if !(addr.ip().is_loopback()
        || (container_mode && addr.ip().is_unspecified() && addr.is_ipv4()))
    {
        return Err("runtime broker requires loopback or explicit container IPv4 binding".into());
    }

    let app = Arc::new(App {
        tokens: Arc::new(tokens),
        resource_locks: Arc::new(ResourceLocks::new()),
        lock_limit: RESOURCE_LOCK_ENTRIES,
        active: Arc::new(ActiveExecutions::new()),
        podman,
        images: Arc::new(images),
        provider_images: Arc::new(provider_images),
        provider_kind,
        provider_session_root,
        import_images,
        dependency_acquisition: Arc::new(RwLock::new(DependencyAcquisitionState::disabled(
            DEPENDENCY_RUNTIME_UNAVAILABLE,
            "dependency acquisition setup has not completed",
        ))),
    });
    app.preflight().await?;
    app.prepull_images().await?;

    let dependency_setup = setup_dependency_acquisition(&app).await;
    app.set_dependency_acquisition(dependency_setup.state.clone());

    let mut base_router = Router::new()
        .route("/health", get(health))
        .route("/v1/resources", get(list_resources))
        .route("/v1/volumes", post(create_volume))
        .route("/v1/volumes/{volume}", axum::routing::delete(remove_volume))
        .route(
            "/v1/volumes/{volume}/import",
            post(import_volume).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)),
        )
        .route("/v1/volumes/{volume}/export", get(export_volume))
        .route("/v1/containers/run", post(run_container))
        .route(
            "/v1/containers/{container}",
            axum::routing::delete(remove_container),
        )
        .route("/v1/containers/{container}/status", get(container_status))
        .route("/v1/providers/start", post(start_provider));
    if dependency_setup.service.is_none() {
        base_router = base_router.route(
            factory_native::dependency_service::DEPENDENCY_ACQUIRE_ROUTE,
            post(dependency_unavailable),
        );
    }
    let mut router = base_router
        .layer(DefaultBodyLimit::max(MAX_JSON_BYTES))
        .with_state(Arc::clone(&app));
    if let Some(service) = dependency_setup.service {
        router = router.merge(service.router());
    }
    let _keep_dependency_proxy = dependency_setup.proxy;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "Factory runtime broker listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn parse_images(raw: String) -> Result<BTreeSet<String>, String> {
    let mut images = BTreeSet::new();
    for image in raw.split(',') {
        let image = image.trim();
        if image.is_empty()
            || image.starts_with('-')
            || image.chars().any(char::is_whitespace)
            || image.contains("://")
        {
            return Err("image allowlist contains an invalid reference".into());
        }
        images.insert(image.to_owned());
    }
    if images.is_empty() {
        return Err("image allowlist must not be empty".into());
    }
    Ok(images)
}

fn parse_registries(raw: String) -> Result<Vec<String>, String> {
    let mut registries = Vec::new();
    for registry in raw.split(',') {
        let registry = registry.trim();
        if registry.is_empty() || registry.chars().any(char::is_whitespace) {
            return Err("dependency registry allowlist contains an invalid entry".into());
        }
        registries.push(registry.to_owned());
    }
    if registries.is_empty() {
        return Err("dependency registry allowlist must not be empty".into());
    }
    Ok(registries)
}

impl App {
    fn dependency_acquisition(&self) -> DependencyAcquisitionState {
        self.dependency_acquisition
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn set_dependency_acquisition(&self, state: DependencyAcquisitionState) {
        *self
            .dependency_acquisition
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
    }

    async fn preflight(&self) -> Result<(), Box<dyn std::error::Error>> {
        let uid_map = std::fs::read_to_string("/proc/self/uid_map").unwrap_or_default();
        if has_host_root_mapping(&uid_map) {
            return Err("runtime broker must run inside a rootless outer user namespace".into());
        }
        let output = self
            .podman_command(["--version"])
            .output()
            .await
            .map_err(|error| format!("podman is unavailable: {error}"))?;
        if !output.status.success() {
            return Err(format!("podman --version failed: {}", stderr(&output)).into());
        }
        let output = self
            .podman_command(["info", "--format", "json"])
            .output()
            .await
            .map_err(|error| format!("podman info failed: {error}"))?;
        if !output.status.success() {
            return Err(format!("podman info failed: {}", stderr(&output)).into());
        }
        let info: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("podman info returned invalid JSON: {error}"))?;
        if info
            .pointer("/host/security/rootless")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        {
            return Err(
                "nested runtime engine must be a rootful engine inside the rootless outer container"
                    .into(),
            );
        }
        Ok(())
    }

    async fn prepull_images(&self) -> Result<(), Box<dyn std::error::Error>> {
        for image in self.images.iter() {
            let exists = self
                .podman_output(["image", "exists", image.as_str()])
                .await?;
            if exists.status.success() {
                continue;
            }
            let output = time::timeout(
                Duration::from_secs(900),
                self.podman_command(["pull", "--quiet", image.as_str()])
                    .output(),
            )
            .await
            .map_err(|_| format!("pulling {image} timed out"))??;
            if !output.status.success() {
                return Err(format!("pulling {image} failed: {}", stderr(&output)).into());
            }
        }
        for (tag, path) in &self.import_images {
            self.import_bundle_image(
                tag,
                path,
                "/usr/local/bin/factory-provider-entrypoint",
                "/session",
                "provider-bundle.tar",
            )
            .await?;
        }
        for image in self.provider_images.iter() {
            let output = self
                .podman_output(["image", "exists", image.as_str()])
                .await?;
            if !output.status.success() {
                return Err(format!("provider image {image} is not available").into());
            }
        }
        Ok(())
    }

    async fn import_bundle_image(
        &self,
        tag: &str,
        path: &std::path::Path,
        entrypoint: &str,
        workdir: &str,
        stable_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !path.is_file() {
            return Err(format!("image bundle {} is missing", path.display()).into());
        }
        let bundle_path = path.to_owned();
        let digest = tokio::task::spawn_blocking(move || bundle_digest(&bundle_path))
            .await
            .map_err(|error| format!("bundle hash task failed: {error}"))??;
        let existing = self
            .podman_output([
                "image",
                "inspect",
                "--format",
                "{{ index .Labels \"io.software-factory.bundle-sha256\" }}",
                tag,
            ])
            .await;
        if let Ok(existing) = existing
            && existing.status.success()
            && String::from_utf8_lossy(&existing.stdout).trim() == digest
        {
            return Ok(());
        }
        let label = format!("LABEL io.software-factory.bundle-sha256={digest}");
        let stable = PathBuf::from("/var/lib/containers/tmp").join(stable_name);
        let source = path.to_owned();
        let stable_for_copy = stable.clone();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::copy(&source, &stable_for_copy)?;
            Ok(())
        })
        .await
        .map_err(|error| format!("bundle copy task failed: {error}"))?
        .map_err(|error| format!("bundle copy failed: {error}"))?;
        let entrypoint_change = format!("ENTRYPOINT [\"{entrypoint}\"]");
        let workdir_change = format!("WORKDIR {workdir}");
        let output = time::timeout(
            Duration::from_secs(900),
            self.podman_command([
                "import",
                "--change",
                "ENV PATH=/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin",
                "--change",
                entrypoint_change.as_str(),
                "--change",
                workdir_change.as_str(),
                "--change",
                label.as_str(),
                stable.to_string_lossy().as_ref(),
                tag,
            ])
            .output(),
        )
        .await
        .map_err(|_| format!("importing {tag} timed out"))??;
        let _ = std::fs::remove_file(&stable);
        if !output.status.success() {
            let existing = self
                .podman_output([
                    "image",
                    "inspect",
                    "--format",
                    "{{ index .Labels \"io.software-factory.bundle-sha256\" }}",
                    tag,
                ])
                .await;
            if !existing.is_ok_and(|existing| {
                existing.status.success()
                    && String::from_utf8_lossy(&existing.stdout).trim() == digest
            }) {
                return Err(format!("importing {tag} failed: {}", stderr(&output)).into());
            }
        }
        Ok(())
    }

    fn owner(&self, headers: &HeaderMap) -> Option<ResourceOwner> {
        self.tokens.authenticate(headers)
    }

    fn resource_lock(&self, name: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.resource_locks
            .acquire_with_limit(name, self.lock_limit)
    }

    async fn managed_resources(&self, owner: ResourceOwner) -> Result<ManagedResources, String> {
        let volumes = self
            .run_bounded(
                [
                    "volume",
                    "ls",
                    "--filter",
                    &format!("label={MANAGED_LABEL}"),
                    "--format",
                    "json",
                ],
                "managed volume listing",
                MAX_RESOURCE_INVENTORY_BYTES,
            )
            .await?;
        let containers = self
            .run_bounded(
                [
                    "ps",
                    "-a",
                    "--filter",
                    &format!("label={MANAGED_LABEL}"),
                    "--format",
                    "json",
                ],
                "managed container listing",
                MAX_RESOURCE_INVENTORY_BYTES,
            )
            .await?;
        let owned = owner.as_str();
        Ok(ManagedResources {
            volumes: parse_volume_inventory(&volumes)?
                .into_iter()
                .filter(|resource| resource.owner.as_deref() == Some(owned))
                .collect(),
            containers: parse_container_inventory(&containers)?
                .into_iter()
                .filter(|resource| resource.owner.as_deref() == Some(owned))
                .collect(),
        })
    }

    async fn remove_volume_locked(
        &self,
        volume: &str,
        owner: ResourceOwner,
    ) -> Result<(), MutationError> {
        let Some(labels) = self
            .volume_labels(volume)
            .await
            .map_err(MutationError::Failed)?
        else {
            return Ok(());
        };
        if !resource_owner_matches(&labels, owner) {
            return Err(MutationError::Conflict(
                "volume is not owned by the authenticated service",
            ));
        }
        self.remove_volume_checked_locked(volume).await
    }

    /// One-time migration for a managed workspace volume created before owner
    /// labels existed. The caller holds the volume lock and has already checked
    /// that the volume is not active. Labels are re-read immediately before
    /// removal so an owner, workspace or managed-label change is still a
    /// conflict.
    async fn remove_legacy_volume_locked(
        &self,
        volume: &str,
        workspace_id: &str,
    ) -> Result<(), MutationError> {
        if self.active.is_active(volume) {
            return Err(MutationError::Conflict("volume has an active execution"));
        }
        let Some(labels) = self
            .volume_labels(volume)
            .await
            .map_err(MutationError::Failed)?
        else {
            return Ok(());
        };
        if !legacy_workspace_identity_matches(&labels, workspace_id) {
            return Err(MutationError::Conflict(
                "volume exists with a different owner or workspace",
            ));
        }
        self.remove_volume_checked_locked(volume).await
    }

    async fn remove_volume_checked_locked(&self, volume: &str) -> Result<(), MutationError> {
        let output = self
            .podman_output(["volume", "rm", "--force", volume])
            .await
            .map_err(MutationError::Failed)?;
        if !output.status.success() {
            let exists = self
                .podman_output(["volume", "exists", volume])
                .await
                .map_err(MutationError::Failed)?;
            if exists.status.code() == Some(1) {
                return Ok(());
            }
            return Err(MutationError::Failed(format!(
                "volume removal failed: {}",
                stderr(&output)
            )));
        }
        Ok(())
    }

    async fn remove_container_locked(
        &self,
        container: &str,
        owner: ResourceOwner,
    ) -> Result<(), MutationError> {
        self.remove_container_matching_locked(container, owner, None)
            .await
    }

    async fn remove_execution_container_locked(
        &self,
        container: &str,
        owner: ResourceOwner,
        volume: &str,
    ) -> Result<(), MutationError> {
        self.remove_container_matching_locked(container, owner, Some(volume))
            .await
    }

    async fn remove_container_matching_locked(
        &self,
        container: &str,
        owner: ResourceOwner,
        expected_workspace: Option<&str>,
    ) -> Result<(), MutationError> {
        let Some(labels) = self
            .container_labels(container)
            .await
            .map_err(MutationError::Failed)?
        else {
            return Ok(());
        };
        let legacy_adoption = owner == ResourceOwner::Supervisor
            && is_valid_exec_container_name(container)
            && expected_workspace
                .map(str::to_owned)
                .or_else(|| labels.get(WORKSPACE_LABEL).cloned())
                .filter(|workspace| is_valid_volume_name(workspace))
                .as_deref()
                .is_some_and(|workspace| legacy_workspace_identity_matches(&labels, workspace));
        if !resource_owner_matches(&labels, owner) && !legacy_adoption {
            return Err(MutationError::Conflict(
                "container is not owned by the authenticated service",
            ));
        }
        if let Some(workspace) = expected_workspace
            && labels.get(WORKSPACE_LABEL).map(String::as_str) != Some(workspace)
        {
            return Err(MutationError::Conflict(
                "container does not belong to the execution workspace",
            ));
        }
        if legacy_adoption && !resource_owner_matches(&labels, owner) {
            tracing::info!(
                %container,
                workspace = labels.get(WORKSPACE_LABEL).map(String::as_str).unwrap_or_default(),
                owner = owner.as_str(),
                "adopted legacy execution container"
            );
        }
        let output = self
            .podman_output(["rm", "--force", "--time=0", container])
            .await
            .map_err(MutationError::Failed)?;
        if !output.status.success() {
            let exists = self
                .podman_output(["container", "exists", container])
                .await
                .map_err(MutationError::Failed)?;
            if exists.status.code() == Some(1) {
                return Ok(());
            }
            return Err(MutationError::Failed(format!(
                "container removal failed: {}",
                stderr(&output)
            )));
        }
        Ok(())
    }

    async fn remove_container_for(
        &self,
        container: &str,
        owner: ResourceOwner,
    ) -> Result<(), MutationError> {
        let lock = self.resource_lock(container);
        let _guard = lock.lock().await;
        self.remove_container_locked(container, owner).await
    }

    async fn remove_execution_container_for(
        &self,
        container: &str,
        owner: ResourceOwner,
        volume: &str,
    ) -> Result<(), MutationError> {
        let lock = self.resource_lock(container);
        let _guard = lock.lock().await;
        self.remove_execution_container_locked(container, owner, volume)
            .await
    }

    /// Remove the execution container and confirm it is absent before its
    /// volume active marker may be released. Any uncertainty keeps the volume
    /// blocked.
    async fn confirm_execution_cleanup(
        &self,
        container: &str,
        owner: ResourceOwner,
        volume: &str,
    ) -> bool {
        if let Err(error) = self
            .remove_execution_container_for(container, owner, volume)
            .await
        {
            tracing::warn!(%container, %error, "execution cleanup removal failed; volume remains blocked");
            return false;
        }
        match self.podman_output(["container", "exists", container]).await {
            Ok(output) if output.status.code() == Some(1) => true,
            Ok(_) => {
                tracing::warn!(%container, "execution container still present; volume remains blocked");
                false
            }
            Err(error) => {
                tracing::warn!(%container, %error, "execution cleanup could not be confirmed; volume remains blocked");
                false
            }
        }
    }

    async fn require_volume_owner(
        &self,
        volume: &str,
        owner: ResourceOwner,
    ) -> Result<(), MutationError> {
        let Some(labels) = self
            .volume_labels(volume)
            .await
            .map_err(MutationError::Failed)?
        else {
            return Err(MutationError::Failed("volume is unavailable".into()));
        };
        if !resource_owner_matches(&labels, owner) {
            return Err(MutationError::Conflict(
                "volume is not owned by the authenticated service",
            ));
        }
        Ok(())
    }

    async fn volume_labels(
        &self,
        volume: &str,
    ) -> Result<Option<BTreeMap<String, String>>, String> {
        let output = self
            .run_bounded(
                ["volume", "inspect", "--format", "json", volume],
                "volume inspect",
                MAX_RESOURCE_INVENTORY_BYTES,
            )
            .await;
        let labels = match output {
            Ok(bytes) => parse_volume_labels(&bytes)?,
            Err(error) => {
                let exists = self.podman_output(["volume", "exists", volume]).await?;
                if exists.status.code() == Some(1) {
                    return Ok(None);
                }
                return Err(error);
            }
        };
        Ok(Some(labels))
    }

    async fn container_labels(
        &self,
        container: &str,
    ) -> Result<Option<BTreeMap<String, String>>, String> {
        let output = self
            .run_bounded(
                ["inspect", "--format", "json", container],
                "container inspect",
                MAX_RESOURCE_INVENTORY_BYTES,
            )
            .await;
        let labels = match output {
            Ok(bytes) => parse_container_labels(&bytes)?,
            Err(error) => {
                let exists = self
                    .podman_output(["container", "exists", container])
                    .await?;
                if exists.status.code() == Some(1) {
                    return Ok(None);
                }
                return Err(error);
            }
        };
        Ok(Some(labels))
    }

    async fn container_present(&self, container: &str) -> Result<bool, String> {
        let output = self
            .podman_output(["container", "exists", container])
            .await?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(format!(
                "container existence check failed: {}",
                stderr(&output)
            )),
        }
    }

    async fn run_bounded<I, S>(
        &self,
        args: I,
        operation: &str,
        limit: usize,
    ) -> Result<Vec<u8>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut child = self
            .podman_command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("{operation} could not start: {error}"))?;
        let Some(stdout) = child.stdout.take() else {
            return Err(format!("{operation} stdout unavailable"));
        };
        let Some(stderr) = child.stderr.take() else {
            return Err(format!("{operation} stderr unavailable"));
        };
        let deadline = time::Instant::now() + COMMAND_TIMEOUT;
        let stdout_task = tokio::spawn(read_capped(stdout, limit));
        let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STREAM_BYTES));
        let (bytes, overflow) = match time::timeout_at(deadline, stdout_task).await {
            Ok(Ok(Ok(value))) => value,
            Ok(Ok(Err(error))) => {
                let _ = child.kill().await;
                return Err(format!("{operation} stdout: {error}"));
            }
            Ok(Err(error)) => {
                let _ = child.kill().await;
                return Err(format!("{operation} reader task: {error}"));
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stderr_task.abort();
                return Err(format!("{operation} timed out"));
            }
        };
        if overflow {
            let _ = child.kill().await;
            let _ = child.wait().await;
            stderr_task.abort();
            return Err(format!("{operation} exceeded its output bound"));
        }
        let status = match time::timeout_at(deadline, child.wait()).await {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => return Err(format!("{operation} wait failed: {error}")),
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                stderr_task.abort();
                return Err(format!("{operation} timed out"));
            }
        };
        let stderr_bytes = match time::timeout_at(deadline, stderr_task).await {
            Ok(Ok(Ok((bytes, _)))) => bytes,
            Ok(Ok(Err(error))) => {
                return Err(format!("{operation} stderr: {error}"));
            }
            Ok(Err(error)) => return Err(format!("{operation} stderr task: {error}")),
            Err(_) => return Err(format!("{operation} stderr timed out")),
        };
        if !status.success() {
            return Err(format!(
                "{operation} failed: {}",
                String::from_utf8_lossy(&stderr_bytes).trim()
            ));
        }
        Ok(bytes)
    }

    fn podman_command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new(&self.podman);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .env("TMPDIR", "/var/lib/containers/tmp")
            .env("STORAGE_DRIVER", "vfs")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    async fn podman_output<I, S>(&self, args: I) -> Result<std::process::Output, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        time::timeout(COMMAND_TIMEOUT, self.podman_command(args).output())
            .await
            .map_err(|_| "podman command timed out".to_owned())?
            .map_err(|error| format!("podman command failed: {error}"))
    }

    async fn run_checked<I, S>(&self, args: I, operation: &str) -> Result<Vec<u8>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = self.podman_output(args).await?;
        if !output.status.success() {
            return Err(format!("{operation} failed: {}", stderr(&output)));
        }
        Ok(output.stdout)
    }

    async fn dependency_image_id(&self, image: &str) -> Result<String, String> {
        let output = self
            .podman_output(["image", "inspect", "--format", "{{.Id}}", image])
            .await?;
        if !output.status.success() {
            return Err(format!(
                "dependency image {image} is not available: {}",
                stderr(&output)
            ));
        }
        let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        factory_native::dependency_service::normalize_image_id(&id)
            .ok_or_else(|| format!("dependency image {image} returned an invalid identity"))
    }

    async fn ensure_dependency_egress_network(
        &self,
    ) -> Result<(String, SocketAddr, PeerSubnet), String> {
        let network = env::var("FACTORY_RUNTIME_DEPENDENCY_EGRESS_NETWORK")
            .unwrap_or_else(|_| "factory-deps-egress".into());
        if !valid_network_name(&network) {
            return Err("FACTORY_RUNTIME_DEPENDENCY_EGRESS_NETWORK is invalid".into());
        }
        let subnet = env::var("FACTORY_RUNTIME_DEPENDENCY_EGRESS_SUBNET")
            .unwrap_or_else(|_| "10.90.0.0/24".into());
        let peer_subnet = PeerSubnet::parse(&subnet).map_err(|error| error.to_string())?;
        let gateway = env::var("FACTORY_RUNTIME_DEPENDENCY_EGRESS_GATEWAY")
            .unwrap_or_else(|_| "10.90.0.1".into());
        let gateway: std::net::IpAddr = gateway
            .parse()
            .map_err(|_| "FACTORY_RUNTIME_DEPENDENCY_EGRESS_GATEWAY is invalid")?;
        if gateway.is_unspecified() || gateway.is_multicast() {
            return Err(
                "FACTORY_RUNTIME_DEPENDENCY_EGRESS_GATEWAY must be a private network address"
                    .into(),
            );
        }
        let port: u16 = env::var("FACTORY_RUNTIME_DEPENDENCY_EGRESS_PORT")
            .unwrap_or_else(|_| "18080".into())
            .parse()
            .map_err(|_| "FACTORY_RUNTIME_DEPENDENCY_EGRESS_PORT is invalid")?;
        if port == 0 {
            return Err("FACTORY_RUNTIME_DEPENDENCY_EGRESS_PORT is invalid".into());
        }
        let exists = self
            .podman_output(["network", "exists", network.as_str()])
            .await?;
        if !exists.status.success() {
            let output = self
                .podman_output([
                    "network",
                    "create",
                    "--internal",
                    "--subnet",
                    subnet.as_str(),
                    "--gateway",
                    &gateway.to_string(),
                    "--label",
                    MANAGED_LABEL,
                    network.as_str(),
                ])
                .await?;
            if !output.status.success() {
                return Err(format!(
                    "creating dependency egress network failed: {}",
                    stderr(&output)
                ));
            }
        }
        let output = self
            .podman_output([
                "network",
                "inspect",
                "--format",
                "{{json .}}",
                network.as_str(),
            ])
            .await?;
        if !output.status.success() {
            return Err(format!(
                "inspecting dependency egress network failed: {}",
                stderr(&output)
            ));
        }
        let inspected: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                format!("dependency egress network inspection is not JSON: {error}")
            })?;
        validate_egress_network_inspection(&inspected, &gateway.to_string(), &subnet)?;
        Ok((network, SocketAddr::new(gateway, port), peer_subnet))
    }
}

fn dependency_configuration() -> Result<Option<(String, PathBuf)>, String> {
    let image = match env::var("FACTORY_RUNTIME_DEPENDENCY_IMAGE") {
        Ok(value) if !value.trim().is_empty() => {
            let image = value.trim().to_owned();
            if image.starts_with('-')
                || image.chars().any(char::is_whitespace)
                || image.contains("://")
            {
                return Err("FACTORY_RUNTIME_DEPENDENCY_IMAGE is invalid".into());
            }
            Some(image)
        }
        _ => None,
    };
    let bundle = match env::var("FACTORY_RUNTIME_DEPENDENCY_BUNDLE") {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value.trim())),
        _ => None,
    };
    match (image, bundle) {
        (None, None) => Ok(None),
        (Some(image), Some(bundle)) => {
            if !bundle.is_absolute() || !bundle.is_file() {
                return Err(
                    "FACTORY_RUNTIME_DEPENDENCY_BUNDLE must be an absolute existing file".into(),
                );
            }
            Ok(Some((image, bundle)))
        }
        _ => Err(
            "FACTORY_RUNTIME_DEPENDENCY_IMAGE and FACTORY_RUNTIME_DEPENDENCY_BUNDLE must be set together"
                .into(),
        ),
    }
}

async fn setup_dependency_acquisition(app: &Arc<App>) -> DependencySetup {
    let configuration = match dependency_configuration() {
        Ok(configuration) => configuration,
        Err(reason) => {
            let reason = bounded_dependency_reason(&reason);
            tracing::warn!(
                code = DEPENDENCY_RUNTIME_UNAVAILABLE,
                %reason,
                "dependency acquisition disabled at startup"
            );
            return DependencySetup {
                state: DependencyAcquisitionState::disabled(DEPENDENCY_RUNTIME_UNAVAILABLE, reason),
                service: None,
                proxy: None,
            };
        }
    };
    let Some((image, bundle)) = configuration else {
        tracing::info!("dependency acquisition endpoint disabled; no dependency image configured");
        return DependencySetup {
            state: DependencyAcquisitionState::disabled(
                DEPENDENCY_RUNTIME_UNAVAILABLE,
                "no dependency image is configured",
            ),
            service: None,
            proxy: None,
        };
    };
    match try_setup_dependency_acquisition(app, &image, &bundle).await {
        Ok(setup) => setup,
        Err(failure) => {
            let reason = bounded_dependency_reason(&failure.reason);
            tracing::warn!(
                code = failure.code,
                %reason,
                "dependency acquisition disabled at startup; runtime broker remains available"
            );
            DependencySetup {
                state: DependencyAcquisitionState::disabled(failure.code, reason),
                service: None,
                proxy: None,
            }
        }
    }
}

async fn try_setup_dependency_acquisition(
    app: &Arc<App>,
    image: &str,
    bundle: &Path,
) -> Result<DependencySetup, DependencySetupFailure> {
    let runtime_failure =
        |reason: String| DependencySetupFailure::new(DEPENDENCY_RUNTIME_UNAVAILABLE, reason);
    let egress_failure =
        |reason: String| DependencySetupFailure::new(DEPENDENCY_EGRESS_UNAVAILABLE, reason);

    app.import_bundle_image(
        image,
        bundle,
        "/usr/local/bin/factory-dependency-acquire",
        "/workspace",
        "dependency-bundle.tar",
    )
    .await
    .map_err(|error| runtime_failure(error.to_string()))?;
    let image_id = app
        .dependency_image_id(image)
        .await
        .map_err(runtime_failure)?;
    let registries = match env::var("FACTORY_RUNTIME_DEPENDENCY_REGISTRIES") {
        Ok(value) if !value.trim().is_empty() => parse_registries(value),
        _ => Ok(factory_native::dependencies::DEFAULT_ALLOWED_REGISTRIES
            .iter()
            .map(|registry| (*registry).to_string())
            .collect()),
    }
    .map_err(runtime_failure)?;
    let (network, proxy_address, peer_subnet) = app
        .ensure_dependency_egress_network()
        .await
        .map_err(egress_failure)?;
    let allowlist = parse_egress_allowlist(
        env::var("FACTORY_RUNTIME_DEPENDENCY_EGRESS_ALLOW").unwrap_or_else(|_| {
            "index.crates.io:443,static.crates.io:443,github.com:443,codeload.github.com:443".into()
        }),
    )
    .map_err(egress_failure)?;
    let (proxy, binding) = spawn_dependency_proxy(proxy_address, peer_subnet, allowlist)
        .await
        .map_err(egress_failure)?;
    let proxy_url = format!("http://{proxy_address}");
    let acquirer = match factory_native::dependency_service::PodmanDependencyAcquirer::new(
        app.podman.clone(),
        image_id.clone(),
    )
    .and_then(|acquirer| acquirer.with_egress(network.clone(), proxy_url.clone(), proxy.ready()))
    {
        Ok(acquirer) => acquirer,
        Err(error) => {
            proxy.abort();
            return Err(runtime_failure(error.to_string()));
        }
    };
    let Some(supervisor_token) = app.tokens.token(ResourceOwner::Supervisor) else {
        proxy.abort();
        return Err(runtime_failure(
            "runtime broker supervisor token is unavailable".into(),
        ));
    };
    let service = match factory_native::dependency_service::DependencyService::new(
        supervisor_token.to_owned(),
        image.to_owned(),
        registries,
        Arc::new(acquirer),
    ) {
        Ok(service) => Arc::new(service.with_guard(Arc::new(BrokerDependencyGuard {
            app: Arc::clone(app),
        }))),
        Err(error) => {
            proxy.abort();
            return Err(runtime_failure(error.to_string()));
        }
    };
    tracing::info!(
        %image,
        %image_id,
        %network,
        %proxy_url,
        binding,
        "dependency acquisition endpoint enabled"
    );
    Ok(DependencySetup {
        state: DependencyAcquisitionState::enabled(network, proxy_url, binding),
        service: Some(service),
        proxy: Some(proxy),
    })
}

async fn spawn_dependency_proxy(
    address: SocketAddr,
    peer_subnet: PeerSubnet,
    allowlist: EgressAllowlist,
) -> Result<(dependency_egress::RunningProxy, &'static str), String> {
    match dependency_egress::spawn(address, allowlist.clone()).await {
        Ok(proxy) => Ok((proxy, "gateway")),
        Err(dependency_egress::EgressProxyError::Io(error))
            if error.kind() == std::io::ErrorKind::AddrNotAvailable =>
        {
            let wildcard = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), address.port());
            tracing::info!(
                %address,
                %wildcard,
                "dependency egress gateway is not assigned; binding a subnet-scoped wildcard listener"
            );
            dependency_egress::spawn_scoped(wildcard, allowlist, Some(peer_subnet))
                .await
                .map(|proxy| (proxy, "wildcard-subnet-scoped"))
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn bounded_dependency_reason(reason: &str) -> String {
    let redacted = factory_native::telemetry::redact(reason);
    let single_line = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    let bounded: String = single_line
        .chars()
        .take(MAX_DEPENDENCY_HEALTH_REASON)
        .collect();
    if bounded.is_empty() {
        "dependency acquisition setup failed".into()
    } else {
        bounded
    }
}

/// Admission guard for dependency acquisition. It reuses the broker's exact
/// per-volume lock, owner-label check and active-execution registry so the
/// dependency route cannot be used as a side channel to mutate a volume that
/// is owned by another service or currently in use.
struct BrokerDependencyGuard {
    app: Arc<App>,
}

struct DependencyLease {
    active: Arc<ActiveExecutions>,
    volume: String,
    container: String,
}

impl Drop for DependencyLease {
    fn drop(&mut self) {
        self.active.remove(&self.volume, &self.container);
    }
}

impl factory_native::dependency_service::DependencyGuard for BrokerDependencyGuard {
    fn enter<'a>(
        &'a self,
        volume: &'a str,
        container: &'a str,
    ) -> factory_native::dependency_service::DependencyGuardFuture<'a> {
        Box::pin(async move {
            let lock = self.app.resource_lock(volume);
            let guard = lock.lock_owned().await;
            let Some(labels) = self.app.volume_labels(volume).await? else {
                return Err("workspace volume does not exist".into());
            };
            if !resource_owner_matches(&labels, ResourceOwner::Supervisor) {
                return Err("workspace volume is not owned by the authenticated service".into());
            }
            if labels
                .get("io.software-factory.workspace")
                .is_none_or(|workspace| workspace.is_empty())
            {
                return Err("workspace volume is not bound to a workspace".into());
            }
            if !self.app.active.register(volume, container) {
                return Err("workspace volume already has an active execution".into());
            }
            drop(guard);
            Ok(Box::new(DependencyLease {
                active: Arc::clone(&self.app.active),
                volume: volume.to_owned(),
                container: container.to_owned(),
            }) as Box<dyn std::any::Any + Send>)
        })
    }
}

fn validate_egress_network_inspection(
    inspected: &serde_json::Value,
    expected_gateway: &str,
    expected_subnet: &str,
) -> Result<(), String> {
    if inspected
        .get("internal")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err("dependency egress network must be an internal network".into());
    }
    if inspected
        .pointer("/labels/io.software-factory.managed")
        .and_then(serde_json::Value::as_str)
        != Some("true")
    {
        return Err(
            "dependency egress network is missing the managed label; refusing an unmanaged network"
                .into(),
        );
    }
    let observed_gateway = inspected
        .pointer("/subnets/0/gateway")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let observed_subnet = inspected
        .pointer("/subnets/0/subnet")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if observed_gateway != expected_gateway || observed_subnet != expected_subnet {
        return Err(format!(
            "dependency egress network does not match its fixed gateway/subnet: {observed_gateway} {observed_subnet}"
        ));
    }
    Ok(())
}

fn valid_network_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn parse_egress_allowlist(raw: String) -> Result<EgressAllowlist, String> {
    let entries: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect();
    EgressAllowlist::new(entries).map_err(|error| error.to_string())
}

fn stderr(output: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    text.chars()
        .take(1024)
        .collect::<String>()
        .trim()
        .to_owned()
}

async fn health(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    Json(json!({
        "healthy": true,
        "isolation_certified": false,
        "engine_exposed": false,
        "network": "none",
        "dependencyAcquisition": app.dependency_acquisition().health_projection(),
    }))
}

async fn dependency_unavailable(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if app.owner(&headers) != Some(ResourceOwner::Supervisor) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.dependency_acquisition() {
        DependencyAcquisitionState::Disabled { code, reason } => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "code": code,
                "error": format!("dependency acquisition unavailable: {reason}"),
                "reason": reason,
            })),
        )
            .into_response(),
        DependencyAcquisitionState::Enabled { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "code": DEPENDENCY_RUNTIME_UNAVAILABLE,
                "error": "dependency acquisition is not available",
                "reason": "dependency acquisition endpoint is not mounted",
            })),
        )
            .into_response(),
    }
}

async fn list_resources(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match app.managed_resources(owner).await {
        Ok(resources) => Json(resources).into_response(),
        Err(error) => operation_error(error),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right) {
        diff |= left ^ right;
    }
    diff == 0
}

fn valid_workspace_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_provider_session_subdir(value: &str) -> bool {
    value.len() >= 8
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn parse_import_images(raw: String) -> Result<Vec<(String, PathBuf)>, String> {
    let mut images = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((tag, path)) = entry.split_once('=') else {
            return Err("FACTORY_RUNTIME_IMPORT_IMAGES entries must be tag=/absolute/path".into());
        };
        let tag = tag.trim();
        let path = PathBuf::from(path.trim());
        if tag.is_empty()
            || tag.starts_with('-')
            || tag.chars().any(char::is_whitespace)
            || !path.is_absolute()
        {
            return Err("FACTORY_RUNTIME_IMPORT_IMAGES entry is invalid".into());
        }
        images.push((tag.to_owned(), path));
    }
    Ok(images)
}

fn has_host_root_mapping(map: &str) -> bool {
    let mut lines = map.lines().filter(|line| !line.trim().is_empty());
    let Some(_first) = lines.next() else {
        return true;
    };
    let mut any = false;
    for line in map.lines().filter(|line| !line.trim().is_empty()) {
        any = true;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 3 {
            return true;
        }
        if fields[0] == "0" && fields[1] == "0" {
            return true;
        }
    }
    !any
}

fn bundle_digest(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("cannot open image bundle: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read image bundle: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_argv(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() || argv.len() > MAX_ARGV_ITEMS {
        return Err("argv size is outside the allowed bound".into());
    }
    let mut total = 0usize;
    for argument in argv {
        if argument.is_empty() || argument.len() > MAX_ARG_BYTES {
            return Err("argv item is empty or too large".into());
        }
        total = total.saturating_add(argument.len());
    }
    if total > MAX_ARGV_BYTES {
        return Err("argv is too large".into());
    }
    Ok(())
}

fn validate_env(env: &[(String, String)]) -> Result<(), String> {
    if env.len() > MAX_ENV_ITEMS {
        return Err("environment has too many entries".into());
    }
    let mut total = 0usize;
    let mut seen = BTreeSet::new();
    for (key, value) in env {
        if !PROVIDER_ENV_KEYS.contains(&key.as_str()) {
            return Err(format!("environment key {key:?} is not allowed"));
        }
        if !seen.insert(key.as_str()) {
            return Err("environment contains a duplicate key".into());
        }
        if value.len() > MAX_ARG_BYTES || value.contains('\0') {
            return Err("environment value is invalid".into());
        }
        total = total.saturating_add(key.len() + value.len());
    }
    if total > MAX_ENV_BYTES {
        return Err("environment is too large".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NewVolume {
    volume: String,
    workspace_id: String,
}

async fn create_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewVolume>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_volume_name(&input.volume) || !valid_workspace_id(&input.workspace_id) {
        return bad_request("invalid volume or workspace identity");
    }
    if owner == ResourceOwner::Provider {
        return forbidden("provider owner cannot create workspace volumes");
    }
    let lock = app.resource_lock(&input.volume);
    let _guard = lock.lock().await;
    if app.active.is_active(&input.volume) {
        return conflict("volume has an active execution");
    }
    match app.volume_labels(&input.volume).await {
        Ok(Some(labels)) => {
            if volume_identity_matches(&labels, owner, &input.workspace_id) {
                if let Err(error) = app.remove_volume_locked(&input.volume, owner).await {
                    return mutation_error(error);
                }
            } else if legacy_workspace_identity_matches(&labels, &input.workspace_id) {
                // A pre-owner managed volume for this exact workspace is safe
                // to reset. The checked removal re-validates the labels and
                // active registry under the same volume lock.
                if let Err(error) = app
                    .remove_legacy_volume_locked(&input.volume, &input.workspace_id)
                    .await
                {
                    return mutation_error(error);
                }
                tracing::info!(
                    volume = %input.volume,
                    workspace = %input.workspace_id,
                    owner = owner.as_str(),
                    "adopted legacy workspace volume"
                );
            } else {
                return conflict("volume exists with a different owner or workspace");
            }
        }
        Ok(None) => {}
        Err(error) => return operation_error(error),
    }
    let label = format!("{WORKSPACE_LABEL}={}", input.workspace_id);
    let owner_label = format!("{OWNER_LABEL}={}", owner.as_str());
    match app
        .run_checked(
            [
                "volume",
                "create",
                "--label",
                &label,
                "--label",
                &owner_label,
                "--label",
                MANAGED_LABEL,
                input.volume.as_str(),
            ],
            "volume create",
        )
        .await
    {
        Ok(stdout) => (
            StatusCode::CREATED,
            Json(json!({
                "volume": input.volume,
                "created": true,
                "output": String::from_utf8_lossy(&stdout).trim()
            })),
        )
            .into_response(),
        Err(error) => operation_error(error),
    }
}

async fn remove_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(volume): AxumPath<String>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_volume_name(&volume) {
        return bad_request("invalid volume name");
    }
    let lock = app.resource_lock(&volume);
    let _guard = lock.lock().await;
    if app.active.is_active(&volume) {
        return conflict("volume has an active execution");
    }
    match app.remove_volume_locked(&volume, owner).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => mutation_error(error),
    }
}

async fn import_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(volume): AxumPath<String>,
    body: Bytes,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_volume_name(&volume) || body.is_empty() || body.len() > MAX_ARCHIVE_BYTES {
        return bad_request("invalid volume import");
    }
    let lock = app.resource_lock(&volume);
    let _guard = lock.lock().await;
    if app.active.is_active(&volume) {
        return conflict("volume has an active execution");
    }
    if let Err(error) = app.require_volume_owner(&volume, owner).await {
        return mutation_error(error);
    }
    let mut child = match app
        .podman_command(["volume", "import", volume.as_str(), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return operation_error(format!("volume import could not start: {error}")),
    };
    let Some(mut stdin) = child.stdin.take() else {
        return operation_error("volume import stdin unavailable");
    };
    let bytes = body.to_vec();
    let writer = tokio::spawn(async move {
        stdin.write_all(&bytes).await?;
        stdin.shutdown().await
    });
    let output = match time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return operation_error(format!("volume import failed: {error}")),
        Err(_) => {
            let _ = app.remove_volume_locked(&volume, owner).await;
            return operation_error("volume import timed out");
        }
    };
    if let Err(error) = writer.await.unwrap_or_else(|error| {
        Err(std::io::Error::other(format!(
            "volume import writer task: {error}"
        )))
    }) {
        return operation_error(format!("volume import writer failed: {error}"));
    }
    if !output.status.success() {
        return operation_error(format!("volume import failed: {}", stderr(&output)));
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn export_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(volume): AxumPath<String>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_volume_name(&volume) {
        return bad_request("invalid volume name");
    }
    let lock = app.resource_lock(&volume);
    let _guard = lock.lock().await;
    if app.active.is_active(&volume) {
        return conflict("volume has an active execution");
    }
    if let Err(error) = app.require_volume_owner(&volume, owner).await {
        return mutation_error(error);
    }
    let mut child = match app
        .podman_command(["volume", "export", volume.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return operation_error(format!("volume export could not start: {error}")),
    };
    let Some(stdout) = child.stdout.take() else {
        return operation_error("volume export stdout unavailable");
    };
    let Some(stderr) = child.stderr.take() else {
        return operation_error("volume export stderr unavailable");
    };
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_ARCHIVE_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STREAM_BYTES));
    let status = match time::timeout(COMMAND_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => return operation_error(format!("volume export failed: {error}")),
        Err(_) => {
            let _ = child.kill().await;
            return operation_error("volume export timed out");
        }
    };
    let (bytes, truncated) = match stdout_task.await {
        Ok(Ok(value)) => value,
        _ => return operation_error("volume export stdout could not be read"),
    };
    let (stderr_bytes, _) = match stderr_task.await {
        Ok(Ok(value)) => value,
        _ => return operation_error("volume export stderr could not be read"),
    };
    if !status.success() {
        return operation_error(format!(
            "volume export failed: {}",
            String::from_utf8_lossy(&stderr_bytes).trim()
        ));
    }
    if truncated {
        return operation_error("volume export exceeded the archive bound");
    }
    (
        StatusCode::OK,
        [("content-type", "application/x-tar")],
        bytes,
    )
        .into_response()
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RunContainer {
    volume: String,
    container: String,
    image: String,
    argv: Vec<String>,
    timeout_secs: u64,
}

async fn run_container(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<RunContainer>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_volume_name(&input.volume) || !is_valid_container_name(&input.container) {
        return bad_request("invalid volume or container identity");
    }
    if !is_valid_exec_container_name(&input.container) {
        return bad_request("execution container has an invalid owner prefix");
    }
    if owner == ResourceOwner::Provider {
        return forbidden("provider owner cannot start execution containers");
    }
    if !app.images.contains(&input.image) {
        return bad_request("image is not allowlisted");
    }
    if let Err(error) = validate_argv(&input.argv) {
        return bad_request(&error);
    }
    if !(1..=MAX_EXECUTION_RUNTIME.as_secs()).contains(&input.timeout_secs) {
        return bad_request("execution timeout is outside the allowed bound");
    }
    let lock = app.resource_lock(&input.volume);
    let volume_guard = lock.lock().await;
    // Volume validation and active-execution registration happen under the
    // volume lock. The lock is released before the long execution; the registry
    // blocks volume deletion/reset/import/export for the run lifetime.
    if let Err(error) = app.require_volume_owner(&input.volume, owner).await {
        return mutation_error(error);
    }
    if !app.active.register(&input.volume, &input.container) {
        return conflict("volume or container name is already active");
    }
    match app.container_present(&input.container).await {
        Ok(true) => {
            app.active.remove(&input.volume, &input.container);
            return conflict("container name is already present");
        }
        Ok(false) => {}
        Err(error) => {
            app.active.remove(&input.volume, &input.container);
            return operation_error(error);
        }
    }
    drop(volume_guard);
    drop(lock);
    let task_app = Arc::clone(&app);
    let task_input = input.clone();
    let task = tokio::spawn(async move {
        tracing::info!(container=%task_input.container, volume=%task_input.volume, "child execution started");
        let result = run_child(&task_app, &task_input, owner).await;
        let confirmed = task_app
            .confirm_execution_cleanup(&task_input.container, owner, &task_input.volume)
            .await;
        if confirmed {
            task_app
                .active
                .remove(&task_input.volume, &task_input.container);
        } else {
            tracing::error!(
                container=%task_input.container,
                volume=%task_input.volume,
                "execution cleanup unconfirmed; volume remains blocked"
            );
        }
        match &result {
            Ok(output) => tracing::info!(
                container=%task_input.container,
                exit_code=?output.exit_code,
                "child execution finished"
            ),
            Err(error) => {
                tracing::warn!(container=%task_input.container, %error, "child execution failed")
            }
        }
        (result, confirmed)
    });
    // The execution task is detached from this request. If the client cancels,
    // the task still runs cleanup and only then releases the volume marker.
    match task.await {
        Ok((result, confirmed)) => {
            if !confirmed {
                return operation_error(
                    "execution cleanup could not be confirmed; volume remains blocked",
                );
            }
            match result {
                Ok(output) => Json(json!({
                    "exitCode": output.exit_code,
                    "stdout": String::from_utf8_lossy(&output.stdout),
                    "stderr": String::from_utf8_lossy(&output.stderr),
                    "stdoutTruncated": output.stdout_truncated,
                    "stderrTruncated": output.stderr_truncated
                }))
                .into_response(),
                Err(error) => operation_error(error),
            }
        }
        Err(error) => {
            tracing::error!(%error, "execution task failed; scheduling cleanup confirmation");
            let cleanup_app = Arc::clone(&app);
            let container = input.container.clone();
            let volume = input.volume.clone();
            tokio::spawn(async move {
                if cleanup_app
                    .confirm_execution_cleanup(&container, owner, &volume)
                    .await
                {
                    cleanup_app.active.remove(&volume, &container);
                }
            });
            operation_error("execution task failed; cleanup is being confirmed")
        }
    }
}

struct ChildOutput {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

async fn run_child(
    app: &App,
    input: &RunContainer,
    owner: ResourceOwner,
) -> Result<ChildOutput, String> {
    let timeout = Duration::from_secs(input.timeout_secs);
    let owner_label = format!("{OWNER_LABEL}={}", owner.as_str());
    let args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        format!("--name={}", input.container),
        format!("--label={MANAGED_LABEL}"),
        format!("--label={WORKSPACE_LABEL}={}", input.volume),
        format!("--label={owner_label}"),
        "--pull=never".to_owned(),
        "--network=none".to_owned(),
        "--read-only".to_owned(),
        "--tmpfs=/tmp:rw,noexec,nosuid,size=512m".to_owned(),
        "--tmpfs=/run:rw,noexec,nosuid,size=16m".to_owned(),
        // Cargo must execute freshly built test binaries. Keep its target
        // directory on an executable ephemeral tmpfs, outside the workspace
        // volume, so build artifacts never enter the exported tar.
        "--tmpfs=/cargo-target:rw,nodev,nosuid,size=512m".to_owned(),
        "--cap-drop=all".to_owned(),
        "--cap-add=NET_ADMIN".to_owned(),
        "--security-opt=no-new-privileges".to_owned(),
        "--ulimit=fsize=67108864".to_owned(),
        "--pids-limit=128".to_owned(),
        "--memory=1g".to_owned(),
        "--cpus=2".to_owned(),
        "--workdir=/workspace".to_owned(),
        "--env=HOME=/tmp".to_owned(),
        "--env=TMPDIR=/tmp".to_owned(),
        "--env=CARGO_TARGET_DIR=/cargo-target".to_owned(),
        format!("--volume={}:/workspace:rw", input.volume),
        input.image.clone(),
    ];
    let mut command = app.podman_command(args.iter().map(String::as_str));
    for argument in &input.argv {
        command.arg(argument);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("child container could not start: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        return Err("child stdout unavailable".into());
    };
    let Some(stderr) = child.stderr.take() else {
        return Err("child stderr unavailable".into());
    };
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_STREAM_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STREAM_BYTES));
    let status = match time::timeout(timeout + Duration::from_secs(15), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            stdout_task.abort();
            stderr_task.abort();
            return Err(format!("child container wait failed: {error}"));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = app
                .remove_execution_container_for(&input.container, owner, &input.volume)
                .await;
            stdout_task.abort();
            stderr_task.abort();
            return Err("child container execution timed out".into());
        }
    };
    let _ = app
        .remove_execution_container_for(&input.container, owner, &input.volume)
        .await;
    let (stdout, stdout_truncated) = stdout_task
        .await
        .map_err(|error| format!("child stdout reader: {error}"))?
        .map_err(|error| format!("child stdout reader: {error}"))?;
    let (stderr, stderr_truncated) = stderr_task
        .await
        .map_err(|error| format!("child stderr reader: {error}"))?
        .map_err(|error| format!("child stderr reader: {error}"))?;
    Ok(ChildOutput {
        exit_code: status.code(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

async fn remove_container(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(container): AxumPath<String>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_container_name(&container) {
        return bad_request("invalid container name");
    }
    let lock = app.resource_lock(&container);
    let _guard = lock.lock().await;
    match app.remove_container_locked(&container, owner).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => mutation_error(error),
    }
}

async fn container_status(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(container): AxumPath<String>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_valid_container_name(&container) {
        return bad_request("invalid container name");
    }
    let lock = app.resource_lock(&container);
    let _guard = lock.lock().await;
    let Some(labels) = (match app.container_labels(&container).await {
        Ok(labels) => labels,
        Err(error) => return operation_error(error),
    }) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"container not found"})),
        )
            .into_response();
    };
    if !resource_owner_matches(&labels, owner) {
        return conflict("container is not owned by the authenticated service");
    }
    let output = match app
        .podman_output([
            "inspect",
            "--format",
            "{{.State.Status}} {{.State.ExitCode}}",
            container.as_str(),
        ])
        .await
    {
        Ok(output) => output,
        Err(error) => return operation_error(error),
    };
    if !output.status.success() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"container not found"})),
        )
            .into_response();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace();
    let status = parts.next().unwrap_or("unknown");
    let exit_code = parts
        .next()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(-1);
    Json(json!({"container":container,"status":status,"exitCode":exit_code})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StartProvider {
    container: String,
    session_subdir: String,
    timeout_secs: u64,
    env: Vec<(String, String)>,
}

async fn start_provider(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<StartProvider>,
) -> Response {
    let Some(owner) = app.owner(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if owner != ResourceOwner::Provider {
        return forbidden("only the provider owner may start provider sessions");
    }
    if !is_valid_provider_container_name(&input.container) {
        return bad_request("invalid provider container identity");
    }
    if !valid_provider_session_subdir(&input.session_subdir) {
        return bad_request("invalid provider session directory");
    }
    if input.timeout_secs > MAX_PROVIDER_RUNTIME.as_secs() {
        return bad_request("provider timeout is outside the allowed bound");
    }
    if let Err(error) = validate_env(&input.env) {
        return bad_request(&error);
    }
    if app.provider_kind.is_empty() {
        return bad_request("provider runtime is not configured");
    }
    let Some(image) = app.provider_images.iter().next() else {
        return bad_request("provider image is not configured");
    };
    if app.provider_images.len() != 1 {
        return bad_request("provider image allowlist must contain exactly one image");
    }
    let lock = app.resource_lock(&input.container);
    let _guard = lock.lock().await;
    let session_source = app.provider_session_root.join(&input.session_subdir);
    let canonical = match session_source.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return bad_request(&format!(
                "provider session directory is unavailable: {error}"
            ));
        }
    };
    let canonical_root = match app.provider_session_root.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return operation_error(format!("provider session root is unavailable: {error}"));
        }
    };
    if !canonical.starts_with(&canonical_root) || !canonical.is_dir() {
        return bad_request("provider session directory escapes the configured root");
    }
    let mount = format!(
        "type=bind,source={},destination=/session,bind-propagation=rprivate",
        canonical.to_string_lossy()
    );
    let mut args: Vec<String> = vec![
        "run".into(),
        "--detach".into(),
        format!("--name={}", input.container),
        format!("--label={MANAGED_LABEL}"),
        format!("--label={OWNER_LABEL}=provider"),
        "--pull=never".into(),
        "--network=none".into(),
        "--read-only".into(),
        "--tmpfs=/tmp:rw,noexec,nosuid,size=512m".into(),
        "--tmpfs=/run:rw,noexec,nosuid,size=16m".into(),
        "--cap-drop=all".into(),
        "--security-opt=no-new-privileges".into(),
        "--ulimit=fsize=67108864".into(),
        "--user=10001:10001".into(),
        "--pids-limit=256".into(),
        "--memory=4g".into(),
        "--cpus=2".into(),
        "--workdir=/session".into(),
        format!("--mount={mount}"),
    ];
    for (key, value) in &input.env {
        args.push(format!("--env={key}={value}"));
    }
    args.push(image.clone());
    let output = match time::timeout(
        COMMAND_TIMEOUT,
        app.podman_command(args.iter().map(String::as_str)).output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return operation_error(format!("provider start failed: {error}")),
        Err(_) => return operation_error("provider start timed out"),
    };
    if !output.status.success() {
        let _ = app
            .remove_container_locked(&input.container, ResourceOwner::Provider)
            .await;
        return operation_error(format!("provider start failed: {}", stderr(&output)));
    }
    tracing::info!(container=%input.container, subdir=%input.session_subdir, "provider container started");
    let reaper = Arc::clone(&app);
    let reaper_container = input.container.clone();
    let reaper_seconds = input.timeout_secs;
    if reaper_seconds > 0 {
        tokio::spawn(async move {
            time::sleep(Duration::from_secs(reaper_seconds)).await;
            tracing::warn!(container=%reaper_container, "provider container reached its lifetime bound; removing");
            if let Err(error) = reaper
                .remove_container_for(&reaper_container, ResourceOwner::Provider)
                .await
            {
                tracing::warn!(container=%reaper_container, %error, "provider reaper cleanup failed");
            }
        });
    }
    (
        StatusCode::CREATED,
        Json(json!({
            "container": input.container,
            "sessionSubdir": input.session_subdir,
            "image": image
        })),
    )
        .into_response()
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
}

fn forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(json!({"error": message}))).into_response()
}

fn conflict(message: &str) -> Response {
    (StatusCode::CONFLICT, Json(json!({"error": message}))).into_response()
}

fn mutation_error(error: MutationError) -> Response {
    match error {
        MutationError::Conflict(message) => conflict(message),
        MutationError::Failed(message) => operation_error(message),
    }
}

fn operation_error(message: impl Into<String>) -> Response {
    let message = message.into();
    let message: String = message.chars().take(1024).collect();
    (StatusCode::BAD_GATEWAY, Json(json!({"error": message}))).into_response()
}

async fn read_bounded<R>(mut reader: R, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
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

/// Read at most `limit` bytes. Returning as soon as the next chunk would cross
/// the limit lets the caller terminate the child instead of buffering an
/// unbounded stream.
async fn read_capped<R>(mut reader: R, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok((captured, false));
        }
        if captured.len().saturating_add(read) > limit {
            return Ok((captured, true));
        }
        captured.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[test]
    fn rootful_outer_user_namespace_is_rejected() {
        assert!(has_host_root_mapping("         0          0 4294967295\n"));
        assert!(has_host_root_mapping("         0          0          1\n"));
        assert!(has_host_root_mapping("garbage\n"));
        assert!(has_host_root_mapping(""));
        assert!(!has_host_root_mapping(
            "         0       1000          1\n         1     100000      65536\n"
        ));
    }

    #[test]
    fn identities_are_strict() {
        assert!(is_valid_volume_name(
            "factory-ws-123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(!is_valid_volume_name("factory-ws-../../etc"));
        assert!(is_valid_container_name(
            "factory-exec-123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(is_valid_container_name(
            "factory-provider-123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(!is_valid_container_name("factory-provider-123"));
        assert!(!is_valid_container_name("factory-deps-123"));
        assert!(valid_provider_session_subdir("provider-session-a"));
        assert!(!valid_provider_session_subdir("../escape"));
        assert!(valid_workspace_id("attempt-1"));
    }

    #[test]
    fn dependency_registry_allowlist_is_strict() {
        assert!(parse_registries("sparse+https://index.crates.io/".into()).is_ok());
        assert!(parse_registries("a,b".into()).is_ok());
        assert!(parse_registries("a,,b".into()).is_err());
        assert!(parse_registries("a b".into()).is_err());
    }

    #[test]
    fn existing_egress_network_must_remain_internal_and_managed() {
        let valid = json!({
            "name": "factory-deps-egress",
            "driver": "bridge",
            "internal": true,
            "subnets": [{"subnet": "10.90.0.0/24", "gateway": "10.90.0.1"}],
            "labels": {"io.software-factory.managed": "true"}
        });
        assert!(validate_egress_network_inspection(&valid, "10.90.0.1", "10.90.0.0/24").is_ok());

        let mut non_internal = valid.clone();
        non_internal["internal"] = json!(false);
        let error = validate_egress_network_inspection(&non_internal, "10.90.0.1", "10.90.0.0/24")
            .expect_err("non-internal network must be rejected");
        assert!(error.contains("internal network"), "{error}");

        let mut unmanaged = valid.clone();
        unmanaged["labels"] = json!({});
        let error = validate_egress_network_inspection(&unmanaged, "10.90.0.1", "10.90.0.0/24")
            .expect_err("unmanaged network must be rejected");
        assert!(error.contains("managed label"), "{error}");

        let mut wrong_subnet = valid;
        wrong_subnet["subnets"][0]["gateway"] = json!("10.90.0.2");
        assert!(
            validate_egress_network_inspection(&wrong_subnet, "10.90.0.1", "10.90.0.0/24").is_err()
        );
    }

    #[test]
    fn argv_and_environment_are_bounded() {
        assert!(validate_argv(&["true".into()]).is_ok());
        assert!(validate_argv(&[]).is_err());
        assert!(
            validate_argv(&["x".repeat(MAX_ARG_BYTES + 1)]).is_err(),
            "oversized argv item accepted"
        );
        assert!(validate_env(&[("HOME".into(), "/tmp".into())]).is_ok());
        assert!(validate_env(&[("LD_PRELOAD".into(), "/x".into())]).is_err());
    }

    #[test]
    fn image_allowlist_rejects_flags_and_empty_entries() {
        assert!(parse_images("docker.io/library/alpine:3.20".into()).is_ok());
        assert!(parse_images("--privileged".into()).is_err());
        assert!(parse_images("a,,b".into()).is_err());
        assert!(parse_images("a b".into()).is_err());
    }

    #[test]
    fn import_images_are_strict() {
        assert!(parse_import_images("factory-provider:dev=/bundles/provider.tar".into()).is_ok());
        assert!(parse_import_images("factory-provider:dev=relative.tar".into()).is_err());
        assert!(parse_import_images("no-equals".into()).is_err());
    }

    #[test]
    fn owner_tokens_are_distinct_and_authenticate_exactly() {
        let directory =
            std::env::temp_dir().join(format!("factory-owner-tokens-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        for (owner, file) in OWNER_TOKEN_FILES {
            std::fs::write(
                directory.join(file),
                format!("{}-owner-token-0123456789abcdef0123456789", owner.as_str()),
            )
            .unwrap();
        }
        let tokens = OwnerTokens::from_directory(&directory).unwrap();
        for (owner, file) in OWNER_TOKEN_FILES {
            let mut headers = HeaderMap::new();
            headers.insert(
                "authorization",
                format!(
                    "Bearer {}",
                    std::fs::read_to_string(directory.join(file)).unwrap()
                )
                .trim()
                .parse()
                .unwrap(),
            );
            assert_eq!(tokens.authenticate(&headers), Some(owner));
        }
        let mut wrong = HeaderMap::new();
        wrong.insert("authorization", "Bearer wrong".parse().unwrap());
        assert_eq!(tokens.authenticate(&wrong), None);
        assert_eq!(tokens.authenticate(&HeaderMap::new()), None);

        std::fs::write(
            directory.join("runtime-broker-verifier.token"),
            std::fs::read_to_string(directory.join("runtime-broker-supervisor.token")).unwrap(),
        )
        .unwrap();
        assert!(OwnerTokens::from_directory(&directory).is_err());

        std::fs::write(directory.join("runtime-broker-verifier.token"), "short").unwrap();
        assert!(OwnerTokens::from_directory(&directory).is_err());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn constant_time_compare_requires_exact_match() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
    }

    #[tokio::test]
    async fn read_capped_stops_at_the_limit() {
        let input = vec![b'x'; 32 * 1024];
        let (bytes, overflow) = read_capped(&input[..], 1024).await.unwrap();
        assert!(overflow);
        assert!(bytes.len() <= 1024);

        let exact = vec![b'y'; 1024];
        let (bytes, overflow) = read_capped(&exact[..], 1024).await.unwrap();
        assert!(!overflow);
        assert_eq!(bytes.len(), 1024);

        let (bytes, overflow) = read_capped(&b"small"[..], 1024).await.unwrap();
        assert!(!overflow);
        assert_eq!(bytes, b"small");
    }

    #[tokio::test]
    async fn resource_locks_keep_one_identity_per_name_at_saturation() {
        let locks = ResourceLocks::new();
        let first = locks.acquire_with_limit("factory-ws-a", RESOURCE_LOCK_ENTRIES);
        assert!(Arc::ptr_eq(
            &first,
            &locks.acquire_with_limit("factory-ws-a", RESOURCE_LOCK_ENTRIES)
        ));
        assert!(!Arc::ptr_eq(
            &first,
            &locks.acquire_with_limit("factory-ws-b", RESOURCE_LOCK_ENTRIES)
        ));

        let bounded = ResourceLocks::new();
        let a = bounded.acquire_with_limit("a", 2);
        let a_identity = Arc::clone(&a);
        let b = bounded.acquire_with_limit("b", 2);
        let c = bounded.acquire_with_limit("c", 2);
        assert!(Arc::ptr_eq(&c, &bounded.overflow));

        let overflow_guard = c.lock().await;
        drop(a);
        let c_again = bounded.acquire_with_limit("c", 2);
        assert!(
            Arc::ptr_eq(&c, &c_again),
            "overflowed name received a second lock identity"
        );
        assert!(
            c_again.try_lock().is_err(),
            "same name bypassed the held overflow lock"
        );
        assert!(!bounded.entries.lock().unwrap().contains_key("c"));

        let a_again = bounded.acquire_with_limit("a", 2);
        assert!(
            Arc::ptr_eq(&a_again, &a_identity),
            "idle exact entry was reclaimed and replaced"
        );
        drop(overflow_guard);
        assert!(c_again.try_lock().is_ok());
        drop((b, c, c_again, a_again));
    }

    static FAKE_PODMAN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// Serialize fake-Podman tests. Parallel tests that fork while another test
    /// is writing an executable script can observe transient `ETXTBSY`; the
    /// guard is held for the whole test body by each caller.
    async fn fake_podman(script: &str) -> (PathBuf, tokio::sync::MutexGuard<'static, ()>) {
        let guard = FAKE_PODMAN_LOCK.lock().await;
        let path =
            std::env::temp_dir().join(format!("factory-fake-podman-{}", uuid::Uuid::new_v4()));
        let temporary = path.with_extension("tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut file = options.open(&temporary).unwrap();
        std::io::Write::write_all(&mut file, script.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);
        std::fs::rename(&temporary, &path).unwrap();
        (path, guard)
    }

    const TEST_SUPERVISOR_TOKEN: &str = "supervisor-token-0123456789abcdef0123456789";
    const TEST_VERIFIER_TOKEN: &str = "verifier-token-0123456789abcdef012345678901";
    const TEST_PROVIDER_TOKEN: &str = "provider-token-0123456789abcdef012345678901";

    fn test_app(podman: PathBuf) -> Arc<App> {
        test_app_with_lock_limit(podman, RESOURCE_LOCK_ENTRIES)
    }

    fn test_app_with_lock_limit(podman: PathBuf, lock_limit: usize) -> Arc<App> {
        Arc::new(App {
            tokens: Arc::new(OwnerTokens {
                entries: vec![
                    (ResourceOwner::Supervisor, TEST_SUPERVISOR_TOKEN.into()),
                    (ResourceOwner::Verifier, TEST_VERIFIER_TOKEN.into()),
                    (ResourceOwner::Provider, TEST_PROVIDER_TOKEN.into()),
                ],
            }),
            resource_locks: Arc::new(ResourceLocks::new()),
            lock_limit,
            active: Arc::new(ActiveExecutions::new()),
            podman,
            images: Arc::new(BTreeSet::from(["test/image:1".to_owned()])),
            provider_images: Arc::new(BTreeSet::new()),
            provider_kind: String::new(),
            provider_session_root: std::env::temp_dir(),
            import_images: Vec::new(),
            dependency_acquisition: Arc::new(RwLock::new(DependencyAcquisitionState::disabled(
                DEPENDENCY_RUNTIME_UNAVAILABLE,
                "test configuration",
            ))),
        })
    }

    fn test_headers(owner: ResourceOwner) -> HeaderMap {
        let token = match owner {
            ResourceOwner::Supervisor => TEST_SUPERVISOR_TOKEN,
            ResourceOwner::Verifier => TEST_VERIFIER_TOKEN,
            ResourceOwner::Provider => TEST_PROVIDER_TOKEN,
        };
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        headers
    }

    async fn response_body(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn remove_volume_requires_the_authenticated_owner() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let marker =
            std::env::temp_dir().join(format!("factory-fake-rm-volume-{}", uuid::Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "exists" ]; then
  exit 0
fi
exit 1
"#,
            marker = marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let unauthenticated = remove_volume(
            State(Arc::clone(&app)),
            HeaderMap::new(),
            AxumPath(volume.clone()),
        )
        .await;
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        assert!(!marker.exists());

        let spoofed = remove_volume(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Verifier),
            AxumPath(volume.clone()),
        )
        .await;
        let spoofed_status = spoofed.status();
        let spoofed_body = response_body(spoofed).await;
        assert_eq!(spoofed_status, StatusCode::CONFLICT, "body: {spoofed_body}");
        assert!(!marker.exists());

        let accepted = remove_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(volume),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        assert!(marker.exists());
    }

    #[tokio::test]
    async fn import_export_execute_and_provider_start_require_the_authenticated_owner() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let marker =
            std::env::temp_dir().join(format!("factory-fake-owner-path-{}", uuid::Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"verifier"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "import" ]; then touch "{marker}"; exit 0; fi
if [ "$1" = "volume" ] && [ "$2" = "export" ]; then touch "{marker}"; exit 0; fi
if [ "$1" = "run" ]; then touch "{marker}"; exit 0; fi
exit 1
"#,
            marker = marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let import = import_volume(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(volume.clone()),
            Bytes::from_static(b"archive"),
        )
        .await;
        assert_eq!(import.status(), StatusCode::CONFLICT);
        assert!(!marker.exists());

        let export = export_volume(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(volume.clone()),
        )
        .await;
        assert_eq!(export.status(), StatusCode::CONFLICT);
        assert!(!marker.exists());

        let execute = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume.clone(),
                container,
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        )
        .await;
        assert_eq!(execute.status(), StatusCode::CONFLICT);
        assert!(!marker.exists());

        let provider = start_provider(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(StartProvider {
                container: format!("factory-provider-{}", uuid::Uuid::new_v4()),
                session_subdir: "provider-session-a".into(),
                timeout_secs: 30,
                env: Vec::new(),
            }),
        )
        .await;
        assert_eq!(provider.status(), StatusCode::FORBIDDEN);
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn managed_inventory_is_scoped_to_the_authenticated_owner() {
        let supervisor_volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let verifier_volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "ls" ]; then
  printf '%s\n' '[{{"Name":"{supervisor_volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}},{{"Name":"{verifier_volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"verifier"}}}}]'
  exit 0
fi
if [ "$1" = "ps" ]; then
  printf '%s\n' '[]'
  exit 0
fi
exit 1
"#
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let supervisor = app
            .managed_resources(ResourceOwner::Supervisor)
            .await
            .unwrap();
        assert_eq!(supervisor.volumes.len(), 1);
        assert_eq!(supervisor.volumes[0].name, supervisor_volume);
        let verifier = app
            .managed_resources(ResourceOwner::Verifier)
            .await
            .unwrap();
        assert_eq!(verifier.volumes.len(), 1);
        assert_eq!(verifier.volumes[0].name, verifier_volume);
    }

    #[tokio::test]
    async fn remove_volume_rejects_missing_owner_and_missing_resource() {
        let unlabelled = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let missing = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  if [ "$5" = "{missing}" ]; then
    exit 1
  fi
  printf '%s\n' '[{{"Name":"{unlabelled}","Labels":{{"io.software-factory.managed":"true"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "exists" ]; then
  exit 1
fi
exit 1
"#
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = remove_volume(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(unlabelled),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let response = remove_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(missing),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn create_volume_refuses_a_preexisting_foreign_volume() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let rm_marker =
            std::env::temp_dir().join(format!("factory-fake-create-rm-{}", uuid::Uuid::new_v4()));
        let create_marker =
            std::env::temp_dir().join(format!("factory-fake-create-new-{}", uuid::Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"verifier","io.software-factory.workspace":"other-workspace"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  touch "{create_marker}"
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display(),
            create_marker = create_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = create_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume,
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(!rm_marker.exists());
        assert!(!create_marker.exists());
    }

    #[tokio::test]
    async fn create_volume_reuses_a_matching_owned_workspace() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let rm_marker =
            std::env::temp_dir().join(format!("factory-fake-reuse-rm-{}", uuid::Uuid::new_v4()));
        let create_marker = std::env::temp_dir().join(format!(
            "factory-fake-reuse-create-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"workspace-a"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  touch "{create_marker}"
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display(),
            create_marker = create_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = create_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume,
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(rm_marker.exists());
        assert!(create_marker.exists());
    }

    #[tokio::test]
    async fn create_volume_adopts_a_matching_legacy_workspace_volume() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let rm_marker =
            std::env::temp_dir().join(format!("factory-fake-legacy-rm-{}", uuid::Uuid::new_v4()));
        let create_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-create-{}",
            uuid::Uuid::new_v4()
        ));
        let args_file =
            std::env::temp_dir().join(format!("factory-fake-legacy-args-{}", uuid::Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.workspace":"workspace-a"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  printf '%s\n' "$@" > "{args_file}"
  touch "{create_marker}"
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display(),
            create_marker = create_marker.display(),
            args_file = args_file.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = create_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume,
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(rm_marker.exists());
        assert!(create_marker.exists());
        let args = std::fs::read_to_string(&args_file).unwrap();
        assert!(args.contains("io.software-factory.workspace=workspace-a"));
        assert!(args.contains("io.software-factory.owner=supervisor"));
        assert!(args.contains("io.software-factory.managed=true"));
        let _ = std::fs::remove_file(&rm_marker);
        let _ = std::fs::remove_file(&create_marker);
        let _ = std::fs::remove_file(&args_file);
    }

    #[tokio::test]
    async fn create_volume_preserves_a_legacy_volume_for_another_workspace() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let rm_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-mismatch-rm-{}",
            uuid::Uuid::new_v4()
        ));
        let create_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-mismatch-create-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.workspace":"workspace-b"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  touch "{create_marker}"
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display(),
            create_marker = create_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = create_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume,
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(!rm_marker.exists());
        assert!(!create_marker.exists());
    }

    #[tokio::test]
    async fn create_volume_rejects_an_active_legacy_volume() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let rm_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-active-rm-{}",
            uuid::Uuid::new_v4()
        ));
        let create_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-active-create-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  touch "{create_marker}"
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display(),
            create_marker = create_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        assert!(app.active.register(&volume, &container));
        let response = create_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume,
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(!rm_marker.exists());
        assert!(!create_marker.exists());
    }

    #[tokio::test]
    async fn create_volume_preserves_an_unlabelled_volume() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let rm_marker = std::env::temp_dir().join(format!(
            "factory-fake-unlabelled-rm-{}",
            uuid::Uuid::new_v4()
        ));
        let create_marker = std::env::temp_dir().join(format!(
            "factory-fake-unlabelled-create-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  touch "{create_marker}"
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display(),
            create_marker = create_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = create_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume,
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(!rm_marker.exists());
        assert!(!create_marker.exists());
    }

    #[tokio::test]
    async fn remove_container_requires_the_expected_owner_label() {
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let marker = std::env::temp_dir().join(format!(
            "factory-fake-rm-container-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "inspect" ]; then
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"provider"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  touch "{marker}"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 0
fi
exit 1
"#,
            marker = marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let rejected = remove_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(container.clone()),
        )
        .await;
        let rejected_status = rejected.status();
        let rejected_body = response_body(rejected).await;
        assert_eq!(
            rejected_status,
            StatusCode::CONFLICT,
            "body: {rejected_body}"
        );
        assert!(!marker.exists());

        let accepted = remove_container(
            State(app),
            test_headers(ResourceOwner::Provider),
            AxumPath(container),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::NO_CONTENT);
        assert!(marker.exists());
    }

    #[tokio::test]
    async fn execution_cleanup_adopts_a_matching_legacy_container() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let rm_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-container-rm-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "inspect" ]; then
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.workspace":"{volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 1
fi
exit 1
"#,
            rm_marker = rm_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        assert!(
            app.confirm_execution_cleanup(&container, ResourceOwner::Supervisor, &volume)
                .await
        );
        assert!(rm_marker.exists());
    }

    #[tokio::test]
    async fn execution_cleanup_preserves_a_legacy_container_for_another_workspace() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let other_volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let rm_marker = std::env::temp_dir().join(format!(
            "factory-fake-legacy-container-mismatch-rm-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "inspect" ]; then
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.workspace":"{other_volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 0
fi
exit 1
"#,
            rm_marker = rm_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        assert!(
            !app.confirm_execution_cleanup(&container, ResourceOwner::Supervisor, &volume)
                .await
        );
        assert!(!rm_marker.exists());
    }

    #[tokio::test]
    async fn inspect_output_is_bounded_before_parsing() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let script = r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  head -c 5000000 /dev/zero | tr '\0' 'x'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "exists" ]; then
  exit 0
fi
exit 1
"#;
        let (podman, _serial) = fake_podman(script).await;
        let app = test_app(podman);
        assert!(app.volume_labels(&volume).await.is_err());
    }

    fn stateful_volume_script(name: &str, state: &Path, log: &Path) -> String {
        format!(
            r#"#!/bin/sh
state="{state}"
log="{log}"
name="{name}"
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  echo inspect >> "$log"
  if [ -f "$state" ]; then
    cat "$state"
    exit 0
  fi
  exit 1
fi
if [ "$1" = "volume" ] && [ "$2" = "exists" ]; then
  if [ -f "$state" ]; then exit 0; else exit 1; fi
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  echo rm >> "$log"
  rm -f "$state"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  owner=""
  workspace=""
  for arg in "$@"; do
    case "$arg" in
      io.software-factory.owner=*) owner="${{arg#*=}}" ;;
      io.software-factory.workspace=*) workspace="${{arg#*=}}" ;;
    esac
  done
  echo create-start >> "$log"
  sleep 0.2
  printf '[{{"Name":"%s","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"%s","io.software-factory.workspace":"%s"}}}}]' "$name" "$owner" "$workspace" > "$state"
  echo create-end >> "$log"
  exit 0
fi
exit 1
"#,
            state = state.display(),
            log = log.display()
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_create_for_different_owners_serializes_per_name() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let state =
            std::env::temp_dir().join(format!("factory-stateful-volume-{}", uuid::Uuid::new_v4()));
        let log = std::env::temp_dir().join(format!(
            "factory-stateful-volume-log-{}",
            uuid::Uuid::new_v4()
        ));
        let (podman, _serial) = fake_podman(&stateful_volume_script(&volume, &state, &log)).await;
        let app = test_app(podman);
        let (supervisor, verifier) = tokio::join!(
            create_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                Json(NewVolume {
                    volume: volume.clone(),
                    workspace_id: "workspace-a".into(),
                }),
            ),
            create_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Verifier),
                Json(NewVolume {
                    volume: volume.clone(),
                    workspace_id: "workspace-a".into(),
                }),
            ),
        );
        let supervisor_status = supervisor.status();
        let supervisor_body = response_body(supervisor).await;
        let verifier_status = verifier.status();
        let verifier_body = response_body(verifier).await;
        let statuses = [supervisor_status, verifier_status];
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::CREATED)
                .count(),
            1,
            "supervisor={supervisor_status} {supervisor_body}; verifier={verifier_status} {verifier_body}"
        );
        assert_eq!(
            statuses
                .iter()
                .filter(|status| **status == StatusCode::CONFLICT)
                .count(),
            1,
            "supervisor={supervisor_status} {supervisor_body}; verifier={verifier_status} {verifier_body}"
        );
        let state = std::fs::read_to_string(&state).unwrap();
        let created_owner = if supervisor_status == StatusCode::CREATED {
            "supervisor"
        } else {
            "verifier"
        };
        assert!(state.contains(created_owner));
        let _ = std::fs::remove_file(&log);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn execution_cleanup_serializes_with_container_delete() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let log =
            std::env::temp_dir().join(format!("factory-cleanup-lock-log-{}", uuid::Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
log="{log}"
volume="{volume}"
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "run" ]; then
  sleep 0.25
  exit 0
fi
if [ "$1" = "inspect" ]; then
  echo inspect-start >> "$log"
  sleep 0.4
  echo inspect-end >> "$log"
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"{volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  echo rm >> "$log"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 1
fi
exit 1
"#,
            log = log.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let run = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume.clone(),
                container: container.clone(),
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        );
        let delete = async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            remove_container(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                AxumPath(container),
            )
            .await
        };
        let (run_response, delete_response) = tokio::join!(run, delete);
        let run_status = run_response.status();
        let run_body = response_body(run_response).await;
        assert_eq!(run_status, StatusCode::OK, "run body: {run_body}");
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
        let log = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert!(!lines.is_empty() && lines.len().is_multiple_of(3));
        for chunk in lines.chunks(3) {
            assert_eq!(
                chunk,
                ["inspect-start", "inspect-end", "rm"],
                "execution cleanup interleaved with container deletion: {lines:?}"
            );
        }
        let _ = std::fs::remove_file(&log);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn execution_cleanup_works_when_volume_and_container_share_overflow() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "run" ]; then
  sleep 0.1
  exit 0
fi
if [ "$1" = "inspect" ]; then
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"{volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 1
fi
exit 1
"#
        );
        // A zero-entry exact table sends both the volume and container names to
        // the shared overflow mutex. This is the former self-deadlock case.
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app_with_lock_limit(podman, 0);
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            run_container(
                State(app),
                test_headers(ResourceOwner::Supervisor),
                Json(RunContainer {
                    volume,
                    container,
                    image: "test/image:1".into(),
                    argv: vec!["/bin/true".into()],
                    timeout_secs: 10,
                }),
            ),
        )
        .await
        .expect("execution cleanup deadlocked on the shared overflow lock");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn active_execution_blocks_volume_mutation_without_holding_the_lock() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "run" ]; then
  sleep 0.5
  exit 0
fi
if [ "$1" = "inspect" ]; then
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"{volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 1
fi
exit 1
"#
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let run = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume.clone(),
                container: format!("factory-exec-{}", uuid::Uuid::new_v4()),
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        );
        tokio::pin!(run);
        tokio::select! {
            biased;
            result = &mut run => panic!("execution finished before registration probe: {}", result.status()),
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
        let removed = tokio::time::timeout(
            Duration::from_millis(500),
            remove_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                AxumPath(volume.clone()),
            ),
        )
        .await
        .expect("volume removal blocked on a lock held by the execution");
        assert_eq!(removed.status(), StatusCode::CONFLICT);
        let created = create_volume(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(NewVolume {
                volume: volume.clone(),
                workspace_id: "workspace-a".into(),
            }),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CONFLICT);
        assert_eq!(run.await.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_execution_keeps_volume_blocked_until_cleanup_is_confirmed() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let log =
            std::env::temp_dir().join(format!("factory-cancel-exec-log-{}", uuid::Uuid::new_v4()));
        let state = std::env::temp_dir().join(format!(
            "factory-cancel-exec-state-{}",
            uuid::Uuid::new_v4()
        ));
        let cleanup = std::env::temp_dir().join(format!(
            "factory-cancel-exec-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
log="{log}"
state="{state}"
cleanup="{cleanup}"
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  exit 0
fi
if [ "$1" = "run" ]; then
  echo run-start >> "$log"
  touch "$state"
  sleep 0.5
  exit 0
fi
if [ "$1" = "inspect" ]; then
  if [ ! -f "$state" ]; then exit 1; fi
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"{volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  rm -f "$state"
  touch "$cleanup"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  if [ -f "$state" ]; then exit 0; else exit 1; fi
fi
exit 1
"#,
            log = log.display(),
            state = state.display(),
            cleanup = cleanup.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let handler = tokio::spawn(run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume.clone(),
                container,
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        ));
        for _ in 0..100 {
            if std::fs::read_to_string(&log)
                .map(|log| log.contains("run-start"))
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handler.abort();
        let _ = handler.await;

        let blocked = remove_volume(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(volume.clone()),
        )
        .await;
        assert_eq!(
            blocked.status(),
            StatusCode::CONFLICT,
            "cancelled execution released the volume before cleanup confirmation"
        );

        let mut released = None;
        for _ in 0..100 {
            let response = remove_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                AxumPath(volume.clone()),
            )
            .await;
            if response.status() == StatusCode::NO_CONTENT {
                released = Some(response.status());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(released, Some(StatusCode::NO_CONTENT));
        assert!(cleanup.exists());
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(&cleanup);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn normal_execution_releases_the_volume_marker_after_cleanup() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let state = std::env::temp_dir().join(format!(
            "factory-normal-exec-state-{}",
            uuid::Uuid::new_v4()
        ));
        let cleanup = std::env::temp_dir().join(format!(
            "factory-normal-exec-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
state="{state}"
cleanup="{cleanup}"
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  exit 0
fi
if [ "$1" = "run" ]; then
  touch "$state"
  exit 0
fi
if [ "$1" = "inspect" ]; then
  if [ ! -f "$state" ]; then exit 1; fi
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"{volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  rm -f "$state"
  touch "$cleanup"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  if [ -f "$state" ]; then exit 0; else exit 1; fi
fi
exit 1
"#,
            state = state.display(),
            cleanup = cleanup.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume.clone(),
                container,
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(cleanup.exists());
        let removed = remove_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(volume),
        )
        .await;
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        let _ = std::fs::remove_file(&cleanup);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn duplicate_active_container_name_is_rejected_without_removal() {
        let volume_a = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let volume_b = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let state = std::env::temp_dir().join(format!(
            "factory-duplicate-active-state-{}",
            uuid::Uuid::new_v4()
        ));
        let runlog = std::env::temp_dir().join(format!(
            "factory-duplicate-active-run-{}",
            uuid::Uuid::new_v4()
        ));
        let cleanup = std::env::temp_dir().join(format!(
            "factory-duplicate-active-cleanup-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
state="{state}"
runlog="{runlog}"
cleanup="{cleanup}"
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '[{{"Name":"%s","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]\n' "$5"
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "rm" ]; then
  exit 0
fi
if [ "$1" = "run" ]; then
  workspace=""
  for arg in "$@"; do
    case "$arg" in
      --label=io.software-factory.workspace=*) workspace="${{arg##*workspace=}}" ;;
    esac
  done
  echo run-start >> "$runlog"
  echo "$workspace" > "$state"
  sleep 1.0
  exit 0
fi
if [ "$1" = "inspect" ]; then
  if [ ! -f "$state" ]; then exit 1; fi
  workspace="$(cat "$state")"
  printf '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"%s"}}}}}}]' "$workspace"
  exit 0
fi
if [ "$1" = "rm" ]; then
  rm -f "$state"
  touch "$cleanup"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  if [ -f "$state" ]; then exit 0; else exit 1; fi
fi
exit 1
"#,
            state = state.display(),
            runlog = runlog.display(),
            cleanup = cleanup.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let first = tokio::spawn(run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume_a.clone(),
                container: container.clone(),
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        ));
        for _ in 0..100 {
            if std::fs::read_to_string(&runlog)
                .map(|log| log.lines().count() == 1)
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let duplicate = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume_b,
                container: container.clone(),
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        )
        .await;
        let duplicate_status = duplicate.status();
        let duplicate_body = response_body(duplicate).await;
        assert_eq!(
            duplicate_status,
            StatusCode::CONFLICT,
            "duplicate body: {duplicate_body}"
        );
        assert_eq!(
            std::fs::read_to_string(&runlog).unwrap().lines().count(),
            1,
            "duplicate execution started its own container"
        );
        assert_eq!(std::fs::read_to_string(&state).unwrap().trim(), volume_a);
        assert!(!cleanup.exists());
        assert_eq!(first.await.unwrap().status(), StatusCode::OK);
        let removed = remove_volume(
            State(app),
            test_headers(ResourceOwner::Supervisor),
            AxumPath(volume_a),
        )
        .await;
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        let _ = std::fs::remove_file(&runlog);
        let _ = std::fs::remove_file(&cleanup);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn duplicate_present_container_with_foreign_workspace_is_not_removed() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let foreign_volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let container = format!("factory-exec-{}", uuid::Uuid::new_v4());
        let run_marker = std::env::temp_dir().join(format!(
            "factory-duplicate-present-run-{}",
            uuid::Uuid::new_v4()
        ));
        let rm_marker = std::env::temp_dir().join(format!(
            "factory-duplicate-present-rm-{}",
            uuid::Uuid::new_v4()
        ));
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  printf '%s\n' '[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "run" ]; then
  touch "{run_marker}"
  exit 0
fi
if [ "$1" = "inspect" ]; then
  printf '%s\n' '[{{"Config":{{"Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"{foreign_volume}"}}}}}}]'
  exit 0
fi
if [ "$1" = "rm" ]; then
  touch "{rm_marker}"
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 0
fi
exit 1
"#,
            run_marker = run_marker.display(),
            rm_marker = rm_marker.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let response = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: volume.clone(),
                container: container.clone(),
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(!run_marker.exists());
        assert!(!rm_marker.exists());

        // Defense in depth: even if the preflight is bypassed, execution cleanup
        // refuses a container whose workspace label is not this volume.
        assert!(
            !app.confirm_execution_cleanup(&container, ResourceOwner::Supervisor, &volume)
                .await
        );
        assert!(!rm_marker.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn long_execution_does_not_block_an_unrelated_resource() {
        let running_volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let other_volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let created =
            std::env::temp_dir().join(format!("factory-unrelated-create-{}", uuid::Uuid::new_v4()));
        let script = format!(
            r#"#!/bin/sh
other="{other_volume}"
marker="{created}"
if [ "$1" = "volume" ] && [ "$2" = "inspect" ]; then
  if [ "$5" = "$other" ]; then
    exit 1
  fi
  printf '%s\n' '[{{"Name":"{running_volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor"}}}}]'
  exit 0
fi
if [ "$1" = "volume" ] && [ "$2" = "exists" ]; then
  if [ "$3" = "$other" ]; then exit 1; fi
  exit 0
fi
if [ "$1" = "run" ]; then
  sleep 0.8
  exit 0
fi
if [ "$1" = "container" ] && [ "$2" = "exists" ]; then
  exit 1
fi
if [ "$1" = "volume" ] && [ "$2" = "create" ]; then
  touch "$marker"
  exit 0
fi
exit 1
"#,
            created = created.display()
        );
        let (podman, _serial) = fake_podman(&script).await;
        let app = test_app(podman);
        let run = run_container(
            State(Arc::clone(&app)),
            test_headers(ResourceOwner::Supervisor),
            Json(RunContainer {
                volume: running_volume,
                container: format!("factory-exec-{}", uuid::Uuid::new_v4()),
                image: "test/image:1".into(),
                argv: vec!["/bin/true".into()],
                timeout_secs: 10,
            }),
        );
        tokio::pin!(run);
        tokio::select! {
            biased;
            result = &mut run => panic!(
                "long execution finished before the unrelated create: {}",
                result.status()
            ),
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
        let create = async {
            create_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                Json(NewVolume {
                    volume: other_volume,
                    workspace_id: "workspace-other".into(),
                }),
            )
            .await
        };
        let created_response = tokio::time::timeout(Duration::from_millis(400), create)
            .await
            .expect("unrelated create was blocked by a long execution");
        assert_eq!(created_response.status(), StatusCode::CREATED);
        assert!(created.exists());
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut run)
                .await
                .is_err(),
            "the long execution unexpectedly finished before the 100 ms probe"
        );
        assert_eq!(run.await.status(), StatusCode::OK);
        let _ = std::fs::remove_file(&created);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_delete_and_reset_never_removes_the_replacement() {
        let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
        let state =
            std::env::temp_dir().join(format!("factory-stateful-replace-{}", uuid::Uuid::new_v4()));
        let log = std::env::temp_dir().join(format!(
            "factory-stateful-replace-log-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &state,
            format!(
                r#"[{{"Name":"{volume}","Labels":{{"io.software-factory.managed":"true","io.software-factory.owner":"supervisor","io.software-factory.workspace":"workspace-a"}}}}]"#
            ),
        )
        .unwrap();
        let (podman, _serial) = fake_podman(&stateful_volume_script(&volume, &state, &log)).await;
        let app = test_app(podman);
        let (created, removed) = tokio::join!(
            create_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                Json(NewVolume {
                    volume: volume.clone(),
                    workspace_id: "workspace-a".into(),
                }),
            ),
            remove_volume(
                State(Arc::clone(&app)),
                test_headers(ResourceOwner::Supervisor),
                AxumPath(volume.clone()),
            ),
        );
        assert_eq!(created.status(), StatusCode::CREATED);
        assert_eq!(removed.status(), StatusCode::NO_CONTENT);
        let log = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        let start = lines
            .iter()
            .position(|line| *line == "create-start")
            .unwrap();
        let end = lines.iter().position(|line| *line == "create-end").unwrap();
        assert!(start < end);
        assert!(
            !lines[start + 1..end].contains(&"rm"),
            "delete interleaved with create reset: {lines:?}"
        );
        let _ = std::fs::remove_file(&state);
        let _ = std::fs::remove_file(&log);
    }

    #[test]
    fn dependency_health_projection_reports_enabled_and_disabled_state() {
        let enabled = DependencyAcquisitionState::enabled(
            "factory-deps-egress".into(),
            "http://10.90.0.1:18080".into(),
            "wildcard-subnet-scoped",
        );
        let projection = enabled.health_projection();
        assert_eq!(projection["enabled"], true);
        assert_eq!(projection["network"], "factory-deps-egress");
        assert_eq!(projection["proxy"], "http://10.90.0.1:18080");
        assert_eq!(projection["binding"], "wildcard-subnet-scoped");

        let disabled = DependencyAcquisitionState::disabled(
            DEPENDENCY_EGRESS_UNAVAILABLE,
            "dependency egress gateway 10.90.0.1:18080 is unavailable",
        );
        let projection = disabled.health_projection();
        assert_eq!(projection["enabled"], false);
        assert_eq!(projection["code"], DEPENDENCY_EGRESS_UNAVAILABLE);
        assert_eq!(
            projection["reason"],
            "dependency egress gateway 10.90.0.1:18080 is unavailable"
        );
        assert!(projection.get("network").is_none());
    }

    #[tokio::test]
    async fn disabled_dependency_route_returns_typed_unavailable_response() {
        let app = test_app(std::env::current_exe().unwrap());
        app.set_dependency_acquisition(DependencyAcquisitionState::disabled(
            DEPENDENCY_EGRESS_UNAVAILABLE,
            "dependency egress gateway is not assigned",
        ));
        let router = Router::new()
            .route(
                factory_native::dependency_service::DEPENDENCY_ACQUIRE_ROUTE,
                post(dependency_unavailable),
            )
            .with_state(app);

        let unauthorized = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(factory_native::dependency_service::DEPENDENCY_ACQUIRE_ROUTE)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(factory_native::dependency_service::DEPENDENCY_ACQUIRE_ROUTE)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {TEST_SUPERVISOR_TOKEN}"))
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await).unwrap();
        assert_eq!(body["code"], DEPENDENCY_EGRESS_UNAVAILABLE);
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|error| error.contains("gateway is not assigned"))
        );
        assert_eq!(body["reason"], "dependency egress gateway is not assigned");
    }

    #[tokio::test]
    async fn health_route_exposes_dependency_availability_reason() {
        let app = test_app(std::env::current_exe().unwrap());
        app.set_dependency_acquisition(DependencyAcquisitionState::disabled(
            DEPENDENCY_RUNTIME_UNAVAILABLE,
            "dependency image is not available",
        ));
        let router = Router::new().route("/health", get(health)).with_state(app);
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await).unwrap();
        assert_eq!(body["healthy"], true);
        assert_eq!(body["dependencyAcquisition"]["enabled"], false);
        assert_eq!(
            body["dependencyAcquisition"]["code"],
            DEPENDENCY_RUNTIME_UNAVAILABLE
        );
        assert_eq!(
            body["dependencyAcquisition"]["reason"],
            "dependency image is not available"
        );
    }
}
