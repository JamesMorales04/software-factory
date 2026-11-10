//! Opt-in real-Podman network-boundary and acquisition test.
//!
//! Proves, with real containers, that the acquisition container on an internal
//! Podman network has no direct egress, that the deny-by-default CONNECT proxy
//! refuses a non-allowlisted destination, that an allowlisted crates.io
//! destination succeeds, and that the production acquirer runs a
//! credential-free container through that proxy with the pinned image ID.
//!
//! Skipped unless `FACTORY_PODMAN_DEPENDENCY_TEST=1` and the test image exists.
//! Requires a host rootless Podman with network access and the image named by
//! `FACTORY_DEPENDENCY_TEST_IMAGE` (default
//! `localhost/software-factory-dependency:test`). No model inference.

use std::{
    io::Cursor,
    net::{IpAddr, ToSocketAddrs},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use factory_native::dependency_service::{
    DEPENDENCY_ACQUIRE_ROUTE, DependencyAcquireWireRequest, DependencyService,
    PodmanDependencyAcquirer,
};
use sha2::{Digest, Sha256};

const TOKEN: &str = "dependency-podman-token-0123456789abcdef";
const REGISTRY: &str = "sparse+https://index.crates.io/";
const RUST_IMAGE: &str = "docker.io/library/rust:1.90.0-bookworm";
const SUBNET: &str = "10.91.0.0/24";
const GATEWAY: &str = "10.91.0.1";
const PROXY_IP: &str = "10.91.0.10";
const EGRESS_ALLOW: &str =
    "index.crates.io:443,static.crates.io:443,github.com:443,codeload.github.com:443";

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn podman() -> Option<PathBuf> {
    let output = Command::new("podman").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join("podman"))
        .find(|candidate| candidate.is_file())
}

fn image_exists(podman: &Path, image: &str) -> bool {
    Command::new(podman)
        .args(["image", "exists", image])
        .status()
        .is_ok_and(|status| status.success())
}

fn image_id(podman: &Path, image: &str) -> Option<String> {
    let output = Command::new(podman)
        .args(["image", "inspect", "--format", "{{.Id}}", image])
        .output()
        .ok()?;
    output.status.success().then(|| {
        factory_native::dependency_service::normalize_image_id(
            String::from_utf8_lossy(&output.stdout).trim(),
        )
    })?
}

fn run(podman: &Path, arguments: &[&str]) -> bool {
    Command::new(podman)
        .args(arguments)
        .status()
        .is_ok_and(|status| status.success())
}

fn output(podman: &Path, arguments: &[&str]) -> Option<std::process::Output> {
    Command::new(podman).args(arguments).output().ok()
}

