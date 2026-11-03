//! Executable Director/implementer/reviewer journey on Infinity Runtime.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use infinity_agent_core::ThreadId;
use infinity_agent_core::message::InfinityMessage;
use infinity_agent_core::system::local::{RunningSystem, ThreadLifecycleEvent};
use infinity_agent_core::system::{AgentEvent, AgentSystemBuilder, ReplaySnapshot, ThreadObserver};
use infinity_agent_core::traits::ConversationStore;
use infinity_provider_protocol::message::{AssistantContent, UserContent};
use serde::{Deserialize, Serialize};

use crate::effects::{
    CrashPolicy, EngineeringTaskTool, RecoveryReport, deliver_pending, reconcile_effects,
};
use crate::engineering::EngineeringWorkspace;
use crate::journal::{CallbackRecord, CandidateRecord, EffectJournal, EffectRecord};
use crate::model::{JourneyPlan, ScriptedModelSource};
use crate::store::{SqliteStore, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("durable story: {0}")]
    Durability(String),
    #[error("journey timed out after {0:?}")]
    Timeout(Duration),
    #[error("journey cancelled")]
    Cancelled,
    #[error("run not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone)]
pub struct PrototypeConfig {
    pub db_path: PathBuf,
    pub workspace_root: PathBuf,
    pub run_id: String,
    pub objective: String,
    pub plan: JourneyPlan,
    pub crash: CrashPolicy,
    pub effect_delay: Duration,
    pub model_delay: Duration,
    pub timeout: Duration,
    /// Test-only deterministic cancellation point. Production cancellation is
    /// requested explicitly, not by elapsed time.
    pub cancel_after: Option<Duration>,
    /// When true, the test cancellation point additionally waits for at least
    /// one effect to exist, making in-flight cancellation deterministic.
    pub cancel_after_effect_start: bool,
}

