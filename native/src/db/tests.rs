use super::*;

use chrono::Utc;
use sqlx::connection::Connection as SqlxConnection;
use sqlx_sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePoolOptions};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::lifecycle::load_attempt;

fn temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()))
}

fn fixture_operations() -> crate::execution_policy::OperationLimits {
    crate::execution_policy::OperationLimits {
        model_request_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
        tool_call_timeout_secs: std::num::NonZeroU32::new(600).unwrap(),
        max_request_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
        max_response_bytes: std::num::NonZeroU32::new(2 * 1024 * 1024).unwrap(),
        max_generation_tokens: std::num::NonZeroU32::new(8_192).unwrap(),
        max_in_flight_operations: std::num::NonZeroU32::new(4).unwrap(),
    }
}

fn fixture_progress() -> crate::execution_policy::ProgressPolicy {
    crate::execution_policy::ProgressPolicy {
        diagnose_after_stagnant_checkpoints: std::num::NonZeroU32::new(2).unwrap(),
        escalate_after_unsuccessful_replans: std::num::NonZeroU32::new(1).unwrap(),
        observation_timeout_secs: std::num::NonZeroU32::new(300).unwrap(),
        autonomy: Default::default(),
    }
}

async fn temp_db(prefix: &str) -> (PathBuf, Database) {
    let path = temp_path(prefix);
    let db = Database::open(&path).await.unwrap();
    (path, db)
}

async fn insert_project(db: &Database, id: &str, repository: &str, base: &str) {
    sqlx::query::query("INSERT INTO projects(id,repository,base_commit,created_at,identity) VALUES (?1,?2,?3,?4,'')")
        .bind(id)
        .bind(repository)
        .bind(base)
        .bind(Utc::now().to_rfc3339())
        .execute(&db.pool)
        .await
        .unwrap();
}

async fn count(db: &Database, sql: &str) -> i64 {
    let (value,): (i64,) = sqlx::query_as::query_as(sql)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    value
}

async fn run_status(db: &Database, run_id: &str) -> String {
    let (status,): (String,) = sqlx::query_as::query_as("SELECT status FROM runs WHERE id=?1")
        .bind(run_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    status
}

async fn run_id_for_candidate(db: &Database, candidate_id: &str) -> String {
    let (run_id,): (String,) = sqlx::query_as::query_as(
        "SELECT r.id FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    run_id
}

struct SeedRun<'a> {
    run_id: &'a str,
    campaign_id: &'a str,
    run_status: &'a str,
    attempt_id: &'a str,
    attempt_status: &'a str,
    spec_sha256: &'a str,
    candidate_and_gate: Option<(&'a str, &'a str)>,
}

async fn seed_run(db: &Database, run: SeedRun<'_>) {
    let now = Utc::now().to_rfc3339();
    sqlx::query::query("INSERT INTO runs VALUES (?1,?2,?3,?4)")
        .bind(run.run_id)
        .bind(run.campaign_id)
        .bind(run.run_status)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)")
        .bind(run.attempt_id)
        .bind(run.run_id)
        .bind(run.attempt_status)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    let Some((candidate_id, gate_outcome)) = run.candidate_and_gate else {
        return;
    };
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
         VALUES (?1,?2,?3,?4,?5,'worker','now')",
    )
    .bind(candidate_id)
    .bind(run.attempt_id)
    .bind(run.spec_sha256)
    .bind("a".repeat(40))
    .bind("b".repeat(40))
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO gate_decisions VALUES (?1,?2,'[]',?3)")
        .bind(candidate_id)
        .bind(gate_outcome)
        .bind(now)
        .execute(&db.pool)
        .await
        .unwrap();
}

fn migration_file_count() -> i64 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
    std::fs::read_dir(directory)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "sql")
        })
        .count() as i64
}

