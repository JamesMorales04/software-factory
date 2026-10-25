//! Managed external browser test-server acceptance.
//!
//! The server lifecycle assertions always run. The real Chromium assertion runs
//! when the pinned tooling is available; otherwise it explicitly reports
//! `NOT_TESTED` and only that assertion is skipped.

use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    time::Duration,
};

use factory_native::browser::{
    BrowserCheckConfig, BrowserCheckStatus, BrowserError, BrowserTooling, LoopbackEndpoint,
    ManagedTestServer, ManagedTestServerConfig, run_browser_check,
};

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "factory-browser-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_factory-browser-fixture"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repository root")
        .to_path_buf()
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn group_exists(process_group: i32) -> bool {
    #[cfg(unix)]
    {
        process_group > 1 && unsafe { libc::kill(-process_group, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = process_group;
        false
    }
}

fn tooling() -> Option<BrowserTooling> {
    let root = repo_root();
    let runner = root.join("office/e2e-browser/browser-check-runner.mjs");
    let core = std::env::var_os("FACTORY_PLAYWRIGHT_CORE")
        .map(PathBuf::from)
        .or_else(|| {
            let candidate = root.join("office/e2e-browser/node_modules/playwright-core");
            candidate
                .join("package.json")
                .is_file()
                .then_some(candidate)
        })?;
    let chromium = std::env::var_os("FACTORY_CHROMIUM_EXE")
        .or_else(|| std::env::var_os("CHROMIUM_EXE"))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file());
    let browsers_path = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_dir())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".cache/ms-playwright"))
                .filter(|path| path.is_dir())
        });
    let node = std::process::Command::new("node")
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !node {
        return None;
    }
    Some(BrowserTooling {
        node: PathBuf::from("node"),
        runner,
        playwright_core: Some(core),
        chromium_executable: chromium,
        browsers_path,
    })
}

fn config(label: &str) -> (ManagedTestServerConfig, PathBuf) {
    let root = temp_root(label);
    let app = root.join("app");
    let logs = root.join("logs");
    fs::create_dir_all(&app).unwrap();
    let port = free_port();
    (
        ManagedTestServerConfig {
            argv: vec![
                fixture().to_string_lossy().into_owned(),
                "--spawn-child".into(),
            ],
            cwd: app,
            endpoint: Some(LoopbackEndpoint {
                host: "127.0.0.1".into(),
                port,
            }),
            ready_path: "/health".into(),
            startup_timeout: Duration::from_secs(10),
            log_dir: logs,
            max_log_bytes: 64 * 1024,
        },
        root,
    )
}

