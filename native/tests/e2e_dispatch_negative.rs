//! Deterministic negative end-to-end boundaries.
//!
//! Starts the real Core, and where relevant the real Worker or Verifier, against
//! disposable SQLite databases and Git repositories. Each case asserts an
//! observable HTTP or process outcome. No provider session is involved.

use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde_json::{Value, json};

const OPERATOR_TOKEN: &str = "operator-token-e2e-negative";
const BOOTSTRAP_TOKEN: &str = "bootstrap-token-123456789012345678901234";
const VERIFIER_TOKEN: &str = "verifier-token-1234567890123456789012";
const REVIEWER_TOKEN: &str = "reviewer-token-1234567890123456789012";
const ATTEMPT_GRANT_KEY: &str = "attempt-grant-key-0123456789abcdef";
const SUPERVISOR_CAPABILITY_KEY: &str = "capability-key-0123456789abcdef0";
const SUPERVISOR_TOKEN: &str = "supervisor-token-negative-1234567890123456";
const FIXTURE_IMAGE: &str = "test/image:1";

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(prefix: &str) -> Self {
        let process = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("factory-e2e-{prefix}-{process}-{nanos}"));
        std::fs::create_dir_all(&path).expect("test root");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct CoreProcess {
    child: Child,
    log: PathBuf,
}

