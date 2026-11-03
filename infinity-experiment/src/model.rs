//! Deterministic, offline model adapter.
//!
//! No provider inference happens here. The adapter inspects durable thread
//! state and the model-visible history, then emits a scripted completion. It
//! is intentionally a `ModelProvider`, not a wrapper around a coding agent:
//! tool execution remains an Infinity responsibility.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use infinity_agent_core::ThreadId;
use infinity_agent_core::system::{ModelSource, ResolvedModel};
use infinity_provider_protocol::message::{AssistantContent, Message, ToolCall, UserContent};
use infinity_provider_protocol::{
    CompletionError, CompletionRequest, FinalResponse, ModelEntry, ModelProvider, ModelStream,
    StreamChunk, Usage,
};
use serde::{Deserialize, Serialize};

use crate::store::SqliteStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementerSpec {
    pub task_id: String,
    pub label: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyPlan {
    pub implementers: Vec<ImplementerSpec>,
    pub reviewer: ImplementerSpec,
}

impl JourneyPlan {
    pub fn default_calculator() -> Self {
        Self {
            implementers: vec![
                ImplementerSpec {
                    task_id: "implement-multiply".to_owned(),
                    label: "implementer-multiply".to_owned(),
                    instructions: "Implement multiply() in calculator.py. Keep add() unchanged. Run the baseline unit tests in your own disposable workspace.".to_owned(),
                },
                ImplementerSpec {
                    task_id: "implement-tests".to_owned(),
                    label: "implementer-tests".to_owned(),
                    instructions: "Add independent multiply() tests to test_calculator.py. Do not edit calculator.py. Syntax-check and run the baseline add test; full integration tests belong to the reviewer.".to_owned(),
                },
            ],
            reviewer: ImplementerSpec {
                task_id: "review".to_owned(),
                label: "reviewer".to_owned(),
                instructions: "Integrate both implementer workspaces, run the full test suite, create the candidate commit, then independently re-run the suite against the candidate.".to_owned(),
            },
        }
    }

    fn implementer(&self, index: usize) -> Option<&ImplementerSpec> {
        self.implementers.get(index)
    }
}

#[derive(Clone)]
pub struct ScriptedModelSource {
    store: SqliteStore,
    plan: Arc<JourneyPlan>,
    root_thread_id: ThreadId,
    model_delay: Duration,
}

impl ScriptedModelSource {
    pub fn new(
        store: SqliteStore,
        plan: Arc<JourneyPlan>,
        root_thread_id: ThreadId,
        model_delay: Duration,
    ) -> Self {
        Self {
            store,
            plan,
            root_thread_id,
            model_delay,
        }
    }
}

#[async_trait(?Send)]
impl ModelSource for ScriptedModelSource {
    async fn resolve(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<ResolvedModel, Box<dyn std::error::Error + Send + Sync>> {
        let is_root = thread_id.as_str() == self.root_thread_id.as_str();
        let provider = Arc::new(ScriptedProvider {
            store: self.store.clone(),
            plan: self.plan.clone(),
            thread_id: thread_id.to_owned(),
            root_thread_id: self.root_thread_id.clone(),
            is_root,
            model_delay: self.model_delay,
        });
        Ok(ResolvedModel {
            provider,
            model_id: "factory-deterministic-fixture".to_owned(),
            context_window: 200_000,
            supports_image_input: false,
        })
    }
}

struct ScriptedProvider {
    store: SqliteStore,
    plan: Arc<JourneyPlan>,
    thread_id: ThreadId,
    root_thread_id: ThreadId,
    is_root: bool,
    model_delay: Duration,
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    async fn list_models(
        &self,
    ) -> Result<Vec<ModelEntry>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(vec![ModelEntry {
            model_id: "factory-deterministic-fixture".to_owned(),
            display_name: "Factory Deterministic Fixture".to_owned(),
            context_window: 200_000,
            max_output_tokens: Some(8_192),
            supports_image_input: false,
        }])
    }

    async fn invoke_model(
        &self,
        _model_id: &str,
        request: CompletionRequest,
    ) -> Result<ModelStream, CompletionError> {
        if !self.model_delay.is_zero() {
            tokio::time::sleep(self.model_delay).await;
        }
        let chunks = if self.is_root {
            self.root_chunks(&request).await?
        } else {
            self.child_chunks(&request).await?
        };
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }
}

impl ScriptedProvider {
    async fn root_chunks(
        &self,
        request: &CompletionRequest,
    ) -> Result<Vec<Result<StreamChunk, CompletionError>>, CompletionError> {
        let children = self
            .store
            .children(&self.root_thread_id)
            .await
            .map_err(|error| CompletionError::ResponseError(error.to_string()))?;
        let reports = count_reports(&request.chat_history);
        let implementer_count = self.plan.implementers.len();

        if children.len() < implementer_count {
            let index = children.len();
            let spec = self
                .plan
                .implementer(index)
                .expect("bug: implementer index out of range");
            return Ok(tool_call(
                &format!("{}-spawn-{index}", self.thread_id),
                "spawn_thread",
                serde_json::json!({
                    "instructions": format!(
                        "TASK={}\nYour thread ID will be reported by the runtime. {}\nUse engineering_task with task_id \"{}\". Then report_to_parent with the result and close_thread.",
                        spec.task_id, spec.instructions, spec.task_id
                    ),
                    "child_of": [self.thread_id.as_str()],
                    "fresh_context": true
                }),
            ));
        }

        if reports < implementer_count {
            return Ok(text(&format!(
                "DIRECTOR: {implementer_count} implementers delegated; waiting for {implementer_count} reports."
            )));
        }

        if children.len() == implementer_count {
            let spec = &self.plan.reviewer;
            return Ok(tool_call(
                &format!("{}-spawn-review", self.thread_id),
                "spawn_thread",
                serde_json::json!({
                    "instructions": format!(
                        "TASK={}\n{}\nUse engineering_task with task_id \"{}\". Then report_to_parent with the review result and close_thread.",
                        spec.task_id, spec.instructions, spec.task_id
                    ),
                    "child_of": [self.thread_id.as_str()],
                    "fresh_context": true
                }),
            ));
        }

        if reports <= implementer_count {
            return Ok(text(
                "DIRECTOR: reviewer delegated; waiting for the independent review report.",
            ));
        }

        let report_text = collect_report_text(&request.chat_history);
        Ok(text(&format!(
            "DIRECTOR COMPLETE\nImplementers: {}\nReview: {}\nAggregated evidence: {}",
            implementer_count,
            if report_text.to_ascii_lowercase().contains("pass") {
                "PASS"
            } else {
                "NOT VERIFIED"
            },
            report_text
        )))
    }

    async fn child_chunks(
        &self,
        request: &CompletionRequest,
    ) -> Result<Vec<Result<StreamChunk, CompletionError>>, CompletionError> {
        let task = extract_task(&request.chat_history).unwrap_or_else(|| {
            if request
                .chat_history
                .iter()
                .any(|message| format!("{message:?}").contains("TASK=review"))
            {
                "review".to_owned()
            } else {
                "unknown".to_owned()
            }
        });
        let tool_calls = answered_tool_calls(&request.chat_history);
        let has_workspace_result = tool_calls.contains("engineering_task");
        let has_report_result = tool_calls.contains("report_to_parent");

        if !has_workspace_result {
            return Ok(tool_call(
                &format!("{}-engineering", self.thread_id),
                "engineering_task",
                serde_json::json!({ "task_id": task }),
            ));
        }

        if !has_report_result {
            let result_text = last_tool_result_text(&request.chat_history)
                .unwrap_or_else(|| "no tool result".to_owned());
            return Ok(tool_call(
                &format!("{}-report", self.thread_id),
                "report_to_parent",
                serde_json::json!({
                    "report": format!(
                        "TASK={} completed. {}",
                        task,
                        truncate(&result_text, 1_500)
                    )
                }),
            ));
        }

        if !tool_calls.contains("close_thread") {
            return Ok(tool_call(
                &format!("{}-close", self.thread_id),
                "close_thread",
                serde_json::json!({
                    "thread_id": self.thread_id.as_str()
                }),
            ));
        }

        Ok(text("Child work is complete and this thread is closing."))
    }
}

fn text(text: &str) -> Vec<Result<StreamChunk, CompletionError>> {
    vec![
        Ok(StreamChunk::Text(text.to_owned())),
        Ok(StreamChunk::Final(FinalResponse {
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                cached_input_tokens: 0,
            }),
        })),
    ]
}

