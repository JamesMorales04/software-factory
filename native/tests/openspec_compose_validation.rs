//! Contract test for the Compose OpenSpec validation job.
//!
//! The committed fixture is the exact Core read model consumed by
//! `deploy/openspec/validate.mjs`. This test re-renders the same change through
//! the deterministic Core renderer and fails if the fixture drifts.
//!
//! The second half writes the files into an isolated workspace and drives the
//! pinned-version validation flow through a deterministic fake runner. It never
//! starts Node, Podman or a model provider.

use std::path::Path;

use factory_native::openspec::{
    OpenSpecArtifacts, OpenSpecChange, OpenSpecReadModel, OpenSpecRequirement, OpenSpecTask,
    archive_outcome, parse_stored_artifact, read_model, render,
};
use factory_native::openspec_cli::{OpenSpecCliError, OpenSpecRunner, validate_with_pinned_cli};
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("../../deploy/openspec/fixtures/openspec-read-model.json");

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sample_change() -> OpenSpecChange {
    OpenSpecChange {
        change_id: "add-bounded-request-validation".into(),
        title: "Add bounded request validation".into(),
        why: "The request boundary accepts malformed identifiers and unbounded payloads.".into(),
        what_changes: vec![
            "Validate request identifiers and payload bounds at the boundary.".into(),
            "Return a typed error instead of a partial request.".into(),
        ],
        non_goals: vec!["Do not change the public request schema.".into()],
        requirements: vec![
            OpenSpecRequirement {
                id: "REQ-001".into(),
                statement: "The request boundary SHALL reject malformed identifiers.".into(),
                acceptance: vec!["A malformed identifier returns a typed error.".into()],
                negative: vec!["A valid identifier is preserved unchanged.".into()],
            },
            OpenSpecRequirement {
                id: "REQ-002".into(),
                statement: "The request boundary SHALL reject unbounded payloads.".into(),
                acceptance: vec!["An oversized payload returns a typed error.".into()],
                negative: vec!["A bounded payload is accepted.".into()],
            },
        ],
        design:
            "## Dependencies\n\nNo new dependency.\n\n## ADRs\n\nADR-0001 records the boundary."
                .into(),
        tasks: vec![
            OpenSpecTask {
                id: "T1".into(),
                title: "Implement identifier validation".into(),
                paths: vec!["src/request.rs".into()],
                acceptance: vec!["REQ-001 a malformed identifier returns a typed error.".into()],
                evidence: vec!["cargo test -p factory-native --lib request".into()],
            },
            OpenSpecTask {
                id: "T2".into(),
                title: "Implement payload bounds".into(),
                paths: vec!["tests/request.rs".into()],
                acceptance: vec!["REQ-002 an oversized payload returns a typed error.".into()],
                evidence: vec!["cargo test -p factory-native --test request".into()],
            },
        ],
        stack_summary: "Rust 2024 with std only.".into(),
        architecture_summary: "One bounded module at the request boundary.".into(),
    }
}

fn fixture_read_model() -> OpenSpecReadModel {
    let artifacts = render(&sample_change()).expect("valid Core render");
    let projection = artifacts.projection(true).expect("valid projection");
    let artifact_json = serde_json::to_string(&artifacts).expect("artifact JSON");
    read_model(
        "wp7-compose-fixture",
        &projection,
        &sha256_hex(artifact_json.as_bytes()),
        &artifacts,
    )
    .expect("valid read model")
}

#[test]
fn committed_fixture_matches_the_core_renderer() {
    let expected = fixture_read_model();
    let committed: OpenSpecReadModel =
        serde_json::from_str(FIXTURE).expect("fixture decodes as the Core read model");
    assert_eq!(committed, expected);
    assert!(committed.validated);
    assert!(!committed.archived);
    assert_eq!(committed.change_id, "add-bounded-request-validation");
    assert_eq!(committed.files.len(), 4);
}

struct FakeRunner {
    change_id: String,
    expected_workspace: std::path::PathBuf,
    fail: bool,
}

impl OpenSpecRunner for FakeRunner {
    fn version(&self, _program: &str) -> Result<String, OpenSpecCliError> {
        Ok("1.16.1".into())
    }

