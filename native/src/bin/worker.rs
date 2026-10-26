//! Trusted Worker: Core lease coordination and Supervisor-mediated execution.
//!
//! The Worker never mounts or executes a project directly. It asks the trusted
//! Supervisor for a durable per-Attempt workspace, sends a bounded command into
//! an allowlisted rootless Podman sandbox, and reports the exact candidate. No
//! provider credentials, Podman socket or host workspace path reach the
//! sandbox.

use std::{env, time::Duration};

use factory_native::db::{ActionSpec, ProviderTerminal, ProviderTurnAuthorization};
use factory_native::execution::{
    ApplyFilesRequest, ExecuteWorkspace, ExecutionResult, ToolRunRequest, WorkspaceAllocation,
    WorkspaceHandle, WorkspaceStatus, WriteFilesRequest,
};
use factory_native::protocol::{
    DynamicToolCallParams, DynamicToolCallResult, DynamicToolContentItem, DynamicToolSpec, JsonRpc,
    RpcEvent, ThreadStartParams, ThreadTokenUsageUpdatedParams, TurnSandboxPolicy, TurnStartParams,
    UserInput,
};
use factory_native::provider_egress::{SharedTurnPolicy, TurnPolicy};
use factory_native::provider_patch::{
    ProviderAction, ProviderUsage, extract_action, extract_action_text, extract_plan, extract_usage,
};
use factory_native::provider_sandbox::{ProviderSandbox, ProviderSandboxConfig, SandboxTransport};
use serde::Deserialize;
use serde_json::{Value, json};
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
            let mut ticker = tokio::time::interval(Duration::from_secs(15));
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

