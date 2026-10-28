//! Opaque browser/device session issuance. Only SHA-256 token digests are persisted.

use crate::db::{Database, DeviceSessionRecord, Error, NewDeviceSession};
use axum::{
    http::{HeaderMap, Uri, header},
    response::Response,
};
use sha2::{Digest, Sha256};
use std::env;

pub const SESSION_LIFETIME_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Read exact HTTPS origins explicitly trusted by Core's Office proxy boundary.
pub fn trusted_origins() -> Vec<String> {
    env::var("FACTORY_TRUSTED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug)]
pub struct IssuedDeviceSession {
    pub session_id: String,
    pub session_token: String,
    pub csrf_token: String,
    pub device: DeviceSessionRecord,
}

/// Create an independent device session. Raw session and CSRF tokens are returned once;
/// Core storage receives only their SHA-256 digests.
pub async fn issue(
    db: &Database,
    device_name: &str,
    created_by: &str,
    now_ms: i64,
) -> Result<IssuedDeviceSession, Error> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_token = uuid::Uuid::new_v4().simple().to_string();
    let csrf_token = uuid::Uuid::new_v4().simple().to_string();
    let device = db
        .create_device_session(NewDeviceSession {
            id: &session_id,
            name: device_name,
            session_token_sha256: &sha256_hex(&session_token),
            csrf_token_sha256: &sha256_hex(&csrf_token),
            now_ms,
            expires_at_ms: now_ms.saturating_add(SESSION_LIFETIME_MS),
            created_by,
        })
        .await?;
    Ok(IssuedDeviceSession {
        session_id,
        session_token,
        csrf_token,
        device,
    })
}

/// Claim the one-time Owner bootstrap slot while persisting the initial device session.
pub async fn issue_owner(
    db: &Database,
    device_name: &str,
    now_ms: i64,
) -> Result<IssuedDeviceSession, Error> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_token = uuid::Uuid::new_v4().simple().to_string();
    let csrf_token = uuid::Uuid::new_v4().simple().to_string();
    let device = db
        .create_owner_device_session(
            &session_id,
            device_name,
            &sha256_hex(&session_token),
            &sha256_hex(&csrf_token),
            now_ms,
            now_ms.saturating_add(SESSION_LIFETIME_MS),
        )
        .await?;
    Ok(IssuedDeviceSession {
        session_id,
        session_token,
        csrf_token,
        device,
    })
}

/// Rotate the active credentials for one device without changing its identity or expiry.
pub async fn rotate(
    db: &Database,
    device: &DeviceSessionRecord,
    actor: &str,
    now_ms: i64,
) -> Result<Option<IssuedDeviceSession>, Error> {
    let session_token = uuid::Uuid::new_v4().simple().to_string();
    let csrf_token = uuid::Uuid::new_v4().simple().to_string();
    if !db
        .rotate_device_session(
            &device.id,
            &sha256_hex(&session_token),
            &sha256_hex(&csrf_token),
            actor,
            now_ms,
        )
        .await?
    {
        return Ok(None);
    }
    let mut updated = device.clone();
    updated.last_activity_at_ms = now_ms;
    Ok(Some(IssuedDeviceSession {
        session_id: device.id.clone(),
        session_token,
        csrf_token,
        device: updated,
    }))
}

pub fn sha256_hex(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Append device session and CSRF cookies to an API response.
pub fn set_cookies(response: &mut Response, issued: &IssuedDeviceSession, secure: bool) {
    let max_age = issued
        .device
        .expires_at_ms
        .saturating_sub(chrono::Utc::now().timestamp_millis())
        .max(0)
        / 1000;
    let secure_attribute = if secure { "; Secure" } else { "" };
    if let Ok(value) = format!(
        "factory_session={}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure_attribute}",
        issued.session_token
    )
    .parse()
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    if let Ok(value) = format!(
        "factory_csrf={}; Path=/; Max-Age={max_age}; SameSite=Strict{secure_attribute}",
        issued.csrf_token
    )
    .parse()
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

/// Clear both cookies for the active browser origin.
pub fn clear_cookies(response: &mut Response, secure: bool) {
    let secure_attribute = if secure { "; Secure" } else { "" };
    if let Ok(value) =
        format!("factory_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict{secure_attribute}")
            .parse()
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    if let Ok(value) =
        format!("factory_csrf=; Path=/; Max-Age=0; SameSite=Strict{secure_attribute}").parse()
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

/// Decide whether cookies must carry Secure from a previously validated exact origin/host.
pub fn secure_cookie_context(headers: &HeaderMap, trusted_origins: &[String]) -> bool {
    let origin_secure = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| {
            origin.starts_with("https://") && trusted_origins.iter().any(|value| value == origin)
        });
    let host_secure = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|host| host.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| {
            trusted_origins.iter().any(|origin| {
                origin.parse::<Uri>().ok().is_some_and(|uri| {
                    uri.scheme_str() == Some("https")
                        && uri.authority().is_some_and(|allowed| {
                            allowed.host().eq_ignore_ascii_case(authority.host())
                                && allowed.port_u16().unwrap_or(443)
                                    == authority.port_u16().unwrap_or(443)
                        })
                })
            })
        });
    origin_secure || host_secure
}
