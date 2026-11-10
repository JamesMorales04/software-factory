//! Feature-gated HTTP support for WP10 browser acceptance.
//!
//! The handlers are registered only in debug/test-support builds. They seed
//! labelled fixture rows and can arm a one-shot trace fault so the browser can
//! exercise the real Core error/retry state. They never grant authority.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::{App, authorized, operator_principal};

/// The browser harness arms the fault only for this exact seeded retry request.
/// The map is therefore bounded to a single entry and cannot grow with caller
/// supplied IDs.
const TRACE_FAULT_MAX_ENTRIES: usize = 1;
const TRACE_RETRY_REQUEST_ID: &str = factory_native::db::WAVE12_TRACE_RETRY_REQUEST;

static TRACE_FAULTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
static TRACE_OVERRIDES: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();

fn faults() -> &'static Mutex<HashMap<String, usize>> {
    TRACE_FAULTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn trace_fault_id_allowed(id: &str) -> bool {
    id == TRACE_RETRY_REQUEST_ID
}

async fn arm_trace_fault(id: &str) -> bool {
    if !trace_fault_id_allowed(id) {
        return false;
    }
    let mut faults = faults().lock().await;
    if !faults.contains_key(id) && faults.len() >= TRACE_FAULT_MAX_ENTRIES {
        return false;
    }
    // React StrictMode invokes the trace effect twice in development. Fail
    // both reads so the browser reaches the error state; the explicit Retry
    // then reads the real projection.
    faults.insert(id.to_string(), 2);
    true
}

fn overrides() -> &'static Mutex<HashMap<String, Value>> {
    TRACE_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn seed(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    session: Option<Extension<factory_native::db::DeviceSessionRecord>>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let principal = operator_principal(session.as_ref().map(|Extension(device)| device));
    let manifest = match app.db.seed_wave12_fixtures(&principal).await {
        Ok(manifest) => manifest,
        Err(error) => return super::api_error(error),
    };
    install_duplicate_trace().await;
    Json(manifest).into_response()
}

pub async fn arm_trace_failure(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !trace_fault_id_allowed(&id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "the trace fault is restricted to the seeded retry request",
                "code": "TEST_TRACE_FAULT_ID_INVALID",
            })),
        )
            .into_response();
    }
    if !arm_trace_fault(&id).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "the trace fault map is at its bound",
                "code": "TEST_TRACE_FAULT_BOUNDED",
            })),
        )
            .into_response();
    }
    Json(json!({"armed": true, "requestId": id})).into_response()
}

/// Called by the authenticated trace handler before the durable read. A fault
/// wins once, then the next read reaches the real Core projection.
pub async fn trace_intercept(id: &str) -> Option<Response> {
    let mut faults = faults().lock().await;
    let mut fire = false;
    if let Some(remaining) = faults.get_mut(id) {
        fire = true;
        if *remaining > 1 {
            *remaining -= 1;
        } else {
            *remaining = 0;
        }
    }
    if fire && faults.get(id).copied() == Some(0) {
        faults.remove(id);
    }
    drop(faults);
    if fire {
        return Some(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "seeded trace fault for browser acceptance",
                    "code": "TEST_TRACE_FAULT",
                })),
            )
                .into_response(),
        );
    }
    overrides()
        .lock()
        .await
        .get(id)
        .cloned()
        .map(|value| Json(value).into_response())
}

async fn install_duplicate_trace() {
    use factory_native::{
        planning::WorkflowState,
        replay::ReplayEvent,
        trace::{LoadedTraceEvent, TraceBounds, project},
        workflow::WorkflowRunRecord,
    };
    let request_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let run_id = format!("workflow-{request_id}");
    let recorded_at = "2026-11-10T00:00:00Z".to_string();
    let run = WorkflowRunRecord {
        id: run_id.clone(),
        request_id: request_id.into(),
        campaign_id: None,
        state: WorkflowState::Planned,
        revision: 2,
        plan_sha256: "e".repeat(64),
        created_at: recorded_at.clone(),
        updated_at: recorded_at.clone(),
    };
    let event = |sequence: u64, from: WorkflowState, to: WorkflowState| ReplayEvent {
        sequence,
        request_id: request_id.into(),
        run_id: run_id.clone(),
        step_id: None,
        kind: "workflow.transitioned".into(),
        state_from: Some(from),
        state_to: Some(to),
        payload_sha256: "a".repeat(64),
        recorded_at: recorded_at.clone(),
    };
    let first = event(1, WorkflowState::Submitted, WorkflowState::Assessing);
    let second = event(2, WorkflowState::Assessing, WorkflowState::Planned);
    let first_payload =
        factory_native::replay::redact_payload(&json!({"from": "SUBMITTED", "to": "ASSESSING"}));
    let second_payload =
        factory_native::replay::redact_payload(&json!({"from": "ASSESSING", "to": "PLANNED"}));
    let loaded = vec![
        LoadedTraceEvent {
            event: first.clone(),
            payload: first_payload.clone(),
            malformed: false,
        },
        LoadedTraceEvent {
            event: first,
            payload: first_payload,
            malformed: false,
        },
        LoadedTraceEvent {
            event: second,
            payload: second_payload,
            malformed: false,
        },
    ];
    let view = project(
        request_id,
        run,
        loaded,
        Vec::new(),
        TraceBounds::new(200, false, 500, false),
    );
    if let Ok(value) = serde_json::to_value(view) {
        overrides()
            .lock()
            .await
            .insert(request_id.to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trace_fault_is_bounded_and_restricted_to_the_seeded_retry_request() {
        faults().lock().await.clear();
        let unrelated = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        assert!(!trace_fault_id_allowed(unrelated));
        assert!(!arm_trace_fault(unrelated).await);
        assert!(faults().lock().await.is_empty());

        assert!(trace_fault_id_allowed(TRACE_RETRY_REQUEST_ID));
        assert!(arm_trace_fault(TRACE_RETRY_REQUEST_ID).await);
        assert!(arm_trace_fault(TRACE_RETRY_REQUEST_ID).await);
        assert_eq!(faults().lock().await.len(), 1);
        assert_eq!(
            faults().lock().await.get(TRACE_RETRY_REQUEST_ID).copied(),
            Some(2)
        );

        // The map never exceeds its strict bound even if a stale entry is
        // present; the endpoint rejects rather than growing.
        faults().lock().await.clear();
        faults().lock().await.insert("stale".into(), 1);
        assert!(!arm_trace_fault(TRACE_RETRY_REQUEST_ID).await);
        faults().lock().await.clear();

        assert!(arm_trace_fault(TRACE_RETRY_REQUEST_ID).await);
        assert!(trace_intercept(TRACE_RETRY_REQUEST_ID).await.is_some());
        assert!(trace_intercept(TRACE_RETRY_REQUEST_ID).await.is_some());
        assert!(trace_intercept(TRACE_RETRY_REQUEST_ID).await.is_none());
        assert!(faults().lock().await.is_empty());
    }
}