enum ProviderRuntime {
    Http {
        base_url: String,
        password: String,
        sandbox: Option<ProviderSandbox>,
        turn_policy: Option<SharedTurnPolicy>,
    },
    Codex {
        sandbox: ProviderSandbox,
        rpc: JsonRpc<tokio::process::ChildStdout, tokio::process::ChildStdin>,
        turn_policy: Option<SharedTurnPolicy>,
    },
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
    api_directory: &'a str,
    provider_session: &'a std::sync::Arc<std::sync::Mutex<Option<String>>>,
    usage_tracker: &'a std::sync::Arc<std::sync::Mutex<ProviderUsage>>,
    turn_policy: Option<&'a SharedTurnPolicy>,
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
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("factory-provider-sessions-{}", std::process::id()))
        });
    std::fs::create_dir_all(&provider_root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&provider_root, std::fs::Permissions::from_mode(0o700))?;
    }
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

    let turn_policy: SharedTurnPolicy = std::sync::Arc::new(std::sync::Mutex::new(None));
    let provider_session: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let usage_tracker: std::sync::Arc<std::sync::Mutex<ProviderUsage>> =
        std::sync::Arc::new(std::sync::Mutex::new(ProviderUsage::default()));
    let provider_kind = env::var("FACTORY_PROVIDER_KIND").unwrap_or_default();
    let codex = provider_kind == "codex";
    let turn_binder = if provider_kind == "opencode" || codex {
        Some(factory_native::provider_egress::TurnBinder::new(
            client.clone(),
            core.clone(),
            session_token.to_owned(),
            session_record_id.clone(),
            worker_id.to_owned(),
            assignment.lease.generation,
        )?)
    } else {
        None
    };
    let runtime = match start_provider_runtime(
        &provider_dir,
        assignment.deadline_secs,
        &assignment.provider,
        &assignment.model,
        assignment.max_turns,
        turn_binder,
        &turn_policy,
    )
    .await
    {
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
    let confined = matches!(
        &runtime,
        ProviderRuntime::Http {
            sandbox: Some(_),
            ..
        } | ProviderRuntime::Codex { .. }
    );
    let result = match runtime {
        ProviderRuntime::Http {
            base_url,
            password,
            sandbox,
            turn_policy: egress_turn_policy,
        } => {
            let api_directory = if sandbox.is_some() {
                "/session"
            } else {
                directory.as_str()
            };
            let context = ProviderContext {
                client,
                core,
                session_token,
                worker_id,
                session_record_id: &session_record_id,
                base_url: base_url.as_str(),
                password: &password,
                directory: &directory,
                api_directory,
                provider_session: &provider_session,
                usage_tracker: &usage_tracker,
                turn_policy: egress_turn_policy.as_ref(),
                assignment,
                supervisor,
            };
            let result = if assignment.allowed_tools.is_empty() {
                run_provider_patch(&context).await
            } else {
                run_provider_tool_loop(&context).await
            };
            if let Some(sandbox) = sandbox {
                let _ = sandbox.stop().await;
            }
            result
        }
        ProviderRuntime::Codex {
            sandbox,
            mut rpc,
            turn_policy: egress_turn_policy,
        } => {
            let context = ProviderContext {
                client,
                core,
                session_token,
                worker_id,
                session_record_id: &session_record_id,
                base_url: "",
                password: "",
                directory: &directory,
                api_directory: "/session",
                provider_session: &provider_session,
                usage_tracker: &usage_tracker,
                turn_policy: egress_turn_policy.as_ref(),
                assignment,
                supervisor,
            };
            let result = run_codex_tool_loop(&context, &mut rpc).await;
            let _ = sandbox.stop().await;
            result
        }
    };
    let provider_session_id = provider_session
        .lock()
        .ok()
        .and_then(|session| session.clone())
        .unwrap_or_default();
    let cleanup_receipt = if provider_session_id.is_empty() {
        "no_external_session".to_owned()
    } else if confined {
        factory_native::provider_sandbox::confined_recovery_receipt(
            &directory,
            &provider_session_id,
            provider_root.as_path(),
        )
        .unwrap_or("sandbox_stopped_cleanup_unconfirmed")
        .to_owned()
    } else {
        "unconfined_cleanup_unconfirmed".to_owned()
    };
    let keep_session = cfg!(debug_assertions)
        && env::var("FACTORY_PROVIDER_KEEP_SESSION").ok().as_deref() == Some("1");
    let directory_removed = if keep_session {
        true
    } else {
        std::fs::remove_dir_all(&provider_dir).is_ok()
    };
    let cleanup_confirmed = !cleanup_receipt.is_empty()
        && !cleanup_receipt.contains("unconfirmed")
        && directory_removed;
    match result {
        Ok(run) => {
            if !run.usage.known || !cleanup_confirmed {
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
                    cleanup_receipt: cleanup_receipt.clone(),
                    error: if !cleanup_confirmed {
                        "provider cleanup was not confirmed".into()
                    } else {
                        "provider usage was not measurable".into()
                    },
                };
                let _ = post_provider_terminal(client, core, session_token, &terminal).await;
                return Err(if !cleanup_confirmed {
                    "provider cleanup was not confirmed; refusing an unaccounted candidate".into()
                } else {
                    "provider usage was not measurable; refusing an unaccounted candidate".into()
                });
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
                cleanup_receipt: if codex {
                    cleanup_receipt.clone()
                } else {
                    "session_delete_verified_absent".into()
                },
                error: String::new(),
            };
            post_provider_terminal(client, core, session_token, &terminal).await?;
            Ok(run.candidate)
        }
        Err(error) => {
            let tracked_usage = usage_tracker.lock().map(|usage| *usage).unwrap_or_default();
            let terminal = ProviderTerminal {
                session_record_id: session_record_id.clone(),
                worker_id: worker_id.into(),
                generation: assignment.lease.generation,
                state: "FAILED".into(),
                provider_session_id: provider_session_id.clone(),
                turn_id: String::new(),
                input_tokens: tracked_usage.input_tokens,
                output_tokens: tracked_usage.output_tokens,
                total_tokens: tracked_usage.total_tokens,
                cost_micros: tracked_usage.cost_micros,
                usage_known: tracked_usage.known,
                cleanup_receipt: cleanup_receipt.clone(),
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
    provider: &str,
    model: &str,
    max_turns: u32,
    turn_binder: Option<factory_native::provider_egress::TurnBinder>,
    turn_policy: &SharedTurnPolicy,
) -> Result<ProviderRuntime, Box<dyn std::error::Error>> {
    let mode = env::var("FACTORY_PROVIDER_SANDBOX").unwrap_or_default();
    match mode.as_str() {
        "bwrap" => {
            let binary = env::var("FACTORY_PROVIDER_BINARY")
                .map_err(|_| "FACTORY_PROVIDER_BINARY required for sandboxed provider")?;
            let kind = env::var("FACTORY_PROVIDER_KIND").unwrap_or_else(|_| "fixture".into());
            if !matches!(kind.as_str(), "fixture" | "opencode" | "codex") {
                return Err(format!("unsupported FACTORY_PROVIDER_KIND {kind:?}").into());
            }
            let opencode = kind == "opencode";
            let codex = kind == "codex";
            if codex && provider != "codex" {
                return Err("Codex transport cannot serve a non-Codex provider assignment".into());
            }
            if opencode && !matches!(provider, "opencode" | "opencode-go") {
                return Err(
                    "OpenCode transport cannot serve a non-OpenCode provider assignment".into(),
                );
            }
            if kind == "fixture" && !matches!(provider, "fixture" | "opencode" | "opencode-go") {
                return Err("fixture transport cannot serve this provider assignment".into());
            }
            let tcp_port = env::var("FACTORY_PROVIDER_TCP_PORT")
                .unwrap_or_else(|_| "18081".into())
                .parse::<u16>()
                .map_err(|_| "FACTORY_PROVIDER_TCP_PORT must be a port number")?;
            if tcp_port == 0 || tcp_port == 18080 {
                return Err(
                    "FACTORY_PROVIDER_TCP_PORT must be a non-zero port other than 18080".into(),
                );
            }
            let args: Vec<String> = if codex {
                vec![
                    "app-server".into(),
                    "--listen".into(),
                    "stdio://".into(),
                    "-c".into(),
                    "model_provider=factory".into(),
                    "-c".into(),
                    "model_providers.factory={name=\"Factory\",base_url=\"http://127.0.0.1:18080/v1\",env_key=\"OPENAI_API_KEY\",wire_api=\"responses\"}".into(),
                    "-c".into(),
                    "approval_policy=never".into(),
                    "-c".into(),
                    "sandbox_mode=read-only".into(),
                    "-c".into(),
                    format!("model={model}"),
                    "-c".into(),
                    "features.shell_tool=false".into(),
                    "-c".into(),
                    "features.unified_exec=false".into(),
                    "-c".into(),
                    "features.unified_exec_tty=false".into(),
                    "-c".into(),
                    "features.view_image=false".into(),
                    "-c".into(),
                    "features.browser_use=false".into(),
                    "-c".into(),
                    "features.in_app_browser=false".into(),
                    "-c".into(),
                    "features.multi_agent=false".into(),
                    "-c".into(),
                    "features.multi_agent_v2=false".into(),
                    "-c".into(),
                    "features.code_mode=false".into(),
                    "-c".into(),
                    "features.code_mode_host=false".into(),
                    "-c".into(),
                    "features.code_mode_only=false".into(),
                    "-c".into(),
                    "features.code_mode_interrupt=false".into(),
                    "-c".into(),
                    "features.code_mode_prewarm=false".into(),
                    "-c".into(),
                    "features.code_mode_buffered_exec=false".into(),
                    "-c".into(),
                    "features.sleep_tool=false".into(),
                    "-c".into(),
                    "features.request_permissions_tool=false".into(),
                    "-c".into(),
                    "features.tool_suggest=false".into(),
                    "-c".into(),
                    "features.goals=false".into(),
                    "-c".into(),
                    "features.default_mode_request_user_input=false".into(),
                    "-c".into(),
                    "features.standalone_web_search=false".into(),
                    "-c".into(),
                    "features.web_search_request=false".into(),
                    "-c".into(),
                    "features.apps=false".into(),
                    "-c".into(),
                    "features.plugins=false".into(),
                    "-c".into(),
                    "web_search=\"disabled\"".into(),
                ]
            } else {
                match env::var("FACTORY_PROVIDER_ARGS") {
                    Ok(value) => serde_json::from_str(&value)
                        .map_err(|_| "FACTORY_PROVIDER_ARGS must be a JSON string array")?,
                    Err(_) if opencode => vec![
                        "serve".into(),
                        "--hostname".into(),
                        "127.0.0.1".into(),
                        "--port".into(),
                        tcp_port.to_string(),
                        "--pure".into(),
                    ],
                    Err(_) => Vec::new(),
                }
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
                    let max_requests = if opencode {
                        max_turns as usize + 4
                    } else if codex {
                        (max_turns as usize).saturating_mul(8).saturating_add(1)
                    } else {
                        max_turns as usize + 1
                    };
                    let mut config = factory_native::provider_egress::EgressConfig::new(
                        &upstream,
                        credential,
                        token,
                        allow_test_upstream,
                    )?
                    .with_model(model.to_owned())
                    .with_max_requests(max_requests)
                    .with_turn_policy(turn_policy.clone());
                    if (opencode || codex)
                        && let Some(binder) = turn_binder
                    {
                        config = config.with_turn_binder(binder);
                    }
                    (Some(config), Some(std::path::PathBuf::from(bridge)))
                }
                Err(_) if opencode || codex => {
                    return Err(format!(
                        "{kind} provider requires FACTORY_PROVIDER_UPSTREAM_URL and egress"
                    )
                    .into());
                }
                Err(_) => (None, None),
            };
            let mut sandbox_env = Vec::new();
            let mut provider_bridge = None;
            let mut provider_tcp = None;
            if opencode {
                let egress_config = egress
                    .as_ref()
                    .ok_or("opencode provider requires provider egress")?;
                let bridge = match env::var("FACTORY_PROVIDER_BRIDGE") {
                    Ok(value) => std::path::PathBuf::from(value),
                    Err(_) => std::env::current_exe()?
                        .parent()
                        .map(|parent| parent.join("factory-provider-bridge"))
                        .ok_or("cannot locate factory-provider-bridge")?,
                };
                if !bridge.is_absolute() || !bridge.is_file() {
                    return Err(format!(
                        "factory-provider-bridge is not an absolute file: {}",
                        bridge.display()
                    )
                    .into());
                }
                let context_limit = env::var("FACTORY_PROVIDER_MODEL_CONTEXT")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|value| *value > 0)
                    .unwrap_or(128_000);
                let output_limit = env::var("FACTORY_PROVIDER_MODEL_OUTPUT")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|value| *value > 0)
                    .unwrap_or(8_192);
                let config = opencode_sandbox_config(
                    provider,
                    model,
                    egress_config.token(),
                    context_limit,
                    output_limit,
                );
                sandbox_env.push((
                    "OPENCODE_CONFIG_CONTENT".into(),
                    serde_json::to_string(&config)?,
                ));
                provider_bridge = Some(bridge);
                provider_tcp = Some(format!("127.0.0.1:{tcp_port}"));
            }
            let mut transport = SandboxTransport::HttpSocket;
            if codex {
                transport = SandboxTransport::Stdio;
                let codex_home = session_dir.join(".codex");
                std::fs::create_dir_all(&codex_home)?;
                let git_init = std::process::Command::new("git")
                    .args(["init", "--quiet"])
                    .arg(session_dir)
                    .status()?;
                if !git_init.success() {
                    return Err("failed to initialize the Codex session repository".into());
                }
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(session_dir)
                    .args(["config", "user.name", "Software Factory"])
                    .status()?;
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(session_dir)
                    .args(["config", "user.email", "factory@localhost.invalid"])
                    .status()?;
                let empty_commit = std::process::Command::new("git")
                    .arg("-C")
                    .arg(session_dir)
                    .args([
                        "commit",
                        "--allow-empty",
                        "--quiet",
                        "-m",
                        "factory provider session",
                    ])
                    .status()?;
                if !empty_commit.success() {
                    return Err("failed to initialize the Codex session commit".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&codex_home, std::fs::Permissions::from_mode(0o700))?;
                }
                sandbox_env.push(("CODEX_HOME".into(), "/session/.codex".into()));
            }
            if env::var("FACTORY_FIXTURE_PROVIDER_USE_EGRESS")
                .ok()
                .as_deref()
                == Some("1")
            {
                sandbox_env.push(("FACTORY_FIXTURE_PROVIDER_USE_EGRESS".into(), "1".into()));
                sandbox_env.push(("FACTORY_FIXTURE_PROVIDER_MODEL".into(), model.to_owned()));
            }
            let mut sandbox = ProviderSandbox::start(ProviderSandboxConfig {
                binary: std::path::PathBuf::from(binary),
                session_dir: session_dir.to_path_buf(),
                args,
                ready_timeout: Duration::from_secs(30),
                max_session_bytes,
                max_lifetime: lifetime,
                bridge_timeout: lifetime,
                address_space_bytes: env::var("FACTORY_PROVIDER_MEMORY_LIMIT_BYTES")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(if opencode {
                        16 * 1024 * 1024 * 1024
                    } else {
                        4 * 1024 * 1024 * 1024
                    }),
                egress,
                egress_bridge,
                provider_bridge,
                provider_tcp,
                env: sandbox_env,
                transport,
                ..Default::default()
            })
            .await?;
            if opencode {
                wait_for_opencode_health(
                    sandbox.base_url(),
                    sandbox.password(),
                    Duration::from_secs(30),
                )
                .await?;
            }
            if codex {
                let (stdin, stdout) = sandbox.take_stdio()?;
                let rpc = JsonRpc::new(stdout, stdin);
                return Ok(ProviderRuntime::Codex {
                    sandbox,
                    rpc,
                    turn_policy: Some(turn_policy.clone()),
                });
            }
            Ok(ProviderRuntime::Http {
                base_url: sandbox.base_url().to_owned(),
                password: sandbox.password().to_owned(),
                sandbox: Some(sandbox),
                turn_policy: Some(turn_policy.clone()),
            })
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
            Ok(ProviderRuntime::Http {
                base_url: base_url.to_string(),
                password,
                sandbox: None,
                turn_policy: None,
            })
        }
        _ => Err("provider sandbox is not configured; refusing an unconfined provider".into()),
    }
}

