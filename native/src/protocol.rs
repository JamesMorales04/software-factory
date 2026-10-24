use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

const MAX_PENDING_NOTIFICATIONS: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("transport ended before response")]
    EndOfStream,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("protocol error: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListParams {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    pub archived: bool,
    pub source_kinds: Vec<&'static str>,
    pub use_state_db_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPage {
    pub data: Vec<ThreadSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    #[serde(default)]
    pub status: Option<ThreadStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStatus {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub active_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadParams<'a> {
    pub thread_id: &'a str,
    pub include_turns: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThreadReadResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLoadedListParams {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLoadedPage {
    pub data: Vec<String>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartParams {
    pub cwd: String,
    pub approval_policy: DenyApprovalPolicy,
    pub sandbox: ReadOnlySandbox,
}

#[derive(Debug, Clone, Serialize)]
pub enum DenyApprovalPolicy {
    #[serde(rename = "never")]
    Never,
}

#[derive(Debug, Clone, Serialize)]
pub enum ReadOnlySandbox {
    #[serde(rename = "read-only")]
    ReadOnly,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadStartResponse {
    pub thread: ThreadSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UserInput {
    Text { text: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartParams {
    pub thread_id: String,
    pub input: Vec<UserInput>,
    pub approval_policy: DenyApprovalPolicy,
    pub sandbox_policy: ReadOnlySandbox,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnStatus {
    Completed,
    Interrupted,
    Failed,
    InProgress,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TurnSummary {
    pub id: String,
    pub status: TurnStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TurnStartResponse {
    pub turn: TurnSummary,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnCompletedParams {
    pub thread_id: String,
    pub turn: TurnSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnInterruptParams {
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcNotification {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TurnWaitOutcome {
    Completed(TurnCompletedParams),
    TimeoutInterruptRequested { interrupt_response: Value },
}

pub struct JsonRpc<R, W> {
    reader: Lines<BufReader<R>>,
    writer: W,
    next_id: u64,
    notifications: VecDeque<RpcNotification>,
}

impl<R, W> JsonRpc<R, W>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader).lines(),
            writer,
            next_id: 1,
            notifications: VecDeque::new(),
        }
    }

    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), RpcError> {
        self.send(json!({"method":method,"params":params})).await
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"id":id,"method":method,"params":params}))
            .await?;
        loop {
            let line = self
                .reader
                .next_line()
                .await?
                .ok_or(RpcError::EndOfStream)?;
            let value: Value = serde_json::from_str(&line)?;
            if let Some(method) = value.get("method").and_then(Value::as_str) {
                if let Some(server_id) = value.get("id") {
                    self.answer_server_request(server_id.clone(), method)
                        .await?;
                    continue;
                }
                self.enqueue_notification(RpcNotification {
                    method: method.to_owned(),
                    params: value.get("params").cloned().unwrap_or(Value::Null),
                })?;
                continue;
            }
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    return Err(RpcError::Protocol(error.to_string()));
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| RpcError::Protocol("response has no result".into()));
            }
            return Err(RpcError::Protocol("unexpected JSON-RPC response id".into()));
        }
    }

    pub async fn start_thread(
        &mut self,
        params: ThreadStartParams,
    ) -> Result<ThreadStartResponse, RpcError> {
        let value = self
            .request("thread/start", serde_json::to_value(params)?)
            .await?;
        serde_json::from_value(value).map_err(RpcError::Json)
    }

    pub async fn start_turn(
        &mut self,
        params: TurnStartParams,
    ) -> Result<TurnStartResponse, RpcError> {
        let value = self
            .request("turn/start", serde_json::to_value(params)?)
            .await?;
        serde_json::from_value(value).map_err(RpcError::Json)
    }

    pub async fn interrupt_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<Value, RpcError> {
        if thread_id.is_empty() || turn_id.is_empty() {
            return Err(RpcError::Protocol(
                "turn/interrupt requires exact nonempty threadId and turnId".into(),
            ));
        }
        self.request(
            "turn/interrupt",
            serde_json::to_value(TurnInterruptParams {
                thread_id: thread_id.to_owned(),
                turn_id: turn_id.to_owned(),
            })?,
        )
        .await
    }

    pub async fn next_notification(&mut self) -> Result<RpcNotification, RpcError> {
        if let Some(notification) = self.notifications.pop_front() {
            return Ok(notification);
        }
        self.read_notification_from_transport().await
    }

    async fn read_notification_from_transport(&mut self) -> Result<RpcNotification, RpcError> {
        loop {
            let line = self
                .reader
                .next_line()
                .await?
                .ok_or(RpcError::EndOfStream)?;
            let value: Value = serde_json::from_str(&line)?;
            let method = value
                .get("method")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::Protocol("expected server notification".into()))?;
            if let Some(server_id) = value.get("id") {
                self.answer_server_request(server_id.clone(), method)
                    .await?;
                continue;
            }
            return Ok(RpcNotification {
                method: method.to_owned(),
                params: value.get("params").cloned().unwrap_or(Value::Null),
            });
        }
    }

    pub async fn wait_for_turn_completed(
        &mut self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<TurnCompletedParams, RpcError> {
        loop {
            if let Some(index) = self.notifications.iter().position(|notification| {
                notification.method == "turn/completed"
                    && notification.params.get("threadId").and_then(Value::as_str)
                        == Some(thread_id)
                    && notification
                        .params
                        .pointer("/turn/id")
                        .and_then(Value::as_str)
                        == Some(turn_id)
            }) {
                let notification = self.notifications.remove(index).unwrap();
                return decode_terminal_notification(notification.params);
            }
            let notification = self.read_notification_from_transport().await?;
            if notification.method == "turn/completed"
                && notification.params.get("threadId").and_then(Value::as_str) == Some(thread_id)
                && notification
                    .params
                    .pointer("/turn/id")
                    .and_then(Value::as_str)
                    == Some(turn_id)
            {
                return decode_terminal_notification(notification.params);
            }
            self.enqueue_notification(notification)?;
        }
    }

    /// Times out by requesting interruption only for the exact known pair. The response
    /// acknowledges the request; it does not establish that the turn is terminal.
    pub async fn wait_for_turn_or_interrupt_on_timeout(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        timeout: std::time::Duration,
    ) -> Result<TurnWaitOutcome, RpcError> {
        match tokio::time::timeout(timeout, self.wait_for_turn_completed(thread_id, turn_id)).await
        {
            Ok(completed) => completed.map(TurnWaitOutcome::Completed),
            Err(_) => {
                let interrupt_response = self.interrupt_turn(thread_id, turn_id).await?;
                Ok(TurnWaitOutcome::TimeoutInterruptRequested { interrupt_response })
            }
        }
    }

    fn enqueue_notification(&mut self, notification: RpcNotification) -> Result<(), RpcError> {
        if self.notifications.len() >= MAX_PENDING_NOTIFICATIONS {
            return Err(RpcError::Protocol(format!(
                "notification queue reached {MAX_PENDING_NOTIFICATIONS}; refusing to drop events"
            )));
        }
        self.notifications.push_back(notification);
        Ok(())
    }

    async fn answer_server_request(&mut self, id: Value, method: &str) -> Result<(), RpcError> {
        let result = match method {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                json!({"decision":"cancel"})
            }
            _ => {
                return self
                    .send(json!({
                        "id":id,
                        "error":{"code":-32601,"message":"unsupported server request; denied by client policy"}
                    }))
                    .await;
            }
        };
        self.send(json!({"id":id,"result":result})).await
    }

    pub async fn list_threads(&mut self, params: ThreadListParams) -> Result<ThreadPage, RpcError> {
        let value = self
            .request("thread/list", serde_json::to_value(params)?)
            .await?;
        serde_json::from_value(value).map_err(RpcError::Json)
    }

    pub async fn read_thread(
        &mut self,
        thread_id: &str,
        include_turns: bool,
    ) -> Result<ThreadReadResponse, RpcError> {
        let value = self
            .request(
                "thread/read",
                serde_json::to_value(ThreadReadParams {
                    thread_id,
                    include_turns,
                })?,
            )
            .await?;
        let response: ThreadReadResponse = serde_json::from_value(value)?;
        if response.thread.id != thread_id {
            return Err(RpcError::Protocol(format!(
                "thread/read identity mismatch: requested {thread_id}, received {}",
                response.thread.id
            )));
        }
        Ok(response)
    }

    pub async fn list_loaded_threads(
        &mut self,
        params: ThreadLoadedListParams,
    ) -> Result<ThreadLoadedPage, RpcError> {
        let value = self
            .request("thread/loaded/list", serde_json::to_value(params)?)
            .await?;
        serde_json::from_value(value).map_err(RpcError::Json)
    }

    async fn send(&mut self, value: Value) -> Result<(), RpcError> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await?;
        Ok(())
    }
}

fn decode_terminal_notification(params: Value) -> Result<TurnCompletedParams, RpcError> {
    let completion: TurnCompletedParams = serde_json::from_value(params)?;
    if completion.turn.status == TurnStatus::InProgress {
        return Err(RpcError::Protocol(
            "turn/completed notification carries nonterminal inProgress status".into(),
        ));
    }
    Ok(completion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn correlates_response_across_notification() {
        let input = b"{\"method\":\"notice\",\"params\":{}}\n{\"id\":1,\"result\":{\"ok\":true}}\n";
        let output = Vec::new();
        let mut rpc = JsonRpc::new(&input[..], output);
        assert_eq!(
            rpc.request("initialize", json!({})).await.unwrap(),
            json!({"ok":true})
        );
    }

    #[tokio::test]
    async fn refuses_to_drop_notifications_when_bounded_queue_is_full() {
        let mut input = String::new();
        for index in 0..=MAX_PENDING_NOTIFICATIONS {
            input.push_str(&format!(
                "{{\"method\":\"notice/{index}\",\"params\":{{}}}}\n"
            ));
        }
        let mut rpc = JsonRpc::new(input.as_bytes(), Vec::new());
        let error = rpc.request("initialize", json!({})).await.unwrap_err();
        assert!(error.to_string().contains("refusing to drop events"));
        assert_eq!(rpc.notifications.len(), MAX_PENDING_NOTIFICATIONS);
    }

    #[tokio::test]
    async fn explicitly_rejects_unknown_server_requests() {
        let (client, server) = tokio::io::duplex(4096);
        let (client_read, client_write) = tokio::io::split(client);
        let server = tokio::spawn(async move {
            let mut io = BufReader::new(server);
            let mut line = String::new();
            io.read_line(&mut line).await.unwrap();
            io.get_mut()
                .write_all(b"{\"id\":99,\"method\":\"unknown/serverRequest\",\"params\":{}}\n")
                .await
                .unwrap();
            line.clear();
            io.read_line(&mut line).await.unwrap();
            let rejection: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(rejection["id"], 99);
            assert_eq!(rejection["error"]["code"], -32601);
            io.get_mut()
                .write_all(b"{\"id\":1,\"result\":{\"ok\":true}}\n")
                .await
                .unwrap();
        });
        let mut rpc = JsonRpc::new(client_read, client_write);
        assert_eq!(
            rpc.request("initialize", json!({})).await.unwrap(),
            json!({"ok":true})
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn starts_thread_and_turn_answers_approval_and_retains_notifications() {
        let (client, server) = tokio::io::duplex(16_384);
        let (client_read, client_write) = tokio::io::split(client);
        let server_task = tokio::spawn(async move {
            let mut io = BufReader::new(server);
            let mut line = String::new();
            io.read_line(&mut line).await.unwrap();
            let thread_start: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(thread_start["method"], "thread/start");
            assert_eq!(thread_start["params"]["approvalPolicy"], "never");
            assert_eq!(thread_start["params"]["sandbox"], "read-only");
            assert_eq!(thread_start["params"]["cwd"], "/tmp/project");
            io.get_mut()
                .write_all(b"{\"method\":\"thread/started\",\"params\":{\"thread\":{\"id\":\"thread-1\"}}}\n")
                .await
                .unwrap();
            io.get_mut()
                .write_all(b"{\"id\":1,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}\n")
                .await
                .unwrap();

            line.clear();
            io.read_line(&mut line).await.unwrap();
            let turn_start: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(turn_start["method"], "turn/start");
            assert_eq!(turn_start["params"]["threadId"], "thread-1");
            assert_eq!(turn_start["params"]["approvalPolicy"], "never");
            assert_eq!(turn_start["params"]["sandboxPolicy"], "read-only");
            assert_eq!(turn_start["params"]["input"][0]["type"], "text");
            assert_eq!(turn_start["params"]["input"][0]["text"], "fixture only");

            io.get_mut()
                .write_all(b"{\"id\":2,\"method\":\"item/commandExecution/requestApproval\",\"params\":{\"threadId\":\"thread-1\",\"turnId\":\"turn-1\",\"itemId\":\"item-1\",\"startedAtMs\":1}}\n")
                .await
                .unwrap();
            line.clear();
            io.read_line(&mut line).await.unwrap();
            let approval_response: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(approval_response["id"], 2);
            assert_eq!(approval_response["result"]["decision"], "cancel");

            io.get_mut()
                .write_all(b"{\"method\":\"turn/started\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"inProgress\",\"items\":[]}}}\n")
                .await
                .unwrap();
            io.get_mut()
                .write_all(b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"other-thread\",\"turn\":{\"id\":\"turn-1\",\"status\":\"completed\",\"items\":[]}}}\n")
                .await
                .unwrap();
            io.get_mut()
                .write_all(b"{\"id\":2,\"result\":{\"turn\":{\"id\":\"turn-1\",\"status\":\"inProgress\",\"items\":[]}}}\n")
                .await
                .unwrap();
            io.get_mut()
                .write_all(b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"completed\",\"items\":[]}}}\n")
                .await
                .unwrap();
        });
        let mut rpc = JsonRpc::new(client_read, client_write);
        let thread = rpc
            .start_thread(ThreadStartParams {
                cwd: "/tmp/project".into(),
                approval_policy: DenyApprovalPolicy::Never,
                sandbox: ReadOnlySandbox::ReadOnly,
            })
            .await
            .unwrap();
        assert_eq!(thread.thread.id, "thread-1");
        let started = rpc
            .start_turn(TurnStartParams {
                thread_id: thread.thread.id,
                input: vec![UserInput::Text {
                    text: "fixture only".into(),
                }],
                approval_policy: DenyApprovalPolicy::Never,
                sandbox_policy: ReadOnlySandbox::ReadOnly,
            })
            .await
            .unwrap();
        assert_eq!(started.turn.status, TurnStatus::InProgress);
        let completed = rpc
            .wait_for_turn_completed("thread-1", "turn-1")
            .await
            .unwrap();
        assert_eq!(completed.turn.status, TurnStatus::Completed);
        assert_eq!(
            rpc.next_notification().await.unwrap().method,
            "thread/started"
        );
        assert_eq!(
            rpc.next_notification().await.unwrap().method,
            "turn/started"
        );
        let mismatched = rpc.next_notification().await.unwrap();
        assert_eq!(mismatched.method, "turn/completed");
        assert_eq!(mismatched.params["threadId"], "other-thread");
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn timeout_requests_interrupt_with_exact_known_thread_and_turn() {
        let (client, server) = tokio::io::duplex(4096);
        let (client_read, client_write) = tokio::io::split(client);
        let server_task = tokio::spawn(async move {
            let mut io = BufReader::new(server);
            let mut line = String::new();
            io.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "turn/interrupt");
            assert_eq!(request["params"]["threadId"], "thread-exact");
            assert_eq!(request["params"]["turnId"], "turn-exact");
            io.get_mut()
                .write_all(format!("{{\"id\":{},\"result\":{{}}}}\n", request["id"]).as_bytes())
                .await
                .unwrap();
        });
        let mut rpc = JsonRpc::new(client_read, client_write);
        let outcome = rpc
            .wait_for_turn_or_interrupt_on_timeout(
                "thread-exact",
                "turn-exact",
                std::time::Duration::from_millis(10),
            )
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            TurnWaitOutcome::TimeoutInterruptRequested { .. }
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn refuses_interrupt_without_both_exact_ids() {
        let mut rpc = JsonRpc::new(&b""[..], Vec::new());
        assert!(matches!(
            rpc.interrupt_turn("", "turn-1").await,
            Err(RpcError::Protocol(_))
        ));
        assert!(matches!(
            rpc.interrupt_turn("thread-1", "").await,
            Err(RpcError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn refuses_nonterminal_status_even_on_turn_completed_notification() {
        let input = b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"inProgress\",\"items\":[]}}}\n";
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let error = rpc
            .wait_for_turn_completed("thread-1", "turn-1")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("nonterminal inProgress"));
    }

    #[tokio::test]
    async fn decodes_thread_list_page_and_status_from_schema_fixture() {
        let input = br#"{"id":1,"result":{"data":[{"id":"thread-1","status":{"type":"idle"},"cwd":"/tmp","turns":[]}],"nextCursor":"next"}}
"#;
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let page = rpc
            .list_threads(ThreadListParams {
                cursor: None,
                limit: Some(100),
                archived: false,
                source_kinds: vec!["appServer"],
                use_state_db_only: true,
            })
            .await
            .unwrap();
        assert_eq!(page.data[0].id, "thread-1");
        assert_eq!(page.data[0].status.as_ref().unwrap().kind, "idle");
        assert_eq!(page.next_cursor.as_deref(), Some("next"));
    }

    #[tokio::test]
    async fn decodes_loaded_thread_page() {
        let input = br#"{"id":1,"result":{"data":["thread-1"],"nextCursor":null}}
"#;
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let page = rpc
            .list_loaded_threads(ThreadLoadedListParams {
                cursor: None,
                limit: Some(100),
            })
            .await
            .unwrap();
        assert_eq!(page.data, ["thread-1"]);
        assert_eq!(page.next_cursor, None);
    }

    #[tokio::test]
    async fn thread_read_rejects_mismatched_identity() {
        let input = br#"{"id":1,"result":{"thread":{"id":"other-thread","status":{"type":"idle"}}}}
"#;
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let error = rpc.read_thread("expected-thread", false).await.unwrap_err();
        assert!(error.to_string().contains("identity mismatch"));
    }

    #[tokio::test]
    async fn thread_read_reports_unloaded_provider_error_without_reclassifying_it() {
        let input = br#"{"id":1,"error":{"code":-32600,"message":"thread not loaded: thread-1"}}
"#;
        let mut rpc = JsonRpc::new(&input[..], Vec::new());
        let error = rpc.read_thread("thread-1", false).await.unwrap_err();
        assert!(error.to_string().contains("thread not loaded"));
    }

    #[tokio::test]
    async fn thread_list_uses_installed_schema_wire_fields() {
        let (client, server) = tokio::io::duplex(4096);
        let (client_read, client_write) = tokio::io::split(client);
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "thread/list");
            assert_eq!(request["params"]["archived"], true);
            assert_eq!(request["params"]["cursor"], "opaque");
            assert_eq!(request["params"]["useStateDbOnly"], true);
            assert_eq!(request["params"]["sourceKinds"][0], "appServer");
            let response = json!({"id":request["id"],"result":{"data":[],"nextCursor":null}});
            reader
                .get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        });
        let mut rpc = JsonRpc::new(client_read, client_write);
        rpc.list_threads(ThreadListParams {
            cursor: Some("opaque".into()),
            limit: Some(100),
            archived: true,
            source_kinds: vec!["appServer"],
            use_state_db_only: true,
        })
        .await
        .unwrap();
        server.await.unwrap();
    }
}
