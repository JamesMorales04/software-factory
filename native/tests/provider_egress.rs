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
use factory_native::provider_egress::{EgressBroker, EgressConfig, start_loopback_bridge};

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