fn opencode_disabled_tools() -> serde_json::Value {
    json!({
        "bash": false,
        "edit": false,
        "write": false,
        "patch": false,
        "multiedit": false,
        "read": false,
        "grep": false,
        "glob": false,
        "list": false,
        "lsp": false,
        "question": false,
        "skill": false,
        "task": false,
        "todowrite": false,
        "todoread": false,
        "webfetch": false
    })
}

fn opencode_sandbox_config(
    provider: &str,
    model: &str,
    egress_token: &str,
    context_limit: u64,
    output_limit: u64,
) -> serde_json::Value {
    json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            provider: {
                "npm": "@ai-sdk/openai-compatible",
                "name": format!("Factory {provider}"),
                "options": {
                    "baseURL": "http://127.0.0.1:18080/v1",
                    "apiKey": egress_token
                },
                "models": {
                    model: {
                        "name": model,
                        "limit": {"context": context_limit, "output": output_limit}
                    }
                }
            }
        },
        "model": format!("{provider}/{model}"),
        "autoupdate": false,
        "share": "disabled",
        "instructions": [],
        "agent": {"plan": {"tools": opencode_disabled_tools()}}
    })
}

async fn wait_for_opencode_health(
    base_url: &str,
    password: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let http = factory_native::runtime::opencode_http_client(Duration::from_secs(10))?;
        let response = http
            .get(format!("{base_url}/global/health"))
            .basic_auth("opencode", Some(password))
            .send()
            .await;
        if let Ok(response) = response
            && response.status().is_success()
        {
            let value: serde_json::Value = response.json().await?;
            if value.get("healthy").and_then(serde_json::Value::as_bool) == Some(true) {
                return Ok(());
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("OpenCode provider did not become healthy inside the sandbox".into());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn finalize_written_provider_work(
    context: &ProviderContext<'_>,
    http: &reqwest::Client,
    handle: &WorkspaceHandle,
    session_id: &str,
    last_message_id: &str,
    usage: ProviderUsage,
    error: &str,
) -> Result<Option<ProviderRun>, Box<dyn std::error::Error>> {
    if provider_state(context).await? == "CANCEL_REQUESTED" {
        return Ok(None);
    }
    post_tool_event(
        context,
        "provider-turn-error",
        "final",
        "RESULT",
        &format!(
            "provider turn ended with an error after at least one applied write; preserving the applied writes for independent verification: {error}"
        ),
    )
    .await?;
    factory_native::runtime::delete_opencode_session(
        http,
        context.base_url,
        context.password,
        context.api_directory,
        session_id,
    )
    .await?;
    let candidate = supervisor_finalize(context, &handle.workspace_id, &handle.capability).await?;
    cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
    Ok(Some(ProviderRun {
        candidate,
        provider_session_id: session_id.to_owned(),
        turn_id: last_message_id.to_owned(),
        usage,
    }))
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
        context.api_directory,
    )
    .await?;
    let mut event_stream = event_response.bytes_stream();
    let mut parser = factory_native::sse::SseParser::new(1_048_576);
    let created = factory_native::runtime::create_opencode_session(
        &http,
        context.base_url,
        context.password,
        context.api_directory,
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
    if let Ok(mut provider_session) = context.provider_session.lock() {
        *provider_session = Some(session_id.clone());
    }
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
        "agent": "plan",
        "tools": opencode_disabled_tools(),
        "parts": [{"type": "text", "text": format!(
            "You are a bounded engineering agent. You MUST change the authorized source: an empty final response is invalid. You may request only these tools: {}. Return ONE JSON object per turn. To change a file use {{\"type\":\"tool_request\",\"call_id\":\"unique\",\"tool\":\"write_file\",\"path\":\"relative/path\",\"content\":\"...full file...\"}}. To run a frozen check use {{\"type\":\"tool_request\",\"call_id\":\"unique\",\"tool\":\"run_command\",\"argv\":[\"program\",\"arg\"],\"timeout_secs\":30}}. To finish use {{\"type\":\"final\",\"files\":[],\"explanation\":\"what changed and why\"}}. The `files` array in a final response MUST be empty; never put paths or strings in it. Allowed paths: {}. Objective: {}. Frozen checks you may run with run_command (argv must match exactly): {}. Source context: {}",
            context.assignment.allowed_tools.join(", "),
            context.assignment.action_spec.allowed_paths.join(", "),
            context.assignment.action_spec.objective,
            serde_json::to_string(
                &context
                    .assignment
                    .action_spec
                    .checks
                    .iter()
                    .map(|check| json!({"id": check.id, "command": check.command, "expectExit": check.expect_exit}))
                    .collect::<Vec<_>>()
            )?,
            serde_json::to_string(&source_context)?
        )}]
    });
    let mut usage = ProviderUsage::default();
    let mut written = false;
    let mut last_message_id = String::new();
    let mut call_ids = std::collections::BTreeSet::new();
    for _ in 0..context.assignment.max_turns {
        let turn = if context.turn_policy.is_some() {
            Some(authorize_provider_turn(context, &prompt).await?)
        } else {
            None
        };
        if let Err(error) = factory_native::runtime::submit_opencode_prompt(
            &http,
            context.base_url,
            context.password,
            context.api_directory,
            &session_id,
            &prompt,
        )
        .await
        {
            if let Some(turn) = &turn {
                release_provider_turn(context, &turn.reservation_id).await;
            }
            if written
                && let Some(run) = finalize_written_provider_work(
                    context,
                    &http,
                    &handle,
                    &session_id,
                    &last_message_id,
                    usage,
                    &error.to_string(),
                )
                .await?
            {
                return Ok(run);
            }
            return Err(error.into());
        }
        let terminal = match tokio::time::timeout(
            Duration::from_secs(context.assignment.deadline_secs),
            factory_native::runtime::wait_for_opencode_prompt_terminal(
                &mut event_stream,
                &mut parser,
                &session_id,
                false,
            ),
        )
        .await
        {
            Ok(Ok(terminal)) => terminal,
            Ok(Err(error)) => {
                if let Some(turn) = &turn {
                    release_provider_turn(context, &turn.reservation_id).await;
                }
                if written
                    && let Some(run) = finalize_written_provider_work(
                        context,
                        &http,
                        &handle,
                        &session_id,
                        &last_message_id,
                        usage,
                        &error.to_string(),
                    )
                    .await?
                {
                    return Ok(run);
                }
                return Err(error.into());
            }
            Err(_) => {
                if let Some(turn) = &turn {
                    release_provider_turn(context, &turn.reservation_id).await;
                }
                if written
                    && let Some(run) = finalize_written_provider_work(
                        context,
                        &http,
                        &handle,
                        &session_id,
                        &last_message_id,
                        usage,
                        "provider deadline exceeded",
                    )
                    .await?
                {
                    return Ok(run);
                }
                return Err("provider deadline exceeded".into());
            }
        };
        let message = match &terminal {
            factory_native::runtime::OpenCodePromptTerminal::Idle {
                cancel_requested: false,
                ..
            } => match factory_native::runtime::read_opencode_assistant_message(
                &http,
                context.base_url,
                context.password,
                context.api_directory,
                &session_id,
            )
            .await
            {
                Ok(message) => message,
                Err(error) => {
                    if let Some(turn) = &turn {
                        release_provider_turn(context, &turn.reservation_id).await;
                    }
                    if written
                        && let Some(run) = finalize_written_provider_work(
                            context,
                            &http,
                            &handle,
                            &session_id,
                            &last_message_id,
                            usage,
                            &error.to_string(),
                        )
                        .await?
                    {
                        return Ok(run);
                    }
                    return Err(error.into());
                }
            },
            _ => {
                if let Some(turn) = &turn {
                    release_provider_turn(context, &turn.reservation_id).await;
                }
                let detail = match &terminal {
                    factory_native::runtime::OpenCodePromptTerminal::Failed { error, .. } => {
                        error.to_string()
                    }
                    factory_native::runtime::OpenCodePromptTerminal::Idle {
                        cancel_requested: true,
                        ..
                    } => "provider cancellation was requested".into(),
                    _ => "provider turn did not complete normally".into(),
                };
                if written
                    && let Some(run) = finalize_written_provider_work(
                        context,
                        &http,
                        &handle,
                        &session_id,
                        &last_message_id,
                        usage,
                        &detail,
                    )
                    .await?
                {
                    return Ok(run);
                }
                return Err(detail.into());
            }
        };
        let outcome =
            factory_native::runtime::reconcile_opencode_prompt(&session_id, terminal, &message)?;
        let message_id = match outcome {
            factory_native::runtime::OpenCodePromptOutcome::Completed { message_id, .. } => {
                message_id
            }
            _ => {
                if let Some(turn) = &turn {
                    release_provider_turn(context, &turn.reservation_id).await;
                }
                return Err("provider turn failed".into());
            }
        };
        last_message_id = message_id.clone();
        let turn_usage = extract_usage(&message);
        if let Some(turn) = &turn {
            settle_provider_turn(context, &turn.reservation_id, &turn_usage).await?;
        }
        usage.input_tokens = usage.input_tokens.saturating_add(turn_usage.input_tokens);
        usage.output_tokens = usage.output_tokens.saturating_add(turn_usage.output_tokens);
        usage.total_tokens = usage.total_tokens.saturating_add(turn_usage.total_tokens);
        usage.cost_micros = usage.cost_micros.saturating_add(turn_usage.cost_micros);
        usage.known = usage.known || turn_usage.known;
        if let Ok(mut tracker) = context.usage_tracker.lock() {
            *tracker = usage;
        }
        let action = match extract_action(&message) {
            Ok(action) => action,
            Err(error) => {
                post_tool_event(
                    context,
                    "invalid-action",
                    "final",
                    "ERROR",
                    &format!("invalid provider action: {error}"),
                )
                .await?;
                prompt = json!({
                    "model": {"providerID": context.assignment.provider, "modelID": context.assignment.model},
                    "parts": [{"type": "text", "text": format!(
                        "Your previous response was rejected: {error}. Return exactly one JSON object. Use a write_file tool_request for every file change. Finish with {{\"type\":\"final\",\"files\":[],\"explanation\":\"...\"}} and keep `files` empty."
                    )}]
                });
                continue;
            }
        };
        match action {
            ProviderAction::Final { files, explanation } => {
                if !written && files.is_empty() {
                    post_tool_event(
                        context,
                        "empty-final",
                        "final",
                        "ERROR",
                        "no file has been written; an empty final response cannot create a candidate",
                    )
                    .await?;
                    prompt = json!({
                        "model": {"providerID": context.assignment.provider, "modelID": context.assignment.model},
                        "parts": [{"type": "text", "text": "You have not written any file, so no candidate can be created. Use a write_file tool_request now with the complete corrected file content. Do not return a final response until at least one file has been written."}]
                    });
                    continue;
                }
                if !explanation.trim().is_empty() {
                    let bounded: String = explanation.chars().take(4_000).collect();
                    post_tool_event(context, "final-explanation", "final", "RESULT", &bounded)
                        .await?;
                }
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
                    context.api_directory,
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
                        context.api_directory,
                        &session_id,
                    )
                    .await;
                    let _ = factory_native::runtime::delete_opencode_session(
                        &http,
                        context.base_url,
                        context.password,
                        context.api_directory,
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
                                prompt = tool_result(
                                    context.assignment.provider.as_str(),
                                    context.assignment.model.as_str(),
                                    &call_id,
                                    "ok",
                                    "write applied",
                                    Some(0),
                                );
                            }
                            Err(error) => {
                                post_tool_event(context, &call_id, &tool, "ERROR", "write failed")
                                    .await?;
                                prompt = tool_result(
                                    context.assignment.provider.as_str(),
                                    context.assignment.model.as_str(),
                                    &call_id,
                                    "failed",
                                    &error.to_string(),
                                    None,
                                );
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
                                prompt = tool_result(
                                    context.assignment.provider.as_str(),
                                    context.assignment.model.as_str(),
                                    &call_id,
                                    status,
                                    &result.stderr,
                                    result.exit_code,
                                );
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
                                prompt = tool_result(
                                    context.assignment.provider.as_str(),
                                    context.assignment.model.as_str(),
                                    &call_id,
                                    "failed",
                                    &error.to_string(),
                                    None,
                                );
                            }
                        }
                    }
                    Err(reason) => {
                        post_tool_event(context, &call_id, &tool, "DENY", &reason).await?;
                        prompt = tool_result(
                            context.assignment.provider.as_str(),
                            context.assignment.model.as_str(),
                            &call_id,
                            "denied",
                            &reason,
                            None,
                        );
                    }
                }
            }
        }
    }
    if written {
        post_tool_event(
            context,
            "turn-budget-exhausted",
            "final",
            "RESULT",
            "provider wrote one or more files but did not emit a final response; finalizing the applied writes for independent verification",
        )
        .await?;
        factory_native::runtime::delete_opencode_session(
            &http,
            context.base_url,
            context.password,
            context.api_directory,
            &session_id,
        )
        .await?;
        let candidate =
            supervisor_finalize(context, &handle.workspace_id, &handle.capability).await?;
        cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
        return Ok(ProviderRun {
            candidate,
            provider_session_id: session_id,
            turn_id: last_message_id,
            usage,
        });
    }
    let _ = factory_native::runtime::delete_opencode_session(
        &http,
        context.base_url,
        context.password,
        context.api_directory,
        &session_id,
    )
    .await;
    let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
    Err("provider exceeded the authorized turn budget without writing a file".into())
}

