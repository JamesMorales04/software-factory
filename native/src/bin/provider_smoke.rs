//! Bounded live provider smoke: one read-only turn per provider.
//!
//! Default-closed. Requires FACTORY_LIVE_PROVIDER_SMOKE=1 and an explicit
//! authorization id plus model. No tools/write access, no retries, bounded
//! timeout and token ceiling. Evidence is JSON; credentials are never written.

use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use factory_native::protocol::{
    DenyApprovalPolicy, JsonRpc, ReadOnlySandbox, ThreadStartParams, TurnStartResponse,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time,
};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SmokeOutput {
    ok: bool,
    summary: String,
}

fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(SmokeOutput)).expect("schema serializes")
}

fn schema_sha256() -> String {
    let bytes = serde_json::to_vec(&schema()).expect("schema serializes");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Serialize)]
struct Evidence {
    provider: String,
    provider_version: String,
    model: String,
    authorization_id: String,
    candidate_sha: String,
    schema_sha256: String,
    structured_output_supported: bool,
    protocol_verified: bool,
    structured_output_verified: bool,
    turn_id: Option<String>,
    session_id: Option<String>,
    usage: Value,
    output: Option<Value>,
    cleanup: Value,
    requests: usize,
    classification: String,
    error: Option<String>,
    duration_ms: u128,
}

fn require(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} required"))
}

