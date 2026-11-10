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

static TRACE_FAULTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
static TRACE_OVERRIDES: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();

fn faults() -> &'static Mutex<HashMap<String, usize>> {
    TRACE_FAULTS.get_or_init(|| Mutex::new(HashMap::new()))
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
    // The Office runs under React StrictMode in development, which invokes the
    // trace effect twice. Fail both reads so the browser reaches the error
    // state; the explicit Retry then reads the real projection.
    faults().lock().await.insert(id.clone(), 2);
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
