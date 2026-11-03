//! Minimal RAP tool server and bridge, used to evaluate asynchronous command
//! execution against Infinity.
//!
//! The server accepts a `RapInvocation`, performs one deliberately fixed and
//! idempotent fixture effect, and POSTs a `RapCallback::ToolResult` to the
//! invocation's callback URL. The bridge turns that callback into an Infinity
//! input message. No arbitrary host command or network access is exposed.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use infinity_agent_core::message::{InputMessage, InputMessageContent};
use infinity_agent_core::system::local::ChannelSender;
use infinity_agent_core::tools::{Tool, ToolContext};
use infinity_agent_core::traits::InputSender;
use infinity_provider_protocol::message::{Text, ToolResult, ToolResultContent, UserContent};
use rap_client::http::{HttpClient, SimpleHttpClient};
use rap_protocol::{
    PlainCallbackClient, ProviderCallId, RapCallback, RapInvocation, RapToolResult, ThreadId,
    ToolCallId,
};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Clone)]
struct RapServerState {
    log_path: std::path::PathBuf,
    applied: Arc<Mutex<HashSet<String>>>,
}

pub struct RapServer {
    endpoint: String,
    task: JoinHandle<()>,
    applied: Arc<Mutex<HashSet<String>>>,
}

impl RapServer {
    pub async fn start(log_path: impl Into<std::path::PathBuf>) -> Result<Self, std::io::Error> {
        let log_path = log_path.into();
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let applied = Arc::new(Mutex::new(HashSet::new()));
        let state = RapServerState {
            log_path,
            applied: applied.clone(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = Router::new()
            .route("/invoke", post(invoke))
            .with_state(state);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            endpoint: format!("http://{address}/invoke"),
            task,
            applied,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn applied_count(&self) -> usize {
        self.applied.lock().expect("bug: applied lock").len()
    }
}

impl Drop for RapServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn invoke(
    State(state): State<RapServerState>,
    Json(invocation): Json<RapInvocation>,
) -> StatusCode {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let first = {
            let mut applied = state.applied.lock().expect("bug: applied lock");
            applied.insert(invocation.id.as_str().to_owned())
        };
        if first {
            let line = format!(
                "rap:{}:{}:{}",
                invocation.group_id, invocation.id, invocation.operation
            );
            if let Err(error) = append_line(&state.log_path, &line) {
                tracing::error!(%error, "RAP fixture effect failed");
            }
        }
        let text = format!(
            "RAP command '{}' completed once for call {}. Fixture effect is idempotent.",
            invocation.operation, invocation.id
        );
        let client = PlainCallbackClient::new();
        rap_protocol::send_tool_result(&client, &invocation, &text, None, false).await;
    });
    StatusCode::ACCEPTED
}

fn append_line(path: &std::path::Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    file.sync_all()
}

/// A tool that dispatches a RAP invocation. It is intentionally not a shell:
/// the operation and arguments are validated by the server fixture.
#[derive(Clone)]
pub struct RapCommandTool {
    pub endpoint: String,
    pub callback_url: String,
    pub client: SimpleHttpClient,
}

#[async_trait]
impl Tool<ChannelSender> for RapCommandTool {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run one fixed asynchronous fixture command through a RAP server."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "nonce": { "type": "string" }
            },
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        args: Value,
        id: ToolCallId,
        call_id: Option<ProviderCallId>,
        context: &ToolContext<ChannelSender>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let invocation = RapInvocation {
            operation: "fixture_command".to_owned(),
            arguments: args,
            id: id.clone(),
            call_id: call_id.clone(),
            callback_url: self.callback_url.clone(),
            group_id: context.group_id.clone(),
            user_id: None,
            thread_ancestors: (context.thread_stack.len() > 1)
                .then(|| context.thread_stack[..context.thread_stack.len() - 1].to_vec()),
        };
        let body = serde_json::to_string(&invocation)?;
        let status = self.client.post(&self.endpoint, &body).await?;
        if !(200..300).contains(&status) {
            let message = InputMessage {
                content: InputMessageContent::User(UserContent::ToolResult(ToolResult {
                    id: id.as_str().to_owned(),
                    call_id: call_id.map(|call_id| call_id.into_inner()),
                    content: vec![ToolResultContent::Text(Text {
                        text: format!("Error: RAP server returned HTTP {status}"),
                    })],
                })),
                group_id: context.group_id.clone(),
                metadata: None,
                synthetic: None,
                display_as: None,
                subscription: false,
            };
            context
                .message_sender
                .send_to_input_queue(message, id.as_str())
                .await
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?;
        }
        Ok(())
    }
}

/// Callback receiver that converts RAP results into Infinity inputs.
pub struct RapBridge {
    pub callback_url: String,
    pub receiver: mpsc::UnboundedReceiver<RapCallback>,
}

impl RapBridge {
    pub async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (callback_url, receiver) =
            rap_client::callback_server::start_callback_channel().await?;
        Ok(Self {
            callback_url,
            receiver,
        })
    }

    /// Spawn the callback-to-Infinity bridge. Returns a task that ends when the
    /// callback channel closes (or the process exits).
    pub fn spawn(self, sender: ChannelSender) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut receiver = self.receiver;
            while let Some(callback) = receiver.recv().await {
                match callback {
                    RapCallback::ToolResult(result) => deliver_tool_result(&sender, result).await,
                    other => {
                        tracing::debug!(?other, "RAP callback ignored by fixture bridge");
                    }
                }
            }
        })
    }
}

async fn deliver_tool_result(sender: &ChannelSender, result: RapToolResult) {
    let text = result
        .text
        .or_else(|| {
            result.content.map(|content| {
                content
                    .into_iter()
                    .filter_map(|item| match item {
                        rap_protocol::RapToolResultContent::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .unwrap_or_else(|| "RAP tool returned no text".to_owned());
    let message = InputMessage {
        content: InputMessageContent::User(UserContent::ToolResult(ToolResult {
            id: result.id.as_str().to_owned(),
            call_id: result.call_id.map(|call_id| call_id.into_inner()),
            content: vec![ToolResultContent::Text(Text { text })],
        })),
        group_id: result.group_id,
        metadata: None,
        synthetic: None,
        display_as: None,
        subscription: false,
    };
    if let Err(error) = sender
        .send_to_input_queue(message, &format!("rap-callback:{}", result.id))
        .await
    {
        tracing::error!(%error, "failed to deliver RAP callback to Infinity");
    }
}

/// Helper for tests: a thread ID with a stable RAP group identity.
pub fn rap_thread(id: &str) -> ThreadId {
    ThreadId::from(id.to_owned())
}
