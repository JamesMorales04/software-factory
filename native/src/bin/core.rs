use std::{env, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use factory_native::db::{
    Database, Error, NewCampaignAuthorization, NewCandidate, QualityReceiptInput, QualityRole,
    WorkerReport,
};
use factory_native::lead::StandingDelegation;
use factory_native::quality::{EvidenceKind, ReceiptResult};
use serde::Deserialize;
use serde_json::json;

struct App {
    db: Database,
    operator_token: String,
    worker_bootstrap_token: String,
    verifier_token: String,
    reviewer_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewProject {
    repository: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAction {
    project_id: String,
    objective: String,
    provider: String,
    model: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCampaign {
    action_id: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    worker_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerRegistration {
    worker_id: String,
    registration_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Heartbeat {
    attempt_id: String,
    worker_id: String,
    generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDelegation {
    delegation: StandingDelegation,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizeCampaign {
    authorization_id: String,
    risk_class: String,
    execution_mode: String,
    actor: String,
    rationale: String,
    authorized_paths: Vec<String>,
    max_attempts: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewLeadRequest {
    project_id: String,
    action_id: String,
    delegation_id: String,
    objective: String,
    project_config_hash: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityReceiptRequest {
    candidate_id: String,
    receipt_id: String,
    principal_id: String,
    #[serde(rename = "actionSpecSha256")]
    action_spec_sha256: String,
    #[serde(rename = "commitSha")]
    commit_sha: String,
    #[serde(rename = "treeSha")]
    tree_sha: String,
    result: ReceiptResult,
    evidence: EvidenceKind,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("factory_native=info")
        .init();
    let database = env::var("FACTORY_DATABASE").unwrap_or_else(|_| "factory-native.sqlite".into());
    let operator_token =
        env::var("FACTORY_OPERATOR_TOKEN").map_err(|_| "FACTORY_OPERATOR_TOKEN required")?;
    let worker_bootstrap_token = env::var("FACTORY_WORKER_BOOTSTRAP_TOKEN")
        .map_err(|_| "FACTORY_WORKER_BOOTSTRAP_TOKEN required")?;
    let verifier_token =
        env::var("FACTORY_VERIFIER_TOKEN").map_err(|_| "FACTORY_VERIFIER_TOKEN required")?;
    let reviewer_token =
        env::var("FACTORY_REVIEWER_TOKEN").map_err(|_| "FACTORY_REVIEWER_TOKEN required")?;
    let tokens = [
        operator_token.as_str(),
        worker_bootstrap_token.as_str(),
        verifier_token.as_str(),
        reviewer_token.as_str(),
    ];
    if operator_token.is_empty()
        || worker_bootstrap_token.len() < 32
        || verifier_token.len() < 32
        || reviewer_token.len() < 32
        || tokens
            .iter()
            .enumerate()
            .any(|(index, token)| token.is_empty() || tokens[index + 1..].contains(token))
    {
        return Err("operator, Worker bootstrap, verifier and reviewer tokens must be nonempty, at least 32 bytes for role tokens, and distinct".into());
    }
    let bind = env::var("FACTORY_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into());
    let addr: SocketAddr = bind.parse()?;
    if !addr.ip().is_loopback() {
        return Err("remote binding requires a separate authenticated transport design".into());
    }
    let app = Arc::new(App {
        db: Database::open(Path::new(&database)).await?,
        operator_token,
        worker_bootstrap_token,
        verifier_token,
        reviewer_token,
    });
    let router = build_router(app);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "native Factory Core listening");
    axum::serve(listener, router).await?;
    Ok(())
}

fn build_router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"healthy":true,"version":"0.1.0"})) }),
        )
        .route("/projects", get(projects).post(register_project))
        .route("/actions", get(actions).post(create_action))
        .route("/campaigns", get(campaigns).post(create_campaign))
        .route("/runs", get(runs))
        .route("/attempts", get(attempts))
        .route("/events", get(events))
        .route("/worker/register", post(register_worker))
        .route("/worker/claim", post(worker_claim))
        .route("/worker/heartbeat", post(worker_heartbeat))
        .route("/worker/report", post(worker_report))
        .route("/worker/candidate", post(worker_candidate))
        .route("/quality/receipts", post(quality_receipt))
        .route("/quality/candidates/{id}", get(quality_candidate))
        .route("/quality/candidates/{id}/receipts", get(quality_receipts))
        .route("/quality/candidates/{id}/gate", get(quality_gate))
        .route("/delegations", post(create_delegation))
        .route(
            "/delegations/{id}",
            get(get_delegation).post(revoke_delegation),
        )
        .route("/lead/requests", post(create_lead_request))
        .route("/lead/requests/{id}", get(get_lead_request))
        .route("/campaigns/{id}/authorize", post(authorize_campaign))
        .route("/campaigns/{id}/authorization", get(campaign_authorization))
        .route("/campaigns/{id}/runs", post(create_campaign_run))
        .route("/runs/{id}/attempts", post(create_run_attempt))
        .route("/candidates", get(candidates))
        .with_state(app)
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {token}").as_str())
}

fn api_error(error: Error) -> Response {
    let status = match error {
        Error::Invalid(_) | Error::Repository(_) | Error::Delegation(_) => StatusCode::BAD_REQUEST,
        Error::Conflict(_) | Error::Constraint(_) => StatusCode::CONFLICT,
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::NotFound => StatusCode::NOT_FOUND,
        Error::Busy => StatusCode::SERVICE_UNAVAILABLE,
        Error::Sqlx(_) | Error::Migrate(_) | Error::File(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error":error.to_string()}))).into_response()
}

async fn register_project(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewProject>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.idempotency_key.is_empty() {
        return api_error(Error::Invalid("idempotency key"));
    }
    match app
        .db
        .register_project(Path::new(&input.repository), &input.idempotency_key)
        .await
    {
        Ok(project) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(error) => api_error(error),
    }
}
async fn projects(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.projects().await)
}
async fn create_action(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewAction>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.idempotency_key.is_empty() {
        return api_error(Error::Invalid("idempotency key"));
    }
    match app
        .db
        .create_action(
            &input.project_id,
            &input.objective,
            &input.provider,
            &input.model,
            &input.idempotency_key,
        )
        .await
    {
        Ok(action) => (StatusCode::CREATED, Json(action)).into_response(),
        Err(error) => api_error(error),
    }
}
async fn actions(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.actions().await)
}
async fn create_campaign(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewCampaign>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.idempotency_key.is_empty() {
        return api_error(Error::Invalid("idempotency key"));
    }
    match app
        .db
        .create_campaign(&input.action_id, &input.idempotency_key)
        .await
    {
        Ok(campaign) => (StatusCode::CREATED, Json(campaign)).into_response(),
        Err(error) => api_error(error),
    }
}
async fn campaigns(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.campaigns().await)
}
async fn runs(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.runs().await)
}
async fn attempts(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.attempts().await)
}
async fn events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.events().await)
}
async fn register_worker(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<WorkerRegistration>,
) -> Response {
    if !authorized(&headers, &app.worker_bootstrap_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let session_token = format!(
        "{}.{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    match app.db.register_worker_session(&input.worker_id,&input.registration_key,&app.worker_bootstrap_token,&session_token).await {
        Ok(session)=>Json(json!({"worker_id":session.worker_id,"session_token":session_token,"created":session.created,"dispatch_supported":false})).into_response(),
        Err(error)=>api_error(error),
    }
}
async fn worker_claim(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Claim>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.worker_id.trim().is_empty() {
        return api_error(Error::Invalid("worker_id"));
    }
    match app
        .db
        .claim_assignment(
            &input.worker_id,
            chrono::Utc::now().timestamp_millis(),
            60_000,
        )
        .await
    {
        Ok(Some(assignment)) => Json(json!({
            "dispatch_supported": true,
            "assignment": assignment
        }))
        .into_response(),
        Ok(None) => Json(json!({
            "dispatch_supported": true,
            "assignment": null
        }))
        .into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_heartbeat(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Heartbeat>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .heartbeat(
            &input.attempt_id,
            &input.worker_id,
            input.generation,
            chrono::Utc::now().timestamp_millis(),
            15_000,
        )
        .await
    {
        Ok(lease) => Json(lease).into_response(),
        Err(error) => api_error(error),
    }
}
async fn worker_report(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<WorkerReport>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .report_terminal(&input, chrono::Utc::now().timestamp_millis())
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_candidate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewCandidate>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .record_candidate(&input, chrono::Utc::now().timestamp_millis())
        .await
    {
        Ok(candidate) => (StatusCode::CREATED, Json(candidate)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn quality_receipt(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(role) = quality_role(&app, &headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let input: QualityReceiptRequest = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid receipt payload"})),
            )
                .into_response();
        }
    };
    let receipt = QualityReceiptInput {
        receipt_id: input.receipt_id,
        principal_id: input.principal_id,
        action_spec_sha256: input.action_spec_sha256,
        commit_sha: input.commit_sha,
        tree_sha: input.tree_sha,
        result: input.result,
        evidence: input.evidence,
    };
    match app
        .db
        .record_quality_receipt(&input.candidate_id, role, &receipt)
        .await
    {
        Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn create_delegation(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewDelegation>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.delegation.autonomous_start {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"autonomous delegation policy is not ratified"})),
        )
            .into_response();
    }
    match app
        .db
        .create_standing_delegation(&input.delegation, &input.idempotency_key)
        .await
    {
        Ok((delegation, created)) => (
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(delegation),
        )
            .into_response(),
        Err(error) => api_error(error),
    }
}

async fn get_delegation(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.standing_delegation(&id).await {
        Ok(delegation) => match app.db.delegation_usage(&id).await {
            Ok(usage) => Json(json!({"delegation":delegation,"usage":usage})).into_response(),
            Err(error) => api_error(error),
        },
        Err(error) => api_error(error),
    }
}

async fn revoke_delegation(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .revoke_standing_delegation(&id, chrono::Utc::now())
        .await
    {
        Ok(delegation) => Json(delegation).into_response(),
        Err(error) => api_error(error),
    }
}

async fn create_lead_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewLeadRequest>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .create_lead_request(
            &input.project_id,
            &input.action_id,
            &input.delegation_id,
            &input.objective,
            &input.project_config_hash,
            &input.idempotency_key,
        )
        .await
    {
        Ok((request, created)) => (
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(request),
        )
            .into_response(),
        Err(error) => api_error(error),
    }
}

async fn get_lead_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request = match app.db.lead_request(&id).await {
        Ok(request) => request,
        Err(error) => return api_error(error),
    };
    let assessment = app.db.lead_assessment(&id).await.ok();
    let proposal = app.db.lead_proposal(&id).await.ok();
    let plan = app.db.frozen_lead_plan(&id).await.ok();
    Json(json!({
        "request": request,
        "assessment": assessment,
        "proposal": proposal,
        "plan": plan
    }))
    .into_response()
}