#[tokio::test]
async fn p1_p2_fresh_schema_uses_migrations_and_reopen_is_stable() {
    let path = temp_path("factory-migrations");
    let db = Database::open(&path).await.unwrap();
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        migration_file_count()
    );
    insert_project(&db, "project", "/fixture", "base").await;
    db.close().await;

    let reopened = Database::open(&path).await.unwrap();
    assert_eq!(
        count(&reopened, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        migration_file_count()
    );
    assert_eq!(count(&reopened, "SELECT COUNT(*) FROM projects").await, 1);
    reopened.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn wave12_fresh_database_applies_latest_migrations() {
    let path = temp_path("factory-wave12-fresh");
    let db = Database::open(&path).await.unwrap();
    let versions: Vec<(i64,)> = sqlx::query_as::query_as(
        "SELECT version FROM _sqlx_migrations
         WHERE version BETWEEN 65 AND 69 ORDER BY version",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        versions
            .into_iter()
            .map(|(version,)| version)
            .collect::<Vec<_>>(),
        vec![65, 66, 67, 68, 69]
    );
    for schema_query in [
        "SELECT reviewer_role FROM quality_receipts LIMIT 0",
        "SELECT required_assignments_json FROM campaign_review_requirements LIMIT 0",
        "SELECT project_offset, updated_at FROM skill_refresh_state LIMIT 0",
        "SELECT source_checked_at, content_reviewed_at FROM skill_packs LIMIT 0",
        "SELECT name FROM sqlite_master
         WHERE type='trigger' AND name='action_specs_immutable_update'",
    ] {
        sqlx::query::query(schema_query)
            .execute(&db.pool)
            .await
            .unwrap();
    }
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn wave12_upgraded_database_applies_only_pending_migrations() {
    let repo_migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
    let subset = std::env::temp_dir().join(format!("factory-migrations-0064-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&subset).unwrap();
    let mut applied = 0u32;
    for entry in std::fs::read_dir(&repo_migrations).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(version) = name
            .split_once('_')
            .and_then(|(prefix, _)| prefix.parse::<u32>().ok())
        else {
            continue;
        };
        if version <= 64 && name.ends_with(".sql") {
            std::fs::copy(entry.path(), subset.join(name.as_ref())).unwrap();
            applied += 1;
        }
    }
    assert!(applied >= 63);
    let path = temp_path("factory-wave12-upgrade");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(subset.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let (pending,): (i64,) = sqlx::query_as::query_as(
        "SELECT COUNT(*) FROM _sqlx_migrations WHERE version BETWEEN 65 AND 69",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending, 0);
    sqlx::query::query(
        "INSERT INTO projects(id,repository,base_commit,created_at,identity)
         VALUES ('project','/fixture','base',?1,'')",
    )
    .bind(Utc::now().to_rfc3339())
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let db = Database::open(&path).await.unwrap();
    let versions: Vec<(i64,)> = sqlx::query_as::query_as(
        "SELECT version FROM _sqlx_migrations
         WHERE version BETWEEN 65 AND 69 ORDER BY version",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        versions
            .into_iter()
            .map(|(version,)| version)
            .collect::<Vec<_>>(),
        vec![65, 66, 67, 68, 69]
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM projects").await, 1);
    db.close().await;
    let _ = std::fs::remove_dir_all(subset);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn legacy_bootstrap_schema_is_adopted_without_data_loss() {
    let path = temp_path("factory-legacy");
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query::query("CREATE TABLE projects (id TEXT PRIMARY KEY, repository TEXT NOT NULL UNIQUE, base_commit TEXT NOT NULL, created_at TEXT NOT NULL)")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO projects(id,repository,base_commit,created_at) VALUES ('keep','/legacy','base','now')")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query::query("PRAGMA user_version = 1")
        .execute(&mut connection)
        .await
        .unwrap();
    connection.close().await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let db = Database::open(&path).await.unwrap();
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM _sqlx_migrations").await,
        migration_file_count()
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM projects WHERE id='keep'").await,
        1
    );
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn run_status_backfill_migration_is_conservative() {
    let (path, db) = temp_db("factory-run-backfill").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action_with_spec(
            "project",
            "backfill objective",
            "opencode",
            "test/model",
            "backfill-action",
            ActionSpecInput {
                allowed_paths: &["seed.txt".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let gate_campaign = db
        .create_campaign(&action.id, "backfill-gate-campaign")
        .await
        .unwrap();
    sqlx::query::query("UPDATE campaigns SET status='AUTHORIZED' WHERE id=?1")
        .bind(&gate_campaign.id)
        .execute(&db.pool)
        .await
        .unwrap();
    let cancelled_campaign = db
        .create_campaign(&action.id, "backfill-cancelled-campaign")
        .await
        .unwrap();
    sqlx::query::query("UPDATE campaigns SET status='CANCELLED' WHERE id=?1")
        .bind(&cancelled_campaign.id)
        .execute(&db.pool)
        .await
        .unwrap();
    let spec_sha256 = action.spec_sha256.as_str();

    for (run_id, status, attempt_id, attempt_status, candidate_and_gate) in [
        (
            "run-gate-accepted",
            "QUEUED",
            "attempt-gate-accepted",
            "CANDIDATE_READY",
            Some(("candidate-gate-accepted", "ACCEPTED")),
        ),
        (
            "run-gate-repair",
            "RUNNING",
            "attempt-gate-repair",
            "CANDIDATE_READY",
            Some(("candidate-gate-repair", "REPAIR_REQUIRED")),
        ),
        (
            "run-gate-blocked",
            "QUEUED",
            "attempt-gate-blocked",
            "CANDIDATE_READY",
            Some(("candidate-gate-blocked", "BLOCKED")),
        ),
        (
            "run-gate-human",
            "RUNNING",
            "attempt-gate-human",
            "CANDIDATE_READY",
            Some(("candidate-gate-human", "HUMAN_REQUIRED")),
        ),
        (
            "run-gate-not-tested",
            "QUEUED",
            "attempt-gate-not-tested",
            "CANDIDATE_READY",
            Some(("candidate-gate-not-tested", "NOT_TESTED")),
        ),
        (
            "run-terminal-cancelled",
            "CANCELLED",
            "attempt-terminal-cancelled",
            "CANDIDATE_READY",
            Some(("candidate-terminal-cancelled", "ACCEPTED")),
        ),
        (
            "run-untouched",
            "QUEUED",
            "attempt-untouched",
            "QUEUED",
            None,
        ),
        ("run-running", "RUNNING", "attempt-running", "LEASED", None),
        ("run-failed", "RUNNING", "attempt-failed", "FAILED", None),
    ] {
        seed_run(
            &db,
            SeedRun {
                run_id,
                campaign_id: &gate_campaign.id,
                run_status: status,
                attempt_id,
                attempt_status,
                spec_sha256,
                candidate_and_gate,
            },
        )
        .await;
    }
    seed_run(
        &db,
        SeedRun {
            run_id: "run-cancelled",
            campaign_id: &cancelled_campaign.id,
            run_status: "QUEUED",
            attempt_id: "attempt-cancelled",
            attempt_status: "QUEUED",
            spec_sha256,
            candidate_and_gate: None,
        },
    )
    .await;
    seed_run(
        &db,
        SeedRun {
            run_id: "run-terminal-failed",
            campaign_id: &cancelled_campaign.id,
            run_status: "FAILED",
            attempt_id: "attempt-terminal-failed",
            attempt_status: "LEASED",
            spec_sha256,
            candidate_and_gate: None,
        },
    )
    .await;

    sqlx::raw_sql::raw_sql(include_str!(
        "../../../migrations/0020_run_status_projection.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();

    for (run_id, expected) in [
        ("run-gate-accepted", "ACCEPTED"),
        ("run-gate-repair", "REPAIR_REQUIRED"),
        ("run-gate-blocked", "REJECTED"),
        ("run-gate-human", "REJECTED"),
        ("run-gate-not-tested", "REJECTED"),
        ("run-terminal-cancelled", "CANCELLED"),
        ("run-untouched", "QUEUED"),
        ("run-running", "RUNNING"),
        ("run-failed", "FAILED"),
        ("run-cancelled", "CANCELLED"),
        ("run-terminal-failed", "FAILED"),
    ] {
        assert_eq!(run_status(&db, run_id).await, expected, "{run_id}");
    }
    assert_eq!(
        db.campaign(&gate_campaign.id).await.unwrap().status,
        "AUTHORIZED"
    );
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
#[tokio::test]
async fn p5_group_readable_database_is_rejected_without_changing_data_or_mode() {
    use std::os::unix::fs::PermissionsExt;
    let (path, db) = temp_db("factory-insecure-db").await;
    sqlx::query::query("CREATE TABLE retained(value TEXT)")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query::query("INSERT INTO retained VALUES ('keep')")
        .execute(&db.pool)
        .await
        .unwrap();
    db.close().await;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Database::open(&path).await,
        Err(Error::Invalid(
            "existing database file grants group or other access"
        ))
    ));
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let options = SqliteConnectOptions::new().filename(&path);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    let (value,): (String,) = sqlx::query_as::query_as("SELECT value FROM retained")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(value, "keep");
    connection.close().await.unwrap();
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
#[tokio::test]
async fn executable_action_specs_require_an_environment_binding() {
    let (path, db) = temp_db("environment-required").await;
    insert_project(&db, "project", "/fixture", &"a".repeat(40)).await;
    let action = db
        .create_action_with_spec(
            "project",
            "Require an environment.",
            "opencode",
            "deepseek-v4.1-flash",
            "environment-required-action",
            ActionSpecInput {
                allowed_paths: &["src".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    assert!(validate_environment_requirement(&action.spec).is_ok());
    db.clear_test_action_environment(&action.id).await.unwrap();
    let action = db.action(&action.id).await.unwrap();
    assert!(matches!(
        validate_environment_requirement(&action.spec),
        Err(Error::IntakeConflict {
            code: "ENVIRONMENT_PLAN_REQUIRED",
            ..
        })
    ));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn database_symlink_path_is_rejected() {
    let directory = std::env::temp_dir().join(format!("factory-db-link-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let target = directory.join("target.sqlite");
    let link = directory.join("linked.sqlite");
    let db = Database::open(&target).await.unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(matches!(
        Database::open(&link).await,
        Err(Error::Invalid(
            "database path must be a regular non-symlink file"
        ))
    ));
    db.close().await;
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn p3_idempotency_replays_same_payload_and_rejects_changes() {
    let (path, db) = temp_db("factory-idempotency").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action("project", "fix bug", "codex", "model-a", "key-one")
        .await
        .unwrap();
    assert_eq!(
        action.id,
        db.create_action("project", "fix bug", "codex", "model-a", "key-one")
            .await
            .unwrap()
            .id
    );
    assert!(matches!(
        db.create_action("project", "different", "codex", "model-a", "key-one")
            .await,
        Err(Error::Conflict(_))
    ));
    let mut connection = db.pool.acquire().await.unwrap();
    assert!(matches!(
        replay(&mut connection, "key-one", "register_project", "digest").await,
        Err(Error::Conflict(_))
    ));
    drop(connection);
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn p4_failure_injection_rolls_back_action_spec_and_event() {
    let (path, db) = temp_db("factory-atomicity").await;
    insert_project(&db, "project", "/fixture", "base").await;
    sqlx::query::query(
        "CREATE TRIGGER fail_spec BEFORE INSERT ON action_specs BEGIN SELECT RAISE(ABORT, 'injected'); END;",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(
        db.create_action("project", "objective", "codex", "model", "action-key")
            .await
            .is_err()
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM actions").await, 0);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM action_specs").await, 0);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM events WHERE kind='action.created'"
        )
        .await,
        0
    );
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn p5_referential_integrity_rejects_unknown_project() {
    let (path, db) = temp_db("factory-references").await;
    let result = sqlx::query::query(
        "INSERT INTO actions VALUES ('a','missing','o','codex','m','R0','PENDING','d','now')",
    )
    .execute(&db.pool)
    .await;
    let error = Error::from(result.unwrap_err());
    assert!(matches!(error, Error::Constraint(_)));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn frozen_spec_is_immutable_and_remains_human_required() {
    let (path, db) = temp_db("factory-spec").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action(
            "project",
            "fix bug",
            "opencode",
            "provider/model",
            "key-two",
        )
        .await
        .unwrap();
    assert_eq!(action.status, "HUMAN_REQUIRED");
    assert_eq!(action.risk, "UNASSESSED");
    assert_eq!(action.spec.base_commit, "base");
    let canonical_spec = serde_json::to_string(&action.spec).unwrap();
    assert_eq!(action.spec_sha256, sha256_hex(canonical_spec.as_bytes()));
    assert_ne!(action.spec_sha256, action.objective_sha256);
    let listed = db.actions().await.unwrap();
    assert_eq!(listed[0].spec, action.spec);
    sqlx::query::query("UPDATE actions SET objective='tampered' WHERE id=?1")
        .bind(&action.id)
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(matches!(db.actions().await, Err(Error::Invalid(_))));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_list_skips_historical_rows_without_a_frozen_spec() {
    let (path, db) = temp_db("factory-action-orphan-spec").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let valid = db
        .create_action(
            "project",
            "valid action",
            "opencode",
            "provider/model",
            "valid-key",
        )
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO actions VALUES ('legacy','project','legacy action','opencode','provider/model','R0','HUMAN_REQUIRED','digest','now')",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    let listed = db.actions().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, valid.id);
    assert!(matches!(db.action("legacy").await, Err(Error::NotFound)));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn campaign_is_durable_but_requires_authorization_for_a_run() {
    let (path, db) = temp_db("factory-campaign").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action("project", "objective", "codex", "model", "action-key")
        .await
        .unwrap();
    let campaign = db
        .create_campaign(&action.id, "campaign-key")
        .await
        .unwrap();
    assert_eq!(campaign.status, "BLOCKED_AUTHORIZATION");
    assert_eq!(
        campaign.id,
        db.create_campaign(&action.id, "campaign-key")
            .await
            .unwrap()
            .id
    );
    assert!(matches!(
        db.create_run(&campaign.id, "run-key").await,
        Err(Error::Invalid(_))
    ));
    let events = db.events().await.unwrap();
    assert!(events.iter().any(|event| event.kind == "campaign.created"));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn project_campaigns_project_gate_effective_status_and_active_state() {
    let (path, db) = temp_db("factory-project-campaign-effective").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action(
            "project",
            "projection objective",
            "opencode",
            "model",
            "action-key",
        )
        .await
        .unwrap();
    let accepted = db
        .create_campaign(&action.id, "accepted-campaign")
        .await
        .unwrap();
    let running = db
        .create_campaign(&action.id, "running-campaign")
        .await
        .unwrap();
    let cancelled = db
        .create_campaign(&action.id, "cancelled-campaign")
        .await
        .unwrap();
    let failed = db
        .create_campaign(&action.id, "failed-campaign")
        .await
        .unwrap();
    for (campaign_id, status) in [
        (&accepted.id, "AUTHORIZED"),
        (&running.id, "AUTHORIZED"),
        (&cancelled.id, "CANCELLED"),
        (&failed.id, "FAILED"),
    ] {
        sqlx::query::query("UPDATE campaigns SET status=?1 WHERE id=?2")
            .bind(status)
            .bind(campaign_id.as_str())
            .execute(&db.pool)
            .await
            .unwrap();
    }
    let spec_sha256 = action.spec_sha256.as_str();
    seed_run(
        &db,
        SeedRun {
            run_id: "run-accepted",
            campaign_id: &accepted.id,
            run_status: "QUEUED",
            attempt_id: "attempt-accepted",
            attempt_status: "CANDIDATE_READY",
            spec_sha256,
            candidate_and_gate: Some(("candidate-accepted", "ACCEPTED")),
        },
    )
    .await;
    seed_run(
        &db,
        SeedRun {
            run_id: "run-running",
            campaign_id: &running.id,
            run_status: "RUNNING",
            attempt_id: "attempt-running",
            attempt_status: "LEASED",
            spec_sha256,
            candidate_and_gate: None,
        },
    )
    .await;

    let campaigns = db.project_campaigns("project").await.unwrap();
    assert_eq!(campaigns.len(), 4);
    let accepted_summary = campaigns
        .iter()
        .find(|campaign| campaign.id == accepted.id)
        .expect("accepted campaign");
    assert_eq!(accepted_summary.status, "AUTHORIZED");
    assert_eq!(accepted_summary.effective_status, "ACCEPTED");
    assert!(!accepted_summary.is_active());
    let running_summary = campaigns
        .iter()
        .find(|campaign| campaign.id == running.id)
        .expect("running campaign");
    assert_eq!(running_summary.effective_status, "AUTHORIZED");
    assert!(running_summary.is_active());
    let cancelled_summary = campaigns
        .iter()
        .find(|campaign| campaign.id == cancelled.id)
        .expect("cancelled campaign");
    assert_eq!(cancelled_summary.status, "CANCELLED");
    assert_eq!(cancelled_summary.effective_status, "CANCELLED");
    assert!(!cancelled_summary.is_active());
    let failed_summary = campaigns
        .iter()
        .find(|campaign| campaign.id == failed.id)
        .expect("failed campaign");
    assert_eq!(failed_summary.status, "FAILED");
    assert_eq!(failed_summary.effective_status, "FAILED");
    assert!(!failed_summary.is_active());

    db.close().await;
    let _ = std::fs::remove_file(path);
}

async fn authorized_attempt(db: &Database) -> String {
    insert_project(db, "project", "/fixture", "base").await;
    let action = db
        .create_action_with_spec(
            "project",
            "objective",
            "codex",
            "model",
            "action-key",
            ActionSpecInput {
                allowed_paths: &["seed.txt".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let campaign = db
        .create_campaign(&action.id, "campaign-key")
        .await
        .unwrap();
    db.authorize_campaign(
        &campaign.id,
        &NewCampaignAuthorization {
            authorization_id: "auth-test".into(),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "test".into(),
            rationale: "fixture setup".into(),
            authorized_paths: vec!["seed.txt".into()],
            provider: String::new(),
            model: String::new(),
            provider_mode: String::new(),
            delegation_id: String::new(),
            allowed_tools: Vec::new(),
            monetary_envelope_id: crate::db::authorization::NO_BILLABLE_ENVELOPE.into(),
            progress: fixture_progress(),
            operations: fixture_operations(),
        },
    )
    .await
    .unwrap();
    let run = db.create_run(&campaign.id, "run-key").await.unwrap();
    let attempt = db.create_attempt(&run.id, "attempt-key").await.unwrap();
    attempt.id
}

#[tokio::test]
async fn fixture_assignment_claim_does_not_require_provider_route_identity() {
    let (path, db) = temp_db("factory-fixture-assignment").await;
    let attempt_id = authorized_attempt(&db).await;

    let assignment = db
        .claim_assignment("worker-fixture", 100, 60_000)
        .await
        .unwrap();

    assert_eq!(
        assignment
            .expect("authorized fixture attempt is claimable")
            .lease
            .attempt_id,
        attempt_id
    );
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn dependency_plan_attaches_before_campaign_and_missing_plan_blocks() {
    let (path, db) = temp_db("factory-dependency-plan-attach").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action_with_spec(
            "project",
            "objective",
            "codex",
            "model",
            "action-key",
            ActionSpecInput {
                allowed_paths: &["src".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let mut missing = action.spec.clone();
    missing.dependencies_required = true;
    missing.dependency_plan = None;
    assert!(crate::db::validate_dependency_requirement(&missing).is_err());

    let planning = crate::planning::DependencyPlan {
        strategy: crate::planning::DependencyStrategy::VendorCache,
        items: Vec::new(),
        cache_ref: "vendor".into(),
        sbom_ref: "artifacts/sbom.json".into(),
        owner_approval: None,
    };
    let plan = crate::dependencies::from_frozen_planning(
        &planning,
        crate::dependencies::DependencyKind::Rust,
        None,
        "vendor",
        "artifacts/sbom.json",
    )
    .unwrap()
    .expect("vendor plan");
    let updated = db
        .set_action_dependency_plan(&action.id, Some(&plan), true)
        .await
        .unwrap();
    assert!(updated.spec.dependencies_required);
    assert_eq!(updated.spec.dependency_plan, Some(plan.clone()));
    let loaded = db.action(&action.id).await.unwrap();
    assert_eq!(loaded.spec.dependency_plan, Some(plan));
    assert!(crate::db::validate_dependency_requirement(&loaded.spec).is_ok());

    let _campaign = db
        .create_campaign(&action.id, "campaign-key")
        .await
        .unwrap();
    assert!(matches!(
        db.set_action_dependency_plan(&action.id, None, false).await,
        Err(Error::Conflict(_))
    ));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn heartbeat_does_not_shorten_a_longer_claim_lease() {
    let path = temp_path("factory-lease-no-shorten");
    let db = Database::open(&path).await.unwrap();
    let attempt_id = authorized_attempt(&db).await;
    let first = db
        .claim_attempt("worker-a", 100, 50)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.expires_at_ms, 150);
    let extended = db
        .heartbeat(&attempt_id, "worker-a", 1, 120, 10)
        .await
        .unwrap();
    assert_eq!(extended.expires_at_ms, 150);
    let extended = db
        .heartbeat(&attempt_id, "worker-a", 1, 130, 60)
        .await
        .unwrap();
    assert_eq!(extended.expires_at_ms, 190);
}

#[tokio::test]
async fn p7_p8_claim_is_fenced_expires_and_survives_restart() {
    let path = temp_path("factory-lease");
    let db = Database::open(&path).await.unwrap();
    let attempt_id = authorized_attempt(&db).await;
    let run_id = db.runs().await.unwrap()[0].id.clone();
    assert_eq!(db.attempts().await.unwrap()[0].id, attempt_id);
    let first = db
        .claim_attempt("worker-a", 100, 50)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.generation, 1);
    assert_eq!(run_status(&db, &run_id).await, "RUNNING");
    assert!(
        db.heartbeat(&attempt_id, "worker-a", 1, 120, 50)
            .await
            .is_ok()
    );
    assert!(matches!(
        db.heartbeat(&attempt_id, "worker-a", 1, 170, 50).await,
        Err(Error::Invalid(_))
    ));
    let second = db
        .claim_attempt("worker-b", 170, 50)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.generation, 2);
    let report = WorkerReport {
        attempt_id: attempt_id.clone(),
        worker_id: "worker-b".into(),
        generation: 2,
        receipt_id: "receipt-one".into(),
        result_sha256: "a".repeat(64),
        terminal_status: "FAILED".into(),
        reason: "fixture terminal".into(),
    };
    assert!(matches!(
        db.report_terminal(
            &WorkerReport {
                terminal_status: "SUCCEEDED".into(),
                ..report.clone()
            },
            180
        )
        .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        db.report_terminal(
            &WorkerReport {
                worker_id: "worker-a".into(),
                generation: 1,
                ..report.clone()
            },
            180
        )
        .await,
        Err(Error::Invalid(_))
    ));
    db.report_terminal(&report, 180).await.unwrap();
    db.report_terminal(&report, 181).await.unwrap();
    assert_eq!(run_status(&db, &run_id).await, "FAILED");
    db.close().await;
    let reopened = Database::open(&path).await.unwrap();
    reopened.report_terminal(&report, 182).await.unwrap();
    let mut connection = reopened.pool.acquire().await.unwrap();
    assert_eq!(
        load_attempt(&mut connection, &attempt_id)
            .await
            .unwrap()
            .status,
        "FAILED"
    );
    drop(connection);
    assert_eq!(run_status(&reopened, &run_id).await, "FAILED");
    reopened.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn worker_cancelled_report_projects_to_run() {
    let (path, db) = temp_db("factory-worker-cancelled").await;
    let attempt_id = authorized_attempt(&db).await;
    let run_id = db.runs().await.unwrap()[0].id.clone();
    let lease = db
        .claim_attempt("worker-a", 100, 50)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run_status(&db, &run_id).await, "RUNNING");
    db.report_terminal(
        &WorkerReport {
            attempt_id: attempt_id.clone(),
            worker_id: "worker-a".into(),
            generation: lease.generation,
            receipt_id: "cancelled-receipt".into(),
            result_sha256: "c".repeat(64),
            terminal_status: "CANCELLED".into(),
            reason: "cancelled".into(),
        },
        110,
    )
    .await
    .unwrap();
    assert_eq!(run_status(&db, &run_id).await, "CANCELLED");
    let events = db.events().await.unwrap();
    assert!(events.iter().any(|event| {
        event.subject_id == run_id
            && event.kind == "run.status_changed"
            && event.detail.contains("status=CANCELLED")
    }));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn p6_concurrent_workers_claim_distinct_attempts() {
    let (path, db) = temp_db("factory-claim").await;
    let first = authorized_attempt(&db).await;
    insert_project(&db, "project-2", "/fixture-2", "base").await;
    let action = db
        .create_action_with_spec(
            "project-2",
            "objective",
            "codex",
            "model",
            "action-key-2",
            ActionSpecInput {
                allowed_paths: &["seed.txt".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let campaign = db
        .create_campaign(&action.id, "campaign-key-2")
        .await
        .unwrap();
    db.authorize_campaign(
        &campaign.id,
        &NewCampaignAuthorization {
            authorization_id: "auth-test-2".into(),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "test".into(),
            rationale: "fixture setup".into(),
            authorized_paths: vec!["seed.txt".into()],
            provider: String::new(),
            model: String::new(),
            provider_mode: String::new(),
            delegation_id: String::new(),
            allowed_tools: Vec::new(),
            monetary_envelope_id: crate::db::authorization::NO_BILLABLE_ENVELOPE.into(),
            progress: fixture_progress(),
            operations: fixture_operations(),
        },
    )
    .await
    .unwrap();
    let run = db.create_run(&campaign.id, "run-key-2").await.unwrap();
    let second = db.create_attempt(&run.id, "attempt-key-2").await.unwrap();
    let left = db.clone();
    let right = db.clone();
    let (a, b) = tokio::join!(
        async move {
            left.claim_attempt("worker-a", 100, 1000)
                .await
                .unwrap()
                .unwrap()
        },
        async move {
            right
                .claim_attempt("worker-b", 100, 1000)
                .await
                .unwrap()
                .unwrap()
        }
    );
    assert_ne!(a.attempt_id, b.attempt_id);
    assert!(a.attempt_id == first || a.attempt_id == second.id);
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn lease_arithmetic_overflow_fails_closed_without_reusing_generation() {
    let (path, db) = temp_db("factory-overflow").await;
    let attempt = authorized_attempt(&db).await;
    assert!(matches!(
        db.claim_attempt("worker-a", i64::MAX - 5, 50).await,
        Err(Error::Invalid(_))
    ));
    assert!(
        db.claim_attempt("worker-a", 100, 50)
            .await
            .unwrap()
            .is_some()
    );
    sqlx::query::query(
        "UPDATE worker_leases SET generation=?1,expires_at_ms=1 WHERE attempt_id=?2",
    )
    .bind(i64::MAX)
    .bind(&attempt)
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(matches!(
        db.claim_attempt("worker-b", 10, 50).await,
        Err(Error::Invalid("worker generation exhausted"))
    ));
    let (generation,): (i64,) =
        sqlx::query_as::query_as("SELECT generation FROM worker_leases WHERE attempt_id=?1")
            .bind(&attempt)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(generation, i64::MAX);
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn worker_registration_stores_only_hash_and_rotates_after_restart() {
    let path = temp_path("factory-worker-session");
    let db = Database::open(&path).await.unwrap();
    assert!(
        db.register_worker_session(
            "worker-a",
            "registration-a",
            "bootstrap-secret-12345678901234567890",
            "session-token-aaaaaaaaaaaaaaaaaaaaaaaa"
        )
        .await
        .unwrap()
        .created
    );
    assert!(
        db.worker_session_matches("worker-a", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
            .await
            .unwrap()
    );
    let workers = db.workers().await.unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].worker_id, "worker-a");
    assert!(
        !db.register_worker_session(
            "worker-a",
            "registration-a",
            "bootstrap-secret-12345678901234567890",
            "session-token-bbbbbbbbbbbbbbbbbbbbbbbb"
        )
        .await
        .unwrap()
        .created
    );
    assert!(
        !db.worker_session_matches("worker-a", "session-token-aaaaaaaaaaaaaaaaaaaaaaaa")
            .await
            .unwrap()
    );
    assert!(matches!(
        db.register_worker_session(
            "worker-a",
            "new-key",
            "bootstrap-secret-12345678901234567890",
            "session-token-cccccccccccccccccccccccc"
        )
        .await,
        Err(Error::Conflict(_))
    ));
    let details: Vec<(String,)> =
        sqlx::query_as::query_as("SELECT detail FROM events WHERE subject_id='worker-a'")
            .fetch_all(&db.pool)
            .await
            .unwrap();
    assert!(
        details
            .iter()
            .all(|(detail,)| !detail.contains("session-token"))
    );
    let (persisted_key,): (String,) = sqlx::query_as::query_as(
        "SELECT key FROM idempotency WHERE operation='register_worker_session'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_ne!(persisted_key, "registration-a");
    assert_eq!(
        persisted_key,
        format!(
            "worker-registration-v1:{}",
            hmac_sha256_hex(b"bootstrap-secret-12345678901234567890", b"registration-a")
        )
    );
    db.close().await;
    let persisted_bytes = std::fs::read(&path).unwrap();
    assert!(
        !persisted_bytes
            .windows(b"registration-a".len())
            .any(|window| window == b"registration-a")
    );
    assert!(
        !persisted_bytes
            .windows(b"bootstrap-secret-12345678901234567890".len())
            .any(|window| window == b"bootstrap-secret-12345678901234567890")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reopened = Database::open(&path).await.unwrap();
    assert!(
        reopened
            .worker_session_matches("worker-a", "session-token-bbbbbbbbbbbbbbbbbbbbbbbb")
            .await
            .unwrap()
    );
    let (persisted_hash,): (String,) = sqlx::query_as::query_as(
        "SELECT token_sha256 FROM worker_sessions WHERE worker_id='worker-a'",
    )
    .fetch_one(&reopened.pool)
    .await
    .unwrap();
    assert_eq!(
        persisted_hash,
        sha256_hex("session-token-bbbbbbbbbbbbbbbbbbbbbbbb".as_bytes())
    );
    reopened.close().await;
    let _ = std::fs::remove_file(path);
}

struct CandidateFixture {
    root: PathBuf,
    db: Database,
    candidate_id: String,
    spec_sha256: String,
    commit_sha: String,
    tree_sha: String,
    worker_id: String,
    generation: i64,
}

impl Drop for CandidateFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git_fixture(root: &Path, arguments: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

async fn enable_semantic_review(
    fixture: &CandidateFixture,
    provider: &str,
    kind: &str,
    model: &str,
    base_url: &str,
) {
    sqlx::query::query("UPDATE factory_settings SET semantic_review_enabled=1 WHERE id=1")
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    fixture
        .db
        .save_provider_settings(
            &ProviderSettingsWrite {
                provider: provider.into(),
                kind: kind.into(),
                model: model.into(),
                base_url: base_url.into(),
                auth_mode: "api_key".into(),
                credential_ciphertext: "ciphertext".into(),
                credential_nonce: "nonce".into(),
                credential_fingerprint: "fingerprint".into(),
                enabled: true,
                is_default: true,
                max_turns: 4,
                max_input_tokens: 60_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                input_micros_per_million: 150_000,
                output_micros_per_million: 600_000,
            },
            "test",
        )
        .await
        .unwrap();
}

async fn candidate_fixture() -> CandidateFixture {
    let root = std::env::temp_dir().join(format!("factory-quality-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_fixture(&repo, &["init", "-q"]);
    git_fixture(&repo, &["config", "user.name", "Fixture"]);
    git_fixture(&repo, &["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(repo.join("seed.txt"), "base\n").unwrap();
    git_fixture(&repo, &["add", "seed.txt"]);
    git_fixture(&repo, &["commit", "-qm", "base"]);

    let db = Database::open(&root.join("factory.sqlite")).await.unwrap();
    let project = db.register_project(&repo, "project-key").await.unwrap();
    let action = db
        .create_action_with_spec(
            &project.id,
            "bounded objective",
            "opencode",
            "test/model",
            "action-key",
            ActionSpecInput {
                allowed_paths: &["seed.txt".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let campaign = db
        .create_campaign(&action.id, "campaign-key")
        .await
        .unwrap();
    db.authorize_campaign(
        &campaign.id,
        &NewCampaignAuthorization {
            authorization_id: "auth-quality".into(),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "test".into(),
            rationale: "quality fixture".into(),
            authorized_paths: vec!["seed.txt".into()],
            provider: String::new(),
            model: String::new(),
            provider_mode: String::new(),
            delegation_id: String::new(),
            allowed_tools: Vec::new(),
            monetary_envelope_id: crate::db::authorization::NO_BILLABLE_ENVELOPE.into(),
            progress: fixture_progress(),
            operations: fixture_operations(),
        },
    )
    .await
    .unwrap();
    let run = db.create_run(&campaign.id, "run-key").await.unwrap();
    let attempt = db.create_attempt(&run.id, "attempt-key").await.unwrap();
    let now_ms = Utc::now().timestamp_millis();
    let lease = db
        .claim_attempt("worker-fixture", now_ms, 60_000)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.attempt_id, attempt.id);

    let workspace_root = root.join("attempts");
    let allocator = crate::workspace::WorkspaceAllocator::new(&repo, &workspace_root).unwrap();
    let mut workspace = allocator
        .allocate(&attempt.id, &project.base_commit)
        .unwrap();
    std::fs::write(workspace.path().join("seed.txt"), "candidate\n").unwrap();
    git_fixture(workspace.path(), &["add", "seed.txt"]);
    git_fixture(workspace.path(), &["commit", "-qm", "candidate"]);
    let revision = workspace.candidate_revision().unwrap();
    workspace.cleanup().unwrap();
    let candidate = db
        .record_candidate(
            &NewCandidate {
                attempt_id: attempt.id.clone(),
                worker_id: "worker-fixture".into(),
                generation: lease.generation,
                action_spec_sha256: action.spec_sha256.clone(),
                commit_sha: revision.commit_sha,
                tree_sha: revision.tree_sha,
                final_source: crate::quality::CandidateFinalSource::ModelFinal,
                salvage: None,
            },
            now_ms,
        )
        .await
        .unwrap();
    CandidateFixture {
        root,
        db,
        candidate_id: candidate.id,
        spec_sha256: candidate.action_spec_sha256,
        commit_sha: candidate.commit_sha,
        tree_sha: candidate.tree_sha,
        worker_id: "worker-fixture".into(),
        generation: lease.generation,
    }
}

struct SalvageFixture {
    root: PathBuf,
    db: Database,
    repo: PathBuf,
    base_commit: String,
    campaign_id: String,
    attempt_id: String,
    worker_id: String,
    generation: i64,
    spec_sha256: String,
    commit_sha: String,
    tree_sha: String,
    checks: Vec<crate::lead::SpecCheck>,
    session_id: String,
    authorization_id: String,
    policy_id: String,
}

impl Drop for SalvageFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn salvage_checks() -> Vec<crate::lead::SpecCheck> {
    use crate::lead::{CheckKind, SpecCheck};
    vec![
        SpecCheck {
            id: "build".into(),
            kind: CheckKind::Build,
            description: "cargo build".into(),
            command: vec!["cargo".into(), "build".into()],
            expect_exit: 0,
            timeout_secs: 120,
            environment_plan_id: None,
            environment_spec_sha256: None,
        },
        SpecCheck {
            id: "test".into(),
            kind: CheckKind::Test,
            description: "cargo test".into(),
            command: vec!["cargo".into(), "test".into()],
            expect_exit: 0,
            timeout_secs: 120,
            environment_plan_id: None,
            environment_spec_sha256: None,
        },
    ]
}

impl SalvageFixture {
    async fn start() -> Self {
        Self::start_with_checks(salvage_checks()).await
    }

    async fn start_with_checks(checks: Vec<crate::lead::SpecCheck>) -> Self {
        let root = std::env::temp_dir().join(format!("factory-salvage-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git_fixture(&repo, &["init", "-q"]);
        git_fixture(&repo, &["config", "user.name", "Fixture"]);
        git_fixture(&repo, &["config", "user.email", "fixture@example.invalid"]);
        std::fs::write(repo.join("seed.txt"), "base\n").unwrap();
        git_fixture(&repo, &["add", "seed.txt"]);
        git_fixture(&repo, &["commit", "-qm", "base"]);
        let db = Database::open(&root.join("factory.sqlite")).await.unwrap();
        let project = db.register_project(&repo, "salvage-project").await.unwrap();
        let action = db
            .create_action_with_spec(
                &project.id,
                "salvage objective",
                "opencode",
                "test/model",
                "salvage-action",
                ActionSpecInput {
                    allowed_paths: &["seed.txt".to_string()],
                    checks: &checks,
                    dependency_plan: None,
                },
            )
            .await
            .unwrap();
        let campaign = db
            .create_campaign(&action.id, "salvage-campaign")
            .await
            .unwrap();
        db.authorize_campaign(
            &campaign.id,
            &NewCampaignAuthorization {
                authorization_id: "auth-salvage".into(),
                risk_class: "R0".into(),
                execution_mode: "FIXTURE".into(),
                actor: "test".into(),
                rationale: "salvage fixture".into(),
                authorized_paths: vec!["seed.txt".into()],
                provider: String::new(),
                model: String::new(),
                provider_mode: String::new(),
                delegation_id: String::new(),
                allowed_tools: Vec::new(),
                monetary_envelope_id: crate::db::authorization::NO_BILLABLE_ENVELOPE.into(),
                progress: fixture_progress(),
                operations: fixture_operations(),
            },
        )
        .await
        .unwrap();
        let authorization = db.campaign_authorization(&campaign.id).await.unwrap();
        let policy = authorization.policy.as_ref().unwrap();
        let run = db.create_run(&campaign.id, "salvage-run").await.unwrap();
        let attempt = db.create_attempt(&run.id, "salvage-attempt").await.unwrap();
        let now_ms = Utc::now().timestamp_millis();
        let lease = db
            .claim_attempt("worker-salvage", now_ms, 60_000)
            .await
            .unwrap()
            .unwrap();
        let allocator =
            crate::workspace::WorkspaceAllocator::new(&repo, root.join("attempts")).unwrap();
        let mut workspace = allocator
            .allocate(&attempt.id, &project.base_commit)
            .unwrap();
        std::fs::write(workspace.path().join("seed.txt"), "salvage candidate\n").unwrap();
        git_fixture(workspace.path(), &["add", "seed.txt"]);
        git_fixture(workspace.path(), &["commit", "-qm", "salvage candidate"]);
        let revision = workspace.candidate_revision().unwrap();
        workspace.cleanup().unwrap();

        let now = Utc::now().to_rfc3339();
        let session_id = "session-salvage".to_string();
        sqlx::query::query(
            "INSERT INTO provider_sessions
             (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,
              state,provider_session_id,turn_id,created_at,updated_at)
             VALUES (?1,?2,'worker-salvage',?3,'opencode','test/model','build','ACTIVE',
                     'ses_salvage','turn-1',?4,?4)",
        )
        .bind(&session_id)
        .bind(&attempt.id)
        .bind(lease.generation)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO provider_turn_reservations
             (reservation_id,session_record_id,attempt_id,generation,provider,model,
              max_input_tokens,max_output_tokens,max_total_tokens,max_cost_micros,
              request_sha256,state,expires_at_ms,created_at,updated_at)
             VALUES ('turn-salvage',?1,?2,?3,'opencode','test/model',1000,1000,2000,0,
                     ?4,'SETTLED',0,?5,?5)",
        )
        .bind(&session_id)
        .bind(&attempt.id)
        .bind(lease.generation)
        .bind("a".repeat(64))
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();

        Self {
            root,
            db,
            repo,
            base_commit: project.base_commit,
            campaign_id: campaign.id,
            attempt_id: attempt.id,
            worker_id: "worker-salvage".into(),
            generation: lease.generation,
            spec_sha256: action.spec_sha256,
            commit_sha: revision.commit_sha,
            tree_sha: revision.tree_sha,
            checks,
            session_id,
            authorization_id: authorization.authorization_id,
            policy_id: policy.policy_id.clone(),
        }
    }

    async fn record_diagnose_checkpoint(&self, decision: &str, stagnant: i64, replans: i64) {
        let now = Utc::now().to_rfc3339();
        let checkpoint_id = format!("diagnose-{decision}-{}", Uuid::new_v4());
        let digest = sha256_hex(checkpoint_id.as_bytes());
        sqlx::query::query(
            "INSERT INTO progress_checkpoints
             (checkpoint_id,campaign_id,work_item_id,attempt_id,operation_id,evidence_ids_json,
              outcome_sha256,classification,decision,stagnant_checkpoints,unsuccessful_replans,
              checkpoint_json,checkpoint_sha256,created_at)
             VALUES (?1,?2,?2,?3,?4,'[]',?5,'STAGNANT',?6,?7,?8,'{}',?5,?9)",
        )
        .bind(&checkpoint_id)
        .bind(&self.campaign_id)
        .bind(&self.attempt_id)
        .bind(format!("op-{checkpoint_id}"))
        .bind(&digest)
        .bind(decision)
        .bind(stagnant)
        .bind(replans)
        .bind(&now)
        .execute(&self.db.pool)
        .await
        .unwrap();
    }

    async fn record_final_nudge(&self) {
        self.db
            .record_provider_tool_event(&ProviderToolEventInput {
                session_record_id: self.session_id.clone(),
                worker_id: self.worker_id.clone(),
                generation: self.generation,
                call_id: "nudge-1".into(),
                tool: "final_nudge".into(),
                decision: "ERROR".into(),
                detail: "final nudge requested".into(),
                check: None,
            })
            .await
            .unwrap();
    }

    async fn record_receipts(&self) {
        for check in &self.checks {
            self.db
                .record_provider_tool_event(&ProviderToolEventInput {
                    session_record_id: self.session_id.clone(),
                    worker_id: self.worker_id.clone(),
                    generation: self.generation,
                    call_id: format!("salvage-check-{}", check.id),
                    tool: "run_command".into(),
                    decision: "RESULT".into(),
                    detail: "exit=Some(0)".into(),
                    check: Some(crate::db::ExecutionCheckInput {
                        command: check.command.clone(),
                        candidate_sha256: "c".repeat(64),
                        exit_code: Some(check.expect_exit),
                        candidate_commit_sha: Some(self.commit_sha.clone()),
                        candidate_tree_sha: Some(self.tree_sha.clone()),
                    }),
                })
                .await
                .unwrap();
        }
    }

    fn submission(&self, commit_sha: &str, tree_sha: &str) -> NewCandidate {
        NewCandidate {
            attempt_id: self.attempt_id.clone(),
            worker_id: self.worker_id.clone(),
            generation: self.generation,
            action_spec_sha256: self.spec_sha256.clone(),
            commit_sha: commit_sha.into(),
            tree_sha: tree_sha.into(),
            final_source: crate::quality::CandidateFinalSource::StagnationSalvage,
            salvage: Some(CandidateSalvageClaim {
                work_item_id: self.campaign_id.clone(),
            }),
        }
    }

    async fn submit(&self) -> Result<CandidateRecord, Error> {
        self.db
            .record_candidate(
                &self.submission(&self.commit_sha, &self.tree_sha),
                Utc::now().timestamp_millis(),
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_raw_receipt(
        &self,
        suffix: &str,
        check_id: &str,
        campaign_id: &str,
        attempt_id: &str,
        work_item_id: &str,
        commit_sha: &str,
        tree_sha: &str,
        outcome: &str,
    ) {
        let now = Utc::now().to_rfc3339();
        let (sequence,): (i64,) = sqlx::query_as::query_as(
            "SELECT COALESCE(MAX(sequence),0)+1 FROM provider_tool_events
             WHERE session_record_id=?1",
        )
        .bind(&self.session_id)
        .fetch_one(&self.db.pool)
        .await
        .unwrap();
        let call_id = format!("raw-{suffix}");
        sqlx::query::query(
            "INSERT INTO provider_tool_events
             (session_record_id,sequence,call_id,tool,decision,detail,created_at)
             VALUES (?1,?2,?3,'run_command','RESULT','exit=Some(0)',?4)",
        )
        .bind(&self.session_id)
        .bind(sequence)
        .bind(&call_id)
        .bind(&now)
        .execute(&self.db.pool)
        .await
        .unwrap();
        let command_json = serde_json::to_string(&self.checks[0].command).unwrap();
        let command_sha256 = sha256_hex(command_json.as_bytes());
        sqlx::query::query(
            "INSERT INTO execution_check_receipts
             (receipt_id,session_record_id,sequence,campaign_id,attempt_id,work_item_id,
              generation,call_id,check_id,suite,command_json,command_sha256,candidate_sha256,
              candidate_commit_sha,candidate_tree_sha,exit_code,expected_exit,outcome,
              result_sha256,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0,0,?16,?17,?18)",
        )
        .bind(format!("raw-receipt-{suffix}"))
        .bind(&self.session_id)
        .bind(sequence)
        .bind(campaign_id)
        .bind(attempt_id)
        .bind(work_item_id)
        .bind(self.generation)
        .bind(&call_id)
        .bind(check_id)
        .bind(crate::db::execution_receipts::suite_for_command(
            &self.checks[0].command,
        ))
        .bind(&command_json)
        .bind(&command_sha256)
        .bind("c".repeat(64))
        .bind(commit_sha)
        .bind(tree_sha)
        .bind(outcome)
        .bind(sha256_hex(format!("raw-{suffix}").as_bytes()))
        .bind(&now)
        .execute(&self.db.pool)
        .await
        .unwrap();
    }

    async fn alternate_candidate(&self) -> (String, String) {
        let allocator = crate::workspace::WorkspaceAllocator::new(
            &self.repo,
            self.root.join("alternate-attempts"),
        )
        .unwrap();
        let mut workspace = allocator
            .allocate("alternate-attempt", &self.base_commit)
            .unwrap();
        std::fs::write(workspace.path().join("seed.txt"), "alternate candidate\n").unwrap();
        git_fixture(workspace.path(), &["add", "seed.txt"]);
        git_fixture(workspace.path(), &["commit", "-qm", "alternate"]);
        let revision = workspace.candidate_revision().unwrap();
        workspace.cleanup().unwrap();
        (revision.commit_sha, revision.tree_sha)
    }
}

fn quality_receipt(fixture: &CandidateFixture, id: &str, principal: &str) -> QualityReceiptInput {
    quality_receipt_for(
        &fixture.spec_sha256,
        &fixture.commit_sha,
        &fixture.tree_sha,
        id,
        principal,
    )
}

fn quality_receipt_for(
    spec_sha256: &str,
    commit_sha: &str,
    tree_sha: &str,
    id: &str,
    principal: &str,
) -> QualityReceiptInput {
    QualityReceiptInput {
        receipt_id: id.into(),
        principal_id: principal.into(),
        reviewer_role: None,
        action_spec_sha256: spec_sha256.into(),
        commit_sha: commit_sha.into(),
        tree_sha: tree_sha.into(),
        result: ReceiptResult::Pass,
        evidence: EvidenceKind::Executed,
        findings: Vec::new(),
        mechanism: ReviewMechanism::Deterministic,
        model_provider: String::new(),
        model_name: String::new(),
        model_reservation_id: String::new(),
        model_request_sha256: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_micros: 0,
        usage_known: false,
    }
}

fn review_finding(
    id: &str,
    severity: crate::review::FindingSeverity,
) -> crate::review::ReviewFinding {
    crate::review::ReviewFinding {
        id: id.into(),
        severity,
        category: crate::review::FindingCategory::Functional,
        message: format!("{severity:?} finding for {id}"),
        points: severity.canonical_points(),
        rationale: "the exact candidate diff demonstrates this finding".into(),
        evidence_refs: vec!["seed.txt".into()],
    }
}

fn quality_reviewer_receipt(
    fixture: &CandidateFixture,
    id: &str,
    principal: &str,
    role: crate::planning::ReviewerRole,
) -> QualityReceiptInput {
    let mut receipt = quality_receipt(fixture, id, principal);
    receipt.reviewer_role = Some(role);
    // Same provider/model is explicitly allowed. Independence comes from the
    // authenticated principal, not from the model route.
    receipt.model_provider = "opencode".into();
    receipt.model_name = "deepseek-v4.1-flash".into();
    receipt
}

async fn insert_reviewer_receipt_direct(
    fixture: &CandidateFixture,
    id: &str,
    principal: &str,
    role: Option<crate::planning::ReviewerRole>,
) {
    sqlx::query::query(
        "INSERT INTO quality_receipts
           (receipt_id,candidate_id,role,principal_id,reviewer_role,action_spec_sha256,
            commit_sha,tree_sha,result,evidence,findings_json,mechanism,model_provider,
            model_name,model_reservation_id,model_request_sha256,input_tokens,output_tokens,
            total_tokens,cost_micros,usage_known,received_at)
         VALUES (?1,?2,'reviewer',?3,?4,?5,?6,?7,'PASS','EXECUTED','[]','DETERMINISTIC',
                 '','','','',0,0,0,0,0,?8)",
    )
    .bind(id)
    .bind(&fixture.candidate_id)
    .bind(principal)
    .bind(role.map(|role| role.as_str()).unwrap_or(""))
    .bind(&fixture.spec_sha256)
    .bind(&fixture.commit_sha)
    .bind(&fixture.tree_sha)
    .bind(Utc::now().to_rfc3339())
    .execute(&fixture.db.pool)
    .await
    .unwrap();
}

async fn insert_reviewer_receipt_with_findings_direct(
    fixture: &CandidateFixture,
    id: &str,
    principal: &str,
    result: &str,
    findings: &[crate::review::ReviewFinding],
) {
    let findings_json = serde_json::to_string(findings).unwrap();
    sqlx::query::query(
        "INSERT INTO quality_receipts
           (receipt_id,candidate_id,role,principal_id,reviewer_role,action_spec_sha256,
            commit_sha,tree_sha,result,evidence,findings_json,mechanism,model_provider,
            model_name,model_reservation_id,model_request_sha256,input_tokens,output_tokens,
            total_tokens,cost_micros,usage_known,received_at)
         VALUES (?1,?2,'reviewer',?3,'',?4,?5,?6,?7,'EXECUTED',?8,'DETERMINISTIC',
                 '','','','',0,0,0,0,0,'2000-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(&fixture.candidate_id)
    .bind(principal)
    .bind(&fixture.spec_sha256)
    .bind(&fixture.commit_sha)
    .bind(&fixture.tree_sha)
    .bind(result)
    .bind(findings_json)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
}

async fn attach_finding_request(fixture: &CandidateFixture) -> String {
    let (campaign_id, project_id, action_id): (String, String, String) = sqlx::query_as::query_as(
        "SELECT r.campaign_id,ac.project_id,ac.id
             FROM candidates cd
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             WHERE cd.id=?1",
    )
    .bind(&fixture.candidate_id)
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    let suffix = Uuid::new_v4();
    let profile_id = format!("profile-{suffix}");
    let revision_id = format!("profile-revision-{suffix}");
    let delegation_id = format!("delegation-{suffix}");
    let lead_id = format!("lead-{suffix}");
    let request_id = format!("request-{suffix}");
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO execution_profiles(id,current_revision,content_hash,created_at,updated_at)
         VALUES (?1,1,'hash',?2,?2)",
    )
    .bind(&profile_id)
    .bind(&now)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO execution_profile_revisions
         (revision_id,profile_id,revision,content_json,content_hash,actor,created_at)
         VALUES (?1,?2,1,'{}','hash','owner',?3)",
    )
    .bind(&revision_id)
    .bind(&profile_id)
    .bind(&now)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
         VALUES (?1,'owner','audit',?2,1,'hash','{}',?3)",
    )
    .bind(&delegation_id)
    .bind(&project_id)
    .bind(&now)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO lead_requests
         (id,project_id,action_id,delegation_id,objective,request_hash,
          project_config_hash,state,created_at,updated_at)
         VALUES (?1,?2,?3,?4,'objective','hash','hash','FROZEN',?5,?5)",
    )
    .bind(&lead_id)
    .bind(&project_id)
    .bind(&action_id)
    .bind(&delegation_id)
    .bind(&now)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO intake_requests
         (id,project_id,objective,mode,profile_id,profile_revision_id,profile_revision,
          profile_hash,profile_name,action_id,delegation_id,lead_request_id,campaign_id,
          submission_json,submission_hash,idempotency_key,created_at,updated_at)
         VALUES (?1,?2,'objective','REVIEW_PLAN',?3,?4,1,'hash','profile',?5,?6,?7,?8,
                 '{}','hash',?9,?10,?10)",
    )
    .bind(&request_id)
    .bind(&project_id)
    .bind(&profile_id)
    .bind(&revision_id)
    .bind(&action_id)
    .bind(&delegation_id)
    .bind(&lead_id)
    .bind(&campaign_id)
    .bind(format!("intake-{suffix}"))
    .bind(&now)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    request_id
}

async fn set_reviewer_requirement(
    db: &Database,
    candidate_id: &str,
    count: i64,
    roles: &[crate::planning::ReviewerRole],
    assignments: &[(crate::planning::ReviewerRole, &str)],
) {
    let (campaign_id,): (String,) = sqlx::query_as::query_as(
        "SELECT r.campaign_id FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let roles_json =
        serde_json::to_string(&roles.iter().map(|role| role.as_str()).collect::<Vec<_>>()).unwrap();
    let assignments_json = serde_json::to_string(
        &assignments
            .iter()
            .map(|(role, principal)| {
                serde_json::json!({
                    "role": role.as_str(),
                    "principalId": principal,
                    "provider": "opencode",
                    "model": "deepseek-v4.1-flash",
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    sqlx::query::query(
        "INSERT INTO campaign_review_requirements
           (campaign_id,reviewer_count,required_roles_json,required_assignments_json,created_at)
         VALUES (?1,?2,?3,?4,?5)",
    )
    .bind(campaign_id)
    .bind(count)
    .bind(roles_json)
    .bind(assignments_json)
    .bind(Utc::now().to_rfc3339())
    .execute(&db.pool)
    .await
    .unwrap();
}

async fn settled_reviewer_model_call(
    db: &Database,
    candidate_id: &str,
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_micros: u64,
) -> String {
    let (project_id,): (String,) = sqlx::query_as::query_as(
        "SELECT ac.project_id FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         JOIN campaigns c ON c.id=r.campaign_id
         JOIN actions ac ON ac.id=c.action_id WHERE cd.id=?1",
    )
    .bind(candidate_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let reservation_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO model_call_reservations
         (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
          purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
          max_cost_micros,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
          response_text,state,expires_at_ms,created_at,updated_at)
         VALUES (?1,'SEMANTIC_REVIEWER','reviewer-model',?2,?3,?3,?4,?5,'semantic_review',
                 ?6,10000,4096,14096,100000,?7,?8,?9,?10,1,'{}','SETTLED',0,?11,?11)",
    )
    .bind(&reservation_id)
    .bind(&project_id)
    .bind(candidate_id)
    .bind(provider)
    .bind(model)
    .bind(sha256_hex(b"review-request"))
    .bind(i64::try_from(input_tokens).unwrap())
    .bind(i64::try_from(output_tokens).unwrap())
    .bind(i64::try_from(input_tokens + output_tokens).unwrap())
    .bind(i64::try_from(cost_micros).unwrap())
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    reservation_id
}

#[allow(clippy::too_many_arguments)]
fn model_review_receipt(
    fixture: &CandidateFixture,
    id: &str,
    result: ReceiptResult,
    reservation_id: &str,
    provider: &str,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_micros: u64,
) -> QualityReceiptInput {
    let findings = if result == ReceiptResult::Pass {
        Vec::new()
    } else {
        vec![crate::review::ReviewFinding {
            id: "semantic-1".into(),
            severity: crate::review::FindingSeverity::Blocking,
            category: crate::review::FindingCategory::Functional,
            message: "bounded semantic defect".into(),
            points: 5,
            rationale: "The diff contains a bounded semantic defect.".into(),
            evidence_refs: vec!["seed.txt".into()],
        }]
    };
    QualityReceiptInput {
        receipt_id: id.into(),
        principal_id: "reviewer-model".into(),
        reviewer_role: None,
        action_spec_sha256: fixture.spec_sha256.clone(),
        commit_sha: fixture.commit_sha.clone(),
        tree_sha: fixture.tree_sha.clone(),
        result,
        evidence: EvidenceKind::Executed,
        findings,
        mechanism: ReviewMechanism::Model,
        model_provider: provider.into(),
        model_name: model.into(),
        model_reservation_id: reservation_id.into(),
        model_request_sha256: sha256_hex(b"review-request"),
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
        cost_micros,
        usage_known: true,
    }
}

#[tokio::test]
async fn multiple_independent_reviewers_are_aggregated_into_one_gate() {
    let fixture = candidate_fixture().await;
    let (campaign_id,): (String,) = sqlx::query_as::query_as(
        "SELECT r.campaign_id FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         WHERE cd.id=?1",
    )
    .bind(&fixture.candidate_id)
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO campaign_review_requirements (campaign_id,reviewer_count,created_at)
         VALUES (?1,2,?2)",
    )
    .bind(&campaign_id)
    .bind(Utc::now().to_rfc3339())
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    let verifier = quality_receipt(&fixture, "verifier-multi", "verifier");
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &verifier)
        .await
        .unwrap();
    let first = quality_receipt(&fixture, "reviewer-a", "reviewer-a");
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &first)
        .await
        .unwrap();
    assert!(
        ingested.gate.is_none(),
        "one of two reviewers must not decide the Gate"
    );
    let second = quality_receipt(&fixture, "reviewer-b", "reviewer-b");
    let pending = fixture
        .db
        .pending_quality_candidates("reviewer")
        .await
        .unwrap();
    assert!(pending.contains(&fixture.candidate_id));
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &second)
        .await
        .unwrap();
    assert_eq!(ingested.gate.unwrap().outcome, GateOutcome::Accepted);
    let pending = fixture
        .db
        .pending_quality_candidates("reviewer")
        .await
        .unwrap();
    assert!(!pending.contains(&fixture.candidate_id));
    let reviewers = fixture
        .db
        .gate_reviewer_ids(&fixture.candidate_id)
        .await
        .unwrap();
    assert_eq!(reviewers.len(), 2);
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn gate_accepts_distinct_principals_with_same_model_and_exact_role_assignments() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[
            (ReviewerRole::Functional, "reviewer-a"),
            (ReviewerRole::CodeQuality, "reviewer-b"),
        ],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-role-coverage", "verifier-1"),
        )
        .await
        .unwrap();
    let first = quality_reviewer_receipt(
        &fixture,
        "reviewer-functional-role",
        "reviewer-a",
        ReviewerRole::Functional,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &first)
        .await
        .unwrap();
    assert!(
        ingested.gate.is_none(),
        "one of two required roles must not decide the Gate"
    );
    let second = quality_reviewer_receipt(
        &fixture,
        "reviewer-code-quality-role",
        "reviewer-b",
        ReviewerRole::CodeQuality,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &second)
        .await
        .unwrap();
    assert_eq!(ingested.gate.unwrap().outcome, GateOutcome::Accepted);
    let receipts = fixture
        .db
        .quality_receipts(&fixture.candidate_id)
        .await
        .unwrap();
    let reviewers = receipts
        .iter()
        .filter(|receipt| receipt.role == QualityRole::Reviewer)
        .collect::<Vec<_>>();
    assert_eq!(reviewers.len(), 2);
    assert!(
        reviewers
            .iter()
            .all(|receipt| receipt.model_provider == "opencode"
                && receipt.model_name == "deepseek-v4.1-flash")
    );
    assert_eq!(
        reviewers
            .iter()
            .map(|receipt| receipt.principal_id.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn gate_blocks_required_roles_without_frozen_principal_assignments() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-unassigned-roles", "verifier-1"),
        )
        .await
        .unwrap();
    let receipt = quality_reviewer_receipt(
        &fixture,
        "reviewer-unassigned-roles",
        "reviewer-a",
        ReviewerRole::Functional,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &receipt)
        .await
        .unwrap();
    let gate = ingested.gate.expect("an unbound plan must decide the Gate");
    assert_eq!(gate.outcome, GateOutcome::Blocked);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("does not bind every required role"))
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn gate_rejects_an_assignment_set_with_an_unexpected_role() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[
            (ReviewerRole::Functional, "reviewer-a"),
            (ReviewerRole::Security, "reviewer-b"),
        ],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-unexpected-assignment", "verifier-1"),
        )
        .await
        .unwrap();
    let receipt = quality_reviewer_receipt(
        &fixture,
        "reviewer-unexpected-assignment",
        "reviewer-a",
        ReviewerRole::Functional,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &receipt)
        .await
        .unwrap();
    let gate = ingested
        .gate
        .expect("a mismatched plan must decide the Gate");
    assert_eq!(gate.outcome, GateOutcome::Blocked);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("does not bind every required role"))
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn reviewer_receipt_rejects_a_role_outside_the_frozen_plan() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[
            (ReviewerRole::Functional, "reviewer-a"),
            (ReviewerRole::CodeQuality, "reviewer-b"),
        ],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-extra-role", "verifier-1"),
        )
        .await
        .unwrap();
    let extra = quality_reviewer_receipt(
        &fixture,
        "reviewer-extra-role",
        "reviewer-security",
        ReviewerRole::Security,
    );
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &extra)
            .await,
        Err(Error::Forbidden(_))
    ));
    let receipts = fixture
        .db
        .quality_receipts(&fixture.candidate_id)
        .await
        .unwrap();
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt.role != QualityRole::Reviewer)
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn gate_blocks_an_extra_role_receipt_even_when_required_roles_pass() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[
            (ReviewerRole::Functional, "reviewer-a"),
            (ReviewerRole::CodeQuality, "reviewer-b"),
        ],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-extra-gate", "verifier-1"),
        )
        .await
        .unwrap();
    let functional = quality_reviewer_receipt(
        &fixture,
        "reviewer-extra-gate-functional",
        "reviewer-a",
        ReviewerRole::Functional,
    );
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &functional)
        .await
        .unwrap();
    // Simulate a historical/foreign row that bypassed ingestion validation.
    insert_reviewer_receipt_direct(
        &fixture,
        "reviewer-extra-gate-security",
        "reviewer-security",
        Some(ReviewerRole::Security),
    )
    .await;
    let required = quality_reviewer_receipt(
        &fixture,
        "reviewer-extra-gate-code-quality",
        "reviewer-b",
        ReviewerRole::CodeQuality,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &required)
        .await
        .unwrap();
    let gate = ingested
        .gate
        .expect("an extra-role receipt must decide the Gate");
    assert_ne!(gate.outcome, GateOutcome::Accepted);
    assert_eq!(gate.outcome, GateOutcome::Blocked);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("not required by the frozen plan"))
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn gate_rejects_a_second_receipt_from_the_same_principal() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[
            (ReviewerRole::Functional, "reviewer-a"),
            (ReviewerRole::CodeQuality, "reviewer-b"),
        ],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-duplicate-principal", "verifier-1"),
        )
        .await
        .unwrap();
    let first = quality_reviewer_receipt(
        &fixture,
        "reviewer-duplicate-principal-one",
        "reviewer-a",
        ReviewerRole::Functional,
    );
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &first)
        .await
        .unwrap();
    let duplicate = quality_reviewer_receipt(
        &fixture,
        "reviewer-duplicate-principal-two",
        "reviewer-a",
        ReviewerRole::CodeQuality,
    );
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &duplicate)
            .await,
        Err(Error::Conflict(_))
    ));
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn gate_blocks_a_roleless_legacy_receipt_when_required_roles_exist() {
    use crate::planning::ReviewerRole;
    let fixture = candidate_fixture().await;
    set_reviewer_requirement(
        &fixture.db,
        &fixture.candidate_id,
        2,
        &[ReviewerRole::Functional, ReviewerRole::CodeQuality],
        &[
            (ReviewerRole::Functional, "reviewer-a"),
            (ReviewerRole::CodeQuality, "reviewer-c"),
        ],
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-role-gap", "verifier-1"),
        )
        .await
        .unwrap();
    // The first receipt waits for the second required role. The direct insert
    // simulates a historical roleless receipt that predates role ingestion
    // validation; once the required roles also pass, that legacy row must block
    // the Gate rather than remain in an accepted set.
    let functional = quality_reviewer_receipt(
        &fixture,
        "reviewer-role-gap-a",
        "reviewer-a",
        ReviewerRole::Functional,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &functional)
        .await
        .unwrap();
    assert!(ingested.gate.is_none());
    insert_reviewer_receipt_direct(&fixture, "reviewer-role-gap-b", "reviewer-b", None).await;
    assert!(
        fixture
            .db
            .gate_for_candidate(&fixture.candidate_id)
            .await
            .unwrap()
            .is_none()
    );
    let pending = fixture
        .db
        .pending_quality_candidates_for_reviewer(
            "reviewer-code-quality-process",
            ReviewerRole::CodeQuality,
        )
        .await
        .unwrap();
    assert!(pending.contains(&fixture.candidate_id));
    let final_receipt = quality_reviewer_receipt(
        &fixture,
        "reviewer-role-gap-c",
        "reviewer-c",
        ReviewerRole::CodeQuality,
    );
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &final_receipt)
        .await
        .unwrap();
    let gate = ingested
        .gate
        .expect("a roleless receipt must decide the Gate");
    assert_eq!(gate.outcome, GateOutcome::Blocked);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("has no role while the frozen plan requires"))
    );
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn codex_app_server_review_requires_exact_provenance_without_a_reservation() {
    let fixture = candidate_fixture().await;
    enable_semantic_review(
        &fixture,
        "codex",
        "codex",
        "gpt-6-luna",
        "https://api.openai.com/v1",
    )
    .await;
    let mut valid = quality_receipt(&fixture, "codex-review", "reviewer");
    valid.mechanism = ReviewMechanism::CodexAppServer;
    valid.model_provider = "codex".into();
    valid.model_name = "gpt-6-luna".into();
    valid.model_request_sha256 = sha256_hex(b"codex-review-request");
    valid.usage_known = true;

    let mut wrong_provider = valid.clone();
    wrong_provider.receipt_id = "codex-review-wrong-provider".into();
    wrong_provider.model_provider = "opencode".into();
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(
                &fixture.candidate_id,
                QualityRole::Reviewer,
                &wrong_provider,
            )
            .await,
        Err(Error::Forbidden(_))
    ));

    let mut reserved = valid.clone();
    reserved.receipt_id = "codex-review-reserved".into();
    reserved.model_reservation_id = "reservation".into();
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &reserved,)
            .await,
        Err(Error::Forbidden(_))
    ));

    let verifier = quality_receipt(&fixture, "verifier-codex", "verifier");
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &verifier)
        .await
        .unwrap();
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &valid)
        .await
        .unwrap();
    assert_eq!(ingested.receipt.mechanism, ReviewMechanism::CodexAppServer);
    assert!(ingested.receipt.model_reservation_id.is_empty());
    assert_eq!(ingested.gate.unwrap().outcome, GateOutcome::Accepted);
    let mut duplicate = valid;
    duplicate.receipt_id = "codex-review-duplicate".into();
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &duplicate)
            .await,
        Err(Error::Conflict(_))
    ));
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn codex_app_server_failure_receipt_preserves_unknown_usage() {
    let fixture = candidate_fixture().await;
    enable_semantic_review(
        &fixture,
        "codex",
        "codex",
        "gpt-6-luna",
        "https://api.openai.com/v1",
    )
    .await;
    let verifier = quality_receipt(&fixture, "verifier-codex-failure", "verifier");
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &verifier)
        .await
        .unwrap();
    let mut failed = quality_receipt(&fixture, "codex-review-failure", "reviewer");
    failed.mechanism = ReviewMechanism::CodexAppServer;
    failed.model_provider = "codex".into();
    failed.model_name = "gpt-6-luna".into();
    failed.model_request_sha256 = sha256_hex(b"codex-review-failure-request");
    failed.result = ReceiptResult::NotTested;
    failed.evidence = EvidenceKind::Unknown;
    failed.findings = vec![crate::review::ReviewFinding {
        id: "codex-review-unavailable".into(),
        severity: crate::review::FindingSeverity::Blocking,
        category: crate::review::FindingCategory::Other,
        message: "Codex authentication is unavailable".into(),
        points: 5,
        rationale: "The reviewer could not authenticate to the native adapter.".into(),
        evidence_refs: vec![fixture.candidate_id.clone()],
    }];
    assert!(!failed.usage_known);
    let ingested = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &failed)
        .await
        .unwrap();
    assert!(!ingested.receipt.usage_known);
    assert_eq!(ingested.gate.unwrap().outcome, GateOutcome::NotTested);
    fixture.db.close().await;
    let _ = std::fs::remove_file(fixture.root.join("factory.sqlite"));
}

#[tokio::test]
async fn model_review_approval_requires_matching_settled_reservation() {
    let fixture = candidate_fixture().await;
    enable_semantic_review(
        &fixture,
        "opencode",
        "opencode",
        "deepseek-v4.1-flash",
        "https://gateway.example/v1",
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-model", "verifier-1"),
        )
        .await
        .unwrap();
    let reservation = settled_reviewer_model_call(
        &fixture.db,
        &fixture.candidate_id,
        "opencode",
        "deepseek-v4.1-flash",
        10,
        20,
        25,
    )
    .await;
    let mut wrong_usage = model_review_receipt(
        &fixture,
        "reviewer-model-wrong",
        ReceiptResult::Pass,
        &reservation,
        "opencode",
        "deepseek-v4.1-flash",
        11,
        20,
        25,
    );
    wrong_usage.model_provider = "opencode".into();
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &wrong_usage)
            .await,
        Err(Error::Forbidden(_))
    ));
    let accepted = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &model_review_receipt(
                &fixture,
                "reviewer-model-pass",
                ReceiptResult::Pass,
                &reservation,
                "opencode",
                "deepseek-v4.1-flash",
                10,
                20,
                25,
            ),
        )
        .await
        .unwrap();
    assert_eq!(accepted.gate.unwrap().outcome, GateOutcome::Accepted);
    let receipts = fixture
        .db
        .quality_receipts(&fixture.candidate_id)
        .await
        .unwrap();
    let reviewer = receipts
        .iter()
        .find(|receipt| receipt.role == QualityRole::Reviewer)
        .unwrap();
    assert_eq!(reviewer.mechanism, ReviewMechanism::Model);
    assert_eq!(reviewer.model_provider, "opencode");
    assert_eq!(reviewer.model_name, "deepseek-v4.1-flash");
    assert_eq!(reviewer.model_reservation_id, reservation);
    assert_eq!(reviewer.input_tokens, 10);
    assert_eq!(reviewer.output_tokens, 20);
    assert_eq!(reviewer.total_tokens, 30);
    assert_eq!(reviewer.cost_micros, 25);
    assert!(reviewer.usage_known);
    fixture.db.close().await;
}

#[tokio::test]
async fn changes_requested_and_uncertain_never_accept() {
    let changed = candidate_fixture().await;
    changed
        .db
        .record_quality_receipt(
            &changed.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&changed, "verifier-change", "verifier-1"),
        )
        .await
        .unwrap();
    let mut requested = quality_receipt(&changed, "reviewer-change", "reviewer-1");
    requested.result = ReceiptResult::ChangesRequested;
    let result = changed
        .db
        .record_quality_receipt(&changed.candidate_id, QualityRole::Reviewer, &requested)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::RepairRequired);
    changed.db.close().await;

    let uncertain = candidate_fixture().await;
    uncertain
        .db
        .record_quality_receipt(
            &uncertain.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&uncertain, "verifier-uncertain", "verifier-1"),
        )
        .await
        .unwrap();
    let mut not_tested = quality_receipt(&uncertain, "reviewer-uncertain", "reviewer-1");
    not_tested.result = ReceiptResult::NotTested;
    let result = uncertain
        .db
        .record_quality_receipt(&uncertain.candidate_id, QualityRole::Reviewer, &not_tested)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::NotTested);
    uncertain.db.close().await;
}

#[tokio::test]
async fn accepted_gate_projects_run_status_and_survives_restart() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    assert_eq!(run_status(&fixture.db, &run_id).await, "RUNNING");
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-run-accepted", "verifier-1"),
        )
        .await
        .unwrap();
    assert_eq!(run_status(&fixture.db, &run_id).await, "RUNNING");
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-run-accepted", "reviewer-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::Accepted);
    assert_eq!(run_status(&fixture.db, &run_id).await, "ACCEPTED");
    let events = fixture.db.events().await.unwrap();
    assert!(events.iter().any(|event| {
        event.subject_id == run_id
            && event.kind == "run.status_changed"
            && event.detail.contains("status=ACCEPTED")
    }));
    let database_path = fixture.root.join("factory.sqlite");
    fixture.db.close().await;
    let reopened = Database::open(&database_path).await.unwrap();
    assert_eq!(run_status(&reopened, &run_id).await, "ACCEPTED");
    reopened.close().await;
}

#[tokio::test]
async fn critical_review_finding_is_a_human_stop() {
    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-critical", "verifier-1"),
        )
        .await
        .unwrap();
    let critical = crate::review::ReviewFinding {
        id: "critical-1".into(),
        severity: crate::review::FindingSeverity::Critical,
        category: crate::review::FindingCategory::Security,
        message: "credential boundary is broken".into(),
        points: 8,
        rationale: "the diff exposes a credential boundary".into(),
        evidence_refs: vec!["seed.txt".into()],
    };
    let mut reviewer = quality_receipt(&fixture, "reviewer-critical", "reviewer-1");
    reviewer.result = ReceiptResult::ChangesRequested;
    reviewer.findings = vec![critical.clone()];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &reviewer)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::HumanRequired);
    let repair = fixture
        .db
        .repair_authorization_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("critical repair decision");
    assert_eq!(repair.decision, "HUMAN_REQUIRED");
    assert_eq!(repair.reason_code, "critical_zero");
    assert_eq!(repair.critical_count, 1);

    let mut contradictory = quality_receipt(&fixture, "reviewer-critical-pass", "reviewer-1");
    contradictory.result = ReceiptResult::Pass;
    contradictory.findings = vec![critical];
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &contradictory)
            .await,
        Err(Error::Invalid(_))
    ));
    fixture.db.close().await;
}