fn codex_dynamic_tools(context: &ProviderContext<'_>) -> Vec<DynamicToolSpec> {
    let mut tools = Vec::new();
    if context
        .assignment
        .allowed_tools
        .iter()
        .any(|tool| tool == "write_file")
    {
        tools.push(DynamicToolSpec::Function {
            name: "write_file".into(),
            description:
                "Write the complete content of one authorized repository file. The Worker applies it through the Supervisor."
                    .into(),
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["path", "content"],
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                }
            }),
        });
    }
    if context
        .assignment
        .allowed_tools
        .iter()
        .any(|tool| tool == "run_command")
    {
        tools.push(DynamicToolSpec::Function {
            name: "run_command".into(),
            description:
                "Run one exact frozen acceptance check command. The Worker runs it through the Supervisor."
                    .into(),
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["argv"],
                "properties": {
                    "argv": {"type": "array", "minItems": 1, "items": {"type": "string"}},
                    "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 300}
                }
            }),
        });
    }
    tools
}

fn codex_prompt(context: &ProviderContext<'_>) -> Result<String, Box<dyn std::error::Error>> {
    let source_context = load_source_context(
        &context.assignment.repository,
        &context.assignment.action_spec.base_commit,
        &context.assignment.action_spec.allowed_paths,
    )?;
    let tools = if context.assignment.allowed_tools.is_empty() {
        "no dynamic tools; return a JSON patch".to_string()
    } else {
        context.assignment.allowed_tools.join(", ")
    };
    Ok(format!(
        "You are a bounded engineering agent. Use only the client-provided dynamic tools: {}. Never use shell, patch, MCP, browser, or any native file tool. To change a file call write_file with the complete file content. To run an allowed acceptance check call run_command with argv that exactly matches one frozen check. When finished, return exactly one JSON object with type final, an empty files array, and a concise explanation: {{\"type\":\"final\",\"files\":[],\"explanation\":\"what changed and why\"}}. Do not add prose outside that JSON. Allowed paths: {}. Objective: {}. Frozen checks: {}. Source context: {}",
        tools,
        context.assignment.action_spec.allowed_paths.join(", "),
        context.assignment.action_spec.objective,
        serde_json::to_string(
            &context
                .assignment
                .action_spec
                .checks
                .iter()
                .map(|check| json!({"id": check.id, "command": check.command, "expectExit": check.expect_exit}))
                .collect::<Vec<_>>()
        )?,
        serde_json::to_string(&source_context)?
    ))
}

