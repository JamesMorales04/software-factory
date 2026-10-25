//! Confined provider process and loopback credential broker.
//!
//! The provider runs in a bubblewrap user/pid/net/ipc/uts/cgroup namespace with
//! an empty HOME, a read-only system image, only its private session directory
//! writable and no host repository, host HOME, runtime socket or Core token.
//! The Worker reaches it through a loopback bridge that requires a per-run
//! Basic-auth capability. The long-lived provider credential is never placed in
//! the sandbox.
//!
//! This is a candidate boundary. It is not certified isolation and does not
//! prove kernel or same-UID separation.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::provider_egress::{EgressBroker, EgressConfig};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UnixStream},
    process::{Child, Command},
    task::JoinHandle,
    time,
};

const MAX_REQUEST_BYTES: usize = 32 * 1024;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(10);
const MONITOR_INTERVAL: Duration = Duration::from_millis(200);
const MAX_STDERR_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxProcessState {
    Alive,
    Gone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxIdentity {
    pid: u32,
    start_time_ticks: u64,
    boot_id: String,
    socket: String,
    created_at_ms: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("provider sandbox configuration is invalid: {0}")]
    Invalid(String),
    #[error("provider sandbox I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider sandbox did not become ready: {0}")]
    NotReady(String),
    #[error("provider sandbox exceeded its resource limit")]
    LimitExceeded,
}

#[derive(Debug, Clone)]
pub struct ProviderSandboxConfig {
    pub binary: PathBuf,
    pub session_dir: PathBuf,
    pub args: Vec<String>,
    pub ready_timeout: Duration,
    pub max_session_bytes: u64,
    pub max_file_bytes: u64,
    pub max_files: usize,
    pub max_lifetime: Duration,
    pub bridge_timeout: Duration,
    pub max_connections: usize,
    pub cpu_seconds: u64,
    pub address_space_bytes: u64,
    pub env: Vec<(String, String)>,
    pub egress: Option<EgressConfig>,
    pub egress_bridge: Option<PathBuf>,
}

impl Default for ProviderSandboxConfig {
    fn default() -> Self {
        Self {
            binary: PathBuf::new(),
            session_dir: PathBuf::new(),
            args: Vec::new(),
            ready_timeout: Duration::from_secs(15),
            max_session_bytes: 64 * 1024 * 1024,
            max_file_bytes: 64 * 1024 * 1024,
            max_files: 1024,
            max_lifetime: Duration::from_secs(300),
            bridge_timeout: Duration::from_secs(120),
            max_connections: 16,
            cpu_seconds: 300,
            address_space_bytes: 1024 * 1024 * 1024,
            env: Vec::new(),
            egress: None,
            egress_bridge: None,
        }
    }
}

pub struct ProviderSandbox {
    child: Child,
    session_dir: PathBuf,
    base_url: String,
    password: String,
    bridge: JoinHandle<()>,
    monitor: JoinHandle<()>,
    stop_monitor: Arc<AtomicBool>,
    limit_exceeded: Arc<AtomicBool>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    egress: Option<EgressBroker>,
}

