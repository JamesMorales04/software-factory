use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    process::Stdio,
    time::Duration,
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time,
};
use uuid::Uuid;

use crate::protocol::{JsonRpc, RpcError};
use crate::sse::SseParser;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("process I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Codex protocol: {0}")]
    Rpc(#[from] RpcError),
    #[error("OpenCode HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider probe timed out")]
    Timeout,
    #[error("provider protocol incompatibility: {0}")]
    Incompatible(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub provider: String,
    pub version: String,
    pub protocol: String,
    pub healthy: bool,
    pub paid_turn_tested: bool,
    pub cleanup_confirmed: bool,
    pub schema_sha256: Option<String>,
    pub session_lifecycle_tested: bool,
    pub session_cleanup_confirmed: Option<bool>,
    pub sse_handshake_tested: bool,
}

pub async fn probe_codex(workspace: &Path) -> Result<ProbeResult, Error> {
    let version = binary_version("codex").await?;
    let mut child = Command::new("codex")
        .arg("app-server")
        .arg("--listen")
        .arg("stdio://")
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let outcome = async {
        let stdin = child.stdin.take().ok_or_else(|| Error::Incompatible("Codex stdin unavailable".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Incompatible("Codex stdout unavailable".into()))?;
        let mut rpc = JsonRpc::new(stdout, stdin);
        let (initialized, models) = time::timeout(Duration::from_secs(15), async {
            let result = rpc.request("initialize", json!({
                "clientInfo":{"name":"software_factory","title":"Software Factory","version":"0.1.0"},
                "capabilities":{}
            })).await?;
            rpc.notify("initialized", json!({})).await?;
            let models = rpc.request("model/list", json!({})).await?;
            Ok::<_, RpcError>((result, models))
        }).await.map_err(|_| Error::Timeout)??;
        if !initialized.is_object() || !models.is_object() {
            return Err(Error::Incompatible("unexpected initialize or model/list response".into()));
        }
        Ok::<_, Error>(())
    }.await;
    finish_probe(&mut child, outcome).await?;
    Ok(ProbeResult {
        provider: "codex".into(),
        version: version.trim().into(),
        protocol: "app-server stdio initialize/model-list".into(),
        healthy: true,
        paid_turn_tested: false,
        cleanup_confirmed: true,
        schema_sha256: None,
        session_lifecycle_tested: false,
        session_cleanup_confirmed: None,
        sse_handshake_tested: false,
    })
}

pub async fn probe_opencode(workspace: &Path) -> Result<ProbeResult, Error> {
    let version = binary_version("opencode").await?;
    let password = Uuid::new_v4().to_string();
    let mut child = Command::new("opencode")
        .args(["serve", "--hostname", "127.0.0.1", "--port", "0", "--pure"])
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let outcome = async {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Incompatible("OpenCode stdout unavailable".into()))?;
        let mut stdout = BufReader::new(stdout);
        let address = time::timeout(Duration::from_secs(15), async {
            for _ in 0..20 {
                let mut line = String::new();
                if stdout.read_line(&mut line).await? == 0 {
                    return Err(Error::Incompatible(
                        "OpenCode closed startup output before listen".into(),
                    ));
                }
                if let Some(value) = line
                    .trim()
                    .strip_prefix("opencode server listening on http://")
                {
                    let address: SocketAddr = value.parse().map_err(|_| {
                        Error::Incompatible("invalid OpenCode listen address".into())
                    })?;
                    if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
                        return Err(Error::Incompatible(
                            "OpenCode did not bind IPv4 loopback".into(),
                        ));
                    }
                    return Ok(address);
                }
            }
            Err(Error::Incompatible(
                "OpenCode startup exceeded 20 output lines".into(),
            ))
        })
        .await
        .map_err(|_| Error::Timeout)??;
        let port = address.port();
        let url = format!("http://127.0.0.1:{port}/global/health");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?;
        let deadline = time::Instant::now() + Duration::from_secs(15);
        let health = loop {
            if time::Instant::now() > deadline {
                return Err(Error::Timeout);
            }
            if let Ok(response) = client
                .get(&url)
                .basic_auth("opencode", Some(&password))
                .send()
                .await
                && response.status().is_success()
            {
                break response.json::<serde_json::Value>().await?;
            }
            if let Some(status) = child.try_wait()? {
                return Err(Error::Incompatible(format!(
                    "OpenCode exited during startup: {status}"
                )));
            }
            time::sleep(Duration::from_millis(100)).await;
        };
        if health.get("healthy").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(Error::Incompatible(
                "health response did not confirm healthy".into(),
            ));
        }
        let reported_version = health
            .get("version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(version.trim())
            .to_string();
        let schema_bytes = client
            .get(format!("http://127.0.0.1:{port}/doc"))
            .basic_auth("opencode", Some(&password))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let schema: serde_json::Value = serde_json::from_slice(&schema_bytes)
            .map_err(|_| Error::Incompatible("OpenCode /doc is not JSON".into()))?;
        if !schema
            .get("openapi")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.starts_with("3."))
        {
            return Err(Error::Incompatible("OpenCode /doc is not OpenAPI 3".into()));
        }
        let workspace = workspace.canonicalize()?.to_string_lossy().to_string();
        let event_response = client
            .get(format!("http://127.0.0.1:{port}/event"))
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .send()
            .await?
            .error_for_status()?;
        if !event_response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            return Err(Error::Incompatible(
                "OpenCode /event did not return SSE".into(),
            ));
        }
        let mut stream = event_response.bytes_stream();
        let mut parser = SseParser::new(1_048_576);
        time::timeout(Duration::from_secs(5), async {
            for _ in 0..64 {
                let chunk = stream.next().await.ok_or_else(|| {
                    Error::Incompatible("OpenCode SSE ended before first event".into())
                })??;
                for event in parser
                    .push(&chunk)
                    .map_err(|error| Error::Incompatible(error.to_string()))?
                {
                    let body: serde_json::Value = serde_json::from_str(&event.data)
                        .map_err(|_| Error::Incompatible("OpenCode SSE data is not JSON".into()))?;
                    if body.get("type").and_then(serde_json::Value::as_str)
                        == Some("server.connected")
                    {
                        return Ok(());
                    }
                }
            }
            Err(Error::Incompatible(
                "OpenCode SSE first event not found within 64 chunks".into(),
            ))
        })
        .await
        .map_err(|_| Error::Timeout)??;
        let session_url = format!("http://127.0.0.1:{port}/session");
        let created: serde_json::Value = client
            .post(&session_url)
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .json(&json!({"title":"Factory protocol preflight"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let session_id = created
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Incompatible("OpenCode session/create returned no id".into()))?
            .to_string();
        let item_url = format!("{session_url}/{session_id}");
        let inspected = client
            .get(&item_url)
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .send()
            .await;
        let deleted = client
            .delete(&item_url)
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .send()
            .await;
        let deleted = deleted.map_err(|error| {
            Error::Incompatible(format!(
                "OpenCode session {session_id} cleanup unconfirmed: {error}"
            ))
        })?;
        if !deleted.status().is_success() || !(deleted.json::<bool>().await?) {
            return Err(Error::Incompatible(format!(
                "OpenCode session {session_id} cleanup unconfirmed"
            )));
        }
        let inspected: serde_json::Value = inspected?.error_for_status()?.json().await?;
        if inspected.get("id").and_then(serde_json::Value::as_str) != Some(&session_id) {
            return Err(Error::Incompatible(
                "OpenCode session inspection identity mismatch".into(),
            ));
        }
        if created.get("directory").and_then(serde_json::Value::as_str) != Some(workspace.as_str())
            || inspected
                .get("directory")
                .and_then(serde_json::Value::as_str)
                != Some(workspace.as_str())
        {
            return Err(Error::Incompatible(
                "OpenCode session workspace identity mismatch".into(),
            ));
        }
        Ok::<_, Error>((reported_version, sha256_hex(&schema_bytes)))
    }
    .await;
    let (reported_version, schema_sha256) = finish_probe(&mut child, outcome).await?;
    Ok(ProbeResult {
        provider: "opencode".into(),
        version: reported_version,
        protocol: "HTTP basic-auth health/OpenAPI/SSE/session".into(),
        healthy: true,
        paid_turn_tested: false,
        cleanup_confirmed: true,
        schema_sha256: Some(schema_sha256),
        session_lifecycle_tested: true,
        session_cleanup_confirmed: Some(true),
        sse_handshake_tested: true,
    })
}

async fn finish_probe<T>(child: &mut Child, outcome: Result<T, Error>) -> Result<T, Error> {
    let cleanup = async {
        if child.try_wait()?.is_none() {
            child.start_kill()?;
        }
        time::timeout(Duration::from_secs(5), child.wait())
            .await
            .map_err(|_| Error::Timeout)??;
        Ok::<_, Error>(())
    }
    .await;
    match (outcome, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(Error::Incompatible(format!(
            "probe cleanup unconfirmed: {cleanup}"
        ))),
        (Err(error), Err(cleanup)) => Err(Error::Incompatible(format!(
            "{error}; probe cleanup unconfirmed: {cleanup}"
        ))),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn binary_version(binary: &str) -> Result<String, Error> {
    let output = Command::new(binary).arg("--version").output().await?;
    if !output.status.success() {
        return Err(Error::Incompatible(format!("{binary} --version failed")));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| Error::Incompatible("version output encoding".into()))
}
