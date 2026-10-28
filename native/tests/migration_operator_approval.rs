//! Upgrade evidence for migration 0027.
//!
//! A populated pre-change database is created with migrations 0001-0025, given
//! valid and incomplete historical specs plus FROZEN/READY/RUNNING requests,
//! candidates and receipts, and then opened through the embedded migrator. The
//! test verifies that original bytes/hashes survive, the new immutable evidence
//! tables and triggers enforce their invariants, and reopening is idempotent.

use std::path::{Path, PathBuf};
use std::time::Duration;

use factory_native::db::Database;
use sqlx_sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

fn temp_paths(label: &str) -> (PathBuf, PathBuf) {
    let unique = uuid::Uuid::new_v4();
    (
        std::env::temp_dir().join(format!("factory-migration-{label}-{unique}.sqlite")),
        std::env::temp_dir().join(format!("factory-migration-{label}-{unique}-migrations")),
    )
}

async fn open_pool(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .unwrap()
}

async fn create_pre_change_database(database: &Path, migration_dir: &Path) {
    std::fs::create_dir_all(migration_dir).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../migrations");
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let version = name
            .split('_')
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();
        if version > 0 && version <= 25 && name.ends_with(".sql") {
            std::fs::copy(entry.path(), migration_dir.join(name.as_ref())).unwrap();
        }
    }
    let migrator = sqlx::migrate::Migrator::new(migration_dir).await.unwrap();
    let pool = open_pool(database).await;
    migrator.run(&pool).await.unwrap();
    populate(&pool).await;
    pool.close().await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(database, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

async fn populate(pool: &SqlitePool) {
    let now = "2026-10-27T00:00:00Z";
    let base = "b".repeat(40);
    sqlx::query::query(
        "INSERT INTO projects(id,repository,base_commit,created_at,identity)
         VALUES ('project','/fixture',?1,?2,'fixture')",
    )
    .bind(&base)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO project_configs
         (project_id,revision,config_hash,repository,base_commit,allowed_paths_json,
          capabilities_json,quality_profile,active,updated_at)
         VALUES ('project',1,?1,'/fixture',?2,'[\"src\"]','[\"repository-edit\"]','generic',1,?3)",
    )
    .bind("c".repeat(64))
    .bind(&base)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at,created_at)
         VALUES ('grant','owner','audit','project',1,?1,'{}',NULL,?2)",
    )
    .bind("d".repeat(64))
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO delegation_usage VALUES ('grant',1,3,1)")
        .execute(pool)
        .await
        .unwrap();

    let valid_spec = format!(
        r#"{{"schema_version":1,"project_id":"project","repository":"/fixture","base_commit":"{base}","objective":"valid historical objective","objective_sha256":"{}","provider":"opencode","model":"deepseek-v4.1-flash","allowed_paths":["src"],"checks":[{{"id":"test","kind":"test","description":"test","command":["cargo","test"],"expect_exit":0,"timeout_secs":60}}]}}"#,
        "e".repeat(64)
    );
    let incomplete_spec = format!(
        r#"{{"schema_version":1,"project_id":"project","repository":"/fixture","base_commit":"{base}","objective":"incomplete historical objective","objective_sha256":"{}","provider":"opencode","model":"deepseek-v4.1-flash","allowed_paths":[],"checks":[]}}"#,
        "f".repeat(64)
    );
    let requests = [
        ("req-frozen", "a-frozen", "FROZEN", None, &valid_spec),
        ("req-ready", "a-ready", "READY", None, &incomplete_spec),
        (
            "req-running",
            "a-running",
            "RUNNING",
            Some("camp-running"),
            &valid_spec,
        ),
    ];
    for (request_id, action_id, state, campaign_id, spec_json) in requests {
        sqlx::query::query(
            "INSERT INTO actions
             (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
             VALUES (?1,'project','historical objective','opencode','deepseek-v4.1-flash',
                     'R0','AUTHORIZED',?2,?3)",
        )
        .bind(action_id)
        .bind("a".repeat(64))
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query::query("INSERT INTO action_specs VALUES (?1,?2,?3)")
            .bind(action_id)
            .bind(spec_json)
            .bind(if spec_json.contains("\"allowed_paths\":[]") {
                "2".repeat(64)
            } else {
                "1".repeat(64)
            })
            .execute(pool)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_requests
             (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,
              state,failure_reason,campaign_id,created_at,updated_at)
             VALUES (?1,'project',?2,'grant','historical objective',?3,?4,?5,NULL,?6,?7,?7)",
        )
        .bind(request_id)
        .bind(action_id)
        .bind("3".repeat(64))
        .bind("c".repeat(64))
        .bind(state)
        .bind(campaign_id)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_assessments(id,request_id,mechanism,assessment_json,schema_sha256,created_at)
             VALUES (?1,?2,'FIXTURE','{}',?3,?4)",
        )
        .bind(format!("assessment-{request_id}"))
        .bind(request_id)
        .bind("4".repeat(64))
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO lead_proposals(id,request_id,assessment_id,proposal_json,proposal_hash,created_at)
             VALUES (?1,?2,?3,'{}',?4,?5)",
        )
        .bind(format!("proposal-{request_id}"))
        .bind(request_id)
        .bind(format!("assessment-{request_id}"))
        .bind("5".repeat(64))
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO frozen_lead_plans
             (id,request_id,proposal_id,plan_hash,config_hash,delegation_hash,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(format!("plan-{request_id}"))
        .bind(request_id)
        .bind(format!("proposal-{request_id}"))
        .bind("6".repeat(64))
        .bind("c".repeat(64))
        .bind("d".repeat(64))
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    }

    sqlx::query::query("INSERT INTO campaigns VALUES ('camp-running','a-running','AUTHORIZED',?1)")
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO campaign_authorizations
         (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
          authorized_paths_json,max_attempts,created_at)
         VALUES ('camp-running','auth-running','R0','FIXTURE','owner','historical', '[\"src\"]',1,?1)",
    )
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query("INSERT INTO runs VALUES ('run-running','camp-running','QUEUED',?1)")
        .bind(now)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query::query(
        "INSERT INTO attempts(id,run_id,status,created_at,sandbox_ready)
         VALUES ('attempt-running','run-running','QUEUED',?1,0)",
    )
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO attempt_specs(attempt_id,action_id,base_commit,spec_json,spec_sha256)
         VALUES ('attempt-running','a-running',?1,'{\"attempt\":\"historical\"}',?2)",
    )
    .bind(&base)
    .bind("7".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO candidates
         (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
         VALUES ('candidate-running','attempt-running',?1,?2,?3,'worker',?4)",
    )
    .bind("7".repeat(64))
    .bind("8".repeat(40))
    .bind("9".repeat(40))
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO quality_receipts
         (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
          result,evidence,received_at)
         VALUES ('receipt-running','candidate-running','verifier','verifier',?1,?2,?3,'PASS','EXECUTED',?4)",
    )
    .bind("7".repeat(64))
    .bind("8".repeat(40))
    .bind("9".repeat(40))
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO gate_decisions(candidate_id,outcome,reasons_json,decided_at)
         VALUES ('candidate-running','ACCEPTED','[]',?1)",
    )
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
}