fn volume_import(podman: &Path, volume: &str, bytes: &[u8]) -> bool {
    use std::io::Write;
    let mut child = match Command::new(podman)
        .args(["volume", "import", volume, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let Some(mut stdin) = child.stdin.take() else {
        return false;
    };
    if stdin.write_all(bytes).is_err() {
        return false;
    }
    drop(stdin);
    child.wait().is_ok_and(|status| status.success())
}

fn fixture_tar() -> (Vec<u8>, String) {
    let lockfile = "version = 4\n\n[[package]]\nname = \"factory-podman-dependency-smoke\"\nversion = \"0.1.0\"\n";
    let mut builder = tar::Builder::new(Vec::new());
    let mut append = |name: &str, body: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, Cursor::new(body))
            .unwrap();
    };
    append(
        "Cargo.toml",
        b"[package]\nname = \"factory-podman-dependency-smoke\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    append("Cargo.lock", lockfile.as_bytes());
    append("src/main.rs", b"fn main() {}\n");
    (builder.into_inner().unwrap(), hex(lockfile.as_bytes()))
}

fn network_ip(podman: &Path, container: &str, skip: &str) -> Option<IpAddr> {
    let output = output(
        podman,
        &[
            "inspect",
            "--format",
            "{{json .NetworkSettings.Networks}}",
            container,
        ],
    )?;
    let networks: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    networks
        .as_object()?
        .values()
        .filter_map(|network| network.get("IPAddress")?.as_str())
        .filter(|address| !address.is_empty() && *address != skip)
        .find_map(|address| address.parse().ok())
}

fn wait_for_proxy(podman: &Path, container: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let health = Command::new(podman)
            .args([
                "exec",
                container,
                "curl",
                "-fsS",
                "--max-time",
                "2",
                "http://127.0.0.1:18080/health",
            ])
            .output();
        if health.is_ok_and(|output| output.status.success()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "dependency egress proxy did not become healthy"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

struct Resources {
    podman: PathBuf,
    network: String,
    proxy: String,
    volume: String,
}

impl Drop for Resources {
    fn drop(&mut self) {
        let _ = Command::new(&self.podman)
            .args(["rm", "-f", &self.proxy])
            .output();
        let _ = Command::new(&self.podman)
            .args(["volume", "rm", "-f", &self.volume])
            .output();
        let _ = Command::new(&self.podman)
            .args(["network", "rm", "-f", &self.network])
            .output();
    }
}

#[tokio::test]
async fn podman_acquirer_enforces_egress_and_pins_the_image() {
    if std::env::var("FACTORY_PODMAN_DEPENDENCY_TEST").as_deref() != Ok("1") {
        eprintln!(
            "SKIPPED real-Podman dependency acquisition: set FACTORY_PODMAN_DEPENDENCY_TEST=1 to opt in"
        );
        return;
    }
    let Some(podman) = podman() else {
        eprintln!("SKIPPED real-Podman dependency acquisition: podman is unavailable");
        return;
    };
    let image = std::env::var("FACTORY_DEPENDENCY_TEST_IMAGE")
        .unwrap_or_else(|_| "localhost/software-factory-dependency:test".into());
    if !image_exists(&podman, &image) {
        eprintln!("SKIPPED real-Podman dependency acquisition: image {image} is unavailable");
        return;
    }
    let Some(image_id) = image_id(&podman, &image) else {
        eprintln!("SKIPPED real-Podman dependency acquisition: image ID is unavailable");
        return;
    };
    let crates_ip = ("index.crates.io", 443)
        .to_socket_addrs()
        .expect("resolve index.crates.io")
        .find_map(|address| match address.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            std::net::IpAddr::V6(_) => None,
        })
        .expect("index.crates.io IPv4");
    let suffix = uuid::Uuid::new_v4().to_string();
    let network = format!("zz-factory-deps-egress-{suffix}");
    let proxy = format!("factory-deps-proxy-{suffix}");
    let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
    let container = format!("factory-deps-{}", uuid::Uuid::new_v4());
    let _resources = Resources {
        podman: podman.clone(),
        network: network.clone(),
        proxy: proxy.clone(),
        volume: volume.clone(),
    };

    assert!(
        run(
            &podman,
            &[
                "network",
                "create",
                "--internal",
                "--subnet",
                SUBNET,
                "--gateway",
                GATEWAY,
                &network,
            ],
        ),
        "internal network create failed"
    );
    assert!(
        run(
            &podman,
            &[
                "run",
                "-d",
                "--name",
                &proxy,
                "--network=bridge",
                &format!("--network={network}:ip={PROXY_IP}"),
                "--dns",
                "1.1.1.1",
                "--read-only",
                "--cap-drop=all",
                "--security-opt=no-new-privileges",
                "--pids-limit=64",
                "--memory=256m",
                "--entrypoint",
                "/usr/local/bin/factory-dependency-egress-proxy",
                "-e",
                "FACTORY_EGRESS_LISTEN=0.0.0.0:18080",
                "-e",
                &format!("FACTORY_EGRESS_ALLOW={EGRESS_ALLOW}"),
                "-e",
                &format!("FACTORY_EGRESS_RESOLVE=index.crates.io={crates_ip}"),
                &image,
            ],
        ),
        "dependency egress proxy container failed to start"
    );
    wait_for_proxy(&podman, &proxy);

    // The internal network has no default route and no external DNS.
    let direct = output(
        &podman,
        &[
            "run",
            "--rm",
            &format!("--network={network}"),
            RUST_IMAGE,
            "curl",
            "-sS",
            "--max-time",
            "3",
            "https://example.com",
        ],
    );
    assert!(
        direct.is_some_and(|output| !output.status.success()),
        "direct egress from the internal network unexpectedly succeeded"
    );

    let bridge_ip = network_ip(&podman, &proxy, PROXY_IP).expect("proxy bridge IP");
    let denied = output(
        &podman,
        &[
            "run",
            "--rm",
            "--network=bridge",
            RUST_IMAGE,
            "curl",
            "-sS",
            "--max-time",
            "5",
            "-x",
            &format!("http://{bridge_ip}:18080"),
            "https://example.com",
        ],
    );
    assert!(
        denied.is_some_and(|output| !output.status.success()),
        "non-allowlisted destination was not denied"
    );

    let mut allowed = None;
    for _ in 0..3 {
        allowed = output(
            &podman,
            &[
                "run",
                "--rm",
                "--network=bridge",
                RUST_IMAGE,
                "curl",
                "-fsS",
                "--max-time",
                "15",
                "-x",
                &format!("http://{bridge_ip}:18080"),
                "--resolve",
                &format!("index.crates.io:443:{crates_ip}"),
                "https://index.crates.io/config.json",
            ],
        );
        if allowed
            .as_ref()
            .is_some_and(|output| output.status.success())
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let allowed = allowed.expect("proxy allow probe");
    assert!(
        allowed.status.success(),
        "allowlisted crates.io destination was refused: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );

    assert!(
        run(&podman, &["volume", "create", &volume]),
        "podman volume create failed"
    );
    let (tar, lockfile_sha256) = fixture_tar();
    assert!(
        volume_import(&podman, &volume, &tar),
        "podman volume import failed"
    );

    // A mismatched pinned image ID must fail closed before any container runs.
    let wrong_id = format!("sha256:{}", "0".repeat(64));
    let wrong = Arc::new(
        PodmanDependencyAcquirer::new(podman.clone(), wrong_id)
            .unwrap()
            .with_egress(
                network.clone(),
                format!("http://{PROXY_IP}:18080"),
                Arc::new(AtomicBool::new(true)),
            )
            .unwrap(),
    );
    let wrong_service = Arc::new(
        DependencyService::new(TOKEN.into(), image.clone(), vec![REGISTRY.into()], wrong).unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, wrong_service.router()).await.unwrap();
    });
    let request = DependencyAcquireWireRequest {
        volume: volume.clone(),
        container: container.clone(),
        workdir: ".".into(),
        lockfile_path: "Cargo.lock".into(),
        lockfile_sha256: lockfile_sha256.clone(),
        vendor_path: "vendor".into(),
        registry: REGISTRY.into(),
        timeout_secs: 300,
    };
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{address}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    let body = response.text().await.unwrap();
    assert!(body.contains("identity mismatch"), "body: {body}");
    server.abort();

    // The real acquirer runs through the internal network and the proxy with
    // the exact image ID that was imported.
    let acquirer = Arc::new(
        PodmanDependencyAcquirer::new(podman.clone(), image_id)
            .unwrap()
            .with_egress(
                network.clone(),
                format!("http://{PROXY_IP}:18080"),
                Arc::new(AtomicBool::new(true)),
            )
            .unwrap(),
    );
    let service = Arc::new(
        DependencyService::new(TOKEN.into(), image.clone(), vec![REGISTRY.into()], acquirer)
            .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, service.router()).await.unwrap();
    });
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(360))
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{address}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&request)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::OK, "broker response: {body}");
    let receipt: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(receipt["image"], image);
    assert_eq!(receipt["stagingCleaned"], true);
    assert!(receipt["vendorSha256"].as_str().unwrap().len() == 64);

    let export = Command::new(&podman)
        .args(["volume", "export", &volume])
        .output()
        .expect("podman volume export");
    assert!(export.status.success());
    let mut archive = tar::Archive::new(Cursor::new(export.stdout));
    let entries: Vec<String> = archive
        .entries()
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().unwrap().to_string_lossy().into_owned())
        .collect();
    for evidence in ["sbom.cdx.json", "licenses.json", "advisories.json"] {
        assert!(
            entries
                .iter()
                .any(|entry| entry.contains(&format!("vendor/.factory-supply-chain/{evidence}"))),
            "missing promoted {evidence}: {entries:?}"
        );
    }

    server.abort();
    assert!(run(&podman, &["rm", "-f", &proxy]));
    assert!(run(&podman, &["volume", "rm", "-f", &volume]));
    assert!(run(&podman, &["network", "rm", "-f", &network]));
    let leftover = Command::new(&podman)
        .args(["container", "exists", &container])
        .status();
    assert!(
        !leftover.is_ok_and(|status| status.success()),
        "dependency container was not cleaned up"
    );
}
