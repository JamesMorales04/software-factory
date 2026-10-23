use std::{path::Path, process::Stdio, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    net::TcpListener,
    process::{Child, Command},
    time,
};
use uuid::Uuid;

use crate::protocol::{JsonRpc, RpcError};

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
    })
}

pub async fn probe_opencode(workspace: &Path) -> Result<ProbeResult, Error> {
    let version = binary_version("opencode").await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let password = Uuid::new_v4().to_string();
    let mut child = Command::new("opencode")
        .args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--pure",
        ])
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let outcome = async {
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
        Ok::<_, Error>((reported_version, sha256_hex(&schema_bytes)))
    }
    .await;
    let (reported_version, schema_sha256) = finish_probe(&mut child, outcome).await?;
    Ok(ProbeResult {
        provider: "opencode".into(),
        version: reported_version,
        protocol: "HTTP basic-auth health/OpenAPI".into(),
        healthy: true,
        paid_turn_tested: false,
        cleanup_confirmed: true,
        schema_sha256: Some(schema_sha256),
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
