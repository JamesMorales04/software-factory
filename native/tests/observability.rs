//! Process-level proof that an unavailable OTLP collector is isolated from Core
//! request handling and that the operator status reports the degradation.

use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde_json::{Value, json};

const OPERATOR_TOKEN: &str = "operator-token-observability";
const BOOTSTRAP_TOKEN: &str = "bootstrap-token-123456789012345678901234";
const VERIFIER_TOKEN: &str = "verifier-token-1234567890123456789012";
const REVIEWER_TOKEN: &str = "reviewer-token-1234567890123456789012";
const ATTEMPT_GRANT_KEY: &str = "attempt-grant-key-0123456789abcdef";

struct CoreProcess {
    child: Child,
    log: PathBuf,
}

impl CoreProcess {
    fn start(database: &Path, port: u16, log: &Path, otlp_endpoint: &str) -> Self {
        let file = File::create(log).expect("core log");
        let child = Command::new(env!("CARGO_BIN_EXE_factory-core"))
            .env("FACTORY_DATABASE", database)
            .env("FACTORY_BIND", format!("127.0.0.1:{port}"))
            .env("FACTORY_OPERATOR_TOKEN", OPERATOR_TOKEN)
            .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
            .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
            .env("FACTORY_REVIEWER_TOKEN", REVIEWER_TOKEN)
            .env("FACTORY_ATTEMPT_GRANT_KEY", ATTEMPT_GRANT_KEY)
            .env("FACTORY_PROJECTS_ROOT", std::env::temp_dir())
            .env("FACTORY_OTLP_ENDPOINT", otlp_endpoint)
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

fn git(repository: &Path, arguments: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .status()
            .expect("git")
            .success()
    );
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
async fn unreachable_collector_does_not_fail_core_requests() {
    let root = std::env::temp_dir().join(format!(
        "factory-observability-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("root");
    let repository = root.join("repo");
    std::fs::create_dir(&repository).expect("repo");
    git(&repository, &["init", "-q"]);
    git(
        &repository,
        &["config", "user.name", "Observability Fixture"],
    );
    git(
        &repository,
        &["config", "user.email", "observability@example.invalid"],
    );
    std::fs::write(repository.join("seed.txt"), "seed\n").expect("seed");
    git(&repository, &["add", "seed.txt"]);
    git(&repository, &["commit", "-qm", "seed"]);

    let otlp_port = free_port();
    let otlp_endpoint = format!("http://127.0.0.1:{otlp_port}/v1/logs");
    let database = root.join("factory.sqlite");
    let log = root.join("core.log");
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(&database, port, &log, &otlp_endpoint);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("client");
    wait_healthy(&client, &base, &mut core).await;

    let project = authorized(
        &client,
        reqwest::Method::POST,
        format!("{base}/projects"),
        Some(json!({"repository": repository, "idempotency_key": "project-key"})),
    )
    .await;
    assert_eq!(project.status(), StatusCode::CREATED);
    let project: Value = project.json().await.expect("project");
    let project_id = project["id"].as_str().expect("project id").to_owned();

    let action = authorized(
        &client,
        reqwest::Method::POST,
        format!("{base}/actions"),
        Some(json!({
            "project_id": project_id,
            "objective": "prove collector outage isolation",
            "provider": "opencode",
            "model": "test/model",
            "idempotency_key": "action-key"
        })),
    )
    .await;
    assert_eq!(action.status(), StatusCode::CREATED);

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
    assert!(!events.as_array().expect("events array").is_empty());

    let bounded: Value = authorized(
        &client,
        reqwest::Method::GET,
        format!("{base}/events?after=0&limit=1"),
        None,
    )
    .await
    .json()
    .await
    .expect("bounded events");
    assert_eq!(bounded.as_array().expect("bounded array").len(), 1);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status: Value = authorized(
            &client,
            reqwest::Method::GET,
            format!("{base}/observability"),
            None,
        )
        .await
        .json()
        .await
        .expect("observability status");
        assert_eq!(status["enabled"], true);
        assert_eq!(status["exported"], 0);
        if status["failed_attempts"].as_u64().unwrap_or(0) >= 1 {
            assert!(status["last_error"].is_string());
            let text = status.to_string();
            assert!(!text.contains(OPERATOR_TOKEN));
            break;
        }
        if Instant::now() >= deadline {
            panic!("collector failure was not observed: {status}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let health = client
        .get(format!("{base}/health"))
        .send()
        .await
        .expect("health");
    assert_eq!(health.status(), StatusCode::OK);

    core.stop();
    let _ = std::fs::remove_dir_all(root);
}
