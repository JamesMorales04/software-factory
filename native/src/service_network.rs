//! Strict network-origin and bind validation for containerized Factory
//! services.
//!
//! Host-native services keep loopback-only bindings. Container mode permits
//! only an explicit IPv4 unspecified bind inside the container and a small
//! allowlist of Compose service hostnames for outbound HTTP; it never enables
//! host networking or arbitrary remote origins.

use std::net::SocketAddr;

pub fn validate_bind(raw: &str, container_mode: bool) -> Result<SocketAddr, String> {
    let addr: SocketAddr = raw
        .parse()
        .map_err(|_| format!("invalid bind address {raw:?}"))?;
    if addr.ip().is_loopback() {
        return Ok(addr);
    }
    if container_mode && addr.ip().is_unspecified() && addr.is_ipv4() {
        return Ok(addr);
    }
    Err(
        "service may bind only to loopback, or to an IPv4 unspecified container interface with FACTORY_CONTAINER_MODE=1"
            .into(),
    )
}

pub fn validate_http_origin(
    raw: &str,
    name: &str,
    container_mode: bool,
    allowed_service_hosts: &[&str],
) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| format!("invalid {name}"))?;
    if url.scheme() != "http"
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port().is_none()
    {
        return Err(format!(
            "{name} must be a plain HTTP origin with an explicit port"
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| format!("{name} must include a host"))?;
    if matches!(host, "127.0.0.1" | "localhost") {
        return Ok(url);
    }
    if container_mode && allowed_service_hosts.contains(&host) {
        return Ok(url);
    }
    Err(format!(
        "{name} must use loopback, or an explicitly allowed Compose service host in container mode"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_require_explicit_container_opt_in() {
        assert!(validate_bind("127.0.0.1:8765", false).is_ok());
        assert!(validate_bind("0.0.0.0:8765", false).is_err());
        assert!(validate_bind("0.0.0.0:8765", true).is_ok());
        assert!(validate_bind("[::]:8765", true).is_err());
        assert!(validate_bind("203.0.113.4:8765", true).is_err());
    }

    #[test]
    fn origins_use_an_explicit_service_allowlist() {
        assert!(validate_http_origin("http://127.0.0.1:8765", "core", false, &["core"]).is_ok());
        assert!(
            validate_http_origin("http://core:8765", "core", false, &["core"]).is_err(),
            "container service DNS must require container mode"
        );
        assert!(validate_http_origin("http://core:8765", "core", true, &["core"]).is_ok());
        assert!(validate_http_origin("http://attacker:8765", "core", true, &["core"]).is_err());
        assert!(validate_http_origin("http://core:8765/path", "core", true, &["core"]).is_err());
        assert!(validate_http_origin("https://core:8765", "core", true, &["core"]).is_err());
    }
}
