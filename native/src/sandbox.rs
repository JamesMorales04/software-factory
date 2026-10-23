//! Rootless Podman confinement for disposable fixture commands.
//!
//! This API is not integrated with Codex or OpenCode. A successful preflight
//! proves rootless Podman availability only; it does not certify host security.
//! Workspace mounts still use a pathname. Same-UID replacement races are not
//! eliminated, so callers must keep execution dispatch blocked pending a mount
//! mechanism that binds the opened workspace identity into Podman.

use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::Arc,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time,
};
use uuid::Uuid;

const MAX_RUNTIME: Duration = Duration::from_secs(30 * 60);
const OUTPUT_LIMIT: usize = 1024 * 1024;
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("rootless Podman is unavailable: {0}")]
    Unavailable(String),
    #[error("sandbox configuration is invalid: {0}")]
    Invalid(String),
    #[error("sandbox process I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("sandbox command failed ({status}): {stderr}")]
    Failed { status: String, stderr: String },
    #[error("sandbox command exceeded its bounded lifetime")]
    TimedOut,
    #[error("sandbox output reader failed: {0}")]
    Reader(String),
    #[error("sandbox ownership ledger error: {0}")]
    Ledger(String),
}

#[derive(Debug, Clone)]
pub struct RootlessPodman {
    executable: OsString,
    allowed_images: BTreeSet<String>,
    max_runtime: Duration,
    workspace_root: std::path::PathBuf,
    root_handle: Arc<File>,
    ledger: Arc<OwnershipLedger>,
    run_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug)]