async fn authorize_campaign(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<AuthorizeCampaign>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let authorization = NewCampaignAuthorization {
        authorization_id: input.authorization_id,
        risk_class: input.risk_class,
        execution_mode: input.execution_mode,
        actor: input.actor,
        rationale: input.rationale,
        authorized_paths: input.authorized_paths,
        max_attempts: input.max_attempts,
    };
    match app.db.authorize_campaign(&id, &authorization).await {
        Ok((record, created)) => (
            if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            Json(record),
        )
            .into_response(),
        Err(error) => api_error(error),
    }
}

async fn campaign_authorization(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.campaign_authorization(&id).await {
        Ok(record) => Json(record).into_response(),
        Err(error) => api_error(error),
    }
}

async fn create_campaign_run(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let key = format!("campaign-run:{id}");
    match app.db.create_run(&id, &key).await {
        Ok(run) => (StatusCode::CREATED, Json(run)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn create_run_attempt(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let key = format!("run-attempt:{id}");
    match app.db.create_attempt(&id, &key).await {
        Ok(attempt) => (StatusCode::CREATED, Json(attempt)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn candidates(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.candidates().await)
}

async fn quality_candidate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.candidate(&id).await {
        Ok(candidate) => Json(candidate).into_response(),
        Err(error) => api_error(error),
    }
}

async fn quality_receipts(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.quality_receipts(&id).await)
}

async fn quality_gate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.gate_for_candidate(&id).await {
        Ok(Some(gate)) => Json(gate).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => api_error(error),
    }
}

fn quality_role(app: &App, headers: &HeaderMap) -> Option<QualityRole> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))?;
    if constant_time_eq(token.as_bytes(), app.verifier_token.as_bytes()) {
        Some(QualityRole::Verifier)
    } else if constant_time_eq(token.as_bytes(), app.reviewer_token.as_bytes()) {
        Some(QualityRole::Reviewer)
    } else {
        None
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

async fn worker_session_authorized(app: &App, headers: &HeaderMap, worker_id: &str) -> bool {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match token {
        Some(token) => app
            .db
            .worker_session_matches(worker_id, token)
            .await
            .unwrap_or(false),
        None => false,
    }
}
fn result_list<T: serde::Serialize>(result: Result<Vec<T>, Error>) -> Response {
    match result {
        Ok(items) => Json(items).into_response(),
        Err(error) => api_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::Value;

    #[tokio::test]
    async fn worker_session_is_bound_to_registered_json_identity() {
        let path =
            std::env::temp_dir().join(format!("factory-core-auth-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Database::open(&path).await.unwrap();
        db.register_worker_session(
            "worker-a",
            "registration-a",
            "bootstrap-secret-12345678901234567890",
            "session-token-aaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .unwrap();
        let app = App {
            db,
            operator_token: "operator".into(),
            worker_bootstrap_token: "bootstrap".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer session-token-aaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        assert!(worker_session_authorized(&app, &headers, "worker-a").await);
        assert!(!worker_session_authorized(&app, &headers, "worker-b").await);
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer session-token-bbbbbbbbbbbbbbbbbbbbbbbb"),
        );
        assert!(!worker_session_authorized(&app, &headers, "worker-a").await);
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    fn delegation_body(autonomous: bool, project_id: &str) -> Vec<u8> {
        let now = chrono::Utc::now();
        let mut grant = StandingDelegation {
            id: "grant-api".into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: project_id.into(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![factory_native::lead::ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![
                factory_native::lead::Capability::RepositoryEdit,
                factory_native::lead::Capability::GenericToolchain,
            ],
            risk_classes: vec![
                factory_native::lead::RiskClass::R0,
                factory_native::lead::RiskClass::R1,
            ],
            autonomous_start: autonomous,
            autonomous_risk_classes: if autonomous {
                vec![factory_native::lead::RiskClass::R0]
            } else {
                Vec::new()
            },
            route: factory_native::lead::ProviderRoute {
                provider: "opencode".into(),
                model: "test/model".into(),
                mode: "build".into(),
            },
            max_attempts: 2,
            max_timeout_secs: 300,
            max_repair_cycles: 1,
            max_wall_clock_secs: 1_800,
            max_parallel: 1,
            max_sessions: 4,
            max_requests: 2,
            max_cumulative_sessions: 8,
            max_cumulative_attempts: 16,
            expires_at: now + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        serde_json::to_vec(&json!({
            "delegation": grant,
            "idempotency_key": "delegation-api-key"
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn delegation_api_requires_operator_and_rejects_autonomy() {
        use tower::ServiceExt;
        let path = std::env::temp_dir().join(format!(
            "factory-core-delegation-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        let repository = std::env::temp_dir().join(format!(
            "factory-core-delegation-repo-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&repository).unwrap();
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repository)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(repository.join("seed.txt"), "seed\n").unwrap();
        for arguments in [vec!["add", "seed.txt"], vec!["commit", "-qm", "seed"]] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repository)
                    .args(arguments)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let project = db
            .register_project(&repository, "project-key")
            .await
            .unwrap();
        let app = Arc::new(App {
            db,
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
        });
        let router = build_router(app);
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/delegations")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(delegation_body(true, &project.id)))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/delegations")
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(delegation_body(true, &project.id)))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/delegations")
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(delegation_body(false, &project.id)))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::CREATED
        );
        let request = axum::http::Request::builder()
            .uri("/delegations/grant-api")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/actions")
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "project_id": project.id,
                    "objective": "Implement the API slice.",
                    "provider": "opencode",
                    "model": "test/model",
                    "idempotency_key": "action-api-key"
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let action: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/lead/requests")
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "project_id": project.id,
                    "action_id": action["id"],
                    "delegation_id": "grant-api",
                    "objective": "Implement the API slice.",
                    "project_config_hash": "a".repeat(64),
                    "idempotency_key": "lead-api-key"
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let lead: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(lead["state"], "SUBMITTED");
        let request = axum::http::Request::builder()
            .uri(format!("/lead/requests/{}", lead["id"].as_str().unwrap()))
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/delegations/grant-api")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(repository);
    }

    #[tokio::test]
    async fn quality_routes_reject_missing_or_cross_role_credentials() {
        use tower::ServiceExt;
        let path = std::env::temp_dir().join(format!(
            "factory-core-routes-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let app = Arc::new(App {
            db: Database::open(&path).await.unwrap(),
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
        });
        let router = build_router(app);
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/quality/receipts")
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/quality/receipts")
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let request = axum::http::Request::builder()
            .uri("/quality/candidates/missing")
            .header(
                "authorization",
                "Bearer verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn quality_role_requires_exact_distinct_role_tokens() {
        let path = std::env::temp_dir().join(format!(
            "factory-core-quality-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let app = App {
            db: Database::open(&path).await.unwrap(),
            operator_token: "operator".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        assert_eq!(quality_role(&app, &headers), Some(QualityRole::Verifier));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb"),
        );
        assert_eq!(quality_role(&app, &headers), Some(QualityRole::Reviewer));
        headers.insert("authorization", HeaderValue::from_static("Bearer operator"));
        assert_eq!(quality_role(&app, &headers), None);
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer verifier-token-aaaaaaaaaaaaaaaaaaaaaaab"),
        );
        assert_eq!(quality_role(&app, &headers), None);
        drop(app);
        let _ = std::fs::remove_file(path);
    }
}
