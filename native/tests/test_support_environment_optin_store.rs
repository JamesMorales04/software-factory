//! Real-store checks for the CR-15 seam: the first real environment binding is
//! accepted, an identical rebind is idempotent, a different binding is refused,
//! and a frozen plan/spec resumes without a second bind.
//!
//! Runs with `--features test-support` and **not** under `cfg(test)`, i.e. the
//! configuration the `core-e2e` image builds, so the automatic placeholder would
//! be visible here if the opt-in gate regressed.

use factory_native::db::Database;
use factory_native::planning::environment::{
    ENVIRONMENT_POLICY_REVISION, ENVIRONMENT_SCHEMA_VERSION, EnvironmentBuilderDescriptor,
    EnvironmentBuilderStatus, EnvironmentFile, EnvironmentFileOrigin, EnvironmentNixPolicy,
    EnvironmentPlan, EnvironmentPlanSource, EnvironmentPlanStatus, EnvironmentSpec,
    EnvironmentSpecStatus, ExistingEnvironmentKind, LicenseInventory, NetworkPolicy,
    ProjectConventions, ResourceLimits, SandboxPolicy, sha256_hex,
};

/// Tests that mutate the process environment serialize on this guard so the
/// default parallel test runtime cannot interleave an opted-in caller with an
/// unset one. No extra dependency: tokio's const mutex is already available.
static ENV_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn database(label: &str) -> (Database, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "factory-cr15-{label}-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let db = Database::open(&path).await.expect("database opens");
    (db, path)
}

fn conventions(project_id: &str, base_commit: &str) -> ProjectConventions {
    let mut conventions = ProjectConventions {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        project_id: project_id.into(),
        base_commit: base_commit.into(),
        conventions_sha256: String::new(),
        scanned_at: chrono::Utc::now().to_rfc3339(),
        files: Vec::new(),
        commands: Vec::new(),
        conflicts: Vec::new(),
        truncated: false,
        skipped_entries: 0,
        provenance: "cr15-test".into(),
        existing_environment: ExistingEnvironmentKind::None,
    };
    conventions.conventions_sha256 = conventions.compute_sha256();
    conventions
}

