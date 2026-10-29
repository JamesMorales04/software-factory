//! Factory-managed external browser test server and browser check.
//!
//! This is a Worker-side capability. It owns a disposable application test
//! server process group, captures bounded logs outside the repository, waits
//! for loopback readiness and runs a real Chromium check through the pinned
//! `playwright-core` tooling. It is TRUSTED_LOCAL control-plane policy, not
//! OS isolation.
//!
//! The server lifecycle is Linux-only and fails closed elsewhere. Browser
//! tooling absence is always reported as `NOT_TESTED`, never `PASSED`.

use std::{
    ffi::OsString,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    task::JoinHandle,
    time,
};
use uuid::Uuid;

const STATE_SCHEMA_VERSION: u32 = 1;
const MIN_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_STARTUP_TIMEOUT: Duration = Duration::from_secs(600);
const MIN_CHECK_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CHECK_TIMEOUT: Duration = Duration::from_secs(300);
const MIN_LOG_BYTES: u64 = 1024;
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_LOG_BYTES: u64 = 1024 * 1024;
const MAX_CHECK_OUTPUT: u64 = 64 * 1024;
const MAX_SCREENSHOT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_READY_PATH: usize = 512;
const MAX_ASSERTION: usize = 2048;
const MAX_ARGV: usize = 64;
const MAX_ARG_BYTES: usize = 4096;
const TERMINATE_TIMEOUT: Duration = Duration::from_secs(5);
const KILL_TIMEOUT: Duration = Duration::from_secs(2);
const LOG_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const READY_POLL: Duration = Duration::from_millis(100);
const PORT_CLOSE_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("browser configuration is invalid: {0}")]
    Invalid(String),
    #[error("browser I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("authorized loopback port {0} is already occupied by an unowned listener")]
    PortOccupied(u16),
    #[error("managed test server did not become ready: {0}")]
    NotReady(String),
    #[error("managed test server startup timed out")]
    StartupTimedOut,
    #[error("managed server state is not owned by this process/boot: {0}")]
    NotOwned(String),
    #[error("authorized loopback port {0} did not close after process termination")]
    PortNotClosed(u16),
    #[error("managed test server process group {0} still has live processes")]
    ProcessRemains(i32),
    #[error("managed test server lifecycle is unsupported on this platform")]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LoopbackEndpoint {
    pub host: String,
    pub port: u16,
}

