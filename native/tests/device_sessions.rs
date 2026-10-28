use std::{fs, path::PathBuf};

use factory_native::{db::Database, device_session};

fn database_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "factory-device-session-{}.sqlite",
        uuid::Uuid::new_v4()
    ))
}

#[tokio::test]
async fn session_tokens_are_hashed_persistent_rotatable_and_revocable() {
    let path = database_path();
    let db = Database::open(&path).await.unwrap();
    assert!(db.owner_bootstrap_available().await.unwrap());

    let now = chrono::Utc::now().timestamp_millis();
    let owner = device_session::issue_owner(&db, "Desktop", now)
        .await
        .unwrap();
    assert!(!db.owner_bootstrap_available().await.unwrap());
    assert!(
        db.device_session_by_token_hash(&device_session::sha256_hex(&owner.session_token), now)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        db.device_session_by_token_hash(&owner.session_token, now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.device_session_csrf_matches(
            &owner.session_id,
            &device_session::sha256_hex(&owner.csrf_token),
            now,
        )
        .await
        .unwrap()
    );

    let rotated = device_session::rotate(&db, &owner.device, &owner.session_id, now + 1)
        .await
        .unwrap()
        .unwrap();
    assert!(
        db.device_session_by_token_hash(&device_session::sha256_hex(&owner.session_token), now + 1)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.device_session_by_token_hash(
            &device_session::sha256_hex(&rotated.session_token),
            now + 1,
        )
        .await
        .unwrap()
        .is_some()
    );

    db.close().await;
    let reopened = Database::open(&path).await.unwrap();
    assert!(
        reopened
            .device_session_by_token_hash(
                &device_session::sha256_hex(&rotated.session_token),
                now + 2,
            )
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        reopened
            .revoke_device_session(&owner.session_id, "test", now + 3)
            .await
            .unwrap()
    );
    assert!(
        reopened
            .device_session_by_token_hash(
                &device_session::sha256_hex(&rotated.session_token),
                now + 4,
            )
            .await
            .unwrap()
            .is_none()
    );
    reopened.close().await;
    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn recovery_revokes_sessions_and_reopens_single_owner_bootstrap() {
    let path = database_path();
    let db = Database::open(&path).await.unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let owner = device_session::issue_owner(&db, "Desktop", now)
        .await
        .unwrap();
    let revoked = db
        .reset_owner_bootstrap("local-recovery", now + 1)
        .await
        .unwrap();
    assert_eq!(revoked, 1);
    assert!(db.owner_bootstrap_available().await.unwrap());
    assert!(db
        .device_session_by_token_hash(
            &device_session::sha256_hex(&owner.session_token),
            now + 2,
        )
        .await
        .unwrap()
        .is_none());
    let recovered = device_session::issue_owner(&db, "Recovered desktop", now + 2)
        .await
        .unwrap();
    assert!(!db.owner_bootstrap_available().await.unwrap());
    assert_ne!(owner.session_id, recovered.session_id);
    db.close().await;
    let _ = fs::remove_file(path);
}
