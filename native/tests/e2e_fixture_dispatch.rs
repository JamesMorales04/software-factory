//! Deterministic end-to-end fixture dispatch.
//!
//! Starts the real Core and Worker, authorizes an R0 FIXTURE Campaign, leases an
//! Attempt, creates a Git candidate, ingests independent receipts and verifies
//! the deterministic gate. No provider session is involved.

use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const OPERATOR_TOKEN: &str = "operator-token-e2e-dispatch";
const BOOTSTRAP_TOKEN: &str = "bootstrap-token-123456789012345678901234";
const VERIFIER_TOKEN: &str = "verifier-token-1234567890123456789012";
const REVIEWER_TOKEN: &str = "reviewer-token-1234567890123456789012";
const ATTEMPT_GRANT_KEY: &str = "attempt-grant-key-0123456789abcdef";
const SUPERVISOR_CAPABILITY_KEY: &str = "capability-key-0123456789abcdef0";
const SUPERVISOR_TOKEN: &str = "supervisor-token-123456789012345678901234";
const FIXTURE_IMAGE: &str = "test/image:1";

struct CoreProcess {
    child: Child,
    log: PathBuf,
}

impl CoreProcess {
    fn start(database: &Path, port: u16, log: &Path) -> Self {
        Self::start_with_lead_fixture(database, port, log, 1, "parallel")
    }

