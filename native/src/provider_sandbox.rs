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
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UnixStream},
    process::{Child, Command},
    task::JoinHandle,
    time,
};

const MAX_REQUEST_BYTES: usize = 32 * 1024;
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("provider sandbox configuration is invalid: {0}")]
    Invalid(String),
    #[error("provider sandbox I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider sandbox did not become ready: {0}")]
    NotReady(String),
}

#[derive(Debug, Clone)]
pub struct ProviderSandboxConfig {
    pub binary: PathBuf,
    pub session_dir: PathBuf,
    pub args: Vec<String>,
    pub ready_timeout: Duration,
}

pub struct ProviderSandbox {
    child: Child,
    session_dir: PathBuf,
    base_url: String,
    password: String,
    bridge: JoinHandle<()>,
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
        let mut command = Command::new(bwrap_binary());
        command
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
            .arg("--bind")
            .arg(&config.session_dir)
            .arg("/session")
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
            .arg(&password)
            .arg("/provider/provider")
            .args(&config.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(
                config.session_dir.join("sandbox.stderr"),
            )?))
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let deadline = time::Instant::now() + config.ready_timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                return Err(SandboxError::NotReady(format!(
                    "provider process exited before readiness: {status}"
                )));
            }
            if UnixStream::connect(&socket).await.is_ok() {
                break;
            }
            if time::Instant::now() >= deadline {
                let _ = child.kill().await;
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
        let bridge = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let expected = expected.clone();
                let socket = socket_for_bridge.clone();
                tokio::spawn(async move {
                    let _ = bridge_connection(stream, &socket, &expected).await;
                });
            }
        });
        Ok(Self {
            child,
            session_dir: config.session_dir,
            base_url: format!("http://{address}"),
            password,
            bridge,
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

    pub async fn stop(mut self) -> Result<(), SandboxError> {
        self.bridge.abort();
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

impl Drop for ProviderSandbox {
    fn drop(&mut self) {
        self.bridge.abort();
    }
}

async fn bridge_connection(
    mut stream: TcpStream,
    socket: &Path,
    expected: &str,
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
    tokio::io::copy_bidirectional(&mut stream, &mut upstream).await?;
    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_REQUEST_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "provider bridge request is too large",
            ));
        }
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(buffer)
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
    Ok(
        matches!(path, "/event" | "/global/health" | "/doc" | "/session")
            || path.starts_with("/session/"),
    )
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