#[tokio::test]
async fn repair_required_gate_projects_run_status() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-run-repair", "verifier-1"),
        )
        .await
        .unwrap();
    let mut reviewer = quality_receipt(&fixture, "reviewer-run-repair", "reviewer-1");
    reviewer.result = ReceiptResult::ChangesRequested;
    reviewer.findings = vec![crate::review::ReviewFinding {
        id: "repair-1".into(),
        severity: crate::review::FindingSeverity::Blocking,
        category: crate::review::FindingCategory::Functional,
        message: "the candidate needs a bounded repair".into(),
        points: 5,
        rationale: "the deterministic evidence shows a functional defect".into(),
        evidence_refs: vec!["seed.txt".into()],
    }];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &reviewer)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::RepairRequired);
    assert_eq!(run_status(&fixture.db, &run_id).await, "REPAIR_REQUIRED");
    let repair = fixture
        .db
        .repair_authorization_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("repair decision is recorded");
    assert_eq!(repair.decision, "DENIED");
    assert_eq!(repair.reason_code, "autonomy_disabled");
    assert_eq!(repair.total_points, 5);
    assert_eq!(repair.blocking_count, 1);
    fixture.db.close().await;
}

#[tokio::test]
async fn failed_verifier_gates_without_waiting_for_reviewer() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    let mut verifier = quality_receipt(&fixture, "verifier-run-verifier-fail", "verifier-1");
    verifier.result = ReceiptResult::Fail;
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &verifier)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::RepairRequired);
    assert_eq!(run_status(&fixture.db, &run_id).await, "REPAIR_REQUIRED");
    assert!(
        fixture
            .db
            .pending_quality_candidates("reviewer")
            .await
            .unwrap()
            .is_empty()
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn enabled_semantic_review_blocks_a_late_deterministic_reviewer_pass() {
    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-late-deterministic", "reviewer-1"),
        )
        .await
        .unwrap();
    sqlx::query::query("UPDATE factory_settings SET semantic_review_enabled=1 WHERE id=1")
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    fixture
        .db
        .save_provider_settings(
            &ProviderSettingsWrite {
                provider: "opencode-go".into(),
                kind: "opencode".into(),
                model: "deepseek-v4.1-flash".into(),
                base_url: "https://opencode.ai/inference/go/openai/v1".into(),
                auth_mode: "api_key".into(),
                credential_ciphertext: "ciphertext".into(),
                credential_nonce: "nonce".into(),
                credential_fingerprint: "fingerprint".into(),
                enabled: true,
                is_default: true,
                max_turns: 4,
                max_input_tokens: 60_000,
                max_output_tokens: 12_000,
                max_cost_micros: 500_000,
                input_micros_per_million: 150_000,
                output_micros_per_million: 600_000,
            },
            "test",
        )
        .await
        .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-after-policy", "verifier-1"),
        )
        .await
        .unwrap();
    let gate = result.gate.unwrap();
    assert_eq!(gate.outcome, GateOutcome::NotTested);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("MODEL") || reason.contains("frozen"))
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn blocked_gate_projects_run_status_to_rejected() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-run-blocked", "verifier-1"),
        )
        .await
        .unwrap();
    let mut reviewer = quality_receipt(&fixture, "reviewer-run-blocked", "reviewer-1");
    reviewer.evidence = EvidenceKind::Mocked;
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &reviewer)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::Blocked);
    assert_eq!(run_status(&fixture.db, &run_id).await, "REJECTED");
    fixture.db.close().await;
}