    fn start_with_lead_fixture(
        database: &Path,
        port: u16,
        log: &Path,
        units: usize,
        mode: &str,
    ) -> Self {
        let file = File::create(log).expect("core log");
        let child = Command::new(env!("CARGO_BIN_EXE_factory-core"))
            .env("FACTORY_DATABASE", database)
            .env("FACTORY_BIND", format!("127.0.0.1:{port}"))
            .env("FACTORY_OPERATOR_TOKEN", OPERATOR_TOKEN)
            .env("FACTORY_ATTEMPT_GRANT_KEY", ATTEMPT_GRANT_KEY)
            .env("FACTORY_PROJECTS_ROOT", std::env::temp_dir())
            .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
            .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
            .env("FACTORY_REVIEWER_TOKEN", REVIEWER_TOKEN)
            .env("FACTORY_LEAD_FIXTURE_ASSESSOR", "1")
            .env("FACTORY_ENABLE_AUTONOMOUS_START", "1")
            .env("FACTORY_LEAD_FIXTURE_UNITS", units.to_string())
            .env("FACTORY_LEAD_FIXTURE_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(file))
            .spawn()
            .expect("spawn factory-core");
        Self {
            child,
            log: log.to_path_buf(),
        }
    }

    fn start_with_lead_model(database: &Path, port: u16, log: &Path, model_url: &str) -> Self {
        let file = File::create(log).expect("core log");
        let child = Command::new(env!("CARGO_BIN_EXE_factory-core"))
            .env("FACTORY_DATABASE", database)
            .env("FACTORY_BIND", format!("127.0.0.1:{port}"))
            .env("FACTORY_OPERATOR_TOKEN", OPERATOR_TOKEN)
            .env("FACTORY_ATTEMPT_GRANT_KEY", ATTEMPT_GRANT_KEY)
            .env("FACTORY_PROJECTS_ROOT", std::env::temp_dir())
            .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
            .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
            .env("FACTORY_REVIEWER_TOKEN", REVIEWER_TOKEN)
            .env("FACTORY_ENABLE_AUTONOMOUS_START", "1")
            .env("FACTORY_MODEL_URL", model_url)
            .env("FACTORY_MODEL_TOKEN", "lead-model-token-0123456789abcdef")
            .env("FACTORY_MODEL_PROVIDER", "opencode")
            .env("FACTORY_MODEL_NAME", "deepseek-v4.1-flash")
            .env("FACTORY_ALLOW_TEST_UPSTREAM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(file))
            .spawn()
            .expect("spawn factory-core");
        Self {
            child,
            log: log.to_path_buf(),
        }
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

struct SupervisorProcess {
    child: Child,
    log: PathBuf,
}

struct SupervisorSpec<'a> {
    port: u16,
    repository: &'a Path,
    log: &'a Path,
    workspace_root: &'a Path,
    state_root: &'a Path,
    volume_root: &'a Path,
    image: &'a str,
    local_backend: bool,
    core_url: &'a str,
}

impl SupervisorProcess {
    fn start(spec: SupervisorSpec<'_>) -> Self {
        let file = File::create(spec.log).expect("supervisor log");
        let mut command = Command::new(env!("CARGO_BIN_EXE_factory-supervisor"));
        command
            .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
            .env("FACTORY_CORE_URL", spec.core_url)
            .env("FACTORY_ATTEMPT_GRANT_KEY", ATTEMPT_GRANT_KEY)
            .env(
                "FACTORY_SUPERVISOR_CAPABILITY_KEY",
                SUPERVISOR_CAPABILITY_KEY,
            )
            .env(
                "FACTORY_SUPERVISOR_BIND",
                format!("127.0.0.1:{}", spec.port),
            )
            .env("FACTORY_SUPERVISOR_WORKSPACE_ROOT", spec.workspace_root)
            .env("FACTORY_SUPERVISOR_STATE_ROOT", spec.state_root)
            .env("FACTORY_SUPERVISOR_PROJECT_ROOTS", spec.repository)
            .env("FACTORY_SUPERVISOR_IMAGES", spec.image)
            .env("FACTORY_SUPERVISOR_MAX_RUNTIME_SECS", "600")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(file));
        if spec.local_backend {
            command
                .env("FACTORY_SUPERVISOR_TEST_BACKEND", "local")
                .env("FACTORY_SUPERVISOR_TEST_VOLUME_ROOT", spec.volume_root);
        }
        let child = command.spawn().expect("spawn factory-supervisor");
        Self {
            child,
            log: spec.log.to_path_buf(),
        }
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SupervisorProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral port")
        .local_addr()
        .expect("local address")
        .port()
}

async fn wait_healthy(client: &reqwest::Client, base: &str, core: &mut CoreProcess) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = core.child.try_wait().expect("core status") {
            let log = std::fs::read_to_string(&core.log).unwrap_or_default();
            panic!("Core exited early with {status}; log:\n{log}");
        }
        if let Ok(response) = client.get(format!("{base}/health")).send().await
            && response.status() == StatusCode::OK
        {
            return;
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(&core.log).unwrap_or_default();
            panic!("Core did not become healthy; log:\n{log}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_supervisor(client: &reqwest::Client, base: &str, supervisor: &mut SupervisorProcess) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = supervisor.child.try_wait().expect("supervisor status") {
            let log = std::fs::read_to_string(&supervisor.log).unwrap_or_default();
            panic!("Supervisor exited early with {status}; log:\n{log}");
        }
        if let Ok(response) = client.get(format!("{base}/health")).send().await
            && response.status() == StatusCode::OK
        {
            return;
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(&supervisor.log).unwrap_or_default();
            panic!("Supervisor did not become healthy; log:\n{log}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn operator(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: String,
    body: Option<Value>,
) -> reqwest::Response {
    let request = client
        .request(method, url)
        .bearer_auth(OPERATOR_TOKEN)
        .header("content-type", "application/json");
    match body {
        Some(body) => request.json(&body).send().await.expect("request"),
        None => request.send().await.expect("request"),
    }
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn run_reviewer(root: &Path, base_url: &str, candidate_id: &str, principal: &str, log_name: &str) {
    run_reviewer_with(root, base_url, candidate_id, principal, log_name, None);
}

fn run_reviewer_with(
    root: &Path,
    base_url: &str,
    candidate_id: &str,
    principal: &str,
    log_name: &str,
    mechanism: Option<&str>,
) {
    let log = File::create(root.join(log_name)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_factory-reviewer"));
    command
        .env("FACTORY_CORE_URL", base_url)
        .env("FACTORY_REVIEWER_TOKEN", REVIEWER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", candidate_id)
        .env("FACTORY_REVIEWER_PRINCIPAL", principal)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    if let Some(mechanism) = mechanism {
        command.env("FACTORY_REVIEW_MECHANISM", mechanism);
    }
    let status = command.status().expect("reviewer");
    assert!(
        status.success(),
        "reviewer failed: {}",
        std::fs::read_to_string(root.join(log_name)).unwrap_or_default()
    );
}

async fn expire_fixture_lease(database: &Path, attempt_id: &str) {
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(database)
                .create_if_missing(false),
        )
        .await
        .expect("open fixture database to advance lease expiry");
    let changed =
        sqlx::query::query("UPDATE worker_leases SET expires_at_ms=1 WHERE attempt_id=?1")
            .bind(attempt_id)
            .execute(&pool)
            .await
            .expect("advance fixture lease expiry");
    assert_eq!(changed.rows_affected(), 1);
    pool.close().await;
}

fn v2_operations() -> factory_native::execution_policy::OperationLimits {
    factory_native::execution_policy::OperationLimits {
        model_request_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
        tool_call_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
        max_request_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
        max_response_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
        max_generation_tokens: std::num::NonZeroU32::new(8_192).unwrap(),
        max_in_flight_operations: std::num::NonZeroU32::new(4).unwrap(),
    }
}

fn v2_progress() -> factory_native::execution_policy::ProgressPolicy {
    factory_native::execution_policy::ProgressPolicy {
        diagnose_after_stagnant_checkpoints: std::num::NonZeroU32::new(2).unwrap(),
        escalate_after_unsuccessful_replans: std::num::NonZeroU32::new(1).unwrap(),
        observation_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
    }
}

fn v2_operations_json() -> Value {
    let operations = v2_operations();
    json!({
        "modelRequestTimeoutSecs": operations.model_request_timeout_secs.get(),
        "toolCallTimeoutSecs": operations.tool_call_timeout_secs.get(),
        "maxRequestBytes": operations.max_request_bytes.get(),
        "maxResponseBytes": operations.max_response_bytes.get(),
        "maxGenerationTokens": operations.max_generation_tokens.get(),
        "maxInFlightOperations": operations.max_in_flight_operations.get(),
    })
}

fn v2_progress_json() -> Value {
    let progress = v2_progress();
    json!({
        "diagnoseAfterStagnantCheckpoints": progress.diagnose_after_stagnant_checkpoints.get(),
        "escalateAfterUnsuccessfulReplans": progress.escalate_after_unsuccessful_replans.get(),
        "observationTimeoutSecs": progress.observation_timeout_secs.get(),
    })
}

fn v2_grant(
    id: &str,
    project_id: &str,
    provider: &str,
    model: &str,
    autonomous_start: bool,
    max_parallel: u32,
) -> factory_native::lead::StandingDelegationV2 {
    let mut grant = factory_native::lead::StandingDelegationV2 {
        id: id.into(),
        principal_id: "owner".into(),
        audit_id: "audit".into(),
        project_id: project_id.into(),
        revision: 1,
        content_hash: String::new(),
        objective_classes: vec![factory_native::lead::ObjectiveClass::Feature],
        allowed_paths: vec!["src".into()],
        capabilities: vec![
            factory_native::lead::Capability::RepositoryEdit,
            factory_native::lead::Capability::GenericToolchain,
        ],
        risk_classes: vec![factory_native::lead::RiskClass::R0],
        autonomous_start,
        autonomous_risk_classes: if autonomous_start {
            vec![factory_native::lead::RiskClass::R0]
        } else {
            Vec::new()
        },
        route: factory_native::lead::ProviderRoute {
            provider: provider.into(),
            model: model.into(),
            mode: "build".into(),
        },
        max_timeout_secs: 300,
        max_parallel,
        operations: v2_operations(),
        progress: v2_progress(),
        expires_at: chrono::Utc::now() + chrono::Duration::days(1),
        revoked_at: None,
    };
    grant.content_hash = grant.compute_content_hash();
    grant
}

async fn seed_v2_grant(database: &Path, grant: &factory_native::lead::StandingDelegationV2) {
    let db = factory_native::db::Database::open(database)
        .await
        .expect("open database to seed V2 delegation");
    db.create_standing_delegation_v2(grant, &format!("{}-key", grant.id))
        .await
        .expect("seed V2 delegation");
    db.close().await;
}

async fn seed_provider_settings(database: &Path, provider: &str, kind: &str, model: &str) {
    let db = factory_native::db::Database::open(database)
        .await
        .expect("open database to seed provider settings");
    db.save_provider_settings(
        &factory_native::db::ProviderSettingsWrite {
            provider: provider.into(),
            kind: kind.into(),
            model: model.into(),
            base_url: "https://provider.example/v1".into(),
            auth_mode: "api_key".into(),
            credential_ciphertext: "ciphertext".into(),
            credential_nonce: "nonce".into(),
            credential_fingerprint: "configured".into(),
            enabled: true,
            is_default: false,
            max_turns: 4,
            max_input_tokens: 100_000,
            max_output_tokens: 12_000,
            max_cost_micros: 500_000,
            input_micros_per_million: 1_000,
            output_micros_per_million: 2_000,
        },
        "e2e-operator",
    )
    .await
    .expect("seed provider settings");
    db.close().await;
}

fn v2_fixture_authorization(
    authorization_id: &str,
    delegation_id: &str,
    actor: &str,
    rationale: &str,
) -> Value {
    json!({
        "authorization_id": authorization_id,
        "risk_class": "R0",
        "execution_mode": "FIXTURE",
        "actor": actor,
        "rationale": rationale,
        "authorized_paths": ["src"],
        "delegation_id": delegation_id,
        "provider_mode": "build",
        "operations": v2_operations_json(),
        "progress": v2_progress_json(),
    })
}

fn v2_provider_authorization(
    authorization_id: &str,
    delegation_id: &str,
    actor: &str,
    rationale: &str,
    provider: &str,
    model: &str,
    authorized_micros: u64,
) -> Value {
    json!({
        "authorization_id": authorization_id,
        "risk_class": "R0",
        "execution_mode": "TRUSTED_LOCAL_PROVIDER",
        "actor": actor,
        "rationale": rationale,
        "authorized_paths": ["src"],
        "provider": provider,
        "model": model,
        "provider_mode": "build",
        "delegation_id": delegation_id,
        "allowed_tools": ["write_file", "run_command"],
        "operations": v2_operations_json(),
        "progress": v2_progress_json(),
        "authorized_micros": authorized_micros,
    })
}

#[allow(clippy::too_many_arguments)]
async fn start_request_scoped_provider_campaign(
    client: &reqwest::Client,
    base_url: &str,
    database: &Path,
    project_id: &str,
    action_id: &str,
    config_hash: &str,
    delegation_id: &str,
    provider: &str,
    model: &str,
    objective: &str,
    suffix: &str,
) -> Value {
    let classified = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/actions/{action_id}/classify"),
        Some(json!({
            "risk_class": "R0",
            "actor": "e2e-operator",
            "rationale": "request-scoped provider lifecycle",
            "idempotency_key": format!("{suffix}-classification")
        })),
    )
    .await;
    assert_eq!(
        classified.status(),
        StatusCode::OK,
        "request-scoped provider action must be classified R0"
    );
    let lead: Value = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id": project_id,
            "action_id": action_id,
            "delegation_id": delegation_id,
            "objective": objective,
            "project_config_hash": config_hash,
            "idempotency_key": format!("{suffix}-lead-request"),
            "authorized_micros": 5_000_000
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let request_id = lead["id"].as_str().unwrap().to_owned();
    let db = factory_native::db::Database::open(database)
        .await
        .expect("open database for model assessment fixture");
    db.begin_lead_assessment(&request_id)
        .await
        .expect("begin assessing request");
    let request = db.lead_request(&request_id).await.expect("lead request");
    let project_config = db.project_config(project_id).await.expect("project config");
    let action = db.action(action_id).await.expect("action");
    let grant = db
        .standing_delegation_v2(delegation_id)
        .await
        .expect("V2 delegation");
    let binding = factory_native::lead::ActionBinding {
        id: action.id.clone(),
        project_id: project_id.into(),
        status: factory_native::lead::ActionStatus::Pending,
    };
    let (assessment, proposal) = factory_native::lead::fixture_lead_assessment(
        &request.id,
        &project_config,
        &binding,
        &action.spec,
        &request.objective,
        &grant,
        chrono::Utc::now(),
    )
    .expect("deterministic MODEL assessment fixture");
    let assessment_input = factory_native::lead::AssessmentInput {
        project_id: project_id.into(),
        action_id: action_id.into(),
        request_id: request.id.clone(),
        objective: request.objective.clone(),
        allowed_evidence_refs: std::collections::BTreeSet::from(["repository-files".into()]),
    };
    db.record_lead_assessment(
        &request.id,
        "MODEL",
        &assessment,
        &assessment_input,
        &format!("{suffix}-assessment"),
    )
    .await
    .expect("record MODEL assessment");
    db.record_lead_proposal(&request.id, &proposal, &format!("{suffix}-proposal"))
        .await
        .expect("record MODEL proposal");
    db.close().await;
    assert_eq!(
        operator(
            client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/freeze"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let started = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        Some(v2_provider_authorization(
            &format!("{suffix}-provider-auth"),
            delegation_id,
            "e2e-operator",
            "request-scoped provider lifecycle",
            provider,
            model,
            5_000_000,
        )),
    )
    .await;
    let status = started.status();
    let body = started.text().await.unwrap_or_default();
    assert_eq!(status, StatusCode::CREATED, "{body}");
    serde_json::from_str(&body).expect("started Campaign projection")
}

#[tokio::test]
async fn fixture_campaign_dispatches_candidate_and_gate_without_provider() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-dispatch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "E2E Fixture"]);
    git(
        &repository,
        &["config", "user.email", "e2e@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);

    let database = root.join("factory.sqlite");
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&database, port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "project-key"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "fixture dispatch",
            "provider": "opencode",
            "model": "test/model",
            "idempotency_key": "action-key",
            "allowed_paths": ["src"],
            "checks": [
                {
                    "id": "content",
                    "kind": "functional",
                    "description": "fixture content",
                    "command": ["grep", "-q", "fixture candidate", "src/fixture.txt"],
                    "expectExit": 0,
                    "timeoutSecs": 30
                },
                {
                    "id": "negative",
                    "kind": "negative",
                    "description": "absent content is rejected",
                    "command": ["grep", "-q", "absent", "src/fixture.txt"],
                    "expectExit": 1,
                    "timeoutSecs": 30
                }
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    let spec_sha = action["spec_sha256"].as_str().unwrap().to_owned();
    let campaign: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns"),
        Some(json!({"action_id": action_id, "idempotency_key": "campaign-key"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let campaign_id = campaign["id"].as_str().unwrap().to_owned();
    assert_eq!(campaign["status"], "BLOCKED_AUTHORIZATION");

    let authorized = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/authorize"),
        Some(v2_fixture_authorization(
            "auth-e2e",
            "fixture-delegation",
            "e2e-operator",
            "deterministic fixture E2E",
        )),
    )
    .await;
    assert_eq!(authorized.status(), StatusCode::CREATED);
    let second_campaign: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns"),
        Some(json!({"action_id":action_id,"idempotency_key":"provider-campaign-2"})),
    )
    .await
    .json()
    .await
    .unwrap();
    // Billable provider execution is request-scoped only. A direct Campaign has
    // no request identity to own the inference envelope, so this boundary is
    // documented and permanently fail-closed.
    let direct_provider = operator(
        &client,
        reqwest::Method::POST,
        format!(
            "{base_url}/campaigns/{}/authorize",
            second_campaign["id"].as_str().unwrap()
        ),
        Some(v2_provider_authorization(
            "provider-auth-direct",
            "provider-grant",
            "e2e-operator",
            "direct provider authorization must fail closed",
            "opencode",
            "deepseek-v4.1-flash",
            5_000_000,
        )),
    )
    .await;
    assert_eq!(
        direct_provider.status(),
        StatusCode::FORBIDDEN,
        "direct TRUSTED_LOCAL_PROVIDER authorization must be forbidden"
    );
    let direct_error: Value = direct_provider.json().await.unwrap();
    assert!(
        direct_error["error"]
            .as_str()
            .is_some_and(|message| message.contains("request-scoped")),
        "direct provider denial must explain the request-scoped path: {direct_error}"
    );

    let run: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/runs"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let run_id = run["id"].as_str().unwrap().to_owned();
    let attempt: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/runs/{run_id}/attempts"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let attempt_id = attempt["id"].as_str().unwrap().to_owned();

    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let podman_backend = std::env::var("FACTORY_E2E_SUPERVISOR_BACKEND")
        .ok()
        .as_deref()
        == Some("podman");
    let fixture_image = if podman_backend {
        "alpine:3.20"
    } else {
        FIXTURE_IMAGE
    };
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: fixture_image,
        local_backend: !podman_backend,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let worker_log = File::create(root.join("worker.log")).unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "worker-e2e")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "registration-key-e2e")
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", fixture_image)
        .env("FACTORY_WORKER_ONCE", "1")
        .env("FACTORY_FIXTURE_FILE", "src/fixture.txt")
        .env("FACTORY_FIXTURE_CONTENT", "fixture candidate\n")
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("worker");
    assert!(
        worker_status.success(),
        "Worker failed: {}",
        std::fs::read_to_string(root.join("worker.log")).unwrap_or_default()
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let candidate = loop {
        let candidates: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = candidates.as_array().and_then(|items| items.first()) {
            break candidate.clone();
        }
        assert!(Instant::now() < deadline, "no candidate recorded");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let candidate_id = candidate["id"].as_str().unwrap().to_owned();
    let commit = candidate["commit_sha"].as_str().unwrap().to_owned();
    let tree = candidate["tree_sha"].as_str().unwrap().to_owned();
    assert_eq!(candidate["attempt_id"], attempt_id);
    assert_eq!(candidate["action_spec_sha256"], spec_sha);
    assert_eq!(
        git(&repository, &["rev-parse", &format!("{commit}^")]),
        base
    );
    assert_eq!(
        git(&repository, &["show", "-s", "--format=%T", &commit]),
        tree
    );
    assert_eq!(
        git(&repository, &["show", &format!("{commit}:src/fixture.txt")]),
        "fixture candidate"
    );
    let runs: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/runs"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        runs.as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == run_id)
            .unwrap()["status"],
        "RUNNING"
    );

    let verifier_log = File::create(root.join("verifier.log")).unwrap();
    let verifier_status = Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", &candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", "verifier-process-e2e")
        .env(
            "FACTORY_VERIFIER_WORKSPACE",
            root.join("verifier-workspaces"),
        )
        .env("FACTORY_VERIFIER_IMAGE", fixture_image)
        .env(
            "FACTORY_VERIFIER_TEST_BACKEND",
            if podman_backend { "" } else { "local" },
        )
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join("verifier-volumes"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(verifier_log.try_clone().unwrap()))
        .stderr(Stdio::from(verifier_log))
        .status()
        .expect("verifier");
    assert!(
        verifier_status.success(),
        "verifier failed: {}",
        std::fs::read_to_string(root.join("verifier.log")).unwrap_or_default()
    );
    run_reviewer(
        &root,
        &base_url,
        &candidate_id,
        "reviewer-process-e2e",
        "reviewer.log",
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/gate"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate["outcome"], "ACCEPTED");
    let runs: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/runs"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        runs.as_array()
            .unwrap()
            .iter()
            .find(|run| run["id"] == run_id)
            .unwrap()["status"],
        "ACCEPTED"
    );

    supervisor.stop();
    core.stop();
    core = CoreProcess::start(&database, port, &root.join("core.log"));
    wait_healthy(&client, &base_url, &mut core).await;
    let gate_after: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/gate"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate_after["outcome"], "ACCEPTED");

    supervisor.stop();
    core.stop();
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}

struct FixtureCandidate {
    candidate_id: String,
    commit_sha: String,
    tree_sha: String,
    action_spec_sha256: String,
}

#[allow(clippy::too_many_arguments)]
async fn drive_fixture_campaign_to_candidate(
    client: &reqwest::Client,
    base_url: &str,
    project_id: &str,
    supervisor_url: &str,
    image: &str,
    suffix: &str,
    objective: &str,
    content: &str,
) -> FixtureCandidate {
    let action: Value = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": objective,
            "provider": "opencode",
            "model": "deepseek-v4.1-flash",
            "idempotency_key": format!("{suffix}-action"),
            "allowed_paths": ["src"],
            "checks": [
                {
                    "id": "functional",
                    "kind": "functional",
                    "description": "candidate content exists",
                    "command": ["grep", "-q", "fixture candidate", "src/fixture.txt"],
                    "expectExit": 0,
                    "timeoutSecs": 30
                },
                {
                    "id": "negative",
                    "kind": "negative",
                    "description": "absent content is rejected",
                    "command": ["grep", "-q", "absent", "src/fixture.txt"],
                    "expectExit": 1,
                    "timeoutSecs": 30
                }
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    let action_spec_sha256 = action["spec_sha256"].as_str().unwrap().to_owned();
    let campaign: Value = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns"),
        Some(json!({"action_id": action_id, "idempotency_key": format!("{suffix}-campaign")})),
    )
    .await
    .json()
    .await
    .unwrap();
    let campaign_id = campaign["id"].as_str().unwrap().to_owned();
    let authorized = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/authorize"),
        Some(v2_fixture_authorization(
            &format!("{suffix}-auth"),
            "semantic-review-delegation",
            "semantic-review-e2e",
            "deterministic semantic review fixture",
        )),
    )
    .await;
    assert_eq!(authorized.status(), StatusCode::CREATED);
    let run: Value = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/runs"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let attempt: Value = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/runs/{}/attempts", run["id"].as_str().unwrap()),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert!(attempt["id"].is_string());

    let worker_log = File::create(
        std::env::temp_dir().join(format!("factory-semantic-review-worker-{suffix}.log")),
    )
    .unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", format!("worker-{suffix}"))
        .env(
            "FACTORY_WORKER_REGISTRATION_KEY",
            format!("registration-{suffix}"),
        )
        .env("FACTORY_SUPERVISOR_URL", supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", image)
        .env("FACTORY_WORKER_ONCE", "1")
        .env("FACTORY_FIXTURE_FILE", "src/fixture.txt")
        .env("FACTORY_FIXTURE_CONTENT", content)
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("worker");
    assert!(worker_status.success(), "Worker failed for {suffix}");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let candidates: Value = operator(
            client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = candidates.as_array().and_then(|items| {
            items
                .iter()
                .find(|candidate| candidate["action_spec_sha256"] == action_spec_sha256)
        }) {
            return FixtureCandidate {
                candidate_id: candidate["id"].as_str().unwrap().to_owned(),
                commit_sha: candidate["commit_sha"].as_str().unwrap().to_owned(),
                tree_sha: candidate["tree_sha"].as_str().unwrap().to_owned(),
                action_spec_sha256,
            };
        }
        assert!(Instant::now() < deadline, "no candidate for {suffix}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn run_fixture_verifier(
    root: &Path,
    base_url: &str,
    candidate_id: &str,
    principal: &str,
    suffix: &str,
    image: &str,
) {
    let verifier_log = File::create(root.join(format!("verifier-{suffix}.log"))).unwrap();
    let verifier_status = Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", principal)
        .env(
            "FACTORY_VERIFIER_WORKSPACE",
            root.join(format!("verifier-workspaces-{suffix}")),
        )
        .env("FACTORY_VERIFIER_IMAGE", image)
        .env("FACTORY_VERIFIER_TEST_BACKEND", "local")
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join(format!("verifier-volumes-{suffix}")),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(verifier_log.try_clone().unwrap()))
        .stderr(Stdio::from(verifier_log))
        .status()
        .expect("verifier");
    assert!(
        verifier_status.success(),
        "verifier failed: {}",
        std::fs::read_to_string(root.join(format!("verifier-{suffix}.log"))).unwrap_or_default()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn semantic_reviewer_model_mode_binds_findings_provenance_and_rejects_malformed_output() {
    use axum::{Router, routing::post};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockModel {
        content: Arc<Mutex<String>>,
        calls: Arc<Mutex<u64>>,
    }
    async fn chat(
        axum::extract::State(state): axum::extract::State<MockModel>,
    ) -> axum::Json<Value> {
        *state.calls.lock().unwrap() += 1;
        axum::Json(json!({
            "choices":[{"message":{"content": state.content.lock().unwrap().clone()}}],
            "usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,"cost_micros":25}
        }))
    }
    let model_content = Arc::new(Mutex::new(String::new()));
    let model_calls = Arc::new(Mutex::new(0));
    let model_app = Router::new()
        .route("/chat/completions", post(chat))
        .with_state(MockModel {
            content: model_content.clone(),
            calls: model_calls.clone(),
        });
    let model_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_url = format!("http://{}", model_listener.local_addr().unwrap());
    let model_server = tokio::spawn(async move {
        axum::serve(model_listener, model_app).await.unwrap();
    });

    let root = std::env::temp_dir().join(format!(
        "factory-e2e-semantic-review-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Semantic Review E2E"]);
    git(
        &repository,
        &["config", "user.email", "semantic-review@example.invalid"],
    );
    std::fs::write(
        repository.join("seed.txt"),
        "seed
",
    )
    .unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let database = root.join("factory.sqlite");
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core =
        CoreProcess::start_with_lead_model(&database, port, &root.join("core.log"), &model_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;
    operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/settings/runtime"),
        Some(json!({
            "semanticReviewEnabled": true,
            "semanticReviewMaxOutputTokens": 8192
        })),
    )
    .await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "semantic-review-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();

    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let first = drive_fixture_campaign_to_candidate(
        &client,
        &base_url,
        &project_id,
        &supervisor_url,
        FIXTURE_IMAGE,
        "first",
        "Add the bounded fixture content.",
        "fixture candidate
",
    )
    .await;
    run_fixture_verifier(
        &root,
        &base_url,
        &first.candidate_id,
        "semantic-verifier-first",
        "first",
        FIXTURE_IMAGE,
    )
    .await;
    *model_content.lock().unwrap() = json!({
        "schemaRevision": 1,
        "candidateCommitSha": first.commit_sha,
        "candidateTreeSha": first.tree_sha,
        "actionSpecSha256": first.action_spec_sha256,
        "verdict": "approve",
        "summary": "The bounded fixture satisfies the frozen objective.",
        "confidence": 93,
        "findings": []
    })
    .to_string();
    run_reviewer_with(
        &root,
        &base_url,
        &first.candidate_id,
        "semantic-reviewer-model",
        "reviewer-first.log",
        Some("MODEL"),
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{}/gate", first.candidate_id),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate["outcome"], "ACCEPTED", "{gate}");
    let receipts: Value = operator(
        &client,
        reqwest::Method::GET,
        format!(
            "{base_url}/quality/candidates/{}/receipts",
            first.candidate_id
        ),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let reviewer = receipts
        .as_array()
        .unwrap()
        .iter()
        .find(|receipt| receipt["role"] == "reviewer")
        .unwrap()
        .clone();
    assert_eq!(reviewer["result"], "PASS");
    assert_eq!(reviewer["mechanism"], "MODEL");
    assert_eq!(reviewer["model_provider"], "opencode");
    assert_eq!(reviewer["model_name"], "deepseek-v4.1-flash");
    assert_eq!(reviewer["input_tokens"], 10);
    assert_eq!(reviewer["output_tokens"], 20);
    assert_eq!(reviewer["total_tokens"], 30);
    assert_eq!(reviewer["cost_micros"], 25);
    assert_eq!(reviewer["usage_known"], true);
    let reservation_id = reviewer["model_reservation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let ledger = factory_native::db::Database::open(&database).await.unwrap();
    let calls = ledger
        .model_calls_for_scope(&first.candidate_id)
        .await
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].reservation_id, reservation_id);
    assert_eq!(calls[0].state, "SETTLED");
    assert_eq!(
        calls[0].role,
        factory_native::db::ModelCallRole::SemanticReviewer
    );
    assert_eq!(calls[0].input_tokens, 10);
    assert_eq!(calls[0].output_tokens, 20);
    assert_eq!(calls[0].cost_micros, 25);
    ledger.close().await;

    let second = drive_fixture_campaign_to_candidate(
        &client,
        &base_url,
        &project_id,
        &supervisor_url,
        FIXTURE_IMAGE,
        "second",
        "Add a second bounded fixture content.",
        "fixture candidate two
",
    )
    .await;
    run_fixture_verifier(
        &root,
        &base_url,
        &second.candidate_id,
        "semantic-verifier-second",
        "second",
        FIXTURE_IMAGE,
    )
    .await;
    *model_content.lock().unwrap() = r#"{"verdict":"approve"}"#.into();
    run_reviewer_with(
        &root,
        &base_url,
        &second.candidate_id,
        "semantic-reviewer-model",
        "reviewer-second.log",
        Some("MODEL"),
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{}/gate", second.candidate_id),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_ne!(gate["outcome"], "ACCEPTED", "{gate}");
    let receipts: Value = operator(
        &client,
        reqwest::Method::GET,
        format!(
            "{base_url}/quality/candidates/{}/receipts",
            second.candidate_id
        ),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let reviewer = receipts
        .as_array()
        .unwrap()
        .iter()
        .find(|receipt| receipt["role"] == "reviewer")
        .unwrap()
        .clone();
    assert_eq!(reviewer["result"], "FAIL");
    assert_eq!(reviewer["mechanism"], "MODEL");
    assert!(
        reviewer["findings"]
            .as_array()
            .is_some_and(|findings| !findings.is_empty())
    );
    assert_eq!(*model_calls.lock().unwrap(), 2);

    supervisor.stop();
    model_server.abort();
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn model_lead_start_dispatches_through_supervisor_to_accepted_gate() {
    use axum::{Router, routing::post};
    use factory_native::lead::{ActionBinding, ActionStatus, ProjectConfig};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockModel {
        content: Arc<Mutex<String>>,
    }
    async fn chat(
        axum::extract::State(state): axum::extract::State<MockModel>,
    ) -> axum::Json<Value> {
        axum::Json(json!({
            "choices":[{"message":{"content": state.content.lock().unwrap().clone()}}],
            "usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,"cost_micros":25}
        }))
    }
    async fn provider_chat() -> axum::Json<Value> {
        axum::Json(json!({
            "id":"chatcmpl-mock","object":"chat.completion","created":1,"model":"deepseek-v4.1-flash",
            "choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}
        }))
    }
    let model_content = Arc::new(Mutex::new(String::new()));
    let model_app = Router::new()
        .route("/chat/completions", post(chat))
        .route("/v1/chat/completions", post(provider_chat))
        .with_state(MockModel {
            content: model_content.clone(),
        });
    let model_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_url = format!("http://{}", model_listener.local_addr().unwrap());
    let model_server = tokio::spawn(async move {
        axum::serve(model_listener, model_app).await.unwrap();
    });

    let root = std::env::temp_dir().join(format!(
        "factory-e2e-lead-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Lead E2E"]);
    git(
        &repository,
        &["config", "user.email", "lead@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let database = root.join("factory.sqlite");
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core =
        CoreProcess::start_with_lead_model(&database, port, &root.join("core.log"), &model_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "lead-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision": 1,
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();

    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "Implement the fixture.",
            "provider": "opencode",
            "model": "deepseek-v4.1-flash",
            "idempotency_key": "lead-action",
            "allowed_paths": ["src"],
            "checks": [
                {
                    "id": "build",
                    "kind": "build",
                    "description": "fixture build gate",
                    "command": ["true"],
                    "expectExit": 0,
                    "timeoutSecs": 30
                },
                {
                    "id": "test",
                    "kind": "test",
                    "description": "fixture test gate",
                    "command": ["true"],
                    "expectExit": 0,
                    "timeoutSecs": 30
                },
                {
                    "id": "functional",
                    "kind": "functional",
                    "description": "provider content exists",
                    "command": ["grep", "-q", "provider candidate", "src/provider.txt"],
                    "expectExit": 0,
                    "timeoutSecs": 30
                },
                {
                    "id": "negative",
                    "kind": "negative",
                    "description": "absent content is rejected",
                    "command": ["grep", "-q", "absent", "src/provider.txt"],
                    "expectExit": 1,
                    "timeoutSecs": 30
                }
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    let spec_sha = action["spec_sha256"].as_str().unwrap().to_owned();

    let grant = v2_grant(
        "lead-grant",
        &project_id,
        "opencode",
        "deepseek-v4.1-flash",
        true,
        1,
    );
    seed_v2_grant(&database, &grant).await;
    seed_provider_settings(&database, "opencode", "opencode", "deepseek-v4.1-flash").await;
    let classified = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions/{action_id}/classify"),
        Some(json!({
            "risk_class": "R0",
            "actor": "lead-e2e-operator",
            "rationale": "deterministic R0 fixture classification",
            "idempotency_key": "lead-classification"
        })),
    )
    .await;
    assert_eq!(classified.status(), StatusCode::OK);

    let lead: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id": project_id,
            "action_id": action_id,
            "delegation_id": "lead-grant",
            "objective": "Implement the fixture.",
            "project_config_hash": config_hash,
            "idempotency_key": "lead-request",
            "authorized_micros": 5_000_000
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let request_id = lead["id"].as_str().unwrap().to_owned();
    let project_config: ProjectConfig = serde_json::from_value(config.clone()).unwrap();
    let action_spec: factory_native::db::ActionSpec =
        serde_json::from_value(action["spec"].clone()).unwrap();
    let (assessment, _) = factory_native::lead::fixture_lead_assessment(
        &request_id,
        &project_config,
        &ActionBinding {
            id: action_id.clone(),
            project_id: project_id.clone(),
            status: ActionStatus::Pending,
        },
        &action_spec,
        "Implement the fixture.",
        &grant,
        chrono::Utc::now(),
    )
    .unwrap();
    *model_content.lock().unwrap() = serde_json::to_string(&assessment).unwrap();

    let assessed = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/assess"),
        None,
    )
    .await;
    let assessed_status = assessed.status();
    let assessed_body = assessed.text().await.unwrap_or_default();
    assert_eq!(assessed_status, StatusCode::OK, "{assessed_body}");
    assert!(assessed_body.contains("\"MODEL\""), "{assessed_body}");
    let ledger = factory_native::db::Database::open(&database).await.unwrap();
    let calls = ledger.model_calls_for_scope("lead-grant").await.unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].role, factory_native::db::ModelCallRole::Lead);
    assert_eq!(calls[0].state, "SETTLED");
    assert_eq!(calls[0].input_tokens, 10);
    assert_eq!(calls[0].output_tokens, 20);
    assert!(calls[0].cost_micros > 0);
    let reservation_id = calls[0].reservation_id.clone();
    ledger.close().await;

    // A Lead request without an operator amount must be refused before any
    // billable assessment call is dispatched.
    let no_amount_action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "No-amount Lead request.",
            "provider": "opencode",
            "model": "deepseek-v4.1-flash",
            "idempotency_key": "lead-no-amount-action",
            "allowed_paths": ["src"],
            "checks": [
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"functional","kind":"functional","description":"content","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent","command":["grep","-q","absent","seed.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let no_amount_lead: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id": project_id,
            "action_id": no_amount_action["id"],
            "delegation_id": "lead-grant",
            "objective": "No-amount Lead request.",
            "project_config_hash": config_hash,
            "idempotency_key": "lead-no-amount-request"
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let no_amount_request_id = no_amount_lead["id"].as_str().unwrap().to_owned();
    let refused = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{no_amount_request_id}/assess"),
        None,
    )
    .await;
    assert_eq!(refused.status(), StatusCode::CONFLICT);
    let refused_body: Value = refused.json().await.unwrap();
    assert_eq!(refused_body["code"], "INFERENCE_AUTHORIZATION_REQUIRED");
    let ledger = factory_native::db::Database::open(&database).await.unwrap();
    assert!(
        ledger
            .lead_model_calls(&no_amount_request_id)
            .await
            .unwrap()
            .is_empty(),
        "a refused assessment must not dispatch a paid model call"
    );
    ledger.close().await;

    let lead_projection: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/lead/requests/{request_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(lead_projection["request"]["assessment_mechanism"], "MODEL");
    let projected_calls = lead_projection["model_calls"].as_array().unwrap();
    assert_eq!(projected_calls.len(), 1);
    assert_eq!(projected_calls[0]["reservation_id"], reservation_id);
    assert_eq!(projected_calls[0]["role"], "lead");
    assert_eq!(projected_calls[0]["provider"], "opencode");
    assert_eq!(projected_calls[0]["model"], "deepseek-v4.1-flash");
    assert_eq!(projected_calls[0]["state"], "SETTLED");
    assert_eq!(projected_calls[0]["input_tokens"], 10);
    assert_eq!(projected_calls[0]["output_tokens"], 20);
    let frozen = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/freeze"),
        None,
    )
    .await;
    assert_eq!(frozen.status(), StatusCode::OK);
    let missing_authorization = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        None,
    )
    .await;
    assert!(
        missing_authorization.status().is_client_error(),
        "a MODEL Lead start without authorization must be a 4xx, got {}",
        missing_authorization.status()
    );
    let campaigns_after_missing: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert!(
        campaigns_after_missing
            .as_array()
            .is_some_and(|items| items.is_empty()),
        "a rejected MODEL Lead start must not create a Campaign"
    );
    let changed_amount = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        Some(v2_provider_authorization(
            "lead-live-start-authorization",
            "lead-grant",
            "lead-e2e-operator",
            "changed provider authorization",
            "opencode",
            "deepseek-v4.1-flash",
            6_000_000,
        )),
    )
    .await;
    assert_eq!(changed_amount.status(), StatusCode::CONFLICT);
    let changed_body: Value = changed_amount.json().await.unwrap();
    assert_eq!(changed_body["code"], "INFERENCE_AUTHORIZATION_CHANGED");
    let campaigns_after_changed: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert!(
        campaigns_after_changed
            .as_array()
            .is_some_and(|items| items.is_empty()),
        "a changed-amount Lead start must not create a Campaign"
    );

    let started_response = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        Some(v2_provider_authorization(
            "lead-live-start-authorization",
            "lead-grant",
            "lead-e2e-operator",
            "explicit provider authorization for the model Lead plan",
            "opencode",
            "deepseek-v4.1-flash",
            5_000_000,
        )),
    )
    .await;
    assert_eq!(started_response.status(), StatusCode::CREATED);
    let started: Value = started_response.json().await.unwrap();
    let attempt_id = started["attempt"]["id"].as_str().unwrap().to_owned();
    assert_eq!(started["campaign"]["status"], "AUTHORIZED");
    let campaign_id = started["campaign"]["id"].as_str().unwrap().to_owned();
    let authorization: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns/{campaign_id}/authorization"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(authorization["executionMode"], "TRUSTED_LOCAL_PROVIDER");
    assert_eq!(authorization["provider"], "opencode");
    assert_eq!(authorization["model"], "deepseek-v4.1-flash");
    assert_eq!(authorization["delegationId"], "lead-grant");
    assert_eq!(authorization["schemaVersion"], 2);
    assert!(authorization["legacy"].is_null());
    assert_eq!(
        authorization["policy"]["operations"]["maxGenerationTokens"],
        8_192
    );
    assert!(
        authorization["policy"]["monetaryEnvelopeId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "provider authorization must bind one request envelope: {authorization}"
    );

    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let provider_session_root = std::env::temp_dir().join(format!(
        "fp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let worker_log = File::create(root.join("worker.log")).unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "lead-worker")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "lead-registration")
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_WORKER_ONCE", "1")
        .env("FACTORY_PROVIDER_SANDBOX", "bwrap")
        .env(
            "FACTORY_PROVIDER_BINARY",
            env!("CARGO_BIN_EXE_factory-fixture-provider"),
        )
        .env("FACTORY_PROVIDER_SESSION_ROOT", &provider_session_root)
        .env("FACTORY_PROVIDER_KEEP_SESSION", "1")
        .env("FACTORY_PROVIDER_UPSTREAM_URL", &model_url)
        .env(
            "FACTORY_PROVIDER_UPSTREAM_TOKEN",
            "real-upstream-credential-0123456789",
        )
        .env(
            "FACTORY_PROVIDER_EGRESS_BRIDGE",
            env!("CARGO_BIN_EXE_factory-egress-bridge"),
        )
        .env("FACTORY_ALLOW_TEST_UPSTREAM", "1")
        .env("FACTORY_FIXTURE_PROVIDER_USE_EGRESS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("worker");
    assert!(
        worker_status.success(),
        "Worker failed: {}",
        std::fs::read_to_string(root.join("worker.log")).unwrap_or_default()
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    let candidate = loop {
        let candidates: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = candidates.as_array().and_then(|items| items.first()) {
            break candidate.clone();
        }
        assert!(Instant::now() < deadline, "no Lead candidate recorded");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(candidate["attempt_id"], attempt_id);
    assert_eq!(candidate["action_spec_sha256"], spec_sha);
    let candidate_id = candidate["id"].as_str().unwrap().to_owned();

    let verifier_log = File::create(root.join("verifier.log")).unwrap();
    let verifier_status = Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", &candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", "lead-verifier")
        .env("FACTORY_VERIFIER_WORKSPACE", root.join("verifier-work"))
        .env("FACTORY_VERIFIER_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_VERIFIER_TEST_BACKEND", "local")
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join("verifier-volumes"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(verifier_log.try_clone().unwrap()))
        .stderr(Stdio::from(verifier_log))
        .status()
        .expect("verifier");
    assert!(
        verifier_status.success(),
        "verifier failed: {}",
        std::fs::read_to_string(root.join("verifier.log")).unwrap_or_default()
    );
    run_reviewer(
        &root,
        &base_url,
        &candidate_id,
        "lead-reviewer",
        "reviewer.log",
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/gate"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate["outcome"], "ACCEPTED");

    supervisor.stop();
    model_server.abort();
    core.stop();
    let _ = std::fs::remove_dir_all(provider_session_root);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn parallel_workgraph_reaches_accepted_on_integrated_candidate() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-integration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Integration E2E"]);
    git(
        &repository,
        &["config", "user.email", "integration@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let database = root.join("factory.sqlite");
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start_with_lead_fixture(
        &database,
        port,
        &root.join("core.log"),
        2,
        "parallel",
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "integration-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision": 1,
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "Integrate parallel unit changes.",
            "provider": "opencode",
            "model": "deepseek-v4.1-flash",
            "idempotency_key": "integration-action",
            "allowed_paths": ["src"],
            "checks": [
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"functional","kind":"functional","description":"unit file present","command":["test","-f","src/unit-1.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent file rejected","command":["grep","-q","absent","src/unit-1.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    let classified = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions/{action_id}/classify"),
        Some(json!({
            "risk_class":"R0",
            "actor":"integration-e2e",
            "rationale":"deterministic R0 fixture",
            "idempotency_key":"integration-classification"
        })),
    )
    .await;
    assert_eq!(classified.status(), StatusCode::OK);

    let grant = v2_grant(
        "integration-grant",
        &project_id,
        "opencode",
        "deepseek-v4.1-flash",
        true,
        2,
    );
    seed_v2_grant(&root.join("factory.sqlite"), &grant).await;
    let lead: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id": project_id,
            "action_id": action_id,
            "delegation_id": "integration-grant",
            "objective": "Integrate parallel unit changes.",
            "project_config_hash": config_hash,
            "idempotency_key": "integration-lead-request"
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let request_id = lead["id"].as_str().unwrap().to_owned();
    let assessed = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/assess"),
        None,
    )
    .await;
    let assessed_status = assessed.status();
    let assessed_body = assessed.text().await.unwrap_or_default();
    assert_eq!(assessed_status, StatusCode::OK, "{assessed_body}");
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/freeze"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let started_response = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        None,
    )
    .await;
    assert_eq!(started_response.status(), StatusCode::CREATED);
    let started: Value = started_response.json().await.unwrap();
    assert_eq!(started["campaign"]["status"], "AUTHORIZED");

    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let fixture_map = json!({
        "Unit 1": {"file": "src/unit-1.txt", "content": "one\n"},
        "Unit 2": {"file": "src/unit-2.txt", "content": "two\n"}
    })
    .to_string();
    for expected in 1..=2 {
        let log = File::create(root.join(format!("worker-{expected}.log"))).unwrap();
        let status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
            .env("FACTORY_CORE_URL", &base_url)
            .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
            .env("FACTORY_WORKER_ID", "integration-worker")
            .env(
                "FACTORY_WORKER_REGISTRATION_KEY",
                "integration-registration",
            )
            .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
            .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
            .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
            .env("FACTORY_WORKER_ONCE", "1")
            .env("FACTORY_FIXTURE_MAP", &fixture_map)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .status()
            .expect("unit worker");
        assert!(status.success(), "unit Worker {expected} failed");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let candidates: Value = operator(
                &client,
                reqwest::Method::GET,
                format!("{base_url}/candidates"),
                None,
            )
            .await
            .json()
            .await
            .unwrap();
            if candidates.as_array().map(Vec::len).unwrap_or(0) >= expected {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "unit candidate {expected} missing"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    let integration_log = File::create(root.join("worker-integration.log")).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "integration-worker")
        .env(
            "FACTORY_WORKER_REGISTRATION_KEY",
            "integration-registration",
        )
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_WORKER_ONCE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(integration_log.try_clone().unwrap()))
        .stderr(Stdio::from(integration_log))
        .status()
        .expect("integration worker");
    assert!(status.success(), "integration Worker failed");

    let deadline = Instant::now() + Duration::from_secs(10);
    let integrated = loop {
        let candidates: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = candidates.as_array().and_then(|items| {
            items
                .iter()
                .find(|candidate| candidate["integration"] == true)
        }) {
            break candidate.clone();
        }
        assert!(Instant::now() < deadline, "integrated candidate missing");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let candidate_id = integrated["id"].as_str().unwrap().to_owned();
    assert!(
        integrated["lineage"]
            .as_array()
            .is_some_and(|parents| parents.len() >= 3)
    );

    let verifier_log = File::create(root.join("verifier-integration.log")).unwrap();
    let verifier_status = Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", &candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", "integration-verifier")
        .env(
            "FACTORY_VERIFIER_WORKSPACE",
            root.join("verifier-integration-work"),
        )
        .env("FACTORY_VERIFIER_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_VERIFIER_TEST_BACKEND", "local")
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join("verifier-integration-volumes"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(verifier_log.try_clone().unwrap()))
        .stderr(Stdio::from(verifier_log))
        .status()
        .expect("integration verifier");
    assert!(
        verifier_status.success(),
        "integration verifier failed: {}",
        std::fs::read_to_string(root.join("verifier-integration.log")).unwrap_or_default()
    );
    run_reviewer(
        &root,
        &base_url,
        &candidate_id,
        "integration-reviewer",
        "reviewer-integration.log",
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/gate"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate["outcome"], "ACCEPTED");

    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn restarted_fanin_workgraph_reaches_accepted_on_integrated_base() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-fanin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Fan-in E2E"]);
    git(
        &repository,
        &["config", "user.email", "fanin@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start_with_lead_fixture(
        &root.join("factory.sqlite"),
        port,
        &root.join("core.log"),
        3,
        "fanin",
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;
    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "fanin-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision":1,"allowedPaths":["src"],
            "capabilities":["repository-edit","generic-toolchain"],
            "qualityProfile":"generic","active":true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id":project_id,
            "objective":"Fan-in unit changes.",
            "provider":"opencode","model":"deepseek-v4.1-flash",
            "idempotency_key":"fanin-action","allowed_paths":["src"],
            "checks":[
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"functional","kind":"functional","description":"fan-in output","command":["test","-f","src/unit-3.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent rejected","command":["grep","-q","absent","src/unit-3.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/actions/{action_id}/classify"),
            Some(json!({"risk_class":"R0","actor":"fanin-e2e","rationale":"R0","idempotency_key":"fanin-classify"}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let grant = v2_grant(
        "fanin-grant",
        &project_id,
        "opencode",
        "deepseek-v4.1-flash",
        true,
        2,
    );
    seed_v2_grant(&root.join("factory.sqlite"), &grant).await;
    let lead: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id":project_id,"action_id":action_id,"delegation_id":"fanin-grant",
            "objective":"Fan-in unit changes.","project_config_hash":config_hash,
            "idempotency_key":"fanin-request"
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let request_id = lead["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/assess"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/freeze"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let started: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(started["campaign"]["status"], "AUTHORIZED");
    let campaign_id = started["campaign"]["id"].as_str().unwrap().to_owned();

    // Claim a root Attempt before the process dies. After restart, advance its
    // persisted expiry deterministically so a recovery worker can fence the
    // old generation without waiting for the production 60s lease duration.
    let stale_registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({"worker_id":"stale-fanin-worker","registration_key":"stale-fanin-registration"}))
        .send()
        .await
        .unwrap();
    assert_eq!(stale_registration.status(), StatusCode::OK);
    let stale_session: Value = stale_registration.json().await.unwrap();
    let stale_claim: Value = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(stale_session["session_token"].as_str().unwrap())
        .json(&json!({"worker_id":"stale-fanin-worker"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let stale_assignment = &stale_claim["assignment"];
    let stale_attempt_id = stale_assignment["lease"]["attempt_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(stale_assignment["lease"]["generation"], 1);

    // Restart Core while a real root Attempt is leased.
    core.stop();
    core = CoreProcess::start_with_lead_fixture(
        &root.join("factory.sqlite"),
        port,
        &root.join("core.log"),
        3,
        "fanin",
    );
    wait_healthy(&client, &base_url, &mut core).await;
    expire_fixture_lease(&root.join("factory.sqlite"), &stale_attempt_id).await;
    let recovery_registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({"worker_id":"recovery-fanin-worker","registration_key":"recovery-fanin-registration"}))
        .send()
        .await
        .unwrap();
    assert_eq!(recovery_registration.status(), StatusCode::OK);
    let recovery_session: Value = recovery_registration.json().await.unwrap();
    let recovery_claim: Value = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(recovery_session["session_token"].as_str().unwrap())
        .json(&json!({"worker_id":"recovery-fanin-worker"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let recovery_assignment = &recovery_claim["assignment"];
    assert_eq!(recovery_assignment["lease"]["attempt_id"], stale_attempt_id);
    assert_eq!(recovery_assignment["lease"]["generation"], 2);
    assert_eq!(
        recovery_assignment["action_spec_sha256"], stale_assignment["action_spec_sha256"],
        "recovery must retain the frozen Action specification"
    );
    let stale_report = client
        .post(format!("{base_url}/worker/report"))
        .bearer_auth(stale_session["session_token"].as_str().unwrap())
        .json(&json!({
            "attempt_id":stale_attempt_id,
            "worker_id":"stale-fanin-worker",
            "generation":1,
            "receipt_id":"stale-after-restart",
            "result_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "terminal_status":"FAILED"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        stale_report.status().is_client_error(),
        "stale lease generation must be fenced after Core restart"
    );
    expire_fixture_lease(&root.join("factory.sqlite"), &stale_attempt_id).await;
    let attempts_after_reclaim: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/attempts"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        attempts_after_reclaim
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["id"] == stale_attempt_id)
            .count(),
        1,
        "lease recovery must not create a duplicate dependent/root Attempt"
    );
    let restored: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/lead/requests/{request_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(restored["request"]["state"], "RUNNING");
    assert_eq!(restored["request"]["campaign_id"], campaign_id);
    let restored_proposal: Value =
        serde_json::from_str(restored["proposal"]["proposal_json"].as_str().unwrap()).unwrap();
    assert_eq!(restored_proposal["workUnits"].as_array().unwrap().len(), 3);
    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;
    let map = json!({
        "Unit 1":{"file":"src/unit-1.txt","content":"one\n"},
        "Unit 2":{"file":"src/unit-2.txt","content":"two\n"},
        "Unit 3":{"file":"src/unit-3.txt","content":"three\n"}
    })
    .to_string();
    let run_worker = |map: Option<String>, id: u32| {
        let base_url = base_url.clone();
        let supervisor_url = supervisor_url.clone();
        let root = root.clone();
        async move {
            let log = File::create(root.join(format!("fanin-worker-{id}.log"))).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_factory-worker"));
            command
                .env("FACTORY_CORE_URL", base_url)
                .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
                .env("FACTORY_WORKER_ID", "fanin-worker")
                .env("FACTORY_WORKER_REGISTRATION_KEY", "fanin-registration")
                .env("FACTORY_SUPERVISOR_URL", supervisor_url)
                .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
                .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
                .env("FACTORY_WORKER_ONCE", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log));
            if let Some(map) = map {
                command.env("FACTORY_FIXTURE_MAP", map);
            }
            let status = command.status().expect("fanin worker");
            assert!(status.success(), "fanin Worker {id} failed");
        }
    };
    for id in 1..=2 {
        run_worker(Some(map.clone()), id).await;
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let attempts: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/attempts"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if attempts.as_array().map(Vec::len).unwrap_or(0) >= 3 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fan-in integration attempt missing"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run_worker(None, 3).await;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let attempts: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/attempts"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if attempts.as_array().map(Vec::len).unwrap_or(0) >= 4 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fan-in dependent attempt missing"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    run_worker(Some(map), 4).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    let candidate_id = loop {
        let candidates: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = candidates.as_array().and_then(|items| {
            items
                .iter()
                .rev()
                .find(|candidate| candidate["integration"] == false)
        }) {
            break candidate["id"].as_str().unwrap().to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "fan-in dependent candidate missing"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let verifier_log = File::create(root.join("fanin-verifier.log")).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", &candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", "fanin-verifier")
        .env(
            "FACTORY_VERIFIER_WORKSPACE",
            root.join("fanin-verifier-work"),
        )
        .env("FACTORY_VERIFIER_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_VERIFIER_TEST_BACKEND", "local")
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join("fanin-verifier-volumes"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(verifier_log.try_clone().unwrap()))
        .stderr(Stdio::from(verifier_log))
        .status()
        .expect("fanin verifier");
    assert!(
        status.success(),
        "fanin verifier failed: {}",
        std::fs::read_to_string(root.join("fanin-verifier.log")).unwrap_or_default()
    );
    run_reviewer(
        &root,
        &base_url,
        &candidate_id,
        "fanin-reviewer",
        "fanin-reviewer.log",
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/gate"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate["outcome"], "ACCEPTED");
    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn failed_fanin_dependency_is_not_dispatched() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-fanin-failed-dependency-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Fan-in E2E"]);
    git(
        &repository,
        &["config", "user.email", "failed-dependency@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start_with_lead_fixture(
        &root.join("factory.sqlite"),
        port,
        &root.join("core.log"),
        3,
        "fanin",
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;
    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "failed-dependency-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision":1,"allowedPaths":["src"],
            "capabilities":["repository-edit","generic-toolchain"],
            "qualityProfile":"generic","active":true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id":project_id,
            "objective":"Fan-in unit changes.",
            "provider":"opencode","model":"deepseek-v4.1-flash",
            "idempotency_key":"failed-dependency-action","allowed_paths":["src"],
            "checks":[
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"functional","kind":"functional","description":"fan-in output","command":["test","-f","src/unit-3.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent rejected","command":["grep","-q","absent","src/unit-3.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/actions/{action_id}/classify"),
            Some(json!({"risk_class":"R0","actor":"failed-dependency-e2e","rationale":"R0","idempotency_key":"failed-dependency-classify"}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let grant = v2_grant(
        "failed-dependency-grant",
        &project_id,
        "opencode",
        "deepseek-v4.1-flash",
        true,
        2,
    );
    seed_v2_grant(&root.join("factory.sqlite"), &grant).await;
    let lead: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id":project_id,"action_id":action_id,"delegation_id":"failed-dependency-grant",
            "objective":"Fan-in unit changes.","project_config_hash":config_hash,
            "idempotency_key":"failed-dependency-request"
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let request_id = lead["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/assess"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/freeze"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let started: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let campaign_id = started["campaign"]["id"].as_str().unwrap().to_owned();
    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;
    let map = json!({
        "Unit 1":{"file":"../outside.txt","content":"one\n"},
        "Unit 2":{"file":"../outside-too.txt","content":"two\n"},
        "Unit 3":{"file":"src/unit-3.txt","content":"three\n"}
    })
    .to_string();
    let run_worker = |map: Option<String>, id: u32| {
        let base_url = base_url.clone();
        let supervisor_url = supervisor_url.clone();
        let root = root.clone();
        async move {
            let log =
                File::create(root.join(format!("failed-dependency-worker-{id}.log"))).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_factory-worker"));
            command
                .env("FACTORY_CORE_URL", base_url)
                .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
                .env("FACTORY_WORKER_ID", "failed-dependency-worker")
                .env(
                    "FACTORY_WORKER_REGISTRATION_KEY",
                    "failed-dependency-registration",
                )
                .env("FACTORY_SUPERVISOR_URL", supervisor_url)
                .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
                .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
                .env("FACTORY_WORKER_ONCE", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log));
            if let Some(map) = map {
                command.env("FACTORY_FIXTURE_MAP", map);
            }
            let mut child = command.spawn().expect("spawn fan-in Worker");
            let deadline = Instant::now() + Duration::from_secs(20);
            let status = loop {
                if let Some(status) = child.try_wait().expect("poll fan-in Worker") {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    panic!("fan-in Worker {id} exceeded its 20s deadline");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            assert!(
                !status.success(),
                "the traversal fixture should make Worker {id} report a failure"
            );
        }
    };
    run_worker(Some(map.clone()), 1).await;
    let deadline = Instant::now() + Duration::from_secs(15);
    let attempts = loop {
        let attempts: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/attempts"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        let records = attempts.as_array().expect("attempt list");
        if records.iter().any(|attempt| attempt["status"] == "FAILED") {
            break attempts;
        }
        assert!(
            Instant::now() < deadline,
            "fan-in root attempts did not finish"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let records = attempts.as_array().unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|attempt| attempt["status"] == "FAILED")
            .count(),
        1,
        "unsafe fixture path should fail exactly one root unit"
    );
    tokio::time::sleep(Duration::from_millis(250)).await;
    let final_attempts: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/attempts"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(final_attempts.as_array().unwrap().len(), 2);
    let database = root.join("factory.sqlite");
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&database)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let task_states: Vec<(String, String, Option<String>)> = sqlx::query_as::query_as(
        "SELECT unit_id,status,attempt_id FROM lead_tasks WHERE campaign_id=?1 ORDER BY sequence",
    )
    .bind(&campaign_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(task_states.len(), 3);
    assert_eq!(
        task_states
            .iter()
            .filter(|(unit, status, _)| unit != "unit-3" && status == "FAILED")
            .count(),
        1,
        "one failed root attempt should be durably recorded"
    );
    assert_eq!(
        task_states.iter().find(|(unit, _, _)| unit == "unit-3"),
        Some(&("unit-3".to_owned(), "FAILED".to_owned(), None)),
        "the fan-in dependent must be failed without an Attempt"
    );
    pool.close().await;
    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}
#[tokio::test]
async fn failed_fanin_integration_propagates_without_dependent_dispatch() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-fanin-integration-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Fan-in E2E"]);
    git(
        &repository,
        &[
            "config",
            "user.email",
            "integration-failure@example.invalid",
        ],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start_with_lead_fixture(
        &root.join("factory.sqlite"),
        port,
        &root.join("core.log"),
        3,
        "fanin",
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;
    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "integration-failure-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision":1,"allowedPaths":["src"],
            "capabilities":["repository-edit","generic-toolchain"],
            "qualityProfile":"generic","active":true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id":project_id,
            "objective":"Fan-in unit changes.",
            "provider":"opencode","model":"deepseek-v4.1-flash",
            "idempotency_key":"integration-failure-action","allowed_paths":["src"],
            "checks":[
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"functional","kind":"functional","description":"fan-in output","command":["test","-f","src/unit-3.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent rejected","command":["grep","-q","absent","src/unit-3.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/actions/{action_id}/classify"),
            Some(json!({"risk_class":"R0","actor":"integration-failure-e2e","rationale":"R0","idempotency_key":"integration-failure-classify"}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let grant = v2_grant(
        "integration-failure-grant",
        &project_id,
        "opencode",
        "deepseek-v4.1-flash",
        true,
        2,
    );
    seed_v2_grant(&root.join("factory.sqlite"), &grant).await;
    let lead: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests"),
        Some(json!({
            "project_id":project_id,"action_id":action_id,"delegation_id":"integration-failure-grant",
            "objective":"Fan-in unit changes.","project_config_hash":config_hash,
            "idempotency_key":"integration-failure-request"
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let request_id = lead["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/assess"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/lead/requests/{request_id}/freeze"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let started: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/start"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let campaign_id = started["campaign"]["id"].as_str().unwrap().to_owned();
    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;
    let map = json!({
        "Unit 1":{"file":"src/unit-1.txt","content":"one\n"},
        "Unit 2":{"file":"src/unit-2.txt","content":"two\n"},
        "Unit 3":{"file":"src/unit-3.txt","content":"three\n"}
    })
    .to_string();
    let run_worker = |map: Option<String>, id: u32, expected_success: bool| {
        let base_url = base_url.clone();
        let supervisor_url = supervisor_url.clone();
        let root = root.clone();
        async move {
            let log =
                File::create(root.join(format!("integration-failure-worker-{id}.log"))).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_factory-worker"));
            command
                .env("FACTORY_CORE_URL", base_url)
                .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
                .env("FACTORY_WORKER_ID", "integration-failure-worker")
                .env(
                    "FACTORY_WORKER_REGISTRATION_KEY",
                    "integration-failure-registration",
                )
                .env("FACTORY_SUPERVISOR_URL", supervisor_url)
                .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
                .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
                .env("FACTORY_WORKER_ONCE", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log));
            if let Some(map) = map {
                command.env("FACTORY_FIXTURE_MAP", map);
            }
            let mut child = command.spawn().expect("spawn integration-failure Worker");
            let deadline = Instant::now() + Duration::from_secs(20);
            let status = loop {
                if let Some(status) = child.try_wait().expect("poll integration-failure Worker") {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    panic!("integration-failure Worker {id} exceeded its 20s deadline");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            assert_eq!(
                status.success(),
                expected_success,
                "Worker {id} exit status"
            );
        }
    };
    run_worker(Some(map.clone()), 1, true).await;
    run_worker(Some(map.clone()), 2, true).await;

    let candidate_deadline = Instant::now() + Duration::from_secs(15);
    let unit_candidates = loop {
        let candidates: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        let units: Vec<Value> = candidates
            .as_array()
            .unwrap()
            .iter()
            .filter(|candidate| candidate["integration"] == false)
            .cloned()
            .collect();
        if units.len() >= 2 {
            break units;
        }
        assert!(
            Instant::now() < candidate_deadline,
            "two root candidates missing"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let mut expected_commits: Vec<String> = unit_candidates
        .iter()
        .map(|candidate| candidate["commit_sha"].as_str().unwrap().to_owned())
        .collect();
    expected_commits.sort();

    // Force the real integration Worker down its failure path after Core has
    // durably created the fan-in integration Attempt and exact candidate list.
    supervisor.stop();
    run_worker(None, 3, false).await;

    let deadline = Instant::now() + Duration::from_secs(15);
    let attempts: Value = loop {
        let attempts: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/attempts"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if attempts.as_array().is_some_and(|items| {
            items.len() == 3 && items.iter().any(|attempt| attempt["status"] == "FAILED")
        }) {
            break attempts;
        }
        assert!(
            Instant::now() < deadline,
            "integration failure was not recorded"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let failed_request: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/lead/requests/{request_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(failed_request["request"]["state"], "FAILED");
    assert_eq!(failed_request["request"]["campaign_id"], campaign_id);
    let integration_attempts: Vec<&Value> = attempts
        .as_array()
        .unwrap()
        .iter()
        .filter(|attempt| attempt["status"] == "FAILED")
        .collect();
    assert_eq!(
        integration_attempts.len(),
        1,
        "one integration Attempt should fail"
    );
    assert_eq!(
        attempts.as_array().unwrap().len(),
        3,
        "dependent unit was dispatched"
    );

    let candidates: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/candidates"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        candidates
            .as_array()
            .unwrap()
            .iter()
            .filter(|candidate| candidate["integration"] == true)
            .count(),
        0,
        "failed integration must not produce an integrated candidate"
    );
    let database = root.join("factory.sqlite");
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&database)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let integration: (String, String, String, String, String) = sqlx::query_as::query_as(
        "SELECT purpose,status,next_task_id,candidates_json,attempt_id
         FROM lead_integrations WHERE campaign_id=?1",
    )
    .bind(&campaign_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(integration.0, "FANIN");
    assert_eq!(integration.1, "FAILED");
    assert_eq!(integration_attempts[0]["id"], integration.4);
    let mut recorded_commits: Vec<String> = serde_json::from_str(&integration.3).unwrap();
    recorded_commits.sort();
    assert_eq!(
        recorded_commits, expected_commits,
        "integration lineage changed"
    );
    let task_states: Vec<(String, String, Option<String>)> = sqlx::query_as::query_as(
        "SELECT unit_id,status,attempt_id FROM lead_tasks WHERE campaign_id=?1 ORDER BY sequence",
    )
    .bind(&campaign_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(task_states.len(), 3);
    assert_eq!(task_states[0].1, "SUCCEEDED");
    assert_eq!(task_states[1].1, "SUCCEEDED");
    assert_eq!(task_states[2], ("unit-3".into(), "FAILED".into(), None));
    pool.close().await;
    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn campaign_cancellation_fences_a_leased_attempt_through_restart() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-cancel-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "E2E Cancel"]);
    git(
        &repository,
        &["config", "user.email", "cancel@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);
    let database = root.join("factory.sqlite");
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&database, port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "cancel-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project["id"],
            "objective": "cancellation fixture",
            "provider": "opencode",
            "model": "test/model",
            "idempotency_key": "cancel-action",
            "allowed_paths": ["src"],
            "checks": [{
                "id": "content",
                "kind": "functional",
                "description": "fixture content",
                "command": ["true"],
                "expectExit": 0,
                "timeoutSecs": 30
            }]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let campaign: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns"),
        Some(json!({"action_id": action["id"], "idempotency_key": "cancel-campaign"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let campaign_id = campaign["id"].as_str().unwrap().to_owned();
    assert_eq!(
        operator(
            &client,
            reqwest::Method::POST,
            format!("{base_url}/campaigns/{campaign_id}/authorize"),
            Some(v2_fixture_authorization(
                "cancel-auth",
                "cancel-delegation",
                "e2e-operator",
                "cancellation fixture",
            )),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let run: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/runs"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/runs/{}/attempts", run["id"].as_str().unwrap()),
        None,
    )
    .await;

    let registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({"worker_id":"cancel-worker","registration_key":"cancel-registration"}))
        .send()
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::OK);
    let session: Value = registration.json().await.unwrap();
    let claim: Value = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(session["session_token"].as_str().unwrap())
        .json(&json!({"worker_id":"cancel-worker"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let attempt_id = claim["assignment"]["lease"]["attempt_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let runs: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/runs"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(runs.as_array().unwrap()[0]["status"], "RUNNING");

    let unauthorized = client
        .post(format!("{base_url}/campaigns/{campaign_id}/cancel"))
        .json(&json!({"reason":"unauthorized"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let cancelled = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/cancel"),
        Some(json!({"reason":"Product Owner cancelled the fixture"})),
    )
    .await;
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(
        cancelled.json::<Value>().await.unwrap()["status"],
        "CANCELLED"
    );

    let detail: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns/{campaign_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(detail["campaign"]["status"], "CANCELLED");
    assert_eq!(detail["runs"][0]["status"], "CANCELLED");
    assert_eq!(detail["attempts"][0]["status"], "CANCELLED");
    assert_eq!(detail["attempts"][0]["id"], attempt_id);
    assert!(detail["candidates"].as_array().unwrap().is_empty());

    let next_registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({"worker_id":"cancel-worker-2","registration_key":"cancel-registration-2"}))
        .send()
        .await
        .unwrap();
    assert_eq!(next_registration.status(), StatusCode::OK);
    let next_session: Value = next_registration.json().await.unwrap();
    let next_claim: Value = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(next_session["session_token"].as_str().unwrap())
        .json(&json!({"worker_id":"cancel-worker-2"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(next_claim["assignment"].is_null());
    let cancelled_operation = client
        .post(format!("{base_url}/worker/workspace_operation"))
        .bearer_auth(session["session_token"].as_str().unwrap())
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": "cancel-worker",
            "generation": 1,
            "workspace_id": "ws-cancelled",
            "operation": "write"
        }))
        .send()
        .await
        .unwrap();
    assert!(
        cancelled_operation.status().is_client_error(),
        "cancelled Attempt must not receive a workspace operation grant"
    );
    let stale = client
        .post(format!("{base_url}/worker/report"))
        .bearer_auth(session["session_token"].as_str().unwrap())
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": "cancel-worker",
            "generation": 1,
            "receipt_id": "cancel-stale-report",
            "result_sha256": "a".repeat(64),
            "terminal_status": "FAILED"
        }))
        .send()
        .await
        .unwrap();
    assert!(stale.status().is_client_error());

    core.stop();
    core = CoreProcess::start(&database, port, &root.join("core.log"));
    wait_healthy(&client, &base_url, &mut core).await;
    let restored: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns/{campaign_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(restored["campaign"]["status"], "CANCELLED");
    assert_eq!(restored["runs"][0]["status"], "CANCELLED");
    assert_eq!(restored["attempts"][0]["status"], "CANCELLED");
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_backed_campaign_reaches_accepted_gate_with_source_and_tools() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-provider-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Provider E2E"]);
    git(
        &repository,
        &["config", "user.email", "provider@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    std::fs::create_dir_all(repository.join("src")).unwrap();
    std::fs::write(repository.join("src/seed.txt"), "seed context\n").unwrap();
    std::fs::write(repository.join("src/.env"), "SUPER_SECRET=canary\n").unwrap();
    std::fs::write(
        repository.join("src/config.txt"),
        "API_TOKEN=ordinary-file-secret\nGITHUB_TOKEN=ordinary-github-secret\n",
    )
    .unwrap();
    git(
        &repository,
        &[
            "add",
            "seed.txt",
            "src/seed.txt",
            "src/.env",
            "src/config.txt",
        ],
    );
    git(&repository, &["commit", "-qm", "seed"]);

    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&root.join("factory.sqlite"), port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "provider-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision": 1,
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "Create the provider candidate file.",
            "provider": "opencode",
            "model": "deepseek-v4.1-flash",
            "idempotency_key": "provider-action",
            "allowed_paths": ["src"],
            "checks": [
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"content","kind":"functional","description":"provider content","command":["grep","-q","provider candidate","src/provider.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent content","command":["grep","-q","absent","src/provider.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    seed_v2_grant(
        &root.join("factory.sqlite"),
        &v2_grant(
            "provider-grant",
            &project_id,
            "opencode",
            "deepseek-v4.1-flash",
            true,
            1,
        ),
    )
    .await;
    seed_provider_settings(
        &root.join("factory.sqlite"),
        "opencode",
        "opencode",
        "deepseek-v4.1-flash",
    )
    .await;
    let started = start_request_scoped_provider_campaign(
        &client,
        &base_url,
        &root.join("factory.sqlite"),
        &project_id,
        &action_id,
        &config_hash,
        "provider-grant",
        "opencode",
        "deepseek-v4.1-flash",
        "Create the provider candidate file.",
        "provider",
    )
    .await;
    let campaign_id = started["campaign"]["id"].as_str().unwrap().to_owned();
    use axum::{Router, routing::post};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Upstream {
        calls: Arc<Mutex<usize>>,
        auth: Arc<Mutex<Vec<String>>>,
    }
    async fn chat(
        axum::extract::State(state): axum::extract::State<Upstream>,
        headers: axum::http::HeaderMap,
    ) -> axum::Json<Value> {
        *state.calls.lock().unwrap() += 1;
        state.auth.lock().unwrap().push(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned(),
        );
        axum::Json(json!({
            "id":"chatcmpl-mock","object":"chat.completion","created":1,"model":"deepseek-v4.1-flash",
            "choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}
        }))
    }
    let upstream_calls = Arc::new(Mutex::new(0));
    let upstream_auth = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/chat/completions", post(chat))
        .with_state(Upstream {
            calls: upstream_calls.clone(),
            auth: upstream_auth.clone(),
        });
    let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}", upstream_listener.local_addr().unwrap());
    let upstream_server = tokio::spawn(async move {
        axum::serve(upstream_listener, app).await.unwrap();
    });

    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let provider_session_root = std::env::temp_dir().join(format!(
        "fp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let worker_log = File::create(root.join("provider-worker.log")).unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "provider-worker")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "provider-registration")
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_PROVIDER_SANDBOX", "bwrap")
        .env(
            "FACTORY_PROVIDER_BINARY",
            env!("CARGO_BIN_EXE_factory-fixture-provider"),
        )
        .env("FACTORY_PROVIDER_SESSION_ROOT", &provider_session_root)
        .env("FACTORY_PROVIDER_KEEP_SESSION", "1")
        .env("FACTORY_PROVIDER_UPSTREAM_URL", &upstream_url)
        .env(
            "FACTORY_PROVIDER_UPSTREAM_TOKEN",
            "real-upstream-credential-0123456789",
        )
        .env(
            "FACTORY_PROVIDER_EGRESS_BRIDGE",
            env!("CARGO_BIN_EXE_factory-egress-bridge"),
        )
        .env("FACTORY_ALLOW_TEST_UPSTREAM", "1")
        .env("FACTORY_FIXTURE_PROVIDER_USE_EGRESS", "1")
        .env("FACTORY_WORKER_ONCE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("provider worker");
    assert!(
        worker_status.success(),
        "provider Worker failed: {}",
        std::fs::read_to_string(root.join("provider-worker.log")).unwrap_or_default()
    );

    let deadline = Instant::now() + Duration::from_secs(15);
    let candidates: Value = loop {
        let value: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if value.as_array().is_some_and(|items| items.len() == 1) {
            break value;
        }
        assert!(Instant::now() < deadline, "provider candidate missing");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let candidate = &candidates[0];
    let candidate_id = candidate["id"].as_str().unwrap().to_owned();
    assert_eq!(
        git(
            &repository,
            &[
                "show",
                &format!(
                    "{}:src/provider.txt",
                    candidate["commit_sha"].as_str().unwrap()
                )
            ]
        ),
        "provider candidate"
    );

    let verifier_log = File::create(root.join("provider-verifier.log")).unwrap();
    let verifier_status = Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", &candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", "provider-verifier")
        .env(
            "FACTORY_VERIFIER_WORKSPACE",
            root.join("provider-verifier-work"),
        )
        .env("FACTORY_VERIFIER_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_VERIFIER_TEST_BACKEND", "local")
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join("provider-verifier-volumes"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(verifier_log.try_clone().unwrap()))
        .stderr(Stdio::from(verifier_log))
        .status()
        .expect("provider verifier");
    assert!(
        verifier_status.success(),
        "provider verifier failed: {}",
        std::fs::read_to_string(root.join("provider-verifier.log")).unwrap_or_default()
    );
    run_reviewer(
        &root,
        &base_url,
        &candidate_id,
        "provider-reviewer",
        "provider-reviewer.log",
    );
    let gate: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/gate"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(gate["outcome"], "ACCEPTED");

    let participants: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/office/participants?campaign_id={campaign_id}&role=IMPLEMENTER"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let participants = participants["participants"].as_array().unwrap();
    assert_eq!(participants.len(), 1, "one implementer session expected");
    let implementer = &participants[0];
    assert_eq!(implementer["role"], "IMPLEMENTER");
    assert_eq!(implementer["campaign_id"], campaign_id);
    assert_eq!(implementer["attempt_generation"], 1);
    assert_eq!(implementer["status"], "COMPLETED");
    assert_eq!(implementer["observation"], "FINAL");
    assert_eq!(implementer["session"]["state"], "COMPLETED");
    assert_eq!(implementer["session"]["cleanup"], "RECORDED");
    assert_eq!(implementer["evidence"]["candidate_id"], candidate_id);
    assert_eq!(implementer["evidence"]["gate_outcome"], "ACCEPTED");
    assert!(
        implementer["evidence"]["verifier_receipt_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        implementer["evidence"]["reviewer_receipt_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    let session_record_id = implementer["session"]["session_record_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let isolation: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/office/participants?campaign_id=other-campaign"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(isolation["participants"].as_array().unwrap().len(), 0);

    let tool_events: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/provider/sessions/{session_record_id}/tool_events?limit=2"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(tool_events["returned"], 2);
    assert_eq!(tool_events["has_more"], true);
    assert_eq!(
        tool_events["session"]["participant_id"],
        format!("implementer:{session_record_id}")
    );
    let first_page = tool_events["events"].as_array().unwrap();
    assert!(first_page.iter().all(|event| event["sequence"].is_i64()));
    assert!(
        first_page
            .iter()
            .all(|event| event["call_ref"].as_str().is_some())
    );
    assert!(
        first_page
            .iter()
            .all(|event| event["provenance"] == "provider_tool_events")
    );
    let after = tool_events["next_sequence"].as_i64().unwrap();
    let mut all_events = first_page.clone();
    let mut after_sequence = after;
    for _ in 0..10 {
        let page: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/provider/sessions/{session_record_id}/tool_events?after_sequence={after_sequence}&limit=100"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        let items = page["events"].as_array().unwrap();
        all_events.extend(items.iter().cloned());
        if !page["has_more"].as_bool().unwrap_or(false) {
            break;
        }
        after_sequence = page["next_sequence"].as_i64().unwrap();
    }
    assert!(all_events.iter().any(|event| event["decision"] == "DENY"));
    assert!(all_events.iter().any(|event| event["decision"] == "ALLOW"));
    assert!(all_events.iter().any(|event| event["decision"] == "RESULT"));
    let mut sequences: Vec<i64> = all_events
        .iter()
        .map(|event| event["sequence"].as_i64().unwrap())
        .collect();
    let original = sequences.clone();
    sequences.sort_unstable();
    sequences.dedup();
    assert_eq!(
        sequences, original,
        "tool events must be monotonic without duplicates"
    );
    let serialized = serde_json::to_string(&all_events).unwrap();
    assert!(!serialized.contains("ordinary-file-secret"));
    assert!(!serialized.contains("ordinary-github-secret"));
    assert!(!serialized.contains("SUPER_SECRET"));
    assert!(!serialized.contains("real-upstream-credential"));
    assert!(
        all_events
            .iter()
            .any(|event| event["detail_state"] == "WITHHELD"),
        "free-form provider denial detail must be withheld"
    );
    for bearer in [VERIFIER_TOKEN, REVIEWER_TOKEN] {
        let response = client
            .get(format!(
                "{base_url}/office/participants?campaign_id={campaign_id}"
            ))
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = client
            .get(format!(
                "{base_url}/provider/sessions/{session_record_id}/tool_events"
            ))
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let session_dir = std::fs::read_dir(&provider_session_root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("provider session directory");
    let prompts = std::fs::read_to_string(session_dir.join("prompts.jsonl")).unwrap();
    let prompts: Vec<serde_json::Value> = prompts
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(prompts.len() >= 4, "expected four provider turns");
    let first_prompt = prompts[0]
        .pointer("/parts/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        first_prompt.contains("src/seed.txt") && first_prompt.contains("seed context"),
        "provider prompt must contain bounded source context: {first_prompt}"
    );
    let later_prompt = prompts[1]
        .pointer("/parts/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        later_prompt.contains("tool_result") && later_prompt.contains("call-1"),
        "provider must receive the mediated tool result: {later_prompt}"
    );
    assert!(
        prompts
            .iter()
            .all(|prompt| !prompt.to_string().contains("SUPER_SECRET")
                && !prompt.to_string().contains("ordinary-file-secret")
                && !prompt.to_string().contains("ordinary-github-secret")),
        "credential-shaped source must never enter provider context"
    );
    let environment = std::fs::read_to_string(session_dir.join("environment.txt")).unwrap();
    for forbidden in [
        "FACTORY_PROVIDER_PASSWORD",
        "FACTORY_OPERATOR_TOKEN",
        "FACTORY_SUPERVISOR_TOKEN",
        "FACTORY_WORKER_BOOTSTRAP_TOKEN",
        "FACTORY_CORE_URL",
    ] {
        assert!(
            !environment.contains(forbidden),
            "sandboxed provider environment leaked {forbidden}: {environment}"
        );
    }
    let probe = std::fs::read_to_string(session_dir.join("probe.txt")).unwrap();
    assert!(probe.contains("host_home_exists=false"), "{probe}");
    assert!(probe.contains("podman_socket_exists=false"), "{probe}");
    assert!(probe.contains("network_egress=false"), "{probe}");

    let database = root.join("factory.sqlite");
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&database)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let session: (String, i64, i64, i64, i64, Option<String>) = sqlx::query_as::query_as(
        "SELECT state,total_tokens,cost_micros,usage_known,input_tokens,cleanup_receipt
         FROM provider_sessions WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1)",
    )
    .bind(&candidate_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(session.0, "COMPLETED");
    assert_eq!(session.1, 120);
    assert_eq!(session.2, 400);
    assert_eq!(session.3, 1);
    assert_eq!(session.4, 40);
    assert_eq!(session.5.as_deref(), Some("session_delete_verified_absent"));
    let turns: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as::query_as(
        "SELECT state,input_tokens,output_tokens,total_tokens,cost_micros
         FROM provider_turn_reservations
         WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1)
         ORDER BY created_at",
    )
    .bind(&candidate_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(turns.len(), 4);
    assert!(turns.iter().all(|turn| turn.0 == "SETTLED"));
    assert_eq!(turns.iter().map(|turn| turn.1).sum::<i64>(), 40);
    assert_eq!(turns.iter().map(|turn| turn.2).sum::<i64>(), 80);
    assert_eq!(turns.iter().map(|turn| turn.4).sum::<i64>(), 400);
    assert_eq!(*upstream_calls.lock().unwrap(), 4);
    assert!(
        upstream_auth
            .lock()
            .unwrap()
            .iter()
            .all(|value| value == "Bearer real-upstream-credential-0123456789")
    );
    let events: Vec<(String, String, String)> = sqlx::query_as::query_as(
        "SELECT call_id,tool,decision FROM provider_tool_events
         WHERE session_record_id=(SELECT session_record_id FROM provider_sessions
           WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1))
         ORDER BY sequence",
    )
    .bind(&candidate_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(events.contains(&("call-1".into(), "read_file".into(), "DENY".into())));
    assert!(events.contains(&("call-2".into(), "write_file".into(), "ALLOW".into())));
    assert!(events.contains(&("call-3".into(), "run_command".into(), "ALLOW".into())));
    assert!(events.iter().any(|event| event.2 == "RESULT"));
    pool.close().await;
    let supervisor_log = std::fs::read_to_string(root.join("supervisor.log")).unwrap_or_default();
    assert!(!supervisor_log.contains("fake-provider-password"));
    upstream_server.abort();
    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(&provider_session_root);
    let _ = std::fs::remove_dir_all(root);
}

#[derive(Clone)]
struct CodexMockState {
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Value>>>,
}

fn codex_mock_base(status: &str, output: Value) -> Value {
    json!({
        "id":"resp_mock","object":"response","created_at":1,"status":status,"model":"gpt-6-luna",
        "output":output,"output_text":"",
        "usage": if status == "completed" {
            json!({"input_tokens":10,"output_tokens":5,"total_tokens":15})
        } else {
            Value::Null
        }
    })
}

fn codex_mock_function_events() -> Vec<(&'static str, Value)> {
    let item = json!({
        "type":"function_call","id":"fc_mock","call_id":"call_dynamic","name":"write_file",
        "arguments":"{\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}",
        "status":"in_progress"
    });
    let done = json!({
        "type":"function_call","id":"fc_mock","call_id":"call_dynamic","name":"write_file",
        "arguments":"{\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}",
        "status":"completed"
    });
    let completed = codex_mock_base("completed", json!([done.clone()]));
    vec![
        (
            "response.created",
            json!({"type":"response.created","sequence_number":0,"response":codex_mock_base("in_progress", json!([]))}),
        ),
        (
            "response.output_item.added",
            json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":item}),
        ),
        (
            "response.function_call_arguments.delta",
            json!({"type":"response.function_call_arguments.delta","sequence_number":2,"item_id":"fc_mock","output_index":0,"delta":"{\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}"}),
        ),
        (
            "response.function_call_arguments.done",
            json!({"type":"response.function_call_arguments.done","sequence_number":3,"item_id":"fc_mock","output_index":0,"arguments":"{\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}"}),
        ),
        (
            "response.output_item.done",
            json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":done}),
        ),
        (
            "response.completed",
            json!({"type":"response.completed","sequence_number":5,"response":completed}),
        ),
    ]
}

fn codex_mock_final_events() -> Vec<(&'static str, Value)> {
    let text =
        "{\"type\":\"final\",\"files\":[],\"explanation\":\"real Codex wrote the candidate\"}";
    let item = json!({
        "type":"message","id":"msg_final","status":"completed","role":"assistant",
        "content":[{"type":"output_text","text":text,"annotations":[]}]
    });
    let completed = codex_mock_base("completed", json!([item.clone()]));
    vec![
        (
            "response.created",
            json!({"type":"response.created","sequence_number":0,"response":codex_mock_base("in_progress", json!([]))}),
        ),
        (
            "response.output_item.added",
            json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_final","status":"in_progress","role":"assistant","content":[]}}),
        ),
        (
            "response.output_text.delta",
            json!({"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_final","output_index":0,"content_index":0,"delta":text}),
        ),
        (
            "response.output_text.done",
            json!({"type":"response.output_text.done","sequence_number":3,"item_id":"msg_final","output_index":0,"content_index":0,"text":text}),
        ),
        (
            "response.output_item.done",
            json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":item}),
        ),
        (
            "response.completed",
            json!({"type":"response.completed","sequence_number":5,"response":completed}),
        ),
    ]
}

async fn codex_mock_responses(
    axum::extract::State(state): axum::extract::State<CodexMockState>,
    body: String,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    state.seen.lock().unwrap().push(value);
    let call = state.calls.fetch_add(1, Ordering::SeqCst);
    let events = if call == 0 {
        codex_mock_function_events()
    } else {
        codex_mock_final_events()
    };
    let mut payload = String::new();
    for (event, data) in events {
        payload.push_str("event: ");
        payload.push_str(event);
        payload.push_str("\ndata: ");
        payload.push_str(&data.to_string());
        payload.push_str("\n\n");
    }
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        payload,
    )
        .into_response()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn codex_fixture_runs_a_mediated_tool_turn_and_records_core_accounting() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-codex-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Codex E2E"]);
    git(
        &repository,
        &["config", "user.email", "codex@example.invalid"],
    );
    std::fs::create_dir_all(repository.join("src")).unwrap();
    std::fs::write(repository.join("src/seed.txt"), "seed context\n").unwrap();
    git(&repository, &["add", "src/seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&root.join("factory.sqlite"), port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "codex-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision": 1,
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "Create the Codex provider candidate file.",
            "provider": "codex",
            "model": "gpt-6-luna",
            "idempotency_key": "codex-action",
            "allowed_paths": ["src"],
            "checks": [
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"content","kind":"functional","description":"provider content","command":["grep","-q","provider candidate","src/provider.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent content","command":["grep","-q","absent","src/provider.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    seed_v2_grant(
        &root.join("factory.sqlite"),
        &v2_grant("codex-grant", &project_id, "codex", "gpt-6-luna", true, 1),
    )
    .await;
    seed_provider_settings(&root.join("factory.sqlite"), "codex", "codex", "gpt-6-luna").await;
    start_request_scoped_provider_campaign(
        &client,
        &base_url,
        &root.join("factory.sqlite"),
        &project_id,
        &action_id,
        &config_hash,
        "codex-grant",
        "codex",
        "gpt-6-luna",
        "Create the Codex provider candidate file.",
        "codex",
    )
    .await;
    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let provider_session_root = std::env::temp_dir().join(format!(
        "fc-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let worker_log = File::create(root.join("codex-worker.log")).unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "codex-worker")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "codex-registration")
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_PROVIDER_SANDBOX", "bwrap")
        .env("FACTORY_PROVIDER_KIND", "codex")
        .env(
            "FACTORY_PROVIDER_BINARY",
            env!("CARGO_BIN_EXE_factory-fixture-codex"),
        )
        .env("FACTORY_PROVIDER_SESSION_ROOT", &provider_session_root)
        .env("FACTORY_PROVIDER_UPSTREAM_URL", "http://127.0.0.1:9/v1")
        .env(
            "FACTORY_PROVIDER_UPSTREAM_TOKEN",
            "real-upstream-credential-0123456789",
        )
        .env(
            "FACTORY_PROVIDER_EGRESS_BRIDGE",
            env!("CARGO_BIN_EXE_factory-egress-bridge"),
        )
        .env("FACTORY_ALLOW_TEST_UPSTREAM", "1")
        .env(
            "FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION",
            "1000000",
        )
        .env(
            "FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION",
            "2000000",
        )
        .env("FACTORY_WORKER_ONCE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("codex worker");
    assert!(
        worker_status.success(),
        "Codex Worker failed: {}",
        std::fs::read_to_string(root.join("codex-worker.log")).unwrap_or_default()
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let candidate: Value = loop {
        let value: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = value.as_array().and_then(|items| items.first()) {
            break candidate.clone();
        }
        assert!(Instant::now() < deadline, "Codex candidate missing");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let candidate_id = candidate["id"].as_str().unwrap().to_owned();
    assert_eq!(
        git(
            &repository,
            &[
                "show",
                &format!(
                    "{}:src/provider.txt",
                    candidate["commit_sha"].as_str().unwrap()
                )
            ]
        ),
        "provider candidate"
    );
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(root.join("factory.sqlite"))
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let session: (String, i64, i64, i64, Option<String>) = sqlx::query_as::query_as(
        "SELECT state,total_tokens,cost_micros,usage_known,cleanup_receipt
         FROM provider_sessions WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1)",
    )
    .bind(&candidate_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(session.0, "COMPLETED");
    assert_eq!(session.1, 100);
    assert_eq!(session.2, 140);
    assert_eq!(session.3, 1);
    assert_ne!(session.4.as_deref(), Some("session_delete_verified_absent"));
    assert!(
        session
            .4
            .as_deref()
            .is_some_and(|receipt| receipt == "sandbox_process_identity_absent"
                || receipt == "sandbox_process_terminated_verified")
    );
    let events: Vec<(String, String)> = sqlx::query_as::query_as(
        "SELECT call_id,decision FROM provider_tool_events
         WHERE session_record_id=(SELECT session_record_id FROM provider_sessions
           WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1))
         ORDER BY sequence",
    )
    .bind(&candidate_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        events
            .iter()
            .any(|(call, decision)| call == "call-100" && decision == "REQUEST")
    );
    assert!(
        events
            .iter()
            .any(|(call, decision)| call == "call-100" && decision == "ALLOW")
    );
    assert!(
        events
            .iter()
            .any(|(call, decision)| call == "call-100" && decision == "RESULT")
    );
    pool.close().await;
    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(&provider_session_root);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn codex_real_executable_reaches_a_candidate_and_binds_upstream_requests() {
    let Some(codex) = std::env::var_os("CODEX_BIN")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!("NOT_TESTED: CODEX_BIN is unavailable");
        return;
    };
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-codex-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Codex E2E"]);
    git(
        &repository,
        &["config", "user.email", "codex@example.invalid"],
    );
    std::fs::create_dir_all(repository.join("src")).unwrap();
    std::fs::write(repository.join("src/seed.txt"), "seed context\n").unwrap();
    git(&repository, &["add", "src/seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&root.join("factory.sqlite"), port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;

    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "codex-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision": 1,
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "Create the Codex provider candidate file.",
            "provider": "codex",
            "model": "gpt-6-luna",
            "idempotency_key": "codex-action",
            "allowed_paths": ["src"],
            "checks": [
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"content","kind":"functional","description":"provider content","command":["grep","-q","provider candidate","src/provider.txt"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent content","command":["grep","-q","absent","src/provider.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    seed_v2_grant(
        &root.join("factory.sqlite"),
        &v2_grant("codex-grant", &project_id, "codex", "gpt-6-luna", true, 1),
    )
    .await;
    seed_provider_settings(&root.join("factory.sqlite"), "codex", "codex", "gpt-6-luna").await;
    start_request_scoped_provider_campaign(
        &client,
        &base_url,
        &root.join("factory.sqlite"),
        &project_id,
        &action_id,
        &config_hash,
        "codex-grant",
        "codex",
        "gpt-6-luna",
        "Create the Codex provider candidate file.",
        "codex-real",
    )
    .await;
    let mock_state = CodexMockState {
        calls: Arc::new(AtomicUsize::new(0)),
        seen: Arc::new(Mutex::new(Vec::new())),
    };
    let mock_app = axum::Router::new()
        .route("/v1/responses", axum::routing::post(codex_mock_responses))
        .with_state(mock_state.clone());
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_url = format!("http://{}", mock_listener.local_addr().unwrap());
    let mock_server = tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });
    let supervisor_port = free_port();
    let supervisor_url = format!("http://127.0.0.1:{supervisor_port}");
    let mut supervisor = SupervisorProcess::start(SupervisorSpec {
        port: supervisor_port,
        repository: &repository,
        log: &root.join("supervisor.log"),
        workspace_root: &root.join("supervisor-work"),
        state_root: &root.join("supervisor-state"),
        volume_root: &root.join("supervisor-volumes"),
        image: FIXTURE_IMAGE,
        local_backend: true,
        core_url: &base_url,
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

    let provider_session_root = std::env::temp_dir().join(format!(
        "fc-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let worker_log = File::create(root.join("codex-worker.log")).unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "codex-worker")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "codex-registration")
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_PROVIDER_SANDBOX", "bwrap")
        .env("FACTORY_PROVIDER_KIND", "codex")
        .env("FACTORY_PROVIDER_BINARY", &codex)
        .env("FACTORY_PROVIDER_SESSION_ROOT", &provider_session_root)
        .env("FACTORY_PROVIDER_UPSTREAM_URL", &mock_url)
        .env("FACTORY_CODEX_REQUIRE_MODEL_LIST", "0")
        .env(
            "FACTORY_PROVIDER_UPSTREAM_TOKEN",
            "real-upstream-credential-0123456789",
        )
        .env(
            "FACTORY_PROVIDER_EGRESS_BRIDGE",
            env!("CARGO_BIN_EXE_factory-egress-bridge"),
        )
        .env("FACTORY_ALLOW_TEST_UPSTREAM", "1")
        .env(
            "FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION",
            "1000000",
        )
        .env(
            "FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION",
            "2000000",
        )
        .env("FACTORY_WORKER_ONCE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("codex worker");
    assert!(
        worker_status.success(),
        "Codex Worker failed: {}",
        std::fs::read_to_string(root.join("codex-worker.log")).unwrap_or_default()
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let candidate: Value = loop {
        let value: Value = operator(
            &client,
            reqwest::Method::GET,
            format!("{base_url}/candidates"),
            None,
        )
        .await
        .json()
        .await
        .unwrap();
        if let Some(candidate) = value.as_array().and_then(|items| items.first()) {
            break candidate.clone();
        }
        assert!(Instant::now() < deadline, "Codex candidate missing");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let candidate_id = candidate["id"].as_str().unwrap().to_owned();
    assert_eq!(
        git(
            &repository,
            &[
                "show",
                &format!(
                    "{}:src/provider.txt",
                    candidate["commit_sha"].as_str().unwrap()
                )
            ]
        ),
        "provider candidate"
    );
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(root.join("factory.sqlite"))
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let session: (String, i64, i64, i64, Option<String>) = sqlx::query_as::query_as(
        "SELECT state,total_tokens,cost_micros,usage_known,cleanup_receipt
         FROM provider_sessions WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1)",
    )
    .bind(&candidate_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(session.0, "COMPLETED");
    assert!(
        session.1 >= 15,
        "Codex token usage was not recorded: {session:?}"
    );
    assert!(
        session.2 > 0,
        "Codex cost was not calculated from configured rates"
    );
    assert_eq!(session.3, 1);
    assert_ne!(session.4.as_deref(), Some("session_delete_verified_absent"));
    assert!(
        session
            .4
            .as_deref()
            .is_some_and(|receipt| receipt == "sandbox_process_identity_absent"
                || receipt == "sandbox_process_terminated_verified")
    );
    let events: Vec<(String, String)> = sqlx::query_as::query_as(
        "SELECT call_id,decision FROM provider_tool_events
         WHERE session_record_id=(SELECT session_record_id FROM provider_sessions
           WHERE attempt_id=(SELECT attempt_id FROM candidates WHERE id=?1))
         ORDER BY sequence",
    )
    .bind(&candidate_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        events
            .iter()
            .any(|(call, decision)| call == "call_dynamic" && decision == "REQUEST")
    );
    assert!(
        events
            .iter()
            .any(|(call, decision)| call == "call_dynamic" && decision == "ALLOW")
    );
    assert!(
        events
            .iter()
            .any(|(call, decision)| call == "call_dynamic" && decision == "RESULT")
    );
    let egress_requests: (i64,) = sqlx::query_as::query_as(
        "SELECT COUNT(*) FROM provider_turn_egress_requests WHERE reservation_id IN (
           SELECT reservation_id FROM provider_turn_reservations WHERE attempt_id=(
             SELECT attempt_id FROM candidates WHERE id=?1))",
    )
    .bind(&candidate_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(egress_requests.0, 2);
    assert_eq!(mock_state.calls.load(Ordering::SeqCst), 2);
    {
        let seen = mock_state.seen.lock().unwrap();
        assert_eq!(seen[0]["model"], "gpt-6-luna");
        let tools = seen[0]["tools"]
            .as_array()
            .expect("Factory must inject the upstream tool allowlist");
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["write_file", "run_command"]);
        assert!(
            seen[0]["input"].as_array().is_none_or(|items| items
                .iter()
                .all(|item| item.get("type").and_then(Value::as_str) != Some("additional_tools"))),
            "Codex additional_tools must not reach upstream"
        );
    }
    pool.close().await;
    mock_server.abort();
    supervisor.stop();
    core.stop();
    let _ = std::fs::remove_dir_all(&provider_session_root);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovery_worker_cleans_an_orphaned_provider_session_after_restart() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use axum::{
        Router,
        extract::State,
        response::{IntoResponse, Response},
    };

    #[derive(Clone)]
    struct CleanupProvider {
        deleted: Arc<AtomicBool>,
    }

    async fn cleanup_delete(State(state): State<CleanupProvider>) -> axum::Json<bool> {
        state.deleted.store(true, Ordering::SeqCst);
        axum::Json(true)
    }

    async fn cleanup_get(State(state): State<CleanupProvider>) -> Response {
        if state.deleted.load(Ordering::SeqCst) {
            reqwest::StatusCode::NOT_FOUND.into_response()
        } else {
            axum::Json(json!({"id":"ses_recovery"})).into_response()
        }
    }

    let root = std::env::temp_dir().join(format!(
        "factory-e2e-provider-recovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "Recovery E2E"]);
    git(
        &repository,
        &["config", "user.email", "recovery@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let provider = CleanupProvider {
        deleted: Arc::new(AtomicBool::new(false)),
    };
    let app = Router::new()
        .route(
            "/session/{id}",
            axum::routing::delete(cleanup_delete).get(cleanup_get),
        )
        .with_state(provider);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_url = format!("http://{}", listener.local_addr().unwrap());
    let provider_server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let provider_password = "recovery-provider-password-123456";

    let database = root.join("factory.sqlite");
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&database, port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;
    let project: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository":repository,"idempotency_key":"recovery-project"})),
    )
    .await
    .json()
    .await
    .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    let config: Value = operator(
        &client,
        reqwest::Method::PUT,
        format!("{base_url}/projects/{project_id}/config"),
        Some(json!({
            "expectedRevision": 1,
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "qualityProfile": "generic",
            "active": true
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let config_hash = config["configHash"].as_str().unwrap().to_owned();
    let action: Value = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id":project_id,
            "objective":"Recovery provider action.",
            "provider":"opencode",
            "model":"deepseek-v4.1-flash",
            "idempotency_key":"recovery-action",
            "allowed_paths":["src"],
            "checks":[
                {"id":"build","kind":"build","description":"build","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"test","kind":"test","description":"test","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"content","kind":"functional","description":"content","command":["true"],"expectExit":0,"timeoutSecs":30},
                {"id":"negative","kind":"negative","description":"absent content","command":["grep","-q","absent","seed.txt"],"expectExit":1,"timeoutSecs":30}
            ]
        })),
    )
    .await
    .json()
    .await
    .unwrap();
    let action_id = action["id"].as_str().unwrap().to_owned();
    seed_v2_grant(
        &database,
        &v2_grant(
            "recovery-grant",
            &project_id,
            "opencode",
            "deepseek-v4.1-flash",
            true,
            1,
        ),
    )
    .await;
    seed_provider_settings(&database, "opencode", "opencode", "deepseek-v4.1-flash").await;
    let started = start_request_scoped_provider_campaign(
        &client,
        &base_url,
        &database,
        &project_id,
        &action_id,
        &config_hash,
        "recovery-grant",
        "opencode",
        "deepseek-v4.1-flash",
        "Recovery provider action.",
        "recovery",
    )
    .await;
    let attempt_id = started["attempt"]["id"].as_str().unwrap().to_owned();
    core.stop();

    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO provider_sessions
         (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,state,
          provider_session_id,provider_directory,created_at,updated_at)
         VALUES ('recovery-session',?1,'recovery-worker',1,'opencode','deepseek-v4.1-flash',
                 'build','ACTIVE','ses_recovery','/tmp/recovery-provider',?2,?2)",
    )
    .bind(&attempt_id)
    .bind(&now)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    core = CoreProcess::start(&database, port, &root.join("core.log"));
    wait_healthy(&client, &base_url, &mut core).await;
    let registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({"worker_id":"recovery-worker","registration_key":"recovery-registration"}))
        .send()
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::OK);
    let session: Value = registration.json().await.unwrap();
    let blocked = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(session["session_token"].as_str().unwrap())
        .json(&json!({"worker_id":"recovery-worker"}))
        .send()
        .await
        .unwrap();
    assert!(
        blocked.status().is_client_error(),
        "unresolved provider session must block dispatch"
    );

    let worker_log = File::create(root.join("recovery-worker.log")).unwrap();
    let recovery_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "recovery-worker")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "recovery-registration")
        .env("FACTORY_SUPERVISOR_URL", "http://127.0.0.1:1")
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_PROVIDER_SANDBOX", "unconfined")
        .env("FACTORY_ALLOW_UNCONFINED_PROVIDER", "1")
        .env("FACTORY_PROVIDER_BASE_URL", &provider_url)
        .env("FACTORY_PROVIDER_PASSWORD", provider_password)
        .env("FACTORY_WORKER_RECOVER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(worker_log.try_clone().unwrap()))
        .stderr(Stdio::from(worker_log))
        .status()
        .expect("recovery worker");
    assert!(
        recovery_status.success(),
        "recovery Worker failed: {}",
        std::fs::read_to_string(root.join("recovery-worker.log")).unwrap_or_default()
    );
    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(&database)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let (state, cleanup): (String, Option<String>) = sqlx::query_as::query_as(
        "SELECT state,cleanup_receipt FROM provider_sessions WHERE session_record_id='recovery-session'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, "CLEANED");
    assert_eq!(cleanup.as_deref(), Some("session_delete_verified_absent"));
    pool.close().await;
    provider_server.abort();
    core.stop();
    let _ = std::fs::remove_dir_all(root);
}
