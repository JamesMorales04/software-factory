//! Trusted local Sandbox Supervisor HTTP service.
//!
//! This is a candidate `TRUSTED_LOCAL` boundary, not certified isolation. It
//! owns durable per-Attempt Git worktrees and executes project code only in an
//! allowlisted rootless Podman image with a named volume. No host workspace
//! pathname is passed to Podman, so replacing the host path cannot redirect the
//! sandbox mount. The service never exposes the Podman socket, provider
//! credentials, Core tokens or host HOME. Failures are closed.

use std::{
    collections::{BTreeSet, HashMap},
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use factory_native::{
    execution::{
        ApplyFilesRequest, BoundedOutput, BrokerVolumes, ExecuteWorkspace, ExecutionError,
        MAX_EXECUTION_RUNTIME, RootlessPodmanVolumes, ToolRunRequest, VolumeBackend,
        WorkspaceAllocation, WorkspaceManager, WriteFilesRequest,
    },
    workspace::WorkspaceError,
    workspace_archive::ArchiveError,
};
use serde_json::json;

const DEFAULT_BIND: &str = "127.0.0.1:8766";
const MIN_TOKEN_BYTES: usize = 32;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_ARGV_ITEMS: usize = 256;
const MAX_ARG_BYTES: usize = 16 * 1024;
const MAX_ARGV_BYTES: usize = 64 * 1024;

const OTHER_SECRET_VARIABLES: [&str; 5] = [
    "FACTORY_OPERATOR_TOKEN",
    "FACTORY_WORKER_BOOTSTRAP_TOKEN",
    "FACTORY_WORKER_REGISTRATION_KEY",
    "FACTORY_VERIFIER_TOKEN",
    "FACTORY_REVIEWER_TOKEN",
];

enum SupervisorBackend {
    Podman(RootlessPodmanVolumes),
    Broker(BrokerVolumes),
    #[cfg(debug_assertions)]
    Local(LocalVolumeBackend),
}

impl VolumeBackend for SupervisorBackend {
    async fn create_volume(&self, name: &str, workspace_id: &str) -> Result<(), ExecutionError> {
        match self {
            Self::Podman(backend) => backend.create_volume(name, workspace_id).await,
            Self::Broker(backend) => backend.create_volume(name, workspace_id).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.create_volume(name, workspace_id).await,
        }
    }
    async fn remove_volume(&self, name: &str) -> Result<(), ExecutionError> {
        match self {
            Self::Podman(backend) => backend.remove_volume(name).await,
            Self::Broker(backend) => backend.remove_volume(name).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.remove_volume(name).await,
        }
    }
    async fn import_volume(&self, name: &str, bytes: &[u8]) -> Result<(), ExecutionError> {
        match self {
            Self::Podman(backend) => backend.import_volume(name, bytes).await,
            Self::Broker(backend) => backend.import_volume(name, bytes).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.import_volume(name, bytes).await,
        }
    }
    async fn export_volume(&self, name: &str) -> Result<Vec<u8>, ExecutionError> {
        match self {
            Self::Podman(backend) => backend.export_volume(name).await,
            Self::Broker(backend) => backend.export_volume(name).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.export_volume(name).await,
        }
    }
    async fn run(
        &self,
        volume: &str,
        container: &str,
        image: &str,
        argv: &[String],
        timeout: Duration,
    ) -> Result<BoundedOutput, ExecutionError> {
        match self {
            Self::Podman(backend) => backend.run(volume, container, image, argv, timeout).await,
            Self::Broker(backend) => backend.run(volume, container, image, argv, timeout).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.run(volume, container, image, argv, timeout).await,
        }
    }
    async fn remove_container(&self, name: &str) -> Result<(), ExecutionError> {
        match self {
            Self::Podman(backend) => backend.remove_container(name).await,
            Self::Broker(backend) => backend.remove_container(name).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.remove_container(name).await,
        }
    }
}

#[cfg(debug_assertions)]
struct LocalVolumeBackend {
    root: PathBuf,
    volumes: std::sync::Mutex<std::collections::HashMap<String, PathBuf>>,
}

#[cfg(debug_assertions)]
impl LocalVolumeBackend {
    fn new(root: PathBuf) -> Result<Self, ExecutionError> {
        std::fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            root,
            volumes: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    fn path(&self, name: &str) -> Result<PathBuf, ExecutionError> {
        self.volumes
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| ExecutionError::Volume("missing test volume".into()))
    }
}

#[cfg(debug_assertions)]
impl VolumeBackend for LocalVolumeBackend {
    async fn create_volume(&self, name: &str, _workspace_id: &str) -> Result<(), ExecutionError> {
        let path = self.root.join(name);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir(&path)?;
        self.volumes.lock().unwrap().insert(name.into(), path);
        Ok(())
    }
    async fn remove_volume(&self, name: &str) -> Result<(), ExecutionError> {
        if let Some(path) = self.volumes.lock().unwrap().remove(name) {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(())
    }
    async fn import_volume(&self, name: &str, bytes: &[u8]) -> Result<(), ExecutionError> {
        let path = self.path(name)?;
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        factory_native::workspace_archive::extract(bytes, &path)?;
        self.volumes.lock().unwrap().insert(name.into(), path);
        Ok(())
    }
    async fn export_volume(&self, name: &str) -> Result<Vec<u8>, ExecutionError> {
        Ok(factory_native::workspace_archive::snapshot(&self.path(name)?)?.bytes)
    }
    async fn run(
        &self,
        volume: &str,
        _container: &str,
        _image: &str,
        argv: &[String],
        timeout: Duration,
    ) -> Result<BoundedOutput, ExecutionError> {
        let path = self.path(volume)?;
        let argv = argv.to_vec();
        let output = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .current_dir(path)
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .env("HOME", "/tmp")
                    .output()
            }),
        )
        .await
        .map_err(|_| ExecutionError::Volume("local test execution timed out".into()))?
        .map_err(|error| ExecutionError::Volume(format!("local test runner: {error}")))??;
        Ok(BoundedOutput {
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }
    async fn remove_container(&self, _name: &str) -> Result<(), ExecutionError> {
        Ok(())
    }
}

struct App<B: factory_native::execution::VolumeBackend> {
    token: String,
    manager: WorkspaceManager<B>,
    max_runtime: Duration,
    allowed_images: BTreeSet<String>,
    project_roots: Vec<PathBuf>,
    workspace_root: PathBuf,
    attempt_grant_key: Vec<u8>,
    capability_key: Vec<u8>,
    operation_nonces: Mutex<HashMap<String, i64>>,
    core_url: reqwest::Url,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("factory_native=info")
        .with_writer(factory_native::telemetry::RedactingMakeWriter)
        .init();
    let token = factory_native::secrets::required("FACTORY_SUPERVISOR_TOKEN")?;
    validate_supervisor_token(&token)?;
    ensure_distinct_from_other_roles(&token)?;
    factory_native::telemetry::register_secret(&token);
    let attempt_grant_key = factory_native::secrets::required("FACTORY_ATTEMPT_GRANT_KEY")?;
    let capability_key = factory_native::secrets::required("FACTORY_SUPERVISOR_CAPABILITY_KEY")?;
    if attempt_grant_key.len() < factory_native::attempt_grant::MIN_KEY_BYTES
        || capability_key.len() < factory_native::attempt_grant::MIN_KEY_BYTES
    {
        return Err("Attempt grant and capability keys must be at least 32 bytes".into());
    }
    factory_native::telemetry::register_secret(&attempt_grant_key);
    factory_native::telemetry::register_secret(&capability_key);
    let container_mode = env::var("FACTORY_CONTAINER_MODE").as_deref() == Ok("1");
    let core_url = factory_native::service_network::validate_http_origin(
        &env::var("FACTORY_CORE_URL").map_err(|_| "FACTORY_CORE_URL required")?,
        "FACTORY_CORE_URL",
        container_mode,
        &["core"],
    )?;
    let workspace_root = required_path("FACTORY_SUPERVISOR_WORKSPACE_ROOT")?;
    let state_root = required_path("FACTORY_SUPERVISOR_STATE_ROOT")?;
    let images = parse_images(
        &env::var("FACTORY_SUPERVISOR_IMAGES").map_err(|_| "FACTORY_SUPERVISOR_IMAGES required")?,
    )?;
    let project_roots = parse_project_roots(
        &env::var("FACTORY_SUPERVISOR_PROJECT_ROOTS")
            .map_err(|_| "FACTORY_SUPERVISOR_PROJECT_ROOTS required")?,
    )?;
    factory_native::telemetry::register_secret(&token);
    factory_native::telemetry::register_secret(&attempt_grant_key);
    factory_native::telemetry::register_secret(&capability_key);
    let max_runtime = match env::var("FACTORY_SUPERVISOR_MAX_RUNTIME_SECS") {
        Ok(value) => Duration::from_secs(value.parse::<u64>()?),
        Err(_) => MAX_EXECUTION_RUNTIME,
    };
    if max_runtime < Duration::from_secs(1) || max_runtime > MAX_EXECUTION_RUNTIME {
        return Err("FACTORY_SUPERVISOR_MAX_RUNTIME_SECS is outside the allowed bound".into());
    }
    let broker_url = env::var("FACTORY_RUNTIME_BROKER_URL").ok();
    let broker_token = factory_native::secrets::optional("FACTORY_RUNTIME_BROKER_TOKEN")?;
    let backend = if env::var("FACTORY_SUPERVISOR_TEST_BACKEND").ok().as_deref() == Some("local") {
        if !cfg!(debug_assertions) {
            return Err("the local Supervisor backend is not available in release builds".into());
        }
        #[cfg(debug_assertions)]
        {
            SupervisorBackend::Local(LocalVolumeBackend::new(required_path(
                "FACTORY_SUPERVISOR_TEST_VOLUME_ROOT",
            )?)?)
        }
        #[cfg(not(debug_assertions))]
        unreachable!("release builds reject the local backend above")
    } else if let (Some(url), Some(token)) = (broker_url.as_deref(), broker_token.as_deref()) {
        let backend = BrokerVolumes::new(url, token.to_owned())?;
        backend.preflight().await?;
        SupervisorBackend::Broker(backend)
    } else if broker_url.is_some() || broker_token.is_some() {
        return Err(
            "FACTORY_RUNTIME_BROKER_URL and FACTORY_RUNTIME_BROKER_TOKEN must be set together"
                .into(),
        );
    } else if container_mode {
        return Err(
            "containerized Supervisor requires FACTORY_RUNTIME_BROKER_URL and FACTORY_RUNTIME_BROKER_TOKEN"
                .into(),
        );
    } else {
        let backend = RootlessPodmanVolumes::new(images.iter().cloned())?;
        backend.preflight().await?;
        SupervisorBackend::Podman(backend)
    };
    let manager =
        WorkspaceManager::new(&workspace_root, &state_root, project_roots.clone(), backend)?;
    let removed = manager.reconcile().await?;
    if removed > 0 {
        tracing::warn!(removed, "reconciled stale workspace executions");
    }
    let bind = env::var("FACTORY_SUPERVISOR_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    let addr = factory_native::service_network::validate_bind(&bind, container_mode)?;
    let app = Arc::new(App {
        token,
        manager,
        max_runtime,
        allowed_images: images,
        project_roots,
        workspace_root,
        attempt_grant_key: attempt_grant_key.into_bytes(),
        capability_key: capability_key.into_bytes(),
        operation_nonces: Mutex::new(HashMap::new()),
        core_url,
    });
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "Factory sandbox supervisor listening");
    axum::serve(listener, build_router(app)).await?;
    Ok(())
}

fn build_router<B: factory_native::execution::VolumeBackend + 'static>(app: Arc<App<B>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/workspaces", post(allocate))
        .route("/v1/workspaces/{workspace_id}", get(status).delete(cleanup))
        .route("/v1/workspaces/{workspace_id}/execute", post(execute))
        .route("/v1/workspaces/{workspace_id}/apply", post(apply_files))
        .route("/v1/workspaces/{workspace_id}/write", post(write_files))
        .route("/v1/workspaces/{workspace_id}/run", post(run_tool))
        .route("/v1/workspaces/{workspace_id}/finalize", post(finalize))
        .route("/v1/integrate", post(integrate))
        .route("/v1/reconcile", post(reconcile))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(app)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "healthy": true,
        "isolation_certified": false,
        "mount": "named-volume",
        "host_workspace_mounted": false
    }))
}

