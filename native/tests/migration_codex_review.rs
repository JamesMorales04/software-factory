//! Upgrade evidence for migration 0048.
//!
//! A populated migration-0047 database is opened through the embedded
//! migrator. Historical deterministic and MODEL receipts must survive, the
//! uniqueness indexes and immutability triggers must remain present, and the
//! new CODEX_APP_SERVER constraint must reject incomplete provenance.

use std::path::Path;

use factory_native::db::Database;
use sqlx_sqlite::{SqliteConnectOptions, SqlitePoolOptions};

async fn pre_change_database(root: &Path) -> std::path::PathBuf {
    let migrations = root.join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();
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
        if version > 0 && version <= 47 && name.ends_with(".sql") {
            std::fs::copy(entry.path(), migrations.join(name.as_ref())).unwrap();
        }
    }
    let database = root.join("factory.sqlite");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    let now = "2026-11-05T00:00:00Z";
    for statement in [
        "INSERT INTO projects(id,repository,base_commit,created_at) VALUES ('project','/fixture','base',?1)",
        "INSERT INTO actions(id,project_id,objective,provider,model,risk,status,objective_sha256,created_at) VALUES ('action','project','objective','opencode','deepseek-v4.1-flash','R0','HUMAN_REQUIRED',?2,?1)",
        "INSERT INTO campaigns(id,action_id,status,created_at) VALUES ('campaign','action','AUTHORIZED',?1)",
        "INSERT INTO runs(id,campaign_id,status,created_at) VALUES ('run','campaign','RUNNING',?1)",
        "INSERT INTO attempts(id,run_id,status,created_at) VALUES ('attempt','run','LEASED',?1)",
        "INSERT INTO candidates(id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at) VALUES ('candidate','attempt',?2,?3,?4,'worker',?1)",
    ] {
        let query = sqlx::query::query(statement).bind(now);
        let query = if statement.contains("?2") {
            query.bind("a".repeat(64))
        } else {
            query
        };
        let query = if statement.contains("?3") {
            query.bind("b".repeat(40)).bind("c".repeat(40))
        } else {
            query
        };
        query.execute(&pool).await.unwrap();
    }
    sqlx::query::query(
        "INSERT INTO quality_receipts
         (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
          result,evidence,received_at,findings_json,mechanism,model_provider,model_name,
          model_reservation_id,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
          model_request_sha256)
         VALUES ('verifier','candidate','verifier','verifier',?1,?2,?3,'PASS','EXECUTED',?4,'[]',
                 'DETERMINISTIC','','','',0,0,0,0,0,'')",
    )
    .bind("a".repeat(64))
    .bind("b".repeat(40))
    .bind("c".repeat(40))
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO quality_receipts
         (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
          result,evidence,received_at,findings_json,mechanism,model_provider,model_name,
          model_reservation_id,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
          model_request_sha256)
         VALUES ('reviewer','candidate','reviewer','reviewer',?1,?2,?3,'PASS','EXECUTED',?4,'[]',
                 'MODEL','opencode','deepseek-v4.1-flash','reservation',10,20,30,25,1,?5)",
    )
    .bind("a".repeat(64))
    .bind("b".repeat(40))
    .bind("c".repeat(40))
    .bind(now)
    .bind("d".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    database
}

#[tokio::test]
async fn migration_0048_preserves_receipts_indexes_and_immutability() {
    let root = std::env::temp_dir().join(format!(
        "factory-migration-codex-review-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let database = pre_change_database(&root).await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let db = Database::open(&database).await.unwrap();
    db.close().await;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(false)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let receipts: Vec<(String, String)> = sqlx::query_as::query_as(
        "SELECT receipt_id,mechanism FROM quality_receipts ORDER BY receipt_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        receipts,
        vec![
            ("reviewer".into(), "MODEL".into()),
            ("verifier".into(), "DETERMINISTIC".into())
        ]
    );
    let indexes: Vec<(String,)> = sqlx::query_as::query_as(
        "SELECT name FROM sqlite_master WHERE type='index' AND name IN
         ('quality_receipt_model_reservation_idx','quality_receipt_model_request_idx')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(indexes.len(), 2);
    assert!(
        sqlx::query::query("UPDATE quality_receipts SET result='FAIL' WHERE receipt_id='reviewer'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query::query(
            "INSERT INTO quality_receipts
             (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
              result,evidence,received_at,findings_json,mechanism,model_provider,model_name,
              model_reservation_id,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
              model_request_sha256)
             VALUES ('forged','candidate','reviewer','reviewer',?1,?2,?3,'PASS','EXECUTED',?4,'[]',
                     'CODEX_APP_SERVER','codex','gpt-6-luna','reservation',10,20,30,0,1,?5)"
        )
        .bind("a".repeat(64))
        .bind("b".repeat(40))
        .bind("c".repeat(40))
        .bind("2026-11-05T00:00:00Z")
        .bind("e".repeat(64))
        .execute(&pool)
        .await
        .is_err()
    );
    pool.close().await;
    let _ = std::fs::remove_dir_all(root);
}