#[tokio::test]
async fn human_required_gate_projects_run_status_to_rejected() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-run-human", "verifier-1"),
        )
        .await
        .unwrap();
    sqlx::query::query(
        "UPDATE campaigns SET status='BLOCKED_AUTHORIZATION'
         WHERE id=(SELECT r.campaign_id FROM candidates cd
                   JOIN attempts a ON a.id=cd.attempt_id
                   JOIN runs r ON r.id=a.run_id
                   WHERE cd.id=?1)",
    )
    .bind(&fixture.candidate_id)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-run-human", "reviewer-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::HumanRequired);
    assert_eq!(run_status(&fixture.db, &run_id).await, "REJECTED");
    fixture.db.close().await;
}

#[tokio::test]
async fn not_tested_gate_projects_run_status_to_rejected() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-run-not-tested", "verifier-1"),
        )
        .await
        .unwrap();
    let mut reviewer = quality_receipt(&fixture, "reviewer-run-not-tested", "reviewer-1");
    reviewer.result = ReceiptResult::NotTested;
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &reviewer)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::NotTested);
    assert_eq!(run_status(&fixture.db, &run_id).await, "REJECTED");
    fixture.db.close().await;
}

#[tokio::test]
async fn gate_does_not_overwrite_a_terminal_run() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-run-terminal", "verifier-1"),
        )
        .await
        .unwrap();
    sqlx::query::query("UPDATE runs SET status='CANCELLED' WHERE id=?1")
        .bind(&run_id)
        .execute(&fixture.db.pool)
        .await
        .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-run-terminal", "reviewer-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::Accepted);
    assert_eq!(run_status(&fixture.db, &run_id).await, "CANCELLED");
    let events = fixture.db.events().await.unwrap();
    assert!(!events.iter().any(|event| {
        event.subject_id == run_id
            && event.kind == "run.status_changed"
            && event.detail.contains("status=ACCEPTED")
    }));
    fixture.db.close().await;
}

