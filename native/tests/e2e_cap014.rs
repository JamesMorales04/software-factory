//! Process-level CAP-014 execution through the Core-owned model gateway.
//!
//! A deterministic fake OpenAI-compatible upstream records no paid activity.
//! The test proves the evaluator uses the shared authorization/budget path and
//! that exhaustion produces a durable FAILED run rather than a false complete.

use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{Json, Router, routing::post};
use reqwest::StatusCode;
use serde_json::{Value, json};

const OPERATOR_TOKEN: &str = "operator-token-cap014";
const BOOTSTRAP_TOKEN: &str = "bootstrap-token-123456789012345678901234";
const VERIFIER_TOKEN: &str = "verifier-token-1234567890123456789012";
const REVIEWER_TOKEN: &str = "reviewer-token-1234567890123456789012";
const ATTEMPT_GRANT_KEY: &str = "attempt-grant-key-0123456789abcdef";
const MODEL_TOKEN: &str = "model-token-0123456789abcdef";

struct CoreProcess {
    child: Child,
    log: PathBuf,
}

impl CoreProcess {
    fn start(database: &Path, port: u16, log: &Path, model_url: &str, cost: u64) -> Self {
        let file = File::create(log).expect("core log");
        let child = Command::new(env!("CARGO_BIN_EXE_factory-core"))
            .env("FACTORY_DATABASE", database)
            .env("FACTORY_BIND", format!("127.0.0.1:{port}"))
            .env("FACTORY_OPERATOR_TOKEN", OPERATOR_TOKEN)
            .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", BOOTSTRAP_TOKEN)
            .env("FACTORY_VERIFIER_TOKEN", VERIFIER_TOKEN)
            .env("FACTORY_REVIEWER_TOKEN", REVIEWER_TOKEN)
            .env("FACTORY_ATTEMPT_GRANT_KEY", ATTEMPT_GRANT_KEY)
            .env("FACTORY_MODEL_URL", model_url)
            .env("FACTORY_MODEL_TOKEN", MODEL_TOKEN)
            .env("FACTORY_MODEL_PROVIDER", "opencode-go")
            .env("FACTORY_MODEL_NAME", "deepseek-v4.1-flash")
            .env("FACTORY_ALLOW_TEST_UPSTREAM", "1")
            .env("FACTORY_EVALUATION_MODEL_MAX_COST_MICROS", cost.to_string())
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

#[derive(Clone)]
struct Upstream {
    calls: Arc<AtomicUsize>,
}

async fn chat(axum::extract::State(state): axum::extract::State<Upstream>) -> Json<Value> {
    state.calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "choices":[{"message":{"content":"{\"intent\":\"change\",\"authority\":\"advisory\"}"}}],
        "usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30,"cost_micros":4}
    }))
}

async fn upstream() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/chat/completions", post(chat))
        .with_state(Upstream {
            calls: calls.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, calls, server)
}

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "factory-cap014-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn run_evaluator(core_url: &str, evaluation_id: &str, output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_factory-s4d-evaluate"))
        .env("FACTORY_OPERATOR_TOKEN", OPERATOR_TOKEN)
        .args(["execute", core_url, evaluation_id])
        .arg(output)
        .output()
        .expect("run factory-s4d-evaluate")
}

async fn model_calls(client: &reqwest::Client, base: &str) -> Value {
    client
        .get(format!("{base}/model/calls?limit=100"))
        .bearer_auth(OPERATOR_TOKEN)
        .send()
        .await
        .expect("list model calls")
        .json()
        .await
        .expect("model calls json")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cap014_executes_through_core_budget_and_records_settled_usage() {
    let (model_url, upstream_calls, upstream_server) = upstream().await;
    let root = temp_root("success");
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(
        &root.join("factory.sqlite"),
        port,
        &root.join("core.log"),
        &model_url,
        2_000_000,
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    wait_healthy(&client, &base, &mut core).await;

    let output_path = root.join("cap014.json");
    let output = run_evaluator(&base, "cap014-success", &output_path);
    assert!(
        output.status.success(),
        "evaluator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence: Value =
        serde_json::from_slice(&std::fs::read(&output_path).expect("evidence file")).unwrap();
    assert_eq!(evidence["status"], "COMPLETE");
    assert_eq!(evidence["fixtures"].as_array().unwrap().len(), 11);
    assert_eq!(evidence["input_tokens"], 220);
    assert_eq!(evidence["output_tokens"], 110);
    assert_eq!(evidence["cost_micros"], 99);
    assert_eq!(upstream_calls.load(Ordering::SeqCst), 11);

    let calls = model_calls(&client, &base).await;
    let calls = calls.as_array().expect("model calls");
    assert_eq!(calls.len(), 11);
    assert!(calls.iter().all(|call| call["role"] == "evaluation"));
    assert!(calls.iter().all(|call| call["state"] == "SETTLED"));
    assert!(calls.iter().all(|call| call["usage_known"] == true));

    core.stop();
    upstream_server.abort();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cap014_budget_exhaustion_produces_failed_evidence_and_stops() {
    let (model_url, upstream_calls, upstream_server) = upstream().await;
    let root = temp_root("budget");
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut core = CoreProcess::start(
        &root.join("factory.sqlite"),
        port,
        &root.join("core.log"),
        &model_url,
        1,
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    wait_healthy(&client, &base, &mut core).await;

    let output_path = root.join("cap014.json");
    let output = run_evaluator(&base, "cap014-budget", &output_path);
    assert!(!output.status.success(), "exhausted run must fail");
    let evidence: Value =
        serde_json::from_slice(&std::fs::read(&output_path).expect("evidence file")).unwrap();
    assert_eq!(evidence["status"], "FAILED");
    assert_eq!(evidence["fixtures"].as_array().unwrap().len(), 1);
    assert_eq!(upstream_calls.load(Ordering::SeqCst), 0);
    assert!(evidence["error"].as_str().unwrap().contains("403"));

    let calls = model_calls(&client, &base).await;
    assert_eq!(calls.as_array().expect("model calls").len(), 0);

    core.stop();
    upstream_server.abort();
    let _ = std::fs::remove_dir_all(root);
}
