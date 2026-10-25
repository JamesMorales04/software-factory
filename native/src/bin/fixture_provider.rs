//! Deterministic OpenCode-compatible provider fixture for confined execution.
//! It contains no provider credentials and only writes inside its private
//! session directory.

use std::{
    convert::Infallible,
    env,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use tokio::sync::broadcast;

#[derive(Clone)]
struct Fixture {
    sender: broadcast::Sender<Vec<u8>>,
    deleted: Arc<AtomicBool>,
    step: Arc<AtomicUsize>,
    directory: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = env::var("FACTORY_PROVIDER_SOCKET")?;
    let directory = env::var("FACTORY_FIXTURE_PROVIDER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/session"));
    std::fs::create_dir_all(&directory)?;
    if let Ok(value) = env::var("FACTORY_FIXTURE_PROVIDER_FLOOD_BYTES") {
        let bytes: usize = value.parse()?;
        std::fs::write(directory.join("flood.bin"), vec![0u8; bytes])?;
    }
    write_environment(&directory)?;
    write_probe(&directory);
    let (sender, _) = broadcast::channel(64);
    let state = Fixture {
        sender,
        deleted: Arc::new(AtomicBool::new(false)),
        step: Arc::new(AtomicUsize::new(0)),
        directory: directory.clone(),
    };
    let app = Router::new()
        .route("/global/health", get(health))
        .route("/event", get(events))
        .route("/session", post(create_session))
        .route("/session/{id}/prompt_async", post(prompt))
        .route("/session/{id}/message", get(message))
        .route("/session/{id}", get(get_session).delete(delete_session))
        .with_state(state);
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    }
    if env::var("FACTORY_FIXTURE_PROVIDER_EGRESS_TEST")
        .ok()
        .as_deref()
        == Some("1")
    {
        let directory = directory.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let base = env::var("OPENAI_BASE_URL").unwrap_or_default();
            let key = env::var("OPENAI_API_KEY").unwrap_or_default();
            let result = reqwest::Client::new()
                .post(format!("{base}/responses"))
                .bearer_auth(key)
                .json(&json!({"model":"fake","input":"hello"}))
                .send()
                .await;
            let text = match result {
                Ok(response) => format!("status={}\n", response.status()),
                Err(error) => format!("error={error}\n"),
            };
            let _ = std::fs::write(directory.join("egress_result.txt"), text);
        });
    }
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({"healthy": true, "version": "fixture-1"}))
}

async fn events(State(state): State<Fixture>) -> Response {
    let receiver = state.sender.subscribe();
    let initial = stream::once(async {
        Ok::<_, Infallible>(axum::body::Bytes::from_static(
            b"data: {\"type\":\"server.connected\"}\n\n",
        ))
    });
    let rest = stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Ok(chunk) => Some((
                Ok::<_, Infallible>(axum::body::Bytes::from(chunk)),
                receiver,
            )),
            Err(_) => None,
        }
    });
    let body = Body::from_stream(initial.chain(rest));
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/event-stream"),
    );
    response
}

async fn create_session(
    State(state): State<Fixture>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let directory = query.get("directory").cloned().unwrap_or_default();
    let _ = state.sender.send(
        b"data: {\"type\":\"session.created\",\"properties\":{\"sessionID\":\"ses_fixture\",\"info\":{\"id\":\"ses_fixture\"}}}\n\n"
            .to_vec(),
    );
    Json(json!({"id":"ses_fixture","directory":directory}))
}

async fn prompt(State(state): State<Fixture>, Json(body): Json<Value>) -> StatusCode {
    append_prompt(&state.directory, &body);
    state.step.fetch_add(1, Ordering::SeqCst);
    let _ = state.sender.send(
        b"data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_fixture\"}}\n\n"
            .to_vec(),
    );
    StatusCode::NO_CONTENT
}

async fn message(State(state): State<Fixture>) -> Response {
    if state.deleted.load(Ordering::SeqCst) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let step = state.step.load(Ordering::SeqCst);
    let text = match step {
        1 => {
            "{\"type\":\"tool_request\",\"call_id\":\"call-1\",\"tool\":\"read_file\",\"path\":\"src/seed.txt\"}"
        }
        2 => {
            "{\"type\":\"tool_request\",\"call_id\":\"call-2\",\"tool\":\"write_file\",\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}"
        }
        3 => {
            "{\"type\":\"tool_request\",\"call_id\":\"call-3\",\"tool\":\"run_command\",\"argv\":[\"grep\",\"-q\",\"provider candidate\",\"src/provider.txt\"],\"timeout_secs\":30}"
        }
        _ => "{\"type\":\"final\",\"files\":[]}",
    };
    Json(json!([{
        "info": {
            "id": format!("msg_fixture_{step}"),
            "role": "assistant",
            "sessionID": "ses_fixture",
            "tokens": {"input": 10, "output": 20, "total": 30},
            "cost": 0.0001
        },
        "parts": [{"type": "text", "text": text}]
    }]))
    .into_response()
}

async fn delete_session(State(state): State<Fixture>) -> Json<bool> {
    state.deleted.store(true, Ordering::SeqCst);
    Json(true)
}

async fn get_session(State(state): State<Fixture>) -> Response {
    if state.deleted.load(Ordering::SeqCst) {
        StatusCode::NOT_FOUND.into_response()
    } else {
        Json(json!({"id":"ses_fixture"})).into_response()
    }
}

fn append_prompt(directory: &std::path::Path, body: &Value) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("prompts.jsonl"))
    {
        let _ = writeln!(file, "{body}");
    }
}

fn write_environment(directory: &std::path::Path) -> Result<(), std::io::Error> {
    let mut entries: Vec<_> = env::vars().collect();
    entries.sort();
    let text = entries
        .into_iter()
        .map(|(key, value)| {
            if ["PASSWORD", "TOKEN", "KEY", "SECRET"]
                .iter()
                .any(|needle| key.contains(needle))
            {
                format!("{key}=<redacted>")
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(directory.join("environment.txt"), format!("{text}\n"))
}

fn write_probe(directory: &std::path::Path) {
    let home = env::var("HOME").unwrap_or_default();
    let host_home = "/home";
    let podman_socket = "/run/user";
    let network = std::net::TcpStream::connect_timeout(
        &"1.1.1.1:80".parse().expect("static address"),
        Duration::from_millis(500),
    )
    .is_ok();
    let text = format!(
        "home={home}\nhost_home_exists={}\npodman_socket_exists={}\nnetwork_egress={network}\n",
        std::path::Path::new(host_home).exists(),
        std::path::Path::new(podman_socket).exists(),
    );
    let _ = std::fs::write(directory.join("probe.txt"), text);
}
