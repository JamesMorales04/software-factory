use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

fn send(value: Value) {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    writeln!(stdout, "{value}").expect("fixture codex write");
    stdout.flush().expect("fixture codex flush");
}

fn env_is(name: &str, value: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some(value)
}

fn account_response(logged_in: bool) -> Value {
    if logged_in {
        return json!({"account":{"type":"chatgpt","email":"fixture@example.invalid","planType":"pro"},"requiresOpenaiAuth":true,"workspaceRouting":null});
    }
    match std::env::var("FACTORY_FIXTURE_CODEX_ACCOUNT").as_deref() {
        Ok("loggedOut") => {
            json!({"account":null,"requiresOpenaiAuth":true,"workspaceRouting":null})
        }
        Ok("apiKey") => {
            json!({"account":{"type":"apiKey"},"requiresOpenaiAuth":true,"workspaceRouting":null})
        }
        Ok("unsupported") => {
            json!({"account":{"type":"amazonBedrock"},"requiresOpenaiAuth":true,"workspaceRouting":null})
        }
        _ => {
            json!({"account":{"type":"chatgpt","email":"fixture@example.invalid","planType":"pro"},"requiresOpenaiAuth":true,"workspaceRouting":null})
        }
    }
}

fn model_response() -> Value {
    match std::env::var("FACTORY_FIXTURE_CODEX_MODEL_MODE").as_deref() {
        Ok("missing") => {
            json!({"data":[{"id":"gpt-6.1-sol","model":"gpt-6.1-sol","hidden":false}]})
        }
        Ok("wrong") => json!({"data":[{"id":"gpt-6-luna","model":"other","hidden":false}]}),
        Ok("duplicate") => json!({"data":[
            {"id":"gpt-6-luna","model":"gpt-6-luna","hidden":false},
            {"id":"gpt-6-luna","model":"gpt-6-luna","hidden":false}
        ]}),
        _ => json!({"data":[{"id":"gpt-6-luna","model":"gpt-6-luna","hidden":false}]}),
    }
}

fn review_text() -> String {
    std::env::var("FACTORY_FIXTURE_CODEX_REVIEW").unwrap_or_else(|_| "No findings.".into())
}

fn review_status(completed: bool) -> &'static str {
    if !completed {
        "interrupted"
    } else if env_is("FACTORY_FIXTURE_CODEX_REVIEW_STATUS", "failed") {
        "failed"
    } else {
        "completed"
    }
}