fn candidate_sha() -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("candidate git HEAD unavailable".into());
    }
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|error| error.to_string())?;
    if !status.status.success() || !status.stdout.is_empty() {
        return Err("live provider smoke requires a clean candidate worktree".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn provider_version(binary: &str) -> Result<String, String> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("{binary} --version failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn validate_output(text: &str) -> Result<SmokeOutput, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let validator = jsonschema::validator_for(&schema()).map_err(|error| error.to_string())?;
    validator
        .validate(&value)
        .map_err(|error| error.to_string())?;
    let output: SmokeOutput = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !output.ok || output.summary.trim().is_empty() || output.summary.len() > 200 {
        return Err("smoke output did not satisfy the semantic contract".into());
    }
    Ok(output)
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|error| error.to_string())
}

async fn codex(
    model: &str,
    max_tokens: u64,
    timeout: Duration,
    workspace: &Path,
) -> Result<Evidence, String> {
    let version = provider_version("codex")?;
    let mut child = Command::new("codex")
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdin = child.stdin.take().ok_or("codex stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("codex stdout unavailable")?;
    let mut rpc = JsonRpc::new(stdout, stdin);
    let outcome = time::timeout(timeout, async {
        rpc.request("initialize", json!({
            "clientInfo":{"name":"software_factory","title":"Software Factory","version":"0.1.0"},
            "capabilities":{}
        })).await.map_err(|error| error.to_string())?;
        rpc.notify("initialized", json!({})).await.map_err(|error| error.to_string())?;
        let models = rpc.request("model/list", json!({})).await.map_err(|error| error.to_string())?;
        let available = models.pointer("/data").and_then(Value::as_array).is_some_and(|items| {
            items.iter().any(|item| item.get("id").and_then(Value::as_str) == Some(model))
        });
        if !available {
            return Err::<Evidence, String>(format!("model {model:?} not available in Codex model/list"));
        }
        let thread = rpc.start_thread(ThreadStartParams {
            cwd: workspace.to_string_lossy().to_string(),
            approval_policy: DenyApprovalPolicy::Never,
            sandbox: ReadOnlySandbox::ReadOnly,
        }).await.map_err(|error| error.to_string())?;
        let thread_id = thread.thread.id;
        let turn = rpc.request("turn/start", json!({
            "threadId": thread_id,
            "input":[{"type":"text","text":"Return only JSON matching the provided schema. Set ok true and summary to a short confirmation that this is a read-only provider smoke test. Do not use tools."}],
            "approvalPolicy":"never",
            "sandboxPolicy":{"type":"readOnly"},
            "model": model,
            "effort":"low",
            "outputSchema": schema()
        })).await.map_err(|error| error.to_string())?;
        let turn: TurnStartResponse = serde_json::from_value(turn).map_err(|error| error.to_string())?;
        let turn_id = turn.turn.id;
        let mut final_text = None;
        let mut usage = json!({});
        let mut completed = false;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let notification = time::timeout(Duration::from_secs(30), rpc.next_notification())
                .await
                .map_err(|_| "Codex notification timeout".to_string())?
                .map_err(|error| error.to_string())?;
            match notification.method.as_str() {
                "item/completed" => {
                    if let Some(item) = notification.params.get("item")
                        && item.get("type").and_then(Value::as_str) == Some("agentMessage")
                        && let Some(text) = item.get("text").and_then(Value::as_str)
                    {
                        final_text = Some(text.to_owned());
                    }
                }
                "thread/tokenUsage/updated" => {
                    usage = notification.params.get("tokenUsage").cloned().unwrap_or(json!({}));
                    let output_tokens = usage.pointer("/last/outputTokens").and_then(Value::as_u64).unwrap_or(0);
                    if output_tokens > max_tokens {
                        let _ = rpc.interrupt_turn(&thread_id, &turn_id).await;
                        return Err::<Evidence, String>(format!(
                            "token ceiling exceeded: {output_tokens} > {max_tokens}; interrupt requested"
                        ));
                    }
                }
                "turn/completed" => {
                    let status = notification.params.pointer("/turn/status").and_then(Value::as_str);
                    if status != Some("completed") {
                        return Err(format!("Codex turn terminal status is {status:?}"));
                    }
                    completed = true;
                    break;
                }
                _ => {}
            }
        }
        if !completed {
            return Err("Codex turn did not complete before timeout".into());
        }
        let text = final_text.ok_or("Codex returned no agentMessage item")?;
        let output = validate_output(&text)?;
        let cleanup = rpc.request("thread/archive", json!({"threadId":thread_id})).await
            .map(|value| json!({"archive_response": value}))
            .unwrap_or_else(|error| json!({"archive_error": error.to_string()}));
        Ok(Evidence {
            provider: "codex".into(),
            provider_version: version,
            model: model.into(),
            authorization_id: env::var("FACTORY_PROVIDER_SMOKE_AUTHORIZATION").unwrap_or_default(),
            candidate_sha: candidate_sha()?,
            schema_sha256: schema_sha256(),
            structured_output_supported: true,
            protocol_verified: true,
            structured_output_verified: true,
            turn_id: Some(turn_id),
            session_id: Some(thread_id),
            usage,
            output: Some(serde_json::to_value(output).map_err(|error| error.to_string())?),
            cleanup,
            requests: 1,
            classification: "LIVE_STRUCTURED_OUTPUT_VERIFIED".into(),
            error: None,
            duration_ms: 0,
        })
    }).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    match outcome {
        Ok(result) => result,
        Err(_) => Err("Codex smoke timed out".into()),
    }
}

async fn opencode(
    model: &str,
    max_tokens: u64,
    timeout: Duration,
    workspace: &Path,
) -> Result<Evidence, String> {
    let version = provider_version("opencode")?;
    let password = Uuid::new_v4().to_string();
    let port = free_port()?;
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
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| error.to_string())?;
    let status = child.stdout.take().ok_or("opencode stdout unavailable")?;
    let mut lines = BufReader::new(status).lines();
    let outcome = time::timeout(timeout, async {
        let mut ready = false;
        for _ in 0..40 {
            let line = lines.next_line().await.map_err(|error| error.to_string())?.ok_or("opencode exited before listen")?;
            if line.contains("opencode server listening on http://") { ready = true; break; }
        }
        if !ready { return Err("opencode server did not report listen address".to_string()); }
        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
        let workspace = workspace.canonicalize().map_err(|e| e.to_string())?.to_string_lossy().to_string();
        let directory = format!("?directory={}", urlencoding(&workspace));
        let session: Value = client.post(format!("{base}/session{directory}"))
            .basic_auth("opencode", Some(&password))
            .json(&json!({"title":"Factory read-only provider smoke"}))
            .send().await.map_err(|e| e.to_string())?
            .error_for_status().map_err(|e| e.to_string())?
            .json().await.map_err(|e| e.to_string())?;
        let session_id = session.get("id").and_then(Value::as_str).ok_or("opencode session id missing")?.to_owned();
        let route = parse_opencode_route(model)?;
        let response = client.post(format!("{base}/session/{session_id}/message{directory}"))
            .basic_auth("opencode", Some(&password))
            .json(&opencode_request(&route, schema()))
            .send().await.map_err(|error| error.to_string())?;
        let status = response.status();
        let body = response.text().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            let classification = classify_http_failure(status.as_u16());
            let cleanup = cleanup_session(&client, &base, &session_id, &directory, &password).await;
            return Err(format!(
                "opencode message failed with HTTP {status}: {body}; classification={classification}; session={session_id}; cleanup={cleanup}"
            ));
        }
        let response: Value = serde_json::from_str(&body)
            .map_err(|error| format!("opencode response is not JSON: {error}; session={session_id}"))?;
        let info = response.get("info").cloned().unwrap_or(json!({}));
        let usage = json!({"tokens": info.get("tokens"), "cost": info.get("cost")});
        let total = info.pointer("/tokens/total").and_then(Value::as_u64).unwrap_or(0);
        if total > max_tokens {
            let cleanup = cleanup_session(&client, &base, &session_id, &directory, &password).await;
            return Err(format!(
                "token ceiling exceeded: {total} > {max_tokens}; session={session_id}; cleanup={cleanup}"
            ));
        }
        let structured = info.get("structured").filter(|value| !value.is_null()).cloned();
        let text = structured.clone().map(|value| value.to_string()).or_else(|| {
            response.get("parts").and_then(Value::as_array).and_then(|parts| {
                parts.iter().find(|part| part.get("type").and_then(Value::as_str) == Some("text"))
                    .and_then(|part| part.get("text")).and_then(Value::as_str).map(str::to_owned)
            })
        });
        let Some(text) = text else {
            let error = info.get("error").cloned().unwrap_or(json!({}));
            let cleanup = cleanup_session(&client, &base, &session_id, &directory, &password).await;
            return Err(format!(
                "opencode returned no structured or text result; error={error}; session={session_id}; cleanup={cleanup}"
            ));
        };
        let output = match validate_output(&text) {
            Ok(output) => output,
            Err(error) => {
                let cleanup = cleanup_session(&client, &base, &session_id, &directory, &password).await;
                return Err(format!(
                    "opencode structured output invalid: {error}; session={session_id}; cleanup={cleanup}"
                ));
            }
        };
        let cleanup = cleanup_session(&client, &base, &session_id, &directory, &password).await;
        if cleanup.get("deleted").and_then(Value::as_bool) != Some(true)
            || cleanup.get("after_delete_success").and_then(Value::as_bool) != Some(false)
        {
            return Err(format!("opencode session cleanup unconfirmed: {cleanup}"));
        }
        Ok(Evidence {
            provider: "opencode".into(),
            provider_version: version,
            model: model.into(),
            authorization_id: env::var("FACTORY_PROVIDER_SMOKE_AUTHORIZATION").unwrap_or_default(),
            candidate_sha: candidate_sha()?,
            schema_sha256: schema_sha256(),
            structured_output_supported: true,
            protocol_verified: true,
            structured_output_verified: structured.is_some(),
            turn_id: info.get("id").and_then(Value::as_str).map(str::to_owned),
            session_id: Some(session_id),
            usage,
            output: Some(serde_json::to_value(output).map_err(|error| error.to_string())?),
            cleanup,
            requests: 1,
            classification: if structured.is_some() { "LIVE_STRUCTURED_OUTPUT_VERIFIED" } else { "LIVE_PROVIDER_PROTOCOL_VERIFIED" }.into(),
            error: None,
            duration_ms: 0,
        })
    }).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    match outcome {
        Ok(result) => result,
        Err(_) => Err("OpenCode smoke timed out".into()),
    }
}

