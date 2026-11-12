//! Deny-by-default HTTPS CONNECT proxy for dependency acquisition egress.
//!
//! The acquisition container is attached to a dedicated internal Podman
//! network with no external route. The only reachable service is this proxy on
//! the network gateway. Cargo is configured to use it through
//! `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`; every CONNECT authority is checked
//! against a fixed host allowlist and everything else is refused. The proxy
//! tunnels bytes only; it does not inspect or log payloads and carries no
//! credentials.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time,
};

pub const MAX_CONNECT_REQUEST_BYTES: usize = 8 * 1024;
pub const MAX_PROXY_CONNECTIONS: usize = 16;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const TUNNEL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum EgressProxyError {
    #[error("dependency egress allowlist is invalid: {0}")]
    InvalidAllowlist(String),
    #[error("dependency egress subnet is invalid: {0}")]
    InvalidSubnet(String),
    #[error("dependency egress proxy I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// IPv4 network allowed to reach a wildcard-bound proxy listener. Rootless
/// nested Podman creates the bridge interface only while a container is
/// attached, so the broker may need to bind before the configured gateway
/// address exists. The wildcard listener remains restricted to the dedicated
/// dependency subnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerSubnet {
    network: Ipv4Addr,
    prefix: u8,
}

impl PeerSubnet {
    pub fn parse(value: &str) -> Result<Self, EgressProxyError> {
        let (network, prefix) = value.split_once('/').ok_or_else(|| {
            EgressProxyError::InvalidSubnet(format!("{value:?} is not a CIDR subnet"))
        })?;
        let network: Ipv4Addr = network.trim().parse().map_err(|_| {
            EgressProxyError::InvalidSubnet(format!("{value:?} is not an IPv4 subnet"))
        })?;
        let prefix: u8 = prefix.trim().parse().map_err(|_| {
            EgressProxyError::InvalidSubnet(format!("{value:?} has an invalid prefix"))
        })?;
        if prefix == 0 || prefix > 32 {
            return Err(EgressProxyError::InvalidSubnet(format!(
                "{value:?} prefix must be between 1 and 32"
            )));
        }
        let mask = u32::MAX.checked_shl(u32::from(32 - prefix)).unwrap_or(0);
        if u32::from(network) & !mask != 0 {
            return Err(EgressProxyError::InvalidSubnet(format!(
                "{value:?} has host bits set"
            )));
        }
        Ok(Self { network, prefix })
    }

    pub fn contains(self, address: IpAddr) -> bool {
        let IpAddr::V4(address) = address else {
            return false;
        };
        let mask = u32::MAX
            .checked_shl(u32::from(32 - self.prefix))
            .unwrap_or(0);
        u32::from(address) & mask == u32::from(self.network) & mask
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressAllowlist {
    entries: BTreeSet<(String, u16)>,
    resolutions: BTreeMap<String, IpAddr>,
}

impl EgressAllowlist {
    pub fn new(entries: impl IntoIterator<Item = String>) -> Result<Self, EgressProxyError> {
        let mut allowlist = BTreeSet::new();
        for entry in entries {
            let entry = entry.trim();
            if entry.is_empty()
                || entry.len() > 300
                || entry
                    .chars()
                    .any(|character| character.is_control() || character.is_whitespace())
                || entry.contains('/')
                || entry.contains('@')
            {
                return Err(EgressProxyError::InvalidAllowlist(format!(
                    "invalid host entry {entry:?}"
                )));
            }
            let (host, port) = parse_authority(entry).ok_or_else(|| {
                EgressProxyError::InvalidAllowlist(format!("invalid host:port {entry:?}"))
            })?;
            allowlist.insert((host, port));
        }
        if allowlist.is_empty() {
            return Err(EgressProxyError::InvalidAllowlist(
                "at least one allowlisted destination is required".into(),
            ));
        }
        Ok(Self {
            entries: allowlist,
            resolutions: BTreeMap::new(),
        })
    }

    /// Optional pinned host-to-IP resolution. A pinned host must also be in the
    /// allowlist; this prevents a DNS answer or resolver failure from changing
    /// the effective destination. Unpinned hosts use the system resolver.
    pub fn with_resolved(
        entries: impl IntoIterator<Item = String>,
        resolutions: impl IntoIterator<Item = (String, IpAddr)>,
    ) -> Result<Self, EgressProxyError> {
        let mut allowlist = Self::new(entries)?;
        for (host, address) in resolutions {
            let host = host.trim().to_ascii_lowercase();
            if !allowlist
                .entries
                .iter()
                .any(|(allowed, _)| allowed == &host)
            {
                return Err(EgressProxyError::InvalidAllowlist(format!(
                    "pinned resolution host {host:?} is not allowlisted"
                )));
            }
            allowlist.resolutions.insert(host, address);
        }
        Ok(allowlist)
    }

    pub fn allows(&self, host: &str, port: u16) -> bool {
        self.entries
            .contains(&(host.trim().to_ascii_lowercase(), port))
    }

    pub fn resolve(&self, host: &str) -> Option<IpAddr> {
        self.resolutions
            .get(&host.trim().to_ascii_lowercase())
            .copied()
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, u16)> {
        self.entries
            .iter()
            .map(|(host, port)| (host.as_str(), *port))
    }
}

fn parse_authority(value: &str) -> Option<(String, u16)> {
    if value.is_empty() || value.len() > 300 {
        return None;
    }
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let (host, rest) = rest.split_once(']')?;
        if host.is_empty() {
            return None;
        }
        let port = rest.strip_prefix(':')?;
        (host.to_owned(), port)
    } else if let Some((host, port)) = value.rsplit_once(':') {
        if host.is_empty() || host.contains(':') {
            return None;
        }
        (host.to_owned(), port)
    } else {
        (value.to_owned(), "443")
    };
    let port = port.parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }
    let host = host.to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
    {
        return None;
    }
    Some((host, port))
}

