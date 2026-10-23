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

use crate::protocol::{
    JsonRpc, RpcError, ThreadListParams, ThreadLoadedListParams, ThreadSummary,
    TurnCompletedParams, TurnWaitOutcome,
};
use crate::sse::{OpenCodeEvent, SseParser};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "classification", rename_all = "snake_case")]
pub enum CodexThreadReconciliation {
    LoadedAndReadableInProbeServer { thread: ThreadSummary },
    IndexedButNotLoadedInProbeServer { thread: ThreadSummary },
    LoadedInProbeServerButNotIndexed,
    NotFoundInStateDbIndex { loaded_in_probe_server: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexTurnAwait {
    Terminal(TurnCompletedParams),
    InterruptRequestedAfterTimeout { response: serde_json::Value },
}

/// Waits for the terminal notification for an exact turn identity. On timeout it requests
/// interruption with that same identity, but does not classify the turn as terminal.
pub async fn await_codex_turn<R, W>(
    rpc: &mut JsonRpc<R, W>,
    thread_id: &str,
    turn_id: &str,
    timeout: Duration,
) -> Result<CodexTurnAwait, RpcError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    match rpc
        .wait_for_turn_or_interrupt_on_timeout(thread_id, turn_id, timeout)
        .await?
    {
        TurnWaitOutcome::Completed(turn) => Ok(CodexTurnAwait::Terminal(turn)),
        TurnWaitOutcome::TimeoutInterruptRequested { interrupt_response } => {
            Ok(CodexTurnAwait::InterruptRequestedAfterTimeout {
                response: interrupt_response,
            })
        }
    }
}

/// Reads Codex App Server thread metadata only. This never resumes a thread or starts a turn.
/// Loaded state refers only to the newly spawned probe server. It cannot establish whether
/// another App Server still owns the thread. Index absence is not proof of cleanup.
pub async fn reconcile_codex_thread(
    workspace: &Path,
    thread_id: &str,
) -> Result<CodexThreadReconciliation, Error> {
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Incompatible("Codex stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Incompatible("Codex stdout unavailable".into()))?;
        let mut rpc = JsonRpc::new(stdout, stdin);
        time::timeout(Duration::from_secs(15), async {
            let initialized = rpc
                .request(
                    "initialize",
                    json!({
                        "clientInfo":{"name":"software_factory_readonly","title":"Software Factory read-only reconciliation","version":"0.1.0"},
                        "capabilities":{}
                    }),
                )
                .await?;
            if !initialized.is_object() {
                return Err(RpcError::Protocol("unexpected initialize response".into()));
            }
            rpc.notify("initialized", json!({})).await?;

            let loaded = collect_loaded_threads(&mut rpc, thread_id).await?;
            let indexed = collect_indexed_thread(&mut rpc, thread_id).await?;
            match (indexed, loaded) {
                (Some(thread), false) => {
                    Ok(CodexThreadReconciliation::IndexedButNotLoadedInProbeServer {
                        thread,
                    })
                }
                (Some(_), true) => {
                    let read = rpc.read_thread(thread_id, false).await?;
                    Ok(CodexThreadReconciliation::LoadedAndReadableInProbeServer {
                        thread: read.thread,
                    })
                }
                (None, true) => Ok(CodexThreadReconciliation::LoadedInProbeServerButNotIndexed),
                (None, false) => Ok(CodexThreadReconciliation::NotFoundInStateDbIndex {
                    loaded_in_probe_server: false,
                }),
            }
        })
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(Error::Rpc)
    }
    .await;
    finish_probe(&mut child, outcome).await
}

async fn collect_loaded_threads<R, W>(
    rpc: &mut JsonRpc<R, W>,
    thread_id: &str,
) -> Result<bool, RpcError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut cursor = None;
    for _ in 0..100 {
        let page = rpc
            .list_loaded_threads(ThreadLoadedListParams {
                cursor: cursor.clone(),
                limit: Some(100),
            })
            .await?;
        if page.data.iter().any(|id| id == thread_id) {
            return Ok(true);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(false);
        }
    }
    Err(RpcError::Protocol(
        "thread/loaded/list exceeded 100 pages".into(),
    ))
}

async fn collect_indexed_thread<R, W>(
    rpc: &mut JsonRpc<R, W>,
    thread_id: &str,
) -> Result<Option<ThreadSummary>, RpcError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    for archived in [false, true] {
        if let Some(thread) = find_indexed_thread(rpc, thread_id, archived).await? {
            return Ok(Some(thread));
        }
    }
    Ok(None)
}