impl LoopbackEndpoint {
    pub fn parse(value: &str) -> Result<Self, BrowserError> {
        let url = reqwest::Url::parse(value)
            .map_err(|_| BrowserError::Invalid("endpoint is not a valid URL".into()))?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(BrowserError::Invalid(
                "endpoint must be a plain http://127.0.0.1:<port> URL".into(),
            ));
        }
        let port = url
            .port()
            .filter(|port| *port != 0)
            .ok_or_else(|| BrowserError::Invalid("endpoint has no authorized port".into()))?;
        Ok(Self {
            host: "127.0.0.1".into(),
            port,
        })
    }

    pub fn allocate() -> Result<Self, BrowserError> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        drop(listener);
        Ok(Self {
            host: "127.0.0.1".into(),
            port,
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn assert_free(&self) -> Result<(), BrowserError> {
        match std::net::TcpListener::bind((self.host.as_str(), self.port)) {
            Ok(listener) => {
                drop(listener);
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                Err(BrowserError::PortOccupied(self.port))
            }
            Err(error) => Err(BrowserError::Io(error)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManagedTestServerConfig {
    /// Explicit argv vector; never a shell string.
    pub argv: Vec<String>,
    /// Exact Attempt workspace directory.
    pub cwd: PathBuf,
    /// Authorized loopback endpoint. `None` allocates a free loopback port.
    pub endpoint: Option<LoopbackEndpoint>,
    pub ready_path: String,
    pub startup_timeout: Duration,
    /// Worker-owned log directory outside the repository.
    pub log_dir: PathBuf,
    pub max_log_bytes: u64,
}

impl Default for ManagedTestServerConfig {
    fn default() -> Self {
        Self {
            argv: Vec::new(),
            cwd: PathBuf::new(),
            endpoint: None,
            ready_path: "/".into(),
            startup_timeout: Duration::from_secs(30),
            log_dir: PathBuf::new(),
            max_log_bytes: DEFAULT_LOG_BYTES,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopReceipt {
    pub pid: u32,
    pub process_group: i32,
    pub endpoint: String,
    pub already_stopped: bool,
    pub port_closed: bool,
    pub process_group_absent: bool,
    pub stdout_log: String,
    pub stderr_log: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub logs_truncated: bool,
}

struct LogCapture {
    stdout: JoinHandle<std::io::Result<()>>,
    stderr: JoinHandle<std::io::Result<()>>,
}

pub struct ManagedTestServer {
    child: Option<Child>,
    pid: u32,
    start_time_ticks: u64,
    boot_id: String,
    process_group: i32,
    endpoint: LoopbackEndpoint,
    ready_path: String,
    cwd: PathBuf,
    log_dir: PathBuf,
    runtime_dir: PathBuf,
    stdout_log: PathBuf,
    stderr_log: PathBuf,
    max_log_bytes: u64,
    logs: Option<LogCapture>,
    stopped: bool,
    detached: bool,
}

impl ManagedTestServer {
    /// Start and own a disposable application test server. The caller must
    /// call [`ManagedTestServer::stop`], or let the process-group Drop guard
    /// clean it up.
    pub async fn start(config: ManagedTestServerConfig) -> Result<Self, BrowserError> {
        validate_server_config(&config)?;
        // Reject an invalid log location before reserving a port or creating
        // runtime state; this also keeps the validation deterministic under
        // concurrent test execution.
        let runtime_dir = prepare_runtime_dirs(&config)?;
        let endpoint = resolve_endpoint(&config)?;
        if let Err(error) = endpoint.assert_free() {
            let _ = std::fs::remove_dir_all(&runtime_dir);
            return Err(error);
        }
        let (stdout_log, stderr_log) = log_paths(&config.log_dir);
        let mut command = server_command(&config, &endpoint, &runtime_dir);
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| BrowserError::NotReady("server process has no pid".into()))?;
        let (start_time_ticks, boot_id) = match process_identity(pid) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = terminate_group(pid as i32, pid, 0, Some(&mut child)).await;
                return Err(error);
            }
        };
        let process_group = pid as i32;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BrowserError::NotReady("server stdout pipe unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BrowserError::NotReady("server stderr pipe unavailable".into()))?;
        let max = config.max_log_bytes;
        let logs = Some(LogCapture {
            stdout: tokio::spawn(drain_to_log(stdout, stdout_log.clone(), max)),
            stderr: tokio::spawn(drain_to_log(stderr, stderr_log.clone(), max)),
        });
        match wait_ready(
            &mut child,
            &endpoint,
            &config.ready_path,
            config.startup_timeout,
        )
        .await
        {
            Ok(()) => Ok(Self {
                child: Some(child),
                pid,
                start_time_ticks,
                boot_id,
                process_group,
                endpoint,
                ready_path: config.ready_path.clone(),
                cwd: config.cwd.clone(),
                log_dir: config.log_dir.clone(),
                runtime_dir,
                stdout_log,
                stderr_log,
                max_log_bytes: config.max_log_bytes,
                logs,
                stopped: false,
                detached: false,
            }),
            Err(error) => {
                let cleanup =
                    terminate_group(process_group, pid, start_time_ticks, Some(&mut child)).await;
                if let Some(logs) = logs {
                    finish_logs(logs).await;
                }
                cap_log_file(&stdout_log, max)?;
                cap_log_file(&stderr_log, max)?;
                let detail = read_log_tail(&stderr_log, 512);
                let _ = fs::remove_dir_all(&runtime_dir);
                match error {
                    BrowserError::NotReady(message) if !detail.is_empty() => {
                        Err(BrowserError::NotReady(format!("{message}: {detail}")))
                    }
                    other => {
                        if let Err(cleanup) = cleanup {
                            return Err(BrowserError::NotReady(format!(
                                "{other}; cleanup failed: {cleanup}"
                            )));
                        }
                        Err(other)
                    }
                }
            }
        }
    }

    /// Start without keeping a Tokio child handle. The server keeps running in
    /// its own process group and is described by a durable state file so a
    /// later `check`/`stop`/`cleanup` operation can use it.
    pub async fn start_detached(
        config: ManagedTestServerConfig,
        state_dir: &Path,
        id: &str,
    ) -> Result<ManagedServerState, BrowserError> {
        validate_server_config(&config)?;
        validate_state_dir(state_dir)?;
        validate_state_id(id)?;
        let endpoint = resolve_endpoint(&config)?;
        endpoint.assert_free()?;
        let (stdout_log, stderr_log) = log_paths(&config.log_dir);
        let stdout_file = create_private_file(&stdout_log)?;
        let stderr_file = create_private_file(&stderr_log)?;
        let runtime_dir = prepare_runtime_dirs(&config)?;
        let mut command = server_command(&config, &endpoint, &runtime_dir);
        command
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .kill_on_drop(false);
        let mut child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| BrowserError::NotReady("server process has no pid".into()))?;
        let (start_time_ticks, boot_id) = match process_identity(pid) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = terminate_group(pid as i32, pid, 0, Some(&mut child)).await;
                return Err(error);
            }
        };
        let process_group = pid as i32;
        if let Err(error) = wait_ready(
            &mut child,
            &endpoint,
            &config.ready_path,
            config.startup_timeout,
        )
        .await
        {
            let cleanup =
                terminate_group(process_group, pid, start_time_ticks, Some(&mut child)).await;
            cap_log_file(&stdout_log, config.max_log_bytes)?;
            cap_log_file(&stderr_log, config.max_log_bytes)?;
            let _ = fs::remove_dir_all(&runtime_dir);
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup) => Err(BrowserError::NotReady(format!(
                    "{error}; cleanup failed: {cleanup}"
                ))),
            };
        }
        let state = ManagedServerState {
            schema_version: STATE_SCHEMA_VERSION,
            id: id.to_owned(),
            pid,
            process_group,
            start_time_ticks,
            boot_id,
            endpoint: endpoint.url(),
            ready_path: config.ready_path.clone(),
            workspace: config.cwd.to_string_lossy().into_owned(),
            log_dir: config.log_dir.to_string_lossy().into_owned(),
            stdout_log: stdout_log.to_string_lossy().into_owned(),
            stderr_log: stderr_log.to_string_lossy().into_owned(),
            runtime_dir: runtime_dir.to_string_lossy().into_owned(),
            created_at_ms: Utc::now().timestamp_millis(),
            max_log_bytes: config.max_log_bytes,
        };
        if let Err(error) = write_state(state_dir, &state) {
            let cleanup =
                terminate_group(process_group, pid, start_time_ticks, Some(&mut child)).await;
            cap_log_file(&stdout_log, config.max_log_bytes)?;
            cap_log_file(&stderr_log, config.max_log_bytes)?;
            let _ = fs::remove_dir_all(&runtime_dir);
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup) => Err(BrowserError::NotReady(format!(
                    "{error}; cleanup failed: {cleanup}"
                ))),
            };
        }
        // The detached child deliberately outlives this process. It was not
        // spawned with kill_on_drop, so dropping the handle leaves it running.
        drop(child);
        Ok(state)
    }

    pub fn endpoint(&self) -> &LoopbackEndpoint {
        &self.endpoint
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn process_group(&self) -> i32 {
        self.process_group
    }

    pub fn start_time_ticks(&self) -> u64 {
        self.start_time_ticks
    }

    pub fn stdout_log(&self) -> &Path {
        &self.stdout_log
    }

    pub fn stderr_log(&self) -> &Path {
        &self.stderr_log
    }

    pub fn workspace(&self) -> &Path {
        &self.cwd
    }

    pub fn state(&self, id: &str) -> ManagedServerState {
        ManagedServerState {
            schema_version: STATE_SCHEMA_VERSION,
            id: id.to_owned(),
            pid: self.pid,
            process_group: self.process_group,
            start_time_ticks: self.start_time_ticks,
            boot_id: self.boot_id.clone(),
            endpoint: self.endpoint.url(),
            ready_path: self.ready_path.clone(),
            workspace: self.cwd.to_string_lossy().into_owned(),
            log_dir: self.log_dir.to_string_lossy().into_owned(),
            stdout_log: self.stdout_log.to_string_lossy().into_owned(),
            stderr_log: self.stderr_log.to_string_lossy().into_owned(),
            runtime_dir: self.runtime_dir.to_string_lossy().into_owned(),
            created_at_ms: Utc::now().timestamp_millis(),
            max_log_bytes: self.max_log_bytes,
        }
    }

    /// Stop the owned process tree, verify process-group absence and port
    /// closure, then cap the captured logs.
    pub async fn stop(mut self) -> Result<StopReceipt, BrowserError> {
        let receipt = self.stop_inner().await?;
        Ok(receipt)
    }

    /// Explicit cleanup of disposable runtime artifacts. Logs are retained
    /// unless the caller removes them; the profile is always removed by the
    /// browser check itself.
    pub fn cleanup_runtime(&self) -> std::io::Result<()> {
        match fs::remove_dir_all(&self.runtime_dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn stop_inner(&mut self) -> Result<StopReceipt, BrowserError> {
        if self.stopped {
            return Ok(self.receipt(true, true, true, false));
        }
        let child = self.child.as_mut();
        terminate_group(self.process_group, self.pid, self.start_time_ticks, child).await?;
        if let Some(logs) = self.logs.take() {
            finish_logs(logs).await;
        }
        let stdout = cap_log_file(&self.stdout_log, self.max_log_bytes)?;
        let stderr = cap_log_file(&self.stderr_log, self.max_log_bytes)?;
        let port_closed = wait_port_closed(self.endpoint.port).await;
        let absent = process_group_absent(self.process_group, self.pid, self.start_time_ticks);
        let _ = fs::remove_dir_all(&self.runtime_dir);
        self.stopped = true;
        if !port_closed {
            return Err(BrowserError::PortNotClosed(self.endpoint.port));
        }
        if !absent {
            return Err(BrowserError::ProcessRemains(self.process_group));
        }
        Ok(StopReceipt {
            pid: self.pid,
            process_group: self.process_group,
            endpoint: self.endpoint.url(),
            already_stopped: false,
            port_closed,
            process_group_absent: absent,
            stdout_log: self.stdout_log.to_string_lossy().into_owned(),
            stderr_log: self.stderr_log.to_string_lossy().into_owned(),
            stdout_bytes: stdout.0,
            stderr_bytes: stderr.0,
            logs_truncated: stdout.1 || stderr.1,
        })
    }

    fn receipt(
        &self,
        already_stopped: bool,
        port_closed: bool,
        absent: bool,
        truncated: bool,
    ) -> StopReceipt {
        StopReceipt {
            pid: self.pid,
            process_group: self.process_group,
            endpoint: self.endpoint.url(),
            already_stopped,
            port_closed,
            process_group_absent: absent,
            stdout_log: self.stdout_log.to_string_lossy().into_owned(),
            stderr_log: self.stderr_log.to_string_lossy().into_owned(),
            stdout_bytes: fs::metadata(&self.stdout_log)
                .map(|metadata| metadata.len())
                .unwrap_or_default(),
            stderr_bytes: fs::metadata(&self.stderr_log)
                .map(|metadata| metadata.len())
                .unwrap_or_default(),
            logs_truncated: truncated,
        }
    }
}

impl Drop for ManagedTestServer {
    fn drop(&mut self) {
        if self.detached || self.stopped {
            return;
        }
        #[cfg(unix)]
        {
            let _ = signal_group(self.process_group, libc::SIGKILL);
            kill_leader_if_owned(self.pid, self.start_time_ticks);
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(logs) = self.logs.take() {
            logs.stdout.abort();
            logs.stderr.abort();
        }
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ManagedServerState {
    pub schema_version: u32,
    pub id: String,
    pub pid: u32,
    pub process_group: i32,
    pub start_time_ticks: u64,
    pub boot_id: String,
    pub endpoint: String,
    pub ready_path: String,
    pub workspace: String,
    pub log_dir: String,
    pub stdout_log: String,
    pub stderr_log: String,
    #[serde(default)]
    pub runtime_dir: String,
    pub created_at_ms: i64,
    pub max_log_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupEntry {
    pub state_file: String,
    pub id: String,
    pub outcome: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupSummary {
    pub removed: usize,
    pub live: usize,
    pub failed: usize,
    pub entries: Vec<CleanupEntry>,
}

/// Stop a detached server from its recorded state. Identity (boot plus process
/// start time) is mandatory; a mismatched state is never signalled.
pub async fn stop_from_state(
    state: &ManagedServerState,
    max_log_bytes: u64,
) -> Result<StopReceipt, BrowserError> {
    validate_state(state)?;
    let current_boot = boot_id()?;
    if state.boot_id != current_boot {
        let stdout = cap_log_file(Path::new(&state.stdout_log), max_log_bytes)?;
        let stderr = cap_log_file(Path::new(&state.stderr_log), max_log_bytes)?;
        if !state.runtime_dir.is_empty() {
            let _ = fs::remove_dir_all(&state.runtime_dir);
        }
        // A state from a previous boot cannot identify a live process. Do not
        // claim the authorized port is closed: an unrelated listener may use it.
        let mut receipt = state_receipt(state, true, false, true);
        receipt.logs_truncated = stdout.1 || stderr.1;
        return Ok(receipt);
    }
    let live = leader_alive(state.pid, state.start_time_ticks)
        || !group_members(state.process_group).is_empty();
    if !live {
        let stdout = cap_log_file(Path::new(&state.stdout_log), max_log_bytes)?;
        let stderr = cap_log_file(Path::new(&state.stderr_log), max_log_bytes)?;
        if !state.runtime_dir.is_empty() {
            let _ = fs::remove_dir_all(&state.runtime_dir);
        }
        let closed = wait_port_closed(endpoint_port(&state.endpoint)?).await;
        let mut receipt = state_receipt(state, true, closed, true);
        receipt.logs_truncated = stdout.1 || stderr.1;
        return Ok(receipt);
    }
    terminate_group(state.process_group, state.pid, state.start_time_ticks, None).await?;
    let port = endpoint_port(&state.endpoint)?;
    let closed = wait_port_closed(port).await;
    let absent = process_group_absent(state.process_group, state.pid, state.start_time_ticks);
    let stdout = cap_log_file(Path::new(&state.stdout_log), max_log_bytes)?;
    let stderr = cap_log_file(Path::new(&state.stderr_log), max_log_bytes)?;
    if !state.runtime_dir.is_empty() {
        let _ = fs::remove_dir_all(&state.runtime_dir);
    }
    if !closed {
        return Err(BrowserError::PortNotClosed(port));
    }
    if !absent {
        return Err(BrowserError::ProcessRemains(state.process_group));
    }
    Ok(StopReceipt {
        pid: state.pid,
        process_group: state.process_group,
        endpoint: state.endpoint.clone(),
        already_stopped: false,
        port_closed: closed,
        process_group_absent: absent,
        stdout_log: state.stdout_log.clone(),
        stderr_log: state.stderr_log.clone(),
        stdout_bytes: stdout.0,
        stderr_bytes: stderr.0,
        logs_truncated: stdout.1 || stderr.1,
    })
}

/// Reap stale state files. Live owned servers are reported and left running
/// unless `stop_live` is set.
pub async fn cleanup_state_dir(
    state_dir: &Path,
    remove_logs: bool,
    stop_live: bool,
) -> Result<CleanupSummary, BrowserError> {
    validate_state_dir(state_dir)?;
    let mut summary = CleanupSummary {
        removed: 0,
        live: 0,
        failed: 0,
        entries: Vec::new(),
    };
    let current_boot = boot_id()?;
    let entries = match fs::read_dir(state_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(summary),
        Err(error) => return Err(BrowserError::Io(error)),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let state = match read_state(&path) {
            Ok(state) => state,
            Err(error) => {
                summary.failed += 1;
                summary.entries.push(CleanupEntry {
                    state_file: path.to_string_lossy().into_owned(),
                    id: String::new(),
                    outcome: "FAILED".into(),
                    detail: error.to_string(),
                });
                continue;
            }
        };
        let live = state.boot_id == current_boot
            && (leader_alive(state.pid, state.start_time_ticks)
                || !group_members(state.process_group).is_empty());
        if live {
            if stop_live {
                match stop_from_state(&state, state.max_log_bytes).await {
                    Ok(_) => {
                        let _ = fs::remove_file(&path);
                        summary.removed += 1;
                        summary.entries.push(CleanupEntry {
                            state_file: path.to_string_lossy().into_owned(),
                            id: state.id.clone(),
                            outcome: "STOPPED".into(),
                            detail: "owned process group terminated".into(),
                        });
                    }
                    Err(error) => {
                        summary.failed += 1;
                        summary.entries.push(CleanupEntry {
                            state_file: path.to_string_lossy().into_owned(),
                            id: state.id.clone(),
                            outcome: "FAILED".into(),
                            detail: error.to_string(),
                        });
                    }
                }
            } else {
                summary.live += 1;
                summary.entries.push(CleanupEntry {
                    state_file: path.to_string_lossy().into_owned(),
                    id: state.id.clone(),
                    outcome: "LIVE".into(),
                    detail: "owned process group is still alive".into(),
                });
            }
            continue;
        }
        let _ = cap_log_file(Path::new(&state.stdout_log), state.max_log_bytes);
        let _ = cap_log_file(Path::new(&state.stderr_log), state.max_log_bytes);
        if remove_logs {
            let _ = fs::remove_file(&state.stdout_log);
            let _ = fs::remove_file(&state.stderr_log);
        }
        if !state.runtime_dir.is_empty() {
            let _ = fs::remove_dir_all(&state.runtime_dir);
        }
        match fs::remove_file(&path) {
            Ok(()) => {
                summary.removed += 1;
                summary.entries.push(CleanupEntry {
                    state_file: path.to_string_lossy().into_owned(),
                    id: state.id.clone(),
                    outcome: "REMOVED".into(),
                    detail: "stale state and disposable artifacts removed".into(),
                });
            }
            Err(error) => {
                summary.failed += 1;
                summary.entries.push(CleanupEntry {
                    state_file: path.to_string_lossy().into_owned(),
                    id: state.id.clone(),
                    outcome: "FAILED".into(),
                    detail: error.to_string(),
                });
            }
        }
    }
    Ok(summary)
}

pub fn write_state(state_dir: &Path, state: &ManagedServerState) -> Result<PathBuf, BrowserError> {
    validate_state_dir(state_dir)?;
    validate_state(state)?;
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|_| BrowserError::Invalid("server state could not be encoded".into()))?;
    let target = state_path(state_dir, &state.id);
    let temporary = state_dir.join(format!(".{}.{}.tmp", state.id, Uuid::new_v4().simple()));
    write_private(&temporary, &bytes)?;
    fs::rename(&temporary, &target)?;
    Ok(target)
}

pub fn read_state(path: &Path) -> Result<ManagedServerState, BrowserError> {
    let bytes = fs::read(path)?;
    if bytes.len() > 64 * 1024 {
        return Err(BrowserError::Invalid("server state is too large".into()));
    }
    let state: ManagedServerState = serde_json::from_slice(&bytes)
        .map_err(|_| BrowserError::Invalid("server state is malformed".into()))?;
    validate_state(&state)?;
    Ok(state)
}

pub fn state_path(state_dir: &Path, id: &str) -> PathBuf {
    state_dir.join(format!("{id}.json"))
}

/// Snapshot of pinned browser tooling. Paths are explicit; there is no PATH
/// lookup for `playwright-core` or Chromium.
#[derive(Debug, Clone)]
pub struct BrowserTooling {
    pub node: PathBuf,
    pub runner: PathBuf,
    pub playwright_core: Option<PathBuf>,
    pub chromium_executable: Option<PathBuf>,
    pub browsers_path: Option<PathBuf>,
}

impl BrowserTooling {
    /// Read explicit tooling paths from the environment. `FACTORY_CHROMIUM_EXE`
    /// is preferred; `CHROMIUM_EXE` is accepted for verifier parity.
    pub fn from_env() -> Self {
        let runner = std::env::var_os("FACTORY_BROWSER_RUNNER")
            .map(PathBuf::from)
            .unwrap_or_else(default_runner);
        Self {
            node: std::env::var_os("FACTORY_NODE_BINARY")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("node")),
            runner,
            playwright_core: std::env::var_os("FACTORY_PLAYWRIGHT_CORE").map(PathBuf::from),
            chromium_executable: std::env::var_os("FACTORY_CHROMIUM_EXE")
                .or_else(|| std::env::var_os("CHROMIUM_EXE"))
                .map(PathBuf::from),
            browsers_path: std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").map(PathBuf::from),
        }
    }

    /// Return the first reason the pinned tooling cannot run, or `None`.
    pub fn missing(&self) -> Option<String> {
        if !self.runner.is_absolute() || !self.runner.is_file() {
            return Some(format!(
                "browser check runner is unavailable: {}",
                self.runner.display()
            ));
        }
        let Some(core) = &self.playwright_core else {
            return Some("FACTORY_PLAYWRIGHT_CORE is not set".into());
        };
        if !core.is_absolute() || !core.join("package.json").is_file() {
            return Some(format!(
                "playwright-core is unavailable: {}",
                core.display()
            ));
        }
        if let Some(executable) = &self.chromium_executable
            && (!executable.is_absolute() || !executable.is_file())
        {
            return Some(format!(
                "Chromium executable is unavailable: {}",
                executable.display()
            ));
        }
        if let Some(path) = &self.browsers_path
            && (!path.is_absolute() || !path.is_dir())
        {
            return Some(format!(
                "PLAYWRIGHT_BROWSERS_PATH is unavailable: {}",
                path.display()
            ));
        }
        if self.chromium_executable.is_none() && self.browsers_path.is_none() {
            return Some(
                "no explicit Chromium executable or PLAYWRIGHT_BROWSERS_PATH is available".into(),
            );
        }
        match std::process::Command::new(&self.node)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => None,
            Ok(status) => Some(format!("node is not usable (exit {status})")),
            Err(error) => Some(format!("node is unavailable: {error}")),
        }
    }
}

fn default_runner() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join("office/e2e-browser/browser-check-runner.mjs"))
        .unwrap_or_else(|| PathBuf::from("office/e2e-browser/browser-check-runner.mjs"))
}

#[derive(Debug, Clone)]
pub struct BrowserCheckConfig {
    pub endpoint: LoopbackEndpoint,
    /// Safe absolute URL path on the authorized endpoint.
    pub path: String,
    pub selector: Option<String>,
    pub text: Option<String>,
    pub timeout: Duration,
    /// Disposable profile directory; it must not exist or be empty.
    pub profile_dir: PathBuf,
    pub evidence_path: PathBuf,
    pub screenshot: Option<PathBuf>,
    pub tooling: BrowserTooling,
    /// Set only when this process owns the managed server that is being checked.
    pub server_pid: Option<u32>,
    pub server_process_group: Option<i32>,
    pub server_start_time_ticks: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserEvidence {
    pub status: String,
    pub url: String,
    pub title: String,
    pub console_errors: u64,
    pub failed_requests: u64,
    pub browser_version: String,
    pub screenshot: Option<String>,
    #[serde(default)]
    pub profile_removed: bool,
    #[serde(default)]
    pub managed_server_owned: bool,
    #[serde(default)]
    pub server_pid: Option<u32>,
    #[serde(default)]
    pub server_process_group: Option<i32>,
    #[serde(default)]
    pub server_start_time_ticks: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BrowserCheckStatus {
    Passed,
    NotTested,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCheckReport {
    pub status: BrowserCheckStatus,
    pub detail: String,
    pub evidence: Option<BrowserEvidence>,
    pub duration_ms: u64,
}

impl BrowserCheckReport {
    pub fn passed(&self) -> bool {
        self.status == BrowserCheckStatus::Passed
    }
}

/// Run a real Chromium assertion through the fixed pinned runner. Missing
/// tooling is `NOT_TESTED`; any assertion or lifecycle failure is `FAILED`.
pub async fn run_browser_check(config: BrowserCheckConfig) -> BrowserCheckReport {
    let started = Instant::now();
    if let Err(error) = validate_check_config(&config) {
        return report(BrowserCheckStatus::Failed, error.to_string(), None, started);
    }
    if let Some(missing) = config.tooling.missing() {
        return report(BrowserCheckStatus::NotTested, missing, None, started);
    }
    let home =
        std::env::temp_dir().join(format!("factory-browser-home-{}", Uuid::new_v4().simple()));
    if let Err(error) = create_private_dir(&home) {
        return report(BrowserCheckStatus::Failed, error.to_string(), None, started);
    }
    let url = format!("{}{}", config.endpoint.url(), config.path);
    let mut command = Command::new(&config.tooling.node);
    command
        .arg(&config.tooling.runner)
        .arg("--url")
        .arg(&url)
        .arg("--profile")
        .arg(&config.profile_dir)
        .arg("--evidence")
        .arg(&config.evidence_path)
        .arg("--timeout-ms")
        .arg(config.timeout.as_millis().to_string())
        .env_clear()
        .env("PATH", base_path())
        .env("HOME", &home)
        .env("TMPDIR", &home)
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(selector) = &config.selector {
        command.arg("--selector").arg(selector);
    }
    if let Some(text) = &config.text {
        command.arg("--text").arg(text);
    }
    if let Some(screenshot) = &config.screenshot {
        command.arg("--screenshot").arg(screenshot);
    }
    if let Some(core) = &config.tooling.playwright_core {
        command.env("FACTORY_PLAYWRIGHT_CORE", core);
    }
    if let Some(executable) = &config.tooling.chromium_executable {
        command.env("CHROMIUM_EXE", executable);
    }
    if let Some(path) = &config.tooling.browsers_path {
        command.env("PLAYWRIGHT_BROWSERS_PATH", path);
    }
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&home);
            let status = if error.kind() == std::io::ErrorKind::NotFound {
                BrowserCheckStatus::NotTested
            } else {
                BrowserCheckStatus::Failed
            };
            return report(
                status,
                format!("browser check could not start: {error}"),
                None,
                started,
            );
        }
    };
    let pid = child.id().unwrap_or_default();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = stdout.map(|stdout| tokio::spawn(drain_to_vec(stdout, MAX_CHECK_OUTPUT)));
    let stderr_task = stderr.map(|stderr| tokio::spawn(drain_to_vec(stderr, MAX_CHECK_OUTPUT)));
    let status = match time::timeout(config.timeout, child.wait()).await {
        Ok(result) => result,
        Err(_) => {
            let _ = signal_group(pid as i32, libc::SIGKILL);
            let _ = child.start_kill();
            let _ = child.wait().await;
            if let Some(task) = stdout_task {
                task.abort();
            }
            if let Some(task) = stderr_task {
                task.abort();
            }
            let _ = fs::remove_dir_all(&config.profile_dir);
            let _ = fs::remove_dir_all(&home);
            return report(
                BrowserCheckStatus::Failed,
                "browser check timed out".into(),
                None,
                started,
            );
        }
    };
    let stdout = finish_output(stdout_task).await;
    let stderr = finish_output(stderr_task).await;
    let detail = summarize_output(stderr.as_deref(), stdout.as_deref());
    let cleanup_ok = remove_dir_all(&config.profile_dir) && remove_dir_all(&home);
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            return report(
                BrowserCheckStatus::Failed,
                format!("browser check wait failed: {error}"),
                None,
                started,
            );
        }
    };
    if status.code() == Some(77) {
        return report(
            BrowserCheckStatus::NotTested,
            if detail.is_empty() {
                "browser tooling is unavailable".into()
            } else {
                detail
            },
            None,
            started,
        );
    }
    if !status.success() {
        return report(
            BrowserCheckStatus::Failed,
            if detail.is_empty() {
                format!("browser check exited {status}")
            } else {
                detail
            },
            None,
            started,
        );
    }
    let mut evidence = match read_evidence(&config.evidence_path) {
        Ok(evidence) => evidence,
        Err(error) => {
            return report(
                BrowserCheckStatus::Failed,
                format!("browser evidence is invalid: {error}"),
                None,
                started,
            );
        }
    };
    evidence.profile_removed = cleanup_ok;
    evidence.managed_server_owned = config.server_pid.is_some()
        && config.server_process_group.is_some()
        && config.server_start_time_ticks.is_some();
    evidence.server_pid = config.server_pid;
    evidence.server_process_group = config.server_process_group;
    evidence.server_start_time_ticks = config.server_start_time_ticks;
    if let Some(screenshot) = &config.screenshot {
        let bounded = fs::metadata(screenshot)
            .map(|metadata| metadata.is_file() && metadata.len() <= MAX_SCREENSHOT_BYTES)
            .unwrap_or(false);
        if !bounded {
            let _ = fs::remove_file(screenshot);
            return report(
                BrowserCheckStatus::Failed,
                "browser screenshot is missing or exceeds its bound".into(),
                Some(evidence),
                started,
            );
        }
    }
    if let Err(error) = write_private(
        &config.evidence_path,
        &serde_json::to_vec_pretty(&evidence).unwrap_or_default(),
    ) {
        return report(
            BrowserCheckStatus::Failed,
            format!("browser evidence could not be updated: {error}"),
            Some(evidence),
            started,
        );
    }
    if evidence.status != "PASSED" {
        return report(
            BrowserCheckStatus::Failed,
            "browser assertion did not pass".into(),
            Some(evidence),
            started,
        );
    }
    if !cleanup_ok {
        return report(
            BrowserCheckStatus::Failed,
            "disposable browser profile/home could not be removed".into(),
            Some(evidence),
            started,
        );
    }
    report(
        BrowserCheckStatus::Passed,
        "browser assertion passed".into(),
        Some(evidence),
        started,
    )
}

