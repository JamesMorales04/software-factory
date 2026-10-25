//! Narrow OpenAI-compatible provider egress broker.
//!
//! The confined provider has no network namespace. A trusted broker accepts
//! requests over a bind-mounted Unix socket, validates a per-run bearer token,
//! restricts method/path/size, injects the long-lived upstream credential and
//! connects only to one pinned, public upstream address. Redirects are refused.

use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use reqwest::redirect::Policy;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UnixListener, UnixStream},
    task::JoinHandle,
    time,
};

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_REQUEST_BODY: usize = 2 * 1024 * 1024;
const MAX_RESPONSE_BODY: usize = 16 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum EgressError {
    #[error("provider egress configuration is invalid: {0}")]
    Invalid(String),
    #[error("provider egress I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider egress upstream failed: {0}")]
    Upstream(String),
}

#[derive(Clone)]
pub struct EgressConfig {
    host: String,
    scheme: String,
    address: SocketAddr,
    credential: String,
    token: String,
    timeout: Duration,
    max_requests: usize,
    allowed_model: Option<String>,
}

impl std::fmt::Debug for EgressConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EgressConfig")
            .field("host", &self.host)
            .field("address", &self.address)
            .field("credential", &"<redacted>")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl EgressConfig {
    pub fn new(
        upstream: &str,
        credential: String,
        token: String,
        allow_test_upstream: bool,
    ) -> Result<Self, EgressError> {
        let url = reqwest::Url::parse(upstream)
            .map_err(|_| EgressError::Invalid("upstream URL is invalid".into()))?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.path(), "" | "/" | "/v1")
        {
            return Err(EgressError::Invalid(
                "upstream must be an origin with an optional /v1 path".into(),
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| EgressError::Invalid("upstream host is missing".into()))?
            .to_owned();
        if !allow_test_upstream && url.scheme() != "https" {
            return Err(EgressError::Invalid(
                "upstream must use HTTPS outside tests".into(),
            ));
        }
        if !matches!(url.scheme(), "https" | "http") {
            return Err(EgressError::Invalid(
                "upstream scheme is not allowed".into(),
            ));
        }
        if credential.len() < 16 || token.len() < 32 {
            return Err(EgressError::Invalid(
                "egress credential and per-run token are too short".into(),
            ));
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| EgressError::Invalid("upstream port is missing".into()))?;
        let address = resolve_public(&host, port, allow_test_upstream)?;
        Ok(Self {
            host,
            scheme: url.scheme().to_owned(),
            address,
            credential,
            token,
            timeout: Duration::from_secs(120),
            max_requests: 8,
            allowed_model: None,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.allowed_model = Some(model);
        self
    }

    pub fn with_max_requests(mut self, max_requests: usize) -> Self {
        self.max_requests = max_requests.max(1);
        self
    }
}

pub struct EgressBroker {
    socket: PathBuf,
    task: JoinHandle<()>,
}

impl EgressBroker {
    pub async fn start(config: EgressConfig, socket: &Path) -> Result<Self, EgressError> {
        let _ = std::fs::remove_file(socket);
        let listener = UnixListener::bind(socket)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
        }
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let task = tokio::spawn({
            async move {
                let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let Ok(permit) = permits.clone().try_acquire_owned() else {
                        continue;
                    };
                    let config = config.clone();
                    let requests = requests.clone();
                    tokio::spawn(async move {
                        let _ = handle(stream, &config, &requests).await;
                        drop(permit);
                    });
                }
            }
        });
        Ok(Self {
            socket: socket.to_path_buf(),
            task,
        })
    }

    pub fn stop(self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.socket);
    }
}

impl Drop for EgressBroker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn handle(
    mut stream: UnixStream,
    config: &EgressConfig,
    requests: &std::sync::atomic::AtomicUsize,
) -> Result<(), EgressError> {
    let request = match time::timeout(Duration::from_secs(10), read_request(&mut stream)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            write_status(&mut stream, 408, "request timed out").await?;
            return Ok(());
        }
    };
    let (headers, body) = split_request(&request)?;
    if !authorized(&headers, &config.token) {
        write_status(&mut stream, 401, "unauthorized").await?;
        return Ok(());
    }
    let Some(line) = headers.lines().next() else {
        write_status(&mut stream, 400, "bad request").await?;
        return Ok(());
    };
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if !matches!(method, "GET" | "POST") || !target.starts_with('/') {
        write_status(&mut stream, 403, "method or path is not allowed").await?;
        return Ok(());
    }
    let path = target.split('?').next().unwrap_or(target);
    if !matches!(
        path,
        "/v1/responses" | "/v1/chat/completions" | "/v1/models"
    ) {
        write_status(&mut stream, 403, "path is not allowed").await?;
        return Ok(());
    }
    if method == "GET" && path != "/v1/models" {
        write_status(&mut stream, 403, "method is not allowed").await?;
        return Ok(());
    }
    if body.len() > MAX_REQUEST_BODY {
        write_status(&mut stream, 413, "request is too large").await?;
        return Ok(());
    }
    let previous = requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if previous >= config.max_requests {
        write_status(&mut stream, 429, "request budget exhausted").await?;
        return Ok(());
    }
    if let Some(model) = &config.allowed_model {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) else {
            write_status(&mut stream, 400, "request body is not JSON").await?;
            return Ok(());
        };
        if value.get("model").and_then(serde_json::Value::as_str) != Some(model.as_str()) {
            write_status(&mut stream, 403, "model is not authorized").await?;
            return Ok(());
        }
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(config.timeout)
        .redirect(Policy::none())
        .resolve(&config.host, config.address)
        .build()
        .map_err(|error| EgressError::Upstream(error.to_string()))?;
    let url = format!("{}{}", upstream_origin(config), path);
    let mut request = client.request(
        reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| EgressError::Invalid("method is invalid".into()))?,
        &url,
    );
    request = request.header("authorization", format!("Bearer {}", config.credential));
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-type") || name.eq_ignore_ascii_case("accept") {
            request = request.header(name.trim(), value.trim());
        }
    }
    if !body.is_empty() {
        request = request.body(body);
    }
    let response = request
        .send()
        .await
        .map_err(|error| EgressError::Upstream(error.to_string()))?;
    let status = response.status();
    if status.is_redirection() {
        write_status(&mut stream, 502, "upstream redirect refused").await?;
        return Ok(());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_owned();
    stream
        .write_all(
            format!(
                "HTTP/1.1 {}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                status.as_u16()
            )
            .as_bytes(),
        )
        .await?;
    let mut response = response;
    let mut total = 0usize;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| EgressError::Upstream(error.to_string()))?
    {
        total = total.saturating_add(chunk.len());
        if total > MAX_RESPONSE_BODY {
            return Ok(());
        }
        stream
            .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
            .await?;
        stream.write_all(&chunk).await?;
        stream.write_all(b"\r\n").await?;
    }
    stream.write_all(b"0\r\n\r\n").await?;
    Ok(())
}