fn tool_call(
    id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> Vec<Result<StreamChunk, CompletionError>> {
    vec![
        Ok(StreamChunk::ToolCall(ToolCall::new(id, name, arguments))),
        Ok(StreamChunk::Final(FinalResponse {
            usage: Some(Usage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                cached_input_tokens: 0,
            }),
        })),
    ]
}

fn count_reports(history: &[Message]) -> usize {
    history
        .iter()
        .filter(|message| match message {
            Message::User { content } => content.iter().any(|item| match item {
                UserContent::ToolResult(result) => result.content.iter().any(|content| {
                    let text = match content {
                        infinity_provider_protocol::message::ToolResultContent::Text(text) => {
                            &text.text
                        }
                        _ => "",
                    };
                    text.starts_with("Report from child thread")
                        || text.contains("has shut down. Report from child thread")
                }),
                _ => false,
            }),
            _ => false,
        })
        .count()
}

fn collect_report_text(history: &[Message]) -> String {
    let mut reports = Vec::new();
    for message in history {
        let Message::User { content } = message else {
            continue;
        };
        for item in content {
            let UserContent::ToolResult(result) = item else {
                continue;
            };
            for content in &result.content {
                if let infinity_provider_protocol::message::ToolResultContent::Text(text) = content
                    && (text.text.starts_with("Report from child thread")
                        || text
                            .text
                            .contains("has shut down. Report from child thread"))
                {
                    reports.push(text.text.clone());
                }
            }
        }
    }
    reports.join("\n---\n")
}

