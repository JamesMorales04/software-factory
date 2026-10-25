//! Trusted Worker: Core lease coordination and Supervisor-mediated execution.
//!
//! The Worker never mounts or executes a project directly. It asks the trusted
//! Supervisor for a durable per-Attempt workspace, sends a bounded command into
//! an allowlisted rootless Podman sandbox, and reports the exact candidate. No
//! provider credentials, Podman socket or host workspace path reach the
//! sandbox.

use std::{env, time::Duration};

use factory_native::db::{ActionSpec, ProviderTerminal};
use factory_native::execution::{
    ApplyFilesRequest, ExecuteWorkspace, ExecutionResult, ToolRunRequest, WorkspaceAllocation,
    WorkspaceHandle, WorkspaceStatus, WriteFilesRequest,
};
use factory_native::provider_patch::{
    ProviderAction, ProviderUsage, command_matches_frozen_check, extract_action, extract_plan,
    extract_usage,
};
use factory_native::provider_sandbox::{ProviderSandbox, ProviderSandboxConfig};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CORE_TIMEOUT: Duration = Duration::from_secs(30);
const SUPERVISOR_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Debug, Deserialize)]
struct ClaimResponse {
    dispatch_supported: bool,
    assignment: Option<Assignment>,
}

#[derive(Debug, Deserialize)]
struct Assignment {
    lease: Lease,
    action_id: String,
    action_spec: ActionSpec,
    action_spec_sha256: String,
    repository: String,
    execution_mode: String,
    kind: String,
    integration_candidates: Vec<String>,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    provider_mode: String,
    #[serde(default)]
    delegation_id: String,
    #[serde(default)]
    max_sessions: u32,
    #[serde(default)]
    max_turns: u32,
    #[serde(default)]
    max_input_tokens: u64,
    #[serde(default)]
    max_output_tokens: u64,
    #[serde(default)]
    max_cost_micros: u64,
    #[serde(default)]
    deadline_secs: u64,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    attempt_grant: String,
}

#[derive(Debug, Deserialize)]
struct Lease {
    attempt_id: String,
    generation: i64,
}

struct Supervisor {
    client: reqwest::Client,
    base: reqwest::Url,
    token: String,
    image: String,
    timeout_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap_token = env::var("FACTORY_WORKER_BOOTSTRAP_TOKEN")
        .map_err(|_| "FACTORY_WORKER_BOOTSTRAP_TOKEN required")?;
    let worker_id = env::var("FACTORY_WORKER_ID").map_err(|_| "FACTORY_WORKER_ID required")?;
    let registration_key = env::var("FACTORY_WORKER_REGISTRATION_KEY")
        .map_err(|_| "FACTORY_WORKER_REGISTRATION_KEY required")?;
    if bootstrap_token.is_empty() || worker_id.trim().is_empty() || registration_key.is_empty() {
        return Err(
            "Worker bootstrap token, identity and registration key must not be empty".into(),
        );
    }
    let core = validated_loopback_url(
        &env::var("FACTORY_CORE_URL").unwrap_or_else(|_| "http://127.0.0.1:8765".into()),
        "FACTORY_CORE_URL",
    )?;
    let supervisor = Supervisor {
        client: reqwest::Client::builder()
            .timeout(SUPERVISOR_TIMEOUT)
            .build()?,
        base: validated_loopback_url(
            &env::var("FACTORY_SUPERVISOR_URL").map_err(|_| "FACTORY_SUPERVISOR_URL required")?,
            "FACTORY_SUPERVISOR_URL",
        )?,
        token: env::var("FACTORY_SUPERVISOR_TOKEN")
            .map_err(|_| "FACTORY_SUPERVISOR_TOKEN required")?,
        image: env::var("FACTORY_WORKER_FIXTURE_IMAGE").unwrap_or_else(|_| "alpine:3.20".into()),
        timeout_secs: env::var("FACTORY_WORKER_FIXTURE_TIMEOUT_SECS")
            .unwrap_or_else(|_| "120".into())
            .parse()?,
    };
    if supervisor.token.len() < 32 {
        return Err("FACTORY_SUPERVISOR_TOKEN must be at least 32 bytes".into());
    }
    if supervisor.timeout_secs == 0 || supervisor.timeout_secs > 600 {
        return Err("FACTORY_WORKER_FIXTURE_TIMEOUT_SECS must be between 1 and 600".into());
    }
    let once = env::var("FACTORY_WORKER_ONCE").ok().as_deref() == Some("1");
    let client = reqwest::Client::builder().timeout(CORE_TIMEOUT).build()?;
    let response = client
        .post(core.join("worker/register")?)
        .bearer_auth(bootstrap_token)
        .json(&json!({"worker_id":worker_id,"registration_key":registration_key}))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Core refused Worker registration: {}", response.status()).into());
    }
    let body: serde_json::Value = response.json().await?;
    let session_token = body
        .get("session_token")
        .and_then(serde_json::Value::as_str)
        .ok_or("Core did not return a Worker session credential")?
        .to_owned();
    if body.get("worker_id").and_then(serde_json::Value::as_str) != Some(worker_id.as_str()) {
        return Err("Core returned a different Worker identity".into());
    }

    if env::var("FACTORY_WORKER_RECOVER").ok().as_deref() == Some("1") {
        recover_provider_sessions(&client, &core, &session_token, &worker_id).await?;
        return Ok(());
    }

    loop {
        let response = client
            .post(core.join("worker/claim")?)
            .bearer_auth(&session_token)
            .json(&json!({"worker_id":worker_id}))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(format!("Core refused Worker claim: {}", response.status()).into());
        }
        let claim: ClaimResponse = response.json().await?;
        if !claim.dispatch_supported {
            return Err("Core reports dispatch unsupported for this Worker".into());
        }
        let Some(assignment) = claim.assignment else {
            if once {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        if !matches!(
            assignment.execution_mode.as_str(),
            "FIXTURE" | "TRUSTED_LOCAL_PROVIDER"
        ) {
            return Err(format!(
                "Worker refuses unverified execution mode {}",
                assignment.execution_mode
            )
            .into());
        }
        let heartbeat_client = client.clone();
        let heartbeat_url = core.join("worker/heartbeat")?;
        let heartbeat_token = session_token.clone();
        let heartbeat_worker = worker_id.clone();
        let heartbeat_attempt = assignment.lease.attempt_id.clone();
        let heartbeat_generation = assignment.lease.generation;
        let heartbeat = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(20));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let response = heartbeat_client
                    .post(heartbeat_url.clone())
                    .bearer_auth(&heartbeat_token)
                    .json(&json!({
                        "attempt_id": heartbeat_attempt,
                        "worker_id": heartbeat_worker,
                        "generation": heartbeat_generation
                    }))
                    .send()
                    .await;
                if response
                    .map(|value| !value.status().is_success())
                    .unwrap_or(true)
                {
                    return;
                }
            }
        });
        let execution = if assignment.kind == "INTEGRATION" {
            execute_assignment(
                &client,
                &core,
                &session_token,
                &worker_id,
                &assignment,
                &supervisor,
            )
            .await
        } else if assignment.execution_mode == "TRUSTED_LOCAL_PROVIDER" {
            execute_provider_assignment(
                &client,
                &core,
                &session_token,
                &worker_id,
                &assignment,
                &supervisor,
            )
            .await
        } else {
            execute_assignment(
                &client,
                &core,
                &session_token,
                &worker_id,
                &assignment,
                &supervisor,
            )
            .await
        };
        heartbeat.abort();
        match execution {
            Ok(candidate) => {
                let endpoint = if assignment.kind == "INTEGRATION" {
                    "worker/integrated_candidate"
                } else {
                    "worker/candidate"
                };
                let response = client
                    .post(core.join(endpoint)?)
                    .bearer_auth(&session_token)
                    .json(&json!({
                        "attempt_id": assignment.lease.attempt_id,
                        "worker_id": worker_id,
                        "generation": assignment.lease.generation,
                        "action_spec_sha256": assignment.action_spec_sha256,
                        "commit_sha": candidate.commit_sha,
                        "tree_sha": candidate.tree_sha
                    }))
                    .send()
                    .await?;
                if !response.status().is_success() {
                    return Err(format!(
                        "Core rejected candidate: {} {}",
                        response.status(),
                        response.text().await.unwrap_or_default()
                    )
                    .into());
                }
                println!(
                    "sandbox candidate accepted for action {}",
                    assignment.action_id
                );
            }
            Err(error) => {
                let message = error.to_string();
                let response = client
                    .post(core.join("worker/report")?)
                    .bearer_auth(&session_token)
                    .json(&json!({
                        "attempt_id": assignment.lease.attempt_id,
                        "worker_id": worker_id,
                        "generation": assignment.lease.generation,
                        "receipt_id": Uuid::new_v4().to_string(),
                        "result_sha256": sha256_hex(message.as_bytes()),
                        "terminal_status": "FAILED"
                    }))
                    .send()
                    .await?;
                if !response.status().is_success() {
                    eprintln!(
                        "Core refused failed-attempt report: {} {}",
                        response.status(),
                        response.text().await.unwrap_or_default()
                    );
                }
                eprintln!("sandbox execution failed: {message}");
                if once {
                    return Err(error);
                }
            }
        }
        if once {
            return Ok(());
        }
    }
}

