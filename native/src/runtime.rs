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
    Completed(TurnCompletedParams),
    Failed(TurnCompletedParams),
    Interrupted(TurnCompletedParams),
    InterruptRequestedAfterTimeout { response: serde_json::Value },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenCodePromptTerminal {
    Idle {
        session_id: String,
        cancel_requested: bool,
    },
    Failed {
        session_id: String,
        error: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpenCodePromptOutcome {
    Completed {
        session_id: String,
        message_id: String,
    },
    Failed {
        session_id: String,
        error: serde_json::Value,
    },
    AbortRequestedAndIdle {
        session_id: String,
    },
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
        TurnWaitOutcome::Completed(turn) => match turn.turn.status {
            crate::protocol::TurnStatus::Completed => Ok(CodexTurnAwait::Completed(turn)),
            crate::protocol::TurnStatus::Failed => Ok(CodexTurnAwait::Failed(turn)),
            crate::protocol::TurnStatus::Interrupted => Ok(CodexTurnAwait::Interrupted(turn)),
            crate::protocol::TurnStatus::InProgress => Err(RpcError::Protocol(
                "turn/completed notification carried in-progress status".into(),
            )),
        },
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
        let client = opencode_http_client(Duration::from_secs(2))?;
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
        let event_response = open_opencode_event_stream(
            &client,
            &format!("http://127.0.0.1:{port}"),
            &password,
            &workspace,
        )
        .await?;
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
        let base_url = format!("http://127.0.0.1:{port}");
        let session_url = format!("{base_url}/session");
        let created = create_opencode_session(
            &client,
            &base_url,
            &password,
            &workspace,
            "Factory protocol preflight",
        )
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
        delete_opencode_session(&client, &base_url, &password, &workspace, &session_id).await?;
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

/// Opens the authenticated OpenCode event stream after verifying that credentials
/// will only be sent to a loopback origin.
pub async fn open_opencode_event_stream(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    directory: &str,
) -> Result<reqwest::Response, Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    let response = client
        .get(format!("{base_url}/event"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .send()
        .await?
        .error_for_status()?;
    if !response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        return Err(Error::Incompatible(
            "OpenCode /event did not return SSE".into(),
        ));
    }
    Ok(response)
}

/// Creates one OpenCode session through the loopback provider API and checks
/// that the returned session is bound to the requested workspace.
pub async fn create_opencode_session(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    directory: &str,
    title: &str,
) -> Result<serde_json::Value, Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    let created: serde_json::Value = client
        .post(format!("{base_url}/session"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .json(&json!({"title":title}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let session_id = created
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Incompatible("OpenCode session/create returned no id".into()))?;
    validate_opencode_session_id(session_id)?;
    Ok(created)
}

/// Submits a prompt asynchronously to one exact session. The accepted HTTP
/// request is not terminal completion; callers must reconcile the session SSE.
pub async fn submit_opencode_prompt(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    directory: &str,
    session_id: &str,
    prompt: &serde_json::Value,
) -> Result<(), Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    validate_opencode_session_id(session_id)?;
    let response = client
        .post(format!("{base_url}/session/{session_id}/prompt_async"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .json(prompt)
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        return Err(Error::Incompatible(format!(
            "OpenCode prompt_async returned HTTP {}",
            response.status()
        )));
    }
    Ok(())
}

/// Requests abort for the exact session. A true HTTP response only confirms
/// that the abort request was accepted; it does not prove interruption.
pub async fn request_opencode_abort(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    directory: &str,
    session_id: &str,
) -> Result<(), Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    validate_opencode_session_id(session_id)?;
    let accepted = client
        .post(format!("{base_url}/session/{session_id}/abort"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .send()
        .await?
        .error_for_status()?
        .json::<bool>()
        .await?;
    if !accepted {
        return Err(Error::Incompatible(
            "OpenCode abort request was not accepted".into(),
        ));
    }
    Ok(())
}

/// Deletes a session and confirms absence with a follow-up read.
pub async fn delete_opencode_session(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    directory: &str,
    session_id: &str,
) -> Result<(), Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    validate_opencode_session_id(session_id)?;
    let deleted = client
        .delete(format!("{base_url}/session/{session_id}"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .send()
        .await?
        .error_for_status()?
        .json::<bool>()
        .await?;
    if !deleted {
        return Err(Error::Incompatible(
            "OpenCode session delete was not confirmed".into(),
        ));
    }
    let after_delete = client
        .get(format!("{base_url}/session/{session_id}"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .send()
        .await?;
    validate_deleted_session_status(after_delete.status())
}

fn validate_opencode_base_url(base_url: &str) -> Result<String, Error> {
    let url = reqwest::Url::parse(base_url)
        .map_err(|_| Error::Incompatible("invalid OpenCode base URL".into()))?;
    let loopback = url
        .host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if url.scheme() != "http"
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Incompatible(
            "OpenCode credentials require a plain HTTP loopback origin".into(),
        ));
    }
    Ok(base_url.trim_end_matches('/').to_owned())
}

fn validate_opencode_session_id(session_id: &str) -> Result<(), Error> {
    if session_id.is_empty()
        || !session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(Error::Incompatible("OpenCode session id is invalid".into()));
    }
    Ok(())
}

/// Exercises an OpenCode prompt through its loopback HTTP and SSE APIs. When
/// `abort_after_submit` is set, the result remains `AbortRequestedAndIdle` until
/// the provider exposes stronger interruption evidence.
async fn run_opencode_prompt(
    base_url: &str,
    password: &str,
    directory: &str,
    title: &str,
    prompt: &serde_json::Value,
    abort_after_submit: bool,
) -> Result<(OpenCodePromptOutcome, serde_json::Value), Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    let client = opencode_http_client(Duration::from_secs(30))?;
    let event_response =
        open_opencode_event_stream(&client, &base_url, password, directory).await?;
    let mut event_stream = event_response.bytes_stream();
    let mut parser = SseParser::new(1_048_576);
    let created = create_opencode_session(&client, &base_url, password, directory, title).await?;
    let session_id = created
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Incompatible("OpenCode session/create returned no id".into()))?
        .to_owned();

    let mut assistant_message = serde_json::Value::Null;
    let outcome = async {
        if created.get("directory").and_then(serde_json::Value::as_str) != Some(directory) {
            return Err(Error::Incompatible(
                "OpenCode created session workspace identity mismatch".into(),
            ));
        }
        wait_for_session_created_event(&mut event_stream, &mut parser, &session_id).await?;
        submit_opencode_prompt(&client, &base_url, password, directory, &session_id, prompt)
            .await?;
        if abort_after_submit {
            request_opencode_abort(&client, &base_url, password, directory, &session_id).await?;
        }
        let terminal = wait_for_opencode_prompt_terminal(
            &mut event_stream,
            &mut parser,
            &session_id,
            abort_after_submit,
        )
        .await?;
        let response = match &terminal {
            OpenCodePromptTerminal::Idle {
                cancel_requested: false,
                ..
            } => {
                read_opencode_assistant_message(
                    &client,
                    &base_url,
                    password,
                    directory,
                    &session_id,
                )
                .await?
            }
            OpenCodePromptTerminal::Idle {
                cancel_requested: true,
                ..
            }
            | OpenCodePromptTerminal::Failed { .. } => serde_json::Value::Null,
        };
        assistant_message = response.clone();
        reconcile_opencode_prompt(&session_id, terminal, &response)
    }
    .await;

    let cleanup =
        delete_opencode_session(&client, &base_url, password, directory, &session_id).await;
    match (outcome, cleanup) {
        (Ok(result), Ok(())) => Ok((result, assistant_message)),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(Error::Incompatible(format!(
            "OpenCode session {session_id} cleanup unconfirmed: {cleanup}"
        ))),
        (Err(error), Err(cleanup)) => Err(Error::Incompatible(format!(
            "{error}; OpenCode session {session_id} cleanup unconfirmed: {cleanup}"
        ))),
    }
}

/// Exercises the OpenCode prompt lifecycle without exposing assistant content.
pub async fn execute_opencode_prompt(
    base_url: &str,
    password: &str,
    directory: &str,
    title: &str,
    prompt: &serde_json::Value,
    abort_after_submit: bool,
) -> Result<OpenCodePromptOutcome, Error> {
    run_opencode_prompt(
        base_url,
        password,
        directory,
        title,
        prompt,
        abort_after_submit,
    )
    .await
    .map(|(outcome, _)| outcome)
}

/// Exercises the OpenCode prompt lifecycle and returns the exact assistant
/// message for a Factory-mediated, tool-free patch extraction step.
pub async fn execute_opencode_prompt_with_response(
    base_url: &str,
    password: &str,
    directory: &str,
    title: &str,
    prompt: &serde_json::Value,
    abort_after_submit: bool,
) -> Result<(OpenCodePromptOutcome, serde_json::Value), Error> {
    run_opencode_prompt(
        base_url,
        password,
        directory,
        title,
        prompt,
        abort_after_submit,
    )
    .await
}

pub fn opencode_http_client(timeout: Duration) -> Result<reqwest::Client, Error> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

pub async fn read_opencode_assistant_message(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    directory: &str,
    session_id: &str,
) -> Result<serde_json::Value, Error> {
    let base_url = validate_opencode_base_url(base_url)?;
    validate_opencode_session_id(session_id)?;
    let messages = client
        .get(format!("{base_url}/session/{session_id}/message"))
        .basic_auth("opencode", Some(password))
        .query(&[("directory", directory)])
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let messages = messages.as_array().ok_or_else(|| {
        Error::Incompatible("OpenCode session message response is not an array".into())
    })?;
    let assistant = messages
        .iter()
        .rev()
        .find(|message| {
            message
                .pointer("/info/role")
                .and_then(serde_json::Value::as_str)
                == Some("assistant")
        })
        .cloned()
        .ok_or_else(|| Error::Incompatible("OpenCode session has no assistant message".into()))?;
    validate_opencode_prompt_response(session_id, &assistant)?;
    Ok(assistant)
}

pub async fn wait_for_session_created_event<S, B, E>(
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

/// Waits for terminal SSE evidence for one exact OpenCode session. `cancel_requested`
/// records that the caller requested abort; `session.idle` does not prove
/// cancellation because normal completion can race with the abort request.
/// Errors take precedence over the unresolved idle-after-abort result.
pub async fn wait_for_opencode_prompt_terminal<S, B, E>(
    stream: &mut S,
    parser: &mut SseParser,
    session_id: &str,
    cancel_requested: bool,
) -> Result<OpenCodePromptTerminal, Error>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    if session_id.trim().is_empty() {
        return Err(Error::Incompatible(
            "OpenCode prompt requires an exact nonempty session identity".into(),
        ));
    }
    time::timeout(Duration::from_secs(30), async {
        for _ in 0..256 {
            let chunk = stream
                .next()
                .await
                .ok_or_else(|| {
                    Error::Incompatible("OpenCode SSE ended before prompt terminal".into())
                })?
                .map_err(|error| {
                    Error::Incompatible(format!("OpenCode SSE read failed: {error}"))
                })?;
            for frame in parser
                .push(chunk.as_ref())
                .map_err(|error| Error::Incompatible(error.to_string()))?
            {
                let event = OpenCodeEvent::parse(&frame.data)
                    .map_err(|error| Error::Incompatible(error.to_string()))?;
                if !matches!(event.kind.as_str(), "session.idle" | "session.error") {
                    continue;
                }
                if event.session_id.as_deref() != Some(session_id) {
                    continue;
                }
                event
                    .require_session(session_id)
                    .map_err(|error| Error::Incompatible(error.to_string()))?;
                if event.kind == "session.error" {
                    let value: serde_json::Value = serde_json::from_str(&frame.data)
                        .map_err(|_| Error::Incompatible("OpenCode SSE data is not JSON".into()))?;
                    let error = value
                        .pointer("/properties/error")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Ok(OpenCodePromptTerminal::Failed {
                        session_id: session_id.to_owned(),
                        error,
                    });
                }
                return Ok(OpenCodePromptTerminal::Idle {
                    session_id: session_id.to_owned(),
                    cancel_requested,
                });
            }
        }
        Err(Error::Incompatible(
            "OpenCode prompt terminal event not found within 256 SSE chunks".into(),
        ))
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// Validates the JSON body returned by OpenCode's synchronous session message
/// endpoint. SSE terminal evidence is still required separately.
pub fn validate_opencode_prompt_response(
    session_id: &str,
    response: &serde_json::Value,
) -> Result<(), Error> {
    let info = response.get("info").ok_or_else(|| {
        Error::Incompatible("OpenCode prompt response has no message info".into())
    })?;
    if info.get("sessionID").and_then(serde_json::Value::as_str) != Some(session_id) {
        return Err(Error::Incompatible(
            "OpenCode prompt response session identity mismatch".into(),
        ));
    }
    if info.get("role").and_then(serde_json::Value::as_str) != Some("assistant")
        || info
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(Error::Incompatible(
            "OpenCode prompt response is not a terminal assistant message".into(),
        ));
    }
    if info.get("error").is_some_and(|error| !error.is_null()) {
        return Err(Error::Incompatible(
            "OpenCode prompt response contains a provider message error".into(),
        ));
    }
    Ok(())
}

/// Combines terminal SSE evidence with the synchronous HTTP prompt response.
/// An idle event alone never becomes a successful completed prompt result.
pub fn reconcile_opencode_prompt(
    session_id: &str,
    terminal: OpenCodePromptTerminal,
    response: &serde_json::Value,
) -> Result<OpenCodePromptOutcome, Error> {
    let observed_session_id = match &terminal {
        OpenCodePromptTerminal::Idle { session_id, .. }
        | OpenCodePromptTerminal::Failed { session_id, .. } => session_id,
    };
    if session_id.trim().is_empty() || observed_session_id != session_id {
        return Err(Error::Incompatible(
            "OpenCode prompt terminal session identity mismatch".into(),
        ));
    }
    match terminal {
        OpenCodePromptTerminal::Failed { session_id, error } => {
            Ok(OpenCodePromptOutcome::Failed { session_id, error })
        }
        OpenCodePromptTerminal::Idle {
            session_id,
            cancel_requested: true,
        } => Ok(OpenCodePromptOutcome::AbortRequestedAndIdle { session_id }),
        OpenCodePromptTerminal::Idle {
            session_id,
            cancel_requested: false,
        } => {
            validate_opencode_prompt_response(&session_id, response)?;
            let message_id = response
                .pointer("/info/id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    Error::Incompatible("OpenCode prompt response has no message id".into())
                })?;
            Ok(OpenCodePromptOutcome::Completed {
                session_id,
                message_id: message_id.to_owned(),
            })
        }
    }
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
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn codex_turn_wait_correlates_thread_and_turn_completion() {
        let input = b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"failed\",\"items\":[]}}}\n";
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let result = await_codex_turn(&mut rpc, "thread-1", "turn-1", Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(
            result,
            CodexTurnAwait::Failed(TurnCompletedParams {
                turn: crate::protocol::TurnSummary {
                    status: crate::protocol::TurnStatus::Failed,
                    ..
                },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn codex_completed_status_is_the_only_success_classification() {
        let input = b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"completed\",\"items\":[]}}}\n";
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let result = await_codex_turn(&mut rpc, "thread-1", "turn-1", Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(result, CodexTurnAwait::Completed(_)));
    }

    #[tokio::test]
    async fn codex_interrupted_status_is_not_success() {
        let input = b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"interrupted\",\"items\":[]}}}\n";
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let result = await_codex_turn(&mut rpc, "thread-1", "turn-1", Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(result, CodexTurnAwait::Interrupted(_)));
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

    #[tokio::test]
    async fn opencode_prompt_waits_for_exact_session_idle_after_other_session_events() {
        let frames = concat!(
            "data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_other\"}}\n\n",
            "data: {\"type\":\"message.part.updated\",\"properties\":{\"sessionID\":\"ses_expected\"}}\n\n",
            "data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_expected\"}}\n\n"
        );
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frames.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        let terminal =
            wait_for_opencode_prompt_terminal(&mut stream, &mut parser, "ses_expected", false)
                .await
                .unwrap();
        assert_eq!(
            terminal,
            OpenCodePromptTerminal::Idle {
                session_id: "ses_expected".into(),
                cancel_requested: false,
            }
        );
    }

    #[tokio::test]
    async fn opencode_prompt_classifies_provider_error_even_after_cancel_request() {
        let frame = "data: {\"type\":\"session.error\",\"properties\":{\"sessionID\":\"ses_expected\",\"error\":{\"name\":\"ProviderError\"}}}\n\n";
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frame.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        let terminal =
            wait_for_opencode_prompt_terminal(&mut stream, &mut parser, "ses_expected", true)
                .await
                .unwrap();
        assert_eq!(
            terminal,
            OpenCodePromptTerminal::Failed {
                session_id: "ses_expected".into(),
                error: json!({"name":"ProviderError"})
            }
        );
        assert_eq!(
            reconcile_opencode_prompt("ses_expected", terminal, &json!({})).unwrap(),
            OpenCodePromptOutcome::Failed {
                session_id: "ses_expected".into(),
                error: json!({"name":"ProviderError"})
            }
        );
    }

    #[tokio::test]
    async fn opencode_idle_after_abort_does_not_claim_cancellation() {
        let frame =
            "data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_expected\"}}\n\n";
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frame.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        let terminal =
            wait_for_opencode_prompt_terminal(&mut stream, &mut parser, "ses_expected", true)
                .await
                .unwrap();
        assert_eq!(
            terminal,
            OpenCodePromptTerminal::Idle {
                session_id: "ses_expected".into(),
                cancel_requested: true,
            }
        );
        assert_eq!(
            reconcile_opencode_prompt("ses_expected", terminal, &json!({})).unwrap(),
            OpenCodePromptOutcome::AbortRequestedAndIdle {
                session_id: "ses_expected".into()
            }
        );
        let completed_response = json!({
            "info":{"id":"msg_completed","sessionID":"ses_expected","role":"assistant"}
        });
        assert_eq!(
            reconcile_opencode_prompt(
                "ses_expected",
                OpenCodePromptTerminal::Idle {
                    session_id: "ses_expected".into(),
                    cancel_requested: true,
                },
                &completed_response,
            )
            .unwrap(),
            OpenCodePromptOutcome::AbortRequestedAndIdle {
                session_id: "ses_expected".into()
            }
        );
    }

    #[tokio::test]
    async fn opencode_prompt_does_not_complete_on_other_session_terminal() {
        let frame =
            "data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_other\"}}\n\n";
        let mut stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(frame.as_bytes().to_vec())]);
        let mut parser = SseParser::new(4096);
        let error =
            wait_for_opencode_prompt_terminal(&mut stream, &mut parser, "ses_expected", false)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("ended before prompt terminal"));
    }

    #[test]
    fn opencode_http_prompt_response_requires_exact_assistant_message_identity() {
        let response = json!({
            "info":{"id":"msg_1","sessionID":"ses_expected","role":"assistant"},
            "parts":[{"type":"text","text":"fixture result"}]
        });
        validate_opencode_prompt_response("ses_expected", &response).unwrap();
        let outcome = reconcile_opencode_prompt(
            "ses_expected",
            OpenCodePromptTerminal::Idle {
                session_id: "ses_expected".into(),
                cancel_requested: false,
            },
            &response,
        )
        .unwrap();
        assert_eq!(
            outcome,
            OpenCodePromptOutcome::Completed {
                session_id: "ses_expected".into(),
                message_id: "msg_1".into(),
            }
        );
        assert!(validate_opencode_prompt_response("ses_other", &response).is_err());
        assert!(
            reconcile_opencode_prompt(
                "ses_expected",
                OpenCodePromptTerminal::Idle {
                    session_id: "ses_other".into(),
                    cancel_requested: false,
                },
                &response,
            )
            .is_err()
        );
        assert!(
            validate_opencode_prompt_response(
                "ses_expected",
                &json!({"info":{"id":"msg_1","sessionID":"ses_expected","role":"user"}})
            )
            .is_err()
        );
        assert!(validate_opencode_prompt_response(
            "ses_expected",
            &json!({"info":{"id":"msg_1","sessionID":"ses_expected","role":"assistant","error":{"name":"ProviderError"}}})
        )
        .is_err());
    }

    #[derive(Clone)]
    struct FakeOpenCodeState {
        expected_authorization: &'static str,
        event_sender: tokio::sync::mpsc::Sender<Vec<u8>>,
        event_receiver:
            std::sync::Arc<tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<Vec<u8>>>>>,
        calls: std::sync::Arc<tokio::sync::Mutex<Vec<(String, bool, serde_json::Value)>>>,
        deleted: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    fn fake_call(
        state: &FakeOpenCodeState,
        route: &str,
        headers: &axum::http::HeaderMap,
        body: serde_json::Value,
    ) -> (String, bool, serde_json::Value) {
        (
            route.into(),
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                == Some(state.expected_authorization),
            body,
        )
    }

    async fn fake_create_session(
        axum::extract::State(state): axum::extract::State<FakeOpenCodeState>,
        headers: axum::http::HeaderMap,
        axum::extract::Query(query): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        let directory = query.get("directory").cloned().unwrap_or_default();
        let subscriber_ready = state.event_receiver.lock().await.is_none();
        state.calls.lock().await.push(fake_call(
            &state,
            &format!("POST /session?directory={directory};subscriber_ready={subscriber_ready}"),
            &headers,
            body,
        ));
        let _ = state.event_sender.send(b"data: {\"type\":\"session.created\",\"properties\":{\"sessionID\":\"ses_fixture\",\"info\":{\"id\":\"ses_fixture\"}}}\n\n".to_vec()).await;
        axum::Json(json!({"id":"ses_fixture","directory":query.get("directory")}))
    }

    async fn fake_events(
        axum::extract::State(state): axum::extract::State<FakeOpenCodeState>,
        headers: axum::http::HeaderMap,
    ) -> axum::response::Response {
        state.calls.lock().await.push(fake_call(
            &state,
            "GET /event",
            &headers,
            serde_json::Value::Null,
        ));
        let Some(receiver) = state.event_receiver.lock().await.take() else {
            return axum::http::StatusCode::CONFLICT.into_response();
        };
        let _ = state
            .event_sender
            .send(b"data: {\"type\":\"server.connected\"}\n\n".to_vec())
            .await;
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|chunk| {
                (
                    Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(chunk)),
                    receiver,
                )
            })
        });
        let mut response = axum::response::Response::new(axum::body::Body::from_stream(stream));
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        response
    }

    async fn fake_prompt(
        axum::extract::State(state): axum::extract::State<FakeOpenCodeState>,
        axum::extract::Path(session_id): axum::extract::Path<String>,
        headers: axum::http::HeaderMap,
        axum::extract::Query(query): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::http::StatusCode {
        state.calls.lock().await.push(fake_call(
            &state,
            &format!(
                "POST /session/{session_id}/prompt_async?directory={}",
                query.get("directory").cloned().unwrap_or_default()
            ),
            &headers,
            body,
        ));
        axum::http::StatusCode::NO_CONTENT
    }

    async fn fake_abort(
        axum::extract::State(state): axum::extract::State<FakeOpenCodeState>,
        axum::extract::Path(session_id): axum::extract::Path<String>,
        headers: axum::http::HeaderMap,
    ) -> axum::Json<bool> {
        state.calls.lock().await.push(fake_call(
            &state,
            &format!("POST /session/{session_id}/abort"),
            &headers,
            serde_json::Value::Null,
        ));
        let _ = state.event_sender.send(b"data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_fixture\"}}\n\n".to_vec()).await;
        axum::Json(true)
    }

    async fn fake_delete(
        axum::extract::State(state): axum::extract::State<FakeOpenCodeState>,
        axum::extract::Path(session_id): axum::extract::Path<String>,
        headers: axum::http::HeaderMap,
    ) -> axum::Json<bool> {
        state.calls.lock().await.push(fake_call(
            &state,
            &format!("DELETE /session/{session_id}"),
            &headers,
            serde_json::Value::Null,
        ));
        state
            .deleted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        axum::Json(true)
    }

    async fn fake_inspect(
        axum::extract::State(state): axum::extract::State<FakeOpenCodeState>,
        axum::extract::Path(session_id): axum::extract::Path<String>,
        headers: axum::http::HeaderMap,
    ) -> axum::response::Response {
        state.calls.lock().await.push(fake_call(
            &state,
            &format!("GET /session/{session_id}"),
            &headers,
            serde_json::Value::Null,
        ));
        if state.deleted.load(std::sync::atomic::Ordering::SeqCst) {
            axum::http::StatusCode::NOT_FOUND.into_response()
        } else {
            axum::Json(json!({"id":session_id})).into_response()
        }
    }

    #[tokio::test]
    async fn loopback_http_sse_prompt_binds_auth_submission_abort_and_cleanup() {
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(8);
        let state = FakeOpenCodeState {
            expected_authorization: "Basic b3BlbmNvZGU6Zml4dHVyZS1zZWNyZXQ=",
            event_sender,
            event_receiver: std::sync::Arc::new(tokio::sync::Mutex::new(Some(event_receiver))),
            calls: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
            deleted: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let app = axum::Router::new()
            .route("/session", axum::routing::post(fake_create_session))
            .route("/event", axum::routing::get(fake_events))
            .route(
                "/session/{session_id}/prompt_async",
                axum::routing::post(fake_prompt),
            )
            .route(
                "/session/{session_id}/abort",
                axum::routing::post(fake_abort),
            )
            .route(
                "/session/{session_id}",
                axum::routing::delete(fake_delete).get(fake_inspect),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let prompt = json!({"model":{"providerID":"fixture","modelID":"local"},"parts":[{"type":"text","text":"fixture prompt"}]});
        let outcome = execute_opencode_prompt(
            &format!("http://{address}"),
            "fixture-secret",
            "/fixture/workspace",
            "Lifecycle fixture",
            &prompt,
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            outcome,
            OpenCodePromptOutcome::AbortRequestedAndIdle {
                session_id: "ses_fixture".into()
            }
        );
        assert!(!format!("{outcome:?}").contains("fixture-secret"));

        let calls = state.calls.lock().await;
        assert!(calls.len() >= 5, "all lifecycle routes should be observed");
        assert!(calls.iter().all(|(_, authorized, _)| *authorized));
        let event_subscription = calls
            .iter()
            .position(|(route, _, _)| route == "GET /event")
            .expect("SSE subscription should be recorded");
        let session_create = calls
            .iter()
            .position(|(route, _, body)| {
                route == "POST /session?directory=/fixture/workspace;subscriber_ready=true"
                    && body["title"] == "Lifecycle fixture"
            })
            .expect("session create should follow SSE subscription");
        assert!(event_subscription < session_create);
        assert!(calls.iter().any(|(route, _, body)| {
            route == "POST /session?directory=/fixture/workspace;subscriber_ready=true"
                && body["title"] == "Lifecycle fixture"
        }));
        assert!(calls.iter().any(|(route, _, body)| {
            route.starts_with("POST /session/ses_fixture/prompt_async") && body == &prompt
        }));
        assert!(
            calls
                .iter()
                .any(|(route, _, _)| route == "POST /session/ses_fixture/abort")
        );
        assert!(
            calls
                .iter()
                .any(|(route, _, _)| route == "DELETE /session/ses_fixture")
        );
        assert!(
            calls
                .iter()
                .any(|(route, _, _)| route == "GET /session/ses_fixture")
        );
        assert!(
            calls
                .iter()
                .all(|(_, _, body)| !body.to_string().contains("fixture-secret"))
        );
        drop(calls);

        let client = opencode_http_client(Duration::from_secs(1)).unwrap();
        assert!(
            open_opencode_event_stream(
                &client,
                "http://example.invalid",
                "fixture-secret",
                "/fixture/workspace"
            )
            .await
            .is_err()
        );
        server.abort();
    }
}