async fn allocate<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Json(input): Json<WorkspaceAllocation>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(message) = verify_allocation_grant(&app, &input) {
        return (StatusCode::FORBIDDEN, Json(json!({"error": message}))).into_response();
    }
    match app.manager.allocate(&input).await {
        Ok(mut handle) => {
            match factory_native::attempt_grant::workspace_capability(
                &app.capability_key,
                &handle.workspace_id,
                &handle.attempt_id,
                handle.generation,
                handle.capability_expires_at_ms,
            ) {
                Ok(capability) => {
                    handle.capability = capability;
                    (StatusCode::CREATED, Json(handle)).into_response()
                }
                Err(message) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": message})),
                )
                    .into_response(),
            }
        }
        Err(error) => execution_error(error),
    }
}

async fn execute<B: factory_native::execution::VolumeBackend + 'static>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(input): Json<ExecuteWorkspace>,
) -> Response {
    let Some(handle) = authorized_workspace(&headers, &app, &workspace_id) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(digest) = factory_native::attempt_grant::request_digest(&json!({
        "image": &input.image,
        "argv": &input.argv,
        "timeout_secs": input.timeout_secs,
        "operation": "execute"
    })) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if operation_grant(&headers, &app, &handle, "execute", &digest).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Err(message) = validate_execute(&app, &input) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response();
    }
    let Some(grant) = headers
        .get("x-factory-operation-grant")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
    else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let fence = spawn_operation_fence(app.clone(), workspace_id.clone(), grant);
    let result = app
        .manager
        .execute(&workspace_id, &input, app.max_runtime)
        .await;
    fence.abort();
    match result {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => execution_error(error),
    }
}