fn codex_usage(total: &factory_native::protocol::TokenUsageBreakdown) -> ProviderUsage {
    let input = total.input_tokens;
    let output = total.output_tokens;
    let total_tokens = total.total_tokens.max(input.saturating_add(output));
    let input_rate = env::var("FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);
    let output_rate = env::var("FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0);
    let (cost_micros, known) = match (input_rate, output_rate) {
        (Some(input_rate), Some(output_rate)) => (
            input
                .saturating_mul(input_rate)
                .saturating_div(1_000_000)
                .saturating_add(output.saturating_mul(output_rate).saturating_div(1_000_000)),
            true,
        ),
        _ => (0, false),
    };
    ProviderUsage {
        input_tokens: input,
        output_tokens: output,
        total_tokens,
        cost_micros,
        known,
    }
}

fn bounded_tool_text(value: &str) -> String {
    value.chars().take(8_000).collect()
}

struct CodexToolState {
    call_ids: std::collections::BTreeSet<String>,
    tool_calls: u32,
    written: bool,
}

async fn execute_codex_tool(
    context: &ProviderContext<'_>,
    handle: &WorkspaceHandle,
    call_id: &str,
    tool: &str,
    arguments: &Value,
    tool_state: &mut CodexToolState,
) -> Result<(bool, String), Box<dyn std::error::Error>> {
    if call_id.trim().is_empty() || !tool_state.call_ids.insert(call_id.to_owned()) {
        return Ok((false, "duplicate or empty dynamic tool call id".into()));
    }
    tool_state.tool_calls = tool_state.tool_calls.saturating_add(1);
    if tool_state.tool_calls > 8 {
        return Ok((false, "dynamic tool call budget exhausted".into()));
    }
    let arguments = match arguments {
        Value::Object(_) => arguments.clone(),
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(value @ Value::Object(_)) => value,
            _ => return Ok((false, "dynamic tool arguments must be a JSON object".into())),
        },
        _ => return Ok((false, "dynamic tool arguments must be a JSON object".into())),
    };
    if arguments.as_object().is_some_and(|object| {
        object
            .keys()
            .any(|key| !matches!(key.as_str(), "path" | "content" | "argv" | "timeout_secs"))
    }) {
        return Ok((
            false,
            "dynamic tool arguments contain unknown fields".into(),
        ));
    }
    let path = arguments
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let content = arguments
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let argv = arguments
        .get("argv")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if arguments.get("argv").is_some()
        && argv.len() != arguments["argv"].as_array().map_or(0, Vec::len)
    {
        return Ok((false, "dynamic tool argv must contain only strings".into()));
    }
    let timeout_secs = arguments
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(60);
    post_tool_event(context, call_id, tool, "REQUEST", "dynamic tool requested").await?;
    let state = provider_state(context).await?;
    if state == "CANCEL_REQUESTED" {
        post_tool_event(
            context,
            call_id,
            tool,
            "DENY",
            "provider cancellation was requested",
        )
        .await?;
        return Ok((false, "provider cancellation was requested".into()));
    }
    let decision = decide_tool(context, tool, &path, &content, &argv, timeout_secs);
    match decision {
        Ok(ToolDecision::Write) => {
            post_tool_event(context, call_id, tool, "ALLOW", "path authorized").await?;
            let file = factory_native::execution::AppliedFile { path, content };
            match supervisor_write(
                context,
                &handle.workspace_id,
                &handle.capability,
                vec![file],
                !tool_state.written,
            )
            .await
            {
                Ok(()) => {
                    tool_state.written = true;
                    post_tool_event(context, call_id, tool, "RESULT", "write applied").await?;
                    Ok((true, "write applied".into()))
                }
                Err(error) => {
                    post_tool_event(context, call_id, tool, "ERROR", "write failed").await?;
                    Ok((false, bounded_tool_text(&format!("write failed: {error}"))))
                }
            }
        }
        Ok(ToolDecision::Run) => {
            post_tool_event(context, call_id, tool, "ALLOW", "frozen check authorized").await?;
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
                    post_tool_event(
                        context,
                        call_id,
                        tool,
                        "RESULT",
                        &format!("exit={:?}", result.exit_code),
                    )
                    .await?;
                    Ok((
                        true,
                        bounded_tool_text(&format!(
                            "exit={:?}\nstdout:\n{}\nstderr:\n{}",
                            result.exit_code, result.stdout, result.stderr
                        )),
                    ))
                }
                Err(error) => {
                    post_tool_event(context, call_id, tool, "ERROR", "command failed").await?;
                    Ok((
                        false,
                        bounded_tool_text(&format!("command failed: {error}")),
                    ))
                }
            }
        }
        Err(reason) => {
            post_tool_event(context, call_id, tool, "DENY", &reason).await?;
            Ok((false, bounded_tool_text(&reason)))
        }
    }
}