fn upstream_origin(config: &EgressConfig) -> String {
    format!(
        "{}://{}:{}",
        config.scheme,
        config.host,
        config.address.port()
    )
}

async fn read_request(stream: &mut UnixStream) -> Result<Vec<u8>, EgressError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(EgressError::Invalid("request ended early".into()));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(EgressError::Invalid("request headers are too large".into()));
        }
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let mut content_length = 0usize;
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(EgressError::Invalid(
                "chunked requests are not allowed".into(),
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value
                .trim()
                .parse()
                .map_err(|_| EgressError::Invalid("content length is invalid".into()))?;
            if content_length > MAX_REQUEST_BODY {
                return Err(EgressError::Invalid("request body is too large".into()));
            }
        }
    }
    while buffer.len() < header_end + content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(EgressError::Invalid("request body ended early".into()));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    buffer.truncate(header_end + content_length);
    Ok(buffer)
}

fn split_request(request: &[u8]) -> Result<(String, Vec<u8>), EgressError> {
    let position = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| EgressError::Invalid("request headers are incomplete".into()))?;
    let headers = String::from_utf8(request[..position].to_vec())
        .map_err(|_| EgressError::Invalid("request headers are not UTF-8".into()))?;
    Ok((headers, request[position + 4..].to_vec()))
}

fn authorized(headers: &str, token: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == format!("Bearer {token}")
        })
    })
}

async fn write_status(
    stream: &mut UnixStream,
    status: u16,
    message: &str,
) -> Result<(), EgressError> {
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
                message.len()
            )
            .as_bytes(),
        )
        .await?;
    Ok(())
}

fn resolve_public(
    host: &str,
    port: u16,
    allow_test_upstream: bool,
) -> Result<SocketAddr, EgressError> {
    let addresses: Vec<SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&(host, port))
        .map_err(|error| EgressError::Invalid(format!("upstream DNS failed: {error}")))?
        .collect();
    let Some(address) = addresses
        .into_iter()
        .find(|address| allow_test_upstream || is_public_ip(address.ip()))
    else {
        return Err(EgressError::Invalid(
            "upstream did not resolve to a public address".into(),
        ));
    };
    Ok(address)
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_multicast()
                || ip.is_unspecified()
                || octets[0] == 100 && (64..=127).contains(&octets[1])
                || octets[0] == 198 && (18..=19).contains(&octets[1])
                || octets[0] >= 240
                || octets[..3] == [192, 0, 0]
                || octets[..3] == [192, 0, 2]
                || octets[..3] == [198, 51, 100]
                || octets[..3] == [203, 0, 113])
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

/// Test helper: starts a loopback TCP bridge to a Unix socket. Production uses
/// the same helper binary inside the provider sandbox.
pub async fn start_loopback_bridge(
    socket: &Path,
) -> Result<(SocketAddr, JoinHandle<()>), EgressError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let socket = socket.to_path_buf();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let socket = socket.clone();
            tokio::spawn(async move {
                let Ok(mut upstream) = UnixStream::connect(socket).await else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
            });
        }
    });
    Ok((address, task))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_private_metadata_and_redirect_configuration() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("169.254.169.254".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("fc00::1".parse().unwrap()));
        assert!(!is_public_ip("100.64.0.1".parse().unwrap()));
        assert!(!is_public_ip("198.18.0.1".parse().unwrap()));
        assert!(!is_public_ip("240.0.0.1".parse().unwrap()));
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        assert!(
            EgressConfig::new(
                "http://127.0.0.1:1",
                "credential-0123456789abcdef".into(),
                "token-0123456789abcdef0123456789abcdef".into(),
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn authorizes_only_the_exact_bearer_token() {
        let token = "token-0123456789abcdef0123456789abcdef";
        let request = format!("POST /v1/responses HTTP/1.1\r\nAuthorization: Bearer {token}");
        assert!(authorized(&request, token));
        assert!(!authorized(
            "POST /v1/responses HTTP/1.1\r\nAuthorization: Bearer other",
            token
        ));
    }
}
