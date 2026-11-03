use std::path::Path;
use std::process::Command;

use factory_infinity_experiment::effects::reconcile_effects;
use factory_infinity_experiment::engineering::sha256_bytes;
use factory_infinity_experiment::journal::{CallbackDecision, EffectJournal, EffectState};
use factory_infinity_experiment::runtime::{
    JourneyReport, PrototypeConfig, RuntimeError, run_journey,
};
use factory_infinity_experiment::store::SqliteStore;
use infinity_agent_core::ThreadId;
use infinity_agent_core::traits::ConversationStore;
use tempfile::TempDir;

#[tokio::test(flavor = "current_thread")]
async fn restart_after_review_effect_recovers_without_duplicate_effects() {
    let root = TempDir::new().expect("tempdir");
    let binary = env!("CARGO_BIN_EXE_infinity-experiment");
    let start = Command::new(binary)
        .args([
            "start",
            "--root",
            root.path().to_str().expect("utf8"),
            "--run-id",
            "restart-1",
            "--crash-after-effect",
            "review",
            "--effect-delay-ms",
            "5",
        ])
        .output()
        .expect("spawn start");
    assert_eq!(
        start.status.code(),
        Some(23),
        "expected crash boundary; stderr: {}",
        String::from_utf8_lossy(&start.stderr)
    );

    let marker_path = root.path().join("workspace/markers/review.json");
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker_path).expect("review marker")).expect("json");
    let marker_candidate = marker["candidate"]["candidate_sha256"]
        .as_str()
        .expect("marker candidate")
        .to_owned();

    let resume = Command::new(binary)
        .args([
            "resume",
            "--root",
            root.path().to_str().expect("utf8"),
            "--run-id",
            "restart-1",
            "--out",
            root.path().join("resume.json").to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("spawn resume");
    assert!(
        resume.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&resume.stderr)
    );
    let report: JourneyReport =
        serde_json::from_slice(&std::fs::read(root.path().join("resume.json")).expect("report"))
            .expect("decode report");
    assert!(report.success, "report: {report:#?}");
    let candidate = report.candidate.as_ref().expect("candidate");
    assert_eq!(candidate.candidate_sha256, marker_candidate);
    assert_eq!(report.metrics.duplicate_external_effects, 0);
    assert!(
        report
            .effects
            .iter()
            .all(|effect| effect.state == EffectState::Completed)
    );
    assert!(
        report
            .callbacks
            .iter()
            .all(|callback| callback.status == "DELIVERED")
    );
    let recovery = report.recovery.as_ref().expect("recovery report");
    assert!(recovery.ambiguous.is_empty(), "recovery: {recovery:#?}");
    assert!(!recovery.recovered_completed.is_empty());

    let log = std::fs::read_to_string(root.path().join("workspace/applied-effects.log"))
        .expect("applied effects log");
    let lines: Vec<&str> = log.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(
        lines.len(),
        3,
        "one actual application per effect: {lines:?}"
    );
    let unique: std::collections::HashSet<&str> = lines.iter().copied().collect();
    assert_eq!(unique.len(), lines.len(), "no effect may be applied twice");
}

