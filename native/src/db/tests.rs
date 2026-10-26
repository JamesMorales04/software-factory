use super::*;

use chrono::Utc;
use sqlx::connection::Connection as SqlxConnection;
use sqlx_sqlite::{SqliteConnectOptions, SqliteConnection};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::lifecycle::load_attempt;

fn temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.sqlite", Uuid::new_v4()))
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
            max_attempts: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let run = db.create_run(&campaign.id, "run-key").await.unwrap();
    let attempt = db.create_attempt(&run.id, "attempt-key").await.unwrap();
    attempt.id
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
            max_attempts: 1,
            ..Default::default()
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
            max_attempts: 1,
            ..Default::default()
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

fn quality_receipt(fixture: &CandidateFixture, id: &str, principal: &str) -> QualityReceiptInput {
    QualityReceiptInput {
        receipt_id: id.into(),
        principal_id: principal.into(),
        action_spec_sha256: fixture.spec_sha256.clone(),
        commit_sha: fixture.commit_sha.clone(),
        tree_sha: fixture.tree_sha.clone(),
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
            evidence_refs: vec!["seed.txt".into()],
        }]
    };
    QualityReceiptInput {
        receipt_id: id.into(),
        principal_id: "reviewer-model".into(),
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
async fn model_review_approval_requires_matching_settled_reservation() {
    let fixture = candidate_fixture().await;
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
    let result = fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Reviewer, &reviewer)
        .await
        .unwrap();
    assert_eq!(result.gate.unwrap().outcome, GateOutcome::RepairRequired);
    assert_eq!(run_status(&fixture.db, &run_id).await, "REPAIR_REQUIRED");
    fixture.db.close().await;
}

#[tokio::test]
async fn blocked_gate_projects_run_status_to_rejected() {
    let fixture = candidate_fixture().await;
    let run_id = run_id_for_candidate(&fixture.db, &fixture.candidate_id).await;
    let mut verifier = quality_receipt(&fixture, "verifier-run-blocked", "verifier-1");
    verifier.evidence = EvidenceKind::Mocked;
    fixture
        .db
        .record_quality_receipt(&fixture.candidate_id, QualityRole::Verifier, &verifier)
        .await
        .unwrap();
    let result = fixture
        .db
        .record_quality_receipt(
            &fixture.candidate_id,
            QualityRole::Reviewer,
            &quality_receipt(&fixture, "reviewer-run-blocked", "reviewer-1"),
        )
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
                },
                Utc::now().timestamp_millis(),
            )
            .await
            .is_err()
    );
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
                    max_attempts: 1,
                    ..Default::default()
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
            max_attempts: 1,
            ..Default::default()
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
async fn model_call_reservations_settle_replay_and_fence_duplicates() {
    let (path, db) = temp_db("factory-model-calls").await;
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
    let policy = model_call_policy(1_000, 500, 10_000);
    let first = model_call_input(&"b".repeat(64), "evaluation-2", 120, 300, 200, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&first).await.unwrap() else {
        panic!("expected a reservation");
    };
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
    assert!(matches!(
        db.authorize_model_call(&third).await,
        Err(Error::Forbidden(_))
    ));
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
async fn model_call_scope_and_global_budgets_are_enforced() {
    let (path, db) = temp_db("factory-model-call-budget").await;
    let policy = model_call_policy(100, 50, 1_000);
    let first = model_call_input(&"e".repeat(64), "evaluation-budget", 40, 10, 20, policy);
    let ModelCallDecision::Reserved(grant) = db.authorize_model_call(&first).await.unwrap() else {
        panic!("expected a reservation");
    };
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
