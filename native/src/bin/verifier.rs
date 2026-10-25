//! Independent verifier process.
//!
//! Fetches a candidate inspection from Core, re-verifies Git identity and
//! candidate scope in a clean worktree, executes the frozen ActionSpec checks
//! with a cleared environment and submits a candidate-bound verifier receipt.
//! It does not trust the Worker's claim and does not start a provider session.
//!
//! This is TRUSTED_LOCAL verification. It is not process, filesystem, network or
//! credential isolation and does not provide independent certification.

use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fmt, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use factory_native::db::CandidateInspection;
use factory_native::execution::{BoundedOutput, RootlessPodmanVolumes, VolumeBackend};
use factory_native::lead::SpecCheck;
use factory_native::workspace::{self, WorkspaceAllocator, WorkspaceError};
use factory_native::workspace_archive;
use serde_json::json;
#[cfg(test)]
use tokio::io::{AsyncRead, AsyncReadExt};
use uuid::Uuid;

#[cfg(test)]
const MAX_CHECK_STREAM_BYTES: usize = 1024 * 1024;
const MAX_GIT_STREAM_BYTES: usize = 1024 * 1024;

const GIT_CONTEXT_VARIABLES: [&str; 9] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
];

const GIT_OVERRIDES: [&str; 6] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "commit.gpgsign=false",
];

const GIT_ENVIRONMENT: [(&str, &str); 12] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
    ("GIT_GRAFT_FILE", "/dev/null"),
    ("GIT_CONFIG_COUNT", "3"),
    ("GIT_CONFIG_KEY_0", "core.hooksPath"),
    ("GIT_CONFIG_VALUE_0", "/dev/null"),
    ("GIT_CONFIG_KEY_1", "core.fsmonitor"),
    ("GIT_CONFIG_VALUE_1", "false"),
    ("GIT_CONFIG_KEY_2", "commit.gpgsign"),
    ("GIT_CONFIG_VALUE_2", "false"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

#[derive(Debug)]
enum VerificationError {
    Infrastructure(String),
    Check(String),
}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Infrastructure(message) | Self::Check(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for VerificationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangedPathModes {
    path: String,
    old_mode: String,
    new_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScopeViolation {
    Empty,
    Inconsistent,
    UnsafePath(String),
    TypeChange(String),
    OutsideAllowedPaths(String),
}

impl fmt::Display for ScopeViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("candidate contains no changed paths"),
            Self::Inconsistent => formatter.write_str("candidate diff metadata is inconsistent"),
            Self::UnsafePath(path) => {
                write!(formatter, "candidate changed an unsafe path: {path:?}")
            }
            Self::TypeChange(path) => write!(
                formatter,
                "candidate changed a symlink or gitlink path: {path:?}"
            ),
            Self::OutsideAllowedPaths(path) => write!(
                formatter,
                "candidate changed a path outside the frozen allowed paths: {path:?}"
            ),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    harden_process_git_environment();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let core = env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into());
    let core = validated_core_url(&core)?;
    let token =
        env::var("FACTORY_VERIFIER_TOKEN").map_err(|_| "FACTORY_VERIFIER_TOKEN required")?;
    let candidate_id =
        env::var("FACTORY_CANDIDATE_ID").map_err(|_| "FACTORY_CANDIDATE_ID required")?;
    let principal =
        env::var("FACTORY_VERIFIER_PRINCIPAL").unwrap_or_else(|_| "verifier-process".into());
    let workspace_root = env::var("FACTORY_VERIFIER_WORKSPACE")
        .map_err(|_| "FACTORY_VERIFIER_WORKSPACE required")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let inspection: CandidateInspection = client
        .get(core.join(&format!("quality/candidates/{candidate_id}/inspection"))?)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if inspection.action_spec.checks.is_empty() {
        return Err("candidate ActionSpec has no executable checks; refusing PASS".into());
    }
    let (outcome, failure) = match verify_candidate(&inspection, Path::new(&workspace_root)).await {
        Ok(()) => ("PASS", None),
        Err(VerificationError::Check(message)) => ("FAIL", Some(message)),
        Err(VerificationError::Infrastructure(message)) => {
            return Err(format!(
                "verifier infrastructure failure; no receipt submitted: {message}"
            )
            .into());
        }
    };
    let receipt = client
        .post(core.join("quality/receipts")?)
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": Uuid::new_v4().to_string(),
            "principal_id": principal,
            "actionSpecSha256": inspection.action_spec_sha256,
            "commitSha": inspection.candidate.commit_sha,
            "treeSha": inspection.candidate.tree_sha,
            "result": outcome,
            "evidence": "EXECUTED"
        }))
        .send()
        .await?;
    if !receipt.status().is_success() {
        return Err(format!(
            "Core rejected verifier receipt: {} {}",
            receipt.status(),
            receipt.text().await.unwrap_or_default()
        )
        .into());
    }
    if let Some(message) = failure {
        return Err(format!("verifier checks failed; FAIL receipt submitted: {message}").into());
    }
    println!("verifier PASS for {}", inspection.candidate.commit_sha);
    Ok(())
}

