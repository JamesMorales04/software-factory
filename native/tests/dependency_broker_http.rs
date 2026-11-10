//! HTTP-boundary tests for the runtime-broker dependency acquisition route.
//!
//! The fixture acquirer calls the real library-level atomic acquisition with a
//! deterministic command runner. This exercises authentication, bounded JSON,
//! request validation, receipt validation and the vendor/evidence promotion
//! path without Podman, a registry or paid inference.

use std::{
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use factory_native::dependency_broker::{
    self, ADVISORY_FILE, CargoEvidenceProducer, CommandRunner, DependencyAcquisitionRequest,
    DependencyEvidenceFile, EVIDENCE_DIR, LICENSE_FILE, SBOM_FILE,
};
use factory_native::dependency_service::{
    DEPENDENCY_ACQUIRE_ROUTE, DependencyAcquireError, DependencyAcquireReceipt,
    DependencyAcquireRequest, DependencyAcquireWireRequest, DependencyAcquirer, DependencyService,
};
use sha2::{Digest, Sha256};

const TOKEN: &str = "dependency-integration-token-0123456789abcdef";
const IMAGE: &str = "localhost/software-factory-dependency:dev";
const VOLUME: &str = "factory-ws-123e4567-e89b-12d3-a456-426614174000";
const CONTAINER: &str = "factory-deps-123e4567-e89b-12d3-a456-426614174000";

struct FixtureAcquirer {
    root: PathBuf,
    fail: bool,
    invocations: Mutex<usize>,
}

impl FixtureAcquirer {
    fn new(root: PathBuf, fail: bool) -> Self {
        Self {
            root,
            fail,
            invocations: Mutex::new(0),
        }
    }
}

impl DependencyAcquirer for FixtureAcquirer {
    fn acquire<'a>(
        &'a self,
        _volume: &'a str,
        _container: &'a str,
        image: &'a str,
        request: &'a DependencyAcquireRequest,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<DependencyAcquireReceipt, DependencyAcquireError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            *self.invocations.lock().unwrap() += 1;
            let broker_request = DependencyAcquisitionRequest {
                root: self.root.clone(),
                workdir: request.workdir.clone(),
                lockfile_path: request.lockfile_path.clone(),
                lockfile_sha256: request.lockfile_sha256.clone(),
                vendor_path: request.vendor_path.clone(),
                fetch_command: vec!["cargo".into(), "fetch".into(), "--locked".into()],
                vendor_command: vec![
                    "cargo".into(),
                    "vendor".into(),
                    "--locked".into(),
                    request.vendor_path.clone(),
                ],
            };
            let runner = FixtureRunner { fail: self.fail };
            let evidence =
                dependency_broker::acquire(&broker_request, &runner, &CargoEvidenceProducer)
                    .map_err(|error| DependencyAcquireError::Command(error.to_string()))?;
            let file = |name: &str| -> Result<(String, String), DependencyAcquireError> {
                let path = format!("{EVIDENCE_DIR}/{name}");
                let sha = evidence
                    .evidence
                    .iter()
                    .find(|file| file.path == path)
                    .map(|file| file.sha256.clone())
                    .ok_or_else(|| {
                        DependencyAcquireError::InvalidReceipt(format!(
                            "fixture evidence {name} missing"
                        ))
                    })?;
                Ok((format!("{}/{path}", request.vendor_path), sha))
            };
            let (sbom_path, sbom_sha256) = file(SBOM_FILE)?;
            let (license_path, license_sha256) = file(LICENSE_FILE)?;
            let (advisory_path, advisory_sha256) = file(ADVISORY_FILE)?;
            Ok(DependencyAcquireReceipt {
                image: image.to_owned(),
                lockfile_sha256: evidence.lockfile_sha256,
                vendor_sha256: evidence.vendor_sha256,
                vendor_entries: evidence.vendor_entries,
                sbom_path,
                sbom_sha256,
                license_path,
                license_sha256,
                advisory_path,
                advisory_sha256,
                staging_cleaned: evidence.staging_cleaned,
            })
        })
    }
}

struct FixtureRunner {
    fail: bool,
}

impl CommandRunner for FixtureRunner {
    fn run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
        self.run_in(argv, cwd, &std::env::temp_dir())
    }

    fn run_in(&self, argv: &[String], _cwd: &Path, _cargo_home: &Path) -> Result<(), String> {
        if self.fail {
            return Err("locked fetch failed: fixture".into());
        }
        if argv.first().map(String::as_str) == Some("cargo")
            && argv.get(1).map(String::as_str) == Some("vendor")
        {
            let vendor = argv.last().unwrap();
            std::fs::create_dir_all(vendor).unwrap();
            std::fs::write(Path::new(vendor).join("crate.txt"), "vendored").unwrap();
        }
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn root() -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("factory-dependency-http-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
    root
}

fn wire(root: &Path) -> DependencyAcquireWireRequest {
    let lockfile = std::fs::read(root.join("Cargo.lock")).unwrap();
    DependencyAcquireWireRequest {
        volume: VOLUME.into(),
        container: CONTAINER.into(),
        workdir: ".".into(),
        lockfile_path: "Cargo.lock".into(),
        lockfile_sha256: hex(&lockfile),
        vendor_path: "vendor".into(),
        registry: "sparse+https://index.crates.io/".into(),
        timeout_secs: 600,
    }
}

async fn spawn(
    root: &Path,
    fail: bool,
) -> (String, Arc<FixtureAcquirer>, tokio::task::JoinHandle<()>) {
    let acquirer = Arc::new(FixtureAcquirer::new(root.to_path_buf(), fail));
    let service = Arc::new(
        DependencyService::new(
            TOKEN.into(),
            IMAGE.into(),
            vec!["sparse+https://index.crates.io/".into()],
            acquirer.clone(),
        )
        .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, service.router()).await.unwrap();
    });
    (format!("http://{address}"), acquirer, handle)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
        .unwrap()
}