    fn validate(
        &self,
        _program: &str,
        workspace: &Path,
        change_id: &str,
    ) -> Result<(String, String), OpenSpecCliError> {
        assert_eq!(change_id, self.change_id);
        assert_eq!(workspace, self.expected_workspace);
        assert!(
            workspace
                .join("openspec/changes/add-bounded-request-validation/proposal.md")
                .is_file()
        );
        if self.fail {
            Err(OpenSpecCliError::Invalid("fixture rejected".into()))
        } else {
            Ok((format!("{change_id}: valid"), String::new()))
        }
    }
}

#[test]
fn rendered_files_drive_the_pinned_validation_runner() {
    let artifacts = render(&sample_change()).expect("valid Core render");
    artifacts.validate().expect("valid artifacts");
    let root =
        std::env::temp_dir().join(format!("factory-openspec-compose-{}", uuid::Uuid::new_v4()));
    let change_dir = root.join("openspec/changes").join(&artifacts.change_id);
    std::fs::create_dir_all(&change_dir).expect("workspace");
    for (relative, content) in artifacts.files() {
        let target = change_dir.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent");
        }
        std::fs::write(&target, content).expect("artifact");
    }

    let runner = FakeRunner {
        change_id: artifacts.change_id.clone(),
        expected_workspace: root.clone(),
        fail: false,
    };
    let validation =
        validate_with_pinned_cli(&root, &artifacts.change_id, &runner).expect("valid change");
    assert_eq!(validation.version, "1.16.1");
    assert_eq!(validation.change_id, artifacts.change_id);

    let failing = FakeRunner {
        change_id: artifacts.change_id.clone(),
        expected_workspace: root.clone(),
        fail: true,
    };
    assert!(validate_with_pinned_cli(&root, &artifacts.change_id, &failing).is_err());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn read_model_rejects_a_changed_artifact() {
    let artifacts = render(&sample_change()).expect("valid Core render");
    let projection = artifacts.projection(true).expect("valid projection");
    let artifact_json = serde_json::to_string(&artifacts).expect("artifact JSON");
    let digest = sha256_hex(artifact_json.as_bytes());
    let mut tampered: OpenSpecArtifacts = artifacts.clone();
    tampered.tasks.push_str("\n<!-- tampered -->\n");
    assert!(read_model("wp7-compose-fixture", &projection, &digest, &tampered).is_err());
}

#[test]
fn archive_contract_is_idempotent_and_rejects_stored_artifact_drift() {
    let artifacts = render(&sample_change()).expect("valid Core render");
    let projection = artifacts.projection(true).expect("valid projection");
    let content_json = serde_json::to_string(&artifacts).expect("artifact JSON");
    let digest = sha256_hex(content_json.as_bytes());
    let parsed = parse_stored_artifact(&content_json, &digest).expect("exact stored artifact");
    let model =
        read_model("wp7-compose-fixture", &projection, &digest, &parsed).expect("valid read model");
    let first = archive_outcome(&model, false).expect("archive outcome");
    let replay = archive_outcome(&model, true).expect("archive replay");
    assert_eq!(
        first.archive_path,
        "openspec/changes/archive/add-bounded-request-validation"
    );
    assert!(first.archived);
    assert!(!first.already_archived);
    assert!(replay.already_archived);

    let mut changed = artifacts.clone();
    changed.tasks.push_str("\n<!-- drift -->\n");
    let changed_json = serde_json::to_string(&changed).expect("changed JSON");
    let changed_digest = sha256_hex(changed_json.as_bytes());
    let error = parse_stored_artifact(&changed_json, &changed_digest)
        .expect_err("changed artifact must not archive");
    assert!(error.to_string().contains("manifest hash"));
}

#[test]
fn fixture_declares_core_authority_and_exact_hashes() {
    let model = fixture_read_model();
    for file in &model.files {
        assert_eq!(file.sha256, sha256_hex(file.content.as_bytes()));
    }
    assert_eq!(
        model.hashes.proposal_sha256,
        model.manifest.hash("proposal.md").expect("proposal hash")
    );
    assert_eq!(
        model.hashes.tasks_sha256,
        model.manifest.hash("tasks.md").expect("tasks hash")
    );
    assert_eq!(
        model.hashes.design_sha256,
        model.manifest.hash("design.md").expect("design hash")
    );
    assert_eq!(
        model.hashes.specs_sha256,
        model.manifest.specs_sha256().expect("specs hash")
    );
    assert_eq!(
        model.change_path,
        "openspec/changes/add-bounded-request-validation"
    );
    assert!(
        model
            .files
            .iter()
            .any(|file| file.content.contains("Core is authoritative"))
    );
}