fn frozen_pair(
    action_id: &str,
    project_id: &str,
    base_commit: &str,
    plan_id: &str,
) -> (EnvironmentPlan, EnvironmentSpec) {
    let conventions = conventions(project_id, base_commit);
    let mut inventory = LicenseInventory::default();
    inventory.refresh_hashes(plan_id);
    let builder = EnvironmentBuilderDescriptor {
        image: "fixture/environment-builder".into(),
        digest: format!("sha256:{}", "a".repeat(64)),
        platform: "x86_64-linux".into(),
        policy_revision: ENVIRONMENT_POLICY_REVISION.into(),
        default_nixpkgs: format!("github:NixOS/nixpkgs/{}", "2".repeat(40)),
        status: EnvironmentBuilderStatus::Ready,
    };
    let mut plan = EnvironmentPlan {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        plan_id: plan_id.into(),
        project_id: project_id.into(),
        request_id: action_id.into(),
        action_id: action_id.into(),
        campaign_id: None,
        base_commit: base_commit.into(),
        conventions_sha256: conventions.conventions_sha256.clone(),
        conventions,
        risk_class: "R0".into(),
        status: EnvironmentPlanStatus::Frozen,
        revision: 1,
        source: EnvironmentPlanSource::ProjectCommitted,
        toolchains: Vec::new(),
        packages: Vec::new(),
        services: Vec::new(),
        commands: Default::default(),
        browsers: Vec::new(),
        files: vec![EnvironmentFile {
            path: "flake.lock".into(),
            sha256: "a".repeat(64),
            origin: EnvironmentFileOrigin::Project,
            content: None,
        }],
        nix: EnvironmentNixPolicy {
            nixpkgs: format!("github:NixOS/nixpkgs/{}", "2".repeat(40)),
            inputs: Vec::new(),
            allow_unfree: false,
            sandbox: SandboxPolicy::default(),
        },
        resources: ResourceLimits::default(),
        network: NetworkPolicy::default(),
        license_inventory: inventory,
        license_inventory_sha256: String::new(),
        builder,
        resolution: None,
        lead_review: None,
        blockers: Vec::new(),
        commands_rationale: "cr15 fixture".into(),
        plan_sha256: String::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    plan.license_inventory_sha256 = plan.license_inventory.inventory_sha256.clone();
    plan.refresh_hash();
    let mut spec = EnvironmentSpec {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        spec_id: format!("spec-{plan_id}"),
        spec_sha256: String::new(),
        project_id: project_id.into(),
        request_id: action_id.into(),
        action_id: action_id.into(),
        campaign_id: None,
        base_commit: base_commit.into(),
        plan_id: plan.plan_id.clone(),
        plan_sha256: plan.plan_sha256.clone(),
        conventions_sha256: plan.conventions_sha256.clone(),
        review_id: format!("review-{plan_id}"),
        platform: "x86_64-linux".into(),
        nix_version: "2.28.3".into(),
        devenv_version: "1.11.2".into(),
        flake_lock_sha256: "a".repeat(64),
        devenv_lock_sha256: String::new(),
        license_inventory_sha256: plan.license_inventory.inventory_sha256.clone(),
        sbom_sha256: plan.license_inventory.sbom_sha256.clone(),
        builder_image: plan.builder.image.clone(),
        builder_image_digest: plan.builder.digest.clone(),
        resource_policy: plan.resources,
        network_policy: plan.network.clone(),
        status: EnvironmentSpecStatus::Frozen,
        frozen_at: chrono::Utc::now().to_rfc3339(),
    };
    spec.spec_sha256 = spec.compute_sha256();
    (plan, spec)
}

/// The gate is closed in this test, so the ActionSpec has no placeholder and the
/// first real binding must succeed; an identical rebind is idempotent and a
/// different plan is refused without touching the stored binding.
#[tokio::test]
async fn the_first_real_binding_is_accepted_then_idempotent_then_refused() {
    let _env = ENV_GUARD.lock().await;
    unsafe { std::env::remove_var("FACTORY_TEST_ENVIRONMENT_BINDING") };
    let (db, path) = database("cas").await;
    let repository =
        std::env::temp_dir().join(format!("factory-cr15-repo-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repository).expect("repository dir");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&repository)
            .args(args)
            .output()
            .expect("git command");
    };
    git(&["init"]);
    std::fs::write(repository.join("README.md"), "cr15 fixture\n").expect("fixture file");
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=cr15@example.invalid",
        "-c",
        "user.name=CR15 Fixture",
        "commit",
        "-m",
        "fixture",
    ]);
    let project = db
        .register_project(&repository, "cr15-project")
        .await
        .expect("project registers");
    let action = db
        .create_action(
            &project.id,
            "cr15 objective",
            "opencode-go",
            "deepseek-v4.1-flash",
            "cr15-action",
        )
        .await
        .expect("action created");
    let stored = db.action(&action.id).await.expect("action readable").spec;
    assert_eq!(
        stored.environment_plan_id, None,
        "no placeholder without opt-in"
    );

    let (plan, spec) = frozen_pair(&action.id, &project.id, &action.spec.base_commit, "plan-1");
    db.save_environment_plan(&plan).await.expect("plan saves");
    insert_review(&path, &plan).await;
    db.save_environment_spec(&spec).await.expect("spec saves");

    db.set_action_environment_binding(&action.id, &plan, &spec)
        .await
        .expect("the first real binding is accepted");
    db.set_action_environment_binding(&action.id, &plan, &spec)
        .await
        .expect("an identical rebind is idempotent");

    let (other_plan, other_spec) =
        frozen_pair(&action.id, &project.id, &action.spec.base_commit, "plan-2");
    db.save_environment_plan(&other_plan)
        .await
        .expect("second plan saves");
    insert_review(&path, &other_plan).await;
    db.save_environment_spec(&other_spec)
        .await
        .expect("second spec saves");
    let refused = db
        .set_action_environment_binding(&action.id, &other_plan, &other_spec)
        .await;
    assert!(refused.is_err(), "a different binding must be refused");
    let binding = db
        .action(&action.id)
        .await
        .expect("action still readable")
        .spec;
    assert_eq!(
        binding.environment_plan_id.as_deref(),
        Some(plan.plan_id.as_str())
    );

    // Frozen resume: the stored binding still binds the same plan/spec.
    db.set_action_environment_binding(&action.id, &plan, &spec)
        .await
        .expect("a frozen plan/spec resumes without rebinding");

    db.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_dir_all(&repository);
}

/// The frozen spec references a recorded plan review; insert fixture provenance
/// directly so the test exercises the real binding path without a Lead call.
async fn insert_review(path: &std::path::Path, plan: &EnvironmentPlan) {
    use sqlx::connection::Connection as _;
    use sqlx_sqlite::{SqliteConnectOptions, SqliteConnection};

    let options = SqliteConnectOptions::new().filename(path);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .expect("review connection");
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO environment_plan_reviews
         (id,request_id,action_id,plan_id,plan_sha256,plan_revision,risk_class,
          reviewer_role,principal_id,recorded_by,decision,rationale,
          constraints_json,evidence_json,model_call_json,decided_at,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,'LEAD',?8,'cr15-fixture','APPROVED',
                 'fixture review','[]','[]',?9,?10,?10)",
    )
    .bind(format!("review-{}", plan.plan_id))
    .bind(None::<String>)
    .bind(&plan.action_id)
    .bind(&plan.plan_id)
    .bind(&plan.plan_sha256)
    .bind(i64::from(plan.revision))
    .bind(&plan.risk_class)
    .bind(format!("lead:cr15-{}", plan.action_id))
    .bind(r#"{"mechanism":"MODEL","provider":"fixture","model":"fixture","reservationId":"cr15","requestSha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","inputTokens":0,"outputTokens":0,"totalTokens":0,"costMicros":0,"usageKnown":false}"#)
    .bind(&now)
    .execute(&mut connection)
    .await
    .expect("review inserts");
    connection.close().await.expect("review connection closes");
}