#[tokio::test]
async fn model_review_findings_and_provenance_survive_restart() {
    let fixture = candidate_fixture().await;
    enable_semantic_review(
        &fixture,
        "opencode",
        "opencode",
        "deepseek-v4.1-flash",
        "https://gateway.example/v1",
    )
    .await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-restart", "verifier-1"),
        )
        .await
        .unwrap();
    let reservation = settled_reviewer_model_call(
        &fixture.db,
        &fixture.candidate_id,
        "opencode",
        "deepseek-v4.1-flash",
        7,
        11,
        13,
    )
    .await;
    let mut receipt = model_review_receipt(
        &fixture,
        "reviewer-restart",
        ReceiptResult::ChangesRequested,
        &reservation,
        "opencode",
        "deepseek-v4.1-flash",
        7,
        11,
        13,
    );
    receipt.principal_id = "reviewer-restart".into();
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &receipt)
        .await
        .unwrap();
    let database_path = fixture.root.join("factory.sqlite");
    fixture.db.close().await;

    let reopened = Database::open(&database_path).await.unwrap();
    let receipts = reopened
        .quality_receipts(&fixture.candidate_id)
        .await
        .unwrap();
    let reviewer = receipts
        .iter()
        .find(|receipt| receipt.role == QualityRole::Reviewer)
        .unwrap();
    assert_eq!(reviewer.result, ReceiptResult::ChangesRequested);
    assert_eq!(reviewer.findings.len(), 1);
    assert_eq!(reviewer.findings[0].id, "semantic-1");
    assert_eq!(reviewer.model_reservation_id, reservation);
    assert_eq!(reviewer.total_tokens, 18);
    let gate = reopened
        .gate_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gate.outcome, GateOutcome::RepairRequired);
    reopened.close().await;
}

#[tokio::test]
async fn candidate_and_independent_receipts_produce_deterministic_gate() {
    let fixture = candidate_fixture().await;
    let candidate = fixture.db.candidate(&fixture.candidate_id).await.unwrap();
    assert_eq!(candidate.commit_sha, fixture.commit_sha);
    assert_eq!(candidate.implementer_id, fixture.worker_id);

    let first = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-receipt-1", "verifier-1"),
        )
        .await
        .unwrap();
    assert!(first.gate.is_none());

    let second = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-receipt-1", "reviewer-1"),
        )
        .await
        .unwrap();
    assert_eq!(second.gate.unwrap().outcome, GateOutcome::Accepted);

    let replay = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-receipt-1", "verifier-1"),
        )
        .await
        .unwrap();
    assert_eq!(replay.gate.unwrap().outcome, GateOutcome::Accepted);
    assert_eq!(
        fixture
            .db
            .quality_receipts(&fixture.candidate_id)
            .await
            .unwrap()
            .len(),
        2
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn gate_requires_pass_verifier_and_reviewer() {
    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-pass-1", "verifier-1"),
        )
        .await
        .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-pass-1", "reviewer-1"),
        )
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::Accepted);
    assert_eq!(
        fixture
            .db
            .gate_for_candidate(&fixture.candidate_id)
            .await
            .unwrap()
            .unwrap()
            .outcome,
        GateOutcome::Accepted
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn receipt_rejects_implementer_self_review_and_role_collusion() {
    let fixture = candidate_fixture().await;
    let self_review = quality_receipt(&fixture, "self", &fixture.worker_id);
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &self_review)
            .await,
        Err(Error::Forbidden(_))
    ));
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier", "same-principal"),
        )
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(
                &fixture.candidate_id,
                QualityRole::Reviewer,
                &quality_receipt(&fixture, "reviewer", "same-principal")
            )
            .await,
        Err(Error::Forbidden(_))
    ));
    fixture.db.close().await;
}

