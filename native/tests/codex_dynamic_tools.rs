#![cfg(unix)]

use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, header::CONTENT_TYPE},
    response::Response,
    routing::post,
};
use factory_native::protocol::{
    DynamicToolCallParams, DynamicToolCallResult, DynamicToolContentItem, DynamicToolSpec, JsonRpc,
    RpcEvent, ThreadStartParams, TurnSandboxPolicy, TurnStartParams, UserInput,
};
use serde_json::{Value, json};
use tokio::{process::Command, time};
use uuid::Uuid;

const MODEL: &str = "mock-model";
const FINAL: &str = "{\"type\":\"final\",\"files\":[],\"explanation\":\"dynamic tool completed\"}";

#[derive(Clone)]
struct MockState {
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Value>>>,
}

struct Mock {
    address: SocketAddr,
    state: MockState,
    task: tokio::task::JoinHandle<()>,
}

impl Mock {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let state = MockState {
            calls: Arc::new(AtomicUsize::new(0)),
            seen: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/v1/responses", post(responses))
            .with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            address,
            state,
            task,
        }
    }

    fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }

    fn seen(&self) -> Vec<Value> {
        self.state.seen.lock().unwrap().clone()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn responses(State(state): State<MockState>, body: String) -> Response {
    let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    state.seen.lock().unwrap().push(value);
    let call = state.calls.fetch_add(1, Ordering::SeqCst);
    let (events, response) = if call == 0 {
        function_call_events()
    } else {
        final_events()
    };
    let mut payload = String::new();
    for (event, data) in events {
        payload.push_str("event: ");
        payload.push_str(event);
        payload.push_str("\ndata: ");
        payload.push_str(&data.to_string());
        payload.push_str("\n\n");
    }
    let _ = response;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from(payload))
        .unwrap()
}

fn base_response(status: &str, output: Value) -> Value {
    json!({
        "id":"resp_mock",
        "object":"response",
        "created_at":1,
        "status":status,
        "model":MODEL,
        "output":output,
        "output_text":"",
        "usage": if status == "completed" {
            json!({"input_tokens":10,"output_tokens":5,"total_tokens":15})
        } else {
            Value::Null
        }
    })
}