async fn run_codex_tool_loop(
    context: &ProviderContext<'_>,
    rpc: &mut JsonRpc<tokio::process::ChildStdout, tokio::process::ChildStdin>,
) -> Result<ProviderRun, Box<dyn std::error::Error>> {
    rpc.initialize("software_factory", "Software Factory", "0.1.0", true)
        .await?;
    if env::var("FACTORY_CODEX_REQUIRE_MODEL_LIST").ok().as_deref() != Some("0") {
        let models = rpc.request("model/list", json!({})).await?;
        let available = models
            .get("data")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("id").and_then(Value::as_str)
                        == Some(context.assignment.model.as_str())
                })
            });
        if !available {
            return Err(format!(
                "Codex model {:?} is not available in model/list",
                context.assignment.model
            )
            .into());
        }
    }
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
    let prompt_text = match codex_prompt(context) {
        Ok(prompt) => prompt,
        Err(error) => {
            let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
            return Err(error);
        }
    };
    let thread = match rpc
        .start_thread(ThreadStartParams {
            cwd: "/session".into(),
            approval_policy: factory_native::protocol::DenyApprovalPolicy::Never,
            sandbox: factory_native::protocol::ReadOnlySandbox::ReadOnly,
            model: Some(context.assignment.model.clone()),
            model_provider: Some("factory".into()),
            ephemeral: Some(true),
            dynamic_tools: Some(codex_dynamic_tools(context)),
            base_instructions: Some(
                "Use only the dynamic tools declared by the client. Do not use shell, patch, MCP, browser, or native file tools. Return the required final JSON object."
                    .into(),
            ),
            developer_instructions: None,
        })
        .await
    {
        Ok(thread) => thread,
        Err(error) => {
            let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
            return Err(error.into());
        }
    };
    let thread_id = thread.thread.id.clone();
    if let Some(model) = thread.model.as_deref()
        && model != context.assignment.model
    {
        return Err(format!(
            "Codex thread model mismatch: requested {:?}, received {model:?}",
            context.assignment.model
        )
        .into());
    }
    if let Some(provider) = thread.model_provider.as_deref()
        && provider != "factory"
    {
        return Err(format!("Codex thread provider mismatch: {provider:?}").into());
    }
    if let Ok(mut session) = context.provider_session.lock() {
        *session = Some(thread_id.clone());
    }
    let active: Value = context
        .client
        .post(context.core.join("worker/provider_active")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation,
            "provider_session_id": thread_id,
            "provider_directory": context.directory
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if active.get("state").and_then(Value::as_str) != Some("ACTIVE") {
        let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
        return Err("Core did not bind the Codex provider session as ACTIVE".into());
    }
    let mut prompt = prompt_text;
    let session_deadline =
        tokio::time::Instant::now() + Duration::from_secs(context.assignment.deadline_secs);
    let mut usage = ProviderUsage::default();
    let mut usage_before = factory_native::protocol::TokenUsageBreakdown {
        cached_input_tokens: 0,
        input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
    };
    let mut tool_state = CodexToolState {
        call_ids: std::collections::BTreeSet::new(),
        tool_calls: 0,
        written: false,
    };
    let mut last_turn_id = String::new();
    let result: Result<ProviderRun, Box<dyn std::error::Error>> = async {
        for _ in 0..context.assignment.max_turns {
            let turn = if context.turn_policy.is_some() {
                Some(authorize_provider_turn(context, &json!({"text": prompt})).await?)
            } else {
                None
            };
            if provider_state(context).await? == "CANCEL_REQUESTED" {
                if let Some(turn) = &turn {
                    release_provider_turn(context, &turn.reservation_id).await;
                }
                return Err("provider cancellation was requested".into());
            }
            let remaining = session_deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                if let Some(turn) = &turn {
                    release_provider_turn(context, &turn.reservation_id).await;
                }
                return Err("Codex provider deadline exceeded".into());
            }
            let started = match tokio::time::timeout(
                remaining,
                rpc.start_turn(TurnStartParams {
                    thread_id: thread_id.clone(),
                    input: vec![UserInput::Text { text: prompt.clone() }],
                    approval_policy: factory_native::protocol::DenyApprovalPolicy::Never,
                    sandbox_policy: TurnSandboxPolicy::ReadOnly { network_access: false },
                    model: Some(context.assignment.model.clone()),
                    effort: Some("medium".into()),
                    output_schema: None,
                }),
            )
            .await
            {
                Ok(Ok(started)) => started,
                Ok(Err(error)) => {
                    if let Some(turn) = &turn {
                        release_provider_turn(context, &turn.reservation_id).await;
                    }
                    return Err(error.into());
                }
                Err(_) => {
                    if let Some(turn) = &turn {
                        release_provider_turn(context, &turn.reservation_id).await;
                    }
                    return Err("Codex turn start deadline exceeded".into());
                }
            };
            let turn_id = started.turn.id.clone();
            last_turn_id = turn_id.clone();
            let mut final_text: Option<String> = None;
            let mut usage_after: Option<factory_native::protocol::TokenUsageBreakdown> = None;
            tool_state.tool_calls = 0;
            loop {
                let poll_at = (tokio::time::Instant::now() + Duration::from_secs(1))
                    .min(session_deadline);
                let event = match tokio::time::timeout_at(poll_at, rpc.next_event()).await {
                    Ok(Ok(event)) => event,
                    Ok(Err(error)) => {
                        if let Some(turn) = &turn {
                            release_provider_turn(context, &turn.reservation_id).await;
                        }
                        return Err(error.into());
                    }
                    Err(_) if tokio::time::Instant::now() >= session_deadline => {
                        let _ = rpc.interrupt_turn(&thread_id, &turn_id).await;
                        if let Some(turn) = &turn {
                            release_provider_turn(context, &turn.reservation_id).await;
                        }
                        return Err("Codex provider deadline exceeded".into());
                    }
                    Err(_) => {
                        if provider_state(context).await? == "CANCEL_REQUESTED" {
                            let _ = rpc.interrupt_turn(&thread_id, &turn_id).await;
                            if let Some(turn) = &turn {
                                release_provider_turn(context, &turn.reservation_id).await;
                            }
                            return Err("provider cancellation was requested".into());
                        }
                        continue;
                    }
                };
                match event {
                    RpcEvent::ServerRequest(request) => {
                        let factory_native::protocol::RpcServerRequest { id, method, params } =
                            request;
                        if method == "item/tool/call" {
                            let call: DynamicToolCallParams = serde_json::from_value(params)?;
                            if call.thread_id != thread_id
                                || call.turn_id != turn_id
                                || call.namespace.is_some()
                            {
                                rpc.respond_error(id, -32602, "dynamic tool identity mismatch")
                                    .await?;
                                return Err("Codex dynamic tool identity mismatch".into());
                            }
                            let (success, text) = execute_codex_tool(
                                context,
                                &handle,
                                &call.call_id,
                                &call.tool,
                                &call.arguments,
                                &mut tool_state,
                            )
                            .await?;
                            rpc.respond(
                                id,
                                serde_json::to_value(DynamicToolCallResult {
                                    content_items: vec![DynamicToolContentItem::InputText {
                                        text,
                                    }],
                                    success,
                                })?,
                            )
                            .await?;
                        } else {
                            rpc.respond_error(id, -32601, "unsupported server request")
                                .await?;
                            return Err(format!(
                                "Codex requested unsupported server method {method:?}"
                            )
                            .into());
                        }
                    }
                    RpcEvent::Notification(notification) => match notification.method.as_str() {
                        "item/started" | "item/completed" => {
                            if notification
                                .params
                                .get("threadId")
                                .and_then(Value::as_str)
                                != Some(thread_id.as_str())
                                || notification
                                    .params
                                    .get("turnId")
                                    .and_then(Value::as_str)
                                    != Some(turn_id.as_str())
                            {
                                continue;
                            }
                            let item_type = notification
                                .params
                                .pointer("/item/type")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            if !matches!(
                                item_type,
                                "agentMessage" | "reasoning" | "plan" | "dynamicToolCall" | "userMessage"
                            ) {
                                return Err(format!(
                                    "Codex emitted non-mediated item type {item_type:?}; failing closed"
                                )
                                .into());
                            }
                            if notification.method == "item/completed"
                                && notification
                                    .params
                                    .pointer("/item/type")
                                    .and_then(Value::as_str)
                                    == Some("agentMessage")
                                && let Some(text) = notification
                                    .params
                                    .pointer("/item/text")
                                    .and_then(Value::as_str)
                            {
                                final_text = Some(text.to_owned());
                            }
                        }
                        "thread/tokenUsage/updated" => {
                            let usage: ThreadTokenUsageUpdatedParams =
                                serde_json::from_value(notification.params)?;
                            if usage.thread_id != thread_id || usage.turn_id != turn_id {
                                continue;
                            }
                            usage_after = Some(usage.token_usage.total);
                        }
                        "turn/completed" => {
                            let thread = notification
                                .params
                                .get("threadId")
                                .and_then(Value::as_str);
                            let turn_identity = notification
                                .params
                                .pointer("/turn/id")
                                .and_then(Value::as_str);
                            let status = notification
                                .params
                                .pointer("/turn/status")
                                .and_then(Value::as_str);
                            if thread != Some(thread_id.as_str())
                                || turn_identity != Some(turn_id.as_str())
                            {
                                return Err("Codex turn completion identity mismatch".into());
                            }
                            if status != Some("completed") {
                                if let Some(turn) = &turn {
                                    release_provider_turn(context, &turn.reservation_id).await;
                                }
                                let detail = notification
                                    .params
                                    .pointer("/turn/error")
                                    .map(Value::to_string)
                                    .map(|detail| detail.chars().take(512).collect::<String>())
                                    .unwrap_or_default();
                                return Err(format!(
                                    "Codex turn terminal status is {status:?}: {detail}"
                                )
                                .into());
                            }
                            break;
                        }
                        _ => {}
                    },
                }
            }
            let turn_usage = match usage_after {
                Some(after) => {
                    let input = after.input_tokens.saturating_sub(usage_before.input_tokens);
                    let output = after.output_tokens.saturating_sub(usage_before.output_tokens);
                    let total = after.total_tokens.saturating_sub(usage_before.total_tokens);
                    usage_before = after;
                    codex_usage(&factory_native::protocol::TokenUsageBreakdown {
                        cached_input_tokens: 0,
                        input_tokens: input,
                        output_tokens: output,
                        reasoning_output_tokens: 0,
                        total_tokens: total,
                    })
                }
                None => ProviderUsage::default(),
            };
            if let Some(turn) = &turn {
                settle_provider_turn(context, &turn.reservation_id, &turn_usage).await?;
            }
            usage.input_tokens = usage.input_tokens.saturating_add(turn_usage.input_tokens);
            usage.output_tokens = usage.output_tokens.saturating_add(turn_usage.output_tokens);
            usage.total_tokens = usage.total_tokens.saturating_add(turn_usage.total_tokens);
            usage.cost_micros = usage.cost_micros.saturating_add(turn_usage.cost_micros);
            usage.known = usage.known || turn_usage.known;
            if let Ok(mut tracker) = context.usage_tracker.lock() {
                *tracker = usage;
            }
            let Some(text) = final_text else {
                prompt = "Return exactly the required final JSON object now. If you have not written a file, use write_file first.".into();
                continue;
            };
            let action = match extract_action_text(&text) {
                Ok(action) => action,
                Err(error) => {
                    post_tool_event(
                        context,
                        "invalid-codex-action",
                        "final",
                        "ERROR",
                        &format!("invalid Codex action: {error}"),
                    )
                    .await?;
                    prompt = format!(
                        "Your previous final response was rejected: {error}. Return exactly one JSON object with type final."
                    );
                    continue;
                }
            };
            match action {
                ProviderAction::Final { files, explanation } => {
                    if !tool_state.written && files.is_empty() {
                        prompt = "No file has been written. Use write_file now, then return the final JSON object.".into();
                        continue;
                    }
                    if !explanation.trim().is_empty() {
                        let bounded: String = explanation.chars().take(4_000).collect();
                        post_tool_event(context, "final-explanation", "final", "RESULT", &bounded)
                            .await?;
                    }
                    if !files.is_empty() {
                        if !context
                            .assignment
                            .allowed_tools
                            .iter()
                            .any(|tool| tool == "write_file")
                        {
                            return Err(
                                "Codex returned files without write_file permission".into()
                            );
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
                            !tool_state.written,
                        )
                        .await?;
                    }
                    let _ = tokio::time::timeout(
                        Duration::from_secs(5),
                        rpc.archive_thread(&thread_id),
                    )
                    .await;
                    let candidate =
                        supervisor_finalize(context, &handle.workspace_id, &handle.capability)
                            .await?;
                    cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
                    return Ok(ProviderRun {
                        candidate,
                        provider_session_id: thread_id,
                        turn_id: last_turn_id.clone(),
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
                    let arguments = json!({
                        "path": path,
                        "content": content,
                        "argv": argv,
                        "timeout_secs": timeout_secs
                    });
                    let (success, result) = execute_codex_tool(
                        context,
                        &handle,
                        &call_id,
                        &tool,
                        &arguments,
                        &mut tool_state,
                    )
                    .await?;
                    prompt = format!(
                        "Tool result (success={success}): {result}. Continue with another dynamic tool or return the final JSON object."
                    );
                }
            }
        }
        if tool_state.written {
            post_tool_event(
                context,
                "codex-turn-budget-exhausted",
                "final",
                "RESULT",
                "Codex wrote files but did not emit a final response; preserving applied writes",
            )
            .await?;
            let _ = rpc.archive_thread(&thread_id).await;
            let candidate =
                supervisor_finalize(context, &handle.workspace_id, &handle.capability).await?;
            cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await?;
            return Ok(ProviderRun {
                candidate,
                provider_session_id: thread_id,
                turn_id: last_turn_id,
                usage,
            });
        }
        let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
        Err("Codex exceeded the authorized turn budget without writing a file".into())
    }
    .await;
    if result.is_err() {
        let _ = cleanup(context.supervisor, &handle.workspace_id, &handle.capability).await;
    }
    result
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
            let check = context
                .assignment
                .action_spec
                .checks
                .iter()
                .find(|check| check.command == argv)
                .ok_or_else(|| "command does not match a frozen acceptance check".to_string())?;
            if timeout_secs == 0 || timeout_secs > u64::from(check.timeout_secs.min(300)) {
                return Err("command timeout exceeds the frozen check bound".into());
            }
            Ok(ToolDecision::Run)
        }
        _ => Err(format!("unknown tool {tool}")),
    }
}

