use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

fn send(value: Value) {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    writeln!(stdout, "{value}").expect("fixture codex write");
    stdout.flush().expect("fixture codex flush");
}

fn main() {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut next_id = 100;
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
                    send(
                        json!({"id":id,"result":{"data":[{"id":"gpt-6-luna","model":"gpt-6-luna","displayName":"GPT-6-Luna"}]}}),
                    );
                }
            }
            "thread/start" => {
                if let Some(id) = id {
                    send(
                        json!({"method":"thread/started","params":{"thread":{"id":"thread-fixture"}}}),
                    );
                    send(
                        json!({"id":id,"result":{"thread":{"id":"thread-fixture"},"cwd":"/session","model":"gpt-6-luna","modelProvider":"factory"}}),
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
