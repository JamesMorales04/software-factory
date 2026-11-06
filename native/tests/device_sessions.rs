use std::{fs, path::PathBuf};

use axum::http::{HeaderMap, HeaderValue, header};
use factory_native::{
    db::{Database, NewDeviceSession},
    device_session,
};

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

#[tokio::test]
async fn activity_touch_is_throttled_to_reduce_writer_contention() {
    let path = database_path();
    let db = Database::open(&path).await.unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let owner = device_session::issue_owner(&db, "Desktop", now)
        .await
        .unwrap();
    let token_hash = device_session::sha256_hex(&owner.session_token);
    db.touch_device_session(&owner.session_id, now + 1_000)
        .await
        .unwrap();
    assert_eq!(
        db.device_session_by_token_hash(&token_hash, now + 1_001)
            .await
            .unwrap()
            .unwrap()
            .last_activity_at_ms,
        now
    );
    db.touch_device_session(&owner.session_id, now + 61_000)
        .await
        .unwrap();
    assert_eq!(
        db.device_session_by_token_hash(&token_hash, now + 61_001)
            .await
            .unwrap()
            .unwrap()
            .last_activity_at_ms,
        now + 61_000
    );
    db.close().await;
    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn sessions_expire_and_csrf_stops_matching_after_expiry() {
    let path = database_path();
    let db = Database::open(&path).await.unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    let session_token_sha256 = device_session::sha256_hex("short-lived-session-token");
    let csrf_token_sha256 = device_session::sha256_hex("short-lived-csrf-token");
    let issued = db
        .create_device_session(NewDeviceSession {
            id: &id,
            name: "Short-lived",
            session_token_sha256: &session_token_sha256,
            csrf_token_sha256: &csrf_token_sha256,
            now_ms: now,
            expires_at_ms: now + 1_000,
            created_by: "test",
        })
        .await
        .unwrap();
    assert!(
        db.device_session_by_token_hash(&session_token_sha256, now)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        db.device_session_by_token_hash(&session_token_sha256, now + 1_001)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !db.device_session_csrf_matches(&issued.id, &csrf_token_sha256, now + 1_001)
            .await
            .unwrap()
    );
    db.close().await;
    let _ = fs::remove_file(path);
}

#[test]
fn secure_cookies_require_an_exact_trusted_https_origin_or_host() {
    let trusted = vec!["https://factory-office.example.ts.net".to_string()];
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://factory-office.example.ts.net"),
    );
    assert!(device_session::secure_cookie_context(&headers, &trusted));
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("http://factory-office.example.ts.net"),
    );
    assert!(!device_session::secure_cookie_context(&headers, &trusted));
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("https://evil.example"),
    );
    assert!(!device_session::secure_cookie_context(&headers, &trusted));

    let mut host = HeaderMap::new();
    host.insert(
        header::HOST,
        HeaderValue::from_static("factory-office.example.ts.net"),
    );
    assert!(device_session::secure_cookie_context(&host, &trusted));
    host.insert(header::HOST, HeaderValue::from_static("evil.example"));
    assert!(!device_session::secure_cookie_context(&host, &trusted));
}