struct OwnershipLedger {
    directory: Arc<File>,
    directory_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ContainerRecord {
    name: String,
    owner_token: String,
    workspace_name: String,
    workspace_device: u64,
    workspace_inode: u64,
    owner_pid: u32,
    owner_start: u64,
    orphaned: bool,
}

struct ContainerGuard {
    ledger: Arc<OwnershipLedger>,
    record: ContainerRecord,
    file_name: String,
    executable: OsString,
    active: bool,
}

struct RootModeGuard {
    root: Arc<File>,
    active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureResult {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl RootlessPodman {
    pub fn new(
        workspace_root: impl AsRef<Path>,
        allowed_images: impl IntoIterator<Item = String>,
        max_runtime: Duration,
    ) -> Result<Self, SandboxError> {
        let workspace_root = workspace_root.as_ref().canonicalize()?;
        if !workspace_root.is_dir() {
            return Err(SandboxError::Invalid(
                "Factory workspace root must be an existing directory".into(),
            ));
        }
        let allowed_images: BTreeSet<String> = allowed_images.into_iter().collect();
        if allowed_images.is_empty()
            || allowed_images.iter().any(|image| {
                image.is_empty() || image.starts_with('-') || image.chars().any(char::is_whitespace)
            })
        {
            return Err(SandboxError::Invalid(
                "an exact non-empty allowlist of image references is required".into(),
            ));
        }
        if max_runtime < Duration::from_secs(1) || max_runtime > MAX_RUNTIME {
            return Err(SandboxError::Invalid(format!(
                "max runtime must be between 1 and {} seconds",
                MAX_RUNTIME.as_secs()
            )));
        }
        ensure_private_directory(&workspace_root)?;
        let root_handle = Arc::new(open_directory(&workspace_root)?);
        let ledger_path = workspace_root.join(".factory-podman-ledger");
        match fs::create_dir(&ledger_path) {
            Ok(()) => set_private_directory(&ledger_path)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        ensure_private_directory(&ledger_path)?;
        let ledger_directory = Arc::new(open_directory(&ledger_path)?);
        let directory_path = proc_fd_path(&ledger_directory);
        let ledger = Arc::new(OwnershipLedger {
            directory: ledger_directory,
            directory_path,
        });
        Ok(Self {
            executable: OsString::from("podman"),
            allowed_images,
            max_runtime,
            workspace_root,
            root_handle,
            ledger,
            run_lock: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Check that the selected Podman engine is local and reports rootless
    /// operation. Failure is closed: callers must not fall back to host exec.
    pub async fn preflight(&self) -> Result<(), SandboxError> {
        let output = time::timeout(
            PREFLIGHT_TIMEOUT,
            Command::new(&self.executable)
                .arg("--remote=false")
                .args(["info", "--format", "json"])
                .env_remove("CONTAINER_HOST")
                .env_remove("CONTAINER_CONNECTION")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| SandboxError::Unavailable("podman info timed out".into()))??;
        if !output.status.success() {
            return Err(SandboxError::Unavailable(format!(
                "podman info failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let info: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
            SandboxError::Unavailable(format!("podman info returned invalid JSON: {error}"))
        })?;
        if info
            .pointer("/host/security/rootless")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err(SandboxError::Unavailable(
                "Podman did not confirm rootless mode".into(),
            ));
        }
        Ok(())
    }

    /// Reconcile containers left by a cancelled Attempt or a prior process.
    /// Live owners are skipped; only orphaned or dead-owner ledger records are
    /// eligible for owner-label verification and forced removal.
    pub async fn reconcile_orphans(&self) -> Result<usize, SandboxError> {
        self.preflight().await?;
        let records = self.ledger.read_records()?;
        let mut removed = 0;
        for (file_name, record) in records {
            if !record.orphaned && process_identity_alive(record.owner_pid, record.owner_start) {
                continue;
            }
            self.ledger
                .remove_owned(&self.executable, &file_name, &record)
                .await?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Execute an explicitly supplied argv in an allowlisted, locally cached
    /// image. Only the canonical workspace directory is mounted writable.
    pub async fn run_fixture(
        &self,
        workspace: impl AsRef<Path>,
        image: &str,
        argv: &[String],
    ) -> Result<FixtureResult, SandboxError> {
        let _run_guard = self.run_lock.lock().await;
        if !self.allowed_images.contains(image) {
            return Err(SandboxError::Invalid(format!(
                "image is not allowlisted: {image}"
            )));
        }
        if argv.is_empty() || argv[0].is_empty() {
            return Err(SandboxError::Invalid(
                "fixture argv must contain an executable".into(),
            ));
        }
        let requested_workspace = workspace.as_ref();
        if !requested_workspace.is_absolute()
            || requested_workspace.parent() != Some(self.workspace_root.as_path())
        {
            return Err(SandboxError::Invalid(
                "workspace mount must be a direct child of the Factory workspace root".into(),
            ));
        }
        let workspace_name = requested_workspace
            .file_name()
            .and_then(OsStr::to_str)
            .filter(|name| valid_workspace_name(name))
            .ok_or_else(|| SandboxError::Invalid("invalid workspace directory name".into()))?
            .to_owned();
        let workspace_file =
            open_directory_nofollow(proc_fd_path(&self.root_handle).join(&workspace_name))?;
        let workspace_metadata = workspace_file.metadata()?;
        if !workspace_metadata.is_dir() {
            return Err(SandboxError::Invalid(
                "workspace handle is not a directory".into(),
            ));
        }
        self.preflight().await?;

        let container_id = Uuid::new_v4();
        let name = format!("factory-fixture-{container_id}");
        let owner_token = Uuid::new_v4().to_string();
        let (record, file_name) =
            self.ledger
                .record(&name, &owner_token, &workspace_name, &workspace_metadata)?;
        let mut guard = ContainerGuard::new(
            self.ledger.clone(),
            record,
            file_name,
            self.executable.clone(),
        );
        let mut root_mode = RootModeGuard::restrict(self.root_handle.clone())?;
        let workspace_path = self.workspace_root.join(&workspace_name);
        ensure_workspace_identity(&workspace_path, &workspace_metadata)?;
        let mount_source = workspace_path.clone();
        let args = self.command_args(&mount_source, image, argv, &name, &owner_token)?;
        let mut podman = Command::new(&self.executable);
        podman
            .args(&args)
            .env_remove("CONTAINER_HOST")
            .env_remove("CONTAINER_CONNECTION")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let spawn = podman.spawn();
        let mut child = match spawn {
            Ok(child) => child,
            Err(error) => {
                guard.cleanup().await?;
                return Err(error.into());
            }
        };
        if let Err(error) = ensure_workspace_identity(&workspace_path, &workspace_metadata) {
            terminate(&mut child).await;
            guard.cleanup().await?;
            return Err(error);
        }
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| SandboxError::Invalid("Podman stdout pipe unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| SandboxError::Invalid("Podman stderr pipe unavailable".into()))?;
        let stdout_task = tokio::spawn(read_bounded(stdout, OUTPUT_LIMIT));
        let stderr_task = tokio::spawn(read_bounded(stderr, OUTPUT_LIMIT));

        let lifetime = self.max_runtime + Duration::from_secs(15);
        let status = match time::timeout(lifetime, child.wait()).await {
            Ok(result) => result?,
            Err(_) => {
                terminate(&mut child).await;
                stdout_task.abort();
                stderr_task.abort();
                guard.cleanup().await?;
                return Err(SandboxError::TimedOut);
            }
        };
        guard.cleanup().await?;
        let (stdout, stdout_truncated) = finish_reader(stdout_task).await?;
        let (stderr, stderr_truncated) = finish_reader(stderr_task).await?;
        let result = FixtureResult {
            exit_code: status.code(),
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        };
        root_mode.restore()?;
        if !status.success() {
            return Err(SandboxError::Failed {
                status: status.to_string(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            });
        }
        Ok(result)
    }

    fn command_args(
        &self,
        mount_source: &Path,
        image: &str,
        argv: &[String],
        name: &str,
        owner_token: &str,
    ) -> Result<Vec<OsString>, SandboxError> {
        if !self.allowed_images.contains(image) {
            return Err(SandboxError::Invalid(format!(
                "image is not allowlisted: {image}"
            )));
        }
        if argv.is_empty() || argv[0].is_empty() {
            return Err(SandboxError::Invalid(
                "fixture argv must contain an executable".into(),
            ));
        }
        let host_path = mount_source
            .to_str()
            .ok_or_else(|| SandboxError::Invalid("workspace path is not UTF-8".into()))?;
        if host_path.contains([':', ',']) {
            return Err(SandboxError::Invalid(
                "workspace path cannot contain ':' or ',' in Podman mount syntax".into(),
            ));
        }
        let seconds = self.max_runtime.as_secs().max(1).to_string();
        let mut args: Vec<OsString> = [
            "--remote=false".into(),
            "run".into(),
            "--rm".into(),
            format!("--name={name}").into(),
            format!("--label=io.software-factory.owner={owner_token}").into(),
            "--pull=never".into(),
            "--network=none".into(),
            "--read-only".into(),
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
            format!("--volume={host_path}:/workspace:rw").into(),
            image.into(),
        ]
        .into();
        args.extend(argv.iter().map(OsString::from));
        Ok(args)
    }
}

impl OwnershipLedger {
    fn record(
        &self,
        name: &str,
        owner_token: &str,
        workspace_name: &str,
        workspace_metadata: &fs::Metadata,
    ) -> Result<(ContainerRecord, String), SandboxError> {
        let id = name
            .strip_prefix("factory-fixture-")
            .ok_or_else(|| SandboxError::Ledger("invalid Factory container name".into()))?;
        let parsed = Uuid::parse_str(id)
            .map_err(|_| SandboxError::Ledger("invalid Factory container UUID".into()))?;
        Uuid::parse_str(owner_token)
            .map_err(|_| SandboxError::Ledger("invalid container owner token".into()))?;
        if !valid_workspace_name(workspace_name) {
            return Err(SandboxError::Ledger("invalid workspace identity".into()));
        }
        let (workspace_device, workspace_inode) = file_identity(workspace_metadata)?;
        let record = ContainerRecord {
            name: format!("factory-fixture-{parsed}"),
            owner_token: owner_token.to_owned(),
            workspace_name: workspace_name.to_owned(),
            workspace_device,
            workspace_inode,
            owner_pid: std::process::id(),
            owner_start: process_start_time(std::process::id())?,
            orphaned: false,
        };
        let file_name = format!("container-{parsed}.json");
        self.write_atomic(&file_name, &record, true)?;
        Ok((record, file_name))
    }

    fn write_atomic(
        &self,
        file_name: &str,
        record: &ContainerRecord,
        create_only: bool,
    ) -> Result<(), SandboxError> {
        validate_record_filename(file_name)?;
        let pending_name = format!(".pending-{}", Uuid::new_v4());
        let pending_path = self.directory_path.join(&pending_name);
        let final_path = self.directory_path.join(file_name);
        let bytes =
            serde_json::to_vec(record).map_err(|error| SandboxError::Ledger(error.to_string()))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_options(&mut options);
        let mut file = options.open(&pending_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        if create_only && fs::symlink_metadata(&final_path).is_ok() {
            let _ = fs::remove_file(&pending_path);
            return Err(SandboxError::Ledger(
                "container ledger record already exists".into(),
            ));
        }
        fs::rename(&pending_path, &final_path)?;
        self.directory.sync_all()?;
        Ok(())
    }

    fn read_records(&self) -> Result<Vec<(String, ContainerRecord)>, SandboxError> {
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.directory_path)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| SandboxError::Ledger("non-UTF-8 ledger entry".into()))?;
            if name.starts_with(".pending-") {
                validate_pending_filename(&name)?;
                let metadata = fs::symlink_metadata(entry.path())?;
                ensure_private_file(&metadata)?;
                fs::remove_file(entry.path())?;
                self.directory.sync_all()?;
                continue;
            }
            validate_record_filename(&name)?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(SandboxError::Ledger(format!(
                    "ledger entry is not a regular file: {name}"
                )));
            }
            ensure_private_file(&metadata)?;
            let bytes = fs::read(entry.path())?;
            let record: ContainerRecord = serde_json::from_slice(&bytes)
                .map_err(|error| SandboxError::Ledger(format!("invalid {name}: {error}")))?;
            validate_record(&name, &record)?;
            records.push((name, record));
        }
        Ok(records)
    }

    async fn remove_owned(
        &self,
        executable: &OsStr,
        file_name: &str,
        record: &ContainerRecord,
    ) -> Result<(), SandboxError> {
        validate_record(file_name, record)?;
        if !container_exists(executable, &record.name).await? {
            self.clear(file_name)?;
            return Ok(());
        }
        let owner = inspect_owner(executable, &record.name).await?;
        if owner != record.owner_token {
            return Err(SandboxError::Ledger(format!(
                "container {} exists but its Factory ownership label does not match",
                record.name
            )));
        }
        let _ = podman_output(
            executable,
            ["--remote=false", "rm", "--force", "--time=0", &record.name],
        )
        .await?;
        if container_exists(executable, &record.name).await? {
            return Err(SandboxError::Ledger(format!(
                "container removal is unconfirmed: {}",
                record.name
            )));
        }
        self.clear(file_name)
    }

    fn clear(&self, file_name: &str) -> Result<(), SandboxError> {
        validate_record_filename(file_name)?;
        let path = self.directory_path.join(file_name);
        if fs::symlink_metadata(&path).is_ok() {
            fs::remove_file(path)?;
            self.directory.sync_all()?;
        }
        if fs::symlink_metadata(self.directory_path.join(file_name)).is_ok() {
            return Err(SandboxError::Ledger(format!(
                "ledger entry removal is unconfirmed: {file_name}"
            )));
        }
        Ok(())
    }

    fn mark_orphaned(
        &self,
        file_name: &str,
        record: &mut ContainerRecord,
    ) -> Result<(), SandboxError> {
        record.orphaned = true;
        self.write_atomic(file_name, record, false)
    }
}

impl ContainerGuard {
    fn new(
        ledger: Arc<OwnershipLedger>,
        record: ContainerRecord,
        file_name: String,
        executable: OsString,
    ) -> Self {
        Self {
            ledger,
            record,
            file_name,
            executable,
            active: true,
        }
    }

    async fn cleanup(&mut self) -> Result<(), SandboxError> {
        self.ledger
            .remove_owned(&self.executable, &self.file_name, &self.record)
            .await?;
        self.active = false;
        Ok(())
    }
}

impl RootModeGuard {
    fn restrict(root: Arc<File>) -> Result<Self, SandboxError> {
        set_open_directory_mode(&root, 0o500)?;
        Ok(Self { root, active: true })
    }

    fn restore(&mut self) -> Result<(), SandboxError> {
        if self.active {
            set_open_directory_mode(&self.root, 0o700)?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for RootModeGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = set_open_directory_mode(&self.root, 0o700);
        }
    }
}

impl Drop for ContainerGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if self
            .ledger
            .mark_orphaned(&self.file_name, &mut self.record)
            .is_err()
        {
            // Keep the durable running record; restart reconciliation will
            // identify it through its dead owner process identity.
        }
        let ledger = self.ledger.clone();
        let record = self.record.clone();
        let file_name = self.file_name.clone();
        let executable = self.executable.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = ledger.remove_owned(&executable, &file_name, &record).await;
            });
        }
    }
}

fn valid_workspace_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn validate_record_filename(value: &str) -> Result<(), SandboxError> {
    let id = value
        .strip_prefix("container-")
        .and_then(|value| value.strip_suffix(".json"))
        .ok_or_else(|| SandboxError::Ledger("invalid ledger filename".into()))?;
    let uuid = Uuid::parse_str(id)
        .map_err(|_| SandboxError::Ledger("invalid ledger UUID filename".into()))?;
    if format!("container-{uuid}.json") != value {
        return Err(SandboxError::Ledger("non-canonical ledger filename".into()));
    }
    Ok(())
}

fn validate_pending_filename(value: &str) -> Result<(), SandboxError> {
    let id = value
        .strip_prefix(".pending-")
        .ok_or_else(|| SandboxError::Ledger("invalid pending ledger filename".into()))?;
    let uuid = Uuid::parse_str(id)
        .map_err(|_| SandboxError::Ledger("invalid pending ledger UUID".into()))?;
    if format!(".pending-{uuid}") != value {
        return Err(SandboxError::Ledger(
            "non-canonical pending filename".into(),
        ));
    }
    Ok(())
}

fn validate_record(file_name: &str, record: &ContainerRecord) -> Result<(), SandboxError> {
    validate_record_filename(file_name)?;
    let id = file_name
        .strip_prefix("container-")
        .and_then(|value| value.strip_suffix(".json"))
        .expect("validated ledger filename");
    Uuid::parse_str(&record.owner_token)
        .map_err(|_| SandboxError::Ledger("invalid owner token in ledger".into()))?;
    if record.name != format!("factory-fixture-{id}")
        || !valid_workspace_name(&record.workspace_name)
        || record.owner_pid == 0
        || record.owner_start == 0
    {
        return Err(SandboxError::Ledger(
            "ledger record identity is invalid".into(),
        ));
    }
    Ok(())
}

fn file_identity(metadata: &fs::Metadata) -> Result<(u64, u64), SandboxError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = metadata;
        Err(SandboxError::Unavailable(
            "sandbox ledger currently requires Linux rootless Podman".into(),
        ))
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), SandboxError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SandboxError::Ledger(format!(
            "private directory is not a real directory: {}",
            path.display()
        )));
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != effective_uid() || metadata.mode() & 0o077 != 0 {
            return Err(SandboxError::Ledger(format!(
                "directory must be owned by this user with mode 0700 or stricter: {}",
                path.display()
            )));
        }
        if metadata.mode() & 0o700 != 0o700 {
            return Err(SandboxError::Ledger(format!(
                "directory must be owner-readable, writable, and searchable: {}",
                path.display()
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(SandboxError::Unavailable(
            "sandbox ledger currently requires Linux rootless Podman".into(),
        ))
    }
}

fn set_private_directory(path: &Path) -> Result<(), SandboxError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(SandboxError::Unavailable(
            "sandbox ledger currently requires Linux rootless Podman".into(),
        ))
    }
}

fn set_open_directory_mode(directory: &File, mode: u32) -> Result<(), SandboxError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        directory.set_permissions(fs::Permissions::from_mode(mode))?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, mode);
        Err(SandboxError::Unavailable(
            "sandbox ledger currently requires Linux rootless Podman".into(),
        ))
    }
}

