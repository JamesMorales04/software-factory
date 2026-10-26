//! Constrained provider egress broker tests using a local fake upstream.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use factory_native::{
    attempt_grant::request_digest,
    provider_egress::{
        EgressBroker, EgressConfig, SharedTurnPolicy, TurnPolicy, start_loopback_bridge,
    },
};

#[derive(Clone)]
struct Upstream {
    seen: Arc<Mutex<Vec<String>>>,
}

async fn responses(State(state): State<Upstream>, headers: HeaderMap, body: String) -> Response {
    state.seen.lock().unwrap().push(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned(),
    );
    if body.contains("redirect") {
        return StatusCode::FOUND.into_response();
    }
    axum::Json(serde_json::json!({
        "id": "resp_test",
        "output": [{"type":"message","content":[{"type":"output_text","text":"ok"}]}]
    }))
    .into_response()
}

#[tokio::test]
async fn broker_injects_credential_and_restricts_routes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/responses", post(responses))
        .with_state(Upstream { seen: seen.clone() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let socket = std::env::temp_dir().join(format!("eg-{}", uuid::Uuid::new_v4().simple()));
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
    let (address, bridge) = start_loopback_bridge(&socket).await.unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let base = format!("http://{address}");
    let response = client
        .post(format!("{base}/v1/responses"))
        .bearer_auth(token)
        .json(&serde_json::json!({"model":"fake","input":"hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [format!("Bearer {credential}")]
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/responses"))
            .bearer_auth("wrong-token")
            .json(&serde_json::json!({"model":"fake","input":"hello"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/embeddings"))
            .bearer_auth(token)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/responses"))
            .bearer_auth(token)
            .json(&serde_json::json!({"redirect":true}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_GATEWAY
    );
    bridge.abort();
    broker.stop();
    server.abort();
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn broker_enforces_a_core_turn_policy_and_rewrites_output() {
    use axum::{Router, extract::State, http::HeaderMap, routing::post};
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Upstream {
        seen: Arc<Mutex<Vec<(String, String)>>>,
    }
    async fn responses(
        State(state): State<Upstream>,
        headers: HeaderMap,
        body: String,
    ) -> axum::Json<serde_json::Value> {
        state.seen.lock().unwrap().push((
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
            body,
        ));
        axum::Json(
            serde_json::json!({"id":"resp","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}),
        )
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/responses", post(responses))
        .with_state(Upstream { seen: seen.clone() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let body = serde_json::json!({"model":"mock-model","input":"hello"});
    let policy: SharedTurnPolicy = Arc::new(Mutex::new(Some(TurnPolicy {
        reservation_id: "reservation".into(),
        max_input_tokens: 100,
        max_output_tokens: 5,
        max_total_tokens: 105,
        max_cost_micros: 1000,
        model: "mock-model".into(),
        request_sha256: request_digest(&body).unwrap(),
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
        requests: 0,
        max_requests: 2,
        strict_request_digest: true,
    })));
    let socket = std::env::temp_dir().join(format!("tp-{}", uuid::Uuid::new_v4().simple()));
    let config = EgressConfig::new(
        &format!("http://{upstream}"),
        "real-upstream-credential-0123456789".into(),
        "egress-token-0123456789abcdef0123456789abcdef".into(),
        true,
    )
    .unwrap()
    .with_turn_policy(policy);
    let broker = EgressBroker::start(config, &socket).await.unwrap();
    let (address, bridge) = start_loopback_bridge(&socket).await.unwrap();
    let client = reqwest::Client::new();
    let url = format!("http://{address}/v1/responses");
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("egress-token-0123456789abcdef0123456789abcdef")
            .json(&serde_json::json!({"model":"other","input":"hello"}))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("egress-token-0123456789abcdef0123456789abcdef")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("egress-token-0123456789abcdef0123456789abcdef")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("egress-token-0123456789abcdef0123456789abcdef")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::TOO_MANY_REQUESTS
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert!(
        seen.iter()
            .all(|(auth, _)| auth == "Bearer real-upstream-credential-0123456789")
    );
    for (_, body) in seen {
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["max_output_tokens"], 5);
    }
    bridge.abort();
    broker.stop();
    server.abort();
    let _ = std::fs::remove_file(socket);
}

#[tokio::test]
async fn actual_codex_executable_uses_the_egress_broker() {
    let Some(codex) = std::env::var("CODEX_BIN")
        .ok()
        .filter(|path| !path.is_empty())
    else {
        eprintln!("skipping actual Codex test: CODEX_BIN not available");
        return;
    };
    use axum::{
        Router,
        extract::State,
        http::HeaderMap,
        response::{IntoResponse, Response},
        routing::post,
    };
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Upstream {
        seen: Arc<Mutex<Vec<String>>>,
    }
    async fn responses(State(state): State<Upstream>, headers: HeaderMap) -> Response {
        state.seen.lock().unwrap().push(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        );
        let response = serde_json::json!({
            "id":"resp_mock","object":"response","created_at":1,"status":"completed","model":"mock-model",
            "output":[{"type":"message","id":"msg_mock","status":"completed","role":"assistant","content":[{"type":"output_text","text":"hello from mock","annotations":[]}]}],
            "usage":{"input_tokens":3,"output_tokens":4,"total_tokens":7}
        });
        let event = format!(
            "event: response.completed\ndata: {}\n\n",
            serde_json::json!({"type":"response.completed","response":response})
        );
        let mut response = Response::new(axum::body::Body::from(event));
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        response.into_response()
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/responses", post(responses))
        .with_state(Upstream { seen: seen.clone() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let socket = std::env::temp_dir().join(format!("cx-{}", uuid::Uuid::new_v4().simple()));
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
    let (address, bridge) = start_loopback_bridge(&socket).await.unwrap();
    let work = std::env::temp_dir().join(format!("cw-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&work).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(40),
        tokio::process::Command::new(&codex)
            .args([
                "exec",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "-c",
                "model_provider=mock",
                "-c",
                &format!("model_providers.mock={{name=\"Mock\",base_url=\"http://{address}/v1\",env_key=\"MOCK_API_KEY\",wire_api=\"responses\"}}"),
                "-m",
                "mock-model",
                "say hello",
            ])
            .current_dir(&work)
            .env("MOCK_API_KEY", token)
            .env("HOME", &work)
            .output(),
    )
    .await
    .expect("Codex timed out")
    .expect("Codex failed");
    assert!(
        output.status.success(),
        "Codex exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty(), "Codex made no model request");
    assert!(
        seen.iter()
            .all(|value| value == &format!("Bearer {credential}")),
        "Codex requests did not use the injected credential: {seen:?}"
    );
    bridge.abort();
    broker.stop();
    server.abort();
    let _ = std::fs::remove_file(socket);
    let _ = std::fs::remove_dir_all(work);
}

#[test]
fn rejects_private_and_metadata_upstreams_without_test_opt_in() {
    for upstream in [
        "http://127.0.0.1:1",
        "http://169.254.169.254",
        "http://10.0.0.1",
        "http://[::1]:1",
    ] {
        assert!(
            EgressConfig::new(
                upstream,
                "real-upstream-credential-0123456789".into(),
                "egress-token-0123456789abcdef0123456789abcdef".into(),
                false,
            )
            .is_err(),
            "upstream {upstream} must be rejected"
        );
    }
}
