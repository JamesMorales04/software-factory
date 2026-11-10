//! Opt-in real-Podman test for the runtime broker's dependency acquirer.
//!
//! This exercises the production `PodmanDependencyAcquirer` argv, named-volume
//! mount, network-enabled ephemeral container, receipt validation and cleanup.
//! It is skipped unless `FACTORY_PODMAN_DEPENDENCY_TEST=1` and the test image
//! named by `FACTORY_DEPENDENCY_TEST_IMAGE` (default
//! `localhost/software-factory-dependency:test`) exists. It never calls a model
//! provider. A no-dependency fixture avoids a registry fetch; live registry
//! coverage is the separately opt-in `dependency_acquire_live` test.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use factory_native::dependency_service::{
    DEPENDENCY_ACQUIRE_ROUTE, DependencyAcquireWireRequest, DependencyService,
    PodmanDependencyAcquirer,
};
use sha2::{Digest, Sha256};

const TOKEN: &str = "dependency-podman-token-0123456789abcdef";
const REGISTRY: &str = "sparse+https://index.crates.io/";

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

#[tokio::test]
async fn podman_acquirer_runs_a_real_credential_free_container() {
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
    let volume = format!("factory-ws-{}", uuid::Uuid::new_v4());
    let container = format!("factory-deps-{}", uuid::Uuid::new_v4());
    let created = Command::new(&podman)
        .args(["volume", "create", &volume])
        .status()
        .is_ok_and(|status| status.success());
    assert!(created, "podman volume create failed");
    let (tar, lockfile_sha256) = fixture_tar();
    assert!(
        volume_import(&podman, &volume, &tar),
        "podman volume import failed"
    );

    let acquirer = Arc::new(PodmanDependencyAcquirer::new(podman.clone()).unwrap());
    let service = Arc::new(
        DependencyService::new(TOKEN.into(), image.clone(), vec![REGISTRY.into()], acquirer)
            .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, service.router()).await.unwrap();
    });
    let request = DependencyAcquireWireRequest {
        volume: volume.clone(),
        container: container.clone(),
        workdir: ".".into(),
        lockfile_path: "Cargo.lock".into(),
        lockfile_sha256,
        vendor_path: "vendor".into(),
        registry: REGISTRY.into(),
        timeout_secs: 300,
    };
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
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("vendor/.factory-supply-chain/sbom.cdx.json")),
        "missing promoted SBOM: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("vendor/.factory-supply-chain/licenses.json")),
        "missing promoted license evidence: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("vendor/.factory-supply-chain/advisories.json")),
        "missing promoted advisory evidence: {entries:?}"
    );

    server.abort();
    let cleanup = Command::new(&podman)
        .args(["volume", "rm", "--force", &volume])
        .status();
    assert!(cleanup.is_ok_and(|status| status.success()));
    let leftover = Command::new(&podman)
        .args(["container", "exists", &container])
        .status();
    assert!(
        !leftover.is_ok_and(|status| status.success()),
        "dependency container was not cleaned up"
    );
}
