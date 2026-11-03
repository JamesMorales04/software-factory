use std::process::Command;

use factory_infinity_experiment::engineering::SandboxMode;
use factory_infinity_experiment::journal::EffectState;
use factory_infinity_experiment::runtime::{PrototypeConfig, run_journey};
use tempfile::TempDir;

/// Optional Podman evidence. It is skipped (with an explicit message) when the
/// host has no Podman or the approved fixture image is absent, so the test suite
/// remains runnable on hosts without the engine. It is never reported as an
/// isolation certification.
#[tokio::test(flavor = "current_thread")]
async fn podman_mode_runs_the_fixture_when_the_image_is_available() {
    let image = std::env::var("FACTORY_INFINITY_PODMAN_IMAGE")
        .unwrap_or_else(|_| "localhost/software-factory-runtime:dev".to_owned());
    let available = Command::new("podman")
        .args(["image", "exists", &image])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !available {
        eprintln!("SKIP: podman image {image} is not available");
        return;
    }

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let root = TempDir::new().expect("tempdir");
            let mut config = PrototypeConfig::fixture(root.path(), "podman");
            config.effect_delay = std::time::Duration::from_millis(5);
            config.sandbox = SandboxMode::Podman {
                image: image.clone(),
            };
            let report = run_journey(&config).await.expect("podman journey");
            assert!(report.success, "report: {report:#?}");
            assert!(
                report
                    .effects
                    .iter()
                    .all(|effect| effect.state == EffectState::Completed)
            );
            let candidate = report.candidate.expect("candidate");
            assert_eq!(
                candidate
                    .review
                    .as_ref()
                    .and_then(|review| review.get("verdict"))
                    .and_then(|verdict| verdict.as_str()),
                Some("PASS")
            );
            let marker = std::fs::read_to_string(root.path().join("workspace/markers/review.json"))
                .expect("review marker");
            assert!(
                marker.contains("ROOTLESS_PODMAN_UNCERTIFIED"),
                "marker must record the actual execution mode: {marker}"
            );
        })
        .await;
}
