//! Durable per-Attempt workspaces and sandbox volume execution.
//!
//! The trusted Supervisor owns the Git worktree. The sandbox never receives a
//! host pathname: the snapshot is imported into a Podman named volume, the
//! container mounts only that volume, and the exported volume is validated
//! before it can replace worktree contents. This removes the former same-UID
//! pathname-to-mount race. It is not a complete isolation or credential
//! certification.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time,
};
use uuid::Uuid;

use crate::{
    sandbox::SandboxError,
    workspace::{CandidateRevision, WorkspaceAllocator, WorkspaceError},
    workspace_archive::{self, ArchiveError},
};

pub const STATE_SCHEMA_VERSION: u32 = 1;
pub const MAX_EXECUTION_RUNTIME: Duration = Duration::from_secs(30 * 60);
const OUTPUT_LIMIT: usize = 1024 * 1024;
const MAX_REPOSITORY_ROOTS: usize = 32;
const STATE_PREFIX: &str = "workspace-";
const VOLUME_PREFIX: &str = "factory-ws-";
const CONTAINER_PREFIX: &str = "factory-exec-";

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("workspace state is invalid: {0}")]
    Invalid(String),
    #[error("workspace state I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace state archive: {0}")]
    Archive(#[from] ArchiveError),
    #[error("workspace Git operation failed: {0}")]
    Workspace(#[from] WorkspaceError),
    #[error("workspace volume operation failed: {0}")]
    Volume(String),
    #[error("sandbox operation failed: {0}")]
    Sandbox(#[from] SandboxError),
    #[error("workspace already has a different identity")]
    Conflict,
    #[error("workspace not found")]
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkspaceAllocation {
    pub attempt_id: String,
    pub repository: String,
    pub base_commit: String,
    #[serde(default)]
    pub generation: i64,
    #[serde(default)]
    pub action_spec_sha256: String,
    /// Core-signed Attempt grant. The Supervisor rejects allocation without a
    /// valid, unexpired grant.
    #[serde(default)]
    pub attempt_grant: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceHandle {
    pub workspace_id: String,
    pub attempt_id: String,
    pub base_commit: String,
    #[serde(default)]
    pub generation: i64,
    pub status: WorkspaceStatus,
    pub candidate: Option<CandidateRevision>,
    #[serde(default)]
    pub capability: String,
    #[serde(default)]
    pub capability_expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceStatus {
    Allocated,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub schema_version: u32,
    pub workspace_id: String,
    pub attempt_id: String,
    pub repository: String,
    pub base_commit: String,
    pub generation: i64,
    pub worktree: String,
    pub worktree_device: u64,
    pub worktree_inode: u64,
    pub volume: String,
    pub container: Option<String>,
    pub status: WorkspaceStatus,
    pub candidate: Option<CandidateRevision>,
    pub snapshot_sha256: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub capability_expires_at_ms: i64,
}

impl WorkspaceRecord {
    fn handle(&self) -> WorkspaceHandle {
        WorkspaceHandle {
            workspace_id: self.workspace_id.clone(),
            attempt_id: self.attempt_id.clone(),
            base_commit: self.base_commit.clone(),
            generation: self.generation,
            status: self.status.clone(),
            candidate: self.candidate.clone(),
            capability: String::new(),
            capability_expires_at_ms: self.capability_expires_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExecuteWorkspace {
    pub image: String,
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AppliedFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ApplyFilesRequest {
    pub allowed_paths: Vec<String>,
    pub files: Vec<AppliedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WriteFilesRequest {
    pub allowed_paths: Vec<String>,
    pub files: Vec<AppliedFile>,
    #[serde(default = "default_true")]
    pub reset: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolRunRequest {
    pub image: String,
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub candidate: Option<CandidateRevision>,
    pub snapshot_sha256: String,
    pub workspace: WorkspaceHandle,
    pub confinement: ConfinementEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfinementEvidence {
    pub mount_kind: String,
    pub host_workspace_mounted: bool,
    pub network: String,
    pub read_only_root: bool,
    pub capabilities_dropped: bool,
    pub no_new_privileges: bool,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

/// Narrow asynchronous volume/container port. Production uses rootless Podman.
pub trait VolumeBackend: Send + Sync {
    fn create_volume(
        &self,
        name: &str,
        workspace_id: &str,
    ) -> impl std::future::Future<Output = Result<(), ExecutionError>> + Send;
    fn remove_volume(
        &self,
        name: &str,
    ) -> impl std::future::Future<Output = Result<(), ExecutionError>> + Send;
    fn import_volume(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> impl std::future::Future<Output = Result<(), ExecutionError>> + Send;
    fn export_volume(
        &self,
        name: &str,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, ExecutionError>> + Send;
    fn run(
        &self,
        volume: &str,
        container: &str,
        image: &str,
        argv: &[String],
        timeout: Duration,
    ) -> impl std::future::Future<Output = Result<BoundedOutput, ExecutionError>> + Send;
    fn remove_container(
        &self,
        name: &str,
    ) -> impl std::future::Future<Output = Result<(), ExecutionError>> + Send;
}

#[derive(Debug, Clone)]
pub struct RootlessPodmanVolumes {
    executable: OsString,
    allowed_images: BTreeSet<String>,
    max_runtime: Duration,
}

impl RootlessPodmanVolumes {
    pub fn new(allowed_images: impl IntoIterator<Item = String>) -> Result<Self, ExecutionError> {
        let allowed_images: BTreeSet<String> = allowed_images.into_iter().collect();
        if allowed_images.is_empty()
            || allowed_images.iter().any(|image| {
                image.is_empty() || image.starts_with('-') || image.chars().any(char::is_whitespace)
            })
        {
            return Err(ExecutionError::Invalid(
                "an exact non-empty image allowlist is required".into(),
            ));
        }
        Ok(Self {
            executable: OsString::from("podman"),
            allowed_images,
            max_runtime: MAX_EXECUTION_RUNTIME,
        })
    }

    pub async fn preflight(&self) -> Result<(), ExecutionError> {
        let output = time::timeout(
            Duration::from_secs(10),
            Command::new(&self.executable)
                .arg("--remote=false")
                .args(["info", "--format", "json"])
                .env_remove("CONTAINER_HOST")
                .env_remove("CONTAINER_CONNECTION")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| ExecutionError::Volume("podman info timed out".into()))??;
        if !output.status.success() {
            return Err(ExecutionError::Volume(format!(
                "podman info failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let info: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| ExecutionError::Volume(format!("invalid podman info: {error}")))?;
        if info
            .pointer("/host/security/rootless")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            return Err(ExecutionError::Volume(
                "Podman did not confirm rootless mode".into(),
            ));
        }
        Ok(())
    }

    async fn podman<I, S>(&self, args: I) -> Result<std::process::Output, ExecutionError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = time::timeout(
            Duration::from_secs(30),
            Command::new(&self.executable)
                .args(args)
                .env_remove("CONTAINER_HOST")
                .env_remove("CONTAINER_CONNECTION")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| ExecutionError::Volume("Podman command timed out".into()))??;
        Ok(output)
    }
}

impl RootlessPodmanVolumes {
    fn container_args(
        &self,
        volume: &str,
        image: &str,
        argv: &[String],
        timeout: Duration,
        container: &str,
    ) -> Result<Vec<OsString>, ExecutionError> {
        validate_volume_name(volume)?;
        validate_container_name(container)?;
        if !self.allowed_images.contains(image) {
            return Err(ExecutionError::Invalid(format!(
                "image is not allowlisted: {image}"
            )));
        }
        if argv.is_empty() || argv[0].is_empty() {
            return Err(ExecutionError::Invalid(
                "execution argv must contain an executable".into(),
            ));
        }
        let seconds = timeout.as_secs().max(1).to_string();
        let mut args: Vec<OsString> = vec![
            "--remote=false".into(),
            "run".into(),
            "--rm".into(),
            format!("--name={container}").into(),
            format!("--label=io.software-factory.workspace={volume}").into(),
            "--pull=never".into(),
            "--network=none".into(),
            "--read-only".into(),
            "--tmpfs=/tmp:rw,noexec,nosuid,size=64m".into(),
            "--cap-drop=all".into(),
            "--security-opt=no-new-privileges".into(),
            "--userns=keep-id".into(),
            "--pids-limit=128".into(),
            "--memory=1g".into(),
            "--cpus=2".into(),
            format!("--timeout={seconds}").into(),
            "--workdir=/workspace".into(),
            "--env=HOME=/tmp".into(),
            "--env=TMPDIR=/tmp".into(),
            format!("--volume={volume}:/workspace:rw").into(),
            image.into(),
        ];
        args.extend(argv.iter().map(OsString::from));
        Ok(args)
    }
}

impl VolumeBackend for RootlessPodmanVolumes {
    async fn create_volume(&self, name: &str, workspace_id: &str) -> Result<(), ExecutionError> {
        validate_volume_name(name)?;
        validate_workspace_id(workspace_id)?;
        let _ = self.remove_volume(name).await;
        let output = self
            .podman([
                "--remote=false",
                "volume",
                "create",
                "--label",
                &format!("io.software-factory.workspace={workspace_id}"),
                name,
            ])
            .await?;
        if !output.status.success() {
            return Err(ExecutionError::Volume(format!(
                "volume create failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    async fn remove_volume(&self, name: &str) -> Result<(), ExecutionError> {
        validate_volume_name(name)?;
        let output = self
            .podman(["--remote=false", "volume", "rm", "--force", name])
            .await?;
        if output.status.success() {
            return Ok(());
        }
        let exists = self
            .podman(["--remote=false", "volume", "exists", name])
            .await?;
        if exists.status.code() == Some(1) {
            return Ok(());
        }
        Err(ExecutionError::Volume(format!(
            "volume removal failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    async fn import_volume(&self, name: &str, bytes: &[u8]) -> Result<(), ExecutionError> {
        validate_volume_name(name)?;
        let mut child = Command::new(&self.executable)
            .args(["--remote=false", "volume", "import", name, "-"])
            .env_remove("CONTAINER_HOST")
            .env_remove("CONTAINER_CONNECTION")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(ExecutionError::Io)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ExecutionError::Volume("volume import stdin unavailable".into()))?;
        let bytes = bytes.to_vec();
        let writer = tokio::spawn(async move {
            stdin.write_all(&bytes).await?;
            stdin.shutdown().await
        });
        let output = time::timeout(Duration::from_secs(60), child.wait_with_output())
            .await
            .map_err(|_| ExecutionError::Volume("volume import timed out".into()))??;
        writer
            .await
            .map_err(|error| ExecutionError::Volume(format!("volume import writer: {error}")))??;
        if !output.status.success() {
            return Err(ExecutionError::Volume(format!(
                "volume import failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    async fn export_volume(&self, name: &str) -> Result<Vec<u8>, ExecutionError> {
        validate_volume_name(name)?;
        let mut child = Command::new(&self.executable)
            .args(["--remote=false", "volume", "export", name])
            .env_remove("CONTAINER_HOST")
            .env_remove("CONTAINER_CONNECTION")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(ExecutionError::Io)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ExecutionError::Volume("volume export stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ExecutionError::Volume("volume export stderr unavailable".into()))?;
        let stdout_task = tokio::spawn(read_bounded(stdout, 64 * 1024 * 1024));
        let status = time::timeout(Duration::from_secs(60), child.wait())
            .await
            .map_err(|_| ExecutionError::Volume("volume export timed out".into()))??;
        let (bytes, truncated) = stdout_task
            .await
            .map_err(|error| ExecutionError::Volume(format!("volume export reader: {error}")))??;
        let stderr = read_bounded(stderr, OUTPUT_LIMIT).await?.0;
        if !status.success() {
            return Err(ExecutionError::Volume(format!(
                "volume export failed: {}",
                String::from_utf8_lossy(&stderr).trim()
            )));
        }
        if truncated {
            return Err(ExecutionError::Volume(
                "volume export exceeded the archive bound".into(),
            ));
        }
        Ok(bytes)
    }

    async fn run(
        &self,
        volume: &str,
        container: &str,
        image: &str,
        argv: &[String],
        timeout: Duration,
    ) -> Result<BoundedOutput, ExecutionError> {
        validate_volume_name(volume)?;
        validate_container_name(container)?;
        if !self.allowed_images.contains(image) {
            return Err(ExecutionError::Invalid(format!(
                "image is not allowlisted: {image}"
            )));
        }
        if argv.is_empty() || argv[0].is_empty() {
            return Err(ExecutionError::Invalid(
                "execution argv must contain an executable".into(),
            ));
        }
        if timeout < Duration::from_secs(1) || timeout > self.max_runtime {
            return Err(ExecutionError::Invalid(
                "execution timeout is outside the configured bound".into(),
            ));
        }
        let args = self.container_args(volume, image, argv, timeout, container)?;
        let mut child = Command::new(&self.executable)
            .args(&args)
            .env_remove("CONTAINER_HOST")
            .env_remove("CONTAINER_CONNECTION")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(ExecutionError::Io)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ExecutionError::Volume("container stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ExecutionError::Volume("container stderr unavailable".into()))?;
        let stdout_task = tokio::spawn(read_bounded(stdout, OUTPUT_LIMIT));
        let stderr_task = tokio::spawn(read_bounded(stderr, OUTPUT_LIMIT));
        let lifetime = timeout + Duration::from_secs(15);
        let status = match time::timeout(lifetime, child.wait()).await {
            Ok(result) => result?,
            Err(_) => {
                let _ = self.remove_container(container).await;
                stdout_task.abort();
                stderr_task.abort();
                return Err(ExecutionError::Volume("sandbox execution timed out".into()));
            }
        };
        let _ = self.remove_container(container).await;
        let (stdout, stdout_truncated) = stdout_task
            .await
            .map_err(|error| ExecutionError::Volume(format!("stdout reader: {error}")))??;
        let (stderr, stderr_truncated) = stderr_task
            .await
            .map_err(|error| ExecutionError::Volume(format!("stderr reader: {error}")))??;
        Ok(BoundedOutput {
            exit_code: status.code(),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        })
    }

    async fn remove_container(&self, name: &str) -> Result<(), ExecutionError> {
        validate_container_name(name)?;
        let output = self
            .podman(["--remote=false", "rm", "--force", "--time=0", name])
            .await?;
        if output.status.success() {
            return Ok(());
        }
        let exists = self
            .podman(["--remote=false", "container", "exists", name])
            .await?;
        if exists.status.code() == Some(1) {
            return Ok(());
        }
        Err(ExecutionError::Volume(format!(
            "container removal failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

pub struct WorkspaceManager<B: VolumeBackend> {
    workspace_root: PathBuf,
    state_root: PathBuf,
    project_roots: Vec<PathBuf>,
    backend: B,
    lock: Arc<tokio::sync::Mutex<()>>,
    active: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    cancelled: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl<B: VolumeBackend> WorkspaceManager<B> {
    pub fn new(
        workspace_root: impl AsRef<Path>,
        state_root: impl AsRef<Path>,
        project_roots: impl IntoIterator<Item = PathBuf>,
        backend: B,
    ) -> Result<Self, ExecutionError> {
        let workspace_root = prepare_private_directory(workspace_root.as_ref())?;
        let state_root = prepare_private_directory(state_root.as_ref())?;
        let project_roots: Vec<PathBuf> = project_roots
            .into_iter()
            .map(|root| canonical_repository_root(&root))
            .collect::<Result<_, _>>()?;
        if project_roots.is_empty() {
            return Err(ExecutionError::Invalid(
                "at least one explicit project root is required".into(),
            ));
        }
        if project_roots.len() > MAX_REPOSITORY_ROOTS {
            return Err(ExecutionError::Invalid(
                "too many project roots are configured".into(),
            ));
        }
        Ok(Self {
            workspace_root,
            state_root,
            project_roots,
            backend,
            lock: Arc::new(tokio::sync::Mutex::new(())),
            active: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            cancelled: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        })
    }

    pub async fn allocate(
        &self,
        input: &WorkspaceAllocation,
    ) -> Result<WorkspaceHandle, ExecutionError> {
        validate_attempt_id(&input.attempt_id)?;
        let _write = self.lock.lock().await;
        if let Some(mut record) = self.load_record(&input.attempt_id)? {
            let authorized = self.authorize_repository(&record.repository)?;
            if record.generation != input.generation
                || !record.base_commit.eq_ignore_ascii_case(&input.base_commit)
                || !authorized
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&input.repository)
            {
                return Err(ExecutionError::Conflict);
            }
            if record.status == WorkspaceStatus::Completed {
                let candidate = record.candidate.clone().ok_or_else(|| {
                    ExecutionError::Invalid("completed workspace has no candidate".into())
                })?;
                crate::workspace::verify_candidate_commit(
                    &authorized,
                    &record.base_commit,
                    &candidate.commit_sha,
                    &candidate.tree_sha,
                )?;
                record.capability_expires_at_ms = (now_ms() as i64)
                    .saturating_add(MAX_EXECUTION_RUNTIME.as_millis() as i64 + 60_000);
                self.write_record(&record)?;
                return Ok(record.handle());
            }
            if let Ok(workspace) = self.open_existing(&record) {
                workspace.reset_to_base()?;
                record.status = WorkspaceStatus::Allocated;
                record.candidate = None;
                record.snapshot_sha256 = None;
                record.container = None;
                record.error = None;
                record.updated_at_ms = now_ms();
                record.capability_expires_at_ms = (now_ms() as i64)
                    .saturating_add(MAX_EXECUTION_RUNTIME.as_millis() as i64 + 60_000);
                self.write_record(&record)?;
                self.backend
                    .create_volume(&record.volume, &record.workspace_id)
                    .await?;
                return Ok(record.handle());
            }
            let _ = self.backend.remove_volume(&record.volume).await;
            self.remove_record(&record.attempt_id)?;
        }
        let repository = self.authorize_repository(&input.repository)?;
        let allocator = WorkspaceAllocator::new(&repository, &self.workspace_root)?;
        let mut workspace = allocator.allocate(&input.attempt_id, &input.base_commit)?;
        let identity = workspace.identity()?;
        let workspace_id = Uuid::new_v4().to_string();
        let volume = format!("{VOLUME_PREFIX}{workspace_id}");
        let record = WorkspaceRecord {
            schema_version: STATE_SCHEMA_VERSION,
            workspace_id: workspace_id.clone(),
            attempt_id: input.attempt_id.clone(),
            repository: repository.to_string_lossy().into_owned(),
            base_commit: input.base_commit.to_lowercase(),
            generation: input.generation,
            worktree: workspace.path().to_string_lossy().into_owned(),
            worktree_device: identity.0,
            worktree_inode: identity.1,
            volume: volume.clone(),
            container: None,
            status: WorkspaceStatus::Allocated,
            candidate: None,
            snapshot_sha256: None,
            created_at_ms: now_ms(),
            updated_at_ms: now_ms(),
            error: None,
            capability_expires_at_ms: (now_ms() as i64)
                .saturating_add(MAX_EXECUTION_RUNTIME.as_millis() as i64 + 60_000),
        };
        if let Err(error) = self.write_record(&record) {
            let _ = workspace.cleanup();
            return Err(error);
        }
        if let Err(error) = self.backend.create_volume(&volume, &workspace_id).await {
            let _ = workspace.cleanup();
            let _ = self.remove_record(&input.attempt_id);
            return Err(error);
        }
        Ok(record.handle())
    }

    pub async fn write_files(
        &self,
        workspace_id: &str,
        input: &WriteFilesRequest,
    ) -> Result<WorkspaceHandle, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        validate_files(&input.allowed_paths, &input.files)?;
        let _write = self.lock.lock().await;
        let mut record = self.find_by_id(workspace_id)?;
        if record.status != WorkspaceStatus::Allocated {
            return Err(ExecutionError::Conflict);
        }
        let workspace = self.open_existing(&record)?;
        if input.reset {
            workspace.reset_to_base()?;
        }
        write_files_to_workspace(&workspace, &input.files)?;
        workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
        record.updated_at_ms = now_ms();
        self.write_record(&record)?;
        Ok(record.handle())
    }

    pub async fn run_tool(
        &self,
        workspace_id: &str,
        input: &ToolRunRequest,
    ) -> Result<ExecutionResult, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        if input.timeout_secs == 0
            || Duration::from_secs(input.timeout_secs) > MAX_EXECUTION_RUNTIME
        {
            return Err(ExecutionError::Invalid(
                "tool timeout is outside the configured bound".into(),
            ));
        }
        let _write = self.lock.lock().await;
        if self.cancelled.lock().unwrap().remove(workspace_id) {
            return Err(ExecutionError::Conflict);
        }
        let mut record = self.find_by_id(workspace_id)?;
        if record.status != WorkspaceStatus::Allocated {
            return Err(ExecutionError::Conflict);
        }
        let workspace = self.open_existing(&record)?;
        workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
        let snapshot = tokio::task::spawn_blocking({
            let path = workspace.path().to_owned();
            move || workspace_archive::snapshot(&path)
        })
        .await
        .map_err(|error| ExecutionError::Invalid(format!("snapshot task: {error}")))??;
        self.backend
            .import_volume(&record.volume, &snapshot.bytes)
            .await?;
        let container = format!("{CONTAINER_PREFIX}{}", Uuid::new_v4());
        record.status = WorkspaceStatus::Running;
        record.container = Some(container.clone());
        record.updated_at_ms = now_ms();
        self.write_record(&record)?;
        self.active
            .lock()
            .unwrap()
            .insert(workspace_id.to_owned(), container.clone());
        let result = self
            .backend
            .run(
                &record.volume,
                &container,
                &input.image,
                &input.argv,
                Duration::from_secs(input.timeout_secs),
            )
            .await;
        self.active.lock().unwrap().remove(workspace_id);
        let exported = self.backend.export_volume(&record.volume).await;
        let _ = self.backend.remove_container(&container).await;
        let _ = self.backend.remove_volume(&record.volume).await;
        record.container = None;
        record.updated_at_ms = now_ms();
        let (result, exported) = match (result, exported) {
            (Ok(result), Ok(exported)) => (result, exported),
            (Err(error), _) | (_, Err(error)) => {
                record.status = WorkspaceStatus::Failed;
                record.error = Some(error.to_string());
                let _ = self.write_record(&record);
                return Err(error);
            }
        };
        let staging = self
            .workspace_root
            .join(format!("stage-{}", Uuid::new_v4()));
        tokio::task::spawn_blocking({
            let bytes = exported;
            let staging = staging.clone();
            move || workspace_archive::extract(&bytes, &staging)
        })
        .await
        .map_err(|error| ExecutionError::Invalid(format!("extract task: {error}")))??;
        workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
        workspace.replace_contents(&staging)?;
        workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
        record.status = WorkspaceStatus::Allocated;
        record.error = if result.exit_code == Some(0) {
            None
        } else {
            Some(format!("tool exited with {:?}", result.exit_code))
        };
        self.write_record(&record)?;
        Ok(ExecutionResult {
            exit_code: result.exit_code,
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
            candidate: None,
            snapshot_sha256: snapshot.sha256,
            workspace: record.handle(),
            confinement: confinement(&input.image),
        })
    }

    pub async fn finalize_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<CandidateRevision, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        let _write = self.lock.lock().await;
        if self.cancelled.lock().unwrap().remove(workspace_id) {
            return Err(ExecutionError::Conflict);
        }
        let mut record = self.find_by_id(workspace_id)?;
        if record.status == WorkspaceStatus::Completed {
            return record.candidate.ok_or_else(|| {
                ExecutionError::Invalid("completed workspace has no candidate".into())
            });
        }
        if record.status != WorkspaceStatus::Allocated {
            return Err(ExecutionError::Conflict);
        }
        let workspace = self.open_existing(&record)?;
        workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
        let candidate =
            workspace.commit_all(&format!("factory provider attempt {}", record.attempt_id))?;
        record.status = WorkspaceStatus::Completed;
        record.candidate = Some(candidate.clone());
        record.error = None;
        record.updated_at_ms = now_ms();
        self.write_record(&record)?;
        let _ = self.backend.remove_volume(&record.volume).await;
        Ok(candidate)
    }

    pub async fn apply_files(
        &self,
        workspace_id: &str,
        input: &ApplyFilesRequest,
    ) -> Result<CandidateRevision, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        validate_files(&input.allowed_paths, &input.files)?;
        let _write = self.lock.lock().await;
        if self.cancelled.lock().unwrap().remove(workspace_id) {
            return Err(ExecutionError::Conflict);
        }
        let mut record = self.find_by_id(workspace_id)?;
        if record.status == WorkspaceStatus::Completed {
            return record.candidate.ok_or_else(|| {
                ExecutionError::Invalid("completed workspace has no candidate".into())
            });
        }
        if record.status == WorkspaceStatus::Failed {
            return Err(ExecutionError::Conflict);
        }
        let workspace = self.open_existing(&record)?;
        workspace.reset_to_base()?;
        let result = (|| -> Result<CandidateRevision, ExecutionError> {
            write_files_to_workspace(&workspace, &input.files)?;
            workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
            Ok(workspace.commit_all(&format!("factory provider attempt {}", record.attempt_id))?)
        })();
        record.updated_at_ms = now_ms();
        match result {
            Ok(candidate) => {
                record.status = WorkspaceStatus::Completed;
                record.candidate = Some(candidate.clone());
                record.error = None;
                self.write_record(&record)?;
                let _ = self.backend.remove_volume(&record.volume).await;
                Ok(candidate)
            }
            Err(error) => {
                record.status = WorkspaceStatus::Failed;
                record.error = Some(error.to_string());
                let _ = self.write_record(&record);
                Err(error)
            }
        }
    }

    pub async fn execute(
        &self,
        workspace_id: &str,
        input: &ExecuteWorkspace,
        max_runtime: Duration,
    ) -> Result<ExecutionResult, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        if input.timeout_secs == 0 || Duration::from_secs(input.timeout_secs) > max_runtime {
            return Err(ExecutionError::Invalid(
                "execution timeout is outside the configured bound".into(),
            ));
        }
        let _write = self.lock.lock().await;
        if self.cancelled.lock().unwrap().remove(workspace_id) {
            return Err(ExecutionError::Conflict);
        }
        let mut record = self.find_by_id(workspace_id)?;
        if record.status == WorkspaceStatus::Completed {
            let candidate = record.candidate.clone().ok_or_else(|| {
                ExecutionError::Invalid("completed workspace has no candidate".into())
            })?;
            return Ok(ExecutionResult {
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                candidate: Some(candidate),
                snapshot_sha256: record.snapshot_sha256.clone().unwrap_or_default(),
                workspace: record.handle(),
                confinement: confinement(&input.image),
            });
        }
        let mut workspace = self.open_existing(&record)?;
        workspace.reset_to_base()?;
        let snapshot = tokio::task::spawn_blocking({
            let path = workspace.path().to_owned();
            move || workspace_archive::snapshot(&path)
        })
        .await
        .map_err(|error| ExecutionError::Invalid(format!("snapshot task: {error}")))??;
        if let Err(error) = self
            .backend
            .import_volume(&record.volume, &snapshot.bytes)
            .await
        {
            record.status = WorkspaceStatus::Failed;
            record.error = Some(error.to_string());
            record.updated_at_ms = now_ms();
            let _ = self.write_record(&record);
            let _ = self.backend.remove_volume(&record.volume).await;
            return Err(error);
        }
        let container = format!("{CONTAINER_PREFIX}{}", Uuid::new_v4());
        record.status = WorkspaceStatus::Running;
        record.container = Some(container.clone());
        record.snapshot_sha256 = Some(snapshot.sha256.clone());
        record.updated_at_ms = now_ms();
        self.write_record(&record)?;
        self.active
            .lock()
            .unwrap()
            .insert(workspace_id.to_owned(), container.clone());
        let outcome = async {
            let result = self
                .backend
                .run(
                    &record.volume,
                    &container,
                    &input.image,
                    &input.argv,
                    Duration::from_secs(input.timeout_secs),
                )
                .await?;
            let exported = self.backend.export_volume(&record.volume).await?;
            let staging = self
                .workspace_root
                .join(format!("stage-{}", Uuid::new_v4()));
            tokio::task::spawn_blocking({
                let bytes = exported;
                let staging = staging.clone();
                move || workspace_archive::extract(&bytes, &staging)
            })
            .await
            .map_err(|error| ExecutionError::Invalid(format!("extract task: {error}")))??;
            if result.exit_code != Some(0) {
                let _ = fs::remove_dir_all(&staging);
                return Ok::<_, ExecutionError>((result, None));
            }
            workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
            workspace.replace_contents(&staging)?;
            workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
            let message = format!("factory attempt {}", record.attempt_id);
            let candidate = workspace.commit_all(&message)?;
            Ok((result, Some(candidate)))
        }
        .await;
        self.active.lock().unwrap().remove(workspace_id);
        record.container = None;
        record.updated_at_ms = now_ms();
        let outcome = match outcome {
            Ok((result, candidate)) => {
                if let Some(candidate) = &candidate {
                    record.status = WorkspaceStatus::Completed;
                    record.candidate = Some(candidate.clone());
                    record.error = None;
                } else {
                    record.status = WorkspaceStatus::Failed;
                    record.error = Some(format!("sandbox exited with {:?}", result.exit_code));
                }
                self.write_record(&record)?;
                self.backend.remove_volume(&record.volume).await?;
                let _ = workspace.cleanup();
                (result, candidate)
            }
            Err(error) => {
                record.status = WorkspaceStatus::Failed;
                record.error = Some(error.to_string());
                let _ = self.write_record(&record);
                let _ = self.backend.remove_container(&container).await;
                let _ = self.backend.remove_volume(&record.volume).await;
                if workspace
                    .verify_identity((record.worktree_device, record.worktree_inode))
                    .is_ok()
                {
                    let _ = workspace.cleanup();
                }
                return Err(error);
            }
        };
        Ok(ExecutionResult {
            exit_code: outcome.0.exit_code,
            stdout: String::from_utf8_lossy(&outcome.0.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&outcome.0.stderr).into_owned(),
            stdout_truncated: outcome.0.stdout_truncated,
            stderr_truncated: outcome.0.stderr_truncated,
            candidate: outcome.1,
            snapshot_sha256: snapshot.sha256,
            workspace: record.handle(),
            confinement: confinement(&input.image),
        })
    }

    pub async fn cancel_active(&self, workspace_id: &str) -> Result<bool, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        self.cancelled
            .lock()
            .unwrap()
            .insert(workspace_id.to_owned());
        let container = self.active.lock().unwrap().get(workspace_id).cloned();
        if let Some(container) = container {
            self.backend.remove_container(&container).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn cleanup(&self, workspace_id: &str) -> Result<(), ExecutionError> {
        validate_workspace_id(workspace_id)?;
        self.cancelled.lock().unwrap().remove(workspace_id);
        let _write = self.lock.lock().await;
        let record = self.find_by_id(workspace_id)?;
        if let Some(container) = &record.container {
            self.backend.remove_container(container).await?;
        }
        self.backend.remove_volume(&record.volume).await?;
        if let Ok(mut workspace) = self.open_existing(&record) {
            let _ = workspace.cleanup();
        }
        self.remove_record(&record.attempt_id)?;
        Ok(())
    }

    pub async fn reconcile(&self) -> Result<usize, ExecutionError> {
        let _write = self.lock.lock().await;
        let mut removed = 0usize;
        for record in self.records()? {
            let mut record = record;
            let mut changed = false;
            if record.status == WorkspaceStatus::Running {
                if let Some(container) = &record.container {
                    let _ = self.backend.remove_container(container).await;
                }
                let _ = self.backend.remove_volume(&record.volume).await;
                record.status = WorkspaceStatus::Failed;
                record.error = Some("Supervisor restarted during execution".into());
                record.container = None;
                record.updated_at_ms = now_ms();
                changed = true;
                removed += 1;
            } else if matches!(record.status, WorkspaceStatus::Completed) {
                let _ = self.backend.remove_volume(&record.volume).await;
            } else if matches!(record.status, WorkspaceStatus::Failed) {
                let _ = self.backend.remove_volume(&record.volume).await;
                if let Ok(mut workspace) = self.open_existing(&record) {
                    let _ = workspace.cleanup();
                }
                let _ = self.remove_record(&record.attempt_id);
                removed += 1;
                continue;
            } else if record.status == WorkspaceStatus::Allocated {
                self.backend
                    .create_volume(&record.volume, &record.workspace_id)
                    .await?;
            }
            if changed {
                self.write_record(&record)?;
            }
        }
        Ok(removed)
    }

    pub fn status(&self, workspace_id: &str) -> Result<WorkspaceHandle, ExecutionError> {
        validate_workspace_id(workspace_id)?;
        self.find_by_id(workspace_id).map(|record| record.handle())
    }

    fn authorize_repository(&self, value: &str) -> Result<PathBuf, ExecutionError> {
        let path = Path::new(value);
        if !path.is_absolute() {
            return Err(ExecutionError::Invalid(
                "repository must be an absolute path".into(),
            ));
        }
        let canonical = path.canonicalize()?;
        if !self
            .project_roots
            .iter()
            .any(|root| canonical.starts_with(root))
        {
            return Err(ExecutionError::Invalid(
                "repository is outside the configured project roots".into(),
            ));
        }
        Ok(canonical)
    }

    fn open_existing(
        &self,
        record: &WorkspaceRecord,
    ) -> Result<crate::workspace::AttemptWorkspace, ExecutionError> {
        let path = Path::new(&record.worktree);
        if path.parent() != Some(self.workspace_root.as_path()) {
            return Err(ExecutionError::Invalid(
                "stored worktree is outside the workspace root".into(),
            ));
        }
        let workspace = crate::workspace::AttemptWorkspace::open(
            Path::new(&record.repository),
            path.to_path_buf(),
            record.attempt_id.clone(),
            record.base_commit.clone(),
        )?;
        workspace.verify_identity((record.worktree_device, record.worktree_inode))?;
        Ok(workspace)
    }

    fn state_path(&self, attempt_id: &str) -> Result<PathBuf, ExecutionError> {
        validate_attempt_id(attempt_id)?;
        Ok(self
            .state_root
            .join(format!("{STATE_PREFIX}{attempt_id}.json")))
    }

    fn load_record(&self, attempt_id: &str) -> Result<Option<WorkspaceRecord>, ExecutionError> {
        let path = self.state_path(attempt_id)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let record: WorkspaceRecord = serde_json::from_slice(&bytes)
                    .map_err(|error| ExecutionError::Invalid(format!("state JSON: {error}")))?;
                validate_record(&record)?;
                Ok(Some(record))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn find_by_id(&self, workspace_id: &str) -> Result<WorkspaceRecord, ExecutionError> {
        for record in self.records()? {
            if record.workspace_id == workspace_id {
                return Ok(record);
            }
        }
        Err(ExecutionError::NotFound)
    }

    fn records(&self) -> Result<Vec<WorkspaceRecord>, ExecutionError> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.state_root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err(ExecutionError::Invalid("non-UTF-8 state filename".into()));
            };
            if name.starts_with('.') || !name.starts_with(STATE_PREFIX) {
                continue;
            }
            let bytes = fs::read(entry.path())?;
            let record: WorkspaceRecord = serde_json::from_slice(&bytes)
                .map_err(|error| ExecutionError::Invalid(format!("state JSON: {error}")))?;
            validate_record(&record)?;
            records.push(record);
        }
        Ok(records)
    }

    fn write_record(&self, record: &WorkspaceRecord) -> Result<(), ExecutionError> {
        validate_record(record)?;
        let path = self.state_path(&record.attempt_id)?;
        let pending = self.state_root.join(format!(".pending-{}", Uuid::new_v4()));
        let bytes = serde_json::to_vec(record)
            .map_err(|error| ExecutionError::Invalid(format!("state encoding: {error}")))?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&pending)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&pending, &path)?;
        let directory = fs::File::open(&self.state_root)?;
        directory.sync_all()?;
        Ok(())
    }

    fn remove_record(&self, attempt_id: &str) -> Result<(), ExecutionError> {
        let path = self.state_path(attempt_id)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn validate_files(allowed_paths: &[String], files: &[AppliedFile]) -> Result<(), ExecutionError> {
    if allowed_paths.is_empty() {
        return Err(ExecutionError::Invalid(
            "provider files require an explicit allowed-path scope".into(),
        ));
    }
    if files.is_empty() || files.len() > 64 {
        return Err(ExecutionError::Invalid(
            "provider files require 1..=64 entries".into(),
        ));
    }
    let mut total = 0usize;
    for file in files {
        if !crate::workspace::is_safe_repository_path(&file.path)
            || !crate::workspace::path_covered(&file.path, allowed_paths)
        {
            return Err(ExecutionError::Invalid(format!(
                "provider file path {:?} is outside the allowed scope",
                file.path
            )));
        }
        if file.content.len() > 256 * 1024 {
            return Err(ExecutionError::Invalid(
                "provider file exceeds the per-file bound".into(),
            ));
        }
        total = total.saturating_add(file.content.len());
    }
    if total > 1024 * 1024 {
        return Err(ExecutionError::Invalid(
            "provider files exceed the total bound".into(),
        ));
    }
    Ok(())
}

fn write_files_to_workspace(
    workspace: &crate::workspace::AttemptWorkspace,
    files: &[AppliedFile],
) -> Result<(), ExecutionError> {
    for file in files {
        let target = workspace.path().join(&file.path);
        let parent = target
            .parent()
            .ok_or_else(|| ExecutionError::Invalid("file path has no parent".into()))?;
        ensure_no_symlink_components(workspace.path(), parent)?;
        std::fs::create_dir_all(parent)?;
        if let Ok(metadata) = std::fs::symlink_metadata(&target)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(ExecutionError::Invalid(
                "provider target is not a regular file".into(),
            ));
        }
        std::fs::write(&target, &file.content)?;
    }
    Ok(())
}

fn ensure_no_symlink_components(root: &Path, path: &Path) -> Result<(), ExecutionError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ExecutionError::Invalid("path escapes the workspace".into()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ExecutionError::Invalid(
                    "path contains a symlink component".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(ExecutionError::Io(error)),
        }
    }
    Ok(())
}

fn confinement(image: &str) -> ConfinementEvidence {
    ConfinementEvidence {
        mount_kind: "NAMED_VOLUME".into(),
        host_workspace_mounted: false,
        network: "NONE".into(),
        read_only_root: true,
        capabilities_dropped: true,
        no_new_privileges: true,
        image: image.into(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn prepare_private_directory(path: &Path) -> Result<PathBuf, ExecutionError> {
    if !path.is_absolute() {
        return Err(ExecutionError::Invalid(
            "Supervisor directories must be absolute".into(),
        ));
    }
    fs::create_dir_all(path)?;
    let canonical = path.canonicalize()?;
    let metadata = fs::symlink_metadata(&canonical)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ExecutionError::Invalid(
            "Supervisor directory must be a real directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != effective_uid() {
            return Err(ExecutionError::Invalid(
                "Supervisor directory must be owned by this user".into(),
            ));
        }
        if metadata.mode() & 0o077 != 0 {
            fs::set_permissions(&canonical, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(canonical)
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

fn canonical_repository_root(path: &Path) -> Result<PathBuf, ExecutionError> {
    if !path.is_absolute() {
        return Err(ExecutionError::Invalid(
            "project roots must be absolute".into(),
        ));
    }
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(ExecutionError::Invalid(
            "project root is not a directory".into(),
        ));
    }
    Ok(canonical)
}

fn validate_attempt_id(value: &str) -> Result<(), ExecutionError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ExecutionError::Invalid(
            "attempt ID must contain only ASCII letters, digits, '-' or '_'".into(),
        ));
    }
    Ok(())
}

fn validate_workspace_id(value: &str) -> Result<(), ExecutionError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| ExecutionError::Invalid("workspace ID must be a UUID".into()))
}

fn validate_volume_name(value: &str) -> Result<(), ExecutionError> {
    let Some(id) = value.strip_prefix(VOLUME_PREFIX) else {
        return Err(ExecutionError::Invalid(
            "workspace volume has an invalid name".into(),
        ));
    };
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| ExecutionError::Invalid("workspace volume has an invalid name".into()))
}

fn validate_container_name(value: &str) -> Result<(), ExecutionError> {
    let Some(id) = value.strip_prefix(CONTAINER_PREFIX) else {
        return Err(ExecutionError::Invalid(
            "sandbox container has an invalid name".into(),
        ));
    };
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| ExecutionError::Invalid("sandbox container has an invalid name".into()))
}

fn validate_record(record: &WorkspaceRecord) -> Result<(), ExecutionError> {
    if record.schema_version != STATE_SCHEMA_VERSION {
        return Err(ExecutionError::Invalid(
            "unsupported workspace state schema".into(),
        ));
    }
    validate_workspace_id(&record.workspace_id)?;
    validate_attempt_id(&record.attempt_id)?;
    validate_volume_name(&record.volume)?;
    if let Some(container) = &record.container {
        validate_container_name(container)?;
    }
    if record.worktree_device == 0 || record.worktree_inode == 0 {
        return Err(ExecutionError::Invalid(
            "stored worktree identity is invalid".into(),
        ));
    }
    if !record
        .base_commit
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit())
        || !matches!(record.base_commit.len(), 40 | 64)
    {
        return Err(ExecutionError::Invalid(
            "stored base commit is not a full object ID".into(),
        ));
    }
    Ok(())
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        if remaining == 0 {
            truncated = true;
            continue;
        }
        let take = remaining.min(read);
        output.extend_from_slice(&buffer[..take]);
        if take < read {
            truncated = true;
        }
    }
    Ok((output, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, process::Command as StdCommand, sync::Mutex};

    #[derive(Default)]
    struct LocalBackend {
        volumes: Mutex<HashMap<String, PathBuf>>,
        root: Mutex<Option<PathBuf>>,
        children: Arc<Mutex<HashMap<String, u32>>>,
    }

    impl LocalBackend {
        fn new(root: PathBuf) -> Self {
            Self {
                volumes: Mutex::new(HashMap::new()),
                root: Mutex::new(Some(root)),
                children: Arc::new(Mutex::new(HashMap::new())),
            }
        }
        fn volume_path(&self, name: &str) -> Result<PathBuf, ExecutionError> {
            self.volumes
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| ExecutionError::Volume("missing test volume".into()))
        }
    }

    impl VolumeBackend for LocalBackend {
        async fn create_volume(
            &self,
            name: &str,
            _workspace_id: &str,
        ) -> Result<(), ExecutionError> {
            validate_volume_name(name)?;
            let root = self.root.lock().unwrap().clone().unwrap();
            let path = root.join(name);
            if path.exists() {
                fs::remove_dir_all(&path)?;
            }
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
            let path = self.volume_path(name)?;
            if path.exists() {
                fs::remove_dir_all(&path)?;
            }
            workspace_archive::extract(bytes, &path)?;
            self.volumes.lock().unwrap().insert(name.into(), path);
            Ok(())
        }

        async fn export_volume(&self, name: &str) -> Result<Vec<u8>, ExecutionError> {
            let path = self.volume_path(name)?;
            Ok(workspace_archive::snapshot(&path)?.bytes)
        }

        async fn run(
            &self,
            volume: &str,
            container: &str,
            image: &str,
            argv: &[String],
            _timeout: Duration,
        ) -> Result<BoundedOutput, ExecutionError> {
            assert_eq!(image, "test/image:1");
            let path = self.volume_path(volume)?;
            let argv = argv.to_vec();
            let container = container.to_owned();
            let children = self.children.clone();
            let output = tokio::task::spawn_blocking(move || {
                let child = StdCommand::new(&argv[0])
                    .args(&argv[1..])
                    .current_dir(path)
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .spawn()?;
                if let Some(pid) = Some(child.id()) {
                    children.lock().unwrap().insert(container.clone(), pid);
                }
                let output = child.wait_with_output();
                children.lock().unwrap().remove(&container);
                output
            })
            .await
            .map_err(|error| ExecutionError::Volume(format!("test runner: {error}")))??;
            Ok(BoundedOutput {
                exit_code: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
                stdout_truncated: false,
                stderr_truncated: false,
            })
        }

        async fn remove_container(&self, name: &str) -> Result<(), ExecutionError> {
            if let Some(pid) = self.children.lock().unwrap().get(name).copied() {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
            }
            Ok(())
        }
    }

    struct Fixture {
        root: PathBuf,
        repository: PathBuf,
        manager: WorkspaceManager<LocalBackend>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("factory-execution-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let repository = root.join("repo");
            fs::create_dir(&repository).unwrap();
            git_test(&repository, &["init", "-q"]);
            git_test(
                &repository,
                &["config", "user.email", "test@example.invalid"],
            );
            git_test(&repository, &["config", "user.name", "Test"]);
            fs::write(repository.join("seed.txt"), "base\n").unwrap();
            git_test(&repository, &["add", "seed.txt"]);
            git_test(&repository, &["commit", "-qm", "base"]);
            let volume_root = root.join("volumes");
            fs::create_dir(&volume_root).unwrap();
            let manager = WorkspaceManager::new(
                root.join("work"),
                root.join("state"),
                [repository.clone()],
                LocalBackend::new(volume_root),
            )
            .unwrap();
            Self {
                root,
                repository,
                manager,
            }
        }
        fn base(&self) -> String {
            String::from_utf8(
                StdCommand::new("git")
                    .args(["-C", self.repository.to_str().unwrap(), "rev-parse", "HEAD"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
            .trim()
            .to_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git_test(root: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
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

    #[tokio::test]
    async fn cancellation_kills_a_running_execution_and_blocks_a_late_retry() {
        let fixture = Fixture::new();
        let base = fixture.base();
        let allocation = WorkspaceAllocation {
            attempt_id: "cancel-run".into(),
            repository: fixture.repository.to_string_lossy().into_owned(),
            base_commit: base,
            generation: 1,
            action_spec_sha256: String::new(),
            attempt_grant: String::new(),
        };
        let handle = fixture.manager.allocate(&allocation).await.unwrap();
        let input = ExecuteWorkspace {
            image: "test/image:1".into(),
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 30".into()],
            timeout_secs: 30,
        };
        let execution =
            fixture
                .manager
                .execute(&handle.workspace_id, &input, Duration::from_secs(30));
        tokio::pin!(execution);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(300)) => {}
            result = &mut execution => panic!("execution completed before cancellation: {result:?}"),
        }
        assert!(
            fixture
                .manager
                .cancel_active(&handle.workspace_id)
                .await
                .unwrap()
        );
        let result = execution.await.unwrap();
        assert!(result.candidate.is_none());
        assert_eq!(result.workspace.status, WorkspaceStatus::Failed);
        let retry = fixture
            .manager
            .execute(
                &handle.workspace_id,
                &ExecuteWorkspace {
                    image: "test/image:1".into(),
                    argv: vec!["/bin/true".into()],
                    timeout_secs: 5,
                },
                Duration::from_secs(5),
            )
            .await;
        assert!(
            matches!(retry, Err(ExecutionError::Conflict)),
            "retry was {retry:?}"
        );
        let _ = fixture.manager.cleanup(&handle.workspace_id).await;
    }

    #[tokio::test]
    async fn executes_snapshot_volume_and_commits_candidate() {
        let fixture = Fixture::new();
        let base = fixture.base();
        let allocation = WorkspaceAllocation {
            attempt_id: "attempt-1".into(),
            repository: fixture.repository.to_string_lossy().into_owned(),
            base_commit: base.clone(),
            action_spec_sha256: String::new(),
            attempt_grant: String::new(),
            generation: 1,
        };
        let handle = fixture.manager.allocate(&allocation).await.unwrap();
        let result = fixture
            .manager
            .execute(
                &handle.workspace_id,
                &ExecuteWorkspace {
                    image: "test/image:1".into(),
                    argv: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        "printf 'sandbox\\n' > result.txt; rm seed.txt".into(),
                    ],
                    timeout_secs: 30,
                },
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.confinement.mount_kind, "NAMED_VOLUME");
        assert!(!result.confinement.host_workspace_mounted);
        let parent = StdCommand::new("git")
            .args([
                "-C",
                fixture.repository.to_str().unwrap(),
                "rev-parse",
                &format!("{}^", result.candidate.as_ref().unwrap().commit_sha),
            ])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&parent.stdout).trim(), base);
        assert_eq!(
            StdCommand::new("git")
                .args([
                    "-C",
                    fixture.repository.to_str().unwrap(),
                    "show",
                    &format!(
                        "{}:result.txt",
                        result.candidate.as_ref().unwrap().commit_sha
                    )
                ])
                .output()
                .unwrap()
                .stdout,
            b"sandbox\n"
        );
        assert_eq!(
            fixture.manager.status(&handle.workspace_id).unwrap().status,
            WorkspaceStatus::Completed
        );
        let again = fixture
            .manager
            .execute(
                &handle.workspace_id,
                &ExecuteWorkspace {
                    image: "test/image:1".into(),
                    argv: vec!["/bin/false".into()],
                    timeout_secs: 30,
                },
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        assert_eq!(again.candidate, result.candidate);
        fixture.manager.cleanup(&handle.workspace_id).await.unwrap();
        assert!(fixture.manager.status(&handle.workspace_id).is_err());
    }

    #[tokio::test]
    async fn rejects_foreign_repositories_and_mismatched_reuse() {
        let fixture = Fixture::new();
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).unwrap();
        let error = fixture
            .manager
            .allocate(&WorkspaceAllocation {
                attempt_id: "outside".into(),
                repository: outside.to_string_lossy().into_owned(),
                base_commit: fixture.base(),
                action_spec_sha256: String::new(),
                attempt_grant: String::new(),
                generation: 1,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, ExecutionError::Invalid(_)));
        let base = fixture.base();
        let allocation = WorkspaceAllocation {
            attempt_id: "conflict".into(),
            repository: fixture.repository.to_string_lossy().into_owned(),
            base_commit: base,
            action_spec_sha256: String::new(),
            attempt_grant: String::new(),
            generation: 1,
        };
        fixture.manager.allocate(&allocation).await.unwrap();
        let mut changed = allocation;
        changed.base_commit = "0".repeat(40);
        assert!(matches!(
            fixture.manager.allocate(&changed).await,
            Err(ExecutionError::Conflict)
        ));
    }

    #[tokio::test]
    #[ignore = "requires rootless Podman and a cached alpine:3.20 image"]
    async fn real_podman_named_volume_execution_produces_candidate() {
        let fixture = Fixture::new();
        let base = fixture.base();
        let backend = RootlessPodmanVolumes::new(["alpine:3.20".to_owned()]).unwrap();
        backend.preflight().await.unwrap();
        let manager = WorkspaceManager::new(
            fixture.root.join("podman-work"),
            fixture.root.join("podman-state"),
            [fixture.repository.clone()],
            backend,
        )
        .unwrap();
        let handle = manager
            .allocate(&WorkspaceAllocation {
                attempt_id: "real-podman".into(),
                repository: fixture.repository.to_string_lossy().into_owned(),
                base_commit: base.clone(),
                action_spec_sha256: String::new(),
                attempt_grant: String::new(),
                generation: 1,
            })
            .await
            .unwrap();
        let result = manager
            .execute(
                &handle.workspace_id,
                &ExecuteWorkspace {
                    image: "alpine:3.20".into(),
                    argv: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        "printf 'podman\\n' > result.txt && rm seed.txt && test \"$HOME\" = /tmp"
                            .into(),
                    ],
                    timeout_secs: 60,
                },
                Duration::from_secs(120),
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(
            StdCommand::new("git")
                .args([
                    "-C",
                    fixture.repository.to_str().unwrap(),
                    "show",
                    &format!(
                        "{}:result.txt",
                        result.candidate.as_ref().unwrap().commit_sha
                    )
                ])
                .output()
                .unwrap()
                .stdout,
            b"podman\n"
        );
        let parent = StdCommand::new("git")
            .args([
                "-C",
                fixture.repository.to_str().unwrap(),
                "rev-parse",
                &format!("{}^", result.candidate.as_ref().unwrap().commit_sha),
            ])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&parent.stdout).trim(), base);
        manager.cleanup(&handle.workspace_id).await.unwrap();
    }

    #[tokio::test]
    async fn completed_workspace_reopens_after_worktree_cleanup() {
        let fixture = Fixture::new();
        let base = fixture.base();
        let allocation = WorkspaceAllocation {
            attempt_id: "completed-reopen".into(),
            repository: fixture.repository.to_string_lossy().into_owned(),
            base_commit: base,
            action_spec_sha256: String::new(),
            attempt_grant: String::new(),
            generation: 1,
        };
        let handle = fixture.manager.allocate(&allocation).await.unwrap();
        let result = fixture
            .manager
            .execute(
                &handle.workspace_id,
                &ExecuteWorkspace {
                    image: "test/image:1".into(),
                    argv: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        "printf 'done\\n' > result.txt".into(),
                    ],
                    timeout_secs: 30,
                },
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        assert!(result.candidate.is_some());
        let reopened = fixture.manager.allocate(&allocation).await.unwrap();
        assert_eq!(reopened.status, WorkspaceStatus::Completed);
        assert_eq!(reopened.candidate, result.candidate);
    }

    #[tokio::test]
    async fn failed_workspace_can_be_reallocated_and_retried() {
        let fixture = Fixture::new();
        let base = fixture.base();
        let allocation = WorkspaceAllocation {
            attempt_id: "failed-retry".into(),
            repository: fixture.repository.to_string_lossy().into_owned(),
            base_commit: base,
            action_spec_sha256: String::new(),
            attempt_grant: String::new(),
            generation: 1,
        };
        let handle = fixture.manager.allocate(&allocation).await.unwrap();
        let failed = fixture
            .manager
            .execute(
                &handle.workspace_id,
                &ExecuteWorkspace {
                    image: "test/image:1".into(),
                    argv: vec!["/bin/sh".into(), "-c".into(), "exit 3".into()],
                    timeout_secs: 30,
                },
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        assert_eq!(failed.exit_code, Some(3));
        assert!(failed.candidate.is_none());
        let reallocated = fixture.manager.allocate(&allocation).await.unwrap();
        assert_eq!(reallocated.status, WorkspaceStatus::Allocated);
        let retried = fixture
            .manager
            .execute(
                &reallocated.workspace_id,
                &ExecuteWorkspace {
                    image: "test/image:1".into(),
                    argv: vec![
                        "/bin/sh".into(),
                        "-c".into(),
                        "printf 'retry\\n' > retry.txt".into(),
                    ],
                    timeout_secs: 30,
                },
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        assert!(retried.candidate.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn path_replacement_during_execution_fails_closed() {
        let fixture = Fixture::new();
        let base = fixture.base();
        let repository = fixture.repository.to_string_lossy().into_owned();
        let handle = fixture
            .manager
            .allocate(&WorkspaceAllocation {
                attempt_id: "replace-race".into(),
                repository,
                base_commit: base,
                action_spec_sha256: String::new(),
                attempt_grant: String::new(),
                generation: 1,
            })
            .await
            .unwrap();
        let state_path = fixture
            .root
            .join("state")
            .join("workspace-replace-race.json");
        let workspace_id = handle.workspace_id.clone();
        let input = ExecuteWorkspace {
            image: "test/image:1".into(),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "sleep 1; printf 'candidate\\n' > result.txt".into(),
            ],
            timeout_secs: 30,
        };
        let execute = fixture
            .manager
            .execute(&workspace_id, &input, Duration::from_secs(60));
        let replace = async {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(bytes) = fs::read(&state_path)
                    && let Ok(value) = serde_json::from_slice::<WorkspaceRecord>(&bytes)
                    && value.status == WorkspaceStatus::Running
                {
                    let moved = PathBuf::from(format!("{}-moved", value.worktree));
                    fs::rename(&value.worktree, &moved).unwrap();
                    fs::create_dir(&value.worktree).unwrap();
                    fs::write(Path::new(&value.worktree).join("decoy.txt"), "decoy\\n").unwrap();
                    return value;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "never reached RUNNING"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        let (result, record) = tokio::join!(execute, replace);
        assert!(result.is_err(), "path replacement must fail closed");
        assert_eq!(
            fs::read_to_string(Path::new(&record.worktree).join("decoy.txt")).unwrap(),
            "decoy\\n"
        );
        assert_eq!(
            fixture.manager.status(&handle.workspace_id).unwrap().status,
            WorkspaceStatus::Failed
        );
    }

    #[test]
    fn podman_arguments_mount_only_the_named_volume() {
        let backend = RootlessPodmanVolumes::new(["alpine:3.20".to_owned()]).unwrap();
        let volume = format!("factory-ws-{}", Uuid::new_v4());
        let container = format!("factory-exec-{}", Uuid::new_v4());
        let args = backend
            .container_args(
                &volume,
                "alpine:3.20",
                &["/bin/echo".into(), "hello".into()],
                Duration::from_secs(30),
                &container,
            )
            .unwrap();
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into())
            .collect();
        for required in [
            "--network=none".to_owned(),
            "--read-only".to_owned(),
            "--cap-drop=all".to_owned(),
            "--security-opt=no-new-privileges".to_owned(),
            "--userns=keep-id".to_owned(),
            format!("--volume={volume}:/workspace:rw"),
        ] {
            assert!(args.contains(&required), "missing {required}");
        }
        assert_eq!(
            args.iter()
                .filter(|arg| arg.starts_with("--volume="))
                .count(),
            1
        );
        assert!(
            args.iter().all(
                |arg| !arg.starts_with("--volume=/") || arg.starts_with("--volume=factory-ws-")
            )
        );
        assert!(!args.iter().any(|arg| arg == "--privileged"
            || arg == "--network=host"
            || arg.contains("podman.sock")));
    }
}
