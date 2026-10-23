use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Host {
    Localhost,
    Ip(IpAddr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreUrl {
    host: Host,
    port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreUrlError {
    Empty,
    SurroundingWhitespace,
    MissingScheme,
    UnsupportedScheme,
    UserInfo,
    MissingHost,
    NonLoopbackHost,
    MissingPort,
    InvalidPort,
    InvalidIpv6Host,
    NonOriginPath,
    Query,
    Fragment,
}

impl fmt::Display for CoreUrlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "URL is empty",
            Self::SurroundingWhitespace => "URL has surrounding whitespace",
            Self::MissingScheme => "URL must start with http://",
            Self::UnsupportedScheme => "URL must use the http scheme",
            Self::UserInfo => "URL must not contain user information",
            Self::MissingHost => "URL must contain a loopback host",
            Self::NonLoopbackHost => "URL host must be loopback (127.0.0.1, [::1], localhost)",
            Self::MissingPort => "URL must include an explicit port",
            Self::InvalidPort => "URL port must be an integer in 1..=65535",
            Self::InvalidIpv6Host => "URL has an invalid bracketed IPv6 host",
            Self::NonOriginPath => "URL path must be empty or '/'",
            Self::Query => "URL must not contain a query",
            Self::Fragment => "URL must not contain a fragment",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CoreUrlError {}

impl CoreUrl {
    pub(crate) fn parse(raw: &str) -> Result<Self, CoreUrlError> {
        if raw.is_empty() {
            return Err(CoreUrlError::Empty);
        }
        if raw.trim() != raw {
            return Err(CoreUrlError::SurroundingWhitespace);
        }
        let Some((scheme, remainder)) = raw.split_once("://") else {
            return Err(CoreUrlError::MissingScheme);
        };
        if !scheme.eq_ignore_ascii_case("http") {
            return Err(CoreUrlError::UnsupportedScheme);
        }
        if remainder.contains('#') {
            return Err(CoreUrlError::Fragment);
        }
        if remainder.contains('?') {
            return Err(CoreUrlError::Query);
        }
        let (authority, path) = match remainder.find('/') {
            Some(index) => (&remainder[..index], &remainder[index..]),
            None => (remainder, ""),
        };
        if !path.is_empty() && path != "/" {
            return Err(CoreUrlError::NonOriginPath);
        }
        if authority.contains('@') {
            return Err(CoreUrlError::UserInfo);
        }
        let (host, port) = parse_authority(authority)?;
        Ok(Self { host, port })
    }

    pub(crate) fn host_header(&self) -> String {
        match self.host {
            Host::Localhost => format!("localhost:{}", self.port),
            Host::Ip(IpAddr::V4(address)) => format!("{address}:{}", self.port),
            Host::Ip(IpAddr::V6(address)) => format!("[{address}]:{}", self.port),
        }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn is_ipv4_localhost(&self) -> bool {
        match self.host {
            Host::Localhost => true,
            Host::Ip(IpAddr::V4(address)) => address == Ipv4Addr::LOCALHOST,
            Host::Ip(IpAddr::V6(_)) => false,
        }
    }

    pub(crate) fn socket_addrs(&self) -> std::io::Result<Vec<SocketAddr>> {
        match self.host {
            Host::Localhost => ("localhost", self.port)
                .to_socket_addrs()
                .map(|addresses| addresses.collect()),
            Host::Ip(address) => Ok(vec![SocketAddr::new(address, self.port)]),
        }
    }
}

impl fmt::Display for CoreUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("http://")?;
        formatter.write_str(&self.host_header())
    }
}

fn parse_authority(authority: &str) -> Result<(Host, u16), CoreUrlError> {
    if authority.is_empty() {
        return Err(CoreUrlError::MissingHost);
    }
    let (host_text, port_text) = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(close) = bracketed.find(']') else {
            return Err(CoreUrlError::InvalidIpv6Host);
        };
        let host = &bracketed[..close];
        let Some(port) = bracketed[close + 1..].strip_prefix(':') else {
            return Err(CoreUrlError::MissingPort);
        };
        let address = Ipv6Addr::from_str(host).map_err(|_| CoreUrlError::InvalidIpv6Host)?;
        if !address.is_loopback() {
            return Err(CoreUrlError::NonLoopbackHost);
        }
        return parse_port(port).map(|port| (Host::Ip(IpAddr::V6(address)), port));
    } else {
        let Some((host, port)) = authority.rsplit_once(':') else {
            return Err(CoreUrlError::MissingPort);
        };
        if host.contains(':') {
            return Err(CoreUrlError::InvalidIpv6Host);
        }
        (host, port)
    };

    if host_text.eq_ignore_ascii_case("localhost") {
        return parse_port(port_text).map(|port| (Host::Localhost, port));
    }
    let address = Ipv4Addr::from_str(host_text).map_err(|_| CoreUrlError::MissingHost)?;
    if !address.is_loopback() {
        return Err(CoreUrlError::NonLoopbackHost);
    }
    parse_port(port_text).map(|port| (Host::Ip(IpAddr::V4(address)), port))
}

fn parse_port(raw: &str) -> Result<u16, CoreUrlError> {
    if raw.is_empty() {
        return Err(CoreUrlError::MissingPort);
    }
    let port = raw.parse::<u16>().map_err(|_| CoreUrlError::InvalidPort)?;
    if port == 0 {
        return Err(CoreUrlError::InvalidPort);
    }
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::{CoreUrl, CoreUrlError};

    #[test]
    fn accepts_loopback_http_origins_with_explicit_port() {
        for value in [
            "http://127.0.0.1:8765",
            "http://127.0.0.1:8765/",
            "http://127.0.0.2:1",
            "http://[::1]:8765",
            "http://localhost:8765",
            "HTTP://LOCALHOST:8765",
        ] {
            assert!(CoreUrl::parse(value).is_ok(), "expected valid: {value}");
        }
    }

    #[test]
    fn rejects_non_loopback_or_non_http_origins() {
        assert_eq!(
            CoreUrl::parse("https://127.0.0.1:8765"),
            Err(CoreUrlError::UnsupportedScheme)
        );
        assert_eq!(
            CoreUrl::parse("http://192.168.1.2:8765"),
            Err(CoreUrlError::NonLoopbackHost)
        );
        assert_eq!(
            CoreUrl::parse("http://example.com:8765"),
            Err(CoreUrlError::MissingHost)
        );
        assert_eq!(
            CoreUrl::parse("http://[::2]:8765"),
            Err(CoreUrlError::NonLoopbackHost)
        );
    }

    #[test]
    fn rejects_userinfo_query_fragment_and_paths() {
        assert_eq!(
            CoreUrl::parse("http://user:pass@127.0.0.1:8765"),
            Err(CoreUrlError::UserInfo)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:8765?probe=1"),
            Err(CoreUrlError::Query)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:8765#top"),
            Err(CoreUrlError::Fragment)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:8765/api"),
            Err(CoreUrlError::NonOriginPath)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:8765//"),
            Err(CoreUrlError::NonOriginPath)
        );
    }

    #[test]
    fn requires_explicit_valid_port() {
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1"),
            Err(CoreUrlError::MissingPort)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:"),
            Err(CoreUrlError::MissingPort)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:0"),
            Err(CoreUrlError::InvalidPort)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:65536"),
            Err(CoreUrlError::InvalidPort)
        );
        assert_eq!(
            CoreUrl::parse("http://127.0.0.1:http"),
            Err(CoreUrlError::InvalidPort)
        );
    }

    #[test]
    fn exposes_the_loopback_publication_port() {
        let url = CoreUrl::parse("http://127.0.0.1:9123").expect("url");
        assert_eq!(url.port(), 9123);
        assert!(url.is_ipv4_localhost());
        assert!(
            CoreUrl::parse("http://localhost:9123")
                .expect("url")
                .is_ipv4_localhost()
        );
    }

    #[test]
    fn rejects_non_ipv4_localhost_publication_hosts() {
        assert!(
            !CoreUrl::parse("http://[::1]:8765")
                .expect("url")
                .is_ipv4_localhost()
        );
        assert!(
            !CoreUrl::parse("http://127.0.0.2:8765")
                .expect("url")
                .is_ipv4_localhost()
        );
    }

    #[test]
    fn rejects_empty_whitespace_and_bad_ipv6_brackets() {
        assert_eq!(CoreUrl::parse(""), Err(CoreUrlError::Empty));
        assert_eq!(
            CoreUrl::parse(" http://127.0.0.1:8765"),
            Err(CoreUrlError::SurroundingWhitespace)
        );
        assert_eq!(
            CoreUrl::parse("http://[::1:8765"),
            Err(CoreUrlError::InvalidIpv6Host)
        );
        assert_eq!(
            CoreUrl::parse("http://::1:8765"),
            Err(CoreUrlError::InvalidIpv6Host)
        );
    }
}