impl PrototypeConfig {
    pub fn fixture(root: impl Into<PathBuf>, run_id: impl Into<String>) -> Self {
        let root = root.into();
        Self {
            db_path: root.join("factory.sqlite"),
            workspace_root: root.join("workspace"),
            run_id: run_id.into(),
            objective: "Implement and independently verify multiply() in a disposable repository."
                .to_owned(),
            plan: JourneyPlan::default_calculator(),
            crash: CrashPolicy::None,
            effect_delay: Duration::from_millis(40),
            model_delay: Duration::ZERO,
            timeout: Duration::from_secs(30),
            cancel_after: None,
            cancel_after_effect_start: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub thread_id: ThreadId,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JourneyMetrics {
    pub wall_ms: u128,
    pub first_child_ms: Option<u128>,
    pub peak_rss_kib: u64,
    pub cpu_user_ms: u128,
    pub cpu_system_ms: u128,
    pub processes: u32,
    pub threads: u32,
    pub callback_queue_latency_ms: Option<f64>,
    pub effect_to_callback_ms: Option<f64>,
    pub restart_recovery_ms: Option<u128>,
    pub duplicate_external_effects: u32,
    pub event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JourneyReport {
    pub run_id: String,
    pub root_thread_id: String,
    pub objective: String,
    pub success: bool,
    pub candidate: Option<CandidateRecord>,
    pub effects: Vec<EffectRecord>,
    pub callbacks: Vec<CallbackRecord>,
    pub events: Vec<EventRecord>,
    pub root_history: Vec<InfinityMessage>,
    pub metrics: JourneyMetrics,
    pub recovery: Option<RecoveryReport>,
}

#[derive(Clone, Default)]
struct ObserverHub {
    events: Arc<Mutex<Vec<EventRecord>>>,
}

struct RecordingObserver {
    hub: ObserverHub,
    thread_id: ThreadId,
}

#[async_trait::async_trait(?Send)]
impl ThreadObserver for RecordingObserver {
    type SubscribeRequest = ();

    fn on_event(&self, thread_id: &ThreadId<str>, event: &AgentEvent) {
        self.hub
            .events
            .lock()
            .expect("bug: event lock")
            .push(EventRecord {
                thread_id: thread_id.to_owned(),
                kind: event_kind(event).to_owned(),
                payload: event_payload(event),
            });
        let _ = &self.thread_id;
    }

    fn on_subscribe(
        &self,
        _thread_id: &ThreadId<str>,
        _request: Self::SubscribeRequest,
        _snapshot: ReplaySnapshot,
    ) {
    }
}

pub async fn run_journey(config: &PrototypeConfig) -> Result<JourneyReport, RuntimeError> {
    let started = Instant::now();
    let store = SqliteStore::open(&config.db_path).await?;
    let journal = EffectJournal::new(store.clone());
    let engineering = Arc::new(EngineeringWorkspace::new(
        &config.workspace_root,
        Arc::new(config.plan.clone()),
    ));
    engineering
        .initialize()
        .await
        .map_err(|error| RuntimeError::Durability(error.to_string()))?;
    let root_id = ThreadId::from(format!("director-{}", config.run_id));
    journal
        .start_run(&config.run_id, &config.objective, root_id.as_str())
        .await?;
    store.ensure_root_thread(&root_id).await?;

    let hub = ObserverHub::default();
    let mut system = build_system(
        config,
        &store,
        &journal,
        &engineering,
        &root_id,
        hub.clone(),
    );
    system
        .send_user_text(&root_id, config.objective.clone())
        .await;

    let wait = wait_for_director(
        config,
        &store,
        &journal,
        &mut system,
        &root_id,
        &hub,
        started,
    )
    .await;

    let _ = system.shutdown().await;
    let (root_history, metrics) = wait?;
    persist_events(&store, &config.run_id, &hub).await?;
    let effects = journal.effects_for_run(&config.run_id).await?;
    let callbacks = journal.callbacks_for_run(&config.run_id).await?;
    let candidate = journal.candidate(&config.run_id).await?;
    let success = candidate
        .as_ref()
        .is_some_and(|candidate| review_verdict(candidate) == Some("PASS"))
        && effects
            .iter()
            .all(|effect| matches!(effect.state, crate::journal::EffectState::Completed));
    journal
        .set_run_status(&config.run_id, if success { "COMPLETED" } else { "FAILED" })
        .await?;
    let mut metrics = metrics;
    metrics.wall_ms = started.elapsed().as_millis();
    metrics.duplicate_external_effects = duplicate_effects(&effects);
    Ok(JourneyReport {
        run_id: config.run_id.clone(),
        root_thread_id: root_id.as_str().to_owned(),
        objective: config.objective.clone(),
        success,
        candidate,
        effects,
        callbacks,
        events: hub.events.lock().expect("bug: event lock").clone(),
        root_history,
        metrics,
        recovery: None,
    })
}

pub async fn resume_journey(config: &PrototypeConfig) -> Result<JourneyReport, RuntimeError> {
    let started = Instant::now();
    let store = SqliteStore::open(&config.db_path).await?;
    let journal = EffectJournal::new(store.clone());
    let root_id = ThreadId::from(format!("director-{}", config.run_id));
    if journal.run_status(&config.run_id).await?.is_none() {
        return Err(RuntimeError::NotFound(config.run_id.clone()));
    }
    let engineering = Arc::new(EngineeringWorkspace::new(
        &config.workspace_root,
        Arc::new(config.plan.clone()),
    ));
    engineering
        .initialize()
        .await
        .map_err(|error| RuntimeError::Durability(error.to_string()))?;

    let mut recovery = reconcile_effects(&journal, &config.run_id).await?;
    let hub = ObserverHub::default();
    let mut system = build_system(
        config,
        &store,
        &journal,
        &engineering,
        &root_id,
        hub.clone(),
    );
    deliver_pending(&journal, &store, &config.run_id, &system.sender()).await?;
    let history_before = store.load_history_up_to(&root_id, None, None).await?;
    let already_final = latest_assistant_text(&history_before)
        .is_some_and(|text| text.contains("DIRECTOR COMPLETE"));
    if !already_final {
        system
            .send_user_text(
                &root_id,
                "Recovery: continue from durable state. Do not repeat completed external effects.",
            )
            .await;
    }
    let wait = wait_for_director(
        config,
        &store,
        &journal,
        &mut system,
        &root_id,
        &hub,
        started,
    )
    .await;
    let _ = system.shutdown().await;
    let (root_history, metrics) = wait?;
    persist_events(&store, &config.run_id, &hub).await?;
    let effects = journal.effects_for_run(&config.run_id).await?;
    let callbacks = journal.callbacks_for_run(&config.run_id).await?;
    recovery.callbacks_delivered = callbacks
        .iter()
        .filter(|callback| callback.status == "DELIVERED")
        .count();
    let candidate = journal.candidate(&config.run_id).await?;
    let success = candidate
        .as_ref()
        .is_some_and(|candidate| review_verdict(candidate) == Some("PASS"))
        && effects
            .iter()
            .all(|effect| matches!(effect.state, crate::journal::EffectState::Completed));
    journal
        .set_run_status(&config.run_id, if success { "COMPLETED" } else { "FAILED" })
        .await?;
    let mut metrics = metrics;
    metrics.wall_ms = started.elapsed().as_millis();
    metrics.restart_recovery_ms = Some(started.elapsed().as_millis());
    metrics.duplicate_external_effects = duplicate_effects(&effects);
    Ok(JourneyReport {
        run_id: config.run_id.clone(),
        root_thread_id: root_id.as_str().to_owned(),
        objective: config.objective.clone(),
        success,
        candidate,
        effects,
        callbacks,
        events: hub.events.lock().expect("bug: event lock").clone(),
        root_history,
        metrics,
        recovery: Some(recovery),
    })
}

/// Cancel a run's live agents and refuse its unsettled effects.
pub async fn cancel_journey(
    store: &SqliteStore,
    journal: &EffectJournal,
    run_id: &str,
    system: &RunningSystem<()>,
) -> Result<(), RuntimeError> {
    let root_id = ThreadId::from(format!("director-{run_id}"));
    cancel_internal(store, journal, run_id, &root_id, system).await
}

async fn cancel_internal(
    store: &SqliteStore,
    journal: &EffectJournal,
    run_id: &str,
    root_id: &ThreadId,
    system: &RunningSystem<()>,
) -> Result<(), RuntimeError> {
    journal.request_cancel(run_id).await?;
    let subtree = store.subtree(root_id).await?;
    for thread_id in subtree {
        store.mark_stopped(&thread_id).await?;
        system
            .send_user_text(&thread_id, "[factory-cancel] stop this run")
            .await;
    }
    for effect in journal.effects_for_run(run_id).await? {
        if matches!(
            effect.state,
            crate::journal::EffectState::Intent | crate::journal::EffectState::Started
        ) {
            journal.cancel_effect(&effect.effect_key).await?;
        }
    }
    journal.set_run_status(run_id, "CANCELLED").await?;
    Ok(())
}

fn build_system(
    config: &PrototypeConfig,
    store: &SqliteStore,
    journal: &EffectJournal,
    engineering: &Arc<EngineeringWorkspace>,
    root_id: &ThreadId,
    hub: ObserverHub,
) -> RunningSystem<()> {
    let model = ScriptedModelSource::new(
        store.clone(),
        Arc::new(config.plan.clone()),
        root_id.clone(),
        config.model_delay,
    );
    let tool = EngineeringTaskTool {
        journal: journal.clone(),
        engineering: engineering.clone(),
        plan: Arc::new(config.plan.clone()),
        run_id: config.run_id.clone(),
        effect_delay: config.effect_delay,
        crash: config.crash.clone(),
        confinement: crate::effects::ConfinementMode::LocalProcessFixedAllowlistNotCertified,
    };
    AgentSystemBuilder::new_local(store.clone(), store.clone(), model)
        .tool(Box::new(tool))
        .extra_system_prompt(
            "You are an Infinity-based Factory agent. Delegate independent work with fresh_context=true. \
             Never claim a test passed without a tool result. External effects are journaled; do not repeat them.",
        )
        .start_with_observer(move |thread_id| RecordingObserver {
            hub: hub.clone(),
            thread_id: thread_id.to_owned(),
        })
}

async fn wait_for_director(
    config: &PrototypeConfig,
    store: &SqliteStore,
    journal: &EffectJournal,
    system: &mut RunningSystem<()>,
    root_id: &ThreadId,
    hub: &ObserverHub,
    started: Instant,
) -> Result<(Vec<InfinityMessage>, JourneyMetrics), RuntimeError> {
    let deadline = tokio::time::Instant::now() + config.timeout;
    let mut first_child_ms = None;
    let mut peak_rss_kib = 0;
    let mut lifecycle_events = Vec::new();
    loop {
        let sample = sample_process();
        peak_rss_kib = peak_rss_kib.max(sample.rss_kib);
        let children = store.children(root_id).await?;
        if first_child_ms.is_none() && !children.is_empty() {
            first_child_ms = Some(started.elapsed().as_millis());
        }
        while let Ok(event) = system.try_next_lifecycle_event() {
            lifecycle_events.push(event);
        }
        deliver_pending(journal, store, &config.run_id, &system.sender()).await?;
        let effects = journal.effects_for_run(&config.run_id).await?;
        if let Some(cancel_after) = config.cancel_after
            && started.elapsed() >= cancel_after
            && (!config.cancel_after_effect_start || !effects.is_empty())
        {
            cancel_internal(store, journal, &config.run_id, root_id, system).await?;
        }
        if journal.is_cancelled(&config.run_id).await? {
            return Err(RuntimeError::Cancelled);
        }
        let history = store.load_history_up_to(root_id, None, None).await?;
        let final_text = latest_assistant_text(&history);
        let callbacks = journal.callbacks_for_run(&config.run_id).await?;
        let all_settled = effects.iter().all(|effect| {
            matches!(
                effect.state,
                crate::journal::EffectState::Completed
                    | crate::journal::EffectState::Ambiguous
                    | crate::journal::EffectState::Failed
                    | crate::journal::EffectState::Cancelled
            )
        });
        let no_pending_callbacks = callbacks
            .iter()
            .all(|callback| callback.status != "PENDING" && callback.status != "DELIVERING");
        let candidate = journal.candidate(&config.run_id).await?;
        if final_text.is_some_and(|text| text.contains("DIRECTOR COMPLETE"))
            && all_settled
            && no_pending_callbacks
            && candidate.is_some()
        {
            let sample = sample_process();
            peak_rss_kib = peak_rss_kib.max(sample.rss_kib);
            let mut events = hub.events.lock().expect("bug: event lock").clone();
            for event in lifecycle_events {
                events.push(lifecycle_event_record(event));
            }
            let metrics = JourneyMetrics {
                wall_ms: started.elapsed().as_millis(),
                first_child_ms,
                peak_rss_kib,
                cpu_user_ms: sample.user_ms,
                cpu_system_ms: sample.system_ms,
                processes: 1,
                threads: sample.threads,
                callback_queue_latency_ms: callback_latency_ms(&callbacks),
                effect_to_callback_ms: effect_to_callback_ms(&effects, &callbacks),
                restart_recovery_ms: None,
                duplicate_external_effects: duplicate_effects(&effects),
                event_count: events.len(),
            };
            // Persist lifecycle events for the durable evidence trail.
            for event in &events {
                store
                    .append_event(
                        &config.run_id,
                        &event.thread_id,
                        &event.kind,
                        &event.payload,
                    )
                    .await?;
            }
            return Ok((history, metrics));
        }
        if tokio::time::Instant::now() >= deadline {
            let effects = journal.effects_for_run(&config.run_id).await?;
            let callbacks = journal.callbacks_for_run(&config.run_id).await?;
            let children = store.children(root_id).await?;
            let history = store.load_history_up_to(root_id, None, None).await?;
            tracing::error!(
                children = children.len(),
                effects = ?effects.iter().map(|effect| (&effect.effect_key, effect.state)).collect::<Vec<_>>(),
                callbacks = ?callbacks.iter().map(|callback| (&callback.callback_id, &callback.status)).collect::<Vec<_>>(),
                latest = latest_assistant_text(&history),
                "journey timed out"
            );
            let info_events: Vec<_> = hub
                .events
                .lock()
                .expect("bug: event lock")
                .iter()
                .filter(|event| event.kind == "info")
                .map(|event| event.payload.clone())
                .collect();
            eprintln!(
                "journey timeout state: children={} effects={:?} callbacks={:?} latest={:?} info={info_events:?}",
                children.len(),
                effects
                    .iter()
                    .map(|effect| (effect.effect_key.clone(), effect.state))
                    .collect::<Vec<_>>(),
                callbacks
                    .iter()
                    .map(|callback| (callback.callback_id.clone(), callback.status.clone()))
                    .collect::<Vec<_>>(),
                latest_assistant_text(&history)
            );
            return Err(RuntimeError::Timeout(config.timeout));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn persist_events(
    store: &SqliteStore,
    run_id: &str,
    hub: &ObserverHub,
) -> Result<(), RuntimeError> {
    let events = hub.events.lock().expect("bug: event lock").clone();
    for event in events {
        store
            .append_event(run_id, &event.thread_id, &event.kind, &event.payload)
            .await?;
    }
    Ok(())
}

fn lifecycle_event_record(event: ThreadLifecycleEvent) -> EventRecord {
    EventRecord {
        thread_id: event.thread_id,
        kind: format!("lifecycle.{:?}", event.state).to_ascii_lowercase(),
        payload: serde_json::json!({ "state": format!("{:?}", event.state) }),
    }
}

fn latest_assistant_text(history: &[InfinityMessage]) -> Option<&str> {
    history.iter().rev().find_map(|message| match message {
        InfinityMessage::Assistant {
            content: AssistantContent::Text(text),
        } => Some(text.text.as_str()),
        _ => None,
    })
}

fn review_verdict(candidate: &CandidateRecord) -> Option<&str> {
    candidate
        .review
        .as_ref()
        .and_then(|review| review.get("verdict"))
        .and_then(|verdict| verdict.as_str())
}

fn duplicate_effects(effects: &[EffectRecord]) -> u32 {
    let mut completed = std::collections::BTreeMap::new();
    for effect in effects
        .iter()
        .filter(|effect| effect.state == crate::journal::EffectState::Completed)
    {
        if let Some(marker) = &effect.marker_path {
            *completed.entry(marker.clone()).or_insert(0u32) += 1;
        }
    }
    completed.values().filter(|count| **count > 1).count() as u32
}

fn callback_latency_ms(callbacks: &[CallbackRecord]) -> Option<f64> {
    let mut values = Vec::new();
    for callback in callbacks {
        if callback.status != "DELIVERED" {
            continue;
        }
        if let Some(value) = callback_duration(callback) {
            values.push(value);
        }
    }
    average(&values)
}

fn callback_duration(callback: &CallbackRecord) -> Option<f64> {
    let received = chrono::DateTime::parse_from_rfc3339(&callback.received_at).ok()?;
    let delivered = chrono::DateTime::parse_from_rfc3339(callback.delivered_at.as_deref()?).ok()?;
    let millis = (delivered - received).num_microseconds()? as f64 / 1_000.0;
    (millis >= 0.0).then_some(millis)
}

fn effect_to_callback_ms(effects: &[EffectRecord], callbacks: &[CallbackRecord]) -> Option<f64> {
    let effects: std::collections::HashMap<&str, &EffectRecord> = effects
        .iter()
        .map(|effect| (effect.effect_key.as_str(), effect))
        .collect();
    let mut values = Vec::new();
    for callback in callbacks {
        let Some(effect) = effects.get(callback.effect_key.as_str()) else {
            continue;
        };
        let Ok(applied) = chrono::DateTime::parse_from_rfc3339(&effect.updated_at) else {
            continue;
        };
        let Ok(received) = chrono::DateTime::parse_from_rfc3339(&callback.received_at) else {
            continue;
        };
        let millis = (received - applied).num_microseconds()? as f64 / 1_000.0;
        if millis >= 0.0 {
            values.push(millis);
        }
    }
    average(&values)
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

#[derive(Debug, Clone, Copy)]
struct ProcSample {
    rss_kib: u64,
    user_ms: u128,
    system_ms: u128,
    threads: u32,
}

fn sample_process() -> ProcSample {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mut rss_kib = 0;
    let mut threads = 1;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            rss_kib = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
        }
        if let Some(value) = line.strip_prefix("Threads:") {
            threads = value.trim().parse().unwrap_or(1);
        }
    }
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.split_whitespace().collect();
    let ticks_per_second = 100.0_f64;
    let user_ticks = fields
        .get(13)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let system_ticks = fields
        .get(14)
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    ProcSample {
        rss_kib,
        user_ms: (user_ticks / ticks_per_second * 1000.0) as u128,
        system_ms: (system_ticks / ticks_per_second * 1000.0) as u128,
        threads,
    }
}

fn event_kind(event: &AgentEvent) -> &'static str {
    match event {
        AgentEvent::UserInput { .. } => "user_input",
        AgentEvent::ToolResult { .. } => "tool_result",
        AgentEvent::SubscriptionEvent { .. } => "subscription_event",
        AgentEvent::OAuthRequired { .. } => "oauth_required",
        AgentEvent::CompactionApplied => "compaction_applied",
        AgentEvent::CompletionStarted => "completion_started",
        AgentEvent::TextChunk { .. } => "text_chunk",
        AgentEvent::ThinkingStarted => "thinking_started",
        AgentEvent::ThinkingChunk { .. } => "thinking_chunk",
        AgentEvent::ThinkingEnded => "thinking_ended",
        AgentEvent::ToolCall { .. } => "tool_call",
        AgentEvent::CompletionFinished { .. } => "completion_finished",
        AgentEvent::UserChoiceRequired { .. } => "user_choice_required",
        AgentEvent::UserChoiceDismissed { .. } => "user_choice_dismissed",
        AgentEvent::Info { .. } => "info",
    }
}

fn event_payload(event: &AgentEvent) -> serde_json::Value {
    match event {
        AgentEvent::UserInput { text } => serde_json::json!({ "text": text }),
        AgentEvent::ToolResult { segments } => {
            serde_json::json!({ "segments": format!("{segments:?}") })
        }
        AgentEvent::SubscriptionEvent { name, text } => {
            serde_json::json!({ "name": name, "text": text })
        }
        AgentEvent::OAuthRequired { auth_url } => {
            serde_json::json!({ "auth_url": auth_url })
        }
        AgentEvent::CompactionApplied => serde_json::json!({}),
        AgentEvent::CompletionStarted => serde_json::json!({}),
        AgentEvent::TextChunk { text } => serde_json::json!({ "text": text }),
        AgentEvent::ThinkingStarted | AgentEvent::ThinkingEnded => serde_json::json!({}),
        AgentEvent::ThinkingChunk { text } => serde_json::json!({ "text": text }),
        AgentEvent::ToolCall {
            name,
            args,
            display_as,
        } => serde_json::json!({ "name": name, "args": args, "display_as": display_as }),
        AgentEvent::CompletionFinished { usage } => serde_json::json!({ "usage": usage }),
        AgentEvent::UserChoiceRequired { choice } => {
            serde_json::json!({ "choice_id": choice.id, "prompt": choice.prompt })
        }
        AgentEvent::UserChoiceDismissed { choice_id } => {
            serde_json::json!({ "choice_id": choice_id })
        }
        AgentEvent::Info { text } => serde_json::json!({ "text": text }),
    }
}

/// Convenience for tests that only need a thread's model-visible tool results.
pub fn tool_result_texts(history: &[InfinityMessage]) -> Vec<String> {
    history
        .iter()
        .filter_map(|message| match message {
            InfinityMessage::ToolResult { result, .. } => Some(
                result
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        infinity_provider_protocol::message::ToolResultContent::Text(text) => {
                            Some(text.text.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            InfinityMessage::User {
                content: UserContent::ToolResult(result),
            } => Some(
                result
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        infinity_provider_protocol::message::ToolResultContent::Text(text) => {
                            Some(text.text.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .collect()
}
