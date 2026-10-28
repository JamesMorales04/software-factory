//! Short-lived device enrollment with operator approval and one independent session per device.
use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use ring::rand::{SecureRandom, SystemRandom};

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::{Database, DevicePairingRecord, DeviceSessionRecord, Error};

const CHALLENGE_LIFETIME_MS: i64 = 180_000;

#[derive(Clone)]
struct PairingState {
    db: Database,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateChallengeInput {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimInput {
    #[serde(rename = "deviceName")]
    device_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChallengeResponse {
    challenge_id: String,
    verification_code: String,
    expires_at: i64,
}

/// Router merged below `/auth`; all management operations must be protected by Core operator-session middleware.
pub fn routes(db: Database) -> Router {
    Router::new()
        .route("/auth/access", get(access_status))
        .route("/auth/device-challenges", post(create_challenge))
        .route("/auth/device-claims/{id}", post(claim_device))
        .route("/auth/device-claims/{id}/poll", post(poll_claim))
        .route("/auth/devices", get(list_devices))
        .route("/auth/devices/revoke-all", post(revoke_all_devices))
        .route("/auth/devices/{id}/approve", post(approve_device))
        .route("/auth/devices/{id}/reject", post(reject_device))
        .route("/auth/devices/{id}", delete(revoke_device))
        .with_state(PairingState { db })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis() as i64)
        .unwrap_or(0)
}

fn secret() -> Option<String> {
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn valid_secret(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn verification_code() -> String {
    format!("{:06}", Uuid::new_v4().as_u128() % 1_000_000)
}

async fn create_challenge(
    State(state): State<PairingState>,
    operator: Option<Extension<DeviceSessionRecord>>,
    Json(_input): Json<CreateChallengeInput>,
) -> Response {
    let Some(Extension(operator)) = operator else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let now = now_ms();
    let id = Uuid::new_v4().to_string();
    let code = verification_code();
    let expires = now.saturating_add(CHALLENGE_LIFETIME_MS);
    let actor = operator.id.as_str();
    if let Err(e) = state
        .db
        .create_device_pairing(&id, &code, actor, now, expires)
        .await
    {
        return db_error(e);
    }
    Json(ChallengeResponse {
        challenge_id: id,
        verification_code: code,
        expires_at: expires,
    })
    .into_response()
}

async fn claim_device(
    State(state): State<PairingState>,
    Path(id): Path<String>,
    Json(input): Json<ClaimInput>,
) -> Response {
    let poll_token = match secret() {
        Some(v) => v,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "secure randomness unavailable",
            )
                .into_response();
        }
    };
    match state
        .db
        .claim_device_pairing(&id, &hash(&poll_token), &input.device_name, now_ms())
        .await
    {
        Ok(record) => {
            crate::telemetry::register_secret(&poll_token);
            Json(json!({"deviceId":id,"verificationCode":record.verification_code,"status":"PENDING","expiresAt":record.expires_at_ms,"pollToken":poll_token})).into_response()
        }
        Err(error) => db_error(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PollInput {
    #[serde(rename = "pollToken")]
    poll_token: String,
}

async fn poll_claim(
    State(state): State<PairingState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<PollInput>,
) -> Response {
    let token = input.poll_token.as_str();
    if !valid_secret(token) {
        return (StatusCode::UNAUTHORIZED, "invalid pairing poll").into_response();
    }
    match state.db.poll_device_pairing(&id, &hash(token), now_ms()).await {
        Ok(poll) if poll.status == "APPROVED" => {
            if let Err(error) = state.db.consume_device_pairing(&id, &hash(token), now_ms()).await {
                return db_error(error);
            }
            let issued = match crate::device_session::issue(
                &state.db,
                poll.device_name.as_deref().unwrap_or("Mobile device"),
                "approved-device-pairing",
                now_ms(),
            ).await {
                Ok(value) => value,
                Err(error) => return db_error(error),
            };
            if let Err(error) = state.db.attach_pairing_device_session(&id, &issued.session_id).await {
                let _ = state.db.revoke_device_session(&issued.session_id, "pairing-attachment-failure", now_ms()).await;
                return db_error(error);
            }
            crate::telemetry::register_secret(&issued.session_token);
            crate::telemetry::register_secret(&issued.csrf_token);
            let mut response = Json(json!({"status":"APPROVED","csrfToken":issued.csrf_token,"expiresAt":issued.device.expires_at_ms})).into_response();
            crate::device_session::set_cookies(
                &mut response,
                &issued,
                crate::device_session::secure_cookie_context(
                    &headers,
                    &crate::device_session::trusted_origins(),
                ),
            );
            response
        }
        Ok(poll) => Json(json!({"status":poll.status,"verificationCode":poll.verification_code,"expiresAt":poll.expires_at_ms})).into_response(),
        Err(error) => db_error(error),
    }
}

async fn list_devices(
    State(state): State<PairingState>,
    operator: Option<Extension<DeviceSessionRecord>>,
) -> Response {
    if operator.is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let now = now_ms();
    let pairings = match state.db.list_device_pairings(now).await {
        Ok(value) => value,
        Err(error) => return db_error(error),
    };
    let sessions = match state.db.device_sessions().await {
        Ok(value) => value,
        Err(error) => return db_error(error),
    };
    let mut devices: Vec<serde_json::Value> = pairings.into_iter().map(device_view).collect();
    for session in sessions {
        if session.revoked_at_ms.is_none() && session.expires_at_ms > now {
            devices.push(json!({"id":session.id,"name":session.name,"status":"CONNECTED","createdAt":session.created_at_ms,"lastActivityAt":session.last_activity_at_ms,"expiresAt":session.expires_at_ms}));
        }
    }
    Json(json!({"devices":devices})).into_response()
}
fn device_view(record: DevicePairingRecord) -> serde_json::Value {
    let id = record.device_session_id.unwrap_or(record.id);
    let status = if record.status == "CONSUMED" {
        "CONNECTED".to_owned()
    } else {
        record.status
    };
    let verification_code = (status == "PENDING").then_some(record.verification_code);
    json!({
        "id": id,
        "status": status,
        "verificationCode": verification_code,
        "name": record.device_name,
        "createdAt": record.created_at_ms,
        "lastActivityAt": record.claimed_at_ms,
        "expiresAt": record.expires_at_ms
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalInput {
    #[serde(rename = "verificationCode")]
    verification_code: String,
}

async fn approve_device(
    State(state): State<PairingState>,
    Path(id): Path<String>,
    operator: Option<Extension<DeviceSessionRecord>>,
    Json(input): Json<ApprovalInput>,
) -> Response {
    let Some(Extension(operator)) = operator else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    decide(state, id, true, Some(input.verification_code), operator.id).await
}
async fn reject_device(
    State(state): State<PairingState>,
    Path(id): Path<String>,
    operator: Option<Extension<DeviceSessionRecord>>,
) -> Response {
    let Some(Extension(operator)) = operator else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    decide(state, id, false, None, operator.id).await
}
async fn decide(
    state: PairingState,
    id: String,
    approve: bool,
    verification_code: Option<String>,
    actor: String,
) -> Response {
    match state
        .db
        .decide_device_pairing(&id, approve, verification_code.as_deref(), &actor, now_ms())
        .await
    {
        Ok(record) => Json(device_view(record)).into_response(),
        Err(error) => db_error(error),
    }
}
async fn revoke_device(
    State(state): State<PairingState>,
    Path(id): Path<String>,
    operator: Option<Extension<DeviceSessionRecord>>,
) -> Response {
    let Some(Extension(operator)) = operator else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match state
        .db
        .revoke_pairing_device(&id, &operator.id, now_ms())
        .await
    {
        Ok(Some(session_id)) => match state
            .db
            .revoke_device_session(&session_id, &operator.id, now_ms())
            .await
        {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => db_error(error),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "device not found").into_response(),
        Err(error) => db_error(error),
    }
}

async fn revoke_all_devices(
    State(state): State<PairingState>,
    operator: Option<Extension<DeviceSessionRecord>>,
) -> Response {
    let Some(Extension(operator)) = operator else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match state
        .db
        .revoke_all_device_sessions(&operator.id, now_ms())
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => db_error(error),
    }
}

async fn access_status() -> Response {
    let endpoint = env::var("FACTORY_TAILSCALE_ORIGIN")
        .ok()
        .filter(|v| valid_private_origin(v));
    Json(json!({"local":{"status":"AVAILABLE"},"tailscale":{"enabled":endpoint.is_some(),"status":if endpoint.is_some(){"UNKNOWN"}else{"DISABLED"},"httpsEndpoint":endpoint,"error":null}})).into_response()
}
fn valid_private_origin(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && !url.host_str().unwrap_or_default().is_empty()
            && url.username().is_empty()
            && url.password().is_none()
            && (url.path().is_empty() || url.path() == "/")
            && url.query().is_none()
            && url.fragment().is_none()
    })
}
fn db_error(error: Error) -> Response {
    match error {
        Error::NotFound => (
            StatusCode::NOT_FOUND,
            "pairing not found, expired, or already used",
        )
            .into_response(),
        Error::Conflict(message) => (StatusCode::CONFLICT, message).into_response(),
        Error::Invalid(message) => (StatusCode::BAD_REQUEST, message).into_response(),
        other => {
            tracing::error!(error = %other, "device pairing operation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "device pairing operation failed",
            )
                .into_response()
        }
    }
}