async fn recover_provider_sessions(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    worker_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let sessions: Vec<serde_json::Value> = client
        .get(core.join("worker/provider_unresolved")?)
        .bearer_auth(session_token)
        .query(&[("worker_id", worker_id)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mode = env::var("FACTORY_PROVIDER_SANDBOX").unwrap_or_default();
    let mut failures = Vec::new();
    for session in sessions {
        let session_record_id = session["session_record_id"]
            .as_str()
            .ok_or("unresolved provider session has no record id")?
            .to_owned();
        let generation = session["generation"]
            .as_i64()
            .ok_or("unresolved provider session has no generation")?;
        let provider_session_id = session["provider_session_id"].as_str().unwrap_or("");
        let directory = session["provider_directory"].as_str().unwrap_or("");
        let receipt = match mode.as_str() {
            "bwrap" => confined_recovery_receipt(directory, provider_session_id),
            "unconfined" => unconfined_recovery_receipt(directory, provider_session_id).await,
            _ => Err("provider sandbox is not configured for recovery".into()),
        };
        let receipt = match receipt {
            Ok(receipt) => receipt,
            Err(error) => {
                failures.push(format!("{session_record_id}: {error}"));
                continue;
            }
        };
        let response = client
            .post(core.join("worker/provider_cleanup")?)
            .bearer_auth(session_token)
            .json(&json!({
                "session_record_id": session_record_id,
                "worker_id": worker_id,
                "generation": generation,
                "cleanup_receipt": receipt
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            failures.push(format!(
                "{session_record_id}: cleanup rejected with {}",
                response.status()
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unresolved provider cleanup failed: {}",
            failures.join("; ")
        )
        .into())
    }
}

fn confined_recovery_receipt(
    directory: &str,
    provider_session_id: &str,
) -> Result<&'static str, String> {
    let root = env::var("FACTORY_PROVIDER_SESSION_ROOT")
        .map_err(|_| "FACTORY_PROVIDER_SESSION_ROOT required for confined recovery")?;
    factory_native::provider_sandbox::confined_recovery_receipt(
        directory,
        provider_session_id,
        std::path::Path::new(&root),
    )
}

async fn unconfined_recovery_receipt(
    directory: &str,
    provider_session_id: &str,
) -> Result<&'static str, String> {
    if provider_session_id.is_empty() {
        return Ok("no_external_session");
    }
    let base_url = validated_loopback_url(
        &env::var("FACTORY_PROVIDER_BASE_URL")
            .map_err(|_| "FACTORY_PROVIDER_BASE_URL required for recovery")?,
        "FACTORY_PROVIDER_BASE_URL",
    )
    .map_err(|error| error.to_string())?;
    let password = env::var("FACTORY_PROVIDER_PASSWORD")
        .map_err(|_| "FACTORY_PROVIDER_PASSWORD required for recovery")?;
    if password.len() < 16 {
        return Err("FACTORY_PROVIDER_PASSWORD must be at least 16 bytes".into());
    }
    factory_native::telemetry::register_secret(&password);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    factory_native::runtime::delete_opencode_session(
        &client,
        base_url.as_str(),
        &password,
        directory,
        provider_session_id,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok("session_delete_verified_absent")
}

fn load_source_context(
    repository: &str,
    base_commit: &str,
    allowed_paths: &[String],
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let listing = std::process::Command::new("git")
        .args([
            "-C",
            repository,
            "ls-tree",
            "-r",
            "--name-only",
            base_commit,
        ])
        .output()?;
    if !listing.status.success() {
        return Err("source context Git listing failed".into());
    }
    let mut files = Vec::new();
    let mut total = 0usize;
    for path in String::from_utf8_lossy(&listing.stdout).lines() {
        if files.len() >= 16 || total >= 64 * 1024 {
            break;
        }
        if !factory_native::lead::is_safe_relative_path(path)
            || !factory_native::workspace::path_covered(path, allowed_paths)
            || is_sensitive_source_path(path)
        {
            continue;
        }
        let output = std::process::Command::new("git")
            .args(["-C", repository, "show", &format!("{base_commit}:{path}")])
            .output()?;
        if !output.status.success() || output.stdout.len() > 16 * 1024 {
            continue;
        }
        let Ok(content) = String::from_utf8(output.stdout) else {
            continue;
        };
        if is_sensitive_source_content(&content) {
            continue;
        }
        total = total.saturating_add(content.len());
        files.push(json!({"path": path, "content": content}));
    }
    Ok(files)
}

fn is_sensitive_source_content(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "api_token",
        "github_token",
        "gh_token",
        "token=",
        "token:",
        "secret",
        "password",
        "private_key",
        "authorization:",
        "bearer ",
        "-----begin ",
        "access_token",
        "client_secret",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn is_sensitive_source_path(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    matches!(
        base.as_str(),
        ".env" | ".npmrc" | ".netrc" | "id_rsa" | "id_ed25519" | "credentials"
    ) || base.contains("secret")
        || base.contains("credential")
        || base.contains("password")
        || base.ends_with(".pem")
        || base.ends_with(".key")
}

struct ProviderRun {
    candidate: factory_native::workspace::CandidateRevision,
    provider_session_id: String,
    turn_id: String,
    usage: ProviderUsage,
}

struct ProviderContext<'a> {
    client: &'a reqwest::Client,
    core: &'a reqwest::Url,
    session_token: &'a str,
    worker_id: &'a str,
    session_record_id: &'a str,
    base_url: &'a str,
    password: &'a str,
    directory: &'a str,
    assignment: &'a Assignment,
    supervisor: &'a Supervisor,
}

async fn execute_provider_assignment(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    worker_id: &str,
    assignment: &Assignment,
    supervisor: &Supervisor,
) -> Result<factory_native::workspace::CandidateRevision, Box<dyn std::error::Error>> {
    let mut seen_tools = std::collections::BTreeSet::new();
    if assignment.allowed_tools.iter().any(|tool| {
        !matches!(tool.as_str(), "write_file" | "run_command") || !seen_tools.insert(tool)
    }) {
        return Err("provider assignment has an unsupported tool permission".into());
    }
    if assignment.deadline_secs == 0
        || !(1..=4).contains(&assignment.max_turns)
        || assignment.max_sessions == 0
        || assignment.max_input_tokens == 0
        || assignment.max_output_tokens == 0
        || assignment.max_cost_micros == 0
        || assignment.delegation_id.is_empty()
    {
        return Err("provider assignment has an unsupported deadline or budget".into());
    }
    let provider_root = env::var_os("FACTORY_PROVIDER_SESSION_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&provider_root)?;
    let provider_dir = provider_root.join(format!("p-{}", Uuid::new_v4().simple()));
    std::fs::create_dir(&provider_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&provider_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let directory = provider_dir.canonicalize()?.to_string_lossy().to_string();
    let intent_url = core.join("worker/provider_intent")?;
    let intent_result = match client
        .post(intent_url)
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": assignment.lease.attempt_id,
            "worker_id": worker_id,
            "generation": assignment.lease.generation,
            "provider": assignment.provider,
            "model": assignment.model,
            "provider_mode": assignment.provider_mode,
            "provider_directory": directory
        }))
        .send()
        .await
    {
        Ok(response) => match response.error_for_status() {
            Ok(response) => response.json().await,
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    };
    let intent: serde_json::Value = match intent_result {
        Ok(intent) => intent,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&provider_dir);
            return Err(error.into());
        }
    };
    let session_record_id = intent
        .get("session_record_id")
        .and_then(serde_json::Value::as_str)
        .ok_or("Core provider intent returned no record id")?
        .to_owned();

    let (base_url, password, sandbox) =
        match start_provider_runtime(&provider_dir, assignment.deadline_secs).await {
            Ok(runtime) => runtime,
            Err(error) => {
                let terminal = ProviderTerminal {
                    session_record_id: session_record_id.clone(),
                    worker_id: worker_id.into(),
                    generation: assignment.lease.generation,
                    state: "FAILED".into(),
                    provider_session_id: String::new(),
                    turn_id: String::new(),
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    cost_micros: 0,
                    usage_known: false,
                    cleanup_receipt: "no_external_session".into(),
                    error: error.to_string(),
                };
                let _ = post_provider_terminal(client, core, session_token, &terminal).await;
                let _ = std::fs::remove_dir_all(&provider_dir);
                return Err(error);
            }
        };

    let result = if assignment.allowed_tools.is_empty() {
        run_provider_patch(&ProviderContext {
            client,
            core,
            session_token,
            worker_id,
            session_record_id: &session_record_id,
            base_url: base_url.as_str(),
            password: &password,
            directory: &directory,
            assignment,
            supervisor,
        })
        .await
    } else {
        run_provider_tool_loop(&ProviderContext {
            client,
            core,
            session_token,
            worker_id,
            session_record_id: &session_record_id,
            base_url: base_url.as_str(),
            password: &password,
            directory: &directory,
            assignment,
            supervisor,
        })
        .await
    };
    if let Some(sandbox) = sandbox {
        let _ = sandbox.stop().await;
    }
    if !(cfg!(debug_assertions)
        && env::var("FACTORY_PROVIDER_KEEP_SESSION").ok().as_deref() == Some("1"))
    {
        let _ = std::fs::remove_dir_all(&provider_dir);
    }
    match result {
        Ok(run) => {
            if !run.usage.known {
                let terminal = ProviderTerminal {
                    session_record_id: session_record_id.clone(),
                    worker_id: worker_id.into(),
                    generation: assignment.lease.generation,
                    state: "FAILED".into(),
                    provider_session_id: run.provider_session_id.clone(),
                    turn_id: run.turn_id.clone(),
                    input_tokens: run.usage.input_tokens,
                    output_tokens: run.usage.output_tokens,
                    total_tokens: run.usage.total_tokens,
                    cost_micros: run.usage.cost_micros,
                    usage_known: run.usage.known,
                    cleanup_receipt: String::new(),
                    error: "provider usage was not measurable".into(),
                };
                let _ = post_provider_terminal(client, core, session_token, &terminal).await;
                return Err(
                    "provider usage was not measurable; refusing an unaccounted candidate".into(),
                );
            }
            let terminal = ProviderTerminal {
                session_record_id: session_record_id.clone(),
                worker_id: worker_id.into(),
                generation: assignment.lease.generation,
                state: "COMPLETED".into(),
                provider_session_id: run.provider_session_id.clone(),
                turn_id: run.turn_id.clone(),
                input_tokens: run.usage.input_tokens,
                output_tokens: run.usage.output_tokens,
                total_tokens: run.usage.total_tokens,
                cost_micros: run.usage.cost_micros,
                usage_known: run.usage.known,
                cleanup_receipt: "session_delete_verified_absent".into(),
                error: String::new(),
            };
            post_provider_terminal(client, core, session_token, &terminal).await?;
            Ok(run.candidate)
        }
        Err(error) => {
            let terminal = ProviderTerminal {
                session_record_id: session_record_id.clone(),
                worker_id: worker_id.into(),
                generation: assignment.lease.generation,
                state: "FAILED".into(),
                provider_session_id: String::new(),
                turn_id: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_micros: 0,
                usage_known: false,
                cleanup_receipt: String::new(),
                error: error.to_string(),
            };
            let _ = post_provider_terminal(client, core, session_token, &terminal).await;
            Err(error)
        }
    }
}

async fn start_provider_runtime(
    session_dir: &std::path::Path,
    deadline_secs: u64,
) -> Result<(String, String, Option<ProviderSandbox>), Box<dyn std::error::Error>> {
    let mode = env::var("FACTORY_PROVIDER_SANDBOX").unwrap_or_default();
    match mode.as_str() {
        "bwrap" => {
            let binary = env::var("FACTORY_PROVIDER_BINARY")
                .map_err(|_| "FACTORY_PROVIDER_BINARY required for sandboxed provider")?;
            let args: Vec<String> = match env::var("FACTORY_PROVIDER_ARGS") {
                Ok(value) => serde_json::from_str(&value)
                    .map_err(|_| "FACTORY_PROVIDER_ARGS must be a JSON string array")?,
                Err(_) => Vec::new(),
            };
            let max_session_bytes = env::var("FACTORY_PROVIDER_SESSION_MAX_BYTES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(64 * 1024 * 1024);
            let lifetime = Duration::from_secs(deadline_secs.saturating_add(30).min(3600));
            let (egress, egress_bridge) = match env::var("FACTORY_PROVIDER_UPSTREAM_URL") {
                Ok(upstream) => {
                    let credential = env::var("FACTORY_PROVIDER_UPSTREAM_TOKEN").map_err(
                        |_| "FACTORY_PROVIDER_UPSTREAM_TOKEN required for provider egress",
                    )?;
                    if credential.len() < 16 {
                        return Err(
                            "FACTORY_PROVIDER_UPSTREAM_TOKEN must be at least 16 bytes".into()
                        );
                    }
                    factory_native::telemetry::register_secret(&credential);
                    let token = env::var("FACTORY_PROVIDER_EGRESS_TOKEN").unwrap_or_else(|_| {
                        format!("{}.{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
                    });
                    let bridge = env::var("FACTORY_PROVIDER_EGRESS_BRIDGE").map_err(
                        |_| "FACTORY_PROVIDER_EGRESS_BRIDGE required for provider egress",
                    )?;
                    let allow_test_upstream =
                        env::var("FACTORY_ALLOW_TEST_UPSTREAM").ok().as_deref() == Some("1");
                    let config = factory_native::provider_egress::EgressConfig::new(
                        &upstream,
                        credential,
                        token,
                        allow_test_upstream,
                    )?;
                    (Some(config), Some(std::path::PathBuf::from(bridge)))
                }
                Err(_) => (None, None),
            };
            let sandbox = ProviderSandbox::start(ProviderSandboxConfig {
                binary: std::path::PathBuf::from(binary),
                session_dir: session_dir.to_path_buf(),
                args,
                ready_timeout: Duration::from_secs(15),
                max_session_bytes,
                max_lifetime: lifetime,
                bridge_timeout: lifetime,
                egress,
                egress_bridge,
                ..Default::default()
            })
            .await?;
            Ok((
                sandbox.base_url().to_owned(),
                sandbox.password().to_owned(),
                Some(sandbox),
            ))
        }
        "unconfined" => {
            if env::var("FACTORY_ALLOW_UNCONFINED_PROVIDER")
                .ok()
                .as_deref()
                != Some("1")
            {
                return Err(
                    "unconfined provider requires explicit FACTORY_ALLOW_UNCONFINED_PROVIDER=1"
                        .into(),
                );
            }
            let base_url = validated_loopback_url(
                &env::var("FACTORY_PROVIDER_BASE_URL")
                    .map_err(|_| "FACTORY_PROVIDER_BASE_URL required for unconfined provider")?,
                "FACTORY_PROVIDER_BASE_URL",
            )?;
            let password = env::var("FACTORY_PROVIDER_PASSWORD")
                .map_err(|_| "FACTORY_PROVIDER_PASSWORD required for unconfined provider")?;
            if password.len() < 16 {
                return Err("FACTORY_PROVIDER_PASSWORD must be at least 16 bytes".into());
            }
            factory_native::telemetry::register_secret(&password);
            Ok((base_url.to_string(), password, None))
        }
        _ => Err("provider sandbox is not configured; refusing an unconfined provider".into()),
    }
}

async fn run_provider_tool_loop(
    context: &ProviderContext<'_>,
) -> Result<ProviderRun, Box<dyn std::error::Error>> {
    let directory = context.directory.to_owned();
    let http = factory_native::runtime::opencode_http_client(Duration::from_secs(
        context.assignment.deadline_secs,
    ))?;
    let event_response = factory_native::runtime::open_opencode_event_stream(
        &http,
        context.base_url,
        context.password,
        &directory,
    )
    .await?;
    let mut event_stream = event_response.bytes_stream();
    let mut parser = factory_native::sse::SseParser::new(1_048_576);
    let created = factory_native::runtime::create_opencode_session(
        &http,
        context.base_url,
        context.password,
        &directory,
        &format!(
            "Factory provider attempt {}",
            context.assignment.lease.attempt_id
        ),
    )
    .await?;
    let session_id = created
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or("OpenCode session/create returned no id")?
        .to_owned();
    let _: serde_json::Value = context
        .client
        .post(context.core.join("worker/provider_active")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation,
            "provider_session_id": session_id,
            "provider_directory": directory
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    factory_native::runtime::wait_for_session_created_event(
        &mut event_stream,
        &mut parser,
        &session_id,
    )
    .await?;

    let allocation = WorkspaceAllocation {
        attempt_id: context.assignment.lease.attempt_id.clone(),
        repository: context.assignment.repository.clone(),
        base_commit: context.assignment.action_spec.base_commit.clone(),
        generation: context.assignment.lease.generation,
        action_spec_sha256: context.assignment.action_spec_sha256.clone(),
        attempt_grant: context.assignment.attempt_grant.clone(),
    };
    let response = context
        .supervisor
        .client
        .post(context.supervisor.base.join("v1/workspaces")?)
        .bearer_auth(&context.supervisor.token)
        .json(&allocation)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Supervisor refused provider workspace allocation: {}",
            response.status()
        )
        .into());
    }
    let handle: WorkspaceHandle = response.json().await?;

    let source_context = load_source_context(
        &context.assignment.repository,
        &context.assignment.action_spec.base_commit,
        &context.assignment.action_spec.allowed_paths,
    )?;
    let mut prompt = json!({
        "model": {"providerID": context.assignment.provider, "modelID": context.assignment.model},
        "parts": [{"type": "text", "text": format!(
            "You are a bounded engineering agent. You may request only these tools: {}. Return ONE JSON object per turn: either {{\"type\":\"tool_request\",\"call_id\":\"unique\",\"tool\":\"write_file\",\"path\":\"relative/path\",\"content\":\"...\"}} or {{\"type\":\"tool_request\",\"call_id\":\"unique\",\"tool\":\"run_command\",\"argv\":[\"program\",\"arg\"],\"timeout_secs\":30}} or {{\"type\":\"final\",\"files\":[]}}. Allowed paths: {}. Objective: {}. Source context: {}",
            context.assignment.allowed_tools.join(", "),
            context.assignment.action_spec.allowed_paths.join(", "),
            context.assignment.action_spec.objective,
            serde_json::to_string(&source_context)?
        )}]
    });
    let mut usage = ProviderUsage::default();
    let mut written = false;
    let mut call_ids = std::collections::BTreeSet::new();
    for _ in 0..context.assignment.max_turns {
        factory_native::runtime::submit_opencode_prompt(
            &http,
            context.base_url,
            context.password,
            &directory,
            &session_id,
            &prompt,
        )
        .await?;
        let terminal = tokio::time::timeout(
            Duration::from_secs(context.assignment.deadline_secs),
            factory_native::runtime::wait_for_opencode_prompt_terminal(
                &mut event_stream,
                &mut parser,
                &session_id,
                false,
            ),
        )
        .await
        .map_err(|_| "provider deadline exceeded")??;
        let message = match &terminal {
            factory_native::runtime::OpenCodePromptTerminal::Idle {
                cancel_requested: false,
                ..
            } => {
                factory_native::runtime::read_opencode_assistant_message(
                    &http,
                    context.base_url,
                    context.password,
                    &directory,
                    &session_id,
                )
                .await?
            }
            _ => return Err("provider turn did not complete normally".into()),
        };
        let outcome =
            factory_native::runtime::reconcile_opencode_prompt(&session_id, terminal, &message)?;
        let message_id = match outcome {
            factory_native::runtime::OpenCodePromptOutcome::Completed { message_id, .. } => {
                message_id
            }
            _ => return Err("provider turn failed".into()),
        };
        let turn_usage = extract_usage(&message);
        usage.input_tokens = usage.input_tokens.saturating_add(turn_usage.input_tokens);
        usage.output_tokens = usage.output_tokens.saturating_add(turn_usage.output_tokens);
        usage.total_tokens = usage.total_tokens.saturating_add(turn_usage.total_tokens);
        usage.cost_micros = usage.cost_micros.saturating_add(turn_usage.cost_micros);
        usage.known = usage.known || turn_usage.known;
        let action = extract_action(&message)
            .map_err(|error| format!("provider action rejected: {error}"))?;
        match action {
            ProviderAction::Final { files } => {
                if !files.is_empty() {
                    if !context
                        .assignment
                        .allowed_tools
                        .contains(&"write_file".into())
                    {
                        return Err("provider returned files without write_file permission".into());
                    }
                    let applied = files
                        .into_iter()
                        .map(|file| factory_native::execution::AppliedFile {
                            path: file.path,
                            content: file.content,
                        })
                        .collect::<Vec<_>>();
                    supervisor_write(
                        context,
                        &handle.workspace_id,
                        &handle.capability,
                        applied,
                        !written,
                    )
                    .await?;
                }
                factory_native::runtime::delete_opencode_session(
                    &http,
                    context.base_url,
                    context.password,
                    &directory,
                    &session_id,
                )
                .await?;
                let candidate =
                    supervisor_finalize(context, &handle.workspace_id, &handle.capability).await?;
                cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
                return Ok(ProviderRun {
                    candidate,
                    provider_session_id: session_id,
                    turn_id: message_id,
                    usage,
                });
            }
            ProviderAction::ToolRequest {
                call_id,
                tool,
                path,
                content,
                argv,
                timeout_secs,
            } => {
                if call_id.trim().is_empty() || !call_ids.insert(call_id.clone()) {
                    return Err("provider tool call id is empty or duplicated".into());
                }
                post_tool_event(context, &call_id, &tool, "REQUEST", "tool requested").await?;
                let state = provider_state(context).await?;
                if state == "CANCEL_REQUESTED" {
                    let _ = factory_native::runtime::request_opencode_abort(
                        &http,
                        context.base_url,
                        context.password,
                        &directory,
                        &session_id,
                    )
                    .await;
                    let _ = factory_native::runtime::delete_opencode_session(
                        &http,
                        context.base_url,
                        context.password,
                        &directory,
                        &session_id,
                    )
                    .await;
                    return Err("provider cancellation was requested".into());
                }
                let decision = decide_tool(context, &tool, &path, &content, &argv, timeout_secs);
                match decision {
                    Ok(ToolDecision::Write) => {
                        post_tool_event(context, &call_id, &tool, "ALLOW", "path authorized")
                            .await?;
                        let file = factory_native::execution::AppliedFile { path, content };
                        match supervisor_write(
                            context,
                            &handle.workspace_id,
                            &handle.capability,
                            vec![file],
                            !written,
                        )
                        .await
                        {
                            Ok(()) => {
                                written = true;
                                post_tool_event(
                                    context,
                                    &call_id,
                                    &tool,
                                    "RESULT",
                                    "write applied",
                                )
                                .await?;
                                prompt = tool_result(&call_id, "ok", "write applied", Some(0));
                            }
                            Err(error) => {
                                post_tool_event(context, &call_id, &tool, "ERROR", "write failed")
                                    .await?;
                                prompt = tool_result(&call_id, "failed", &error.to_string(), None);
                            }
                        }
                    }
                    Ok(ToolDecision::Run) => {
                        post_tool_event(
                            context,
                            &call_id,
                            &tool,
                            "ALLOW",
                            "frozen check authorized",
                        )
                        .await?;
                        match supervisor_run(
                            context,
                            &handle.workspace_id,
                            &handle.capability,
                            &argv,
                            timeout_secs,
                        )
                        .await
                        {
                            Ok(result) => {
                                let status = if result.exit_code == Some(0) {
                                    "ok"
                                } else {
                                    "failed"
                                };
                                post_tool_event(
                                    context,
                                    &call_id,
                                    &tool,
                                    "RESULT",
                                    &format!("exit={:?}", result.exit_code),
                                )
                                .await?;
                                prompt =
                                    tool_result(&call_id, status, &result.stderr, result.exit_code);
                            }
                            Err(error) => {
                                post_tool_event(
                                    context,
                                    &call_id,
                                    &tool,
                                    "ERROR",
                                    "command failed",
                                )
                                .await?;
                                prompt = tool_result(&call_id, "failed", &error.to_string(), None);
                            }
                        }
                    }
                    Err(reason) => {
                        post_tool_event(context, &call_id, &tool, "DENY", &reason).await?;
                        prompt = tool_result(&call_id, "denied", &reason, None);
                    }
                }
            }
        }
    }
    let _ = factory_native::runtime::delete_opencode_session(
        &http,
        context.base_url,
        context.password,
        &directory,
        &session_id,
    )
    .await;
    let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
    Err("provider exceeded the authorized turn budget".into())
}

enum ToolDecision {
    Write,
    Run,
}

fn decide_tool(
    context: &ProviderContext<'_>,
    tool: &str,
    path: &str,
    content: &str,
    argv: &[String],
    timeout_secs: u64,
) -> Result<ToolDecision, String> {
    if !context
        .assignment
        .allowed_tools
        .iter()
        .any(|allowed| allowed == tool)
    {
        return Err(format!("tool {tool} is not authorized"));
    }
    match tool {
        "write_file" => {
            if !factory_native::lead::is_safe_relative_path(path)
                || !factory_native::workspace::path_covered(
                    path,
                    &context.assignment.action_spec.allowed_paths,
                )
            {
                return Err("write path is outside the allowed scope".into());
            }
            if content.len() > 256 * 1024 {
                return Err("write content exceeds the per-file bound".into());
            }
            Ok(ToolDecision::Write)
        }
        "run_command" => {
            if !command_matches_frozen_check(argv, &context.assignment.action_spec.checks) {
                return Err("command does not match a frozen acceptance check".into());
            }
            if timeout_secs == 0 || timeout_secs > 300 {
                return Err("command timeout is outside the allowed bound".into());
            }
            Ok(ToolDecision::Run)
        }
        _ => Err(format!("unknown tool {tool}")),
    }
}

fn tool_result(
    call_id: &str,
    status: &str,
    output: &str,
    exit_code: Option<i32>,
) -> serde_json::Value {
    json!({
        "model": null,
        "parts": [{"type": "text", "text": serde_json::to_string(&json!({
            "type": "tool_result",
            "call_id": call_id,
            "status": status,
            "output": output,
            "exit_code": exit_code
        })).unwrap_or_default()}]
    })
}

async fn provider_state(
    context: &ProviderContext<'_>,
) -> Result<String, Box<dyn std::error::Error>> {
    let state: serde_json::Value = context
        .client
        .post(context.core.join("worker/provider_state")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(state
        .get("state")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("UNKNOWN")
        .to_owned())
}

async fn post_tool_event(
    context: &ProviderContext<'_>,
    call_id: &str,
    tool: &str,
    decision: &str,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = context
        .client
        .post(context.core.join("worker/provider_tool_event")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation,
            "call_id": call_id,
            "tool": tool,
            "decision": decision,
            "detail": detail
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Core rejected provider tool event: {}", response.status()).into());
    }
    Ok(())
}

struct WorkspaceOperation<'a> {
    client: &'a reqwest::Client,
    core: &'a reqwest::Url,
    session_token: &'a str,
    worker_id: &'a str,
    attempt_id: &'a str,
    generation: i64,
    workspace_id: &'a str,
    operation: &'a str,
}

async fn request_workspace_operation(
    request: WorkspaceOperation<'_>,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = request
        .client
        .post(request.core.join("worker/workspace_operation")?)
        .bearer_auth(request.session_token)
        .json(&json!({
            "attempt_id": request.attempt_id,
            "worker_id": request.worker_id,
            "generation": request.generation,
            "workspace_id": request.workspace_id,
            "operation": request.operation
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Core refused workspace operation {}: {}",
            request.operation,
            response.status()
        )
        .into());
    }
    let value: serde_json::Value = response.json().await?;
    value
        .get("grant")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Core returned no workspace operation grant".into())
}

async fn supervisor_write(
    context: &ProviderContext<'_>,
    workspace_id: &str,
    capability: &str,
    files: Vec<factory_native::execution::AppliedFile>,
    reset: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let operation = request_workspace_operation(WorkspaceOperation {
        client: context.client,
        core: context.core,
        session_token: context.session_token,
        worker_id: context.worker_id,
        attempt_id: &context.assignment.lease.attempt_id,
        generation: context.assignment.lease.generation,
        workspace_id,
        operation: "write",
    })
    .await?;
    let response = context
        .supervisor
        .client
        .post(
            context
                .supervisor
                .base
                .join(&format!("v1/workspaces/{workspace_id}/write"))?,
        )
        .bearer_auth(capability)
        .header("x-factory-operation-grant", operation)
        .json(&WriteFilesRequest {
            allowed_paths: context.assignment.action_spec.allowed_paths.clone(),
            files,
            reset,
        })
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Supervisor write failed: {}", response.status()).into());
    }
    Ok(())
}

async fn supervisor_run(
    context: &ProviderContext<'_>,
    workspace_id: &str,
    capability: &str,
    argv: &[String],
    timeout_secs: u64,
) -> Result<ExecutionResult, Box<dyn std::error::Error>> {
    let operation = request_workspace_operation(WorkspaceOperation {
        client: context.client,
        core: context.core,
        session_token: context.session_token,
        worker_id: context.worker_id,
        attempt_id: &context.assignment.lease.attempt_id,
        generation: context.assignment.lease.generation,
        workspace_id,
        operation: "run",
    })
    .await?;
    let response = context
        .supervisor
        .client
        .post(
            context
                .supervisor
                .base
                .join(&format!("v1/workspaces/{workspace_id}/run"))?,
        )
        .bearer_auth(capability)
        .header("x-factory-operation-grant", operation)
        .json(&ToolRunRequest {
            image: context.supervisor.image.clone(),
            argv: argv.to_vec(),
            timeout_secs,
        })
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Supervisor run failed: {}", response.status()).into());
    }
    Ok(response.json().await?)
}

