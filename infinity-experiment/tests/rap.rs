use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use factory_infinity_experiment::rap::{RapBridge, RapCommandTool, RapServer};
use infinity_agent_core::ThreadId;
use infinity_agent_core::message::{InfinityMessage, InputMessage, InputMessageContent};
use infinity_agent_core::system::{AgentSystemBuilder, ModelSource, ResolvedModel};
use infinity_agent_core::traits::ConversationStore;
use infinity_provider_protocol::message::{AssistantContent, Message, ToolCall, UserContent};
use infinity_provider_protocol::{
    CompletionError, CompletionRequest, FinalResponse, ModelEntry, ModelProvider, ModelStream,
    StreamChunk, Usage,
};
use rap_client::http::{HttpClient, SimpleHttpClient};
use rap_protocol::{RapInvocation, ThreadId as RapThreadId, ToolCallId};
use tempfile::TempDir;

struct RapTestModelSource {
    thread_id: ThreadId,
}

#[async_trait(?Send)]
impl ModelSource for RapTestModelSource {
    async fn resolve(
        &self,
        _thread_id: &ThreadId<str>,
    ) -> Result<ResolvedModel, Box<dyn std::error::Error + Send + Sync>> {
        Ok(ResolvedModel {
            provider: Arc::new(RapTestProvider {
                thread_id: self.thread_id.clone(),
            }),
            model_id: "rap-fixture".to_owned(),
            context_window: 100_000,
            supports_image_input: false,
        })
    }
}

struct RapTestProvider {
    thread_id: ThreadId,
}

#[async_trait]
impl ModelProvider for RapTestProvider {
    async fn list_models(
        &self,
    ) -> Result<Vec<ModelEntry>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![ModelEntry {
            model_id: "rap-fixture".to_owned(),
            display_name: "RAP fixture".to_owned(),
            context_window: 100_000,
            max_output_tokens: Some(1_000),
            supports_image_input: false,
        }])
    }

    async fn invoke_model(
        &self,
        _model_id: &str,
        request: CompletionRequest,
    ) -> Result<ModelStream, CompletionError> {
        let has_result = request.chat_history.iter().any(|message| match message {
            Message::User { content } => content.iter().any(|item| match item {
                UserContent::ToolResult(result) => result
                    .content
                    .iter()
                    .any(|content| format!("{content:?}").contains("RAP command")),
                _ => false,
            }),
            _ => false,
        });
        let has_call = request.chat_history.iter().any(|message| match message {
            Message::Assistant { content } => content.iter().any(|item| match item {
                AssistantContent::ToolCall(call) => call.function.name == "run_command",
                _ => false,
            }),
            _ => false,
        });
        let chunks = if has_result {
            vec![
                Ok(StreamChunk::Text("RAP COMPLETE".to_owned())),
                Ok(StreamChunk::Final(FinalResponse {
                    usage: Some(Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                        total_tokens: 2,
                        cached_input_tokens: 0,
                    }),
                })),
            ]
        } else if has_call {
            vec![
                Ok(StreamChunk::Text("waiting for RAP callback".to_owned())),
                Ok(StreamChunk::Final(FinalResponse {
                    usage: Some(Usage::default()),
                })),
            ]
        } else {
            vec![
                Ok(StreamChunk::ToolCall(ToolCall::new(
                    format!("{}-rap", self.thread_id),
                    "run_command",
                    serde_json::json!({"nonce": "fixture"}),
                ))),
                Ok(StreamChunk::Final(FinalResponse {
                    usage: Some(Usage::default()),
                })),
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rap_async_command_callback_resumes_infinity_and_is_idempotent() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let root = TempDir::new().expect("tempdir");
            let store = factory_infinity_experiment::store::SqliteStore::open(
                root.path().join("rap.sqlite"),
            )
            .await
            .expect("store");
            let server = RapServer::start(root.path().join("rap-effects.log"))
                .await
                .expect("rap server");
            let bridge = RapBridge::start().await.expect("bridge");
            let callback_url = bridge.callback_url.clone();
            let tool = RapCommandTool {
                endpoint: server.endpoint().to_owned(),
                callback_url: callback_url.clone(),
                client: SimpleHttpClient::new(),
            };
            let thread_id = ThreadId::from("rap-thread");
            let system = AgentSystemBuilder::new_local(
                store.clone(),
                store.clone(),
                RapTestModelSource {
                    thread_id: thread_id.clone(),
                },
            )
            .without_builtin_tools()
            .tool(Box::new(tool))
            .callback_url(callback_url.clone())
            .start_with_observer(|_| infinity_agent_core::system::EventCollector::new());
            let bridge_task = bridge.spawn(system.sender());
            store.ensure_root_thread(&thread_id).await.expect("root");
            system.send_user_text(&thread_id, "run the fixture").await;

            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                let history = store
                    .load_history_up_to(&thread_id, None, None)
                    .await
                    .expect("history");
                if history.iter().any(|message| match message {
                    InfinityMessage::Assistant { content } => {
                        format!("{content:?}").contains("RAP COMPLETE")
                    }
                    _ => false,
                }) {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "RAP callback did not resume the thread: {history:#?}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(server.applied_count(), 1);
            let log = std::fs::read_to_string(root.path().join("rap-effects.log")).expect("log");
            assert_eq!(log.lines().count(), 1);

            // Duplicate invocation with the same RAP identity: the server
            // applies the external effect once, and Infinity ignores the
            // duplicate tool result because the call is already answered.
            let client = SimpleHttpClient::new();
            for _ in 0..2 {
                let invocation = RapInvocation {
                    operation: "fixture_command".to_owned(),
                    arguments: serde_json::json!({"nonce": "fixture"}),
                    id: ToolCallId::from("rap-thread-rap"),
                    call_id: None,
                    callback_url: callback_url.clone(),
                    group_id: RapThreadId::from("rap-thread"),
                    user_id: None,
                    thread_ancestors: None,
                };
                let status = client
                    .post(
                        server.endpoint(),
                        &serde_json::to_string(&invocation).expect("invocation"),
                    )
                    .await
                    .expect("post");
                assert_eq!(status, 202);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(server.applied_count(), 1);
            let history = store
                .load_history_up_to(&thread_id, None, None)
                .await
                .expect("history");
            let rap_results = history
                .iter()
                .filter(|message| match message {
                    InfinityMessage::ToolResult { result, .. } => {
                        format!("{result:?}").contains("RAP command")
                    }
                    _ => false,
                })
                .count();
            assert_eq!(
                rap_results, 1,
                "duplicate callback must not duplicate history"
            );
            let _ = InputMessage {
                content: InputMessageContent::User(UserContent::text("noop")),
                group_id: thread_id.clone(),
                metadata: None,
                synthetic: None,
                display_as: None,
                subscription: false,
            };
            bridge_task.abort();
            system.shutdown().await;
        })
        .await;
}
