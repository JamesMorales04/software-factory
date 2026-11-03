use factory_infinity_experiment::runtime::{PrototypeConfig, run_journey};
use tempfile::TempDir;

#[tokio::test(flavor = "current_thread")]
async fn deterministic_director_journey_reaches_candidate() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let _ = tracing_subscriber::fmt()
                .with_env_filter("info,infinity_agent_core=debug")
                .with_test_writer()
                .try_init();
            let root = TempDir::new().expect("tempdir");
            let mut config = PrototypeConfig::fixture(root.path(), "journey-1");
            config.effect_delay = std::time::Duration::from_millis(10);
            let report = run_journey(&config).await.expect("journey");
            assert!(report.success, "report: {report:#?}");
            assert!(report.candidate.is_some());
            assert_eq!(report.effects.len(), 3, "two implementers and one reviewer");
            assert!(report.effects.iter().all(|effect| matches!(
                effect.state,
                factory_infinity_experiment::journal::EffectState::Completed
            )));
            assert_eq!(report.metrics.duplicate_external_effects, 0);
            assert!(report.metrics.first_child_ms.is_some());
            let candidate = report.candidate.expect("candidate");
            assert_eq!(
                candidate
                    .review
                    .as_ref()
                    .and_then(|review| review.get("verdict"))
                    .and_then(|v| v.as_str()),
                Some("PASS")
            );
            assert!(
                report
                    .root_history
                    .iter()
                    .any(|message| format!("{message:?}").contains("DIRECTOR COMPLETE"))
            );
        })
        .await;
}