async fn supervisor_finalize(
    context: &ProviderContext<'_>,
    workspace_id: &str,
    capability: &str,
) -> Result<factory_native::workspace::CandidateRevision, Box<dyn std::error::Error>> {
    let operation = request_workspace_operation(WorkspaceOperation {
        client: context.client,
        core: context.core,
        session_token: context.session_token,
        worker_id: context.worker_id,
        attempt_id: &context.assignment.lease.attempt_id,
        generation: context.assignment.lease.generation,
        workspace_id,
        operation: "finalize",
    })
    .await?;
    let response = context
        .supervisor
        .client
        .post(
            context
                .supervisor
                .base
                .join(&format!("v1/workspaces/{workspace_id}/finalize"))?,
        )
        .bearer_auth(capability)
        .header("x-factory-operation-grant", operation)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Supervisor finalize failed: {}", response.status()).into());
    }
    Ok(response.json().await?)
}

async fn run_provider_patch(
    context: &ProviderContext<'_>,
) -> Result<ProviderRun, Box<dyn std::error::Error>> {
    let directory = context.directory.to_owned();
    let allowed = context.assignment.action_spec.allowed_paths.join(", ");
    let source_context = load_source_context(
        &context.assignment.repository,
        &context.assignment.action_spec.base_commit,
        &context.assignment.action_spec.allowed_paths,
    )?;
    let tool_instruction = if context.assignment.allowed_tools.is_empty() {
        "Return ONLY a JSON object of the form {\"files\":[{\"path\":\"relative/path\",\"content\":\"...\"}]}."
            .to_string()
    } else {
        format!(
            "You may request only these tools: {}. Return ONLY a JSON object of the form {{\"files\":[{{\"path\":\"relative/path\",\"content\":\"...\"}}],\"commands\":[{{\"argv\":[\"program\",\"arg\"],\"timeout_secs\":30}}]}}. Commands must exactly match one frozen acceptance check.",
            context.assignment.allowed_tools.join(", ")
        )
    };
    let prompt = json!({
        "model": {"providerID": context.assignment.provider, "modelID": context.assignment.model},
        "parts": [{
            "type": "text",
            "text": format!(
                "You are a bounded code generator. {tool_instruction} Allowed paths: {allowed}. Objective: {}. Bounded source context: {}",
                context.assignment.action_spec.objective,
                serde_json::to_string(&source_context)?
            )
        }]
    });
    let http = factory_native::runtime::opencode_http_client(Duration::from_secs(
        context.assignment.deadline_secs,
    ))?;
    let event_response = factory_native::runtime::open_opencode_event_stream(
        &http,
        context.base_url,
        context.password,
        &directory,
    )
    .await?;
    let mut event_stream = event_response.bytes_stream();
    let mut parser = factory_native::sse::SseParser::new(1_048_576);
    let created = factory_native::runtime::create_opencode_session(
        &http,
        context.base_url,
        context.password,
        &directory,
        &format!(
            "Factory provider attempt {}",
            context.assignment.lease.attempt_id
        ),
    )
    .await?;
    let provider_session_id = created
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or("OpenCode session/create returned no id")?
        .to_owned();
    let active: serde_json::Value = context
        .client
        .post(context.core.join("worker/provider_active")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation,
            "provider_session_id": provider_session_id,
            "provider_directory": directory
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if active.get("state").and_then(serde_json::Value::as_str) != Some("ACTIVE") {
        let _ = factory_native::runtime::delete_opencode_session(
            &http,
            context.base_url,
            context.password,
            &directory,
            &provider_session_id,
        )
        .await;
        return Err("Core did not bind the provider session as ACTIVE".into());
    }
    factory_native::runtime::wait_for_session_created_event(
        &mut event_stream,
        &mut parser,
        &provider_session_id,
    )
    .await?;
    factory_native::runtime::submit_opencode_prompt(
        &http,
        context.base_url,
        context.password,
        &directory,
        &provider_session_id,
        &prompt,
    )
    .await?;
    let terminal = tokio::time::timeout(
        Duration::from_secs(context.assignment.deadline_secs),
        factory_native::runtime::wait_for_opencode_prompt_terminal(
            &mut event_stream,
            &mut parser,
            &provider_session_id,
            false,
        ),
    )
    .await
    .map_err(|_| "provider deadline exceeded")??;
    let response = match &terminal {
        factory_native::runtime::OpenCodePromptTerminal::Idle {
            cancel_requested: false,
            ..
        } => {
            factory_native::runtime::read_opencode_assistant_message(
                &http,
                context.base_url,
                context.password,
                &directory,
                &provider_session_id,
            )
            .await?
        }
        _ => serde_json::Value::Null,
    };
    let outcome = factory_native::runtime::reconcile_opencode_prompt(
        &provider_session_id,
        terminal,
        &response,
    )?;
    let (session_id, turn_id) = match outcome {
        factory_native::runtime::OpenCodePromptOutcome::Completed {
            session_id,
            message_id,
        } => (session_id, message_id),
        factory_native::runtime::OpenCodePromptOutcome::Failed { session_id, error } => {
            let _ = factory_native::runtime::delete_opencode_session(
                &http,
                context.base_url,
                context.password,
                &directory,
                &session_id,
            )
            .await;
            return Err(format!("provider session {session_id} failed: {error}").into());
        }
        factory_native::runtime::OpenCodePromptOutcome::AbortRequestedAndIdle { session_id } => {
            let _ = factory_native::runtime::delete_opencode_session(
                &http,
                context.base_url,
                context.password,
                &directory,
                &session_id,
            )
            .await;
            return Err(format!("provider session {session_id} was aborted").into());
        }
    };
    let state: serde_json::Value = context
        .client
        .post(context.core.join("worker/provider_state")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if state.get("state").and_then(serde_json::Value::as_str) == Some("CANCEL_REQUESTED") {
        let _ = factory_native::runtime::request_opencode_abort(
            &http,
            context.base_url,
            context.password,
            &directory,
            &session_id,
        )
        .await;
        let _ = factory_native::runtime::delete_opencode_session(
            &http,
            context.base_url,
            context.password,
            &directory,
            &session_id,
        )
        .await;
        return Err("provider cancellation was requested".into());
    }
    factory_native::runtime::delete_opencode_session(
        &http,
        context.base_url,
        context.password,
        &directory,
        &session_id,
    )
    .await?;
    let plan = extract_plan(&response, &context.assignment.action_spec.allowed_paths)
        .map_err(|error| format!("provider plan rejected: {error}"))?;
    if plan.files.is_empty() {
        return Err("provider plan has no files".into());
    }
    if !context
        .assignment
        .allowed_tools
        .contains(&"write_file".into())
    {
        return Err("provider returned files without the write_file permission".into());
    }
    if !plan.commands.is_empty()
        && !context
            .assignment
            .allowed_tools
            .contains(&"run_command".into())
    {
        return Err("provider returned commands without the run_command permission".into());
    }
    for command in &plan.commands {
        if !factory_native::provider_patch::command_matches_frozen_check(
            &command.argv,
            &context.assignment.action_spec.checks,
        ) {
            return Err(format!(
                "provider command is not an authorized frozen check: {:?}",
                command.argv
            )
            .into());
        }
    }
    let usage = extract_usage(&response);
    let allocation = WorkspaceAllocation {
        attempt_id: context.assignment.lease.attempt_id.clone(),
        repository: context.assignment.repository.clone(),
        base_commit: context.assignment.action_spec.base_commit.clone(),
        generation: context.assignment.lease.generation,
        action_spec_sha256: context.assignment.action_spec_sha256.clone(),
        attempt_grant: context.assignment.attempt_grant.clone(),
    };
    let response = context
        .supervisor
        .client
        .post(context.supervisor.base.join("v1/workspaces")?)
        .bearer_auth(&context.supervisor.token)
        .json(&allocation)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Supervisor refused provider workspace allocation: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    let handle: WorkspaceHandle = response.json().await?;
    let files = plan
        .files
        .into_iter()
        .map(|file| factory_native::execution::AppliedFile {
            path: file.path,
            content: file.content,
        })
        .collect::<Vec<_>>();
    if context.assignment.allowed_tools.is_empty() {
        let apply = ApplyFilesRequest {
            allowed_paths: context.assignment.action_spec.allowed_paths.clone(),
            files,
        };
        let operation = request_workspace_operation(WorkspaceOperation {
            client: context.client,
            core: context.core,
            session_token: context.session_token,
            worker_id: context.worker_id,
            attempt_id: &context.assignment.lease.attempt_id,
            generation: context.assignment.lease.generation,
            workspace_id: &handle.workspace_id,
            operation: "apply",
        })
        .await?;
        let response = context
            .supervisor
            .client
            .post(
                context
                    .supervisor
                    .base
                    .join(&format!("v1/workspaces/{}/apply", handle.workspace_id))?,
            )
            .bearer_auth(&handle.capability)
            .header("x-factory-operation-grant", operation)
            .json(&apply)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
            return Err(format!("Supervisor provider apply failed: {status} {body}").into());
        }
        let candidate: factory_native::workspace::CandidateRevision = response.json().await?;
        cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
        return Ok(ProviderRun {
            candidate,
            provider_session_id: session_id,
            turn_id,
            usage,
        });
    }
    let write = WriteFilesRequest {
        allowed_paths: context.assignment.action_spec.allowed_paths.clone(),
        files,
        reset: true,
    };
    let response = context
        .supervisor
        .client
        .post(
            context
                .supervisor
                .base
                .join(&format!("v1/workspaces/{}/write", handle.workspace_id))?,
        )
        .bearer_auth(&handle.capability)
        .json(&write)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
        return Err(format!("Supervisor provider write failed: {status} {body}").into());
    }
    for command in plan.commands {
        let response = context
            .supervisor
            .client
            .post(
                context
                    .supervisor
                    .base
                    .join(&format!("v1/workspaces/{}/run", handle.workspace_id))?,
            )
            .bearer_auth(&handle.capability)
            .json(&ToolRunRequest {
                image: context.supervisor.image.clone(),
                argv: command.argv.clone(),
                timeout_secs: command.timeout_secs,
            })
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
            return Err(format!("Supervisor tool run failed: {status} {body}").into());
        }
        let result: ExecutionResult = response.json().await?;
        if result.exit_code != Some(0) {
            let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
            return Err(format!(
                "provider command {:?} failed with {:?}: {}",
                command.argv, result.exit_code, result.stderr
            )
            .into());
        }
    }
    let response = context
        .supervisor
        .client
        .post(
            context
                .supervisor
                .base
                .join(&format!("v1/workspaces/{}/finalize", handle.workspace_id))?,
        )
        .bearer_auth(&handle.capability)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
        return Err(format!("Supervisor finalize failed: {status} {body}").into());
    }
    let candidate: factory_native::workspace::CandidateRevision = response.json().await?;
    cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
    Ok(ProviderRun {
        candidate,
        provider_session_id: session_id,
        turn_id,
        usage,
    })
}

