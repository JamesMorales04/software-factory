//! Real bubblewrap provider boundary tests. These are candidate-bound local
//! observations, not independent isolation certification.

use std::time::Duration;

use factory_native::{
    provider_egress::{EgressBroker, EgressConfig},
    provider_sandbox::{ProviderSandbox, ProviderSandboxConfig},
};

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

#[tokio::test]
async fn sandbox_identity_supports_confirmed_termination() {
    if !bwrap_available() {
        return;
    }
    let session_dir = std::env::temp_dir().join(format!("pi-{}", uuid::Uuid::new_v4().simple()));
    let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
        binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_factory-fixture-provider")),
        session_dir: session_dir.clone(),
        args: Vec::new(),
        ready_timeout: Duration::from_secs(10),
        ..Default::default()
    })
    .await
    .expect("start sandboxed fixture provider");
    assert_eq!(
        factory_native::provider_sandbox::terminate_sandbox_identity(&session_dir).unwrap(),
        factory_native::provider_sandbox::SandboxProcessState::Gone
    );
    assert_eq!(
        factory_native::provider_sandbox::verify_sandbox_identity(&session_dir).unwrap(),
        factory_native::provider_sandbox::SandboxProcessState::Gone
    );
    let _ = sandbox.stop().await;
    let _ = std::fs::remove_dir_all(session_dir);
}

#[tokio::test]
async fn confined_provider_reaches_only_the_scoped_egress_broker() {
    use std::sync::{Arc, Mutex};

    use axum::{Router, extract::State, http::HeaderMap, routing::post};

    #[derive(Clone)]
    struct Upstream {
        seen: Arc<Mutex<Vec<String>>>,
    }
    async fn response(
        State(state): State<Upstream>,
        headers: HeaderMap,
    ) -> axum::Json<serde_json::Value> {
        state.seen.lock().unwrap().push(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        );
        axum::Json(serde_json::json!({"id":"resp_test","output":[]}))
    }
    if !bwrap_available() {
        return;
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/responses", post(response))
        .with_state(Upstream { seen: seen.clone() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let session_dir = std::env::temp_dir().join(format!("pe-{}", uuid::Uuid::new_v4().simple()));
    let token = "egress-token-0123456789abcdef0123456789abcdef";
    let credential = "real-upstream-credential-0123456789";
    let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
        binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_factory-fixture-provider")),
        session_dir: session_dir.clone(),
        args: Vec::new(),
        ready_timeout: Duration::from_secs(10),
        egress: Some(
            EgressConfig::new(
                &format!("http://{upstream}"),
                credential.into(),
                token.into(),
                true,
            )
            .unwrap(),
        ),
        egress_bridge: Some(std::path::PathBuf::from(env!(
            "CARGO_BIN_EXE_factory-egress-bridge"
        ))),
        env: vec![("FACTORY_FIXTURE_PROVIDER_EGRESS_TEST".into(), "1".into())],
        ..Default::default()
    })
    .await
    .expect("start sandboxed fixture provider with egress");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !session_dir.join("egress_result.txt").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "egress result missing"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let result = std::fs::read_to_string(session_dir.join("egress_result.txt")).unwrap();
    assert!(result.contains("status=200"), "{result}");
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [format!("Bearer {credential}")]
    );
    let environment = std::fs::read_to_string(session_dir.join("environment.txt")).unwrap();
    assert!(!environment.contains(credential), "{environment}");
    assert!(
        environment.contains("OPENAI_API_KEY=<redacted>"),
        "{environment}"
    );
    sandbox.stop().await.unwrap();
    server.abort();
    let _ = std::fs::remove_dir_all(session_dir);
}