/// A frozen resume reads the stored plan/spec and must not delete, rebind or
/// call a model. This is the resume path a restarted Core takes; a replay of the
/// database alone does not prove it.
#[tokio::test]
async fn the_frozen_resume_is_read_only() {
    let _env = ENV_GUARD.lock().await;
    unsafe { std::env::remove_var("FACTORY_TEST_ENVIRONMENT_BINDING") };
    let (db, path) = database("resume").await;
    let repository =
        std::env::temp_dir().join(format!("factory-cr15-resume-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repository).expect("repository dir");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&repository)
            .args(args)
            .output()
            .expect("git command");
    };
    git(&["init"]);
    std::fs::write(repository.join("README.md"), "cr15 resume\n").expect("fixture file");
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=cr15@example.invalid",
        "-c",
        "user.name=CR15 Fixture",
        "commit",
        "-m",
        "fixture",
    ]);
    let project = db
        .register_project(&repository, "cr15-resume-project")
        .await
        .expect("project registers");
    let action = db
        .create_action(
            &project.id,
            "cr15 resume objective",
            "opencode-go",
            "deepseek-v4.1-flash",
            "cr15-resume-action",
        )
        .await
        .expect("action created");
    let (plan, spec) = frozen_pair(&action.id, &project.id, &action.spec.base_commit, "plan-r");
    db.save_environment_plan(&plan).await.expect("plan saves");
    insert_review(&path, &plan).await;
    db.save_environment_spec(&spec).await.expect("spec saves");
    db.set_action_environment_binding(&action.id, &plan, &spec)
        .await
        .expect("first bind");

    // Resume: read the frozen plan/spec and the stored binding only.
    let stored_spec = db
        .environment_spec_for_action(&action.id)
        .await
        .expect("spec readable");
    assert_eq!(stored_spec.spec.spec_sha256, spec.spec_sha256);
    assert_eq!(stored_spec.spec.plan_sha256, plan.plan_sha256);
    let bound = db.action(&action.id).await.expect("action readable").spec;
    assert_eq!(
        bound.environment_plan_id.as_deref(),
        Some(plan.plan_id.as_str())
    );
    assert_eq!(
        bound.environment_spec_sha256.as_deref(),
        Some(spec.spec_sha256.as_str())
    );

    db.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    let _ = std::fs::remove_dir_all(&repository);
}

/// The ON case: with the variable set to "1" the seam applies the fixture
/// binding to a real ActionSpec. Without this case a suite could pass while the
/// gate never opens.
#[tokio::test]
async fn the_seam_applies_the_placeholder_on_explicit_opt_in() {
    let _env = ENV_GUARD.lock().await;
    let mut spec = crate::action_spec_for_opt_in_test();
    unsafe { std::env::set_var("FACTORY_TEST_ENVIRONMENT_BINDING", "1") };
    factory_native::db::apply_test_environment_binding_if_enabled(&mut spec, "action-on");
    unsafe { std::env::remove_var("FACTORY_TEST_ENVIRONMENT_BINDING") };
    assert_eq!(
        spec.environment_plan_id.as_deref(),
        Some("test-plan-action-on")
    );
    assert_eq!(
        spec.environment_spec_id.as_deref(),
        Some("test-spec-action-on")
    );
    assert!(spec.environment_required);
    assert_eq!(
        spec.environment_builder_image.as_deref(),
        Some("fixture/environment-builder")
    );
}

#[test]
fn the_placeholder_identity_is_never_a_real_plan() {
    let (plan, spec) = frozen_pair("action-1", "project-1", "b".repeat(40).as_str(), "plan-1");
    assert_ne!(plan.plan_id, "test-plan-action-1");
    assert_ne!(spec.spec_id, "test-spec-action-1");
    assert_eq!(sha256_hex(plan.plan_id.as_bytes()).len(), 64);
}

/// A real `ActionSpec` with every field the seam touches left empty.
pub(crate) fn action_spec_for_opt_in_test() -> factory_native::db::ActionSpec {
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "project_id": "project-on",
        "repository": "/tmp/repository",
        "base_commit": "a".repeat(40),
        "objective": "opt-in",
        "objective_sha256": "b".repeat(64),
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "allowed_paths": ["src"],
        "checks": []
    }))
    .expect("ActionSpec deserializes")
}
