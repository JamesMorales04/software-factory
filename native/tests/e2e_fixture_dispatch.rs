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

const OPERATOR_TOKEN: &str = "operator-token-e2e-dispatch";
const BOOTSTRAP_TOKEN: &str = "bootstrap-token-123456789012345678901234";
const VERIFIER_TOKEN: &str = "verifier-token-1234567890123456789012";
const REVIEWER_TOKEN: &str = "reviewer-token-1234567890123456789012";
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
}

impl SupervisorProcess {
    fn start(spec: SupervisorSpec<'_>) -> Self {
        let file = File::create(spec.log).expect("supervisor log");
        let mut command = Command::new(env!("CARGO_BIN_EXE_factory-supervisor"));
        command
            .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
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
        Some(json!({
            "authorization_id": "auth-e2e",
            "risk_class": "R0",
            "execution_mode": "FIXTURE",
            "actor": "e2e-operator",
            "rationale": "deterministic fixture E2E",
            "authorized_paths": ["src"],
            "max_attempts": 1
        })),
    )
    .await;
    assert_eq!(authorized.status(), StatusCode::CREATED);
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
    let reviewer = client
        .post(format!("{base_url}/quality/receipts"))
        .bearer_auth(REVIEWER_TOKEN)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": "reviewer-e2e",
            "principal_id": "reviewer-e2e",
            "actionSpecSha256": spec_sha,
            "commitSha": commit,
            "treeSha": tree,
            "result": "PASS",
            "evidence": "EXECUTED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reviewer.status(), StatusCode::CREATED);
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

#[tokio::test]
async fn lead_fixture_start_dispatches_through_supervisor_to_accepted_gate() {
    use factory_native::lead::{
        Capability, ObjectiveClass, ProviderRoute, RiskClass, StandingDelegation,
    };

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
                    "description": "fixture content exists",
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

    let mut grant = StandingDelegation {
        id: "lead-grant".into(),
        principal_id: "owner".into(),
        audit_id: "audit".into(),
        project_id: project_id.clone(),
        revision: 1,
        content_hash: String::new(),
        objective_classes: vec![ObjectiveClass::Feature],
        allowed_paths: vec!["src".into()],
        capabilities: vec![Capability::RepositoryEdit, Capability::GenericToolchain],
        risk_classes: vec![RiskClass::R0, RiskClass::R1],
        autonomous_start: true,
        autonomous_risk_classes: vec![RiskClass::R0],
        route: ProviderRoute {
            provider: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            mode: "build".into(),
        },
        max_attempts: 2,
        max_timeout_secs: 300,
        max_repair_cycles: 0,
        max_wall_clock_secs: 1_800,
        max_parallel: 1,
        max_sessions: 4,
        max_requests: 2,
        max_cumulative_sessions: 8,
        max_cumulative_attempts: 16,
        expires_at: chrono::Utc::now() + chrono::Duration::days(1),
        revoked_at: None,
    };
    grant.content_hash = grant.compute_content_hash();
    let delegated = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/delegations"),
        Some(json!({"delegation": grant, "idempotency_key": "lead-grant-key"})),
    )
    .await;
    assert_eq!(delegated.status(), StatusCode::CREATED);

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
            "idempotency_key": "lead-request"
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
    let frozen = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/lead/requests/{request_id}/freeze"),
        None,
    )
    .await;
    assert_eq!(frozen.status(), StatusCode::OK);
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
    let attempt_id = started["attempt"]["id"].as_str().unwrap().to_owned();
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
    });
    wait_supervisor(&client, &supervisor_url, &mut supervisor).await;

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
    let reviewer = client
        .post(format!("{base_url}/quality/receipts"))
        .bearer_auth(REVIEWER_TOKEN)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": "lead-reviewer",
            "principal_id": "lead-reviewer",
            "actionSpecSha256": spec_sha,
            "commitSha": candidate["commit_sha"],
            "treeSha": candidate["tree_sha"],
            "result": "PASS",
            "evidence": "EXECUTED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reviewer.status(), StatusCode::CREATED);
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

    let mut grant = factory_native::lead::StandingDelegation {
        id: "integration-grant".into(),
        principal_id: "owner".into(),
        audit_id: "audit".into(),
        project_id: project_id.clone(),
        revision: 1,
        content_hash: String::new(),
        objective_classes: vec![factory_native::lead::ObjectiveClass::Feature],
        allowed_paths: vec!["src".into()],
        capabilities: vec![
            factory_native::lead::Capability::RepositoryEdit,
            factory_native::lead::Capability::GenericToolchain,
        ],
        risk_classes: vec![factory_native::lead::RiskClass::R0],
        autonomous_start: true,
        autonomous_risk_classes: vec![factory_native::lead::RiskClass::R0],
        route: factory_native::lead::ProviderRoute {
            provider: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            mode: "build".into(),
        },
        max_attempts: 2,
        max_timeout_secs: 300,
        max_repair_cycles: 0,
        max_wall_clock_secs: 1800,
        max_parallel: 2,
        max_sessions: 8,
        max_requests: 2,
        max_cumulative_sessions: 16,
        max_cumulative_attempts: 32,
        expires_at: chrono::Utc::now() + chrono::Duration::days(1),
        revoked_at: None,
    };
    grant.content_hash = grant.compute_content_hash();
    let delegated = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/delegations"),
        Some(json!({"delegation": grant, "idempotency_key": "integration-grant-key"})),
    )
    .await;
    assert_eq!(delegated.status(), StatusCode::CREATED);

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
    let spec_sha = integrated["action_spec_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
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
    let reviewer = client
        .post(format!("{base_url}/quality/receipts"))
        .bearer_auth(REVIEWER_TOKEN)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": "integration-reviewer",
            "principal_id": "integration-reviewer",
            "actionSpecSha256": spec_sha,
            "commitSha": integrated["commit_sha"],
            "treeSha": integrated["tree_sha"],
            "result": "PASS",
            "evidence": "EXECUTED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reviewer.status(), StatusCode::CREATED);
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