fn ensure_private_file(metadata: &fs::Metadata) -> Result<(), SandboxError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != effective_uid() || metadata.mode() & 0o077 != 0 {
            return Err(SandboxError::Ledger(
                "ledger file must be owned by this user and private".into(),
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = metadata;
        Err(SandboxError::Unavailable(
            "sandbox ledger currently requires Linux rootless Podman".into(),
        ))
    }
}

fn set_private_file_options(options: &mut OpenOptions) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
}

fn open_directory(path: &Path) -> Result<File, SandboxError> {
    open_directory_nofollow(path.to_owned())
}

fn open_directory_nofollow(path: PathBuf) -> Result<File, SandboxError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW)
            .open(path)
            .map_err(SandboxError::Io)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(SandboxError::Unavailable(
            "sandbox ledger currently requires Linux rootless Podman".into(),
        ))
    }
}

fn proc_fd_path(file: &File) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            file.as_raw_fd()
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        PathBuf::new()
    }
}

fn ensure_workspace_identity(path: &Path, expected: &fs::Metadata) -> Result<(), SandboxError> {
    let actual = fs::symlink_metadata(path)?;
    if actual.file_type().is_symlink() || !actual.is_dir() {
        return Err(SandboxError::Ledger(
            "workspace path changed or became a symlink".into(),
        ));
    }
    let (actual_device, actual_inode) = file_identity(&actual)?;
    let (expected_device, expected_inode) = file_identity(expected)?;
    if actual_device != expected_device || actual_inode != expected_inode {
        return Err(SandboxError::Ledger(
            "workspace path identity changed after it was opened".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // `geteuid` has no preconditions and returns the effective Unix user ID.
    unsafe { geteuid() }
}

#[cfg(target_os = "linux")]
fn process_start_time(pid: u32) -> Result<u64, SandboxError> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close = stat
        .rfind(')')
        .ok_or_else(|| SandboxError::Ledger("malformed process stat".into()))?;
    stat[close + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| SandboxError::Ledger("process start time missing".into()))?
        .parse()
        .map_err(|error| SandboxError::Ledger(format!("invalid process start time: {error}")))
}

#[cfg(not(target_os = "linux"))]
fn process_start_time(_pid: u32) -> Result<u64, SandboxError> {
    Err(SandboxError::Unavailable(
        "sandbox ledger currently requires Linux rootless Podman".into(),
    ))
}

fn process_identity_alive(pid: u32, start: u64) -> bool {
    process_start_time(pid).is_ok_and(|current| current == start)
}

async fn podman_output<I, S>(executable: &OsStr, args: I) -> Result<Output, SandboxError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = time::timeout(
        Duration::from_secs(10),
        Command::new(executable)
            .args(args)
            .env_remove("CONTAINER_HOST")
            .env_remove("CONTAINER_CONNECTION")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| SandboxError::Unavailable("Podman ownership command timed out".into()))??;
    Ok(output)
}

async fn container_exists(executable: &OsStr, name: &str) -> Result<bool, SandboxError> {
    let output = podman_output(executable, ["--remote=false", "container", "exists", name]).await?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(SandboxError::Unavailable(format!(
            "could not determine whether container {name} exists: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

async fn inspect_owner(executable: &OsStr, name: &str) -> Result<String, SandboxError> {
    let output = podman_output(
        executable,
        [
            "--remote=false",
            "inspect",
            "--format={{ index .Config.Labels \"io.software-factory.owner\" }}",
            name,
        ],
    )
    .await?;
    if !output.status.success() {
        return Err(SandboxError::Unavailable(format!(
            "could not inspect container ownership: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let label = String::from_utf8(output.stdout)
        .map_err(|error| SandboxError::Ledger(format!("invalid owner label output: {error}")))?;
    Ok(label.strip_suffix('\n').unwrap_or(&label).to_owned())
}

async fn terminate(child: &mut Child) {
    let _ = child.start_kill();
    let _ = time::timeout(Duration::from_secs(5), child.wait()).await;
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut captured = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        let keep = remaining.min(read);
        captured.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok((captured, truncated))
}

async fn finish_reader(
    task: tokio::task::JoinHandle<Result<(Vec<u8>, bool), std::io::Error>>,
) -> Result<(Vec<u8>, bool), SandboxError> {
    time::timeout(Duration::from_secs(5), task)
        .await
        .map_err(|_| SandboxError::Reader("reader did not terminate".into()))?
        .map_err(|error| SandboxError::Reader(error.to_string()))?
        .map_err(SandboxError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn private_root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("factory-podman-private-test-{}", Uuid::new_v4()));
        match fs::create_dir(&root) {
            Ok(()) => set_private_directory(&root).unwrap(),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("create test root: {error}"),
        }
        root
    }

    fn policy() -> RootlessPodman {
        RootlessPodman::new(
            private_root(),
            ["alpine:3.20".to_owned()],
            Duration::from_secs(90),
        )
        .unwrap()
    }

    #[test]
    fn command_uses_required_confinement_and_only_the_workspace_mount() {
        let sandbox = policy();
        let args = sandbox
            .command_args(
                Path::new("/tmp/factory-workspace"),
                "alpine:3.20",
                &["/bin/echo".into(), "hello; echo unsafe".into()],
                "factory-fixture-test",
                "00000000-0000-4000-8000-000000000000",
            )
            .unwrap();
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into())
            .collect();
        for required in [
            "--remote=false",
            "--pull=never",
            "--network=none",
            "--read-only",
            "--cap-drop=all",
            "--security-opt=no-new-privileges",
            "--userns=keep-id",
            "--pids-limit=128",
            "--memory=1g",
            "--cpus=2",
            "--timeout=90",
            "--label=io.software-factory.owner=00000000-0000-4000-8000-000000000000",
            "--volume=/tmp/factory-workspace:/workspace:rw",
        ] {
            assert!(args.contains(&required.to_owned()), "missing {required}");
        }
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--network=host" || arg == "--privileged")
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--env-host" || arg.starts_with("--device"))
        );
        assert_eq!(
            args.iter()
                .filter(|arg| arg.starts_with("--volume="))
                .count(),
            1
        );
        assert_eq!(
            args[args.len() - 3..],
            ["alpine:3.20", "/bin/echo", "hello; echo unsafe"]
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("/home/") && arg.contains(".ssh"))
        );
    }

    #[test]
    fn rejects_unallowlisted_image_bad_argv_and_ambiguous_mount_paths() {
        let sandbox = policy();
        assert!(
            sandbox
                .command_args(
                    Path::new("/tmp/work"),
                    "ubuntu:latest",
                    &["true".into()],
                    "name",
                    "00000000-0000-4000-8000-000000000000"
                )
                .is_err()
        );
        assert!(
            sandbox
                .command_args(
                    Path::new("/tmp/work"),
                    "alpine:3.20",
                    &[],
                    "name",
                    "00000000-0000-4000-8000-000000000000"
                )
                .is_err()
        );
        assert!(
            sandbox
                .command_args(
                    Path::new("/tmp/work,unsafe"),
                    "alpine:3.20",
                    &["true".into()],
                    "name",
                    "00000000-0000-4000-8000-000000000000"
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_mounts_outside_the_factory_workspace_root_before_preflight() {
        let sandbox = policy();
        let error = sandbox
            .run_fixture("/", "alpine:3.20", &["true".into()])
            .await
            .unwrap_err();
        assert!(matches!(error, SandboxError::Invalid(_)));
    }

    #[tokio::test]
    async fn captures_bounded_output_while_draining_excess_bytes() {
        let (mut writer, reader) = tokio::io::duplex(16);
        let producer = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            writer.write_all(b"0123456789abcdef").await.unwrap();
        });
        let (captured, truncated) = read_bounded(reader, 8).await.unwrap();
        producer.await.unwrap();
        assert_eq!(captured, b"01234567");
        assert!(truncated);
    }

    #[test]
    fn ownership_ledger_fsync_record_tracks_workspace_identity_and_clears() {
        let sandbox = policy();
        let workspace_name = format!("ledger-{}", Uuid::new_v4());
        let workspace = sandbox.workspace_root.join(&workspace_name);
        fs::create_dir(&workspace).unwrap();
        let handle =
            open_directory_nofollow(proc_fd_path(&sandbox.root_handle).join(&workspace_name))
                .unwrap();
        let metadata = handle.metadata().unwrap();
        let id = Uuid::new_v4();
        let token = Uuid::new_v4().to_string();
        let name = format!("factory-fixture-{id}");
        let (record, file_name) = sandbox
            .ledger
            .record(&name, &token, &workspace_name, &metadata)
            .unwrap();
        let record_path = sandbox.ledger.directory_path.join(&file_name);
        let file_metadata = fs::symlink_metadata(&record_path).unwrap();
        ensure_private_file(&file_metadata).unwrap();
        let records = sandbox.ledger.read_records().unwrap();
        assert_eq!(records, vec![(file_name.clone(), record.clone())]);
        assert_eq!(record.workspace_inode, file_identity(&metadata).unwrap().1);
        sandbox.ledger.clear(&file_name).unwrap();
        assert!(fs::symlink_metadata(record_path).is_err());
        let _ = fs::remove_dir(workspace);
    }

    #[test]
    fn rejects_workspace_roots_with_group_or_other_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("factory-podman-unprivate-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            RootlessPodman::new(&root, ["alpine:3.20".to_owned()], Duration::from_secs(10))
                .is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[ignore = "requires rootless Podman and a cached alpine:3.20 image"]
    async fn rootless_alpine_fixture_is_confined_and_workspace_is_writable() {
        let sandbox = policy();
        sandbox.preflight().await.unwrap();
        let workspace = sandbox
            .workspace_root
            .join(format!("factory-podman-fixture-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).unwrap();
        let result = sandbox
            .run_fixture(
                &workspace,
                "alpine:3.20",
                &[
                    "/bin/sh".into(),
                    "-c".into(),
                    "[ \"$HOME\" = /tmp ] && echo fixture > /workspace/result && if touch /root/forbidden 2>/dev/null; then exit 17; fi && if wget -T 3 -q -O /dev/null https://example.com; then exit 18; fi && grep -Eq '^NoNewPrivs:[[:space:]]*1$' /proc/self/status && grep -Eq '^CapEff:[[:space:]]*0+$' /proc/self/status && ! grep -Eq ' /home/| /run/user/' /proc/self/mountinfo".into(),
                ],
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(
            fs::read_to_string(workspace.join("result")).unwrap(),
            "fixture\n"
        );
        let _ = fs::remove_dir_all(workspace);

        let cancel_workspace = sandbox
            .workspace_root
            .join(format!("factory-podman-cancel-{}", Uuid::new_v4()));
        fs::create_dir(&cancel_workspace).unwrap();
        let cancel_sandbox = sandbox.clone();
        let cancel_path = cancel_workspace.clone();
        let running = tokio::spawn(async move {
            cancel_sandbox
                .run_fixture(cancel_path, "alpine:3.20", &["sleep".into(), "30".into()])
                .await
        });
        let deadline = time::Instant::now() + Duration::from_secs(15);
        let running_name = loop {
            if time::Instant::now() >= deadline {
                panic!("cancel test container did not start");
            }
            let records = sandbox.ledger.read_records().unwrap();
            let mut active = None;
            for (_, record) in records {
                if record.workspace_name == cancel_workspace.file_name().unwrap().to_string_lossy()
                    && container_exists(&sandbox.executable, &record.name)
                        .await
                        .unwrap()
                {
                    active = Some(record.name);
                    break;
                }
            }
            if let Some(name) = active {
                break name;
            }
            time::sleep(Duration::from_millis(100)).await;
        };
        running.abort();
        let _ = running.await;
        let _ = sandbox.reconcile_orphans().await.unwrap();
        assert!(
            !container_exists(&sandbox.executable, &running_name)
                .await
                .unwrap()
        );
        assert!(
            sandbox
                .ledger
                .read_records()
                .unwrap()
                .iter()
                .all(|(_, record)| record.name != running_name)
        );
        let _ = fs::remove_dir_all(cancel_workspace);

        let crash_workspace = sandbox
            .workspace_root
            .join(format!("factory-podman-crash-{}", Uuid::new_v4()));
        fs::create_dir(&crash_workspace).unwrap();
        let crash_handle = open_directory_nofollow(
            proc_fd_path(&sandbox.root_handle).join(crash_workspace.file_name().unwrap()),
        )
        .unwrap();
        let crash_metadata = crash_handle.metadata().unwrap();
        let crash_id = Uuid::new_v4();
        let crash_name = format!("factory-fixture-{crash_id}");
        let crash_token = Uuid::new_v4().to_string();
        let (mut crash_record, crash_file) = sandbox
            .ledger
            .record(
                &crash_name,
                &crash_token,
                crash_workspace.file_name().unwrap().to_str().unwrap(),
                &crash_metadata,
            )
            .unwrap();
        crash_record.owner_pid = u32::MAX;
        crash_record.owner_start = u64::MAX;
        sandbox
            .ledger
            .write_atomic(&crash_file, &crash_record, false)
            .unwrap();
        let detached = podman_output(
            &sandbox.executable,
            [
                "--remote=false".to_owned(),
                "run".into(),
                "--detach".into(),
                format!("--name={crash_name}"),
                format!("--label=io.software-factory.owner={crash_token}"),
                "--pull=never".into(),
                "--network=none".into(),
                "--read-only".into(),
                "--cap-drop=all".into(),
                "--security-opt=no-new-privileges".into(),
                "--timeout=30".into(),
                "alpine:3.20".into(),
                "sleep".into(),
                "30".into(),
            ],
        )
        .await
        .unwrap();
        assert!(detached.status.success());
        assert_eq!(sandbox.reconcile_orphans().await.unwrap(), 1);
        assert!(
            !container_exists(&sandbox.executable, &crash_name)
                .await
                .unwrap()
        );
        assert!(fs::symlink_metadata(sandbox.ledger.directory_path.join(crash_file)).is_err());
        let _ = fs::remove_dir_all(crash_workspace);

        let short_lived = RootlessPodman::new(
            sandbox.workspace_root.clone(),
            ["alpine:3.20".to_owned()],
            Duration::from_secs(1),
        )
        .unwrap();
        let timeout_workspace = sandbox
            .workspace_root
            .join(format!("factory-podman-timeout-{}", Uuid::new_v4()));
        fs::create_dir_all(&timeout_workspace).unwrap();
        let started = std::time::Instant::now();
        let timed = short_lived
            .run_fixture(
                &timeout_workspace,
                "alpine:3.20",
                &["sleep".into(), "10".into()],
            )
            .await;
        assert!(timed.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = fs::remove_dir_all(timeout_workspace);
    }
}