async fn cleanup_session(
    client: &reqwest::Client,
    base: &str,
    session_id: &str,
    directory: &str,
    password: &str,
) -> Value {
    let deleted = match client
        .delete(format!("{base}/session/{session_id}{directory}"))
        .basic_auth("opencode", Some(password))
        .send()
        .await
    {
        Ok(response) => match response.error_for_status() {
            Ok(response) => response.json::<bool>().await.unwrap_or(false),
            Err(_) => false,
        },
        Err(_) => false,
    };
    let after = client
        .get(format!("{base}/session/{session_id}{directory}"))
        .basic_auth("opencode", Some(password))
        .send()
        .await
        .ok()
        .map(|response| response.status().as_u16())
        .unwrap_or(0);
    json!({
        "deleted": deleted,
        "after_delete_status": after,
        "after_delete_success": (200..300).contains(&after)
    })
}

#[derive(Debug, PartialEq, Eq)]
struct OpencodeRoute<'a> {
    provider_id: &'a str,
    model_id: &'a str,
}

fn parse_opencode_route(model: &str) -> Result<OpencodeRoute<'_>, String> {
    let (provider_id, model_id) = model
        .split_once('/')
        .ok_or_else(|| "OpenCode model must be provider/model".to_string())?;
    if provider_id.is_empty()
        || model_id.is_empty()
        || provider_id.contains('/')
        || model_id.contains('/')
    {
        return Err(format!("invalid OpenCode route {model:?}"));
    }
    if !matches!(provider_id, "opencode" | "opencode-go") {
        return Err(format!(
            "unknown OpenCode provider {provider_id:?}; explicit opencode or opencode-go route required"
        ));
    }
    Ok(OpencodeRoute {
        provider_id,
        model_id,
    })
}