pub struct RunningProxy {
    address: SocketAddr,
    ready: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl RunningProxy {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Shared readiness flag. It becomes false if the listener stops; the
    /// broker refuses acquisition when it is false.
    pub fn ready(&self) -> Arc<AtomicBool> {
        self.ready.clone()
    }

    pub fn abort(self) {
        self.task.abort();
    }
}

/// Bind and start the proxy. Readiness is set only after a successful bind.
pub async fn spawn(
    address: SocketAddr,
    allowlist: EgressAllowlist,
) -> Result<RunningProxy, EgressProxyError> {
    spawn_scoped(address, allowlist, None).await
}

/// Bind and start the proxy with an optional source-network restriction. The
/// restriction is required when the listener binds a wildcard address because
/// the dedicated bridge does not exist yet; connections from every other
/// network are dropped before any HTTP request is parsed.
pub async fn spawn_scoped(
    address: SocketAddr,
    allowlist: EgressAllowlist,
    peers: Option<PeerSubnet>,
) -> Result<RunningProxy, EgressProxyError> {
    let listener = TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    let ready = Arc::new(AtomicBool::new(true));
    let ready_task = ready.clone();
    let allowlist = Arc::new(allowlist);
    let task = tokio::spawn(async move {
        let permits = Arc::new(Semaphore::new(MAX_PROXY_CONNECTIONS));
        loop {
            let (socket, peer) = match listener.accept().await {
                Ok(value) => value,
                Err(_) => break,
            };
            if peers.is_some_and(|subnet| !subnet.contains(peer.ip())) {
                tracing::warn!(%peer, "dependency egress peer rejected");
                drop(socket);
                continue;
            }
            let Ok(permit) = permits.clone().try_acquire_owned() else {
                drop(socket);
                continue;
            };
            let allowlist = allowlist.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = handle_connection(socket, allowlist).await;
            });
        }
        ready_task.store(false, Ordering::SeqCst);
    });
    Ok(RunningProxy {
        address,
        ready,
        task,
    })
}

async fn handle_connection(
    mut socket: TcpStream,
    allowlist: Arc<EgressAllowlist>,
) -> Result<(), std::io::Error> {
    let request = match time::timeout(CONNECT_TIMEOUT, read_request(&mut socket)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            let _ = respond(&mut socket, "408 Request Timeout").await;
            return Ok(());
        }
    };
    if request.starts_with(b"GET /health ") {
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await?;
        socket.flush().await?;
        return Ok(());
    }
    let Some((method, authority)) = parse_connect_request(&request) else {
        let _ = respond(&mut socket, "400 Bad Request").await;
        return Ok(());
    };
    if method != "CONNECT" {
        let _ = respond(&mut socket, "405 Method Not Allowed").await;
        return Ok(());
    }
    let Some((host, port)) = parse_authority(&authority) else {
        let _ = respond(&mut socket, "400 Bad Request").await;
        return Ok(());
    };
    if !allowlist.allows(&host, port) {
        tracing::warn!(%host, port, "dependency egress denied");
        let _ = respond(&mut socket, "403 Forbidden").await;
        return Ok(());
    }
    let upstream = match time::timeout(CONNECT_TIMEOUT, async {
        match allowlist.resolve(&host) {
            Some(address) => TcpStream::connect(SocketAddr::new(address, port)).await,
            None => TcpStream::connect((host.as_str(), port)).await,
        }
    })
    .await
    {
        Ok(Ok(upstream)) => upstream,
        Ok(Err(_)) | Err(_) => {
            let _ = respond(&mut socket, "502 Bad Gateway").await;
            return Ok(());
        }
    };
    tracing::info!(%host, port, "dependency egress allowed");
    socket
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    socket.flush().await?;
    let mut upstream = upstream;
    let _ = time::timeout(
        TUNNEL_IDLE_TIMEOUT,
        tokio::io::copy_bidirectional(&mut socket, &mut upstream),
    )
    .await;
    Ok(())
}