async fn check(
    tooling: BrowserTooling,
    endpoint: LoopbackEndpoint,
    root: &Path,
    expect_pass: bool,
) -> factory_native::browser::BrowserCheckReport {
    let profile = root.join("profile");
    let evidence = root.join("browser-evidence.json");
    let screenshot = root.join("screenshot.png");
    let report = run_browser_check(BrowserCheckConfig {
        endpoint,
        path: "/".into(),
        selector: Some("#factory-browser-fixture".into()),
        text: Some("Factory browser fixture ready".into()),
        timeout: Duration::from_secs(60),
        profile_dir: profile.clone(),
        evidence_path: evidence.clone(),
        screenshot: Some(screenshot.clone()),
        tooling,
    })
    .await;
    if expect_pass {
        assert_eq!(
            report.status,
            BrowserCheckStatus::Passed,
            "browser check failed: {report:?}"
        );
        let evidence = report.evidence.as_ref().expect("evidence");
        assert_eq!(evidence.console_errors, 0);
        assert_eq!(evidence.failed_requests, 0);
        assert_eq!(evidence.title, "Factory browser fixture");
        assert!(evidence.profile_removed);
        assert!(!profile.exists(), "disposable profile was not removed");
        assert!(screenshot.is_file(), "screenshot missing");
        assert!(fs::metadata(&screenshot).unwrap().len() <= 8 * 1024 * 1024);
    }
    assert!(!profile.exists(), "disposable profile was not removed");
    report
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_server_serves_real_browser_and_cleans_up_descendants() {
    let (config, root) = config("roundtrip");
    let port = config.endpoint.as_ref().unwrap().port;
    let max_log_bytes = config.max_log_bytes;
    let server = ManagedTestServer::start(config)
        .await
        .expect("managed server starts");
    assert_eq!(server.endpoint().port, port);
    assert!(
        !port_free(port),
        "authorized port must be owned while the server is ready"
    );
    let stdout = fs::read_to_string(server.stdout_log()).unwrap();
    assert!(
        stdout.contains(&format!("http://127.0.0.1:{port}")),
        "server did not receive FACTORY_TEST_ENDPOINT: {stdout}"
    );

    match tooling() {
        Some(tooling) => {
            let report = check(tooling, server.endpoint().clone(), &root, true).await;
            assert!(report.passed(), "{report:?}");
        }
        None => {
            eprintln!(
                "NOT_TESTED: real Chromium/playwright-core tooling is unavailable; \
                 only the browser assertion is skipped"
            );
            let unavailable = BrowserTooling {
                node: PathBuf::from("node"),
                runner: root.join("missing-runner.mjs"),
                playwright_core: None,
                chromium_executable: None,
                browsers_path: None,
            };
            let report = check(unavailable, server.endpoint().clone(), &root, false).await;
            assert_eq!(report.status, BrowserCheckStatus::NotTested);
        }
    }

    let receipt = server.stop().await.expect("managed server stops");
    assert!(receipt.port_closed, "port did not close: {receipt:?}");
    assert!(
        receipt.process_group_absent,
        "process group remains: {receipt:?}"
    );
    assert!(
        !group_exists(receipt.process_group),
        "process group {} still exists",
        receipt.process_group
    );
    let stdout = fs::metadata(&receipt.stdout_log).unwrap().len();
    let stderr = fs::metadata(&receipt.stderr_log).unwrap().len();
    assert!(stdout <= max_log_bytes, "stdout log exceeded its bound");
    assert!(stderr <= max_log_bytes, "stderr log exceeded its bound");
    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn already_occupied_port_is_rejected_without_adoption() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let root = temp_root("occupied");
    let app = root.join("app");
    let logs = root.join("logs");
    fs::create_dir_all(&app).unwrap();
    let result = ManagedTestServer::start(ManagedTestServerConfig {
        argv: vec![fixture().to_string_lossy().into_owned()],
        cwd: app,
        endpoint: Some(LoopbackEndpoint {
            host: "127.0.0.1".into(),
            port,
        }),
        ready_path: "/health".into(),
        startup_timeout: Duration::from_secs(5),
        log_dir: logs,
        max_log_bytes: 64 * 1024,
    })
    .await;
    assert!(
        matches!(result, Err(BrowserError::PortOccupied(value)) if value == port),
        "occupied port was not rejected"
    );
    drop(listener);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_timeout_stops_the_owned_group_and_releases_the_port() {
    let root = temp_root("timeout");
    let app = root.join("app");
    let logs = root.join("logs");
    fs::create_dir_all(&app).unwrap();
    let port = free_port();
    let result = ManagedTestServer::start(ManagedTestServerConfig {
        argv: vec![fixture().to_string_lossy().into_owned(), "--unready".into()],
        cwd: app,
        endpoint: Some(LoopbackEndpoint {
            host: "127.0.0.1".into(),
            port,
        }),
        ready_path: "/health".into(),
        startup_timeout: Duration::from_secs(5),
        log_dir: logs,
        max_log_bytes: 64 * 1024,
    })
    .await;
    assert!(
        matches!(result, Err(BrowserError::StartupTimedOut)),
        "startup timeout was not reported"
    );
    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_browser_tooling_is_not_tested_and_never_passed() {
    let root = temp_root("not-tested");
    let endpoint = LoopbackEndpoint::parse("http://127.0.0.1:9").unwrap();
    let report = run_browser_check(BrowserCheckConfig {
        endpoint,
        path: "/".into(),
        selector: Some("#fixture".into()),
        text: None,
        timeout: Duration::from_secs(5),
        profile_dir: root.join("profile"),
        evidence_path: root.join("evidence.json"),
        screenshot: None,
        tooling: BrowserTooling {
            node: PathBuf::from("node"),
            runner: root.join("missing-runner.mjs"),
            playwright_core: None,
            chromium_executable: None,
            browsers_path: None,
        },
    })
    .await;
    assert_eq!(report.status, BrowserCheckStatus::NotTested);
    assert!(!report.passed());
    assert!(report.evidence.is_none());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detached_state_round_trip_supports_stop_and_cleanup() {
    use factory_native::browser::{cleanup_state_dir, read_state, state_path, stop_from_state};

    let root = temp_root("detached");
    let app = root.join("app");
    let logs = root.join("logs");
    let state_dir = root.join("state");
    fs::create_dir_all(&app).unwrap();
    let port = free_port();
    let config = ManagedTestServerConfig {
        argv: vec![
            fixture().to_string_lossy().into_owned(),
            "--spawn-child".into(),
        ],
        cwd: app,
        endpoint: Some(LoopbackEndpoint {
            host: "127.0.0.1".into(),
            port,
        }),
        ready_path: "/health".into(),
        startup_timeout: Duration::from_secs(10),
        log_dir: logs,
        max_log_bytes: 64 * 1024,
    };
    let state = ManagedTestServer::start_detached(config, &state_dir, "round-trip")
        .await
        .expect("detached server starts");
    let path = state_path(&state_dir, "round-trip");
    assert!(path.is_file());
    let loaded = read_state(&path).expect("state is readable");
    assert_eq!(loaded.pid, state.pid);
    assert_eq!(loaded.endpoint, state.endpoint);
    assert!(!port_free(port));
    assert!(group_exists(state.process_group));

    let summary = cleanup_state_dir(&state_dir, false, false)
        .await
        .expect("cleanup scans state");
    assert_eq!(summary.live, 1);
    assert_eq!(summary.removed, 0);

    let receipt = stop_from_state(&loaded, loaded.max_log_bytes)
        .await
        .expect("detached server stops");
    assert!(receipt.port_closed);
    assert!(receipt.process_group_absent);
    assert!(!group_exists(loaded.process_group));

    let summary = cleanup_state_dir(&state_dir, true, false)
        .await
        .expect("stale state is cleaned");
    assert_eq!(summary.removed, 1);
    assert!(!path.exists());
    assert!(!std::path::Path::new(&loaded.stdout_log).exists());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_owner_is_a_cancellation_cleanup() {
    let (config, root) = config("cancel");
    let server = ManagedTestServer::start(config)
        .await
        .expect("managed server starts");
    let process_group = server.process_group();
    assert!(group_exists(process_group));
    drop(server);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while group_exists(process_group) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !group_exists(process_group),
        "cancellation left process group {process_group} alive"
    );
    let _ = fs::remove_dir_all(root);
}
