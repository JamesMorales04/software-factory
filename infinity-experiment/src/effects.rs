//! Infinity tool that executes the fixed engineering fixture through the
//! durable effect journal, plus restart reconciliation and callback delivery.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use infinity_agent_core::ThreadId;
use infinity_agent_core::message::{InputMessage, InputMessageContent};
use infinity_agent_core::system::local::ChannelSender;
use infinity_agent_core::tools::{Tool, ToolContext};
use infinity_agent_core::traits::{InputSender, StateStore};
use infinity_provider_protocol::message::{Text, ToolResult, ToolResultContent, UserContent};
use rap_protocol::ToolCallId;
use serde::{Deserialize, Serialize};

use crate::engineering::{EngineeringWorkspace, MarkerArtifact, read_marker_result};
use crate::journal::{BeginOutcome, CallbackRecord, EffectJournal, EffectState};
use crate::model::JourneyPlan;
use crate::store::{SqliteStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashPolicy {
    None,
    /// Die after the external marker is durable but before the callback is
    /// delivered. The next process must reconcile, not repeat the effect.
    AfterEffect,
    /// Same, but only after the named fixture task. Used to crash after the
    /// reviewer has produced a candidate.
    AfterEffectNamed(String),
    /// Die after intent/start and before the external marker exists. Recovery
    /// must report an ambiguous outcome and refuse automatic replay.
    AfterStart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfinementMode {
    LocalProcessFixedAllowlistNotCertified,
}

#[derive(Clone)]
pub struct EngineeringTaskTool {
    pub journal: EffectJournal,
    pub engineering: Arc<EngineeringWorkspace>,
    pub plan: Arc<JourneyPlan>,
    pub run_id: String,
    pub effect_delay: Duration,
    pub crash: CrashPolicy,
    pub confinement: ConfinementMode,
}

#[async_trait]
impl Tool<ChannelSender> for EngineeringTaskTool {
    fn name(&self) -> &str {
        "engineering_task"
    }

    fn description(&self) -> &str {
        "Execute one fixed engineering fixture task. Only allowlisted task ids may be used; the tool cannot run arbitrary commands."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "enum": self.plan.implementers.iter().map(|spec| spec.task_id.clone())
                        .chain(std::iter::once(self.plan.reviewer.task_id.clone()))
                        .collect::<Vec<_>>()
                }
            },
            "required": ["task_id"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        id: ToolCallId,
        _call_id: Option<rap_protocol::ProviderCallId>,
        context: &ToolContext<ChannelSender>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let task_id = args
            .get("task_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "task_id is required".to_owned())?;
        let known = self
            .plan
            .implementers
            .iter()
            .any(|spec| spec.task_id == task_id)
            || self.plan.reviewer.task_id == task_id;
        if !known {
            return Err(format!("task_id '{task_id}' is not allowlisted").into());
        }

        let request_json = serde_json::to_string(&args)?;
        let (effect_key, outcome) = self
            .journal
            .begin_effect(
                &self.run_id,
                context.group_id.as_str(),
                id.as_str(),
                self.name(),
                &request_json,
            )
            .await?;
        match outcome {
            BeginOutcome::New { attempt, lease } => {
                let tool = self.clone();
                let sender = context.message_sender.clone();
                let thread_id = context.group_id.clone();
                let tool_call_id = id.as_str().to_owned();
                let task_id = task_id.to_owned();
                tokio::spawn(async move {
                    if let Err(error) = tool
                        .run_effect(
                            &effect_key,
                            attempt,
                            &lease,
                            &thread_id,
                            &tool_call_id,
                            &task_id,
                            &sender,
                        )
                        .await
                    {
                        tracing::error!(%effect_key, %error, "effect execution failed");
                    }
                });
            }
            BeginOutcome::Completed { result }
            | BeginOutcome::Applied {
                result: Some(result),
            } => {
                let text = result_text(&result);
                self.finish_callback(
                    &effect_key,
                    self.current_attempt(&effect_key).await?,
                    context.group_id.as_str(),
                    id.as_str(),
                    &text,
                    &context.message_sender,
                )
                .await?;
            }
            BeginOutcome::Applied { result: None } | BeginOutcome::InFlight { .. } => {
                tracing::info!(%effect_key, "effect already in flight; not repeating");
            }
            BeginOutcome::Cancelled => {
                tracing::info!(%effect_key, "effect cancelled; not starting");
            }
            BeginOutcome::Failed { error } => {
                return Err(format!("effect previously failed: {error}").into());
            }
        }
        Ok(())
    }
}

impl EngineeringTaskTool {
    async fn current_attempt(&self, effect_key: &str) -> Result<i64, StoreError> {
        Ok(self
            .journal
            .effect(effect_key)
            .await?
            .map(|effect| effect.attempt)
            .unwrap_or(1))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_effect(
        &self,
        effect_key: &str,
        attempt: i64,
        lease: &str,
        thread_id: &ThreadId,
        tool_call_id: &str,
        task_id: &str,
        sender: &ChannelSender,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self
            .journal
            .mark_started(effect_key, attempt, lease)
            .await?
        {
            return Ok(());
        }
        if self.journal.is_cancelled(&self.run_id).await? {
            self.journal.cancel_effect(effect_key).await?;
            return Ok(());
        }
        if self.effect_delay > Duration::ZERO {
            tokio::time::sleep(self.effect_delay).await;
        }
        if self.journal.is_cancelled(&self.run_id).await? {
            self.journal.cancel_effect(effect_key).await?;
            return Ok(());
        }
        self.engineering.record_application(effect_key)?;

        if self.crash == CrashPolicy::AfterStart {
            std::process::exit(23);
        }
        let artifact = match self.engineering.perform(task_id).await {
            Ok(artifact) => artifact,
            Err(error) => {
                let result = serde_json::json!({
                    "task_id": task_id,
                    "status": "ERROR",
                    "error": error.to_string(),
                    "confinement": self.confinement,
                });
                let artifact = MarkerArtifact {
                    marker_path: String::new(),
                    marker_sha256: String::new(),
                    result,
                    candidate: None,
                };
                return self
                    .finish_effect(
                        effect_key,
                        attempt,
                        lease,
                        thread_id,
                        tool_call_id,
                        task_id,
                        artifact,
                        sender,
                    )
                    .await;
            }
        };
        self.finish_effect(
            effect_key,
            attempt,
            lease,
            thread_id,
            tool_call_id,
            task_id,
            artifact,
            sender,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_effect(
        &self,
        effect_key: &str,
        attempt: i64,
        lease: &str,
        thread_id: &ThreadId,
        tool_call_id: &str,
        task_id: &str,
        artifact: MarkerArtifact,
        sender: &ChannelSender,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !artifact.marker_path.is_empty() {
            let applied = self
                .journal
                .mark_applied(
                    effect_key,
                    attempt,
                    lease,
                    &artifact.marker_path,
                    &artifact.marker_sha256,
                )
                .await?;
            if !applied {
                tracing::warn!(%effect_key, "effect application could not be recorded");
                return Ok(());
            }
        }
        let crash_matches = match &self.crash {
            CrashPolicy::AfterEffect => true,
            CrashPolicy::AfterEffectNamed(name) => name == task_id,
            _ => false,
        };
        if crash_matches {
            // The process is the crash boundary in the restart test. The
            // marker and APPLIED ledger row are already durable.
            std::process::exit(23);
        }
        let result_json = serde_json::to_string(&artifact.result)?;
        self.journal
            .complete(effect_key, attempt, &result_json)
            .await?;
        if let Some(candidate) = &artifact.candidate {
            self.journal
                .save_candidate(
                    &self.run_id,
                    &candidate.candidate_sha256,
                    &candidate.manifest,
                    Some(&candidate.review),
                )
                .await?;
        }
        self.finish_callback(
            effect_key,
            attempt,
            thread_id.as_str(),
            tool_call_id,
            &result_text(&result_json),
            sender,
        )
        .await?;
        Ok(())
    }

    async fn finish_callback(
        &self,
        effect_key: &str,
        attempt: i64,
        _thread_id: &str,
        _tool_call_id: &str,
        text: &str,
        sender: &ChannelSender,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let payload = serde_json::to_string(&serde_json::json!({ "text": text }))?;
        let decision = self
            .journal
            .submit_callback(effect_key, attempt, &payload)
            .await?;
        tracing::info!(%effect_key, ?decision, "callback recorded");
        deliver_pending(
            &self.journal,
            &self.store_for_delivery(),
            &self.run_id,
            sender,
        )
        .await?;
        Ok(())
    }

    fn store_for_delivery(&self) -> SqliteStore {
        self.journal.store().clone()
    }
}

/// Deliver all pending callbacks for a run. Safe to call concurrently: each
/// callback is claimed with an atomic `PENDING -> DELIVERING` update.
pub async fn deliver_pending(
    journal: &EffectJournal,
    store: &SqliteStore,
    run_id: &str,
    sender: &ChannelSender,
) -> Result<usize, StoreError> {
    let callbacks = journal.claim_pending(run_id, 64).await?;
    let mut delivered = 0;
    for callback in callbacks {
        if store
            .is_thread_stopped(&ThreadId::from(callback.thread_id.clone()))
            .await?
        {
            journal
                .mark_callback_status(&callback.callback_id, "REJECTED_STOPPED")
                .await?;
            continue;
        }
        let text = callback_text(&callback);
        let message = tool_result_message(&callback, text);
        match sender
            .send_to_input_queue(message, &callback.callback_id)
            .await
        {
            Ok(()) => {
                journal.mark_delivered(&callback.callback_id).await?;
                delivered += 1;
            }
            Err(error) => {
                tracing::warn!(callback_id = %callback.callback_id, %error, "callback delivery failed; releasing");
                journal.release_callback(&callback.callback_id).await?;
            }
        }
    }
    Ok(delivered)
}

fn callback_text(callback: &CallbackRecord) -> String {
    let value: serde_json::Value = serde_json::from_str(&callback.payload_json)
        .unwrap_or_else(|_| serde_json::json!({ "text": callback.payload_json.clone() }));
    value
        .get("text")
        .and_then(|text| text.as_str())
        .unwrap_or(&callback.payload_json)
        .to_owned()
}

fn tool_result_message(callback: &CallbackRecord, text: String) -> InputMessage {
    InputMessage {
        content: InputMessageContent::User(UserContent::ToolResult(ToolResult {
            id: callback.tool_call_id.clone(),
            call_id: None,
            content: vec![ToolResultContent::Text(Text { text })],
        })),
        group_id: ThreadId::from(callback.thread_id.clone()),
        metadata: None,
        synthetic: None,
        display_as: None,
        subscription: false,
    }
}

fn result_text(result_json: &str) -> String {
    let value: serde_json::Value =
        serde_json::from_str(result_json).unwrap_or_else(|_| serde_json::json!(result_json));
    if let Some(text) = value.get("text").and_then(|text| text.as_str()) {
        return text.to_owned();
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| result_json.to_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub run_id: String,
    pub recovered_completed: Vec<String>,
    pub ambiguous: Vec<String>,
    pub callbacks_delivered: usize,
    pub duplicate_callbacks: usize,
    pub stale_callbacks: Vec<String>,
}

pub async fn reconcile_effects(
    journal: &EffectJournal,
    run_id: &str,
) -> Result<RecoveryReport, StoreError> {
    let mut recovered_completed = Vec::new();
    let mut ambiguous = Vec::new();
    for effect in journal.effects_for_run(run_id).await? {
        match effect.state {
            EffectState::Intent
            | EffectState::Started
            | EffectState::Applied
            | EffectState::Ambiguous => {
                let verified = effect
                    .marker_path
                    .as_deref()
                    .and_then(|path| verify_marker(path, effect.marker_sha256.as_deref()));
                match verified {
                    Some(marker) => {
                        let result = marker
                            .get("result")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!({"status":"RECOVERED"}));
                        let result_json = serde_json::to_string(&result)?;
                        journal
                            .restore_completed(&effect.effect_key, &result_json)
                            .await?;
                        if let Some(candidate) = marker.get("candidate").and_then(|v| v.as_object())
                            && let (Some(sha), Some(manifest), Some(review)) = (
                                candidate.get("candidate_sha256").and_then(|v| v.as_str()),
                                candidate.get("manifest"),
                                candidate.get("review"),
                            )
                        {
                            journal
                                .save_candidate(run_id, sha, manifest, Some(review))
                                .await?;
                        }
                        recovered_completed.push(effect.effect_key);
                    }
                    None => {
                        journal
                            .mark_ambiguous(
                                &effect.effect_key,
                                "no verifiable external marker after restart; automatic replay refused",
                            )
                            .await?;
                        ambiguous.push(effect.effect_key);
                    }
                }
            }
            _ => {}
        }
    }
    journal.reset_delivering(run_id).await?;

    // Re-materialize a callback row for each completed effect. Duplicate
    // inserts are ignored, so a crash after delivery does not re-deliver.
    let mut duplicate_callbacks = 0;
    for effect in journal.effects_for_run(run_id).await? {
        if effect.state == EffectState::Completed {
            let payload = serde_json::to_string(&serde_json::json!({
                "text": effect.result_json.clone().unwrap_or_default()
            }))?;
            match journal
                .submit_callback(&effect.effect_key, effect.attempt, &payload)
                .await?
            {
                crate::journal::CallbackDecision::Duplicate => duplicate_callbacks += 1,
                crate::journal::CallbackDecision::Stale => {
                    // An old attempt must never settle a live thread.
                }
                _ => {}
            }
        }
    }

    // Ambiguous effects never replay, but the waiting thread must not become a
    // zombie. Answer its tool call once with an explicit recovery refusal.
    for effect_key in &ambiguous {
        let effect = journal.effect(effect_key).await?.expect("effect exists");
        let payload = serde_json::to_string(&serde_json::json!({
            "text": format!(
                "Error: tool call {} ({}) has an ambiguous outcome after restart. \
                 Automatic replay was refused to avoid a duplicate external effect. \
                 An operator must reconcile the external state before retrying.",
                effect.tool_call_id, effect.tool_name
            )
        }))?;
        journal
            .submit_recovery_callback(effect_key, &payload)
            .await?;
    }
    Ok(RecoveryReport {
        run_id: run_id.to_owned(),
        recovered_completed,
        ambiguous,
        callbacks_delivered: 0,
        duplicate_callbacks,
        stale_callbacks: Vec::new(),
    })
}

fn verify_marker(path: &str, expected_sha: Option<&str>) -> Option<serde_json::Value> {
    let bytes = std::fs::read(Path::new(path)).ok()?;
    if let Some(expected) = expected_sha {
        let actual = crate::engineering::sha256_bytes(&bytes);
        if actual != expected {
            return None;
        }
    }
    let value = read_marker_result(path).ok()?;
    Some(value)
}