fn main() {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut next_id = 100;
    let mut logged_in = false;
    while let Some(Ok(line)) = lines.next() {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            continue;
        };
        let id = value.get("id").cloned();
        match method {
            "initialize" => {
                if let Some(id) = id {
                    send(
                        json!({"id":id,"result":{"userAgent":"factory-fixture-codex","codexHome":"/tmp","platformFamily":"unix","platformOs":"linux"}}),
                    );
                }
            }
            "initialized" => {}
            "model/list" => {
                if let Some(id) = id {
                    send(json!({"id":id,"result":model_response()}));
                }
            }
            "account/read" => {
                if let Some(id) = id {
                    send(json!({"id":id,"result":account_response(logged_in)}));
                }
            }
            "account/login/start" => {
                if let Some(id) = id {
                    send(json!({"id":id,"result":{
                        "type":"chatgptDeviceCode",
                        "loginId":"login-fixture",
                        "verificationUrl":"https://example.invalid/device",
                        "userCode":"FACTORY-TEST"
                    }}));
                    send(
                        json!({"method":"account/login/completed","params":{"loginId":"login-fixture","success":true,"error":null}}),
                    );
                    logged_in = true;
                }
            }
            "account/login/cancel" => {
                if let Some(id) = id {
                    send(json!({"id":id,"result":{"status":"canceled"}}));
                }
            }
            "thread/start" => {
                if let Some(id) = id {
                    let params = value.get("params").cloned().unwrap_or(json!({}));
                    let model = params
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("gpt-6-luna");
                    let provider = std::env::var("FACTORY_FIXTURE_CODEX_PROVIDER")
                        .unwrap_or_else(|_| "factory".into());
                    let cwd = params
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or("/session");
                    if let Ok(path) = std::env::var("FACTORY_FIXTURE_CODEX_CAPTURE") {
                        let _ = std::fs::write(path, serde_json::to_vec(&params).unwrap());
                    }
                    send(
                        json!({"method":"thread/started","params":{"thread":{"id":"thread-fixture"}}}),
                    );
                    send(
                        json!({"id":id,"result":{"thread":{"id":"thread-fixture"},"cwd":cwd,"model":model,"modelProvider":provider}}),
                    );
                }
            }
            "thread/read" => {
                if let Some(id) = id {
                    let thread_id = value
                        .pointer("/params/threadId")
                        .and_then(Value::as_str)
                        .unwrap_or("thread-fixture");
                    let text = review_text();
                    send(
                        json!({"id":id,"result":{"thread":{"id":thread_id,"turns":[{"items":[{"type":"exitedReviewMode","review":text}]}]}}}),
                    );
                }
            }
            "review/start" => {
                if let Some(id) = id {
                    let thread_id = value
                        .pointer("/params/threadId")
                        .and_then(Value::as_str)
                        .unwrap_or("thread-fixture")
                        .to_owned();
                    send(
                        json!({"id":id,"result":{"reviewThreadId":thread_id,"turn":{"id":"review-turn-fixture","status":"inProgress"}}}),
                    );
                    let status =
                        review_status(!env_is("FACTORY_FIXTURE_CODEX_REVIEW_INTERRUPTED", "1"));
                    if status == "completed" {
                        send(
                            json!({"method":"item/completed","params":{"threadId":thread_id,"turnId":"review-turn-fixture","item":{"id":"review-item","type":"exitedReviewMode","review":review_text()}}}),
                        );
                        if std::env::var("FACTORY_FIXTURE_CODEX_NO_USAGE").as_deref() != Ok("1") {
                            send(
                                json!({"method":"thread/tokenUsage/updated","params":{"threadId":thread_id,"turnId":"review-turn-fixture","tokenUsage":{"last":{"cachedInputTokens":0,"inputTokens":120,"outputTokens":80,"reasoningOutputTokens":0,"totalTokens":200},"total":{"cachedInputTokens":0,"inputTokens":120,"outputTokens":80,"reasoningOutputTokens":0,"totalTokens":200}}}}),
                            );
                        }
                    }
                    send(
                        json!({"method":"turn/completed","params":{"threadId":thread_id,"turn":{"id":"review-turn-fixture","status":status}}}),
                    );
                }
            }
            "thread/archive" | "thread/delete" => {
                if let Some(id) = id {
                    send(json!({"id":id,"result":{}}));
                }
            }
            "turn/start" => {
                let Some(id) = id else { continue };
                let turn_id = "turn-fixture";
                send(json!({"id":id,"result":{"turn":{"id":turn_id,"status":"inProgress"}}}));
                send(
                    json!({"method":"turn/started","params":{"threadId":"thread-fixture","turn":{"id":turn_id,"status":"inProgress"}}}),
                );
                let native = std::env::var("FACTORY_FIXTURE_CODEX_NATIVE").as_deref() == Ok("1");
                if native {
                    send(
                        json!({"method":"item/started","params":{"threadId":"thread-fixture","turnId":turn_id,"item":{"id":"native-1","type":"commandExecution","command":"id","status":"inProgress"}}}),
                    );
                } else {
                    let call_id = format!("call-{next_id}");
                    send(json!({"id":next_id,"method":"item/tool/call","params":{
                        "threadId":"thread-fixture","turnId":turn_id,"callId":call_id,"tool":"write_file","namespace":null,
                        "arguments":{"path":"src/provider.txt","content":"provider candidate\n"}
                    }}));
                    next_id += 1;
                    loop {
                        let Some(Ok(line)) = lines.next() else { return };
                        let Ok(response) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if response.get("id").and_then(Value::as_u64) == Some(next_id - 1) {
                            break;
                        }
                    }
                }
                if std::env::var("FACTORY_FIXTURE_CODEX_NO_FINAL").as_deref() != Ok("1") {
                    send(
                        json!({"method":"item/completed","params":{"threadId":"thread-fixture","turnId":turn_id,"item":{"id":"message-1","type":"agentMessage","text":"{\"type\":\"final\",\"files\":[],\"explanation\":\"fixture codex wrote the candidate\"}"}}}),
                    );
                }
                send(
                    json!({"method":"thread/tokenUsage/updated","params":{"threadId":"thread-fixture","turnId":turn_id,"tokenUsage":{"last":{"cachedInputTokens":0,"inputTokens":60,"outputTokens":40,"reasoningOutputTokens":0,"totalTokens":100},"total":{"cachedInputTokens":0,"inputTokens":60,"outputTokens":40,"reasoningOutputTokens":0,"totalTokens":100}}}}),
                );
                send(
                    json!({"method":"turn/completed","params":{"threadId":"thread-fixture","turn":{"id":turn_id,"status":"completed"}}}),
                );
            }
            "turn/interrupt" => {
                if let Some(id) = id {
                    send(json!({"id":id,"result":{}}));
                }
            }
            _ => {
                if let Some(id) = id {
                    send(
                        json!({"id":id,"error":{"code":-32601,"message":"fixture codex unsupported method"}}),
                    );
                }
            }
        }
    }
}