impl CoreProcess {
    fn start(database: &Path, port: u16, log: &Path) -> Self {
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
}

impl SupervisorProcess {
    fn start(root: &Path, repository: &Path, core_url: &str) -> (Self, String) {
        let port = free_port();
        let log = root.join("supervisor.log");
        let file = File::create(&log).expect("supervisor log");
        let child = Command::new(env!("CARGO_BIN_EXE_factory-supervisor"))
            .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
            .env("FACTORY_CORE_URL", core_url)
            .env("FACTORY_ATTEMPT_GRANT_KEY", ATTEMPT_GRANT_KEY)
            .env(
                "FACTORY_SUPERVISOR_CAPABILITY_KEY",
                SUPERVISOR_CAPABILITY_KEY,
            )
            .env("FACTORY_SUPERVISOR_BIND", format!("127.0.0.1:{port}"))
            .env(
                "FACTORY_SUPERVISOR_WORKSPACE_ROOT",
                root.join("supervisor-work"),
            )
            .env(
                "FACTORY_SUPERVISOR_STATE_ROOT",
                root.join("supervisor-state"),
            )
            .env("FACTORY_SUPERVISOR_PROJECT_ROOTS", repository)
            .env("FACTORY_SUPERVISOR_IMAGES", FIXTURE_IMAGE)
            .env("FACTORY_SUPERVISOR_TEST_BACKEND", "local")
            .env(
                "FACTORY_SUPERVISOR_TEST_VOLUME_ROOT",
                root.join("supervisor-volumes"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(file))
            .spawn()
            .expect("spawn factory-supervisor");
        let url = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                use std::io::{Read, Write};
                let _ = stream.write_all(
                    b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                );
                let mut response = String::new();
                let _ = stream.read_to_string(&mut response);
                if response.starts_with("HTTP/1.1 200") {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "supervisor did not become healthy"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        (Self { child }, url)
    }
}

impl Drop for SupervisorProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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

async fn start_core(root: &Path) -> (CoreProcess, reqwest::Client, String) {
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let database = root.join("factory.sqlite");
    let mut core = CoreProcess::start(&database, port, &root.join("core.log"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    wait_healthy(&client, &base_url, &mut core).await;
    (core, client, base_url)
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

fn create_repository(root: &Path) -> PathBuf {
    let repository = root.join("repo");
    std::fs::create_dir(&repository).expect("repo");
    git(&repository, &["init", "-q"]);
    git(&repository, &["config", "user.name", "E2E Negative"]);
    git(
        &repository,
        &["config", "user.email", "e2e-negative@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").expect("seed");
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);
    repository
}

async fn create_project(
    client: &reqwest::Client,
    base_url: &str,
    repository: &Path,
    key: &str,
) -> String {
    let response = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/projects"),
        Some(json!({"repository": repository, "idempotency_key": key})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let project: Value = response.json().await.unwrap();
    project["id"].as_str().unwrap().to_owned()
}

async fn create_action(
    client: &reqwest::Client,
    base_url: &str,
    project_id: &str,
    key: &str,
    allowed_paths: &[&str],
    checks: Value,
) -> Value {
    let response = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "negative boundary",
            "provider": "opencode",
            "model": "test/model",
            "idempotency_key": key,
            "allowed_paths": allowed_paths,
            "checks": checks
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response.json().await.unwrap()
}

async fn create_campaign(
    client: &reqwest::Client,
    base_url: &str,
    action_id: &str,
    key: &str,
) -> Value {
    let response = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns"),
        Some(json!({"action_id": action_id, "idempotency_key": key})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    response.json().await.unwrap()
}

async fn authorize_campaign(
    client: &reqwest::Client,
    base_url: &str,
    campaign_id: &str,
    authorization_id: &str,
    allowed_paths: &[&str],
) {
    let response = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/authorize"),
        Some(json!({
            "authorization_id": authorization_id,
            "risk_class": "R0",
            "execution_mode": "FIXTURE",
            "actor": "e2e-negative-operator",
            "rationale": "deterministic negative E2E",
            "authorized_paths": allowed_paths,
            "max_attempts": 1
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}

async fn create_run(client: &reqwest::Client, base_url: &str, campaign_id: &str) -> String {
    let response = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/runs"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let run: Value = response.json().await.unwrap();
    run["id"].as_str().unwrap().to_owned()
}

async fn create_attempt(client: &reqwest::Client, base_url: &str, run_id: &str) -> String {
    let response = operator(
        client,
        reqwest::Method::POST,
        format!("{base_url}/runs/{run_id}/attempts"),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let attempt: Value = response.json().await.unwrap();
    attempt["id"].as_str().unwrap().to_owned()
}

struct AttemptFixture {
    run_id: String,
    attempt_id: String,
    action_spec_sha256: String,
}

async fn authorize_and_start_attempt(
    client: &reqwest::Client,
    base_url: &str,
    action: &Value,
    key: &str,
    allowed_paths: &[&str],
) -> AttemptFixture {
    let action_id = action["id"].as_str().unwrap();
    let action_spec_sha256 = action["spec_sha256"].as_str().unwrap().to_owned();
    let campaign = create_campaign(client, base_url, action_id, &format!("{key}-campaign")).await;
    let campaign_id = campaign["id"].as_str().unwrap();
    authorize_campaign(
        client,
        base_url,
        campaign_id,
        &format!("{key}-authorization"),
        allowed_paths,
    )
    .await;
    let run_id = create_run(client, base_url, campaign_id).await;
    let attempt_id = create_attempt(client, base_url, &run_id).await;
    AttemptFixture {
        run_id,
        attempt_id,
        action_spec_sha256,
    }
}

fn run_worker_once(
    root: &Path,
    base_url: &str,
    supervisor_url: &str,
    worker_id: &str,
    fixture_file: &str,
    fixture_content: &str,
) -> ExitStatus {
    let log_path = root.join(format!("worker-{worker_id}.log"));
    let log = File::create(&log_path).expect("worker log");
    Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", worker_id)
        .env(
            "FACTORY_WORKER_REGISTRATION_KEY",
            format!("registration-{worker_id}"),
        )
        .env("FACTORY_SUPERVISOR_URL", supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", SUPERVISOR_TOKEN)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_WORKER_ONCE", "1")
        .env("FACTORY_FIXTURE_FILE", fixture_file)
        .env("FACTORY_FIXTURE_CONTENT", fixture_content)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .status()
        .expect("worker")
}

fn run_verifier(root: &Path, base_url: &str, candidate_id: &str, principal: &str) -> ExitStatus {
    let log_path = root.join(format!("verifier-{principal}.log"));
    let log = File::create(&log_path).expect("verifier log");
    Command::new(env!("CARGO_BIN_EXE_factory-verifier"))
        .env("FACTORY_CORE_URL", base_url)
        .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
        .env("FACTORY_CANDIDATE_ID", candidate_id)
        .env("FACTORY_VERIFIER_PRINCIPAL", principal)
        .env(
            "FACTORY_VERIFIER_WORKSPACE",
            root.join(format!("verifier-workspaces-{principal}")),
        )
        .env("FACTORY_VERIFIER_IMAGE", FIXTURE_IMAGE)
        .env("FACTORY_VERIFIER_TEST_BACKEND", "local")
        .env(
            "FACTORY_VERIFIER_TEST_VOLUME_ROOT",
            root.join(format!("verifier-volumes-{principal}")),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .status()
        .expect("verifier")
}

#[tokio::test]
async fn campaign_detail_excludes_foreign_candidates_gates_receipts_and_events() {
    let root = TestRoot::new("negative-campaign-projection");
    let repository = create_repository(root.path());
    let (mut core, client, base_url) = start_core(root.path()).await;
    let project_id = create_project(&client, &base_url, &repository, "projection-project").await;
    let action_a = create_action(
        &client,
        &base_url,
        &project_id,
        "projection-action-a",
        &["src"],
        json!([]),
    )
    .await;
    let action_b = create_action(
        &client,
        &base_url,
        &project_id,
        "projection-action-b",
        &["src"],
        json!([]),
    )
    .await;
    let campaign_a = create_campaign(
        &client,
        &base_url,
        action_a["id"].as_str().unwrap(),
        "projection-campaign-a",
    )
    .await;
    let campaign_b = create_campaign(
        &client,
        &base_url,
        action_b["id"].as_str().unwrap(),
        "projection-campaign-b",
    )
    .await;
    let campaign_a_id = campaign_a["id"].as_str().unwrap().to_owned();
    let campaign_b_id = campaign_b["id"].as_str().unwrap().to_owned();
    authorize_campaign(
        &client,
        &base_url,
        &campaign_a_id,
        "projection-authorization-a",
        &["src"],
    )
    .await;
    authorize_campaign(
        &client,
        &base_url,
        &campaign_b_id,
        "projection-authorization-b",
        &["src"],
    )
    .await;
    let run_a = create_run(&client, &base_url, &campaign_a_id).await;
    let attempt_a = create_attempt(&client, &base_url, &run_a).await;
    let run_b = create_run(&client, &base_url, &campaign_b_id).await;
    let attempt_b = create_attempt(&client, &base_url, &run_b).await;

    let pool = sqlx_sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx_sqlite::SqliteConnectOptions::new()
                .filename(root.path().join("factory.sqlite"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
    let now = "2026-01-01T00:00:00Z";
    sqlx::query::query(
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
         VALUES ('delegation-shared','owner','audit',?1,1,'hash','{}',?2)",
    )
    .bind(&project_id)
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    let seed_rows = [
        (
            "a",
            action_a["id"].as_str().unwrap(),
            action_a["spec_sha256"].as_str().unwrap(),
            campaign_a_id.as_str(),
            run_a.as_str(),
            attempt_a.as_str(),
        ),
        (
            "b",
            action_b["id"].as_str().unwrap(),
            action_b["spec_sha256"].as_str().unwrap(),
            campaign_b_id.as_str(),
            run_b.as_str(),
            attempt_b.as_str(),
        ),
    ];
    for (suffix, action_id, spec_sha256, campaign_id, run_id, attempt_id) in seed_rows {
        let request_id = format!("request-{suffix}");
        let task_id = format!("task-{suffix}");
        let candidate_id = format!("candidate-{suffix}");
        let commit_sha = suffix.repeat(40);
        let tree_sha = suffix.to_uppercase().repeat(40);
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,
              state,campaign_id,created_at,updated_at)
             VALUES (?1,?2,?3,'delegation-shared','projection objective','request-hash',
                     'config-hash','RUNNING',?4,?5,?5)",
        )
        .bind(&request_id)
        .bind(&project_id)
        .bind(action_id)
        .bind(campaign_id)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_tasks
             (task_id,campaign_id,request_id,unit_id,sequence,status,base_commit,spec_json,
              spec_sha256,depends_on_json,run_id,attempt_id,candidate_id,created_at,updated_at)
             VALUES (?1,?2,?3,?4,0,'RUNNING','base','{}',?5,'[]',?6,?7,?8,?9,?9)",
        )
        .bind(&task_id)
        .bind(campaign_id)
        .bind(&request_id)
        .bind(format!("unit-{suffix}"))
        .bind(spec_sha256)
        .bind(run_id)
        .bind(attempt_id)
        .bind(&candidate_id)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO candidates
             (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
             VALUES (?1,?2,?3,?4,?5,'worker',?6)",
        )
        .bind(&candidate_id)
        .bind(attempt_id)
        .bind(spec_sha256)
        .bind(&commit_sha)
        .bind(&tree_sha)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO quality_receipts
             (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
              result,evidence,received_at)
             VALUES (?1,?2,'verifier',?3,?4,?5,?6,'PASS','EXECUTED',?7)",
        )
        .bind(format!("receipt-{suffix}"))
        .bind(&candidate_id)
        .bind(format!("verifier-{suffix}"))
        .bind(spec_sha256)
        .bind(&commit_sha)
        .bind(&tree_sha)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query::query("INSERT INTO gate_decisions VALUES (?1,'ACCEPTED','[]',?2)")
            .bind(&candidate_id)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        for (subject_id, kind) in [
            (campaign_id, "campaign.created"),
            (request_id.as_str(), "lead.request_created"),
            (task_id.as_str(), "lead.plan_completed"),
            (run_id, "run.status_changed"),
            (attempt_id, "worker.claimed"),
            (candidate_id.as_str(), "quality.gate_decided"),
            (candidate_id.as_str(), "quality.receipt_recorded"),
            ("delegation-shared", "delegation.released"),
        ] {
            sqlx::query::query(
                "INSERT INTO events(subject_id,kind,detail,recorded_at) VALUES (?1,?2,'',?3)",
            )
            .bind(subject_id)
            .bind(kind)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        }
    }
    for (subject_id, kind) in [
        ("lead-unrelated", "lead.request_created"),
        ("delegation-foreign", "delegation.released"),
    ] {
        sqlx::query::query(
            "INSERT INTO events(subject_id,kind,detail,recorded_at) VALUES (?1,?2,'',?3)",
        )
        .bind(subject_id)
        .bind(kind)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;

    let detail_a: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns/{campaign_a_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let candidates = detail_a["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(candidates, vec!["candidate-a"]);
    let subjects = detail_a["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["subject_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        campaign_a_id.as_str(),
        "request-a",
        "task-a",
        run_a.as_str(),
        attempt_a.as_str(),
        "candidate-a",
        "delegation-shared",
    ] {
        assert!(
            subjects.contains(&expected),
            "missing {expected}: {subjects:?}"
        );
    }
    for foreign in [
        campaign_b_id.as_str(),
        "request-b",
        "task-b",
        run_b.as_str(),
        attempt_b.as_str(),
        "candidate-b",
        "lead-unrelated",
        "delegation-foreign",
    ] {
        assert!(!subjects.contains(&foreign), "leaked {foreign}");
    }

    let detail_b: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/campaigns/{campaign_b_id}"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let candidates = detail_b["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(candidates, vec!["candidate-b"]);
    let subjects = detail_b["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["subject_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    for expected in [
        campaign_b_id.as_str(),
        "request-b",
        "task-b",
        run_b.as_str(),
        attempt_b.as_str(),
        "candidate-b",
        "delegation-shared",
    ] {
        assert!(
            subjects.contains(&expected),
            "missing {expected}: {subjects:?}"
        );
    }
    for foreign in [
        campaign_a_id.as_str(),
        "request-a",
        "task-a",
        run_a.as_str(),
        attempt_a.as_str(),
        "candidate-a",
        "lead-unrelated",
        "delegation-foreign",
    ] {
        assert!(!subjects.contains(&foreign), "leaked {foreign}");
    }

    core.stop();
    drop(core);
    drop(root);
}

#[tokio::test]
async fn candidate_outside_frozen_allowed_paths_is_rejected_without_record() {
    let root = TestRoot::new("negative-scope");
    let repository = create_repository(root.path());
    let (mut core, client, base_url) = start_core(root.path()).await;
    let project_id = create_project(&client, &base_url, &repository, "scope-project").await;
    let action = create_action(
        &client,
        &base_url,
        &project_id,
        "scope-action",
        &["src"],
        json!([]),
    )
    .await;
    let fixture = authorize_and_start_attempt(&client, &base_url, &action, "scope", &["src"]).await;
    let (_supervisor, supervisor_url) =
        SupervisorProcess::start(root.path(), &repository, &base_url);

    let status = run_worker_once(
        root.path(),
        &base_url,
        &supervisor_url,
        "worker-scope",
        "outside.txt",
        "outside frozen scope\n",
    );
    let log =
        std::fs::read_to_string(root.path().join("worker-worker-scope.log")).unwrap_or_default();
    assert!(
        !status.success(),
        "Worker unexpectedly succeeded; log:\n{log}"
    );

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
    let attempts = attempts.as_array().unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["id"], fixture.attempt_id);
    assert_eq!(attempts[0]["status"], "LEASED");

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
    assert_eq!(candidates.as_array().unwrap().len(), 0);

    core.stop();
    drop(core);
    drop(root);
}

#[tokio::test]
async fn failing_frozen_check_produces_repair_required_gate_not_accepted() {
    let root = TestRoot::new("negative-check");
    let repository = create_repository(root.path());
    let (mut core, client, base_url) = start_core(root.path()).await;
    let project_id = create_project(&client, &base_url, &repository, "check-project").await;
    let action = create_action(
        &client,
        &base_url,
        &project_id,
        "check-action",
        &["src"],
        json!([
            {
                "id": "must-contain-required",
                "kind": "functional",
                "description": "required content is present",
                "command": ["grep", "-q", "required", "src/fixture.txt"],
                "expectExit": 0,
                "timeoutSecs": 30
            }
        ]),
    )
    .await;
    let fixture = authorize_and_start_attempt(&client, &base_url, &action, "check", &["src"]).await;
    let (_supervisor, supervisor_url) =
        SupervisorProcess::start(root.path(), &repository, &base_url);

    let status = run_worker_once(
        root.path(),
        &base_url,
        &supervisor_url,
        "worker-check",
        "src/fixture.txt",
        "fixture candidate\n",
    );
    assert!(
        status.success(),
        "Worker failed before producing a candidate: {} core: {}",
        std::fs::read_to_string(root.path().join("worker-worker-check.log")).unwrap_or_default(),
        std::fs::read_to_string(root.path().join("core.log")).unwrap_or_default()
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
    let candidates = candidates.as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    let candidate_id = candidate["id"].as_str().unwrap();
    assert_eq!(candidate["attempt_id"], fixture.attempt_id);

    let status = run_verifier(
        root.path(),
        &base_url,
        candidate_id,
        "verifier-negative-check",
    );
    let log = std::fs::read_to_string(root.path().join("verifier-verifier-negative-check.log"))
        .unwrap_or_default();
    assert!(
        !status.success(),
        "Verifier unexpectedly passed a failing check; log:\n{log}"
    );

    let receipts: Value = operator(
        &client,
        reqwest::Method::GET,
        format!("{base_url}/quality/candidates/{candidate_id}/receipts"),
        None,
    )
    .await
    .json()
    .await
    .unwrap();
    let verifier_receipt = receipts
        .as_array()
        .unwrap()
        .iter()
        .find(|receipt| receipt["role"] == "verifier")
        .expect("verifier receipt");
    assert_eq!(verifier_receipt["result"], "FAIL");
    assert_eq!(verifier_receipt["evidence"], "EXECUTED");

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
    assert_ne!(gate["outcome"], "ACCEPTED");
    assert_eq!(gate["outcome"], "REPAIR_REQUIRED");

    let reviewer = client
        .post(format!("{base_url}/quality/receipts"))
        .bearer_auth(REVIEWER_TOKEN)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": "reviewer-negative-check",
            "principal_id": "reviewer-negative-check",
            "actionSpecSha256": fixture.action_spec_sha256,
            "commitSha": candidate["commit_sha"],
            "treeSha": candidate["tree_sha"],
            "result": "PASS",
            "evidence": "EXECUTED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reviewer.status(), StatusCode::CONFLICT);
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
            .find(|run| run["id"] == fixture.run_id)
            .unwrap()["status"],
        "REPAIR_REQUIRED"
    );

    core.stop();
    drop(core);
    drop(root);
}

#[tokio::test]
async fn unauthorized_run_is_rejected_and_authorized_run_without_attempt_has_no_claim() {
    let root = TestRoot::new("negative-authorization");
    let repository = create_repository(root.path());
    let (mut core, client, base_url) = start_core(root.path()).await;
    let project_id = create_project(&client, &base_url, &repository, "authorization-project").await;
    let action = create_action(
        &client,
        &base_url,
        &project_id,
        "authorization-action",
        &["src"],
        json!([]),
    )
    .await;
    let action_id = action["id"].as_str().unwrap();
    let campaign = create_campaign(&client, &base_url, action_id, "authorization-campaign").await;
    assert_eq!(campaign["status"], "BLOCKED_AUTHORIZATION");
    let campaign_id = campaign["id"].as_str().unwrap();

    let unauthorized_run = operator(
        &client,
        reqwest::Method::POST,
        format!("{base_url}/campaigns/{campaign_id}/runs"),
        None,
    )
    .await;
    assert_eq!(unauthorized_run.status(), StatusCode::BAD_REQUEST);

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
    assert_eq!(runs.as_array().unwrap().len(), 0);

    authorize_campaign(
        &client,
        &base_url,
        campaign_id,
        "authorization-grant",
        &["src"],
    )
    .await;
    let run_id = create_run(&client, &base_url, campaign_id).await;

    let status = run_worker_once(
        root.path(),
        &base_url,
        "http://127.0.0.1:9",
        "worker-no-attempt",
        "src/fixture.txt",
        "fixture candidate\n",
    );
    let log = std::fs::read_to_string(root.path().join("worker-worker-no-attempt.log"))
        .unwrap_or_default();
    assert!(
        status.success(),
        "Worker failed with no assignable attempt; log:\n{log}"
    );

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
    assert_eq!(attempts.as_array().unwrap().len(), 0);

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
    assert_eq!(candidates.as_array().unwrap().len(), 0);

    let registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({
            "worker_id": "worker-claim-negative",
            "registration_key": "registration-worker-claim-negative"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::OK);
    let session: Value = registration.json().await.unwrap();
    let session_token = session["session_token"].as_str().unwrap();
    let claim = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(session_token)
        .json(&json!({"worker_id": "worker-claim-negative"}))
        .send()
        .await
        .unwrap();
    assert_eq!(claim.status(), StatusCode::OK);
    let claim: Value = claim.json().await.unwrap();
    assert_eq!(claim["dispatch_supported"].as_bool(), Some(true));
    assert!(claim["assignment"].is_null());

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
    let runs = runs.as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["id"], run_id);
    assert_eq!(runs[0]["status"], "QUEUED");

    core.stop();
    drop(core);
    drop(root);
}

#[tokio::test]
async fn stale_lease_generation_is_rejected_by_worker_api() {
    let root = TestRoot::new("negative-lease");
    let repository = create_repository(root.path());
    let (mut core, client, base_url) = start_core(root.path()).await;
    let project_id = create_project(&client, &base_url, &repository, "lease-project").await;
    let action = create_action(
        &client,
        &base_url,
        &project_id,
        "lease-action",
        &["src"],
        json!([]),
    )
    .await;
    let fixture = authorize_and_start_attempt(&client, &base_url, &action, "lease", &["src"]).await;
    let worker_id = "worker-lease";

    let registration = client
        .post(format!("{base_url}/worker/register"))
        .bearer_auth(BOOTSTRAP_TOKEN)
        .json(&json!({
            "worker_id": worker_id,
            "registration_key": "registration-worker-lease"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(registration.status(), StatusCode::OK);
    let session: Value = registration.json().await.unwrap();
    let session_token = session["session_token"].as_str().unwrap();

    let claim = client
        .post(format!("{base_url}/worker/claim"))
        .bearer_auth(session_token)
        .json(&json!({"worker_id": worker_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(claim.status(), StatusCode::OK);
    let claim: Value = claim.json().await.unwrap();
    assert_eq!(claim["dispatch_supported"].as_bool(), Some(true));
    let assignment = &claim["assignment"];
    assert!(!assignment.is_null());
    let lease = &assignment["lease"];
    let attempt_id = lease["attempt_id"].as_str().unwrap();
    assert_eq!(attempt_id, fixture.attempt_id);
    let generation = lease["generation"].as_i64().unwrap();
    assert_eq!(generation, 1);
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
            .find(|run| run["id"] == fixture.run_id)
            .unwrap()["status"],
        "RUNNING"
    );

    let stale_heartbeat = client
        .post(format!("{base_url}/worker/heartbeat"))
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": worker_id,
            "generation": generation + 1
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale_heartbeat.status(), StatusCode::BAD_REQUEST);

    let live_heartbeat = client
        .post(format!("{base_url}/worker/heartbeat"))
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": worker_id,
            "generation": generation
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(live_heartbeat.status(), StatusCode::OK);

    let stale_report = client
        .post(format!("{base_url}/worker/report"))
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": worker_id,
            "generation": generation + 1,
            "receipt_id": "stale-lease-report",
            "result_sha256": "a".repeat(64),
            "terminal_status": "FAILED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale_report.status(), StatusCode::BAD_REQUEST);

    let live_report = client
        .post(format!("{base_url}/worker/report"))
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": worker_id,
            "generation": generation,
            "receipt_id": "live-lease-report",
            "result_sha256": "b".repeat(64),
            "terminal_status": "FAILED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(live_report.status(), StatusCode::NO_CONTENT);

    let heartbeat_after_terminal = client
        .post(format!("{base_url}/worker/heartbeat"))
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": worker_id,
            "generation": generation
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(heartbeat_after_terminal.status(), StatusCode::BAD_REQUEST);

    let second_report = client
        .post(format!("{base_url}/worker/report"))
        .bearer_auth(session_token)
        .json(&json!({
            "attempt_id": attempt_id,
            "worker_id": worker_id,
            "generation": generation,
            "receipt_id": "second-lease-report",
            "result_sha256": "c".repeat(64),
            "terminal_status": "FAILED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(second_report.status(), StatusCode::BAD_REQUEST);

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
    let attempts = attempts.as_array().unwrap();
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["id"], attempt_id);
    assert_eq!(attempts[0]["status"], "FAILED");
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
            .find(|run| run["id"] == fixture.run_id)
            .unwrap()["status"],
        "FAILED"
    );

    // Lease expiry needs more than the fixed 60s claim lease or a test-only
    // clock. It is covered by
    // native/src/db/tests.rs::p7_p8_claim_is_fenced_expires_and_survives_restart
    // rather than simulated here against the real process clock.

    core.stop();
    drop(core);
    drop(root);
}
