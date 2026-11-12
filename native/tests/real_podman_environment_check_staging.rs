//! Real-Podman regression for environment-check workspace staging.
//!
//! `podman volume import` merges into an existing volume. Staging therefore
//! must remove and recreate the volume so a file deleted from the Git worktree
//! cannot survive from an earlier check. This test is gated because it needs a
//! rootless engine; run it with:
//!
//! ```text
//! FACTORY_REAL_PODMAN_TESTS=1 cargo test -p factory-native \
//!   --test real_podman_environment_check_staging -- --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use factory_native::execution::{
    RootlessPodmanVolumes, VolumeBackend, WorkspaceAllocation, WorkspaceManager,
};
use factory_native::runtime_broker::ResourceOwner;
use factory_native::workspace_archive;

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git must start");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn extract(bytes: &[u8], path: &Path) {
    workspace_archive::extract(bytes, path).expect("snapshot must extract");
}

#[tokio::test]
async fn restaging_removes_a_file_deleted_from_the_worktree() {
    if std::env::var("FACTORY_REAL_PODMAN_TESTS").as_deref() != Ok("1") {
        eprintln!("skipped: set FACTORY_REAL_PODMAN_TESTS=1 to run real Podman staging");
        return;
    }

    let root = std::env::var_os("FACTORY_REAL_PODMAN_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "factory-real-podman-staging-{}",
                uuid::Uuid::new_v4()
            ))
        });
    std::fs::create_dir_all(&root).unwrap();
    let repository = root.join("repo");
    std::fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(
        &repository,
        &["config", "user.email", "test@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Test"]);
    std::fs::write(repository.join("old.txt"), "old\n").unwrap();
    std::fs::write(repository.join("modified.txt"), "before\n").unwrap();
    git(&repository, &["add", "old.txt", "modified.txt"]);
    git(&repository, &["commit", "-qm", "base"]);
    let base_commit = git(&repository, &["rev-parse", "HEAD"]);

    let backend = RootlessPodmanVolumes::new(
        ["localhost/factory-staging-fixture:test".to_owned()],
        ResourceOwner::Supervisor,
    )
    .unwrap();
    backend.preflight().await.expect("rootless podman");
    let manager = WorkspaceManager::new(
        root.join("work"),
        root.join("state"),
        [repository.clone()],
        backend.clone(),
    )
    .unwrap();
    let attempt_id = format!("real-staging-{}", uuid::Uuid::new_v4().simple());
    let handle = manager
        .allocate(&WorkspaceAllocation {
            attempt_id: attempt_id.clone(),
            repository: repository.to_string_lossy().into_owned(),
            base_commit,
            generation: 1,
            action_spec_sha256: "a".repeat(64),
            attempt_grant: String::new(),
        })
        .await
        .expect("workspace allocation");
    let volume = manager.workspace_volume(&handle.workspace_id).unwrap();

    manager
        .stage_workspace_for_check(&handle.workspace_id)
        .await
        .expect("first staging");
    let first = root.join("snapshot-1");
    extract(&backend.export_volume(&volume).await.unwrap(), &first);
    assert_eq!(
        std::fs::read_to_string(first.join("old.txt")).unwrap(),
        "old\n"
    );

    let worktree = root.join("work").join(&attempt_id);
    std::fs::remove_file(worktree.join("old.txt")).unwrap();
    std::fs::write(worktree.join("modified.txt"), "after\n").unwrap();
    manager
        .stage_workspace_for_check(&handle.workspace_id)
        .await
        .expect("second staging replaces the volume");
    let second = root.join("snapshot-2");
    extract(&backend.export_volume(&volume).await.unwrap(), &second);
    assert!(
        !second.join("old.txt").exists(),
        "a file deleted from the worktree must not survive volume import"
    );
    assert_eq!(
        std::fs::read_to_string(second.join("modified.txt")).unwrap(),
        "after\n"
    );

    manager.cleanup(&handle.workspace_id).await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