#[tokio::test]
async fn unauthorized_and_malformed_requests_are_rejected() {
    let root = root();
    let (base, acquirer, handle) = spawn(&root, false).await;
    let response = client()
        .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .json(&wire(&root))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

    let mut object = serde_json::to_value(wire(&root)).unwrap();
    object.as_object_mut().unwrap().insert(
        "token".into(),
        serde_json::Value::String("provider-secret".into()),
    );
    let response = client()
        .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&object)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let body = response.text().await.unwrap();
    assert!(!body.contains("provider-secret"));
    assert_eq!(*acquirer.invocations.lock().unwrap(), 0);
    handle.abort();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn valid_request_promotes_vendor_and_evidence_atomically() {
    let root = root();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
    let (base, acquirer, handle) = spawn(&root, false).await;
    let response = client()
        .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&wire(&root))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(!body.contains(TOKEN));
    let receipt: DependencyAcquireReceipt = serde_json::from_str(&body).unwrap();
    assert_eq!(receipt.lockfile_sha256, wire(&root).lockfile_sha256);
    assert_eq!(receipt.vendor_entries, 4);
    assert!(receipt.staging_cleaned);
    assert_eq!(
        receipt.sbom_path,
        format!("vendor/{EVIDENCE_DIR}/{SBOM_FILE}")
    );
    assert_eq!(*acquirer.invocations.lock().unwrap(), 1);
    assert!(root.join("vendor/crate.txt").is_file());
    assert!(
        root.join("vendor")
            .join(EVIDENCE_DIR)
            .join(SBOM_FILE)
            .is_file()
    );
    assert!(!root.join("vendor/old.txt").exists());
    let evidence = receipt.evidence();
    assert_eq!(evidence.sbom_ref, receipt.sbom_path);
    assert_eq!(evidence.vendor_sha256, receipt.vendor_sha256);
    handle.abort();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn unsafe_paths_registry_bounds_and_lockfile_mismatch_do_not_touch_vendor() {
    let root = root();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
    let (base, acquirer, handle) = spawn(&root, false).await;

    let mutations: [fn(&mut DependencyAcquireWireRequest); 5] = [
        |request| request.workdir = "../escape".into(),
        |request| request.vendor_path = "vendor/../../x".into(),
        |request| request.registry = "https://example.invalid/index".into(),
        |request| request.timeout_secs = 901,
        |request| request.volume = "host-volume".into(),
    ];
    for mutate in mutations {
        let mut request = wire(&root);
        mutate(&mut request);
        let response = client()
            .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
            .bearer_auth(TOKEN)
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }
    assert_eq!(*acquirer.invocations.lock().unwrap(), 0);
    assert_eq!(
        std::fs::read_to_string(root.join("vendor/old.txt")).unwrap(),
        "old"
    );

    let mut mismatch = wire(&root);
    mismatch.lockfile_sha256 = "0".repeat(64);
    let response = client()
        .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&mismatch)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    assert_eq!(*acquirer.invocations.lock().unwrap(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("vendor/old.txt")).unwrap(),
        "old"
    );
    handle.abort();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn command_failure_preserves_the_previous_vendor_tree() {
    let root = root();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
    let (base, acquirer, handle) = spawn(&root, true).await;
    let response = client()
        .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&wire(&root))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    let body = response.text().await.unwrap();
    assert!(!body.contains(TOKEN));
    assert_eq!(*acquirer.invocations.lock().unwrap(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("vendor/old.txt")).unwrap(),
        "old"
    );
    handle.abort();
    let _ = std::fs::remove_dir_all(root);
}

#[derive(Clone, Default)]
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn broker_logs_do_not_contain_credentials() {
    let root = root();
    let (base, _acquirer, handle) = spawn(&root, false).await;
    let capture = CaptureWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("SKIPPED log capture: a global tracing subscriber is already installed");
        handle.abort();
        let _ = std::fs::remove_dir_all(root);
        return;
    }
    let response = client()
        .post(format!("{base}{DEPENDENCY_ACQUIRE_ROUTE}"))
        .bearer_auth(TOKEN)
        .json(&wire(&root))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let logs = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    assert!(!logs.contains(TOKEN), "captured logs: {logs}");
    assert!(
        logs.contains("dependency acquisition requested"),
        "captured logs: {logs}"
    );
    handle.abort();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn evidence_file_type_is_exported_for_callers() {
    let file = DependencyEvidenceFile {
        path: format!("{EVIDENCE_DIR}/{SBOM_FILE}"),
        sha256: "a".repeat(64),
    };
    let encoded = serde_json::to_string(&file).unwrap();
    assert!(encoded.contains("sbom.cdx.json"));
}