impl ProviderSandbox {
    pub async fn start(config: ProviderSandboxConfig) -> Result<Self, SandboxError> {
        if !config.binary.is_absolute() || !config.binary.is_file() {
            return Err(SandboxError::Invalid(
                "provider binary must be an absolute file".into(),
            ));
        }
        if !config.session_dir.is_absolute() {
            return Err(SandboxError::Invalid(
                "provider session directory must be absolute".into(),
            ));
        }
        std::fs::create_dir_all(&config.session_dir)?;
        set_private_directory(&config.session_dir)?;
        let socket = config.session_dir.join("provider.sock");
        let _ = std::fs::remove_file(&socket);
        let password = format!(
            "{}.{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let egress = if let Some(egress_config) = &config.egress {
            let bridge = config
                .egress_bridge
                .as_ref()
                .ok_or_else(|| SandboxError::Invalid("egress bridge binary is required".into()))?;
            if !bridge.is_absolute() || !bridge.is_file() {
                return Err(SandboxError::Invalid(
                    "egress bridge binary must be an absolute file".into(),
                ));
            }
            Some(
                EgressBroker::start(
                    egress_config.clone(),
                    &config.session_dir.join("egress.sock"),
                )
                .await
                .map_err(|error| SandboxError::Invalid(error.to_string()))?,
            )
        } else {
            None
        };
        let mut command = Command::new(prlimit_binary());
        command
            .arg(format!("--fsize={}", config.max_file_bytes))
            .arg(format!("--cpu={}", config.cpu_seconds))
            .arg(format!("--as={}", config.address_space_bytes))
            .arg("--nofile=256")
            .arg(bwrap_binary())
            .arg("--unshare-all")
            .arg("--unshare-user")
            .arg("--die-with-parent")
            .arg("--new-session")
            .arg("--disable-userns")
            .arg("--cap-drop")
            .arg("ALL")
            .arg("--clearenv")
            .arg("--ro-bind")
            .arg("/usr")
            .arg("/usr")
            .arg("--ro-bind-try")
            .arg("/bin")
            .arg("/bin")
            .arg("--ro-bind-try")
            .arg("/lib")
            .arg("/lib")
            .arg("--ro-bind-try")
            .arg("/lib64")
            .arg("/lib64")
            .arg("--ro-bind-try")
            .arg("/etc/ssl")
            .arg("/etc/ssl")
            .arg("--dir")
            .arg("/provider")
            .arg("--ro-bind")
            .arg(&config.binary)
            .arg("/provider/provider")
            .arg("--ro-bind")
            .arg(
                config
                    .egress_bridge
                    .as_deref()
                    .unwrap_or(Path::new("/dev/null")),
            )
            .arg("/provider/egress-bridge")
            .arg("--bind")
            .arg(&config.session_dir)
            .arg("/session")
            .arg("--size")
            .arg((64 * 1024 * 1024).to_string())
            .arg("--tmpfs")
            .arg("/tmp")
            .arg("--proc")
            .arg("/proc")
            .arg("--dev")
            .arg("/dev")
            .arg("--chdir")
            .arg("/session")
            .arg("--setenv")
            .arg("PATH")
            .arg("/usr/bin:/bin")
            .arg("--setenv")
            .arg("HOME")
            .arg("/tmp")
            .arg("--setenv")
            .arg("TMPDIR")
            .arg("/tmp")
            .arg("--setenv")
            .arg("FACTORY_PROVIDER_SOCKET")
            .arg("/session/provider.sock")
            .arg("--setenv")
            .arg("OPENCODE_SERVER_PASSWORD")
            .arg(&password);
        for (key, value) in &config.env {
            command.arg("--setenv").arg(key).arg(value);
        }
        if let Some(egress) = &config.egress {
            command
                .arg("--setenv")
                .arg("FACTORY_EGRESS_LISTEN_PORT")
                .arg("18080")
                .arg("--setenv")
                .arg("FACTORY_EGRESS_SOCKET")
                .arg("/session/egress.sock")
                .arg("--setenv")
                .arg("OPENAI_BASE_URL")
                .arg("http://127.0.0.1:18080/v1")
                .arg("--setenv")
                .arg("OPENAI_API_KEY")
                .arg(egress.token());
        }
        if config.egress.is_some() {
            command
                .arg("/bin/sh")
                .arg("-c")
                .arg("/provider/egress-bridge & exec /provider/provider \"$@\"")
                .arg("factory-provider")
                .args(&config.args);
        } else {
            command.arg("/provider/provider").args(&config.args);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let pid = child.id().unwrap_or_default();
        write_sandbox_identity(&config.session_dir, pid, &socket)?;
        let stderr = child.stderr.take().map(|stderr| {
            tokio::spawn(async move {
                let mut stderr = stderr;
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    match stderr.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if buffer.len() < MAX_STDERR_BYTES {
                                let remaining = MAX_STDERR_BYTES - buffer.len();
                                buffer.extend_from_slice(&chunk[..read.min(remaining)]);
                            }
                        }
                    }
                }
                buffer
            })
        });
        let deadline = time::Instant::now() + config.ready_timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                if let Some(stderr) = stderr {
                    stderr.abort();
                }
                return Err(SandboxError::NotReady(format!(
                    "provider process exited before readiness: {status}"
                )));
            }
            if UnixStream::connect(&socket).await.is_ok() {
                break;
            }
            if time::Instant::now() >= deadline {
                let _ = child.kill().await;
                if let Some(stderr) = stderr {
                    stderr.abort();
                }
                return Err(SandboxError::NotReady(
                    "provider socket did not appear".into(),
                ));
            }
            time::sleep(Duration::from_millis(50)).await;
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let expected = basic_authorization(&password);
        let socket_for_bridge = socket.clone();
        let max_connections = config.max_connections.max(1);
        let bridge_timeout = config.bridge_timeout;
        let bridge = tokio::spawn(async move {
            let permits = Arc::new(tokio::sync::Semaphore::new(max_connections));
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    let _ = stream;
                    continue;
                };
                let expected = expected.clone();
                let socket = socket_for_bridge.clone();
                tokio::spawn(async move {
                    let _ = bridge_connection(stream, &socket, &expected, bridge_timeout).await;
                    drop(permit);
                });
            }
        });
        let stop_monitor = Arc::new(AtomicBool::new(false));
        let limit_exceeded = Arc::new(AtomicBool::new(false));
        let monitor = tokio::spawn({
            let stop = stop_monitor.clone();
            let exceeded = limit_exceeded.clone();
            let directory = config.session_dir.clone();
            let max_bytes = config.max_session_bytes;
            let max_files = config.max_files;
            let max_lifetime = config.max_lifetime;
            async move {
                let started = time::Instant::now();
                while !stop.load(Ordering::SeqCst) {
                    time::sleep(MONITOR_INTERVAL).await;
                    let exceeded_now = started.elapsed() > max_lifetime
                        || directory_usage(&directory, max_files)
                            .map(|(bytes, _)| bytes > max_bytes)
                            .unwrap_or(true);
                    if exceeded_now {
                        exceeded.store(true, Ordering::SeqCst);
                        #[cfg(unix)]
                        unsafe {
                            libc::kill(pid as i32, libc::SIGKILL);
                        }
                        return;
                    }
                }
            }
        });
        Ok(Self {
            child,
            session_dir: config.session_dir,
            base_url: format!("http://{address}"),
            password,
            bridge,
            monitor,
            stop_monitor,
            limit_exceeded,
            stderr,
            egress,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn limit_exceeded(&self) -> bool {
        self.limit_exceeded.load(Ordering::SeqCst)
    }

    pub async fn stop(mut self) -> Result<(), SandboxError> {
        self.stop_monitor.store(true, Ordering::SeqCst);
        self.monitor.abort();
        self.bridge.abort();
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.await;
        }
        if let Some(egress) = self.egress.take() {
            egress.stop();
        }
        if self.limit_exceeded() {
            return Err(SandboxError::LimitExceeded);
        }
        Ok(())
    }
}