async fn respond(socket: &mut TcpStream, status: &str) -> std::io::Result<()> {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    socket.write_all(response.as_bytes()).await?;
    socket.flush().await
}

async fn read_request(socket: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = socket.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_CONNECT_REQUEST_BYTES {
            return Err(std::io::Error::other(
                "dependency egress CONNECT request exceeded its bound",
            ));
        }
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(buffer)
}

fn parse_connect_request(request: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(request).ok()?;
    let line = text.lines().next()?.trim_end_matches('\r');
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_ascii_uppercase();
    let authority = parts.next()?.to_owned();
    let version = parts.next()?;
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return None;
    }
    Some((method, authority))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn echo_server() -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0u8; 1024];
                    loop {
                        let Ok(read) = socket.read(&mut buffer).await else {
                            break;
                        };
                        if read == 0 {
                            break;
                        }
                        if socket.write_all(&buffer[..read]).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        (address, task)
    }

    async fn connect(proxy: SocketAddr, authority: &str) -> std::io::Result<(TcpStream, String)> {
        let mut socket = TcpStream::connect(proxy).await?;
        socket
            .write_all(
                format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            let read = socket.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            response.extend_from_slice(&chunk[..read]);
            if response.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let status = String::from_utf8_lossy(&response)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        Ok((socket, status))
    }

    #[test]
    fn peer_subnet_requires_a_bounded_ipv4_network() {
        let subnet = PeerSubnet::parse("10.90.0.0/24").unwrap();
        assert!(subnet.contains("10.90.0.1".parse().unwrap()));
        assert!(subnet.contains("10.90.0.254".parse().unwrap()));
        assert!(!subnet.contains("10.90.1.1".parse().unwrap()));
        assert!(!subnet.contains("127.0.0.1".parse().unwrap()));
        assert!(!subnet.contains("::1".parse().unwrap()));

        for invalid in [
            "10.90.0.0",
            "10.90.0.0/0",
            "10.90.0.0/33",
            "10.90.0.1/24",
            "not-a-subnet/24",
            "2001:db8::/64",
        ] {
            assert!(
                PeerSubnet::parse(invalid).is_err(),
                "invalid subnet {invalid} was accepted"
            );
        }
    }

    #[tokio::test]
    async fn scoped_proxy_drops_peers_outside_the_dependency_subnet() {
        let allowlist = EgressAllowlist::new(["index.crates.io:443".into()]).unwrap();
        let proxy = spawn_scoped(
            "127.0.0.1:0".parse().unwrap(),
            allowlist,
            Some(PeerSubnet::parse("10.90.0.0/24").unwrap()),
        )
        .await
        .unwrap();
        let mut socket = TcpStream::connect(proxy.address()).await.unwrap();
        socket
            .write_all(b"GET /health HTTP/1.1\r\nHost: proxy\r\n\r\n")
            .await
            .unwrap();
        let mut buffer = [0u8; 64];
        match time::timeout(Duration::from_secs(2), socket.read(&mut buffer)).await {
            Ok(Ok(0)) => {}
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            Ok(Ok(read)) => panic!("out-of-scope peer received {read} response bytes"),
            Ok(Err(error)) => panic!("out-of-scope peer failed unexpectedly: {error}"),
            Err(_) => panic!("out-of-scope proxy connection remained open"),
        }
        proxy.abort();
    }

    #[tokio::test]
    async fn allowlisted_connect_tunnels_bytes() {
        let (echo, echo_task) = echo_server().await;
        let allowlist = EgressAllowlist::new([format!("127.0.0.1:{}", echo.port())]).unwrap();
        let proxy = spawn("127.0.0.1:0".parse().unwrap(), allowlist)
            .await
            .unwrap();
        let (mut socket, status) = connect(proxy.address(), &format!("127.0.0.1:{}", echo.port()))
            .await
            .unwrap();
        assert_eq!(status, "HTTP/1.1 200 Connection Established");
        socket.write_all(b"ping").await.unwrap();
        let mut buffer = [0u8; 4];
        socket.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"ping");
        proxy.abort();
        echo_task.abort();
    }

    #[tokio::test]
    async fn pinned_resolution_uses_the_allowlisted_address() {
        let (echo, echo_task) = echo_server().await;
        let allowlist = EgressAllowlist::with_resolved(
            [format!("pinned.invalid:{}", echo.port())],
            [("pinned.invalid".into(), echo.ip())],
        )
        .unwrap();
        let proxy = spawn("127.0.0.1:0".parse().unwrap(), allowlist)
            .await
            .unwrap();
        let (mut socket, status) =
            connect(proxy.address(), &format!("pinned.invalid:{}", echo.port()))
                .await
                .unwrap();
        assert_eq!(status, "HTTP/1.1 200 Connection Established");
        socket.write_all(b"pin").await.unwrap();
        let mut buffer = [0u8; 3];
        socket.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"pin");
        proxy.abort();
        echo_task.abort();
    }

    #[tokio::test]
    async fn non_allowlisted_connect_is_denied() {
        let allowlist = EgressAllowlist::new(["index.crates.io:443".into()]).unwrap();
        let proxy = spawn("127.0.0.1:0".parse().unwrap(), allowlist)
            .await
            .unwrap();
        let (_socket, status) = connect(proxy.address(), "127.0.0.1:9").await.unwrap();
        assert_eq!(status, "HTTP/1.1 403 Forbidden");
        proxy.abort();
    }

    #[tokio::test]
    async fn health_probe_is_public_and_does_not_require_a_tunnel() {
        let allowlist = EgressAllowlist::new(["index.crates.io:443".into()]).unwrap();
        let proxy = spawn("127.0.0.1:0".parse().unwrap(), allowlist)
            .await
            .unwrap();
        let mut socket = TcpStream::connect(proxy.address()).await.unwrap();
        socket
            .write_all(b"GET /health HTTP/1.1\r\nHost: proxy\r\n\r\n")
            .await
            .unwrap();
        let mut buffer = [0u8; 256];
        let read = socket.read(&mut buffer).await.unwrap();
        assert!(String::from_utf8_lossy(&buffer[..read]).starts_with("HTTP/1.1 200 OK"));
        proxy.abort();
    }

    #[tokio::test]
    async fn malformed_and_oversized_requests_are_rejected() {
        let allowlist = EgressAllowlist::new(["index.crates.io:443".into()]).unwrap();
        let proxy = spawn("127.0.0.1:0".parse().unwrap(), allowlist)
            .await
            .unwrap();

        let mut socket = TcpStream::connect(proxy.address()).await.unwrap();
        socket
            .write_all(b"GET / HTTP/1.1\r\nHost: index.crates.io\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        let mut buffer = [0u8; 256];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            response.extend_from_slice(&buffer[..read]);
            if response.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        assert!(String::from_utf8_lossy(&response).starts_with("HTTP/1.1 405"));

        let mut socket = TcpStream::connect(proxy.address()).await.unwrap();
        let oversized = format!(
            "CONNECT index.crates.io:443 HTTP/1.1\r\nHost: {}\r\n\r\n",
            "a".repeat(MAX_CONNECT_REQUEST_BYTES)
        );
        socket.write_all(oversized.as_bytes()).await.unwrap();
        let read = socket.read(&mut buffer).await.unwrap_or(0);
        assert!(read == 0 || String::from_utf8_lossy(&buffer[..read]).starts_with("HTTP/1.1"));
        proxy.abort();
    }

    #[test]
    fn allowlist_parsing_is_strict() {
        let allowlist = EgressAllowlist::new([
            "index.crates.io".into(),
            "static.crates.io:443".into(),
            "[::1]:8443".into(),
        ])
        .unwrap();
        assert!(allowlist.allows("index.crates.io", 443));
        assert!(allowlist.allows("STATIC.CRATES.IO", 443));
        assert!(allowlist.allows("::1", 8443));
        assert!(!allowlist.allows("index.crates.io", 80));
        assert!(EgressAllowlist::new(Vec::<String>::new()).is_err());
        assert!(EgressAllowlist::new(["http://index.crates.io".into()]).is_err());
        assert!(EgressAllowlist::new(["index.crates.io:0".into()]).is_err());
        assert!(EgressAllowlist::new(["index crates.io:443".into()]).is_err());
    }
}