fn extract_task(history: &[Message]) -> Option<String> {
    for message in history.iter().rev() {
        match message {
            Message::Assistant { content } => {
                for item in content {
                    if let AssistantContent::ToolCall(call) = item
                        && let Some(instructions) = call
                            .function
                            .arguments
                            .get("instructions")
                            .and_then(|value| value.as_str())
                    {
                        for line in instructions.lines() {
                            if let Some(task) = line.strip_prefix("TASK=") {
                                return Some(task.trim().to_owned());
                            }
                        }
                    }
                }
            }
            Message::User { content } => {
                for item in content {
                    let UserContent::ToolResult(result) = item else {
                        continue;
                    };
                    for content in &result.content {
                        if let infinity_provider_protocol::message::ToolResultContent::Text(text) =
                            content
                        {
                            for line in text.text.lines() {
                                if let Some(task) = line.strip_prefix("TASK=") {
                                    return Some(task.trim().to_owned());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn answered_tool_calls(history: &[Message]) -> std::collections::HashSet<String> {
    let mut answered_ids = std::collections::HashSet::new();
    let mut names = std::collections::HashMap::new();
    for message in history {
        match message {
            Message::Assistant { content } => {
                for item in content {
                    if let AssistantContent::ToolCall(call) = item {
                        names.insert(call.id.clone(), call.function.name.clone());
                    }
                }
            }
            Message::User { content } => {
                for item in content {
                    if let UserContent::ToolResult(result) = item
                        && let Some(name) = names.get(&result.id)
                    {
                        answered_ids.insert(name.clone());
                    }
                }
            }
        }
    }
    answered_ids
}

fn last_tool_result_text(history: &[Message]) -> Option<String> {
    for message in history.iter().rev() {
        let Message::User { content } = message else {
            continue;
        };
        for item in content {
            let UserContent::ToolResult(result) = item else {
                continue;
            };
            let text = result
                .content
                .iter()
                .filter_map(|content| match content {
                    infinity_provider_protocol::message::ToolResultContent::Text(text) => {
                        Some(text.text.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push_str("...");
    truncated
}