async fn post_provider_terminal(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    terminal: &ProviderTerminal,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client
        .post(core.join("worker/provider_terminal")?)
        .bearer_auth(session_token)
        .json(terminal)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Core rejected provider terminal evidence: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    Ok(())
}

async fn execute_assignment(
    client: &reqwest::Client,
    core: &reqwest::Url,
    session_token: &str,
    worker_id: &str,
    assignment: &Assignment,
    supervisor: &Supervisor,
) -> Result<factory_native::workspace::CandidateRevision, Box<dyn std::error::Error>> {
    if assignment.kind == "INTEGRATION" {
        return integrate_assignment(assignment, supervisor).await;
    }
    let map = env::var("FACTORY_FIXTURE_MAP")
        .ok()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok());
    let mapped = map
        .as_ref()
        .and_then(|value| value.get(&assignment.action_spec.objective));
    let file = mapped
        .and_then(|value| value.get("file"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            env::var("FACTORY_FIXTURE_FILE").unwrap_or_else(|_| "factory-fixture-change.txt".into())
        });
    if !factory_native::lead::is_safe_relative_path(&file) {
        return Err("fixture file path is unsafe".into());
    }
    let content = mapped
        .and_then(|value| value.get("content"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            env::var("FACTORY_FIXTURE_CONTENT").unwrap_or_else(|_| {
                format!(
                    "fixture candidate\nobjective: {}\n",
                    assignment.action_spec.objective
                )
            })
        });
    let allocation = WorkspaceAllocation {
        attempt_id: assignment.lease.attempt_id.clone(),
        repository: assignment.repository.clone(),
        base_commit: assignment.action_spec.base_commit.clone(),
        generation: assignment.lease.generation,
        action_spec_sha256: assignment.action_spec_sha256.clone(),
        attempt_grant: assignment.attempt_grant.clone(),
    };
    let response = supervisor
        .client
        .post(supervisor.base.join("v1/workspaces")?)
        .bearer_auth(&supervisor.token)
        .json(&allocation)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Supervisor refused workspace allocation: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    let handle: WorkspaceHandle = response.json().await?;
    if handle.status == WorkspaceStatus::Completed {
        return handle
            .candidate
            .ok_or_else(|| "completed workspace has no candidate".into());
    }
    let request = ExecuteWorkspace {
        image: supervisor.image.clone(),
        argv: vec![
            "/bin/sh".into(),
            "-c".into(),
            "mkdir -p \"$(dirname \"$2\")\" && printf '%s' \"$1\" > \"$2\"".into(),
            "factory-fixture".into(),
            content,
            file,
        ],
        timeout_secs: supervisor.timeout_secs,
    };
    let operation = request_workspace_operation(WorkspaceOperation {
        client,
        core,
        session_token,
        worker_id,
        attempt_id: &assignment.lease.attempt_id,
        generation: assignment.lease.generation,
        workspace_id: &handle.workspace_id,
        operation: "execute",
    })
    .await?;
    let response = supervisor
        .client
        .post(
            supervisor
                .base
                .join(&format!("v1/workspaces/{}/execute", handle.workspace_id))?,
        )
        .bearer_auth(&handle.capability)
        .header("x-factory-operation-grant", operation)
        .json(&request)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let _ = cleanup(supervisor, &handle.workspace_id, &handle.capability).await;
        return Err(format!("Supervisor execution failed: {status} {body}").into());
    }
    let result: ExecutionResult = response.json().await?;
    let cleanup_result = cleanup(supervisor, &handle.workspace_id, &handle.capability).await;
    let Some(candidate) = result.candidate else {
        let stderr = result
            .stderr
            .lines()
            .take(5)
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(format!("sandbox exited with {:?}: {stderr}", result.exit_code).into());
    };
    if let Err(error) = cleanup_result {
        eprintln!("Supervisor workspace cleanup failed: {error}");
    }
    Ok(candidate)
}