#[tokio::test]
async fn actual_opencode_executable_uses_the_egress_broker() {
    use std::sync::{Arc, Mutex};

    use axum::{Router, extract::State, http::HeaderMap, routing::post};
    use tokio::io::{AsyncBufReadExt, BufReader};

    let opencode = std::env::var("OPENCODE_BIN").ok().or_else(|| {
        std::process::Command::new("sh")
            .args(["-c", "command -v opencode"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    });
    let Some(opencode) = opencode.filter(|path| !path.is_empty()) else {
        eprintln!("skipping actual OpenCode test: OPENCODE_BIN not available");
        return;
    };
    #[derive(Clone)]
    struct Upstream {
        seen: Arc<Mutex<Vec<String>>>,
    }
    async fn chat(
        State(state): State<Upstream>,
        headers: HeaderMap,
    ) -> axum::Json<serde_json::Value> {
        state.seen.lock().unwrap().push(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        );
        axum::Json(serde_json::json!({
            "id":"chatcmpl-mock","object":"chat.completion","created":1,"model":"mock-model",
            "choices":[{"index":0,"message":{"role":"assistant","content":"hello from mock"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}
        }))
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .with_state(Upstream { seen: seen.clone() });
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = upstream_listener.local_addr().unwrap();
    let upstream_server = tokio::spawn(async move {
        axum::serve(upstream_listener, app).await.unwrap();
    });
    let socket = std::env::temp_dir().join(format!("oe-{}", uuid::Uuid::new_v4().simple()));
    let token = "egress-token-0123456789abcdef0123456789abcdef";
    let credential = "real-upstream-credential-0123456789";
    let config = EgressConfig::new(
        &format!("http://{upstream}"),
        credential.into(),
        token.into(),
        true,
    )
    .unwrap();
    let broker = EgressBroker::start(config, &socket).await.unwrap();
    let (bridge_address, bridge) = factory_native::provider_egress::start_loopback_bridge(&socket)
        .await
        .unwrap();
    let opencode_config = serde_json::json!({
        "provider": {
            "factorymock": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "Factory Mock",
                "options": {
                    "baseURL": format!("http://{bridge_address}/v1"),
                    "apiKey": token
                },
                "models": {
                    "mock-model": {"name":"Mock","limit":{"context":10000,"output":1000}}
                }
            }
        },
        "model": "factorymock/mock-model"
    });
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let mut child = tokio::process::Command::new(&opencode)
        .args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--pure",
        ])
        .env("OPENCODE_SERVER_PASSWORD", "test")
        .env(
            "OPENCODE_CONFIG_CONTENT",
            serde_json::to_string(&opencode_config).unwrap(),
        )
        .env("HOME", "/tmp")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("start OpenCode");
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let line = tokio::time::timeout(Duration::from_secs(1), lines.next_line())
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();
        if line
            .as_deref()
            .is_some_and(|line| line.contains("server listening"))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "OpenCode did not start"
        );
    }
    let base = format!("http://127.0.0.1:{port}");
    let client = factory_native::runtime::opencode_http_client(Duration::from_secs(30)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let health = client
            .get(format!("{base}/global/health"))
            .basic_auth("opencode", Some("test"))
            .send()
            .await;
        if health.is_ok_and(|response| response.status().is_success()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "OpenCode health did not become ready"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let created = factory_native::runtime::create_opencode_session(
        &client,
        &base,
        "test",
        "/tmp",
        "Factory OpenCode compatibility",
    )
    .await
    .expect("create OpenCode session");
    let session_id = created["id"].as_str().unwrap().to_owned();
    factory_native::runtime::submit_opencode_prompt(
        &client,
        &base,
        "test",
        "/tmp",
        &session_id,
        &serde_json::json!({
            "model": {"providerID":"factorymock","modelID":"mock-model"},
            "parts": [{"type":"text","text":"hello"}]
        }),
    )
    .await
    .expect("submit OpenCode prompt");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while seen.lock().unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "OpenCode did not reach mock upstream"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty(), "OpenCode made no model request");
    assert!(
        seen.iter()
            .all(|value| value == &format!("Bearer {credential}")),
        "OpenCode requests did not use the injected credential: {seen:?}"
    );
    let _ = factory_native::runtime::delete_opencode_session(
        &client,
        &base,
        "test",
        "/tmp",
        &session_id,
    )
    .await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    bridge.abort();
    broker.stop();
    upstream_server.abort();
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn confined_recovery_requires_confirmed_process_absence() {
    if !bwrap_available() {
        return;
    }
    let root = std::env::temp_dir().join(format!("pr-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&root).unwrap();
    let session_dir = root.join("session");
    let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
        binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_factory-fixture-provider")),
        session_dir: session_dir.clone(),
        args: Vec::new(),
        ready_timeout: Duration::from_secs(10),
        ..Default::default()
    })
    .await
    .expect("start sandboxed fixture provider");
    assert_eq!(
        factory_native::provider_sandbox::confined_recovery_receipt(
            session_dir.to_str().unwrap(),
            "ses_recovery",
            &root,
        )
        .unwrap(),
        "sandbox_process_terminated_verified"
    );
    assert_eq!(
        factory_native::provider_sandbox::verify_sandbox_identity(&session_dir).unwrap(),
        factory_native::provider_sandbox::SandboxProcessState::Gone
    );
    let _ = sandbox.stop().await;
    assert_eq!(
        factory_native::provider_sandbox::confined_recovery_receipt(
            session_dir.to_str().unwrap(),
            "ses_recovery",
            &root,
        )
        .unwrap(),
        "sandbox_process_identity_absent"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn sandbox_kills_a_provider_that_exceeds_the_session_quota() {
    if !bwrap_available() {
        return;
    }
    let session_dir = std::env::temp_dir().join(format!("pq-{}", uuid::Uuid::new_v4().simple()));
    let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
        binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_factory-fixture-provider")),
        session_dir: session_dir.clone(),
        args: Vec::new(),
        ready_timeout: Duration::from_secs(10),
        max_session_bytes: 1024 * 1024,
        max_file_bytes: 64 * 1024 * 1024,
        max_lifetime: Duration::from_secs(30),
        bridge_timeout: Duration::from_secs(30),
        env: vec![(
            "FACTORY_FIXTURE_PROVIDER_FLOOD_BYTES".into(),
            (8 * 1024 * 1024).to_string(),
        )],
        ..Default::default()
    })
    .await
    .expect("start sandboxed fixture provider");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(sandbox.limit_exceeded(), "provider quota was not enforced");
    assert!(matches!(
        sandbox.stop().await,
        Err(factory_native::provider_sandbox::SandboxError::LimitExceeded)
    ));
    let _ = std::fs::remove_dir_all(session_dir);
}

#[tokio::test]
async fn sandbox_broker_requires_capability_and_rejects_unbounded_paths() {
    if !bwrap_available() {
        return;
    }
    let session_dir = std::env::temp_dir().join(format!("ps-{}", uuid::Uuid::new_v4().simple()));
    let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
        binary: std::path::PathBuf::from(env!("CARGO_BIN_EXE_factory-fixture-provider")),
        session_dir: session_dir.clone(),
        args: Vec::new(),
        ready_timeout: Duration::from_secs(10),
        ..Default::default()
    })
    .await
    .expect("start sandboxed fixture provider");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let base = sandbox.base_url();
    let password = sandbox.password();

    assert_eq!(
        client
            .get(format!("{base}/global/health"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{base}/global/health"))
            .basic_auth("opencode", Some("wrong-password"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/workspaces"))
            .basic_auth("opencode", Some(password))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    let health: serde_json::Value = client
        .get(format!("{base}/global/health"))
        .basic_auth("opencode", Some(password))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["healthy"], true);

    let probe = std::fs::read_to_string(session_dir.join("probe.txt")).unwrap();
    assert!(probe.contains("host_home_exists=false"), "{probe}");
    assert!(probe.contains("podman_socket_exists=false"), "{probe}");
    assert!(probe.contains("network_egress=false"), "{probe}");
    let environment = std::fs::read_to_string(session_dir.join("environment.txt")).unwrap();
    for forbidden in [
        "FACTORY_PROVIDER_PASSWORD",
        "FACTORY_OPERATOR_TOKEN",
        "FACTORY_WORKER_BOOTSTRAP_TOKEN",
        "FACTORY_CORE_URL",
    ] {
        assert!(
            !environment.contains(forbidden),
            "sandbox environment leaked {forbidden}: {environment}"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&session_dir)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    assert_eq!(
        factory_native::provider_sandbox::verify_sandbox_identity(&session_dir).unwrap(),
        factory_native::provider_sandbox::SandboxProcessState::Alive
    );
    assert!(
        !session_dir.join("sandbox.json").exists(),
        "provider identity must not live in the writable session directory"
    );
    sandbox.stop().await.unwrap();
    assert_eq!(
        factory_native::provider_sandbox::verify_sandbox_identity(&session_dir).unwrap(),
        factory_native::provider_sandbox::SandboxProcessState::Gone
    );
    let _ = std::fs::remove_dir_all(session_dir);
}