pub fn verify_sandbox_identity(session_dir: &Path) -> Result<SandboxProcessState, SandboxError> {
    let path = session_dir.join("sandbox.json");
    let bytes = std::fs::read(&path)
        .map_err(|_| SandboxError::NotReady("sandbox identity is absent".into()))?;
    let identity: SandboxIdentity = serde_json::from_slice(&bytes)
        .map_err(|_| SandboxError::Invalid("sandbox identity is malformed".into()))?;
    let boot_id = read_trimmed(Path::new("/proc/sys/kernel/random/boot_id"))?;
    if boot_id != identity.boot_id {
        return Ok(SandboxProcessState::Gone);
    }
    let Some(start_time) = process_start_time(identity.pid)? else {
        return Ok(SandboxProcessState::Gone);
    };
    if start_time != identity.start_time_ticks || !process_is_alive(identity.pid)? {
        return Ok(SandboxProcessState::Gone);
    }
    Ok(SandboxProcessState::Alive)
}

pub fn confined_recovery_receipt(
    directory: &str,
    provider_session_id: &str,
    root: &Path,
) -> Result<&'static str, String> {
    let root = root
        .canonicalize()
        .map_err(|_| "provider session root is unavailable")?;
    if directory.is_empty() {
        return if provider_session_id.is_empty() {
            Ok("no_external_session")
        } else {
            Err("confined session has an external id but no directory".into())
        };
    }
    let directory = Path::new(directory)
        .canonicalize()
        .map_err(|_| "provider session directory is unavailable")?;
    if !directory.starts_with(&root) {
        return Err("provider session directory is outside the configured root".into());
    }
    match verify_sandbox_identity(&directory) {
        Ok(SandboxProcessState::Gone) => Ok("sandbox_process_identity_absent"),
        Ok(SandboxProcessState::Alive) => {
            terminate_sandbox_identity(&directory).map_err(|error| error.to_string())?;
            Ok("sandbox_process_terminated_verified")
        }
        Err(_) if provider_session_id.is_empty() => Ok("no_external_session"),
        Err(error) => Err(error.to_string()),
    }
}