async fn find_indexed_thread<R, W>(
    rpc: &mut JsonRpc<R, W>,
    thread_id: &str,
    archived: bool,
) -> Result<Option<ThreadSummary>, RpcError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    const SOURCE_KINDS: &[&str] = &[
        "cli",
        "vscode",
        "exec",
        "appServer",
        "subAgent",
        "subAgentReview",
        "subAgentCompact",
        "subAgentThreadSpawn",
        "subAgentOther",
        "unknown",
    ];
    let mut cursor = None;
    for _ in 0..100 {
        let page = rpc
            .list_threads(ThreadListParams {
                cursor: cursor.clone(),
                limit: Some(100),
                archived,
                source_kinds: SOURCE_KINDS.to_vec(),
                use_state_db_only: true,
            })
            .await?;
        if let Some(thread) = page.data.into_iter().find(|thread| thread.id == thread_id) {
            return Ok(Some(thread));
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(None);
        }
    }
    Err(RpcError::Protocol("thread/list exceeded 100 pages".into()))
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
                    let body = OpenCodeEvent::parse(&event.data)
                        .map_err(|error| Error::Incompatible(error.to_string()))?;
                    if body.kind == "server.connected" {
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
        let status = client
            .get(format!("http://127.0.0.1:{port}/session/status"))
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .send()
            .await;
        let messages = client
            .get(format!("{item_url}/message"))
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .send()
            .await;
        let session_created_event =
            wait_for_session_created_event(&mut stream, &mut parser, &session_id).await;
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
        let after_delete = client
            .get(&item_url)
            .basic_auth("opencode", Some(&password))
            .query(&[("directory", &workspace)])
            .send()
            .await?;
        validate_deleted_session_status(after_delete.status())?;
        let inspected: serde_json::Value = inspected?.error_for_status()?.json().await?;
        let status: serde_json::Value = status?.error_for_status()?.json().await?;
        let messages: serde_json::Value = messages?.error_for_status()?.json().await?;
        validate_session_inspection(
            &session_id,
            &workspace,
            &created,
            &inspected,
            &status,
            &messages,
        )?;
        session_created_event?;
        Ok::<_, Error>((reported_version, sha256_hex(&schema_bytes)))
    }
    .await;
    let (reported_version, schema_sha256) = finish_probe(&mut child, outcome).await?;
    Ok(ProbeResult {
        provider: "opencode".into(),
        version: reported_version,
        protocol: "HTTP basic-auth health/OpenAPI/SSE/session status/messages/delete verification"
            .into(),
        healthy: true,
        paid_turn_tested: false,
        cleanup_confirmed: true,
        schema_sha256: Some(schema_sha256),
        session_lifecycle_tested: true,
        session_cleanup_confirmed: Some(true),
        sse_handshake_tested: true,
    })
}

async fn wait_for_session_created_event<S, B, E>(
    stream: &mut S,
    parser: &mut SseParser,
    session_id: &str,
) -> Result<(), Error>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    time::timeout(Duration::from_secs(5), async {
        for _ in 0..128 {
            let chunk = stream
                .next()
                .await
                .ok_or_else(|| {
                    Error::Incompatible("OpenCode SSE ended before session event".into())
                })?
                .map_err(|error| {
                    Error::Incompatible(format!("OpenCode SSE read failed: {error}"))
                })?;
            for event in parser
                .push(chunk.as_ref())
                .map_err(|error| Error::Incompatible(error.to_string()))?
            {
                let parsed = OpenCodeEvent::parse(&event.data)
                    .map_err(|error| Error::Incompatible(error.to_string()))?;
                if parsed.kind == "session.created"
                    && parsed.session_id.as_deref() == Some(session_id)
                {
                    parsed
                        .require_session(session_id)
                        .map_err(|error| Error::Incompatible(error.to_string()))?;
                    return Ok(());
                }
            }
        }
        Err(Error::Incompatible(
            "OpenCode SSE session.created event not found within 128 chunks".into(),
        ))
    })
    .await
    .map_err(|_| Error::Timeout)?
}

fn validate_session_inspection(
    session_id: &str,
    workspace: &str,
    created: &serde_json::Value,
    inspected: &serde_json::Value,
    status: &serde_json::Value,
    messages: &serde_json::Value,
) -> Result<(), Error> {
    for (label, session) in [("created", created), ("inspected", inspected)] {
        if session.get("id").and_then(serde_json::Value::as_str) != Some(session_id) {
            return Err(Error::Incompatible(format!(
                "OpenCode session {label} identity mismatch"
            )));
        }
        if session.get("directory").and_then(serde_json::Value::as_str) != Some(workspace) {
            return Err(Error::Incompatible(format!(
                "OpenCode session {label} workspace identity mismatch"
            )));
        }
    }
    if !status.is_object() {
        return Err(Error::Incompatible(
            "OpenCode /session/status response is not an object".into(),
        ));
    }
    if !messages.is_array() {
        return Err(Error::Incompatible(
            "OpenCode session message response is not an array".into(),
        ));
    }
    Ok(())
}