fn report(
    status: BrowserCheckStatus,
    detail: String,
    evidence: Option<BrowserEvidence>,
    started: Instant,
) -> BrowserCheckReport {
    BrowserCheckReport {
        status,
        detail: detail.chars().take(1000).collect(),
        evidence,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    }
}

fn read_evidence(path: &Path) -> Result<BrowserEvidence, BrowserError> {
    let bytes = fs::read(path)?;
    if bytes.len() > 64 * 1024 {
        return Err(BrowserError::Invalid(
            "browser evidence is too large".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| BrowserError::Invalid("browser evidence is malformed".into()))
}

fn validate_server_config(config: &ManagedTestServerConfig) -> Result<(), BrowserError> {
    if config.argv.is_empty() || config.argv[0].is_empty() {
        return Err(BrowserError::Invalid(
            "server argv must contain an executable".into(),
        ));
    }
    if config.argv.len() > MAX_ARGV || config.argv.iter().any(|value| value.len() > MAX_ARG_BYTES) {
        return Err(BrowserError::Invalid(
            "server argv exceeds its bound".into(),
        ));
    }
    if !config.cwd.is_absolute() || !config.cwd.is_dir() {
        return Err(BrowserError::Invalid(
            "server cwd must be an absolute directory".into(),
        ));
    }
    if !config.log_dir.is_absolute() {
        return Err(BrowserError::Invalid(
            "server log directory must be absolute".into(),
        ));
    }
    if config.startup_timeout < MIN_STARTUP_TIMEOUT || config.startup_timeout > MAX_STARTUP_TIMEOUT
    {
        return Err(BrowserError::Invalid(
            "startup timeout must be between 5 and 600 seconds".into(),
        ));
    }
    if !(MIN_LOG_BYTES..=MAX_LOG_BYTES).contains(&config.max_log_bytes) {
        return Err(BrowserError::Invalid(
            "max log bytes must be between 1 KiB and 16 MiB".into(),
        ));
    }
    validate_url_path(&config.ready_path)?;
    Ok(())
}

fn validate_check_config(config: &BrowserCheckConfig) -> Result<(), BrowserError> {
    validate_url_path(&config.path)?;
    if config.selector.as_deref().is_none_or(str::is_empty)
        && config.text.as_deref().is_none_or(str::is_empty)
    {
        return Err(BrowserError::Invalid(
            "browser check requires a selector or text assertion".into(),
        ));
    }
    if config
        .selector
        .as_deref()
        .is_some_and(|value| value.len() > MAX_ASSERTION)
        || config
            .text
            .as_deref()
            .is_some_and(|value| value.len() > MAX_ASSERTION)
    {
        return Err(BrowserError::Invalid(
            "browser assertion exceeds its bound".into(),
        ));
    }
    if config.timeout < MIN_CHECK_TIMEOUT || config.timeout > MAX_CHECK_TIMEOUT {
        return Err(BrowserError::Invalid(
            "browser check timeout must be between 1 and 300 seconds".into(),
        ));
    }
    if !config.profile_dir.is_absolute()
        || config.profile_dir == Path::new("/")
        || !config.evidence_path.is_absolute()
    {
        return Err(BrowserError::Invalid(
            "browser profile and evidence paths must be absolute".into(),
        ));
    }
    if config.profile_dir.exists() && !is_empty_directory(&config.profile_dir) {
        return Err(BrowserError::Invalid(
            "browser profile directory must not already contain data".into(),
        ));
    }
    if let Some(screenshot) = &config.screenshot
        && (!screenshot.is_absolute() || screenshot == Path::new("/"))
    {
        return Err(BrowserError::Invalid(
            "browser screenshot path must be absolute".into(),
        ));
    }
    Ok(())
}

fn validate_url_path(path: &str) -> Result<(), BrowserError> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() > MAX_READY_PATH
        || path
            .chars()
            .any(|value| value.is_control() || value == '\\' || value == '#' || value == '?')
        || path
            .split('/')
            .any(|segment| segment == ".." || segment == ".")
    {
        return Err(BrowserError::Invalid(
            "URL path must be a safe absolute path".into(),
        ));
    }
    Ok(())
}

fn validate_state(state: &ManagedServerState) -> Result<(), BrowserError> {
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(BrowserError::Invalid(
            "server state schema is unsupported".into(),
        ));
    }
    validate_state_id(&state.id)?;
    endpoint_port(&state.endpoint)?;
    if state.pid <= 1 || state.process_group <= 1 {
        return Err(BrowserError::Invalid(
            "server state has an invalid process identity".into(),
        ));
    }
    if state.start_time_ticks == 0 || state.boot_id.is_empty() {
        return Err(BrowserError::Invalid(
            "server state has no process identity".into(),
        ));
    }
    if !(MIN_LOG_BYTES..=MAX_LOG_BYTES).contains(&state.max_log_bytes) {
        return Err(BrowserError::Invalid(
            "server state has an invalid log bound".into(),
        ));
    }
    Ok(())
}