async fn apply_files<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(input): Json<ApplyFilesRequest>,
) -> Response {
    let Some(handle) = authorized_workspace(&headers, &app, &workspace_id) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(digest) = factory_native::attempt_grant::request_digest(&json!({
        "files": &input.files,
        "operation": "apply"
    })) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(grant) = operation_grant(&headers, &app, &handle, "apply", &digest) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if input.allowed_paths != grant.allowed_paths {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut input = input;
    input.allowed_paths = grant.allowed_paths;
    match app.manager.apply_files(&workspace_id, &input).await {
        Ok(candidate) => (StatusCode::OK, Json(candidate)).into_response(),
        Err(error) => execution_error(error),
    }
}

async fn write_files<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(input): Json<WriteFilesRequest>,
) -> Response {
    let Some(handle) = authorized_workspace(&headers, &app, &workspace_id) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(digest) = factory_native::attempt_grant::request_digest(&json!({
        "files": &input.files,
        "reset": input.reset
    })) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(grant) = operation_grant(&headers, &app, &handle, "write", &digest) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if input.allowed_paths != grant.allowed_paths {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut input = input;
    input.allowed_paths = grant.allowed_paths;
    match app.manager.write_files(&workspace_id, &input).await {
        Ok(handle) => (StatusCode::OK, Json(handle)).into_response(),
        Err(error) => execution_error(error),
    }
}

async fn run_tool<B: factory_native::execution::VolumeBackend + 'static>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(input): Json<ToolRunRequest>,
) -> Response {
    let Some(handle) = authorized_workspace(&headers, &app, &workspace_id) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(digest) = factory_native::attempt_grant::request_digest(&json!({
        "argv": &input.argv,
        "timeout_secs": input.timeout_secs
    })) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(grant) = operation_grant(&headers, &app, &handle, "run", &digest) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if !grant.checks.iter().any(|check| check == &input.argv) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.allowed_images.contains(&input.image) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"image is not allowlisted"})),
        )
            .into_response();
    }
    if input.argv.is_empty() || input.argv.len() > MAX_ARGV_ITEMS {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid tool argv"})),
        )
            .into_response();
    }
    let mut total = 0usize;
    for argument in &input.argv {
        if argument.len() > MAX_ARG_BYTES || argument.as_bytes().contains(&0) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid tool argument"})),
            )
                .into_response();
        }
        total = total.saturating_add(argument.len());
    }
    if total > MAX_ARGV_BYTES
        || input.timeout_secs == 0
        || Duration::from_secs(input.timeout_secs) > app.max_runtime
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"tool request exceeds bounds"})),
        )
            .into_response();
    }
    let Some(grant) = headers
        .get("x-factory-operation-grant")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
    else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let fence = spawn_operation_fence(app.clone(), workspace_id.clone(), grant);
    let result = app.manager.run_tool(&workspace_id, &input).await;
    fence.abort();
    match result {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => execution_error(error),
    }
}