async fn integrate_assignment(
    assignment: &Assignment,
    supervisor: &Supervisor,
) -> Result<factory_native::workspace::CandidateRevision, Box<dyn std::error::Error>> {
    if assignment.integration_candidates.len() < 2 {
        return Err("integration assignment has fewer than two candidates".into());
    }
    let response = supervisor
        .client
        .post(supervisor.base.join("v1/integrate")?)
        .bearer_auth(&supervisor.token)
        .json(&json!({
            "repository": assignment.repository,
            "baseCommit": assignment.action_spec.base_commit,
            "candidateCommits": assignment.integration_candidates
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Supervisor integration failed: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    let value: serde_json::Value = response.json().await?;
    let commit_sha = value
        .get("commit_sha")
        .and_then(serde_json::Value::as_str)
        .ok_or("integration response has no commit")?;
    let tree_sha = value
        .get("tree_sha")
        .and_then(serde_json::Value::as_str)
        .ok_or("integration response has no tree")?;
    Ok(factory_native::workspace::CandidateRevision {
        commit_sha: commit_sha.into(),
        tree_sha: tree_sha.into(),
    })
}

async fn cleanup(
    supervisor: &Supervisor,
    workspace_id: &str,
    capability: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = supervisor
        .client
        .delete(
            supervisor
                .base
                .join(&format!("v1/workspaces/{workspace_id}"))?,
        )
        .bearer_auth(capability)
        .send()
        .await?;
    if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    Err(format!("Supervisor cleanup returned {}", response.status()).into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validated_loopback_url(
    raw: &str,
    variable: &str,
) -> Result<reqwest::Url, Box<dyn std::error::Error>> {
    let url = reqwest::Url::parse(raw)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("{variable} must be a loopback HTTP origin with explicit port").into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::validated_loopback_url;

    #[test]
    fn rejects_userinfo_host_bypass() {
        assert!(validated_loopback_url("http://127.0.0.1:x@attacker.example", "TEST").is_err());
        assert!(validated_loopback_url("http://localhost:8765", "TEST").is_ok());
        assert!(validated_loopback_url("http://127.0.0.1:8765/path", "TEST").is_err());
    }
}