async fn scalar(pool: &SqlitePool, sql: &str) -> i64 {
    let (value,): (i64,) = sqlx::query_as::query_as(sql).fetch_one(pool).await.unwrap();
    value
}

#[tokio::test]
async fn migration_0027_preserves_populated_history_and_enforces_new_evidence() {
    let (database, migration_dir) = temp_paths("operator-approval");
    create_pre_change_database(&database, &migration_dir).await;

    let first = Database::open(&database).await.unwrap();
    first.close().await;
    let reopened = Database::open(&database).await.unwrap();
    reopened.close().await;

    let pool = open_pool(&database).await;
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=27 AND success=1"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM lead_requests WHERE state='FROZEN'"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM lead_requests WHERE state='READY'"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM lead_requests WHERE state='RUNNING'"
        )
        .await,
        1
    );
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM candidates").await, 1);
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM quality_receipts").await,
        1
    );
    let (valid_json, valid_sha): (String, String) = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id='a-frozen'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(valid_json.contains("\"allowed_paths\":[\"src\"]"));
    assert_eq!(valid_sha, "1".repeat(64));
    let (incomplete_json, incomplete_sha): (String, String) = sqlx::query_as::query_as(
        "SELECT spec_json,spec_sha256 FROM action_specs WHERE action_id='a-ready'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(incomplete_json.contains("\"checks\":[]"));
    assert_eq!(incomplete_sha, "2".repeat(64));
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM action_specs WHERE action_id='a-ready' AND spec_sha256='2222222222222222222222222222222222222222222222222222222222222222'"
        )
        .await,
        1
    );

    // Existing immutable evidence remains immutable.
    assert!(
        sqlx::query::query("UPDATE action_specs SET spec_json='{}' WHERE action_id='a-frozen'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query::query(
            "UPDATE attempt_specs SET spec_json='{}' WHERE attempt_id='attempt-running'"
        )
        .execute(&pool)
        .await
        .is_err()
    );

    // New draft/spec coexistence, withdrawal and start/binding evidence guards.
    sqlx::query::query(
        "INSERT INTO actions
         (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
         VALUES ('a-draft','project','draft','opencode','deepseek-v4.1-flash',
                 'UNASSESSED','HUMAN_REQUIRED',?1,'2026-10-27T00:00:00Z')",
    )
    .bind("a".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO action_drafts(action_id,assessment_input_json,input_sha256,created_at)
         VALUES ('a-draft','{}',?1,'2026-10-27T00:00:00Z')",
    )
    .bind("b".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query::query(
            "INSERT INTO action_drafts(action_id,assessment_input_json,input_sha256,created_at)
             VALUES ('a-frozen','{}',?1,'2026-10-27T00:00:00Z')"
        )
        .bind("b".repeat(64))
        .execute(&pool)
        .await
        .is_err()
    );
    assert!(
        sqlx::query::query("DELETE FROM action_drafts WHERE action_id='a-draft'")
            .execute(&pool)
            .await
            .is_err()
    );
    sqlx::query::query(
        "INSERT INTO lead_withdrawals(request_id,principal_id,principal_label,reason,created_at)
         VALUES ('req-frozen','operator',NULL,'historical withdrawal','2026-10-27T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query::query(
            "UPDATE lead_withdrawals SET reason='changed' WHERE request_id='req-frozen'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    sqlx::query::query(
        "INSERT INTO lead_start_evidence
         (request_id,campaign_id,mode,principal_id,principal_label,rationale,idempotency_key,
          request_hash,plan_hash,profile_revision_id,profile_hash,spec_sha256,config_hash,
          delegation_hash,policy_hash,authorization_digest,created_at)
         VALUES ('req-running','camp-running','OPERATOR','operator',NULL,'approved','key',
                 ?1,?2,NULL,NULL,?3,?4,?5,?6,?7,'2026-10-27T00:00:00Z')",
    )
    .bind("3".repeat(64))
    .bind("6".repeat(64))
    .bind("1".repeat(64))
    .bind("c".repeat(64))
    .bind("d".repeat(64))
    .bind("e".repeat(64))
    .bind("f".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query::query(
            "UPDATE lead_start_evidence SET rationale='changed' WHERE request_id='req-running'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    sqlx::query::query(
        "INSERT INTO campaign_execution_bindings
         (campaign_id,request_id,plan_id,spec_sha256,envelope_hash,created_at)
         VALUES ('camp-running','req-running','plan-req-running',?1,?2,'2026-10-27T00:00:00Z')",
    )
    .bind("1".repeat(64))
    .bind("e".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query::query(
            "DELETE FROM campaign_execution_bindings WHERE campaign_id='camp-running'"
        )
        .execute(&pool)
        .await
        .is_err()
    );
    pool.close().await;
    let _ = std::fs::remove_file(database);
    let _ = std::fs::remove_dir_all(migration_dir);
}