async fn finalize<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Response {
    let Some(handle) = authorized_workspace(&headers, &app, &workspace_id) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(digest) = factory_native::attempt_grant::request_digest(&json!({
        "operation": "finalize",
        "workspace_id": workspace_id
    })) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if operation_grant(&headers, &app, &handle, "finalize", &digest).is_none() {
        return StatusCode::FORBIDDEN.into_response();
    }
    match app.manager.finalize_workspace(&workspace_id).await {
        Ok(candidate) => (StatusCode::OK, Json(candidate)).into_response(),
        Err(error) => execution_error(error),
    }
}

async fn status<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Response {
    if authorized_workspace(&headers, &app, &workspace_id).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.manager.status(&workspace_id) {
        Ok(handle) => (StatusCode::OK, Json(handle)).into_response(),
        Err(error) => execution_error(error),
    }
}

async fn cleanup<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Response {
    if authorized_workspace(&headers, &app, &workspace_id).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.manager.cleanup(&workspace_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => execution_error(error),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct IntegrateRequest {
    repository: String,
    base_commit: String,
    candidate_commits: Vec<String>,
}

async fn integrate<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
    Json(input): Json<IntegrateRequest>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    tracing::info!(base = %input.base_commit, candidates = input.candidate_commits.len(), "integration request");
    if input.candidate_commits.len() < 2 || input.candidate_commits.len() > 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"integration requires 2..=8 candidates"})),
        )
            .into_response();
    }
    let repository = match std::path::Path::new(&input.repository).canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":error.to_string()})),
            )
                .into_response();
        }
    };
    if !app
        .project_roots
        .iter()
        .any(|root| repository.starts_with(root))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"repository is outside configured project roots"})),
        )
            .into_response();
    }
    let workspace_root = app.workspace_root.clone();
    let base = input.base_commit;
    let candidates = input.candidate_commits;
    let result = tokio::task::spawn_blocking(move || {
        factory_native::workspace::integrate_candidates(
            &repository,
            &base,
            &candidates,
            &workspace_root,
        )
    })
    .await;
    match result {
        Ok(Ok(integrated)) => (StatusCode::OK, Json(integrated)).into_response(),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}

async fn reconcile<B: factory_native::execution::VolumeBackend>(
    State(app): State<Arc<App<B>>>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.manager.reconcile().await {
        Ok(removed) => (StatusCode::OK, Json(json!({"removed": removed}))).into_response(),
        Err(error) => execution_error(error),
    }
}

fn verify_allocation_grant<B: factory_native::execution::VolumeBackend>(
    app: &App<B>,
    input: &WorkspaceAllocation,
) -> Result<(), String> {
    let claims = factory_native::attempt_grant::AttemptGrantClaims {
        attempt_id: input.attempt_id.clone(),
        worker_id: String::new(),
        generation: 0,
        repository: input.repository.clone(),
        base_commit: input.base_commit.clone(),
        action_spec_sha256: input.action_spec_sha256.clone(),
        expires_at_ms: 0,
    };
    // The grant is verified against its own signed claims; the allocation fields
    // must match the signed attempt, repository, base commit and spec digest.
    let grant = factory_native::attempt_grant::parse(&input.attempt_grant)?;
    if grant.attempt_id != claims.attempt_id
        || grant.generation != input.generation
        || grant.repository != claims.repository
        || !grant.base_commit.eq_ignore_ascii_case(&claims.base_commit)
        || grant.action_spec_sha256 != claims.action_spec_sha256
    {
        return Err("Attempt grant does not match the allocation".into());
    }
    factory_native::attempt_grant::verify(
        &app.attempt_grant_key,
        &input.attempt_grant,
        &grant,
        chrono::Utc::now().timestamp_millis(),
    )
}

fn authorized_workspace<B: factory_native::execution::VolumeBackend>(
    headers: &HeaderMap,
    app: &App<B>,
    workspace_id: &str,
) -> Option<factory_native::execution::WorkspaceHandle> {
    let token = bearer_token(headers)?;
    let handle = app.manager.status(workspace_id).ok()?;
    factory_native::attempt_grant::verify_workspace_capability(
        &app.capability_key,
        token,
        &handle.workspace_id,
        &handle.attempt_id,
        handle.generation,
        handle.capability_expires_at_ms,
        chrono::Utc::now().timestamp_millis(),
    )
    .ok()?;
    Some(handle)
}

fn operation_grant<B: factory_native::execution::VolumeBackend>(
    headers: &HeaderMap,
    app: &App<B>,
    handle: &factory_native::execution::WorkspaceHandle,
    operation: &str,
    request_sha256: &str,
) -> Option<factory_native::attempt_grant::OperationGrantClaims> {
    let token = headers
        .get("x-factory-operation-grant")
        .and_then(|value| value.to_str().ok())?;
    let claims = factory_native::attempt_grant::parse_operation(token).ok()?;
    if claims.attempt_id != handle.attempt_id
        || claims.generation != handle.generation
        || claims.workspace_id != handle.workspace_id
        || claims.operation != operation
        || claims.request_sha256 != request_sha256
    {
        return None;
    }
    factory_native::attempt_grant::verify_operation(
        &app.attempt_grant_key,
        token,
        &claims,
        chrono::Utc::now().timestamp_millis(),
    )
    .ok()?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut nonces = app.operation_nonces.lock().ok()?;
    nonces.retain(|_, expiry| *expiry > now);
    if nonces
        .insert(claims.nonce.clone(), claims.expires_at_ms)
        .is_some()
    {
        return None;
    }
    Some(claims)
}

