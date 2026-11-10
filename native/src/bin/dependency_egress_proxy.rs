//! Standalone deny-by-default dependency egress proxy.
//!
//! The installed runtime broker runs the same proxy in-process. This binary
//! exists so the proxy can be exercised as a real container on an internal
//! Podman network in the opt-in network-boundary test, and for diagnostics.
//! It carries no credentials and logs only host/port decisions.

use std::error::Error;

use factory_native::dependency_egress::{self, EgressAllowlist};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listen = std::env::var("FACTORY_EGRESS_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:18080".into())
        .parse()?;
    let raw =
        std::env::var("FACTORY_EGRESS_ALLOW").map_err(|_| "FACTORY_EGRESS_ALLOW is required")?;
    let entries: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect();
    let resolutions: Vec<(String, std::net::IpAddr)> = std::env::var("FACTORY_EGRESS_RESOLVE")
        .unwrap_or_default()
        .split(',')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (host, address) = entry
                .split_once('=')
                .ok_or_else(|| "FACTORY_EGRESS_RESOLVE entries must be host=ip".to_string())?;
            let address = address
                .trim()
                .parse()
                .map_err(|_| "FACTORY_EGRESS_RESOLVE address is invalid".to_string())?;
            Ok((host.trim().to_owned(), address))
        })
        .collect::<Result<_, String>>()?;
    let allowlist = EgressAllowlist::with_resolved(entries, resolutions)?;
    let proxy = dependency_egress::spawn(listen, allowlist).await?;
    eprintln!(
        "factory-dependency-egress-proxy listening on {}",
        proxy.address()
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}