fn tool_result(
    provider: &str,
    model: &str,
    call_id: &str,
    status: &str,
    output: &str,
    exit_code: Option<i32>,
) -> serde_json::Value {
    json!({
        "model": {"providerID": provider, "modelID": model},
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
    request_sha256: String,
}

fn request_digest(value: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    factory_native::attempt_grant::request_digest(value).map_err(Into::into)
}

async fn authorize_provider_turn(
    context: &ProviderContext<'_>,
    prompt: &serde_json::Value,
) -> Result<ProviderTurnAuthorization, Box<dyn std::error::Error>> {
    let Some(policy) = context.turn_policy else {
        return Err("provider turn authorization requires an egress policy".into());
    };
    let canonical = json!({"model": context.assignment.model, "input": prompt});
    let digest = request_digest(&canonical)?;
    let max_requests = if context.assignment.provider == "codex" {
        8
    } else {
        1
    };
    let response = context
        .client
        .post(context.core.join("worker/provider_turn_authorize")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "session_record_id": context.session_record_id,
            "worker_id": context.worker_id,
            "generation": context.assignment.lease.generation,
            "requested_output_tokens": 4096,
            "request_sha256": digest,
            "max_requests": max_requests
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Core refused provider turn authorization: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    let authorization: ProviderTurnAuthorization = response.json().await?;
    *policy.lock().map_err(|_| "provider turn policy lock")? = Some(TurnPolicy {
        reservation_id: authorization.reservation_id.clone(),
        max_input_tokens: authorization.max_input_tokens,
        max_output_tokens: authorization.max_output_tokens,
        max_total_tokens: authorization.max_total_tokens,
        max_cost_micros: authorization.max_cost_micros,
        model: authorization.model.clone(),
        request_sha256: authorization.request_sha256.clone(),
        expires_at_ms: authorization.expires_at_ms,
        requests: 0,
        max_requests: authorization.max_requests as usize,
        strict_request_digest: env::var("FACTORY_PROVIDER_KIND")
            .map(|kind| !matches!(kind.as_str(), "opencode" | "codex"))
            .unwrap_or(true),
        binding: false,
    });
    Ok(authorization)
}

async fn settle_provider_turn(
    context: &ProviderContext<'_>,
    reservation_id: &str,
    usage: &ProviderUsage,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(policy) = context.turn_policy {
        *policy.lock().map_err(|_| "provider turn policy lock")? = None;
    }
    let response = context
        .client
        .post(context.core.join("worker/provider_turn_settle")?)
        .bearer_auth(context.session_token)
        .json(&json!({
            "usage": {
                "reservation_id": reservation_id,
                "worker_id": context.worker_id,
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "total_tokens": usage.total_tokens,
                "cost_micros": usage.cost_micros,
                "usage_known": usage.known
            }
        }))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!(
            "Core rejected provider turn usage: {} {}",
            response.status(),
            response.text().await.unwrap_or_default()
        )
        .into());
    }
    Ok(())
}

async fn release_provider_turn(context: &ProviderContext<'_>, reservation_id: &str) {
    if let Some(policy) = context.turn_policy
        && let Ok(mut policy) = policy.lock()
    {
        *policy = None;
    }
    let _ = context
        .client
        .post(
            context
                .core
                .join("worker/provider_turn_release")
                .unwrap_or_else(|_| context.core.clone()),
        )
        .bearer_auth(context.session_token)
        .json(&json!({
            "reservation_id": reservation_id,
            "worker_id": context.worker_id
        }))
        .send()
        .await;
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
            "operation": request.operation,
            "request_sha256": request.request_sha256
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
    let digest = request_digest(&json!({"files": &files, "reset": reset}))?;
    let operation = request_workspace_operation(WorkspaceOperation {
        client: context.client,
        core: context.core,
        session_token: context.session_token,
        worker_id: context.worker_id,
        attempt_id: &context.assignment.lease.attempt_id,
        generation: context.assignment.lease.generation,
        workspace_id,
        operation: "write",
        request_sha256: digest,
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
    let digest = request_digest(&json!({"argv": argv, "timeout_secs": timeout_secs}))?;
    let operation = request_workspace_operation(WorkspaceOperation {
        client: context.client,
        core: context.core,
        session_token: context.session_token,
        worker_id: context.worker_id,
        attempt_id: &context.assignment.lease.attempt_id,
        generation: context.assignment.lease.generation,
        workspace_id,
        operation: "run",
        request_sha256: digest,
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
    let digest = request_digest(&json!({"operation": "finalize", "workspace_id": workspace_id}))?;
    let operation = request_workspace_operation(WorkspaceOperation {
        client: context.client,
        core: context.core,
        session_token: context.session_token,
        worker_id: context.worker_id,
        attempt_id: &context.assignment.lease.attempt_id,
        generation: context.assignment.lease.generation,
        workspace_id,
        operation: "finalize",
        request_sha256: digest,
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
        "agent": "plan",
        "tools": opencode_disabled_tools(),
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
        context.api_directory,
    )
    .await?;
    let mut event_stream = event_response.bytes_stream();
    let mut parser = factory_native::sse::SseParser::new(1_048_576);
    let created = factory_native::runtime::create_opencode_session(
        &http,
        context.base_url,
        context.password,
        context.api_directory,
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
    if let Ok(mut provider_session) = context.provider_session.lock() {
        *provider_session = Some(provider_session_id.clone());
    }
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
            context.api_directory,
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
    let turn = if context.turn_policy.is_some() {
        Some(authorize_provider_turn(context, &prompt).await?)
    } else {
        None
    };
    if let Err(error) = factory_native::runtime::submit_opencode_prompt(
        &http,
        context.base_url,
        context.password,
        context.api_directory,
        &provider_session_id,
        &prompt,
    )
    .await
    {
        if let Some(turn) = &turn {
            release_provider_turn(context, &turn.reservation_id).await;
        }
        return Err(error.into());
    }
    let terminal = match tokio::time::timeout(
        Duration::from_secs(context.assignment.deadline_secs),
        factory_native::runtime::wait_for_opencode_prompt_terminal(
            &mut event_stream,
            &mut parser,
            &provider_session_id,
            false,
        ),
    )
    .await
    {
        Ok(Ok(terminal)) => terminal,
        Ok(Err(error)) => {
            if let Some(turn) = &turn {
                release_provider_turn(context, &turn.reservation_id).await;
            }
            return Err(error.into());
        }
        Err(_) => {
            if let Some(turn) = &turn {
                release_provider_turn(context, &turn.reservation_id).await;
            }
            return Err("provider deadline exceeded".into());
        }
    };
    let response = match &terminal {
        factory_native::runtime::OpenCodePromptTerminal::Idle {
            cancel_requested: false,
            ..
        } => {
            factory_native::runtime::read_opencode_assistant_message(
                &http,
                context.base_url,
                context.password,
                context.api_directory,
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
            if let Some(turn) = &turn {
                release_provider_turn(context, &turn.reservation_id).await;
            }
            let _ = factory_native::runtime::delete_opencode_session(
                &http,
                context.base_url,
                context.password,
                context.api_directory,
                &session_id,
            )
            .await;
            return Err(format!("provider session {session_id} failed: {error}").into());
        }
        factory_native::runtime::OpenCodePromptOutcome::AbortRequestedAndIdle { session_id } => {
            if let Some(turn) = &turn {
                release_provider_turn(context, &turn.reservation_id).await;
            }
            let _ = factory_native::runtime::delete_opencode_session(
                &http,
                context.base_url,
                context.password,
                context.api_directory,
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
            context.api_directory,
            &session_id,
        )
        .await;
        let _ = factory_native::runtime::delete_opencode_session(
            &http,
            context.base_url,
            context.password,
            context.api_directory,
            &session_id,
        )
        .await;
        return Err("provider cancellation was requested".into());
    }
    factory_native::runtime::delete_opencode_session(
        &http,
        context.base_url,
        context.password,
        context.api_directory,
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
    if let Ok(mut tracker) = context.usage_tracker.lock() {
        *tracker = usage;
    }
    if let Some(turn) = &turn {
        settle_provider_turn(context, &turn.reservation_id, &usage).await?;
    }
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
        let digest = request_digest(&json!({"files": &apply.files, "operation": "apply"}))?;
        let operation = request_workspace_operation(WorkspaceOperation {
            client: context.client,
            core: context.core,
            session_token: context.session_token,
            worker_id: context.worker_id,
            attempt_id: &context.assignment.lease.attempt_id,
            generation: context.assignment.lease.generation,
            workspace_id: &handle.workspace_id,
            operation: "apply",
            request_sha256: digest,
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
    let digest = request_digest(&json!({
        "image": &request.image,
        "argv": &request.argv,
        "timeout_secs": request.timeout_secs,
        "operation": "execute"
    }))?;
    let operation = request_workspace_operation(WorkspaceOperation {
        client,
        core,
        session_token,
        worker_id,
        attempt_id: &assignment.lease.attempt_id,
        generation: assignment.lease.generation,
        workspace_id: &handle.workspace_id,
        operation: "execute",
        request_sha256: digest,
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