fn function_call_events() -> (Vec<(&'static str, Value)>, Value) {
    let item = json!({
        "type":"function_call",
        "id":"fc_mock",
        "call_id":"call_dynamic",
        "name":"write_file",
        "arguments":"{\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}",
        "status":"in_progress"
    });
    let done_item = json!({
        "type":"function_call",
        "id":"fc_mock",
        "call_id":"call_dynamic",
        "name":"write_file",
        "arguments":"{\"path\":\"src/provider.txt\",\"content\":\"provider candidate\\n\"}",
        "status":"completed"
    });
    let response = base_response("completed", json!([done_item.clone()]));
    (
        vec![
            (
                "response.created",
                json!({"type":"response.created","sequence_number":0,"response":base_response("in_progress", json!([]))}),
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
                json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":done_item}),
            ),
            (
                "response.completed",
                json!({"type":"response.completed","sequence_number":5,"response":response}),
            ),
        ],
        response,
    )
}

fn final_events() -> (Vec<(&'static str, Value)>, Value) {
    let item = json!({
        "type":"message",
        "id":"msg_final",
        "status":"completed",
        "role":"assistant",
        "content":[{"type":"output_text","text":FINAL,"annotations":[]}]
    });
    let response = base_response("completed", json!([item]));
    (
        vec![
            (
                "response.created",
                json!({"type":"response.created","sequence_number":0,"response":base_response("in_progress", json!([]))}),
            ),
            (
                "response.output_item.added",
                json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_final","status":"in_progress","role":"assistant","content":[]}}),
            ),
            (
                "response.output_text.delta",
                json!({"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_final","output_index":0,"content_index":0,"delta":FINAL}),
            ),
            (
                "response.output_text.done",
                json!({"type":"response.output_text.done","sequence_number":3,"item_id":"msg_final","output_index":0,"content_index":0,"text":FINAL}),
            ),
            (
                "response.output_item.done",
                json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":item}),
            ),
            (
                "response.completed",
                json!({"type":"response.completed","sequence_number":5,"response":response}),
            ),
        ],
        response,
    )
}

struct Root {
    path: PathBuf,
    home: PathBuf,
    codex_home: PathBuf,
    work: PathBuf,
}

impl Root {
    fn create() -> Self {
        let path = std::env::temp_dir().join(format!(
            "factory-codex-dynamic-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));
        let home = path.join("home");
        let codex_home = path.join("codex-home");
        let work = path.join("work");
        for directory in [&home, &codex_home, &work] {
            fs::create_dir_all(directory).unwrap();
        }
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&work)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Codex Dynamic"])
            .current_dir(&work)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "codex@example.invalid"])
            .current_dir(&work)
            .status()
            .unwrap();
        fs::write(work.join("seed.txt"), "seed\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "seed.txt"])
            .current_dir(&work)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "seed"])
            .current_dir(&work)
            .status()
            .unwrap();
        Self {
            path,
            home,
            codex_home,
            work,
        }
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn codex_bin() -> Option<PathBuf> {
    let value = std::env::var_os("CODEX_BIN")?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_file().then_some(path)
}

fn dynamic_tools() -> Vec<DynamicToolSpec> {
    vec![
        DynamicToolSpec::Function {
            name: "write_file".into(),
            description: "Write a file".into(),
            input_schema: json!({
                "type":"object",
                "additionalProperties":false,
                "required":["path","content"],
                "properties":{"path":{"type":"string"},"content":{"type":"string"}}
            }),
        },
        DynamicToolSpec::Function {
            name: "run_command".into(),
            description: "Run a frozen check".into(),
            input_schema: json!({
                "type":"object",
                "additionalProperties":false,
                "required":["argv"],
                "properties":{"argv":{"type":"array","minItems":1,"items":{"type":"string"}},"timeout_secs":{"type":"integer","minimum":1,"maximum":300}}
            }),
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_codex_app_server_invokes_a_client_dynamic_tool() {
    let Some(codex) = codex_bin() else {
        eprintln!("NOT_TESTED: CODEX_BIN is unavailable");
        return;
    };
    let root = Root::create();
    let mock = Mock::start().await;
    let provider = format!(
        "model_providers.mock={{name=\"Mock\",base_url=\"http://{}/v1\",env_key=\"MOCK_API_KEY\",wire_api=\"responses\"}}",
        mock.address
    );
    let stderr = fs::File::create(root.path.join("stderr.log")).unwrap();
    let mut command = Command::new(codex);
    for name in [
        "HOME",
        "CODEX_HOME",
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "OPENAI_ACCESS_TOKEN",
        "OPENAI_REFRESH_TOKEN",
        "OPENAI_BASE_URL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ] {
        command.env_remove(name);
    }
    let mut child = command
        .args(["app-server", "--listen", "stdio://"])
        .args(["-c", "model_provider=mock"])
        .args(["-c", "model=mock-model"])
        .args(["-c", provider.as_str()])
        .args(["-c", "approval_policy=never"])
        .args(["-c", "sandbox_mode=read-only"])
        .args(["-c", "features.shell_tool=false"])
        .args(["-c", "features.unified_exec=false"])
        .args(["-c", "features.unified_exec_tty=false"])
        .args(["-c", "features.view_image=false"])
        .args(["-c", "features.multi_agent=false"])
        .args(["-c", "features.goals=false"])
        .args(["-c", "features.default_mode_request_user_input=false"])
        .args(["-c", "features.standalone_web_search=false"])
        .args(["-c", "features.web_search_request=false"])
        .args(["-c", "web_search=\"disabled\""])
        .current_dir(&root.work)
        .env("HOME", &root.home)
        .env("CODEX_HOME", &root.codex_home)
        .env("MOCK_API_KEY", "mock-token-0123456789")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .process_group(0)
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut rpc = JsonRpc::new(stdout, stdin);
    rpc.initialize("factory_test", "Factory test", "0.1.0", true)
        .await
        .unwrap();
    let _ = rpc.request("model/list", json!({})).await.unwrap();
    let thread = rpc
        .start_thread(ThreadStartParams {
            cwd: root.work.to_string_lossy().into_owned(),
            model: Some(MODEL.into()),
            model_provider: Some("mock".into()),
            dynamic_tools: Some(dynamic_tools()),
            ephemeral: Some(true),
            base_instructions: Some("Use only dynamic tools.".into()),
            developer_instructions: None,
            ..Default::default()
        })
        .await
        .unwrap();
    let turn = rpc
        .start_turn(TurnStartParams {
            thread_id: thread.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Write src/provider.txt through the dynamic tool.".into(),
            }],
            sandbox_policy: TurnSandboxPolicy::ReadOnly {
                network_access: false,
            },
            model: Some(MODEL.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let deadline = time::Instant::now() + Duration::from_secs(60);
    let mut final_text = None;
    let mut tool_call = None;
    loop {
        assert!(
            time::Instant::now() < deadline,
            "dynamic tool turn timed out"
        );
        let event = time::timeout_at(deadline, rpc.next_event())
            .await
            .unwrap()
            .unwrap();
        match event {
            RpcEvent::ServerRequest(request) => {
                if request.method == "item/tool/call" {
                    let params: DynamicToolCallParams =
                        serde_json::from_value(request.params).unwrap();
                    assert_eq!(params.thread_id, thread.thread.id);
                    assert_eq!(params.turn_id, turn.turn.id);
                    assert!(params.namespace.is_none());
                    tool_call = Some(params.clone());
                    rpc.respond(
                        request.id,
                        serde_json::to_value(DynamicToolCallResult {
                            content_items: vec![DynamicToolContentItem::InputText {
                                text: "write applied".into(),
                            }],
                            success: true,
                        })
                        .unwrap(),
                    )
                    .await
                    .unwrap();
                } else {
                    panic!("unexpected server request {}", request.method);
                }
            }
            RpcEvent::Notification(notification) => match notification.method.as_str() {
                "item/completed"
                    if notification
                        .params
                        .pointer("/item/type")
                        .and_then(Value::as_str)
                        == Some("agentMessage") =>
                {
                    final_text = notification
                        .params
                        .pointer("/item/text")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                "turn/completed" => {
                    let status = notification
                        .params
                        .pointer("/turn/status")
                        .and_then(Value::as_str);
                    assert_eq!(status, Some("completed"));
                    break;
                }
                _ => {}
            },
        }
    }
    let call = tool_call.expect("Codex must invoke the client dynamic tool");
    assert_eq!(call.arguments["path"], "src/provider.txt");
    assert_eq!(final_text.as_deref(), Some(FINAL));
    assert_eq!(
        mock.calls(),
        2,
        "dynamic tool requires a second model request"
    );
    let seen = mock.seen();
    let names = seen[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"write_file"),
        "dynamic tool declared: {names:?}"
    );
    for forbidden in [
        "exec_command",
        "write_stdin",
        "apply_patch",
        "view_image",
        "multi_agent_v1",
        "web_search",
    ] {
        assert!(
            !names.contains(&forbidden),
            "native tool {forbidden} was not disabled: {names:?}"
        );
    }
    assert!(
        seen[1].to_string().contains("call_dynamic")
            || seen[1].to_string().contains("write applied"),
        "second upstream request must include the mediated tool result: {}",
        seen[1]
    );
    let _ = child.kill().await;
    let _ = child.wait().await;
}
