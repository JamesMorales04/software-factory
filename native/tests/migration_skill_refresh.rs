//! Upgrade evidence for migrations 0067 and 0068.
//!
//! A populated pre-change database is upgraded through the embedded migrator.
//! The durable project cursor must survive reopen, and historical skill-pack
//! rows must retain their source-check date while gaining an empty content
//! review field (fail closed until an explicitly reviewed revision exists).

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
        if version > 0 && version <= 64 && name.ends_with(".sql") {
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
    let legacy_json = r#"{
        "technology":"rust",
        "version":"1.90",
        "sourceUrls":["https://doc.rust-lang.org/"],
        "lastVerifiedAt":"2026-01-01T00:00:00Z",
        "reviewAfter":"2026-04-01T00:00:00Z",
        "bestPractices":["use cargo test"],
        "antiPatterns":[],
        "commands":[],
        "securityNotes":[],
        "deprecations":[],
        "license":"CC-BY-4.0",
        "provenance":"official",
        "contentSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }"#;
    sqlx::query::query(
        "INSERT INTO skill_packs
         (id,technology,version,content_json,content_sha256,source_urls_json,
          last_verified_at,review_after,license,provenance,created_at)
         VALUES ('historical','rust','1.90',?1,?2,'[]',
                 '2026-01-01T00:00:00Z','2026-04-01T00:00:00Z','MIT','historical',
                 '2026-01-01T00:00:00Z')",
    )
    .bind(legacy_json)
    .bind("a".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    database
}

#[tokio::test]
async fn migration_0067_cursor_and_0068_review_split_preserve_history() {
    let root = std::env::temp_dir().join(format!(
        "factory-migration-skill-refresh-{}-{}",
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
    assert_eq!(db.skill_refresh_project_offset().await.unwrap(), 0);
    db.set_skill_refresh_project_offset(37).await.unwrap();
    let packs = db.skill_packs().await.unwrap();
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].id, "historical");
    assert_eq!(packs[0].source_checked_at, "2026-01-01T00:00:00Z");
    assert_eq!(packs[0].content_reviewed_at, "");
    let legacy =
        factory_native::planning::SkillPack::from_persisted_json(&packs[0].content_json).unwrap();
    assert_eq!(legacy.source_checked_at, "2026-01-01T00:00:00Z");
    assert_eq!(legacy.content_reviewed_at, "");
    assert_eq!(legacy.review_after, "");
    assert_eq!(
        legacy.review_basis,
        factory_native::planning::SkillReviewBasis::SourceAvailabilityOnly
    );
    assert!(legacy.is_stale("2026-11-09T00:00:00Z"));
    db.close().await;

    let reopened = Database::open(&database).await.unwrap();
    assert_eq!(reopened.skill_refresh_project_offset().await.unwrap(), 37);
    reopened.close().await;

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
    for version in [67, 68] {
        let (count,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=?1 AND success=1",
        )
        .bind(version)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "migration {version} must be applied");
    }
    pool.close().await;
    let _ = std::fs::remove_file(database);
    let _ = std::fs::remove_dir_all(root);
}
