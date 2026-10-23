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
            "idempotency_key": "action-key"
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

    let worker_log = File::create(root.join("worker.log")).unwrap();
    let worker_status = Command::new(env!("CARGO_BIN_EXE_factory-worker"))
        .env("FACTORY_CORE_URL", &base_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
        .env("FACTORY_WORKER_ID", "worker-e2e")
        .env("FACTORY_WORKER_REGISTRATION_KEY", "registration-key-e2e")
        .env("FACTORY_WORKSPACE_ROOT", root.join("workspaces"))
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

    let verifier = client
        .post(format!("{base_url}/quality/receipts"))
        .bearer_auth(VERIFIER_TOKEN)
        .json(&json!({
            "candidate_id": candidate_id,
            "receipt_id": "verifier-e2e",
            "principal_id": "verifier-e2e",
            "actionSpecSha256": spec_sha,
            "commitSha": commit,
            "treeSha": tree,
            "result": "PASS",
            "evidence": "EXECUTED"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(verifier.status(), StatusCode::CREATED);
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

    core.stop();
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}