#[tokio::test(flavor = "current_thread")]
async fn ambiguous_effect_is_not_replayed_and_accepts_no_callback() {
    let root = TempDir::new().expect("tempdir");
    let store = SqliteStore::open(root.path().join("db.sqlite"))
        .await
        .expect("store");
    let journal = EffectJournal::new(store.clone());
    let root_id = ThreadId::from("root-ambiguous");
    store.ensure_root_thread(&root_id).await.expect("root");
    journal
        .start_run("ambiguous", "objective", root_id.as_str())
        .await
        .expect("run");
    let (key, outcome) = journal
        .begin_effect(
            "ambiguous",
            root_id.as_str(),
            "call-1",
            "engineering_task",
            "{}",
        )
        .await
        .expect("begin");
    assert!(matches!(
        outcome,
        factory_infinity_experiment::journal::BeginOutcome::New { .. }
    ));
    let effect = journal.effect(&key).await.expect("effect").expect("row");
    assert!(
        journal
            .mark_started(
                &key,
                effect.attempt,
                effect.lease_token.as_deref().expect("lease")
            )
            .await
            .expect("started")
    );

    let recovery = reconcile_effects(&journal, "ambiguous")
        .await
        .expect("reconcile");
    assert_eq!(recovery.ambiguous, vec![key.clone()]);
    let effect = journal.effect(&key).await.expect("effect").expect("row");
    assert_eq!(effect.state, EffectState::Ambiguous);
    let callbacks = journal
        .callbacks_for_run("ambiguous")
        .await
        .expect("callbacks");
    assert_eq!(callbacks.len(), 1, "ambiguous call must be settled once");
    assert_eq!(callbacks[0].status, "PENDING");
    assert!(
        callbacks[0].payload_json.contains("ambiguous outcome"),
        "settlement must explain the refusal: {}",
        callbacks[0].payload_json
    );
    let decision = journal
        .submit_callback(&key, effect.attempt, "{\"text\":\"late\"}")
        .await
        .expect("submit");
    assert_eq!(decision, CallbackDecision::Rejected);
    assert_eq!(
        journal
            .callbacks_for_run("ambiguous")
            .await
            .expect("callbacks")
            .len(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn verified_marker_reconciles_to_completed_and_enqueues_once() {
    let root = TempDir::new().expect("tempdir");
    let store = SqliteStore::open(root.path().join("db.sqlite"))
        .await
        .expect("store");
    let journal = EffectJournal::new(store.clone());
    let root_id = ThreadId::from("root-marker");
    store.ensure_root_thread(&root_id).await.expect("root");
    journal
        .start_run("marker", "objective", root_id.as_str())
        .await
        .expect("run");
    let (key, _) = journal
        .begin_effect("marker", root_id.as_str(), "call-1", "tool", "{}")
        .await
        .expect("begin");
    let effect = journal.effect(&key).await.expect("effect").expect("row");
    journal
        .mark_started(
            &key,
            effect.attempt,
            effect.lease_token.as_deref().expect("lease"),
        )
        .await
        .expect("started");
    let marker_path = root.path().join("marker.json");
    let marker = serde_json::json!({
        "result": {"status": "PASS", "value": 42},
        "candidate": null
    });
    let bytes = serde_json::to_vec(&marker).expect("marker bytes");
    std::fs::write(&marker_path, &bytes).expect("marker write");
    let recorded = journal
        .mark_applied(
            &key,
            effect.attempt,
            effect.lease_token.as_deref().expect("lease"),
            marker_path.to_str().expect("utf8"),
            &sha256_bytes(&bytes),
        )
        .await
        .expect("applied");
    assert!(recorded);

    let recovery = reconcile_effects(&journal, "marker")
        .await
        .expect("reconcile");
    assert_eq!(recovery.recovered_completed, vec![key.clone()]);
    assert!(recovery.ambiguous.is_empty());
    let effect = journal.effect(&key).await.expect("effect").expect("row");
    assert_eq!(effect.state, EffectState::Completed);
    let callbacks = journal
        .callbacks_for_run("marker")
        .await
        .expect("callbacks");
    assert_eq!(callbacks.len(), 1);
    assert_eq!(callbacks[0].status, "PENDING");

    let claimed = journal.claim_pending("marker", 10).await.expect("claim");
    assert_eq!(claimed.len(), 1);
    journal
        .mark_delivered(&claimed[0].callback_id)
        .await
        .expect("delivered");
    assert!(
        journal
            .claim_pending("marker", 10)
            .await
            .expect("claim again")
            .is_empty()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_and_stale_callbacks_are_fenced() {
    let root = TempDir::new().expect("tempdir");
    let store = SqliteStore::open(root.path().join("db.sqlite"))
        .await
        .expect("store");
    let journal = EffectJournal::new(store.clone());
    let root_id = ThreadId::from("root-callbacks");
    store.ensure_root_thread(&root_id).await.expect("root");
    journal
        .start_run("callbacks", "objective", root_id.as_str())
        .await
        .expect("run");
    let (key, _) = journal
        .begin_effect("callbacks", root_id.as_str(), "call-1", "tool", "{}")
        .await
        .expect("begin");
    let effect = journal.effect(&key).await.expect("effect").expect("row");
    journal
        .mark_started(
            &key,
            effect.attempt,
            effect.lease_token.as_deref().expect("lease"),
        )
        .await
        .expect("started");
    journal
        .complete(&key, effect.attempt, "{\"status\":\"PASS\"}")
        .await
        .expect("complete");

    let mut handles = Vec::new();
    for _ in 0..32 {
        let journal = journal.clone();
        let key = key.clone();
        handles.push(tokio::spawn(async move {
            journal
                .submit_callback(&key, 1, "{\"text\":\"result\"}")
                .await
                .expect("submit")
        }));
    }
    let mut accepted = 0;
    let mut duplicate = 0;
    for handle in handles {
        match handle.await.expect("join") {
            CallbackDecision::Accepted => accepted += 1,
            CallbackDecision::Duplicate => duplicate += 1,
            other => panic!("unexpected decision: {other:?}"),
        }
    }
    assert_eq!(accepted, 1);
    assert_eq!(duplicate, 31);
    let stale = journal
        .submit_callback(&key, 2, "{\"text\":\"stale\"}")
        .await
        .expect("stale");
    assert_eq!(stale, CallbackDecision::Stale);
    assert_eq!(
        journal
            .callbacks_for_run("callbacks")
            .await
            .expect("callbacks")
            .len(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn fresh_children_have_independent_reconstructable_contexts() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let root = TempDir::new().expect("tempdir");
            let mut config = PrototypeConfig::fixture(root.path(), "contexts");
            config.effect_delay = std::time::Duration::from_millis(5);
            let report = run_journey(&config).await.expect("journey");
            assert!(report.success, "report: {report:#?}");
            let store = SqliteStore::open(root.path().join("factory.sqlite"))
                .await
                .expect("store");
            let root_id = ThreadId::from(report.root_thread_id.clone());
            let children = store.children(&root_id).await.expect("children");
            assert_eq!(children.len(), 3);
            let expected = ["implement-multiply", "implement-tests", "review"];
            for (index, child) in children.into_iter().enumerate() {
                let info = store.thread_info(&child).await.expect("info").expect("row");
                assert!(info.fresh_context);
                assert_eq!(
                    info.parent_thread_id.as_ref().map(|id| id.as_str()),
                    Some(root_id.as_str())
                );
                let ancestors = store.get_ancestor_chain(&child).await.expect("ancestors");
                assert_eq!(ancestors.len(), 1);
                assert_eq!(ancestors[0].0, root_id);
                let history = store
                    .load_history_with_ancestors(&child)
                    .await
                    .expect("history");
                let text = format!("{history:?}");
                assert!(
                    text.contains(&format!("TASK={}", expected[index])),
                    "child must contain its own task {}: {text}",
                    expected[index]
                );
                for other in expected.iter().filter(|task| **task != expected[index]) {
                    assert!(
                        !text.contains(&format!("TASK={other}")),
                        "fresh child must not inherit sibling task {other}: {text}"
                    );
                }
                assert!(
                    !text.contains("Implement and independently verify multiply()"),
                    "fresh child must not inherit the Director objective"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_prevents_unsettled_effects_and_callbacks() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let root = TempDir::new().expect("tempdir");
            let mut config = PrototypeConfig::fixture(root.path(), "cancel");
            config.effect_delay = std::time::Duration::from_millis(500);
            config.cancel_after = Some(std::time::Duration::from_millis(10));
            config.cancel_after_effect_start = true;
            let error = run_journey(&config).await.expect_err("must cancel");
            assert!(matches!(error, RuntimeError::Cancelled), "error: {error:?}");
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;

            let store = SqliteStore::open(root.path().join("factory.sqlite"))
                .await
                .expect("store");
            let journal = EffectJournal::new(store.clone());
            assert_eq!(
                journal
                    .run_status("cancel")
                    .await
                    .expect("status")
                    .as_deref(),
                Some("CANCELLED")
            );
            let effects = journal.effects_for_run("cancel").await.expect("effects");
            assert!(!effects.is_empty());
            assert!(effects.iter().all(|effect| matches!(
                effect.state,
                EffectState::Cancelled | EffectState::Completed | EffectState::Intent
            )));
            let callbacks = journal
                .callbacks_for_run("cancel")
                .await
                .expect("callbacks");
            assert!(
                callbacks
                    .iter()
                    .all(|callback| callback.status != "DELIVERED")
            );
            let log_path = Path::new(&config.workspace_root).join("applied-effects.log");
            if log_path.exists() {
                let log = std::fs::read_to_string(log_path).expect("log");
                assert!(
                    log.trim().is_empty(),
                    "cancelled effects must not apply: {log}"
                );
            }
        })
        .await;
}