fn spawn_operation_fence<B: factory_native::execution::VolumeBackend + 'static>(
    app: Arc<App<B>>,
    workspace_id: String,
    grant: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .no_proxy()
            .build()
            .expect("fence client");
        let url = app
            .core_url
            .join("supervisor/workspace_operation_valid")
            .ok();
        let Some(url) = url else {
            return;
        };
        let mut failures = 0u8;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let valid = match client
                .post(url.clone())
                .json(&json!({"grant": &grant}))
                .send()
                .await
            {
                Ok(response) => response.status().is_success(),
                Err(_) => false,
            };
            if valid {
                failures = 0;
            } else {
                failures = failures.saturating_add(1);
                if failures >= 2 {
                    let _ = app.manager.cancel_active(&workspace_id).await;
                    return;
                }
            }
        }
    })
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
}

fn validate_execute<B: factory_native::execution::VolumeBackend>(
    app: &App<B>,
    input: &ExecuteWorkspace,
) -> Result<(), String> {
    if !app.allowed_images.contains(&input.image) {
        return Err("image is not in the configured exact allowlist".into());
    }
    if input.argv.is_empty() || input.argv[0].is_empty() {
        return Err("argv must contain an executable".into());
    }
    if input.argv.len() > MAX_ARGV_ITEMS {
        return Err(format!("argv must contain at most {MAX_ARGV_ITEMS} items"));
    }
    let mut total_bytes = 0usize;
    for argument in &input.argv {
        if argument.len() > MAX_ARG_BYTES {
            return Err(format!(
                "each argv item must be at most {MAX_ARG_BYTES} bytes"
            ));
        }
        if argument.as_bytes().contains(&0) {
            return Err("argv must not contain NUL bytes".into());
        }
        total_bytes = total_bytes.saturating_add(argument.len());
    }
    if total_bytes > MAX_ARGV_BYTES {
        return Err(format!("argv must total at most {MAX_ARGV_BYTES} bytes"));
    }
    if input.timeout_secs == 0 || Duration::from_secs(input.timeout_secs) > app.max_runtime {
        return Err(format!(
            "timeout_secs must be between 1 and {}",
            app.max_runtime.as_secs()
        ));
    }
    Ok(())
}

fn execution_error(error: ExecutionError) -> Response {
    tracing::warn!(error = %error, "sandbox supervisor request failed");
    let status = match &error {
        ExecutionError::Invalid(_) => StatusCode::BAD_REQUEST,
        ExecutionError::NotFound => StatusCode::NOT_FOUND,
        ExecutionError::Conflict => StatusCode::CONFLICT,
        ExecutionError::Archive(ArchiveError::Invalid(_)) => StatusCode::BAD_REQUEST,
        ExecutionError::Archive(ArchiveError::TooLarge(_)) => StatusCode::PAYLOAD_TOO_LARGE,
        ExecutionError::Workspace(WorkspaceError::Invalid(_)) => StatusCode::BAD_REQUEST,
        ExecutionError::Volume(_) | ExecutionError::Sandbox(_) => StatusCode::SERVICE_UNAVAILABLE,
        ExecutionError::Io(_) | ExecutionError::Archive(_) | ExecutionError::Workspace(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (status, Json(json!({"error": error.to_string()}))).into_response()
}

fn required_path(variable: &str) -> Result<PathBuf, String> {
    let value = env::var(variable).map_err(|_| format!("{variable} required"))?;
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(format!("{variable} must be an absolute path"));
    }
    Ok(path)
}

fn parse_project_roots(raw: &str) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("FACTORY_SUPERVISOR_PROJECT_ROOTS must be a comma-separated list".into());
        }
        let path = PathBuf::from(part);
        if !path.is_absolute() || path == PathBuf::from("/") {
            return Err("project roots must be absolute and must not be /".into());
        }
        roots.push(path);
    }
    if roots.is_empty() {
        return Err("FACTORY_SUPERVISOR_PROJECT_ROOTS must not be empty".into());
    }
    Ok(roots)
}

fn validate_supervisor_token(token: &str) -> Result<(), String> {
    if token.len() < MIN_TOKEN_BYTES {
        return Err(format!(
            "FACTORY_SUPERVISOR_TOKEN must be at least {MIN_TOKEN_BYTES} bytes"
        ));
    }
    Ok(())
}

fn ensure_distinct_from_other_roles(token: &str) -> Result<(), String> {
    for variable in OTHER_SECRET_VARIABLES {
        if env::var(variable).is_ok_and(|other| other == token) {
            return Err(format!(
                "FACTORY_SUPERVISOR_TOKEN must differ from {variable}"
            ));
        }
    }
    Ok(())
}