fn validate_state_dir(path: &Path) -> Result<(), BrowserError> {
    if !path.is_absolute() {
        return Err(BrowserError::Invalid(
            "state directory must be absolute".into(),
        ));
    }
    create_private_dir(path)?;
    Ok(())
}

fn validate_state_id(id: &str) -> Result<(), BrowserError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
    {
        return Err(BrowserError::Invalid(
            "state id must be a bounded safe slug".into(),
        ));
    }
    Ok(())
}

fn resolve_endpoint(config: &ManagedTestServerConfig) -> Result<LoopbackEndpoint, BrowserError> {
    match &config.endpoint {
        Some(endpoint) => Ok(endpoint.clone()),
        None => LoopbackEndpoint::allocate(),
    }
}

fn prepare_runtime_dirs(config: &ManagedTestServerConfig) -> Result<PathBuf, BrowserError> {
    prepare_log_dir(config)?;
    let runtime = config
        .log_dir
        .join(format!("runtime-{}", Uuid::new_v4().simple()));
    create_private_dir(&runtime)?;
    create_private_dir(&runtime.join("home"))?;
    create_private_dir(&runtime.join("tmp"))?;
    Ok(runtime)
}

fn prepare_log_dir(config: &ManagedTestServerConfig) -> Result<(), BrowserError> {
    if !config.cwd.is_absolute() || !config.cwd.is_dir() {
        return Err(BrowserError::Invalid(
            "server cwd must be an absolute directory".into(),
        ));
    }
    let cwd = config.cwd.canonicalize()?;
    if config.log_dir.starts_with(&cwd) {
        return Err(BrowserError::Invalid(
            "server logs must be written outside the repository".into(),
        ));
    }
    fs::create_dir_all(&config.log_dir)?;
    set_private_directory(&config.log_dir)?;
    let log_dir = config.log_dir.canonicalize()?;
    if log_dir == cwd || log_dir.starts_with(&cwd) {
        return Err(BrowserError::Invalid(
            "server logs must be written outside the repository".into(),
        ));
    }
    Ok(())
}

