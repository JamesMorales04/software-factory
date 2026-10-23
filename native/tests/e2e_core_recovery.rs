//! Process-level deterministic recovery E2E.
//!
//! Starts the real Core binary against a disposable SQLite database and Git
//! repository, creates durable state, kills the process, restarts it, and
//! verifies the same Action is still present. No provider session is used.

use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde_json::{Value, json};

const OPERATOR_TOKEN: &str = "operator-token-e2e-recovery";
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

async fn authorized(
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

#[tokio::test]
async fn core_restart_preserves_durable_project_action_and_events() {
    let root = std::env::temp_dir().join(format!(
        "factory-e2e-recovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("root");
    let repository = root.join("repo");
    std::fs::create_dir(&repository).expect("repo");
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.name", "E2E Fixture"],
        vec!["config", "user.email", "e2e@example.invalid"],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(arguments)
                .status()
                .expect("git")
                .success()
        );
    }
    std::fs::write(repository.join("seed.txt"), "seed\n").expect("seed");
    for arguments in [vec!["add", "seed.txt"], vec!["commit", "-qm", "seed"]] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(arguments)
                .status()
                .expect("git")
                .success()
        );
    }

    let database = root.join("factory.sqlite");
    let log = root.join("core.log");
    let mut port = free_port();
    let mut base = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&database, port, &log);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client");
    for attempt in 0..5 {
        wait_healthy(&client, &base, &mut core).await;
        let response = authorized(
            &client,
            reqwest::Method::POST,
            format!("{base}/projects"),
            Some(json!({"repository": repository, "idempotency_key": "project-key"})),
        )
        .await;
        if response.status() == StatusCode::CREATED {
            break;
        }
        if attempt == 4 {
            panic!(
                "project registration failed: {}",
                response.text().await.unwrap_or_default()
            );
        }
        core.stop();
        port = free_port();
        base = format!("http://127.0.0.1:{port}");
        core = CoreProcess::start(&database, port, &log);
    }
    let project: Value = authorized(
        &client,
        reqwest::Method::GET,
        format!("{base}/projects"),
        None,
    )
    .await
    .json()
    .await
    .expect("projects");
    let project_id = project[0]["id"].as_str().expect("project id").to_owned();

    let action: Value = authorized(
        &client,
        reqwest::Method::POST,
        format!("{base}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "verify durable recovery",
            "provider": "opencode",
            "model": "test/model",
            "idempotency_key": "action-key"
        })),
    )
    .await
    .json()
    .await
    .expect("action");
    let action_id = action["id"].as_str().expect("action id").to_owned();
    assert_eq!(action["status"], "HUMAN_REQUIRED");

    core.stop();
    core = CoreProcess::start(&database, port, &log);
    wait_healthy(&client, &base, &mut core).await;

    let actions: Value = authorized(
        &client,
        reqwest::Method::GET,
        format!("{base}/actions"),
        None,
    )
    .await
    .json()
    .await
    .expect("actions");
    assert_eq!(actions.as_array().expect("array").len(), 1);
    assert_eq!(actions[0]["id"], action_id);
    assert_eq!(actions[0]["status"], "HUMAN_REQUIRED");
    assert_eq!(
        actions[0]["spec"]["base_commit"],
        action["spec"]["base_commit"]
    );

    let events: Value = authorized(
        &client,
        reqwest::Method::GET,
        format!("{base}/events"),
        None,
    )
    .await
    .json()
    .await
    .expect("events");
    let kinds: Vec<&str> = events
        .as_array()
        .expect("events array")
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"project.registered"));
    assert!(kinds.contains(&"action.created"));

    core.stop();
    drop(core);
    let _ = std::fs::remove_dir_all(root);
}