pub fn terminate_sandbox_identity(session_dir: &Path) -> Result<SandboxProcessState, SandboxError> {
    if verify_sandbox_identity(session_dir)? == SandboxProcessState::Gone {
        return Ok(SandboxProcessState::Gone);
    }
    let bytes = std::fs::read(session_dir.join("sandbox.json"))?;
    let identity: SandboxIdentity = serde_json::from_slice(&bytes)
        .map_err(|_| SandboxError::Invalid("sandbox identity is malformed".into()))?;
    #[cfg(unix)]
    unsafe {
        if libc::kill(identity.pid as i32, libc::SIGTERM) != 0 {
            return Ok(SandboxProcessState::Gone);
        }
    }
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if verify_sandbox_identity(session_dir)? == SandboxProcessState::Gone {
            return Ok(SandboxProcessState::Gone);
        }
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(identity.pid as i32, libc::SIGKILL);
    }
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if verify_sandbox_identity(session_dir)? == SandboxProcessState::Gone {
            return Ok(SandboxProcessState::Gone);
        }
    }
    Err(SandboxError::NotReady(
        "sandbox process identity did not terminate".into(),
    ))
}

fn write_sandbox_identity(session_dir: &Path, pid: u32, socket: &Path) -> Result<(), SandboxError> {
    if pid == 0 {
        return Err(SandboxError::Invalid("provider process has no pid".into()));
    }
    let start_time_ticks = process_start_time(pid)?
        .ok_or_else(|| SandboxError::NotReady("provider process disappeared".into()))?;
    let boot_id = read_trimmed(Path::new("/proc/sys/kernel/random/boot_id"))?;
    let identity = SandboxIdentity {
        pid,
        start_time_ticks,
        boot_id,
        socket: socket.to_string_lossy().into_owned(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
    };
    let path = session_dir.join("sandbox.json");
    let bytes = serde_json::to_vec(&identity)
        .map_err(|_| SandboxError::Invalid("sandbox identity encoding failed".into()))?;
    std::fs::write(&path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn process_start_time(pid: u32) -> Result<Option<u64>, SandboxError> {
    let path = PathBuf::from(format!("/proc/{pid}/stat"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let Some(rest) = text.rsplit_once(')').map(|(_, rest)| rest) else {
        return Ok(None);
    };
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let Some(start) = fields.get(19) else {
        return Ok(None);
    };
    start
        .parse()
        .map(Some)
        .map_err(|_| SandboxError::Invalid("provider process start time is malformed".into()))
}

fn process_is_alive(pid: u32) -> Result<bool, SandboxError> {
    let path = PathBuf::from(format!("/proc/{pid}/stat"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(false);
    };
    let Some(rest) = text.rsplit_once(')').map(|(_, rest)| rest) else {
        return Ok(false);
    };
    let Some(state) = rest.split_whitespace().next() else {
        return Ok(false);
    };
    Ok(!matches!(state, "Z" | "X" | "x"))
}

fn read_trimmed(path: &Path) -> Result<String, SandboxError> {
    Ok(std::fs::read_to_string(path)?.trim().to_owned())
}

impl Drop for ProviderSandbox {
    fn drop(&mut self) {
        self.stop_monitor.store(true, Ordering::SeqCst);
        self.monitor.abort();
        self.bridge.abort();
        if let Some(egress) = self.egress.take() {
            egress.stop();
        }
    }
}

async fn bridge_connection(
    mut stream: TcpStream,
    socket: &Path,
    expected: &str,
    bridge_timeout: Duration,
) -> Result<(), std::io::Error> {
    let request = time::timeout(BRIDGE_TIMEOUT, read_request(&mut stream))
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "request headers timed out")
        })??;
    if !request_is_allowed(&request)? {
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }
    if !request_authorized(&request, expected) {
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
        return Ok(());
    }
    let mut upstream = UnixStream::connect(socket).await?;
    upstream.write_all(&request).await?;
    time::timeout(
        bridge_timeout,
        tokio::io::copy_bidirectional(&mut stream, &mut upstream),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "bridge timed out"))??;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "request headers ended early",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_REQUEST_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider bridge request is too large",
            ));
        }
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = std::str::from_utf8(&buffer[..header_end]).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "headers are not UTF-8")
    })?;
    let mut content_length = 0usize;
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "chunked request framing is not allowed",
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid content length")
            })?;
            if content_length > MAX_BODY_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "provider bridge body is too large",
                ));
            }
        }
    }
    while buffer.len() < header_end + content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "request body ended early",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > header_end + content_length {
            buffer.truncate(header_end + content_length);
        }
    }
    Ok(force_connection_close(&buffer))
}