fn log_paths(log_dir: &Path) -> (PathBuf, PathBuf) {
    (
        log_dir.join("test-server.stdout.log"),
        log_dir.join("test-server.stderr.log"),
    )
}

fn server_command(
    config: &ManagedTestServerConfig,
    endpoint: &LoopbackEndpoint,
    runtime_dir: &Path,
) -> Command {
    let mut command = Command::new(&config.argv[0]);
    command
        .args(&config.argv[1..])
        .current_dir(&config.cwd)
        .env_clear()
        .env("PATH", base_path())
        .env("HOME", runtime_dir.join("home"))
        .env("TMPDIR", runtime_dir.join("tmp"))
        .env("HOST", "127.0.0.1")
        .env("PORT", endpoint.port.to_string())
        .env("FACTORY_TEST_ENDPOINT", endpoint.url())
        .stdin(Stdio::null())
        .process_group(0);
    command
}

async fn wait_ready(
    child: &mut Child,
    endpoint: &LoopbackEndpoint,
    ready_path: &str,
    timeout: Duration,
) -> Result<(), BrowserError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| BrowserError::NotReady(error.to_string()))?;
    let url = format!("{}{}", endpoint.url(), ready_path);
    let deadline = time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Err(BrowserError::NotReady(format!(
                "server exited before readiness: {status}"
            )));
        }
        if let Ok(response) = client.get(&url).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        if time::Instant::now() >= deadline {
            return Err(BrowserError::StartupTimedOut);
        }
        time::sleep(READY_POLL).await;
    }
}