async fn verify_candidate(
    inspection: &CandidateInspection,
    workspace_root: &Path,
) -> Result<(), VerificationError> {
    let repository = Path::new(&inspection.repository);
    if inspection.integration {
        let expected: Vec<String> = inspection
            .lineage
            .iter()
            .filter(|parent| !parent.eq_ignore_ascii_case(&inspection.base_commit))
            .cloned()
            .collect();
        if expected.len() < 2 {
            return Err(VerificationError::Check(
                "integrated candidate has insufficient parent evidence".into(),
            ));
        }
        workspace::verify_integrated_candidate(
            repository,
            &inspection.base_commit,
            &inspection.candidate.commit_sha,
            &inspection.candidate.tree_sha,
            &expected,
        )
        .map_err(|error| {
            VerificationError::Check(format!("integration lineage rejected: {error}"))
        })?;
    } else {
        verify_git_identity(
            repository,
            &inspection.base_commit,
            &inspection.candidate.commit_sha,
            &inspection.candidate.tree_sha,
        )?;
    }
    enforce_candidate_scope(inspection)?;
    if inspection.action_spec.checks.is_empty() {
        return Err(VerificationError::Infrastructure(
            "candidate ActionSpec has no executable checks; refusing PASS".into(),
        ));
    }
    sandboxed_checks(inspection, workspace_root).await
}