fn validate_deleted_session_status(status: reqwest::StatusCode) -> Result<(), Error> {
    if status == reqwest::StatusCode::NOT_FOUND {
        Ok(())
    } else {
        Err(Error::Incompatible(format!(
            "OpenCode deleted session remained inspectable: HTTP {status}"
        )))
    }
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

#[cfg(test)]
mod codex_reconciliation_tests {
    use super::*;

    #[tokio::test]
    async fn codex_turn_wait_correlates_thread_and_turn_completion() {
        let input = b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"failed\",\"items\":[]}}}\n";
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let result = await_codex_turn(&mut rpc, "thread-1", "turn-1", Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(
            result,
            CodexTurnAwait::Terminal(TurnCompletedParams {
                turn: crate::protocol::TurnSummary {
                    status: crate::protocol::TurnStatus::Failed,
                    ..
                },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn indexed_thread_lookup_follows_opaque_cursor() {
        let input = concat!(
            "{\"id\":1,\"result\":{\"data\":[{\"id\":\"first\"}],\"nextCursor\":\"opaque-cursor\"}}\n",
            "{\"method\":\"thread/status/changed\",\"params\":{}}\n",
            "{\"id\":2,\"result\":{\"data\":[{\"id\":\"target\",\"status\":{\"type\":\"idle\"}}],\"nextCursor\":null}}\n"
        );
        let mut rpc = JsonRpc::new(input.as_bytes(), Vec::new());
        let found = find_indexed_thread(&mut rpc, "target", false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, "target");
        assert_eq!(found.status.unwrap().kind, "idle");
    }

    #[tokio::test]
    async fn indexed_thread_lookup_checks_archived_partition() {
        let input = concat!(
            "{\"id\":1,\"result\":{\"data\":[],\"nextCursor\":null}}\n",
            "{\"id\":2,\"result\":{\"data\":[{\"id\":\"target\"}],\"nextCursor\":null}}\n"
        );
        let mut rpc = JsonRpc::new(input.as_bytes(), Vec::new());
        let found = collect_indexed_thread(&mut rpc, "target")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, "target");
    }

    #[tokio::test]
    async fn loaded_list_follows_cursor_before_classifying_target() {
        let input = concat!(
            "{\"id\":1,\"result\":{\"data\":[\"other\"],\"nextCursor\":\"opaque-cursor\"}}\n",
            "{\"id\":2,\"result\":{\"data\":[\"target\"],\"nextCursor\":null}}\n"
        );
        let mut rpc = JsonRpc::new(input.as_bytes(), Vec::new());
        assert!(collect_loaded_threads(&mut rpc, "target").await.unwrap());
    }

    #[test]
    fn index_absence_classification_does_not_claim_cleanup() {
        let result = CodexThreadReconciliation::NotFoundInStateDbIndex {
            loaded_in_probe_server: false,
        };
        let encoded = serde_json::to_value(result).unwrap();
        assert_eq!(encoded["classification"], "not_found_in_state_db_index");
        assert_eq!(encoded["loaded_in_probe_server"], false);
        assert!(encoded.get("cleanup_confirmed").is_none());
    }

    #[test]
    fn validates_no_prompt_session_inspection_shapes_and_identity() {
        let created = json!({"id":"ses_1","directory":"/tmp"});
        let inspected = json!({"id":"ses_1","directory":"/tmp"});
        validate_session_inspection(
            "ses_1",
            "/tmp",
            &created,
            &inspected,
            &json!({}),
            &json!([]),
        )
        .unwrap();
        assert!(
            validate_session_inspection(
                "ses_1",
                "/tmp",
                &created,
                &json!({"id":"ses_other","directory":"/tmp"}),
                &json!({}),
                &json!([]),
            )
            .is_err()
        );
        assert!(
            validate_session_inspection(
                "ses_1",
                "/tmp",
                &created,
                &inspected,
                &json!([]),
                &json!([]),
            )
            .is_err()
        );
        assert!(
            validate_session_inspection(
                "ses_1",
                "/tmp",
                &created,
                &inspected,
                &json!({}),
                &json!({}),
            )
            .is_err()
        );
    }

    #[test]
    fn confirms_session_cleanup_only_after_not_found_response() {
        validate_deleted_session_status(reqwest::StatusCode::NOT_FOUND).unwrap();
        assert!(validate_deleted_session_status(reqwest::StatusCode::OK).is_err());
    }

    #[tokio::test]
    async fn observes_matching_session_created_event_from_sse_fixture() {
        let frame = "data: {\"id\":\"evt_1\",\"type\":\"session.created\",\"properties\":{\"sessionID\":\"ses_1\",\"info\":{\"id\":\"ses_1\"}}}\n\n";
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frame.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        wait_for_session_created_event(&mut stream, &mut parser, "ses_1")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ignores_other_session_events_until_matching_creation() {
        let frame = concat!(
            "data: {\"type\":\"session.created\",\"properties\":{\"sessionID\":\"ses_other\",\"info\":{\"id\":\"ses_other\"}}}\n\n",
            "data: {\"type\":\"session.created\",\"properties\":{\"sessionID\":\"ses_expected\",\"info\":{\"id\":\"ses_expected\"}}}\n\n"
        );
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frame.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        wait_for_session_created_event(&mut stream, &mut parser, "ses_expected")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn refuses_to_treat_connection_event_as_session_identity() {
        let frame = "data: {\"type\":\"server.connected\"}\n\n";
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frame.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        let error = wait_for_session_created_event(&mut stream, &mut parser, "ses_1")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("session event"));
    }
}