async fn terminate_group(
    process_group: i32,
    leader_pid: u32,
    leader_start_time: u64,
    mut child: Option<&mut Child>,
) -> Result<(), BrowserError> {
    if process_group <= 1 {
        return Err(BrowserError::Invalid(
            "invalid managed process group".into(),
        ));
    }
    let mut live =
        leader_alive(leader_pid, leader_start_time) || !group_members(process_group).is_empty();
    if live {
        let _ = signal_group(process_group, libc::SIGTERM);
        let deadline = time::Instant::now() + TERMINATE_TIMEOUT;
        loop {
            if let Some(child) = child.as_deref_mut() {
                let _ = child.try_wait()?;
            }
            live = leader_alive(leader_pid, leader_start_time)
                || !group_members(process_group).is_empty();
            if !live || time::Instant::now() >= deadline {
                break;
            }
            time::sleep(Duration::from_millis(50)).await;
        }
    }
    if live {
        let _ = signal_group(process_group, libc::SIGKILL);
        kill_leader_if_owned(leader_pid, leader_start_time);
        let deadline = time::Instant::now() + KILL_TIMEOUT;
        loop {
            if let Some(child) = child.as_deref_mut() {
                let _ = child.try_wait()?;
            }
            live = leader_alive(leader_pid, leader_start_time)
                || !group_members(process_group).is_empty();
            if !live || time::Instant::now() >= deadline {
                break;
            }
            time::sleep(Duration::from_millis(50)).await;
        }
    }
    if live {
        return Err(BrowserError::ProcessRemains(process_group));
    }
    if let Some(child) = child {
        let _ = child.wait().await;
    }
    Ok(())
}