fn force_connection_close(request: &[u8]) -> Vec<u8> {
    let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
        return request.to_vec();
    };
    let (headers, body) = request.split_at(position + 4);
    let text = String::from_utf8_lossy(&headers[..position]);
    let mut output = Vec::with_capacity(request.len() + 32);
    for line in text.split("\r\n") {
        if line
            .split_once(':')
            .is_some_and(|(name, _)| name.eq_ignore_ascii_case("connection"))
        {
            continue;
        }
        output.extend_from_slice(line.as_bytes());
        output.extend_from_slice(b"\r\n");
    }
    output.extend_from_slice(b"Connection: close\r\n\r\n");
    output.extend_from_slice(body);
    output
}

fn request_is_allowed(request: &[u8]) -> Result<bool, std::io::Error> {
    let text = std::str::from_utf8(request).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "request is not UTF-8")
    })?;
    let Some(line) = text.lines().next() else {
        return Ok(false);
    };
    let mut parts = line.split_whitespace();
    let Some(method) = parts.next() else {
        return Ok(false);
    };
    let Some(target) = parts.next() else {
        return Ok(false);
    };
    if !matches!(method, "GET" | "POST" | "DELETE") || !target.starts_with('/') {
        return Ok(false);
    }
    let path = target.split('?').next().unwrap_or(target);
    let allowed = matches!(
        path,
        "/event" | "/global/health" | "/doc" | "/session" | "/session/status"
    ) || path.strip_prefix("/session/").is_some_and(|suffix| {
        let mut parts = suffix.split('/');
        let Some(id) = parts.next() else {
            return false;
        };
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return false;
        }
        match parts.next() {
            None => parts.next().is_none(),
            Some("prompt_async" | "message" | "abort") => parts.next().is_none(),
            Some(_) => false,
        }
    });
    Ok(allowed)
}

fn request_authorized(request: &[u8], expected: &str) -> bool {
    let Ok(text) = std::str::from_utf8(request) else {
        return false;
    };
    text.lines()
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization")
                && constant_time_eq(value.trim().as_bytes(), expected.as_bytes())
        })
}