fn parse_images(raw: &str) -> Result<BTreeSet<String>, String> {
    let mut images = BTreeSet::new();
    for part in raw.split(',') {
        let image = part.trim();
        if image.is_empty() {
            return Err(
                "FACTORY_SUPERVISOR_IMAGES must be a non-empty comma-separated allowlist".into(),
            );
        }
        images.insert(image.to_owned());
    }
    Ok(images)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use factory_native::execution::{BoundedOutput, VolumeBackend};
    use std::{collections::HashMap, fs, path::Path, process::Command, sync::Mutex};
    use tower::ServiceExt;
    use uuid::Uuid;

    const TEST_TOKEN: &str = "supervisor-token-0123456789abcdef0123456789abcdef";
    const TEST_GRANT_KEY: &[u8] = b"attempt-grant-key-0123456789abcdef";
    const TEST_CAPABILITY_KEY: &[u8] = b"capability-key-0123456789abcdef0";

    #[derive(Default)]
    struct TestBackend {
        volumes: Mutex<HashMap<String, PathBuf>>,
    }

    impl TestBackend {
        fn path(&self, volume: &str) -> PathBuf {
            self.volumes.lock().unwrap().get(volume).cloned().unwrap()
        }
    }

    impl VolumeBackend for TestBackend {
        async fn create_volume(
            &self,
            name: &str,
            _workspace_id: &str,
        ) -> Result<(), ExecutionError> {
            let path = std::env::temp_dir().join(format!("factory-test-volume-{}", Uuid::new_v4()));
            fs::create_dir(&path)?;
            self.volumes.lock().unwrap().insert(name.into(), path);
            Ok(())
        }
        async fn remove_volume(&self, name: &str) -> Result<(), ExecutionError> {
            if let Some(path) = self.volumes.lock().unwrap().remove(name) {
                let _ = fs::remove_dir_all(path);
            }
            Ok(())
        }
        async fn import_volume(&self, name: &str, bytes: &[u8]) -> Result<(), ExecutionError> {
            let path = self.path(name);
            if path.exists() {
                fs::remove_dir_all(&path)?;
            }
            factory_native::workspace_archive::extract(bytes, &path)?;
            Ok(())
        }
        async fn export_volume(&self, name: &str) -> Result<Vec<u8>, ExecutionError> {
            Ok(factory_native::workspace_archive::snapshot(&self.path(name))?.bytes)
        }
        async fn run(
            &self,
            volume: &str,
            _container: &str,
            _image: &str,
            argv: &[String],
            _timeout: Duration,
        ) -> Result<BoundedOutput, ExecutionError> {
            let path = self.path(volume);
            let output = Command::new(&argv[0])
                .args(&argv[1..])
                .current_dir(path)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .output()?;
            Ok(BoundedOutput {
                exit_code: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
                stdout_truncated: false,
                stderr_truncated: false,
            })
        }
        async fn remove_container(&self, _name: &str) -> Result<(), ExecutionError> {
            Ok(())
        }
    }

    struct TestFixture {
        app: Arc<App<TestBackend>>,
        root: PathBuf,
        repository: PathBuf,
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> TestFixture {
        let root = std::env::temp_dir().join(format!("factory-supervisor-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let repository = root.join("repo");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "-q"]);
        git(
            &repository,
            &["config", "user.email", "test@example.invalid"],
        );
        git(&repository, &["config", "user.name", "Test"]);
        fs::write(repository.join("seed.txt"), "base\n").unwrap();
        git(&repository, &["add", "seed.txt"]);
        git(&repository, &["commit", "-qm", "base"]);
        let images: BTreeSet<String> = ["test/image:1".to_owned()].into();
        let manager = WorkspaceManager::new(
            root.join("work"),
            root.join("state"),
            [repository.clone()],
            TestBackend::default(),
        )
        .unwrap();
        TestFixture {
            app: Arc::new(App {
                token: TEST_TOKEN.into(),
                manager,
                max_runtime: MAX_EXECUTION_RUNTIME,
                allowed_images: images,
                project_roots: vec![repository.clone()],
                workspace_root: root.join("integration-work"),
                attempt_grant_key: TEST_GRANT_KEY.to_vec(),
                capability_key: TEST_CAPABILITY_KEY.to_vec(),
                operation_nonces: Mutex::new(HashMap::new()),
                core_url: reqwest::Url::parse("http://127.0.0.1:1").unwrap(),
            }),
            root,
            repository,
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn base(repository: &Path) -> String {
        String::from_utf8(
            Command::new("git")
                .args(["-C", repository.to_str().unwrap(), "rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .into()
    }

    fn allocation_body(attempt_id: &str, repository: &Path) -> serde_json::Value {
        let base = base(repository);
        let spec_sha256 = "a".repeat(64);
        let claims = factory_native::attempt_grant::AttemptGrantClaims {
            attempt_id: attempt_id.into(),
            worker_id: "worker-test".into(),
            generation: 1,
            repository: repository.to_string_lossy().into_owned(),
            base_commit: base.clone(),
            action_spec_sha256: spec_sha256.clone(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
        };
        let grant = factory_native::attempt_grant::sign(TEST_GRANT_KEY, &claims).unwrap();
        json!({
            "attemptId": attempt_id,
            "repository": repository,
            "baseCommit": base,
            "generation": 1,
            "actionSpecSha256": spec_sha256,
            "attemptGrant": grant
        })
    }

    fn json_request(uri: &str, token: Option<&str>, body: &serde_json::Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    fn operation_grant(
        handle: &serde_json::Value,
        operation: &str,
        body: &serde_json::Value,
    ) -> String {
        let request_sha256 = match operation {
            "execute" => factory_native::attempt_grant::request_digest(&json!({
                "image": &body["image"],
                "argv": &body["argv"],
                "timeout_secs": body["timeoutSecs"],
                "operation": "execute"
            }))
            .unwrap(),
            "write" => factory_native::attempt_grant::request_digest(&json!({
                "files": &body["files"],
                "reset": true
            }))
            .unwrap(),
            "run" => factory_native::attempt_grant::request_digest(&json!({
                "argv": &body["argv"],
                "timeout_secs": body["timeoutSecs"]
            }))
            .unwrap(),
            _ => "b".repeat(64),
        };
        let claims = factory_native::attempt_grant::OperationGrantClaims {
            attempt_id: handle["attemptId"].as_str().unwrap().into(),
            worker_id: "worker-test".into(),
            generation: handle["generation"].as_i64().unwrap_or(1),
            workspace_id: handle["workspaceId"].as_str().unwrap().into(),
            operation: operation.into(),
            allowed_paths: vec!["src".into()],
            checks: vec![vec![
                "/bin/sh".into(),
                "-c".into(),
                "printf 'http\\n' > result.txt".into(),
            ]],
            request_sha256,
            nonce: uuid::Uuid::new_v4().to_string(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
        };
        factory_native::attempt_grant::sign_operation(TEST_GRANT_KEY, &claims).unwrap()
    }

    fn operation_request(
        uri: &str,
        capability: &str,
        grant: &str,
        body: &serde_json::Value,
    ) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {capability}"))
            .header("x-factory-operation-grant", grant)
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn status(router: &Router, request: Request<Body>) -> StatusCode {
        router.clone().oneshot(request).await.unwrap().status()
    }

    async fn body_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_is_public_and_does_not_claim_certification() {
        let fixture = fixture();
        let response = build_router(fixture.app.clone())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["healthy"], true);
        assert_eq!(value["isolation_certified"], false);
    }

    #[tokio::test]
    async fn workspace_routes_require_the_exact_bearer_token() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let body = json!({
            "attemptId": "attempt-1",
            "repository": fixture.repository,
            "baseCommit": base(&fixture.repository)
        });
        for token in [None, Some("wrong")] {
            assert_eq!(
                status(&router, json_request("/v1/workspaces", token, &body)).await,
                StatusCode::UNAUTHORIZED
            );
        }
        let wrong_scheme = Request::builder()
            .method("POST")
            .uri("/v1/workspaces")
            .header("content-type", "application/json")
            .header("authorization", format!("Token {TEST_TOKEN}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(
            status(&router, wrong_scheme).await,
            StatusCode::UNAUTHORIZED
        );
        let reconcile = Request::builder()
            .method("POST")
            .uri("/v1/reconcile")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(&router, reconcile).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn allocates_executes_and_cleans_a_workspace_over_http() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let allocate = allocation_body("attempt-1", &fixture.repository);
        let response = router
            .clone()
            .oneshot(json_request("/v1/workspaces", Some(TEST_TOKEN), &allocate))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let handle = body_json(response).await;
        assert_eq!(handle["status"], "ALLOCATED");
        let workspace_id = handle["workspaceId"].as_str().unwrap().to_owned();
        let capability = handle["capability"].as_str().unwrap().to_owned();

        let execution = json!({
            "image": "test/image:1",
            "argv": ["/bin/sh", "-c", "printf 'http\\n' > result.txt"],
            "timeoutSecs": 30
        });
        let response = router
            .clone()
            .oneshot(operation_request(
                &format!("/v1/workspaces/{workspace_id}/execute"),
                &capability,
                &operation_grant(&handle, "execute", &execution),
                &execution,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value = body_json(response).await;
        assert_eq!(value["exitCode"], 0);
        assert_eq!(value["confinement"]["mountKind"], "NAMED_VOLUME");
        assert_eq!(value["confinement"]["hostWorkspaceMounted"], false);
        let commit = value["candidate"]["commitSha"].as_str().unwrap();
        assert_eq!(
            String::from_utf8(
                Command::new("git")
                    .args([
                        "-C",
                        fixture.repository.to_str().unwrap(),
                        "show",
                        &format!("{commit}:result.txt")
                    ])
                    .output()
                    .unwrap()
                    .stdout
            )
            .unwrap(),
            "http\n"
        );

        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/workspaces/{workspace_id}"))
                    .header("authorization", format!("Bearer {capability}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn workspace_capability_is_bound_to_one_attempt_and_expiry() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let first = body_json(
            router
                .clone()
                .oneshot(json_request(
                    "/v1/workspaces",
                    Some(TEST_TOKEN),
                    &allocation_body("attempt-cap-a", &fixture.repository),
                ))
                .await
                .unwrap(),
        )
        .await;
        let second = body_json(
            router
                .clone()
                .oneshot(json_request(
                    "/v1/workspaces",
                    Some(TEST_TOKEN),
                    &allocation_body("attempt-cap-b", &fixture.repository),
                ))
                .await
                .unwrap(),
        )
        .await;
        let first_id = first["workspaceId"].as_str().unwrap();
        let second_id = second["workspaceId"].as_str().unwrap();
        let first_capability = first["capability"].as_str().unwrap();
        let second_capability = second["capability"].as_str().unwrap();
        assert_ne!(first_capability, second_capability);

        let status_request = |id: &str, token: &str| {
            Request::builder()
                .uri(format!("/v1/workspaces/{id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap()
        };
        assert_eq!(
            status(&router, status_request(first_id, first_capability)).await,
            StatusCode::OK
        );
        assert_eq!(
            status(&router, status_request(second_id, first_capability)).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&router, status_request(first_id, TEST_TOKEN)).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&router, status_request(first_id, "tampered")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn workspace_mutations_require_a_matching_unexpired_operation_grant() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let handle = body_json(
            router
                .clone()
                .oneshot(json_request(
                    "/v1/workspaces",
                    Some(TEST_TOKEN),
                    &allocation_body("attempt-op", &fixture.repository),
                ))
                .await
                .unwrap(),
        )
        .await;
        let workspace_id = handle["workspaceId"].as_str().unwrap();
        let capability = handle["capability"].as_str().unwrap();
        let mismatched_scope = json!({
            "allowedPaths": ["README.md"],
            "files": [{"path":"README.md","content":"x"}]
        });
        let write_digest = factory_native::attempt_grant::request_digest(&json!({
            "files": &mismatched_scope["files"],
            "reset": true
        }))
        .unwrap();
        let mut claims = factory_native::attempt_grant::OperationGrantClaims {
            attempt_id: "attempt-op".into(),
            worker_id: "worker-test".into(),
            generation: 1,
            workspace_id: workspace_id.into(),
            operation: "write".into(),
            allowed_paths: vec!["src".into()],
            checks: vec![vec!["/bin/true".into()]],
            request_sha256: write_digest,
            nonce: uuid::Uuid::new_v4().to_string(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
        };
        let grant = factory_native::attempt_grant::sign_operation(TEST_GRANT_KEY, &claims).unwrap();
        assert_eq!(
            status(
                &router,
                operation_request(
                    &format!("/v1/workspaces/{workspace_id}/write"),
                    capability,
                    &grant,
                    &mismatched_scope,
                ),
            )
            .await,
            StatusCode::FORBIDDEN
        );
        let run = json!({"image":"test/image:1","argv":["/bin/false"],"timeoutSecs":30});
        let run_grant = operation_grant(&handle, "run", &run);
        assert_eq!(
            status(
                &router,
                operation_request(
                    &format!("/v1/workspaces/{workspace_id}/run"),
                    capability,
                    &run_grant,
                    &run,
                ),
            )
            .await,
            StatusCode::FORBIDDEN
        );
        claims.expires_at_ms = 1;
        let expired =
            factory_native::attempt_grant::sign_operation(TEST_GRANT_KEY, &claims).unwrap();
        let write = json!({
            "allowedPaths": ["src"],
            "files": [{"path":"src/ok.txt","content":"ok"}]
        });
        assert_eq!(
            status(
                &router,
                operation_request(
                    &format!("/v1/workspaces/{workspace_id}/write"),
                    capability,
                    &expired,
                    &write,
                ),
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn allocation_requires_a_grant_for_that_exact_attempt() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let mut wrong = allocation_body("attempt-grant-a", &fixture.repository);
        let signed_for_other = allocation_body("attempt-grant-b", &fixture.repository);
        wrong["attemptGrant"] = signed_for_other["attemptGrant"].clone();
        assert_eq!(
            status(
                &router,
                json_request("/v1/workspaces", Some(TEST_TOKEN), &wrong)
            )
            .await,
            StatusCode::FORBIDDEN
        );
        let mut stale_generation = allocation_body("attempt-grant-a", &fixture.repository);
        stale_generation["generation"] = json!(2);
        assert_eq!(
            status(
                &router,
                json_request("/v1/workspaces", Some(TEST_TOKEN), &stale_generation)
            )
            .await,
            StatusCode::FORBIDDEN
        );
        let mut expired = allocation_body("attempt-grant-expired", &fixture.repository);
        let claims = factory_native::attempt_grant::AttemptGrantClaims {
            attempt_id: "attempt-grant-expired".into(),
            worker_id: "worker-test".into(),
            generation: 1,
            repository: fixture.repository.to_string_lossy().into_owned(),
            base_commit: base(&fixture.repository),
            action_spec_sha256: expired["actionSpecSha256"].as_str().unwrap().into(),
            expires_at_ms: 1,
        };
        expired["attemptGrant"] =
            json!(factory_native::attempt_grant::sign(TEST_GRANT_KEY, &claims).unwrap());
        assert_eq!(
            status(
                &router,
                json_request("/v1/workspaces", Some(TEST_TOKEN), &expired)
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn rejects_invalid_input_and_unknown_fields() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).unwrap();
        let outside_body = allocation_body("outside", &outside);
        assert_eq!(
            status(
                &router,
                json_request("/v1/workspaces", Some(TEST_TOKEN), &outside_body)
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        let allocate = allocation_body("attempt-2", &fixture.repository);
        let handle = body_json(
            router
                .clone()
                .oneshot(json_request("/v1/workspaces", Some(TEST_TOKEN), &allocate))
                .await
                .unwrap(),
        )
        .await;
        let workspace_id = handle["workspaceId"].as_str().unwrap();
        let capability = handle["capability"].as_str().unwrap();
        for execution in [
            json!({"image": "test/image:1", "argv": [], "timeoutSecs": 30}),
            json!({"image": "other/image:1", "argv": ["/bin/true"], "timeoutSecs": 30}),
            json!({"image": "test/image:1", "argv": ["/bin/true"], "timeoutSecs": 0}),
            json!({"image": "test/image:1", "argv": ["/bin/true"], "timeoutSecs": MAX_EXECUTION_RUNTIME.as_secs() + 1}),
            json!({"image": "test/image:1", "argv": ["/bin/true"], "timeoutSecs": 30, "network": "host"}),
        ] {
            assert!(
                status(
                    &router,
                    json_request(
                        &format!("/v1/workspaces/{workspace_id}/execute"),
                        Some(capability),
                        &execution
                    )
                )
                .await
                .is_client_error(),
                "unexpected status for {execution}"
            );
        }
    }
}