fn process_group_absent(process_group: i32, leader_pid: u32, leader_start: u64) -> bool {
    !leader_alive(leader_pid, leader_start) && group_members(process_group).is_empty()
}

async fn wait_port_closed(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    for _ in 0..20 {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        match std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)) {
            Ok(_) => time::sleep(PORT_CLOSE_POLL).await,
            Err(_) => return true,
        }
    }
    false
}

fn endpoint_port(endpoint: &str) -> Result<u16, BrowserError> {
    LoopbackEndpoint::parse(endpoint).map(|endpoint| endpoint.port)
}

fn state_receipt(
    state: &ManagedServerState,
    already_stopped: bool,
    port_closed: bool,
    absent: bool,
) -> StopReceipt {
    StopReceipt {
        pid: state.pid,
        process_group: state.process_group,
        endpoint: state.endpoint.clone(),
        already_stopped,
        port_closed,
        process_group_absent: absent,
        stdout_log: state.stdout_log.clone(),
        stderr_log: state.stderr_log.clone(),
        stdout_bytes: fs::metadata(&state.stdout_log)
            .map(|metadata| metadata.len())
            .unwrap_or_default(),
        stderr_bytes: fs::metadata(&state.stderr_log)
            .map(|metadata| metadata.len())
            .unwrap_or_default(),
        logs_truncated: false,
    }
}

fn process_identity(pid: u32) -> Result<(u64, String), BrowserError> {
    let start = process_start_time(pid)
        .ok_or_else(|| BrowserError::NotReady("server process disappeared".into()))?;
    let boot = boot_id()?;
    Ok((start, boot))
}

fn boot_id() -> Result<String, BrowserError> {
    #[cfg(target_os = "linux")]
    {
        Ok(fs::read_to_string("/proc/sys/kernel/random/boot_id")?
            .trim()
            .to_owned())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(BrowserError::Unsupported)
    }
}

#[derive(Debug, Clone, Copy)]
struct ProcStat {
    state: char,
    process_group: i32,
    start_time: u64,
}

fn read_proc_stat(pid: u32) -> Option<ProcStat> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(ProcStat {
        state: fields.first()?.chars().next()?,
        process_group: fields.get(2)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

fn process_start_time(pid: u32) -> Option<u64> {
    read_proc_stat(pid)
        .filter(|stat| !matches!(stat.state, 'Z' | 'X' | 'x'))
        .map(|stat| stat.start_time)
}

fn leader_alive(pid: u32, start_time: u64) -> bool {
    read_proc_stat(pid)
        .is_some_and(|stat| !matches!(stat.state, 'Z' | 'X' | 'x') && stat.start_time == start_time)
}

fn group_members(process_group: i32) -> Vec<u32> {
    let mut members = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return members;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if let Some(stat) = read_proc_stat(pid)
            && stat.process_group == process_group
            && !matches!(stat.state, 'Z' | 'X' | 'x')
        {
            members.push(pid);
        }
    }
    members
}

#[cfg(unix)]
fn signal_group(process_group: i32, signal: i32) -> std::io::Result<()> {
    if process_group <= 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to signal an invalid process group",
        ));
    }
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(not(unix))]
fn signal_group(_process_group: i32, _signal: i32) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "process groups require Unix",
    ))
}