fn basic_authorization(password: &str) -> String {
    format!(
        "Basic {}",
        base64(format!("opencode:{password}").as_bytes())
    )
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(TABLE[(first >> 2) as usize] as char);
        output.push(TABLE[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(third & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn prlimit_binary() -> PathBuf {
    std::env::var_os("FACTORY_PRLIMIT_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("prlimit"))
}

fn directory_usage(root: &Path, max_entries: usize) -> Result<(u64, usize), std::io::Error> {
    let mut stack = vec![root.to_path_buf()];
    let mut bytes = 0u64;
    let mut entries = 0usize;
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            entries = entries.saturating_add(1);
            if entries > max_entries {
                return Ok((u64::MAX, entries));
            }
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok((bytes, entries))
}

fn bwrap_binary() -> PathBuf {
    std::env::var_os("FACTORY_BWRAP_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bwrap"))
}

fn set_private_directory(path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bwrap_available() -> bool {
        std::process::Command::new("bwrap")
            .args([
                "--unshare-all",
                "--die-with-parent",
                "--ro-bind",
                "/usr",
                "/usr",
                "--ro-bind-try",
                "/bin",
                "/bin",
                "--ro-bind-try",
                "/lib",
                "/lib",
                "--ro-bind-try",
                "/lib64",
                "/lib64",
                "--proc",
                "/proc",
                "/bin/true",
            ])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn bridge_rejects_unbounded_paths_and_forces_connection_close() {
        assert!(request_is_allowed(b"GET /session/ses_1/message HTTP/1.1\r\n\r\n").unwrap());
        assert!(request_is_allowed(b"POST /session/ses_1/prompt_async HTTP/1.1\r\n\r\n").unwrap());
        assert!(!request_is_allowed(b"GET /session/ses_1/shell HTTP/1.1\r\n\r\n").unwrap());
        assert!(!request_is_allowed(b"POST /v1/workspaces HTTP/1.1\r\n\r\n").unwrap());
        assert!(!request_is_allowed(b"GET /session/../etc HTTP/1.1\r\n\r\n").unwrap());
        let request =
            b"GET /global/health HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
        let closed = force_connection_close(request);
        let text = String::from_utf8(closed).unwrap();
        assert!(!text.to_ascii_lowercase().contains("connection: keep-alive"));
        assert_eq!(text.matches("Connection: close").count(), 1);
    }

    #[test]
    fn base64_matches_rfc_examples() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[tokio::test]
    async fn real_bwrap_hides_host_home_repositories_sockets_and_network() {
        if !bwrap_available() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "factory-bwrap-probe-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let repository = root.join("repo");
        std::fs::create_dir(&repository).unwrap();
        let host_home = std::env::var("HOME").unwrap_or_else(|_| "/home".into());
        let host_socket = format!(
            "/run/user/{}/podman/podman.sock",
            std::process::Command::new("id")
                .arg("-u")
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|value| value.trim().to_owned())
                .unwrap_or_else(|| "1000".into())
        );
        let script = format!(
            "test ! -e {host_home} && test ! -e {} && test ! -e {host_socket} && test ! -e /run/user && test -z \"$FACTORY_PROVIDER_PASSWORD\" && test -z \"$FACTORY_OPERATOR_TOKEN\"",
            repository.display()
        );
        let output = Command::new("bwrap")
            .args([
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
                "--clearenv",
                "--ro-bind",
                "/usr",
                "/usr",
                "--ro-bind-try",
                "/bin",
                "/bin",
                "--ro-bind-try",
                "/lib",
                "/lib",
                "--ro-bind-try",
                "/lib64",
                "/lib64",
                "--tmpfs",
                "/tmp",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--setenv",
                "PATH",
                "/usr/bin:/bin",
                "--chdir",
                "/tmp",
                "/bin/sh",
                "-c",
                &script,
            ])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "bwrap probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let network = Command::new("bwrap")
            .args([
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
                "--clearenv",
                "--ro-bind",
                "/usr",
                "/usr",
                "--ro-bind-try",
                "/bin",
                "/bin",
                "--ro-bind-try",
                "/lib",
                "/lib",
                "--ro-bind-try",
                "/lib64",
                "/lib64",
                "--tmpfs",
                "/tmp",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--setenv",
                "PATH",
                "/usr/bin:/bin",
                "--chdir",
                "/tmp",
                "/bin/bash",
                "-c",
                "exec 3<>/dev/tcp/1.1.1.1/80",
            ])
            .output()
            .await
            .unwrap();
        assert!(
            !network.status.success(),
            "sandbox unexpectedly reached external network"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