async fn sandboxed_checks(
    inspection: &CandidateInspection,
    workspace_root: &Path,
) -> Result<(), VerificationError> {
    let image = env::var("FACTORY_VERIFIER_IMAGE")
        .map_err(|_| VerificationError::Infrastructure("FACTORY_VERIFIER_IMAGE required".into()))?;
    let test_backend = env::var("FACTORY_VERIFIER_TEST_BACKEND").ok().as_deref() == Some("local");
    if test_backend && !cfg!(debug_assertions) {
        return Err(VerificationError::Infrastructure(
            "the local verifier backend is not available in release builds".into(),
        ));
    }
    let backend = if test_backend {
        #[cfg(debug_assertions)]
        {
            let root =
                PathBuf::from(env::var("FACTORY_VERIFIER_TEST_VOLUME_ROOT").map_err(|_| {
                    VerificationError::Infrastructure(
                        "FACTORY_VERIFIER_TEST_VOLUME_ROOT required".into(),
                    )
                })?);
            VerifierBackend::Local(
                LocalVolumeBackend::new(root).map_err(VerificationError::Infrastructure)?,
            )
        }
        #[cfg(not(debug_assertions))]
        unreachable!("release builds reject the local backend")
    } else {
        let backend = RootlessPodmanVolumes::new([image.clone()]).map_err(|error| {
            VerificationError::Infrastructure(format!("verifier sandbox configuration: {error}"))
        })?;
        backend.preflight().await.map_err(|error| {
            VerificationError::Infrastructure(format!("verifier sandbox unavailable: {error}"))
        })?;
        VerifierBackend::Podman(backend)
    };
    std::fs::create_dir_all(workspace_root).map_err(|error| {
        VerificationError::Infrastructure(format!(
            "verifier workspace could not be created: {error}"
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(workspace_root, std::fs::Permissions::from_mode(0o700)).map_err(
            |error| {
                VerificationError::Infrastructure(format!(
                    "verifier workspace permissions: {error}"
                ))
            },
        )?;
    }
    let allocator = WorkspaceAllocator::new(Path::new(&inspection.repository), workspace_root)
        .map_err(|error| {
            VerificationError::Infrastructure(format!("verifier workspace setup: {error}"))
        })?;
    let mut workspace = allocator
        .allocate(
            &format!("verify-{}", inspection.candidate.id),
            &inspection.candidate.commit_sha,
        )
        .map_err(|error| {
            VerificationError::Infrastructure(format!("verifier worktree allocation: {error}"))
        })?;
    let result = run_sandboxed_checks(
        &backend,
        workspace.path(),
        &image,
        &inspection.action_spec.checks,
    )
    .await;
    let _ = workspace.cleanup();
    result
}

async fn run_sandboxed_checks<B: VolumeBackend>(
    backend: &B,
    worktree: &Path,
    image: &str,
    checks: &[SpecCheck],
) -> Result<(), VerificationError> {
    let snapshot = workspace_archive::snapshot(worktree).map_err(|error| {
        VerificationError::Infrastructure(format!("verifier snapshot failed: {error}"))
    })?;
    let volume = format!("factory-ws-{}", Uuid::new_v4());
    backend
        .create_volume(&volume, &Uuid::new_v4().to_string())
        .await
        .map_err(|error| {
            VerificationError::Infrastructure(format!("verifier volume create failed: {error}"))
        })?;
    let result = async {
        backend
            .import_volume(&volume, &snapshot.bytes)
            .await
            .map_err(|error| {
                VerificationError::Infrastructure(format!("verifier volume import failed: {error}"))
            })?;
        for check in checks {
            let Some(executable) = check.command.first() else {
                return Err(VerificationError::Infrastructure(format!(
                    "check {} has no executable",
                    check.id
                )));
            };
            let _ = executable;
            let container = format!("factory-exec-{}", Uuid::new_v4());
            let output: BoundedOutput = backend
                .run(
                    &volume,
                    &container,
                    image,
                    &check.command,
                    Duration::from_secs(u64::from(check.timeout_secs)),
                )
                .await
                .map_err(|error| {
                    VerificationError::Infrastructure(format!(
                        "check {} sandbox failed: {error}",
                        check.id
                    ))
                })?;
            let code = output.exit_code.unwrap_or(-1);
            if code != check.expect_exit {
                return Err(VerificationError::Check(format!(
                    "check {} exited {code}, expected {}: {}",
                    check.id,
                    check.expect_exit,
                    check_diagnostic(
                        &output.stdout,
                        output.stdout_truncated,
                        &output.stderr,
                        output.stderr_truncated
                    )
                )));
            }
        }
        Ok(())
    }
    .await;
    let cleanup = backend.remove_volume(&volume).await;
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(VerificationError::Infrastructure(format!(
            "verifier volume cleanup failed: {error}"
        ))),
        (Err(error), Err(cleanup)) => Err(VerificationError::Infrastructure(format!(
            "{error}; verifier volume cleanup failed: {cleanup}"
        ))),
    }
}

enum VerifierBackend {
    Podman(RootlessPodmanVolumes),
    #[cfg(debug_assertions)]
    Local(LocalVolumeBackend),
}

impl VolumeBackend for VerifierBackend {
    async fn create_volume(
        &self,
        name: &str,
        workspace_id: &str,
    ) -> Result<(), factory_native::execution::ExecutionError> {
        match self {
            Self::Podman(backend) => backend.create_volume(name, workspace_id).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.create_volume(name, workspace_id).await,
        }
    }
    async fn remove_volume(
        &self,
        name: &str,
    ) -> Result<(), factory_native::execution::ExecutionError> {
        match self {
            Self::Podman(backend) => backend.remove_volume(name).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.remove_volume(name).await,
        }
    }
    async fn import_volume(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), factory_native::execution::ExecutionError> {
        match self {
            Self::Podman(backend) => backend.import_volume(name, bytes).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.import_volume(name, bytes).await,
        }
    }
    async fn export_volume(
        &self,
        name: &str,
    ) -> Result<Vec<u8>, factory_native::execution::ExecutionError> {
        match self {
            Self::Podman(backend) => backend.export_volume(name).await,
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
    ) -> Result<BoundedOutput, factory_native::execution::ExecutionError> {
        match self {
            Self::Podman(backend) => backend.run(volume, container, image, argv, timeout).await,
            #[cfg(debug_assertions)]
            Self::Local(backend) => backend.run(volume, container, image, argv, timeout).await,
        }
    }
    async fn remove_container(
        &self,
        name: &str,
    ) -> Result<(), factory_native::execution::ExecutionError> {
        match self {
            Self::Podman(backend) => backend.remove_container(name).await,
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
    fn new(root: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        Ok(Self {
            root,
            volumes: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }
    fn path(&self, name: &str) -> Result<PathBuf, String> {
        self.volumes
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| "missing verifier test volume".into())
    }
}

#[cfg(debug_assertions)]
impl VolumeBackend for LocalVolumeBackend {
    async fn create_volume(
        &self,
        name: &str,
        _workspace_id: &str,
    ) -> Result<(), factory_native::execution::ExecutionError> {
        let path = self.root.join(name);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir(&path)?;
        self.volumes.lock().unwrap().insert(name.into(), path);
        Ok(())
    }
    async fn remove_volume(
        &self,
        name: &str,
    ) -> Result<(), factory_native::execution::ExecutionError> {
        if let Some(path) = self.volumes.lock().unwrap().remove(name) {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(())
    }
    async fn import_volume(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), factory_native::execution::ExecutionError> {
        let path = self
            .path(name)
            .map_err(factory_native::execution::ExecutionError::Volume)?;
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        workspace_archive::extract(bytes, &path)?;
        self.volumes.lock().unwrap().insert(name.into(), path);
        Ok(())
    }
    async fn export_volume(
        &self,
        name: &str,
    ) -> Result<Vec<u8>, factory_native::execution::ExecutionError> {
        Ok(workspace_archive::snapshot(
            &self
                .path(name)
                .map_err(factory_native::execution::ExecutionError::Volume)?,
        )?
        .bytes)
    }
    async fn run(
        &self,
        volume: &str,
        _container: &str,
        _image: &str,
        argv: &[String],
        timeout: Duration,
    ) -> Result<BoundedOutput, factory_native::execution::ExecutionError> {
        let path = self
            .path(volume)
            .map_err(factory_native::execution::ExecutionError::Volume)?;
        let argv = argv.to_vec();
        let output = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || {
                std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .current_dir(path)
                    .env_clear()
                    .env("PATH", command_path())
                    .env("HOME", "/tmp")
                    .output()
            }),
        )
        .await
        .map_err(|_| {
            factory_native::execution::ExecutionError::Volume(
                "local verifier check timed out".into(),
            )
        })?
        .map_err(|error| {
            factory_native::execution::ExecutionError::Volume(format!(
                "local verifier runner: {error}"
            ))
        })??;
        Ok(BoundedOutput {
            exit_code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }
    async fn remove_container(
        &self,
        _name: &str,
    ) -> Result<(), factory_native::execution::ExecutionError> {
        Ok(())
    }
}

#[cfg(test)]
async fn run_checks(
    worktree: &Path,
    checks: &[SpecCheck],
    check_home: &Path,
) -> Result<(), VerificationError> {
    for check in checks {
        let Some(executable) = check.command.first() else {
            return Err(VerificationError::Infrastructure(format!(
                "check {} has no executable",
                check.id
            )));
        };
        let mut command = tokio::process::Command::new(executable);
        command
            .args(&check.command[1..])
            .current_dir(worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .envs(safe_check_environment(check_home))
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            VerificationError::Infrastructure(format!(
                "check {} could not start: {error}",
                check.id
            ))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            VerificationError::Infrastructure(format!("check {} has no stdout pipe", check.id))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            VerificationError::Infrastructure(format!("check {} has no stderr pipe", check.id))
        })?;
        let run = async {
            tokio::join!(
                child.wait(),
                read_bounded(stdout, MAX_CHECK_STREAM_BYTES),
                read_bounded(stderr, MAX_CHECK_STREAM_BYTES)
            )
        };
        // `kill_on_drop` and the explicit kill only stop the direct child.
        // Grandchildren that detach into another process group can outlive the
        // verifier; bounding that requires OS-level isolation this process does
        // not provide.
        let (status, stdout, stderr) =
            match tokio::time::timeout(Duration::from_secs(u64::from(check.timeout_secs)), run)
                .await
            {
                Ok(result) => result,
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(VerificationError::Check(format!(
                        "check {} timed out after {}s",
                        check.id, check.timeout_secs
                    )));
                }
            };
        let status = status.map_err(|error| {
            VerificationError::Infrastructure(format!(
                "check {} could not be waited for: {error}",
                check.id
            ))
        })?;
        let (stdout, stdout_truncated) = stdout.map_err(|error| {
            VerificationError::Infrastructure(format!(
                "check {} stdout could not be read: {error}",
                check.id
            ))
        })?;
        let (stderr, stderr_truncated) = stderr.map_err(|error| {
            VerificationError::Infrastructure(format!(
                "check {} stderr could not be read: {error}",
                check.id
            ))
        })?;
        let code = status.code().unwrap_or(-1);
        if code != check.expect_exit {
            return Err(VerificationError::Check(format!(
                "check {} exited {code}, expected {}: {}",
                check.id,
                check.expect_exit,
                check_diagnostic(&stdout, stdout_truncated, &stderr, stderr_truncated)
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
async fn read_bounded<R>(mut reader: R, limit: usize) -> io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        if read > remaining {
            captured.extend_from_slice(&chunk[..remaining]);
            truncated = true;
        } else {
            captured.extend_from_slice(&chunk[..read]);
        }
    }
    Ok((captured, truncated))
}

fn check_diagnostic(
    stdout: &[u8],
    stdout_truncated: bool,
    stderr: &[u8],
    stderr_truncated: bool,
) -> String {
    let stderr = rendered_stream(stderr, stderr_truncated);
    if stderr.is_empty() {
        rendered_stream(stdout, stdout_truncated)
    } else {
        stderr
    }
}

fn rendered_stream(bytes: &[u8], truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(bytes).trim().to_owned();
    if truncated {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str("[truncated]");
    }
    text
}

#[cfg(test)]
fn safe_check_environment(check_home: &Path) -> Vec<(OsString, OsString)> {
    vec![
        (OsString::from("PATH"), command_path()),
        (OsString::from("HOME"), check_home.as_os_str().to_owned()),
        (OsString::from("TMPDIR"), check_home.as_os_str().to_owned()),
    ]
}

fn enforce_candidate_scope(inspection: &CandidateInspection) -> Result<(), VerificationError> {
    let repository = Path::new(&inspection.repository);
    let changed = workspace::changed_paths(
        repository,
        &inspection.base_commit,
        &inspection.candidate.commit_sha,
    )
    .map_err(|error| match error {
        WorkspaceError::Invalid(message) => {
            VerificationError::Check(format!("candidate scope rejected: {message}"))
        }
        other => {
            VerificationError::Infrastructure(format!("candidate scope inspection failed: {other}"))
        }
    })?;
    let modes = changed_path_modes(
        repository,
        &inspection.base_commit,
        &inspection.candidate.commit_sha,
    )?;
    if let Some(violation) =
        scope_violation(&changed, &inspection.action_spec.allowed_paths, &modes)
    {
        return Err(VerificationError::Check(format!(
            "candidate scope rejected: {violation}"
        )));
    }
    Ok(())
}

fn scope_violation(
    paths: &[String],
    allowed: &[String],
    modes: &[ChangedPathModes],
) -> Option<ScopeViolation> {
    if paths.is_empty() {
        return Some(ScopeViolation::Empty);
    }
    let mut mode_paths = HashSet::new();
    for entry in modes {
        if !mode_paths.insert(entry.path.as_str()) {
            return Some(ScopeViolation::Inconsistent);
        }
        if matches!(entry.old_mode.as_str(), "120000" | "160000")
            || matches!(entry.new_mode.as_str(), "120000" | "160000")
        {
            return Some(ScopeViolation::TypeChange(entry.path.clone()));
        }
    }
    if mode_paths.len() != paths.len() {
        return Some(ScopeViolation::Inconsistent);
    }
    for path in paths {
        if !mode_paths.contains(path.as_str()) {
            return Some(ScopeViolation::Inconsistent);
        }
        if !workspace::is_safe_repository_path(path) {
            return Some(ScopeViolation::UnsafePath(path.clone()));
        }
        if !workspace::path_covered(path, allowed) {
            return Some(ScopeViolation::OutsideAllowedPaths(path.clone()));
        }
    }
    None
}

fn changed_path_modes(
    repository: &Path,
    base: &str,
    commit: &str,
) -> Result<Vec<ChangedPathModes>, VerificationError> {
    let output = run_git(
        repository,
        &[
            "diff",
            "--raw",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "-z",
            base,
            commit,
        ],
    )?;
    parse_raw_diff(&output).map_err(|message| {
        VerificationError::Infrastructure(format!("candidate diff metadata is invalid: {message}"))
    })
}

fn parse_raw_diff(output: &[u8]) -> Result<Vec<ChangedPathModes>, String> {
    let mut entries = Vec::new();
    let mut records = output.split(|byte| *byte == 0);
    while let Some(header) = records.next() {
        if header.is_empty() {
            break;
        }
        let header = header
            .strip_prefix(b":")
            .ok_or("raw diff record has no mode header")?;
        let path = records.next().ok_or("raw diff record has no path")?;
        let path = std::str::from_utf8(path).map_err(|_| "raw diff path is not UTF-8")?;
        let header = std::str::from_utf8(header).map_err(|_| "raw diff header is not UTF-8")?;
        let mut fields = header.split(' ');
        let old_mode = fields.next().ok_or("raw diff record has no old mode")?;
        let new_mode = fields.next().ok_or("raw diff record has no new mode")?;
        let _old_id = fields.next().ok_or("raw diff record has no old object")?;
        let _new_id = fields.next().ok_or("raw diff record has no new object")?;
        let _status = fields.next().ok_or("raw diff record has no status")?;
        if fields.next().is_some() {
            return Err("raw diff record has extra fields".into());
        }
        entries.push(ChangedPathModes {
            path: path.to_owned(),
            old_mode: old_mode.to_owned(),
            new_mode: new_mode.to_owned(),
        });
    }
    Ok(entries)
}

fn verify_git_identity(
    repository: &Path,
    base: &str,
    commit: &str,
    tree: &str,
) -> Result<(), VerificationError> {
    let actual_tree = git(repository, &["show", "-s", "--format=%T", commit])?;
    if actual_tree != tree {
        return Err(VerificationError::Check(
            "candidate tree does not match the claimed commit".into(),
        ));
    }
    let parents = git(repository, &["show", "-s", "--format=%P", commit])?;
    let mut parents = parents.split_whitespace();
    if parents.next() != Some(base) || parents.next().is_some() {
        return Err(VerificationError::Check(
            "candidate is not a single-parent child of the frozen base".into(),
        ));
    }
    Ok(())
}

fn hardened_git() -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command.args(GIT_OVERRIDES).env_clear();
    command.env("PATH", command_path());
    for (key, value) in GIT_ENVIRONMENT {
        command.env(key, value);
    }
    command
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, VerificationError> {
    let mut child = hardened_git()
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            VerificationError::Infrastructure(format!(
                "git {} could not run: {error}",
                args.join(" ")
            ))
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| VerificationError::Infrastructure("git has no stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| VerificationError::Infrastructure("git has no stderr pipe".into()))?;
    let stdout_reader =
        std::thread::spawn(move || read_bounded_blocking(stdout, MAX_GIT_STREAM_BYTES));
    let stderr_reader =
        std::thread::spawn(move || read_bounded_blocking(stderr, MAX_GIT_STREAM_BYTES));
    let status = child.wait().map_err(|error| {
        VerificationError::Infrastructure(format!(
            "git {} could not be waited for: {error}",
            args.join(" ")
        ))
    })?;
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| VerificationError::Infrastructure("git stdout reader panicked".into()))?
        .map_err(|error| {
            VerificationError::Infrastructure(format!(
                "git {} stdout could not be read: {error}",
                args.join(" ")
            ))
        })?;
    let (stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| VerificationError::Infrastructure("git stderr reader panicked".into()))?
        .map_err(|error| {
            VerificationError::Infrastructure(format!(
                "git {} stderr could not be read: {error}",
                args.join(" ")
            ))
        })?;
    if !status.success() {
        return Err(VerificationError::Infrastructure(format!(
            "git {} failed: {}",
            args.join(" "),
            rendered_stream(&stderr, stderr_truncated)
        )));
    }
    if stdout_truncated {
        return Err(VerificationError::Infrastructure(format!(
            "git {} output exceeded {MAX_GIT_STREAM_BYTES} bytes",
            args.join(" ")
        )));
    }
    Ok(stdout)
}

fn read_bounded_blocking<R>(mut reader: R, limit: usize) -> io::Result<(Vec<u8>, bool)>
where
    R: io::Read,
{
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        if read > remaining {
            captured.extend_from_slice(&chunk[..remaining]);
            truncated = true;
        } else {
            captured.extend_from_slice(&chunk[..read]);
        }
    }
    Ok((captured, truncated))
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, VerificationError> {
    let output = run_git(cwd, args)?;
    let output = String::from_utf8(output).map_err(|error| {
        VerificationError::Infrastructure(format!(
            "git {} returned non-UTF-8 output: {error}",
            args.join(" ")
        ))
    })?;
    Ok(output.trim().to_owned())
}

fn command_path() -> OsString {
    env::var_os("PATH")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("/usr/local/bin:/usr/bin:/bin"))
}

fn harden_process_git_environment() {
    // SAFETY: called from `main` before the async runtime or any other thread
    // is started, so no concurrent environment access can occur.
    unsafe {
        for variable in GIT_CONTEXT_VARIABLES {
            env::remove_var(variable);
        }
        for (key, value) in GIT_ENVIRONMENT {
            env::set_var(key, value);
        }
    }
}

fn validated_core_url(raw: &str) -> Result<reqwest::Url, Box<dyn std::error::Error>> {
    let url = reqwest::Url::parse(raw)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("FACTORY_CORE_URL must be a loopback HTTP origin with explicit port".into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_native::lead::CheckKind;

    fn check(id: &str, command: Vec<&str>, expect_exit: i32) -> SpecCheck {
        SpecCheck {
            id: id.into(),
            kind: CheckKind::Functional,
            description: id.into(),
            command: command.into_iter().map(str::to_owned).collect(),
            expect_exit,
            timeout_secs: 30,
        }
    }

    fn modes(path: &str, old_mode: &str, new_mode: &str) -> ChangedPathModes {
        ChangedPathModes {
            path: path.into(),
            old_mode: old_mode.into(),
            new_mode: new_mode.into(),
        }
    }

    #[tokio::test]
    async fn checks_execute_argv_and_honor_expected_exit() {
        let root = std::env::temp_dir().join(format!("factory-verifier-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let home = root.join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(root.join("candidate.txt"), "expected content\n").unwrap();
        run_checks(
            &root,
            &[check(
                "present",
                vec!["grep", "-q", "expected content", "candidate.txt"],
                0,
            )],
            &home,
        )
        .await
        .unwrap();
        run_checks(
            &root,
            &[check(
                "no-factory-environment",
                vec!["sh", "-c", "env | grep -q '^FACTORY_'"],
                1,
            )],
            &home,
        )
        .await
        .unwrap();
        assert!(
            run_checks(
                &root,
                &[check(
                    "wrong-exit",
                    vec!["grep", "-q", "expected content", "candidate.txt"],
                    1,
                )],
                &home,
            )
            .await
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn bounded_reader_truncates_and_drains() {
        let (captured, truncated) = read_bounded(&b"abcdefghij"[..], 4).await.unwrap();
        assert_eq!(captured, b"abcd");
        assert!(truncated);
        let (captured, truncated) = read_bounded(&b"abcdef"[..], 6).await.unwrap();
        assert_eq!(captured, b"abcdef");
        assert!(!truncated);
        let (captured, truncated) = read_bounded(&b"abcdef"[..], 0).await.unwrap();
        assert!(captured.is_empty());
        assert!(truncated);
        let (captured, truncated) = read_bounded(&b""[..], 4).await.unwrap();
        assert!(captured.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn check_environment_is_an_allowlist() {
        let home = Path::new("/tmp/factory-check-home");
        let environment = safe_check_environment(home);
        let keys: Vec<&str> = environment
            .iter()
            .map(|(key, _)| key.to_str().unwrap())
            .collect();
        assert_eq!(keys, ["PATH", "HOME", "TMPDIR"]);
        assert!(
            environment
                .iter()
                .all(|(key, _)| !key.to_string_lossy().starts_with("FACTORY_"))
        );
        assert_eq!(environment[1].1, OsString::from("/tmp/factory-check-home"));
        assert_eq!(environment[2].1, OsString::from("/tmp/factory-check-home"));
    }

    #[test]
    fn scope_decision_rejects_empty_unsafe_out_of_scope_and_links() {
        let allowed = vec!["src".to_string()];
        assert_eq!(
            scope_violation(
                &["src/a.rs".to_string()],
                &allowed,
                &[modes("src/a.rs", "100644", "100644")]
            ),
            None
        );
        assert_eq!(
            scope_violation(&[], &allowed, &[]),
            Some(ScopeViolation::Empty)
        );
        assert_eq!(
            scope_violation(
                &["docs/a.md".to_string()],
                &allowed,
                &[modes("docs/a.md", "100644", "100644")]
            ),
            Some(ScopeViolation::OutsideAllowedPaths("docs/a.md".into()))
        );
        assert_eq!(
            scope_violation(
                &["src/link".to_string()],
                &allowed,
                &[modes("src/link", "100644", "120000")]
            ),
            Some(ScopeViolation::TypeChange("src/link".into()))
        );
        assert_eq!(
            scope_violation(
                &["src/gitlink".to_string()],
                &allowed,
                &[modes("src/gitlink", "160000", "160000")]
            ),
            Some(ScopeViolation::TypeChange("src/gitlink".into()))
        );
        assert_eq!(
            scope_violation(
                &["../escape".to_string()],
                &allowed,
                &[modes("../escape", "100644", "100644")]
            ),
            Some(ScopeViolation::UnsafePath("../escape".into()))
        );
        assert_eq!(
            scope_violation(
                &["src/a.rs".to_string()],
                &allowed,
                &[modes("src/b.rs", "100644", "100644")]
            ),
            Some(ScopeViolation::Inconsistent)
        );
        assert_eq!(
            scope_violation(
                &["src/a.rs".to_string()],
                &[],
                &[modes("src/a.rs", "100644", "100644")]
            ),
            Some(ScopeViolation::OutsideAllowedPaths("src/a.rs".into()))
        );
    }

    #[test]
    fn parses_raw_diff_modes_and_paths() {
        let output = b":100644 100644 \
1111111111111111111111111111111111111111 \
2222222222222222222222222222222222222222 M\0src/a.rs\0\
:000000 120000 0000000000000000000000000000000000000000 \
3333333333333333333333333333333333333333 A\0src/link\0";
        let entries = parse_raw_diff(output).unwrap();
        assert_eq!(
            entries,
            vec![
                modes("src/a.rs", "100644", "100644"),
                modes("src/link", "000000", "120000"),
            ]
        );
        assert_eq!(
            parse_raw_diff(b"100644 100644 a b M\0src/a.rs\0"),
            Err("raw diff record has no mode header".into())
        );
    }

    #[test]
    fn git_identity_requires_single_parent_on_frozen_base() {
        let root = std::env::temp_dir().join(format!("factory-verifier-git-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Verifier Test"],
            vec!["config", "user.email", "verifier@example.invalid"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(root.join("seed"), "base\n").unwrap();
        for arguments in [vec!["add", "seed"], vec!["commit", "-qm", "base"]] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let base = git(&root, &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(root.join("seed"), "candidate\n").unwrap();
        for arguments in [vec!["add", "seed"], vec!["commit", "-qm", "candidate"]] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let commit = git(&root, &["rev-parse", "HEAD"]).unwrap();
        let tree = git(&root, &["show", "-s", "--format=%T", &commit]).unwrap();
        verify_git_identity(&root, &base, &commit, &tree).unwrap();
        assert!(verify_git_identity(&root, &commit, &commit, &tree).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