fn kill_leader_if_owned(pid: u32, start_time: u64) {
    if !leader_alive(pid, start_time) {
        return;
    }
    #[cfg(unix)]
    {
        // A TRUSTED_LOCAL server may leave its process group. Verify the exact
        // /proc start time immediately before the direct fallback signal so PID
        // reuse cannot redirect it.
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

async fn drain_to_log(
    mut reader: impl AsyncRead + Unpin,
    path: PathBuf,
    limit: u64,
) -> std::io::Result<()> {
    let mut file = tokio::fs::File::create(&path).await?;
    let mut bytes = 0u64;
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let room = limit.saturating_sub(bytes);
        let take = (read as u64).min(room) as usize;
        if take > 0 {
            file.write_all(&buffer[..take]).await?;
            bytes += take as u64;
        }
        if take < read {
            truncated = true;
        }
    }
    file.flush().await?;
    let _ = truncated;
    Ok(())
}

async fn finish_output(
    task: Option<JoinHandle<std::io::Result<(String, bool)>>>,
) -> Option<String> {
    let task = task?;
    let abort = task.abort_handle();
    match time::timeout(LOG_DRAIN_TIMEOUT, task).await {
        Ok(Ok(Ok((text, _)))) => Some(text),
        Ok(_) => None,
        Err(_) => {
            abort.abort();
            None
        }
    }
}

async fn drain_to_vec(
    mut reader: impl AsyncRead + Unpin,
    limit: u64,
) -> std::io::Result<(String, bool)> {
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let room = limit.saturating_sub(captured.len() as u64);
        let take = (read as u64).min(room) as usize;
        captured.extend_from_slice(&buffer[..take]);
        if take < read {
            truncated = true;
        }
    }
    Ok((String::from_utf8_lossy(&captured).into_owned(), truncated))
}

async fn finish_logs(logs: LogCapture) {
    for task in [logs.stdout, logs.stderr] {
        let abort = task.abort_handle();
        if time::timeout(LOG_DRAIN_TIMEOUT, task).await.is_err() {
            abort.abort();
        }
    }
}

fn cap_log_file(path: &Path, limit: u64) -> Result<(u64, bool), BrowserError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, false)),
        Err(error) => return Err(BrowserError::Io(error)),
    };
    if metadata.len() <= limit {
        return Ok((metadata.len(), false));
    }
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(metadata.len().saturating_sub(limit)))?;
    let mut tail = Vec::new();
    file.take(limit).read_to_end(&mut tail)?;
    fs::write(path, &tail)?;
    set_private_file(path)?;
    Ok((tail.len() as u64, true))
}

fn read_log_tail(path: &Path, limit: usize) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return String::new();
    };
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    let _ = file.seek(SeekFrom::Start(metadata.len().saturating_sub(limit as u64)));
    let mut buffer = vec![0u8; length.min(limit)];
    let read = file.read(&mut buffer).unwrap_or_default();
    buffer.truncate(read);
    String::from_utf8_lossy(&buffer).trim().to_owned()
}

fn summarize_output(stderr: Option<&str>, stdout: Option<&str>) -> String {
    let text = stderr.filter(|value| !value.trim().is_empty()).or(stdout);
    text.map(|value| value.trim().chars().take(1000).collect())
        .unwrap_or_default()
}

fn create_private_file(path: &Path) -> Result<fs::File, BrowserError> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), BrowserError> {
    let mut file = create_private_file(path)?;
    use std::io::Write;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<(), BrowserError> {
    match fs::create_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(BrowserError::Io(error)),
    }
    set_private_directory(path)?;
    Ok(())
}

fn set_private_directory(path: &Path) -> Result<(), BrowserError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private_file(path: &Path) -> Result<(), BrowserError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn is_empty_directory(path: &Path) -> bool {
    path.read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

fn remove_dir_all(path: &Path) -> bool {
    match fs::remove_dir_all(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

fn base_path() -> OsString {
    std::env::var_os("PATH")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("/usr/local/bin:/usr/bin:/bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_endpoint_rejects_non_loopback_and_foreign_parts() {
        assert_eq!(
            LoopbackEndpoint::parse("http://127.0.0.1:4173").unwrap(),
            LoopbackEndpoint {
                host: "127.0.0.1".into(),
                port: 4173
            }
        );
        for invalid in [
            "https://127.0.0.1:4173",
            "http://localhost:4173",
            "http://127.0.0.2:4173",
            "http://127.0.0.1:0",
            "http://user@127.0.0.1:4173",
            "http://127.0.0.1:4173/path",
            "http://127.0.0.1:4173?query=1",
        ] {
            assert!(
                LoopbackEndpoint::parse(invalid).is_err(),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn url_paths_reject_traversal_and_query_fragments() {
        assert!(validate_url_path("/").is_ok());
        assert!(validate_url_path("/health").is_ok());
        for invalid in [
            "",
            "health",
            "/../etc/passwd",
            "/a/../b",
            "/x?y",
            "/x#y",
            "/a\\b",
        ] {
            assert!(validate_url_path(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn state_ids_are_bounded_slugs() {
        assert!(validate_state_id("run-123_abc").is_ok());
        for invalid in ["", "../x", "a/b", "a b", &"x".repeat(65)] {
            assert!(validate_state_id(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn log_cap_retains_only_the_tail() {
        let root = std::env::temp_dir().join(format!("fbc-{}", Uuid::new_v4().simple()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("log");
        std::fs::write(&path, b"0123456789").unwrap();
        let (bytes, truncated) = cap_log_file(&path, 4).unwrap();
        assert!(truncated);
        assert_eq!(bytes, 4);
        assert_eq!(std::fs::read(&path).unwrap(), b"6789");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn logs_inside_the_workspace_are_rejected_before_creation() {
        let root = std::env::temp_dir().join(format!("fbc-{}", Uuid::new_v4().simple()));
        let workspace = root.join("repo");
        std::fs::create_dir_all(&workspace).unwrap();
        let log_dir = workspace.join("logs");
        let result = ManagedTestServer::start(ManagedTestServerConfig {
            argv: vec!["/bin/true".into()],
            cwd: workspace,
            endpoint: None,
            ready_path: "/".into(),
            startup_timeout: Duration::from_secs(5),
            log_dir: log_dir.clone(),
            max_log_bytes: 4096,
        })
        .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("in-repository log directory was accepted"),
        };
        assert!(
            matches!(error, BrowserError::Invalid(_)),
            "unexpected browser error: {error:?}"
        );
        assert!(!log_dir.exists(), "in-repository log directory was created");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn own_process_stat_is_readable() {
        let pid = std::process::id();
        let stat = read_proc_stat(pid).expect("current process stat");
        assert!(stat.start_time > 0);
        assert_eq!(stat.process_group, unsafe { libc::getpgrp() });
        assert!(leader_alive(pid, stat.start_time));
        assert!(!leader_alive(pid, stat.start_time.saturating_add(1)));
    }
}