fn opencode_request(route: &OpencodeRoute<'_>, schema: Value) -> Value {
    json!({
        "model":{"providerID":route.provider_id,"modelID":route.model_id},
        "agent":"plan",
        "tools":{"bash":false,"edit":false,"write":false,"patch":false,"multiedit":false},
        "format":{"type":"json_schema","schema":schema,"retryCount":0},
        "parts":[{"type":"text","text":"Return only JSON matching the provided schema. Set ok true and summary to a short confirmation that this is a read-only provider smoke test. Do not use tools."}]
    })
}

fn classify_http_failure(status: u16) -> &'static str {
    match status {
        401 | 403 => "LIVE_PROVIDER_AUTH_BLOCKED",
        404 | 405 | 415 | 422 => "LIVE_PROVIDER_PROTOCOL_BLOCKED",
        408 | 429 | 500..=599 => "LIVE_PROVIDER_PROTOCOL_BLOCKED",
        _ => "LIVE_VALIDATION_FAILED",
    }
}

fn urlencoding(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('?', "%3F")
        .replace('&', "%26")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("FACTORY_LIVE_PROVIDER_SMOKE").as_deref() != Ok("1") {
        return Err("FACTORY_LIVE_PROVIDER_SMOKE=1 required; no paid call made".into());
    }
    let authorization_id = require("FACTORY_PROVIDER_SMOKE_AUTHORIZATION")?;
    let provider = env::args()
        .nth(1)
        .ok_or("usage: factory-provider-smoke codex|opencode")?;
    let model = require("FACTORY_PROVIDER_MODEL")?;
    let max_tokens: u64 = env::var("FACTORY_SMOKE_MAX_TOKENS")
        .unwrap_or_else(|_| "1024".into())
        .parse()?;
    let timeout = Duration::from_secs(
        env::var("FACTORY_SMOKE_TIMEOUT_SECS")
            .unwrap_or_else(|_| "240".into())
            .parse()?,
    );
    let workspace = PathBuf::from(
        env::var("FACTORY_SMOKE_WORKSPACE")
            .unwrap_or_else(|_| "/tmp/factory-provider-smoke".into()),
    );
    std::fs::create_dir_all(&workspace)?;
    let evidence_path = PathBuf::from(
        env::var("FACTORY_EVIDENCE_PATH").unwrap_or_else(|_| "provider-smoke.json".into()),
    );
    let started = Instant::now();
    let result = match provider.as_str() {
        "codex" => codex(&model, max_tokens, timeout, &workspace).await,
        "opencode" => opencode(&model, max_tokens, timeout, &workspace).await,
        _ => Err("provider must be codex or opencode".into()),
    };
    let mut evidence = match result {
        Ok(mut evidence) => {
            evidence.authorization_id = authorization_id;
            evidence.duration_ms = started.elapsed().as_millis();
            evidence
        }
        Err(error) => {
            let evidence = Evidence {
                provider,
                provider_version: String::new(),
                model,
                authorization_id,
                candidate_sha: candidate_sha().unwrap_or_default(),
                schema_sha256: schema_sha256(),
                structured_output_supported: false,
                protocol_verified: false,
                structured_output_verified: false,
                turn_id: None,
                session_id: None,
                usage: json!({}),
                output: None,
                cleanup: json!({}),
                requests: 0,
                classification: "LIVE_PROVIDER_SMOKE_FAILED".into(),
                error: Some(error),
                duration_ms: started.elapsed().as_millis(),
            };
            std::fs::write(&evidence_path, serde_json::to_vec_pretty(&evidence)?)?;
            return Err(format!(
                "live provider smoke failed; evidence at {}",
                evidence_path.display()
            )
            .into());
        }
    };
    evidence.duration_ms = started.elapsed().as_millis();
    std::fs::write(&evidence_path, serde_json::to_vec_pretty(&evidence)?)?;
    println!("{}", serde_json::to_string_pretty(&evidence)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_go_and_opencode_are_distinct_provider_identities() {
        let go = parse_opencode_route("opencode-go/deepseek-v4.1-flash").unwrap();
        assert_eq!(go.provider_id, "opencode-go");
        assert_eq!(go.model_id, "deepseek-v4.1-flash");
        let ordinary = parse_opencode_route("opencode/deepseek-v4.1-flash").unwrap();
        assert_eq!(ordinary.provider_id, "opencode");
        assert_ne!(go.provider_id, ordinary.provider_id);

        let body = opencode_request(&go, schema());
        assert_eq!(body["model"]["providerID"], "opencode-go");
        assert_eq!(body["model"]["modelID"], "deepseek-v4.1-flash");
        assert_eq!(body["tools"]["bash"], false);
        assert_eq!(body["format"]["type"], "json_schema");
    }

    #[test]
    fn unknown_or_malformed_opencode_routes_fail_explicitly() {
        for route in [
            "",
            "deepseek-v4.1-flash",
            "/deepseek",
            "opencode/",
            "opencode/",
            "other/model",
            "opencode/a/b",
        ] {
            assert!(parse_opencode_route(route).is_err(), "{route}");
        }
    }

    #[test]
    fn credential_and_protocol_failures_are_classified() {
        assert_eq!(classify_http_failure(401), "LIVE_PROVIDER_AUTH_BLOCKED");
        assert_eq!(classify_http_failure(403), "LIVE_PROVIDER_AUTH_BLOCKED");
        assert_eq!(classify_http_failure(422), "LIVE_PROVIDER_PROTOCOL_BLOCKED");
        assert_eq!(classify_http_failure(500), "LIVE_PROVIDER_PROTOCOL_BLOCKED");
        assert_eq!(classify_http_failure(400), "LIVE_VALIDATION_FAILED");
    }
}