#[tokio::test]
async fn receipt_rejects_conflicts_wrong_binding_and_forged_generation() {
    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier", "verifier-1"),
        )
        .await
        .unwrap();
    let mut changed = quality_receipt(&fixture, "verifier", "verifier-1");
    changed.result = ReceiptResult::Fail;
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &changed)
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(
                &fixture.candidate_id,
                QualityRole::Verifier,
                &quality_receipt(&fixture, "another-verifier", "verifier-2")
            )
            .await,
        Err(Error::Conflict(_))
    ));
    let mut wrong = quality_receipt(&fixture, "verifier-wrong", "verifier-3");
    wrong.tree_sha = "0".repeat(40);
    assert!(matches!(
        fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &wrong)
            .await,
        Err(Error::Forbidden(_))
    ));
    let candidate = fixture.db.candidate(&fixture.candidate_id).await.unwrap();
    assert!(
        fixture
            .db
            .record_candidate(
                &NewCandidate {
                    attempt_id: candidate.attempt_id,
                    worker_id: fixture.worker_id.clone(),
                    generation: fixture.generation + 1,
                    action_spec_sha256: fixture.spec_sha256.clone(),
                    commit_sha: candidate.commit_sha,
                    tree_sha: candidate.tree_sha,
                    final_source: crate::quality::CandidateFinalSource::ModelFinal,
                    salvage: None,
                },
                Utc::now().timestamp_millis(),
            )
            .await
            .is_err()
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn stagnation_salvage_candidate_records_verified_source_and_event() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let candidate = fixture.submit().await.unwrap();
    assert_eq!(
        candidate.final_source,
        crate::quality::CandidateFinalSource::StagnationSalvage
    );
    let events = fixture.db.events().await.unwrap();
    let recorded = events
        .iter()
        .find(|event| event.subject_id == candidate.id && event.kind == "candidate.recorded")
        .expect("candidate.recorded event");
    assert!(recorded.detail.contains("finalSource=STAGNATION_SALVAGE"));
    let salvage = events
        .iter()
        .find(|event| {
            event.subject_id == candidate.id && event.kind == "worker.stagnation_finalized"
        })
        .expect("stagnation salvage event");
    let detail: serde_json::Value = serde_json::from_str(&salvage.detail).unwrap();
    assert_eq!(detail["decision"], "DIAGNOSE");
    assert_eq!(detail["modelTurns"], 1);
    assert_eq!(detail["finalNudges"], 1);
    assert_eq!(detail["frozenChecks"], 2);
    assert_eq!(detail["passedChecks"], 2);
    assert_eq!(detail["attemptId"], fixture.attempt_id);
    assert_eq!(detail["workItemId"], fixture.campaign_id);
    assert_eq!(detail["campaignId"], fixture.campaign_id);
    assert_eq!(detail["policyId"], fixture.policy_id);
    assert_eq!(detail["authorizationId"], fixture.authorization_id);
    assert!(
        detail["diagnoseCheckpointId"]
            .as_str()
            .is_some_and(|value| value.starts_with("diagnose-DIAGNOSE-"))
    );
    assert_eq!(detail["stagnantCheckpoints"], 2);
    assert_eq!(detail["diff"]["changedFiles"], 1);
    assert_eq!(
        detail["diff"]["changedPaths"],
        serde_json::json!(["seed.txt"])
    );
    assert!(
        detail["diff"]["diffStat"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    let receipts = detail["checkReceipts"].as_array().unwrap();
    assert_eq!(receipts.len(), 2);
    let check_ids = receipts
        .iter()
        .map(|receipt| receipt["checkId"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(check_ids, ["build", "test"].into_iter().collect());
    fixture.db.close().await;
}

#[tokio::test]
async fn model_final_candidate_never_emits_stagnation_evidence() {
    let fixture = candidate_fixture().await;
    let candidate = fixture.db.candidate(&fixture.candidate_id).await.unwrap();
    assert_eq!(
        candidate.final_source,
        crate::quality::CandidateFinalSource::ModelFinal
    );
    let events = fixture.db.events().await.unwrap();
    assert!(
        events.iter().all(|event| {
            event.subject_id != fixture.candidate_id || event.kind != "worker.stagnation_finalized"
        }),
        "a normal model final must not emit worker.stagnation_finalized"
    );
    let recorded = events
        .iter()
        .find(|event| {
            event.subject_id == fixture.candidate_id && event.kind == "candidate.recorded"
        })
        .expect("candidate.recorded event");
    assert!(recorded.detail.contains("finalSource=MODEL_FINAL"));
    fixture.db.close().await;
}

#[tokio::test]
async fn r0_salvage_can_reach_an_accepted_gate() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let candidate = fixture.submit().await.unwrap();
    fixture
        .db
        .record_quality_receipt(
            &candidate.id,
            QualityRole::Verifier,
            &quality_receipt_for(
                &fixture.spec_sha256,
                &fixture.commit_sha,
                &fixture.tree_sha,
                "verifier",
                "verifier-1",
            ),
        )
        .await
        .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &candidate.id,
            QualityRole::Reviewer,
            &quality_receipt_for(
                &fixture.spec_sha256,
                &fixture.commit_sha,
                &fixture.tree_sha,
                "reviewer",
                "reviewer-1",
            ),
        )
        .await
        .unwrap();
    let gate = result.gate.expect("gate decision");
    assert_eq!(gate.outcome, GateOutcome::Accepted);
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_without_exact_per_check_receipts_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_a_wrong_check_receipt_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    for suffix in ["build", "test"] {
        fixture
            .insert_raw_receipt(
                suffix,
                "wrong-check",
                &fixture.campaign_id,
                &fixture.attempt_id,
                &fixture.campaign_id,
                &fixture.commit_sha,
                &fixture.tree_sha,
                "PASS",
            )
            .await;
    }
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_an_extra_duplicate_receipt_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    fixture
        .insert_raw_receipt(
            "duplicate-build",
            "build",
            &fixture.campaign_id,
            &fixture.attempt_id,
            &fixture.campaign_id,
            &fixture.commit_sha,
            &fixture.tree_sha,
            "PASS",
        )
        .await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_a_failing_receipt_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture
        .insert_raw_receipt(
            "build-pass",
            "build",
            &fixture.campaign_id,
            &fixture.attempt_id,
            &fixture.campaign_id,
            &fixture.commit_sha,
            &fixture.tree_sha,
            "PASS",
        )
        .await;
    fixture
        .insert_raw_receipt(
            "test-fail",
            "test",
            &fixture.campaign_id,
            &fixture.attempt_id,
            &fixture.campaign_id,
            &fixture.commit_sha,
            &fixture.tree_sha,
            "FAIL",
        )
        .await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_stale_candidate_identity_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let (alternate_commit, alternate_tree) = fixture.alternate_candidate().await;
    let error = fixture
        .db
        .record_candidate(
            &fixture.submission(&alternate_commit, &alternate_tree),
            Utc::now().timestamp_millis(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_stale_attempt_campaign_or_work_item_receipts_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    let (action_id,): (String,) =
        sqlx::query_as::query_as("SELECT action_id FROM campaigns WHERE id=?1")
            .bind(&fixture.campaign_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    let other_run = fixture
        .db
        .create_run(&fixture.campaign_id, "other-run")
        .await
        .unwrap();
    let other_attempt = fixture
        .db
        .create_attempt(&other_run.id, "other-attempt")
        .await
        .unwrap();
    let other_campaign = fixture
        .db
        .create_campaign(&action_id, "other-campaign")
        .await
        .unwrap();
    let cases = [
        (
            "attempt",
            fixture.campaign_id.as_str(),
            other_attempt.id.as_str(),
            fixture.campaign_id.as_str(),
        ),
        (
            "campaign",
            other_campaign.id.as_str(),
            fixture.attempt_id.as_str(),
            fixture.campaign_id.as_str(),
        ),
        (
            "work-item",
            fixture.campaign_id.as_str(),
            fixture.attempt_id.as_str(),
            "other-work-item",
        ),
    ];
    for (suffix, campaign, attempt, work_item) in cases {
        fixture
            .insert_raw_receipt(
                suffix,
                "build",
                campaign,
                attempt,
                work_item,
                &fixture.commit_sha,
                &fixture.tree_sha,
                "PASS",
            )
            .await;
    }
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_without_a_durable_diagnose_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_after_a_later_continue_checkpoint_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_diagnose_checkpoint("CONTINUE", 0, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_zero_frozen_checks_is_rejected() {
    let fixture = SalvageFixture::start_with_checks(Vec::new()).await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    let error = fixture.submit().await.unwrap_err();
    match error {
        Error::Forbidden(message) => assert!(
            message.contains(crate::quality::SALVAGE_EMPTY_FROZEN_CHECKS),
            "{message}"
        ),
        other => panic!("expected typed empty-check rejection, got {other:?}"),
    }
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_a_non_diagnose_progress_decision_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("CONTINUE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_without_a_durable_final_nudge_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_receipts().await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_with_insufficient_diagnose_counts_is_rejected() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 1, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn salvage_rejects_forged_worker_counts_and_an_empty_diff() {
    let value = serde_json::json!({
        "attempt_id": "attempt",
        "worker_id": "worker",
        "generation": 1,
        "action_spec_sha256": "a".repeat(64),
        "commit_sha": "b".repeat(40),
        "tree_sha": "c".repeat(40),
        "final_source": "STAGNATION_SALVAGE",
        "salvage": {
            "workItemId": "campaign",
            "passedChecks": 99,
            "finalNudges": 99,
            "diff": {"changedFiles": 99}
        }
    });
    assert!(serde_json::from_value::<NewCandidate>(value).is_err());

    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let base_tree = git_fixture(&fixture.repo, &["rev-parse", "HEAD^{tree}"]);
    let error = fixture
        .db
        .record_candidate(
            &fixture.submission(&fixture.base_commit, &base_tree),
            Utc::now().timestamp_millis(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Forbidden(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn a_work_item_can_use_only_one_stagnation_salvage() {
    let fixture = SalvageFixture::start().await;
    fixture.record_diagnose_checkpoint("DIAGNOSE", 2, 0).await;
    fixture.record_final_nudge().await;
    fixture.record_receipts().await;
    let other_run = fixture
        .db
        .create_run(&fixture.campaign_id, "prior-salvage-run")
        .await
        .unwrap();
    let other_attempt = fixture
        .db
        .create_attempt(&other_run.id, "prior-salvage-attempt")
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at,
          integration,lineage_json,final_source)
         VALUES ('prior-salvage',?1,?2,?3,?4,'prior-worker','2026-01-01T00:00:00Z',
                 0,'[]','STAGNATION_SALVAGE')",
    )
    .bind(&other_attempt.id)
    .bind(&fixture.spec_sha256)
    .bind(&fixture.commit_sha)
    .bind(&fixture.tree_sha)
    .execute(&fixture.db.pool)
    .await
    .unwrap();
    let error = fixture.submit().await.unwrap_err();
    assert!(matches!(error, Error::Conflict(_)), "{error:?}");
    fixture.db.close().await;
}

#[tokio::test]
async fn candidate_gate_and_receipt_rows_are_immutable() {
    let fixture = candidate_fixture().await;
    assert!(
        sqlx::query::query("UPDATE candidates SET commit_sha='tampered' WHERE id=?1")
            .bind(&fixture.candidate_id)
            .execute(&fixture.db.pool)
            .await
            .is_err()
    );
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier", "verifier-1"),
        )
        .await
        .unwrap();
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer", "reviewer-1"),
        )
        .await
        .unwrap();
    assert!(
        sqlx::query::query("UPDATE gate_decisions SET outcome='ACCEPTED' WHERE candidate_id=?1")
            .bind(&fixture.candidate_id)
            .execute(&fixture.db.pool)
            .await
            .is_err()
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn project_retry_keeps_first_base_after_repository_advances() {
    let root = std::env::temp_dir().join(format!("factory-project-test-{} ", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let git = |arguments: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("content"), "one").unwrap();
    git(&["add", "content"]);
    git(&["commit", "-qm", "first"]);
    std::fs::create_dir(root.join("nested")).unwrap();
    let db = Database::open(&root.join("factory.sqlite")).await.unwrap();
    let first = db.register_project(&root, "same-request").await.unwrap();
    assert_eq!(first.repository, root.to_string_lossy());
    std::fs::write(root.join("content"), "two").unwrap();
    git(&["add", "content"]);
    git(&["commit", "-qm", "second"]);
    let replayed = db
        .register_project(&root.join("nested"), "same-request")
        .await
        .unwrap();
    assert_eq!(first.id, replayed.id);
    assert_eq!(first.base_commit, replayed.base_commit);
    db.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn project_source_identity_is_explicit_and_duplicates_are_rejected() {
    let root = std::env::temp_dir().join(format!("factory-project-source-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let git = |arguments: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("content"), "one").unwrap();
    git(&["add", "content"]);
    git(&["commit", "-qm", "first"]);
    let db = Database::open(&root.join("factory.sqlite")).await.unwrap();
    let source = RegisterProjectSource {
        name: "Widgets".into(),
        source_kind: "local".into(),
        source_url: String::new(),
        source_key: "local:widgets".into(),
        source_visibility: String::new(),
        default_branch: "main".into(),
    };
    let project = db
        .register_project_with_source(&root, &source, "source-key")
        .await
        .unwrap();
    assert_eq!(project.name, "Widgets");
    assert_eq!(project.source_kind, "local");
    assert_eq!(project.source_key, "local:widgets");
    assert_eq!(project.default_branch, "main");
    let replay = db
        .register_project_with_source(&root, &source, "source-key")
        .await
        .unwrap();
    assert_eq!(project.id, replay.id);
    assert!(matches!(
        db.register_project_with_source(&root, &source, "another-key")
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        db.project_by_source_key("local:widgets")
            .await
            .unwrap()
            .map(|project| project.id),
        Some(project.id.clone())
    );
    assert!(
        db.project_by_source_key("local:missing")
            .await
            .unwrap()
            .is_none()
    );
    db.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn project_discovery_is_persisted_against_its_scanned_commit() {
    let (path, db) = temp_db("factory-project-discovery").await;
    insert_project(&db, "project", "/fixture", &"a".repeat(40)).await;
    let base_commit = "a".repeat(40);
    let discovery = crate::project_import::ProjectDiscovery {
        base_commit: base_commit.clone(),
        toolchains: Vec::new(),
        suggested_capabilities: vec![crate::lead::Capability::RepositoryEdit],
        suggested_checks: Vec::new(),
        scanned_manifests: vec!["Cargo.toml".into()],
        truncated: false,
        skipped_entries: 0,
        provenance: "test".into(),
        scanned_at: chrono::Utc::now().to_rfc3339(),
    };
    let saved = db
        .save_project_discovery("project", &discovery)
        .await
        .unwrap();
    assert_eq!(saved.base_commit, base_commit);
    let loaded = db.project_discovery("project").await.unwrap().unwrap();
    assert_eq!(
        loaded.discovery.suggested_capabilities,
        discovery.suggested_capabilities
    );
    assert_eq!(loaded.discovery.scanned_manifests, vec!["Cargo.toml"]);
    assert!(db.project_discovery("missing").await.is_err());
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[test]
fn p9_database_is_send_sync_and_not_connection_mutexed() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Database>();
}

#[tokio::test]
async fn authorization_rejects_unratified_risk_and_execution_modes() {
    let (path, db) = temp_db("factory-auth-modes").await;
    insert_project(&db, "project", "/fixture", "base").await;
    let action = db
        .create_action_with_spec(
            "project",
            "objective",
            "codex",
            "model",
            "action-key",
            ActionSpecInput {
                allowed_paths: &["src".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let campaign = db
        .create_campaign(&action.id, "campaign-key")
        .await
        .unwrap();
    for (risk, mode) in [("R1", "FIXTURE"), ("R0", "ISOLATED")] {
        let result = db
            .authorize_campaign(
                &campaign.id,
                &NewCampaignAuthorization {
                    authorization_id: format!("auth-{risk}-{mode}"),
                    risk_class: risk.into(),
                    execution_mode: mode.into(),
                    actor: "test".into(),
                    rationale: "negative authorization".into(),
                    authorized_paths: vec!["src".into()],
                    provider: String::new(),
                    model: String::new(),
                    provider_mode: String::new(),
                    delegation_id: String::new(),
                    allowed_tools: Vec::new(),
                    monetary_envelope_id: crate::db::authorization::NO_BILLABLE_ENVELOPE.into(),
                    progress: fixture_progress(),
                    operations: fixture_operations(),
                },
            )
            .await;
        assert!(matches!(result, Err(Error::Forbidden(_))));
    }
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
#[tokio::test]
async fn candidate_outside_scope_and_symlink_are_rejected() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!("factory-scope-{}", Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git_fixture(&repo, &["init", "-q"]);
    git_fixture(&repo, &["config", "user.name", "Scope Test"]);
    git_fixture(&repo, &["config", "user.email", "scope@example.invalid"]);
    std::fs::write(repo.join("seed.txt"), "seed\n").unwrap();
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/keep.txt"), "keep\n").unwrap();
    git_fixture(&repo, &["add", "."]);
    git_fixture(&repo, &["commit", "-qm", "base"]);
    let db = Database::open(&root.join("factory.sqlite")).await.unwrap();
    let project = db.register_project(&repo, "project-key").await.unwrap();
    let action = db
        .create_action_with_spec(
            &project.id,
            "scope test",
            "opencode",
            "test/model",
            "action-key",
            ActionSpecInput {
                allowed_paths: &["src".to_string()],
                checks: &[],
                dependency_plan: None,
            },
        )
        .await
        .unwrap();
    let campaign = db
        .create_campaign(&action.id, "campaign-key")
        .await
        .unwrap();
    db.authorize_campaign(
        &campaign.id,
        &NewCampaignAuthorization {
            authorization_id: "auth-scope".into(),
            risk_class: "R0".into(),
            execution_mode: "FIXTURE".into(),
            actor: "test".into(),
            rationale: "scope negative".into(),
            authorized_paths: vec!["src".into()],
            provider: String::new(),
            model: String::new(),
            provider_mode: String::new(),
            delegation_id: String::new(),
            allowed_tools: Vec::new(),
            monetary_envelope_id: crate::db::authorization::NO_BILLABLE_ENVELOPE.into(),
            progress: fixture_progress(),
            operations: fixture_operations(),
        },
    )
    .await
    .unwrap();
    let run = db.create_run(&campaign.id, "run-key").await.unwrap();
    let attempt = db.create_attempt(&run.id, "attempt-key").await.unwrap();
    let now = Utc::now().timestamp_millis();
    let lease = db
        .claim_attempt("worker-scope", now, 60_000)
        .await
        .unwrap()
        .unwrap();
    let allocator =
        crate::workspace::WorkspaceAllocator::new(&repo, root.join("attempts")).unwrap();
    let mut workspace = allocator
        .allocate(&attempt.id, &project.base_commit)
        .unwrap();
    std::fs::write(workspace.path().join("outside.txt"), "outside\n").unwrap();
    git_fixture(workspace.path(), &["add", "outside.txt"]);
    git_fixture(workspace.path(), &["commit", "-qm", "outside"]);
    let outside = workspace.candidate_revision().unwrap();
    let result = db
        .record_candidate(
            &NewCandidate {
                attempt_id: attempt.id.clone(),
                worker_id: "worker-scope".into(),
                generation: lease.generation,
                action_spec_sha256: action.spec_sha256.clone(),
                commit_sha: outside.commit_sha,
                tree_sha: outside.tree_sha,
                final_source: crate::quality::CandidateFinalSource::ModelFinal,
                salvage: None,
            },
            now,
        )
        .await;
    assert!(matches!(result, Err(Error::Forbidden(_))));
    workspace.cleanup().unwrap();

    let second_run = db.create_run(&campaign.id, "run-key-2").await.unwrap();
    let second = db
        .create_attempt(&second_run.id, "attempt-key-2")
        .await
        .unwrap();
    let lease = db
        .claim_attempt("worker-scope", now + 1, 60_000)
        .await
        .unwrap()
        .unwrap();
    let mut workspace = allocator
        .allocate(&second.id, &project.base_commit)
        .unwrap();
    symlink("../seed.txt", workspace.path().join("src/link.txt")).unwrap();
    git_fixture(workspace.path(), &["add", "src/link.txt"]);
    git_fixture(workspace.path(), &["commit", "-qm", "symlink"]);
    let linked = workspace.candidate_revision().unwrap();
    let result = db
        .record_candidate(
            &NewCandidate {
                attempt_id: second.id.clone(),
                worker_id: "worker-scope".into(),
                generation: lease.generation,
                action_spec_sha256: action.spec_sha256.clone(),
                commit_sha: linked.commit_sha,
                tree_sha: linked.tree_sha,
                final_source: crate::quality::CandidateFinalSource::ModelFinal,
                salvage: None,
            },
            now + 1,
        )
        .await;
    assert!(matches!(result, Err(Error::Forbidden(_))));
    workspace.cleanup().unwrap();
    db.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn event_kind_vocabulary_is_closed() {
    let (path, db) = temp_db("factory-event-kind").await;
    let mut transaction = db.pool.begin().await.unwrap();
    super::record_event(&mut transaction, "subject", "action.created", "created")
        .await
        .unwrap();
    assert!(matches!(
        super::record_event(&mut transaction, "subject", "unknown.event", "x").await,
        Err(Error::Invalid("unknown event kind"))
    ));
    transaction.rollback().await.unwrap();
    db.close().await;
    let _ = std::fs::remove_file(path);
}

fn model_call_input(
    digest: &str,
    scope: &str,
    estimated_input: u64,
    requested_output: u64,
    estimated_cost: u64,
    policy: ModelCallPolicy,
) -> NewModelCallAuthorization {
    NewModelCallAuthorization {
        role: ModelCallRole::Evaluation,
        principal_id: "evaluator".into(),
        project_id: "evaluation".into(),
        objective_id: scope.into(),
        scope_id: scope.into(),
        provider: "opencode-go".into(),
        model: "deepseek-v4.1-flash".into(),
        purpose: "evaluation".into(),
        request_sha256: digest.into(),
        estimated_input_tokens: estimated_input,
        requested_output_tokens: requested_output,
        estimated_cost_micros: estimated_cost,
        policy,
        global_policy: None,
    }
}

fn model_call_policy(input: u64, output: u64, cost: u64) -> ModelCallPolicy {
    ModelCallPolicy {
        max_input_tokens: input,
        max_output_tokens: output,
        max_cost_micros: cost,
    }
}

#[tokio::test]
async fn test_subscription_dispatch_admission_is_scoped_to_opted_in_scopes() {
    let (path, db) = temp_db("factory-model-call-admission-scope").await;
    db.admit_test_subscription_dispatch("admitted-scope")
        .await
        .unwrap();
    let policy = model_call_policy(1_000, 500, 10_000);
    let admitted = model_call_input(&"3".repeat(64), "admitted-scope", 100, 50, 100, policy);
    assert!(matches!(
        db.authorize_model_call(&admitted).await.unwrap(),
        ModelCallDecision::Reserved(_)
    ));
    let other = model_call_input(&"4".repeat(64), "unadmitted-scope", 100, 50, 100, policy);
    assert!(
        matches!(
            db.authorize_model_call(&other).await,
            Err(Error::SubscriptionFunding(
                crate::execution_policy::SubscriptionFundingRefusal::BindingRequired
            ))
        ),
        "opting one scope in must not admit an unrelated scope"
    );
    db.close().await;

    // A separate database that never opts in stays fully fail-closed.
    let (second_path, second) = temp_db("factory-model-call-admission-absent").await;
    let refused = model_call_input(&"5".repeat(64), "admitted-scope", 100, 50, 100, policy);
    assert!(matches!(
        second.authorize_model_call(&refused).await,
        Err(Error::SubscriptionFunding(
            crate::execution_policy::SubscriptionFundingRefusal::BindingRequired
        ))
    ));
    second.close().await;
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(second_path);
}

#[tokio::test]
async fn model_call_reservations_settle_replay_and_fence_duplicates() {
    let (path, db) = temp_db("factory-model-calls").await;
    db.admit_test_subscription_dispatch("evaluation-1")
        .await
        .unwrap();
    let policy = model_call_policy(1_000, 500, 10_000);
    let input = model_call_input(&"a".repeat(64), "evaluation-1", 120, 300, 200, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&input).await.unwrap() else {
        panic!("expected a reservation");
    };
    assert_eq!(grant.max_output_tokens, 300);
    assert!(matches!(
        db.authorize_model_call(&input).await,
        Err(Error::Conflict("model call is already in flight"))
    ));
    let usage = ModelCallUsage {
        reservation_id: grant.reservation_id.clone(),
        input_tokens: 100,
        output_tokens: 200,
        total_tokens: 300,
        cost_micros: 150,
        usage_known: true,
        content: "{\"ok\":true}".into(),
    };
    db.settle_model_call(&usage).await.unwrap();
    let replay = db.authorize_model_call(&input).await.unwrap();
    let ModelCallDecision::Replayed(replay) = replay else {
        panic!("expected a settled replay");
    };
    assert_eq!(replay.content, "{\"ok\":true}");
    assert_eq!(replay.total_tokens, 300);
    assert!(matches!(
        db.settle_model_call(&ModelCallUsage {
            reservation_id: grant.reservation_id.clone(),
            input_tokens: 101,
            output_tokens: 200,
            total_tokens: 301,
            cost_micros: 150,
            usage_known: true,
            content: "different".into(),
        })
        .await,
        Err(Error::Conflict("model call already settled differently"))
    ));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn model_call_budget_unknown_usage_and_ambiguity_fail_closed() {
    let (path, db) = temp_db("factory-model-call-safety").await;
    for scope in ["evaluation-2", "evaluation-3", "evaluation-4"] {
        db.admit_test_subscription_dispatch(scope).await.unwrap();
    }
    let policy = model_call_policy(1_000, 500, 10_000);
    let first = model_call_input(&"b".repeat(64), "evaluation-2", 120, 300, 200, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&first).await.unwrap() else {
        panic!("expected a reservation");
    };
    let first_bound = grant.max_cost_micros;
    assert!(matches!(
        db.settle_model_call(&ModelCallUsage {
            reservation_id: grant.reservation_id.clone(),
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_micros: 0,
            usage_known: false,
            content: "unmeasured".into(),
        })
        .await,
        Err(Error::Forbidden(_))
    ));
    assert!(matches!(
        db.authorize_model_call(&first).await,
        Err(Error::Forbidden(_))
    ));

    let second = model_call_input(&"c".repeat(64), "evaluation-3", 900, 400, 1_000, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&second).await.unwrap() else {
        panic!("expected a reservation");
    };
    db.release_model_call(&grant.reservation_id, "test release", false)
        .await
        .unwrap();
    let ModelCallDecision::Reserved(_) = db.authorize_model_call(&second).await.unwrap() else {
        panic!("released call should be retryable");
    };

    let third = model_call_input(&"d".repeat(64), "evaluation-4", 600, 100, 1_000, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&third).await.unwrap() else {
        panic!("expected a reservation");
    };
    sqlx::query::query(
        "UPDATE model_call_reservations SET expires_at_ms=0 WHERE reservation_id=?1",
    )
    .bind(&grant.reservation_id)
    .execute(&db.pool)
    .await
    .unwrap();
    let swept = db.unresolved_model_calls().await.unwrap();
    assert_eq!(
        swept
            .iter()
            .find(|record| record.reservation_id == grant.reservation_id)
            .map(|record| record.state.as_str()),
        Some("AMBIGUOUS")
    );
    assert!(matches!(
        db.authorize_model_call(&third).await,
        Err(Error::Forbidden(_))
    ));
    let unresolved = db.unresolved_model_calls().await.unwrap();
    assert_eq!(unresolved.len(), 2);
    assert!(
        unresolved
            .iter()
            .all(|record| matches!(record.state.as_str(), "AMBIGUOUS" | "UNKNOWN"))
    );
    let (unmeasured, bound) = db.unresolved_model_call_budget().await.unwrap();
    assert_eq!(unmeasured, 2);
    assert_eq!(bound, first_bound + grant.max_cost_micros);
    db.resolve_ambiguous_model_call(&grant.reservation_id, "operator", "reconciled")
        .await
        .unwrap();
    let ModelCallDecision::Reserved(_) = db.authorize_model_call(&third).await.unwrap() else {
        panic!("resolved call should be retryable");
    };
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn ambiguous_model_call_release_requires_reason_and_records_audit_after_operator_resolution()
{
    let (path, db) = temp_db("factory-model-call-resolution").await;
    let scope = "evaluation-release";
    db.admit_test_subscription_dispatch(scope).await.unwrap();
    let policy = model_call_policy(1_000, 500, 10_000);
    let input = model_call_input(&"f".repeat(64), scope, 120, 300, 200, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&input).await.unwrap() else {
        panic!("expected a reservation");
    };
    sqlx::query::query(
        "UPDATE model_call_reservations SET expires_at_ms=0 WHERE reservation_id=?1",
    )
    .bind(&grant.reservation_id)
    .execute(&db.pool)
    .await
    .unwrap();
    let unresolved = db.unresolved_model_calls().await.unwrap();
    assert_eq!(
        unresolved
            .iter()
            .find(|record| record.reservation_id == grant.reservation_id)
            .map(|record| record.state.as_str()),
        Some("AMBIGUOUS")
    );
    let (count, bound) = db.unresolved_model_call_budget().await.unwrap();
    assert_eq!(count, 1);
    assert_eq!(bound, grant.max_cost_micros);

    for (actor, reason) in [("", "operator reason"), ("operator", "  ")] {
        assert!(matches!(
            db.resolve_ambiguous_model_call(&grant.reservation_id, actor, reason)
                .await,
            Err(Error::Invalid("model call resolution identity"))
        ));
        let unresolved = db.unresolved_model_calls().await.unwrap();
        assert_eq!(
            unresolved
                .iter()
                .find(|record| record.reservation_id == grant.reservation_id)
                .map(|record| record.state.as_str()),
            Some("AMBIGUOUS"),
            "an invalid resolution must not release the ambiguous reservation"
        );
        assert_eq!(db.unresolved_model_call_budget().await.unwrap(), (1, bound));
        assert!(
            !db.events()
                .await
                .unwrap()
                .iter()
                .any(|event| event.kind == "model.call_resolved"),
            "an invalid resolution must not emit a resolution audit event"
        );
    }

    db.resolve_ambiguous_model_call(
        &grant.reservation_id,
        "owner",
        "provider aborted before usage was reported",
    )
    .await
    .unwrap();
    let records = db.model_calls_for_scope(scope).await.unwrap();
    let record = records
        .iter()
        .find(|record| record.reservation_id == grant.reservation_id)
        .expect("resolved model call");
    assert_eq!(record.state, "RELEASED");
    assert!(record.error.contains("owner"));
    assert!(record.error.contains("provider aborted"));
    assert_eq!(db.unresolved_model_call_budget().await.unwrap(), (0, 0));
    let events = db.events().await.unwrap();
    let audit = events
        .iter()
        .find(|event| {
            event.kind == "model.call_resolved"
                && event.subject_id == scope
                && event.detail.contains(&grant.reservation_id)
        })
        .expect("model call resolution audit event");
    assert!(audit.detail.contains("actor=owner"));
    assert!(
        audit
            .detail
            .contains("provider aborted before usage was reported")
    );
    assert!(matches!(
        db.resolve_ambiguous_model_call(&grant.reservation_id, "owner", "again")
            .await,
        Err(Error::Conflict("model call is not ambiguous"))
    ));
    let ModelCallDecision::Reserved(_) = db.authorize_model_call(&input).await.unwrap() else {
        panic!("an explicitly released call should be retryable");
    };
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn model_call_scope_and_global_budgets_are_enforced() {
    let (path, db) = temp_db("factory-model-call-budget").await;
    for scope in [
        "evaluation-budget",
        "evaluation-global-a",
        "evaluation-global-b",
    ] {
        db.admit_test_subscription_dispatch(scope).await.unwrap();
    }
    let policy = model_call_policy(100, 50, 1_000);
    let first = model_call_input(&"e".repeat(64), "evaluation-budget", 40, 10, 20, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&first).await.unwrap() else {
        panic!("expected a reservation");
    };
    assert_eq!(
        grant.max_input_tokens, 100,
        "a lower policy budget must not be raised to the technical ceiling"
    );
    assert_eq!(grant.max_cost_micros, 1_000);
    db.settle_model_call(&ModelCallUsage {
        reservation_id: grant.reservation_id,
        input_tokens: 40,
        output_tokens: 10,
        total_tokens: 50,
        cost_micros: 20,
        usage_known: true,
        content: "{\"ok\":true}".into(),
    })
    .await
    .unwrap();
    let second = model_call_input(&"f".repeat(64), "evaluation-budget", 70, 10, 20, policy);
    assert!(matches!(
        db.authorize_model_call(&second).await,
        Err(Error::Forbidden(_))
    ));

    let mut global_first = model_call_input(
        &"1".repeat(64),
        "evaluation-global-a",
        80,
        10,
        20,
        model_call_policy(1_000, 100, 1_000),
    );
    global_first.global_policy = Some(model_call_policy(150, 100, 1_000));
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&global_first).await.unwrap()
    else {
        panic!("expected a reservation");
    };
    db.settle_model_call(&ModelCallUsage {
        reservation_id: grant.reservation_id,
        input_tokens: 80,
        output_tokens: 10,
        total_tokens: 90,
        cost_micros: 20,
        usage_known: true,
        content: "{\"ok\":true}".into(),
    })
    .await
    .unwrap();
    let mut global_second = model_call_input(
        &"2".repeat(64),
        "evaluation-global-b",
        80,
        10,
        20,
        model_call_policy(1_000, 100, 1_000),
    );
    global_second.global_policy = Some(model_call_policy(150, 100, 1_000));
    assert!(matches!(
        db.authorize_model_call(&global_second).await,
        Err(Error::Forbidden(_))
    ));
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn model_call_policy_near_i64_max_is_clamped_to_the_technical_ceiling() {
    let (path, db) = temp_db("factory-model-call-ceiling").await;
    let scope = "evaluation-ceiling";
    db.admit_test_subscription_dispatch(scope).await.unwrap();
    let input = model_call_input(
        &"7".repeat(64),
        scope,
        100,
        0,
        500,
        model_call_policy(u64::MAX, u64::MAX, u64::MAX),
    );
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&input).await.unwrap() else {
        panic!("expected a reservation");
    };
    assert_eq!(
        grant.max_input_tokens,
        crate::execution_policy::SAFE_MAX_MODEL_CALL_INPUT_TOKENS
    );
    assert_eq!(
        grant.max_output_tokens,
        crate::execution_policy::SAFE_MAX_MODEL_CALL_OUTPUT_TOKENS
    );
    assert_eq!(
        grant.max_total_tokens,
        crate::execution_policy::SAFE_MAX_MODEL_CALL_TOTAL_TOKENS
    );
    assert_eq!(
        grant.max_cost_micros,
        crate::execution_policy::SAFE_MAX_MODEL_CALL_COST_MICROS
    );
    let events = db.events().await.unwrap();
    assert!(
        events.iter().any(|event| {
            event.kind == "model.call_authorized" && event.detail.contains("capped=true")
        }),
        "a clamped policy must leave bounded evidence in the event ledger"
    );
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn many_model_call_reservations_and_legacy_rows_do_not_overflow_accounting() {
    let (path, db) = temp_db("factory-model-call-overflow").await;
    insert_project(&db, "evaluation", "/fixture", "base").await;
    let now = Utc::now().to_rfc3339();
    let now_ms = Utc::now().timestamp_millis();
    // A subscription-bound Campaign has no cumulative monetary envelope, so
    // eight reservations can legitimately be in flight at once. That is the
    // shape that used to overflow SUM(max_cost_micros).
    sqlx::query::query(
        "INSERT INTO actions
         (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
         VALUES ('action-overflow','evaluation','objective','opencode-go',
                 'deepseek-v4.1-flash','R0','AUTHORIZED','hash',?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO campaigns(id,action_id,status,created_at)
         VALUES ('campaign-overflow','action-overflow','AUTHORIZED',?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO runs VALUES ('run-overflow','campaign-overflow','QUEUED',?1)")
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO attempts(id,run_id,status,created_at)
         VALUES ('attempt-overflow','run-overflow','ACCEPTED',?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
         VALUES ('candidate-overflow','attempt-overflow','hash','commit','tree','worker',?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO inference_campaign_modes
         (campaign_id,mode,classified_at_ms,evidence_sha256)
         VALUES ('campaign-overflow','SUBSCRIPTION',?1,?2)",
    )
    .bind(now_ms)
    .bind("e".repeat(64))
    .execute(&db.pool)
    .await
    .unwrap();
    db.admit_test_subscription_dispatch("campaign-overflow")
        .await
        .unwrap();
    let scope = "candidate-overflow";
    let policy = model_call_policy(u64::MAX, u64::MAX, u64::MAX);
    for index in 0..8u64 {
        let mut input =
            model_call_input(&format!("{:064x}", index + 1), scope, 100, 50, 500, policy);
        input.global_policy = Some(policy);
        let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&input).await.unwrap()
        else {
            panic!("expected reservation {index}");
        };
        assert_eq!(
            grant.max_cost_micros,
            crate::execution_policy::SAFE_MAX_MODEL_CALL_COST_MICROS
        );
    }
    let records = db.model_calls_for_scope(scope).await.unwrap();
    assert_eq!(records.len(), 8);

    // The semantic reviewer uses the same reservation path and ceiling as every
    // other role.
    let mut reviewer = model_call_input(&format!("{:064x}", 900), scope, 100, 50, 500, policy);
    reviewer.role = ModelCallRole::SemanticReviewer;
    reviewer.principal_id = "reviewer".into();
    reviewer.purpose = "semantic_review".into();
    reviewer.global_policy = Some(policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&reviewer).await.unwrap()
    else {
        panic!("expected a reviewer reservation");
    };
    assert_eq!(
        grant.max_cost_micros,
        crate::execution_policy::SAFE_MAX_MODEL_CALL_COST_MICROS
    );
    assert_eq!(db.model_calls_for_scope(scope).await.unwrap().len(), 9);

    // A pre-repair ledger may already contain reservations near i64::MAX / 4.
    // Their sum exceeds SQLite's signed INTEGER range, but the projection must
    // stay finite and fail closed instead of raising a database error.
    let legacy_cost = i64::MAX as u64 / 4;
    for index in 0..5u64 {
        sqlx::query::query(
            "INSERT INTO model_call_reservations
             (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
              purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
              max_cost_micros,state,expires_at_ms,created_at,updated_at)
             VALUES (?1,'EVALUATION','evaluator','evaluation',?2,?2,'opencode-go',
                     'deepseek-v4.1-flash','evaluation',?3,100,100,200,?4,'AMBIGUOUS',?5,?6,?6)",
        )
        .bind(format!("legacy-{index}"))
        .bind(scope)
        .bind(format!("{:064x}", 100 + index))
        .bind(i64::try_from(legacy_cost).unwrap())
        .bind(i64::MAX - 1)
        .bind(&now)
        .execute(&db.pool)
        .await
        .unwrap();
    }
    let (count, bound) = db.unresolved_model_call_budget().await.unwrap();
    assert_eq!(count, 5);
    assert_eq!(bound, legacy_cost * 5);
    let latest = model_call_input(&format!("{:064x}", 999), scope, 100, 50, 500, policy);
    assert!(
        matches!(
            db.authorize_model_call(&latest).await,
            Ok(ModelCallDecision::Reserved(_))
        ),
        "legacy large reservations must not break a subscription authorization"
    );
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn test_subscription_admission_supports_exact_and_e2e_wildcard_scopes() {
    let path = std::env::temp_dir().join(format!(
        "factory-test-admissions-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let db = Database::open(&path).await.unwrap();
    let mut connection = db.pool.acquire().await.unwrap();
    assert!(
        !test_subscription_dispatch_admitted_in_tx(&mut connection, "campaign-exact")
            .await
            .unwrap()
    );
    db.admit_test_subscription_dispatch("campaign-exact")
        .await
        .unwrap();
    assert!(
        test_subscription_dispatch_admitted_in_tx(&mut connection, "campaign-exact")
            .await
            .unwrap()
    );
    assert!(
        !test_subscription_dispatch_admitted_in_tx(&mut connection, "campaign-other")
            .await
            .unwrap()
    );
    db.admit_test_subscription_dispatch("*").await.unwrap();
    sqlx::query::query("UPDATE test_subscription_fixture_mode SET enabled=0")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        !test_subscription_dispatch_admitted_in_tx(&mut connection, "campaign-other")
            .await
            .unwrap()
    );
    sqlx::query::query("UPDATE test_subscription_fixture_mode SET enabled=1")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        test_subscription_dispatch_admitted_in_tx(&mut connection, "campaign-other")
            .await
            .unwrap()
    );
    drop(connection);
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn owner_confirmed_campaign_admission_enables_subscription_mode() {
    use crate::execution_policy::{OwnerBillingAssumption, OwnerControlledExtraUsage};
    use crate::subscription_admission::{TestModelRole, TestRouteBinding, WAVE3_TEST_PROFILE_ID};

    let path = std::env::temp_dir().join(format!(
        "factory-subscription-admission-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let db = Database::open(&path).await.unwrap();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO projects(id,repository,base_commit,created_at,identity)
         VALUES ('project-admission','/fixture',?1,?2,'')",
    )
    .bind("a".repeat(40))
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO actions(id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
         VALUES ('action-admission','project-admission','objective','opencode-go','deepseek-v4.1-flash','R0','PENDING','hash',?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO campaigns(id,action_id,status,created_at)
         VALUES ('campaign-admission','action-admission','BLOCKED_AUTHORIZATION',?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    let assumption = OwnerBillingAssumption {
        assumption_id: "owner-extra-usage".into(),
        confirmed_by: "Product Owner".into(),
        confirmed_at_ms: Utc::now().timestamp_millis() - 1_000,
        valid_until_ms: Utc::now().timestamp_millis() + 3_600_000,
        extra_usage: OwnerControlledExtraUsage::Disabled,
        statement: "Owner confirms OpenCode Go Extra Usage is disabled in the Console".into(),
    };
    let admission = ScopeAdmissionRecord {
        admission_id: "admission-1".into(),
        intake_request_id: "intake-1".into(),
        lead_request_id: "lead-1".into(),
        test_profile_id: WAVE3_TEST_PROFILE_ID.into(),
        execution_profile_id: "profile-admission".into(),
        provider: "opencode-go".into(),
        model: "deepseek-v4.1-flash".into(),
        settings_revision: "1".into(),
        profile_revision_id: "revision-1".into(),
        profile_hash: "b".repeat(64),
        assumption,
        routes: vec![TestRouteBinding {
            role: TestModelRole::Implementer,
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            settings_revision: "1".into(),
            transport: "OPENCODE_GO_SUBSCRIPTION".into(),
        }],
        state: "ACTIVE".into(),
        campaign_id: None,
        created_at_ms: Utc::now().timestamp_millis(),
        expires_at_ms: Utc::now().timestamp_millis() + 3_600_000,
        receipt_sha256: "c".repeat(64),
    };
    sqlx::query::query(
        "INSERT INTO provider_settings
         (provider,kind,model,base_url,auth_mode,credential_ciphertext,credential_nonce,
          credential_fingerprint,enabled,is_default,max_turns,max_input_tokens,
          max_output_tokens,max_cost_micros,input_micros_per_million,
          output_micros_per_million,revision,updated_at)
         VALUES ('opencode-go','opencode','deepseek-v4.1-flash','https://example.invalid/v1',
                 'api_key','ciphertext','nonce','fingerprint',1,0,4,100000,12000,500000,
                 1000,2000,1,?1)",
    )
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    let mut connection = db.pool.acquire().await.unwrap();
    super::subscription_admissions::insert_enabled_campaign_admission_in_tx(
        &mut connection,
        "campaign-admission",
        "auth-admission",
        &admission,
        "opencode-go",
        "deepseek-v4.1-flash",
        "1",
        &"d".repeat(64),
    )
    .await
    .unwrap();
    let (state, mode): (String, String) = sqlx::query_as::query_as(
        "SELECT sfp.state,icm.mode
         FROM subscription_funding_policies sfp
         JOIN inference_campaign_modes icm ON icm.campaign_id=sfp.campaign_id
         WHERE sfp.campaign_id='campaign-admission'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(state, "ENABLED");
    assert_eq!(mode, "SUBSCRIPTION");
    let observed_at_ms: i64 = sqlx::query_scalar::query_scalar(
        "SELECT observed_at_ms FROM subscription_funding_policies
         WHERE campaign_id='campaign-admission'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    let now_ms = observed_at_ms + 1;
    db.pause_subscription_funding_for_quota(
        "campaign-admission",
        "auth-admission",
        now_ms,
        None,
        "provider quota exhausted",
    )
    .await
    .unwrap();
    let paused: String = sqlx::query_scalar::query_scalar(
        "SELECT state FROM subscription_funding_policies WHERE campaign_id='campaign-admission'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(paused, "PAUSED_QUOTA");
    drop(connection);
    db.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn open_reviewer_findings_block_the_gate() {
    use crate::review::{FindingCategory, FindingSeverity, ReviewFinding};

    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-open-finding", "verifier-1"),
        )
        .await
        .unwrap();
    let mut review = quality_receipt(&fixture, "reviewer-open-finding", "reviewer-1");
    review.findings = vec![ReviewFinding {
        id: "finding-open".into(),
        severity: FindingSeverity::Warning,
        category: FindingCategory::Functional,
        message: "the exported file is not closed deterministically".into(),
        points: FindingSeverity::Warning.canonical_points(),
        rationale: "the candidate diff shows a missing close path".into(),
        evidence_refs: vec!["src/export.rs:42".into()],
    }];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::RepairRequired);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("remain open"))
    );
    fixture.db.close().await;
}

#[tokio::test]
async fn informational_only_review_findings_accept_with_observations() {
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-info-only", "verifier-1"),
        )
        .await
        .unwrap();
    let mut review = quality_receipt(&fixture, "reviewer-info-only", "reviewer-1");
    review.findings = vec![
        review_finding("info-1", FindingSeverity::Info),
        review_finding("info-2", FindingSeverity::Info),
    ];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::Accepted);
    assert!(gate.reasons.iter().any(|reason| {
        reason.contains("2 informational reviewer finding(s) recorded as observations")
    }));

    // Informational findings stay visible in the receipt evidence.
    let receipts = fixture
        .db
        .quality_receipts(&fixture.candidate_id)
        .await
        .unwrap();
    let reviewer = receipts
        .iter()
        .find(|receipt| receipt.role == QualityRole::Reviewer)
        .unwrap();
    assert_eq!(reviewer.findings.len(), 2);
    assert!(
        reviewer
            .findings
            .iter()
            .all(|finding| finding.severity == FindingSeverity::Info)
    );

    // An info-only passing review cannot produce or deny a repair cycle.
    assert!(
        fixture
            .db
            .repair_authorization_for_candidate(&fixture.candidate_id)
            .await
            .unwrap()
            .is_none()
    );
    let (repairs,): (i64,) =
        sqlx::query_as::query_as("SELECT COUNT(*) FROM autonomy_repairs WHERE candidate_id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert_eq!(repairs, 0);
    fixture.db.close().await;
}

#[tokio::test]
async fn five_passing_reviewers_with_informational_findings_accept() {
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    set_reviewer_requirement(&fixture.db, &fixture.candidate_id, 5, &[], &[]).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-five-info", "verifier-1"),
        )
        .await
        .unwrap();
    for index in 1..=5 {
        let mut review = quality_receipt(
            &fixture,
            &format!("reviewer-five-info-{index}"),
            &format!("reviewer-{index}"),
        );
        review.findings = vec![review_finding(
            &format!("info-{index}"),
            FindingSeverity::Info,
        )];
        let result = fixture
            .db
            .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
            .await
            .unwrap();
        if index < 5 {
            assert!(result.gate.is_none(), "the Gate waits for all reviewers");
        } else {
            let gate = result.gate.expect("the fifth reviewer decides the Gate");
            assert_eq!(gate.outcome, GateOutcome::Accepted);
            assert!(gate.reasons.iter().any(|reason| {
                reason.contains("5 informational reviewer finding(s) recorded as observations")
            }));
        }
    }
    assert!(
        fixture
            .db
            .repair_authorization_for_candidate(&fixture.candidate_id)
            .await
            .unwrap()
            .is_none()
    );
    let (repairs,): (i64,) =
        sqlx::query_as::query_as("SELECT COUNT(*) FROM autonomy_repairs WHERE candidate_id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert_eq!(repairs, 0);
    fixture.db.close().await;
}

#[tokio::test]
async fn warning_review_finding_still_requires_a_disposition() {
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-warning-disposition", "verifier-1"),
        )
        .await
        .unwrap();
    let mut review = quality_receipt(&fixture, "reviewer-warning-disposition", "reviewer-1");
    review.findings = vec![review_finding("warning-1", FindingSeverity::Warning)];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::RepairRequired);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("1 reviewer finding(s) remain open"))
    );
    let repair = fixture
        .db
        .repair_authorization_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("warning finding creates a repair decision");
    assert_eq!(repair.decision, "DENIED");
    assert_eq!(repair.reason_code, "contradictory_review");
    assert_eq!(
        repair.total_points,
        FindingSeverity::Warning.canonical_points()
    );
    assert_eq!(repair.warning_count, 1);
    assert_eq!(repair.info_count, 0);
    fixture.db.close().await;
}

#[tokio::test]
async fn informational_finding_on_a_non_pass_receipt_remains_actionable() {
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-non-pass-info", "verifier-1"),
        )
        .await
        .unwrap();
    let mut review = quality_receipt(&fixture, "reviewer-non-pass-info", "reviewer-1");
    review.result = ReceiptResult::ChangesRequested;
    review.findings = vec![review_finding("info-change", FindingSeverity::Info)];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::RepairRequired);
    let repair = fixture
        .db
        .repair_authorization_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("non-PASS Info remains a scored repair input");
    assert_eq!(repair.decision, "DENIED");
    assert_eq!(repair.reason_code, "autonomy_disabled");
    assert_eq!(
        repair.total_points,
        FindingSeverity::Info.canonical_points()
    );
    assert_eq!(repair.warning_count, 0);
    assert_eq!(repair.info_count, 1);
    fixture.db.close().await;
}

#[tokio::test]
async fn blocking_review_finding_under_a_pass_receipt_still_requires_repair() {
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-blocking-pass", "verifier-1"),
        )
        .await
        .unwrap();
    // Simulate a historical/malformed PASS row that bypassed ingestion
    // validation. A later receipt recomputes the Gate over every durable row.
    insert_reviewer_receipt_with_findings_direct(
        &fixture,
        "reviewer-blocking-pass",
        "reviewer-historical",
        "PASS",
        &[review_finding("blocking-1", FindingSeverity::Blocking)],
    )
    .await;
    let trigger = quality_receipt(&fixture, "reviewer-blocking-trigger", "reviewer-1");
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &trigger)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::RepairRequired);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("contradicts a blocking finding"))
    );
    let repair = fixture
        .db
        .repair_authorization_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("blocking finding creates a repair decision");
    assert_eq!(repair.decision, "DENIED");
    assert_eq!(repair.reason_code, "contradictory_review");
    assert_eq!(
        repair.total_points,
        FindingSeverity::Blocking.canonical_points()
    );
    assert_eq!(repair.blocking_count, 1);
    assert_eq!(repair.info_count, 0);
    fixture.db.close().await;
}

#[tokio::test]
async fn critical_review_finding_remains_a_human_stop_with_observations() {
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-critical-info", "verifier-1"),
        )
        .await
        .unwrap();
    let mut review = quality_receipt(&fixture, "reviewer-critical-info", "reviewer-1");
    review.result = ReceiptResult::ChangesRequested;
    review.findings = vec![
        review_finding("critical-1", FindingSeverity::Critical),
        review_finding("info-critical", FindingSeverity::Info),
    ];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::HumanRequired);
    assert!(
        gate.reasons
            .iter()
            .any(|reason| reason.contains("critical review finding"))
    );
    let repair = fixture
        .db
        .repair_authorization_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("critical finding creates a repair decision");
    assert_eq!(repair.decision, "HUMAN_REQUIRED");
    assert_eq!(repair.reason_code, "critical_zero");
    // The receipt is CHANGES_REQUESTED, so Info is not an observation and is
    // still scored exactly as before.
    assert_eq!(
        repair.total_points,
        FindingSeverity::Critical.canonical_points() + FindingSeverity::Info.canonical_points()
    );
    assert_eq!(repair.critical_count, 1);
    assert_eq!(repair.info_count, 1);
    fixture.db.close().await;
}

#[tokio::test]
async fn resolving_a_warning_reevaluates_the_gate_idempotently() {
    use crate::planning::{FindingDisposition, FindingRoute, FindingStatus};
    use crate::review::FindingSeverity;

    let fixture = candidate_fixture().await;
    let request_id = attach_finding_request(&fixture).await;
    fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Verifier,
            &quality_receipt(&fixture, "verifier-warning-reeval", "verifier-1"),
        )
        .await
        .unwrap();
    let mut review = quality_receipt(&fixture, "reviewer-warning-reeval", "reviewer-1");
    review.findings = vec![review_finding("warning-reeval", FindingSeverity::Warning)];
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &review)
        .await
        .unwrap();
    let gate = result.gate.expect("gate is decided");
    assert_eq!(gate.outcome, GateOutcome::RepairRequired);
    assert_eq!(
        run_status(
            &fixture.db,
            &run_id_for_candidate(&fixture.db, &fixture.candidate_id).await
        )
        .await,
        "REPAIR_REQUIRED"
    );

    let (campaign_id,): (String,) = sqlx::query_as::query_as(
        "SELECT r.campaign_id FROM candidates cd
         JOIN attempts a ON a.id=cd.attempt_id
         JOIN runs r ON r.id=a.run_id
         WHERE cd.id=?1",
    )
    .bind(&fixture.candidate_id)
    .fetch_one(&fixture.db.pool)
    .await
    .unwrap();
    let (candidate_count,): (i64,) =
        sqlx::query_as::query_as("SELECT COUNT(*) FROM candidates WHERE id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    let (receipt_count,): (i64,) =
        sqlx::query_as::query_as("SELECT COUNT(*) FROM quality_receipts WHERE candidate_id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();

    let dispositions = fixture.db.finding_dispositions(&request_id).await.unwrap();
    let warning = dispositions
        .iter()
        .find(|disposition| disposition.finding_id == "warning-reeval")
        .expect("warning disposition");
    assert_eq!(warning.status, "OPEN");
    assert_eq!(
        warning.candidate_id.as_deref(),
        Some(fixture.candidate_id.as_str())
    );

    // Re-evaluating unchanged evidence is a no-op.
    let unchanged = fixture
        .db
        .reevaluate_gate_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("gate exists");
    assert_eq!(unchanged.outcome, GateOutcome::RepairRequired);
    assert_eq!(
        count(&fixture.db, "SELECT COUNT(*) FROM gate_decision_revisions",).await,
        0
    );

    let resolved = FindingDisposition {
        request_id: request_id.clone(),
        finding_id: warning.finding_id.clone(),
        route: FindingRoute::Repairer,
        status: FindingStatus::Resolved,
        candidate_id: fixture.candidate_id.clone(),
        reviewer_principal: warning.reviewer_principal.clone(),
        resolved_by: "operator".into(),
        evidence_refs: vec!["test:warning_fixed".into()],
        owner_principal: None,
        owner_decision: None,
    };
    fixture
        .db
        .save_finding_disposition(&resolved)
        .await
        .unwrap();

    let revised = fixture
        .db
        .reevaluate_gate_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("gate exists");
    assert_eq!(revised.outcome, GateOutcome::Accepted);
    assert_eq!(
        run_status(
            &fixture.db,
            &run_id_for_candidate(&fixture.db, &fixture.candidate_id).await
        )
        .await,
        "ACCEPTED"
    );
    let campaign = fixture
        .db
        .campaigns()
        .await
        .unwrap()
        .into_iter()
        .find(|campaign| campaign.id == campaign_id)
        .expect("campaign");
    assert_eq!(campaign.effective_status, "ACCEPTED");

    // Re-evaluation accepts no new evidence and creates no candidate.
    let (after_candidates,): (i64,) =
        sqlx::query_as::query_as("SELECT COUNT(*) FROM candidates WHERE id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    let (after_receipts,): (i64,) =
        sqlx::query_as::query_as("SELECT COUNT(*) FROM quality_receipts WHERE candidate_id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert_eq!(after_candidates, candidate_count);
    assert_eq!(after_receipts, receipt_count);

    // The original decision remains immutable; the new decision is append-only.
    let (base_outcome,): (String,) =
        sqlx::query_as::query_as("SELECT outcome FROM gate_decisions WHERE candidate_id=?1")
            .bind(&fixture.candidate_id)
            .fetch_one(&fixture.db.pool)
            .await
            .unwrap();
    assert_eq!(base_outcome, "REPAIR_REQUIRED");
    assert_eq!(
        count(&fixture.db, "SELECT COUNT(*) FROM gate_decision_revisions",).await,
        1
    );

    // Replaying the same re-evaluation does not append a duplicate decision.
    let replay = fixture
        .db
        .reevaluate_gate_for_candidate(&fixture.candidate_id)
        .await
        .unwrap()
        .expect("gate exists");
    assert_eq!(replay.outcome, GateOutcome::Accepted);
    assert_eq!(
        count(&fixture.db, "SELECT COUNT(*) FROM gate_decision_revisions",).await,
        1
    );
    let events = fixture.db.events().await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "quality.gate_reevaluated")
            .count(),
        1
    );
    fixture.db.close().await;
}
