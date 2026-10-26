use std::{env, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use factory_native::lead::StandingDelegation;
use factory_native::quality::{EvidenceKind, ReceiptResult};
use factory_native::{
    attempt_grant::AttemptGrantClaims,
    db::{
        Database, Error, NewCampaignAuthorization, NewCandidate, NewIntegratedCandidate,
        NewProjectConfig, QualityReceiptInput, QualityRole, WorkerAssignment, WorkerReport,
    },
};
use serde::Deserialize;
use serde_json::json;

struct App {
    db: Database,
    operator_token: String,
    worker_bootstrap_token: String,
    verifier_token: String,
    reviewer_token: String,
    lead_fixture_assessor: bool,
    lead_autonomous_start: bool,
    model_gateway: Option<factory_native::model_gateway::ModelGateway>,
    attempt_grant_key: Vec<u8>,
    observability: factory_native::observability::ExporterHandle,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewProject {
    repository: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderTurnAuthorize {
    session_record_id: String,
    worker_id: String,
    generation: i64,
    requested_output_tokens: u64,
    request_sha256: String,
    #[serde(default = "default_provider_turn_max_requests")]
    max_requests: u32,
}

fn default_provider_turn_max_requests() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderTurnRelease {
    reservation_id: String,
    worker_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderTurnBind {
    session_record_id: String,
    worker_id: String,
    generation: i64,
    reservation_id: String,
    request_sha256: String,
    model: String,
    path: String,
    body_bytes: u64,
    #[serde(default)]
    sequence: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveProviderTurn {
    actor: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderTurnSettle {
    usage: factory_native::db::ProviderTurnUsage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceOperationValidation {
    grant: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceOperationRequest {
    attempt_id: String,
    worker_id: String,
    generation: i64,
    workspace_id: String,
    operation: String,
    request_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderSessionRef {
    session_record_id: String,
    worker_id: String,
    generation: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderActive {
    session_record_id: String,
    worker_id: String,
    generation: i64,
    provider_session_id: String,
    #[serde(default)]
    provider_directory: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderCleanup {
    session_record_id: String,
    worker_id: String,
    generation: i64,
    cleanup_receipt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelCampaignRequest {
    #[serde(default)]
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassifyAction {
    risk_class: String,
    actor: String,
    rationale: String,
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
    #[serde(default)]
    allowed_paths: Vec<String>,
    #[serde(default)]
    checks: Vec<factory_native::lead::SpecCheck>,
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
    #[serde(default)]
    provider: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    provider_mode: String,
    #[serde(default)]
    delegation_id: String,
    #[serde(default)]
    max_sessions: u32,
    #[serde(default)]
    max_turns: u32,
    #[serde(default)]
    max_input_tokens: u64,
    #[serde(default)]
    max_output_tokens: u64,
    #[serde(default)]
    max_cost_micros: u64,
    #[serde(default)]
    deadline_secs: u64,
    #[serde(default)]
    allowed_tools: Vec<String>,
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
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ResolveModelCall {
    actor: String,
    reason: String,
}

#[derive(Deserialize, Default)]
struct ModelCallListQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModelCallHttpRequest {
    role: factory_native::db::ModelCallRole,
    principal_id: String,
    project_id: String,
    objective_id: String,
    scope_id: String,
    purpose: String,
    system_prompt: String,
    user_prompt: String,
    max_output_tokens: u64,
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
    #[serde(default)]
    findings: Vec<factory_native::review::ReviewFinding>,
    #[serde(default)]
    mechanism: factory_native::db::ReviewMechanism,
    #[serde(default, rename = "modelProvider")]
    model_provider: String,
    #[serde(default, rename = "modelName")]
    model_name: String,
    #[serde(default, rename = "modelReservationId")]
    model_reservation_id: String,
    #[serde(default, rename = "modelRequestSha256")]
    model_request_sha256: String,
    #[serde(default, rename = "inputTokens")]
    input_tokens: u64,
    #[serde(default, rename = "outputTokens")]
    output_tokens: u64,
    #[serde(default, rename = "totalTokens")]
    total_tokens: u64,
    #[serde(default, rename = "costMicros")]
    cost_micros: u64,
    #[serde(default, rename = "usageKnown")]
    usage_known: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("factory_native=info")
        .with_writer(factory_native::telemetry::RedactingMakeWriter)
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
    for token in &tokens {
        factory_native::telemetry::register_secret(token);
    }
    let bind = env::var("FACTORY_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into());
    let attempt_grant_key =
        env::var("FACTORY_ATTEMPT_GRANT_KEY").map_err(|_| "FACTORY_ATTEMPT_GRANT_KEY required")?;
    if attempt_grant_key.len() < factory_native::attempt_grant::MIN_KEY_BYTES {
        return Err("FACTORY_ATTEMPT_GRANT_KEY must be at least 32 bytes".into());
    }
    factory_native::telemetry::register_secret(&attempt_grant_key);
    let container_mode = env::var("FACTORY_CONTAINER_MODE").as_deref() == Ok("1");
    let addr: SocketAddr = validate_bind(&bind, container_mode)?;
    let db = Database::open(Path::new(&database)).await?;
    let observability = match factory_native::observability::Config::from_env() {
        Ok(Some(config)) => match factory_native::observability::start(db.clone(), config) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::warn!(%error, "OTLP export disabled");
                factory_native::observability::ExporterHandle::disabled_with_error(
                    error.to_string(),
                )
            }
        },
        Ok(None) => factory_native::observability::ExporterHandle::disabled(),
        Err(error) => {
            tracing::warn!(%error, "OTLP export disabled");
            factory_native::observability::ExporterHandle::disabled_with_error(error.to_string())
        }
    };
    let app = Arc::new(App {
        db,
        operator_token,
        worker_bootstrap_token,
        verifier_token,
        reviewer_token,
        lead_fixture_assessor: env::var("FACTORY_LEAD_FIXTURE_ASSESSOR").ok().as_deref()
            == Some("1"),
        lead_autonomous_start: env::var("FACTORY_ENABLE_AUTONOMOUS_START").ok().as_deref()
            == Some("1"),
        model_gateway: model_gateway_from_env()?,
        attempt_grant_key: attempt_grant_key.into_bytes(),
        observability,
    });
    let router = build_router(Arc::clone(&app));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "native Factory Core listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    app.observability.shutdown().await;
    app.db.close().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn build_router(app: Arc<App>) -> Router {
    let api = api_router(app);
    let nested = api.clone();
    let mut router = Router::new().merge(api).nest("/core-api", nested);
    if let Ok(directory) = env::var("FACTORY_OFFICE_DIR") {
        let directory = Path::new(&directory);
        if directory.is_dir() {
            let index = directory.join("index.html");
            router = router.fallback_service(
                tower_http::services::ServeDir::new(directory)
                    .not_found_service(tower_http::services::ServeFile::new(index)),
            );
        }
    }
    router
}

fn api_router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"healthy":true,"version":"0.1.0"})) }),
        )
        .route("/projects", get(projects).post(register_project))
        .route(
            "/projects/{id}/config",
            get(project_config).put(update_project_config),
        )
        .route("/actions", get(actions).post(create_action))
        .route("/actions/{id}/classify", post(classify_action))
        .route("/campaigns", get(campaigns).post(create_campaign))
        .route("/campaigns/{id}", get(campaign_detail))
        .route("/campaigns/{id}/cancel", post(cancel_campaign))
        .route("/runs", get(runs))
        .route("/attempts", get(attempts))
        .route("/events", get(events))
        .route("/observability", get(observability_status))
        .route("/worker/register", post(register_worker))
        .route("/worker/claim", post(worker_claim))
        .route("/worker/heartbeat", post(worker_heartbeat))
        .route("/worker/report", post(worker_report))
        .route(
            "/worker/workspace_operation",
            post(worker_workspace_operation),
        )
        .route(
            "/supervisor/workspace_operation_valid",
            post(supervisor_workspace_operation_valid),
        )
        .route("/worker/provider_intent", post(worker_provider_intent))
        .route("/worker/provider_active", post(worker_provider_active))
        .route("/worker/provider_terminal", post(worker_provider_terminal))
        .route("/worker/provider_cleanup", post(worker_provider_cleanup))
        .route(
            "/worker/provider_tool_event",
            post(worker_provider_tool_event),
        )
        .route("/worker/provider_state", post(worker_provider_state))
        .route(
            "/worker/provider_turn_authorize",
            post(worker_provider_turn_authorize),
        )
        .route(
            "/worker/provider_turn_settle",
            post(worker_provider_turn_settle),
        )
        .route(
            "/worker/provider_turn_release",
            post(worker_provider_turn_release),
        )
        .route(
            "/worker/provider_turn_bind",
            post(worker_provider_turn_bind),
        )
        .route(
            "/provider/turn_reservations/{id}/resolve",
            post(resolve_provider_turn),
        )
        .route(
            "/provider/turn_reservations",
            get(provider_turn_reservations),
        )
        .route("/provider/sessions", get(provider_sessions))
        .route(
            "/worker/provider_unresolved",
            get(worker_provider_unresolved),
        )
        .route("/worker/candidate", post(worker_candidate))
        .route(
            "/worker/integrated_candidate",
            post(worker_integrated_candidate),
        )
        .route("/quality/receipts", post(quality_receipt))
        .route("/model/calls", get(model_calls).post(model_call))
        .route("/model/calls/{id}/resolve", post(resolve_model_call))
        .route("/quality/candidates/{id}", get(quality_candidate))
        .route("/quality/candidates/{id}/receipts", get(quality_receipts))
        .route("/quality/candidates/{id}/gate", get(quality_gate))
        .route(
            "/quality/candidates/{id}/inspection",
            get(quality_candidate_inspection),
        )
        .route("/delegations", get(delegations).post(create_delegation))
        .route(
            "/delegations/{id}",
            get(get_delegation).post(revoke_delegation),
        )
        .route(
            "/lead/requests",
            get(lead_requests).post(create_lead_request),
        )
        .route("/lead/requests/{id}", get(get_lead_request))
        .route("/lead/requests/{id}/assess", post(assess_lead_request))
        .route("/lead/requests/{id}/freeze", post(freeze_lead_request))
        .route("/lead/requests/{id}/start", post(start_lead_request))
        .route("/campaigns/{id}/authorize", post(authorize_campaign))
        .route("/campaigns/{id}/authorization", get(campaign_authorization))
        .route("/campaigns/{id}/runs", post(create_campaign_run))
        .route("/runs/{id}/attempts", post(create_run_attempt))
        .route("/candidates", get(candidates))
        .route("/workers", get(workers))
        .with_state(app)
}

fn model_gateway_from_env()
-> Result<Option<factory_native::model_gateway::ModelGateway>, Box<dyn std::error::Error>> {
    if env::var_os("FACTORY_LEAD_MODEL_URL").is_some() {
        return Err(
            "FACTORY_LEAD_MODEL_URL is retired; configure FACTORY_MODEL_URL/TOKEN/NAME".into(),
        );
    }
    let url = match env::var("FACTORY_MODEL_URL") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(error) => return Err(Box::new(error)),
    };
    let token = env::var("FACTORY_MODEL_TOKEN").map_err(|_| "FACTORY_MODEL_TOKEN required")?;
    if token.len() < 16 {
        return Err("FACTORY_MODEL_TOKEN must be at least 16 bytes".into());
    }
    factory_native::telemetry::register_secret(&token);
    let provider = env::var("FACTORY_MODEL_PROVIDER").unwrap_or_else(|_| "opencode-go".into());
    let model = env::var("FACTORY_MODEL_NAME").unwrap_or_else(|_| "deepseek-v4.1-flash".into());
    let timeout_secs = env_u64("FACTORY_MODEL_TIMEOUT_SECS", 60)?;
    let input_micros = env_u64("FACTORY_MODEL_INPUT_MICROS_PER_MILLION", 150_000)?;
    let output_micros = env_u64("FACTORY_MODEL_OUTPUT_MICROS_PER_MILLION", 600_000)?;
    let lead_policy = policy_from_env("FACTORY_LEAD_MODEL", 128_000, 8_192, 300_000)?;
    let reviewer_policy = policy_from_env("FACTORY_REVIEW_MODEL", 400_000, 8_192, 1_000_000)?;
    let evaluation_policy = policy_from_env("FACTORY_EVALUATION_MODEL", 400_000, 8_192, 2_000_000)?;
    let global_policy = policy_from_env("FACTORY_MODEL_GLOBAL", 2_000_000, 100_000, 10_000_000)?;
    let allow_loopback_http = env::var("FACTORY_ALLOW_TEST_UPSTREAM").ok().as_deref() == Some("1");
    Ok(Some(factory_native::model_gateway::ModelGateway::new(
        factory_native::model_gateway::ModelGatewayConfig {
            base_url: url,
            credential: token,
            provider,
            model,
            timeout: std::time::Duration::from_secs(timeout_secs),
            input_micros_per_million: input_micros,
            output_micros_per_million: output_micros,
            lead_policy,
            reviewer_policy,
            evaluation_policy,
            global_policy,
            allow_loopback_http,
        },
    )?))
}

fn env_u64(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be a nonnegative integer").into()),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(Box::new(error)),
    }
}

fn policy_from_env(
    prefix: &str,
    input_default: u64,
    output_default: u64,
    cost_default: u64,
) -> Result<factory_native::db::ModelCallPolicy, Box<dyn std::error::Error>> {
    Ok(factory_native::db::ModelCallPolicy {
        max_input_tokens: env_u64(&format!("{prefix}_MAX_INPUT_TOKENS"), input_default)?,
        max_output_tokens: env_u64(&format!("{prefix}_MAX_OUTPUT_TOKENS"), output_default)?,
        max_cost_micros: env_u64(&format!("{prefix}_MAX_COST_MICROS"), cost_default)?,
    })
}

fn validate_bind(
    raw: &str,
    container_mode: bool,
) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let addr: SocketAddr = raw.parse()?;
    if addr.ip().is_loopback() {
        return Ok(addr);
    }
    if container_mode && addr.ip().is_unspecified() && addr.is_ipv4() {
        tracing::warn!(
            %addr,
            "FACTORY_CONTAINER_MODE=1: binding the container interface; publish only to host loopback"
        );
        return Ok(addr);
    }
    Err("remote binding requires FACTORY_CONTAINER_MODE=1 with an IPv4 unspecified container bind; publish only host loopback".into())
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
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
async fn delegations(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.standing_delegations().await {
        Ok(grants) => Json(grants).into_response(),
        Err(error) => api_error(error),
    }
}

async fn lead_requests(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.lead_requests().await {
        Ok(requests) => Json(requests).into_response(),
        Err(error) => api_error(error),
    }
}

async fn projects(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.projects().await)
}
async fn project_config(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.project_config(&id).await {
        Ok(config) => (StatusCode::OK, Json(config)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn update_project_config(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<NewProjectConfig>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.update_project_config(&id, &input).await {
        Ok(config) => (StatusCode::OK, Json(config)).into_response(),
        Err(error) => api_error(error),
    }
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
        .create_action_with_spec(
            &input.project_id,
            &input.objective,
            &input.provider,
            &input.model,
            &input.idempotency_key,
            factory_native::db::ActionSpecInput {
                allowed_paths: &input.allowed_paths,
                checks: &input.checks,
            },
        )
        .await
    {
        Ok(action) => (StatusCode::CREATED, Json(action)).into_response(),
        Err(error) => api_error(error),
    }
}
async fn classify_action(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<ClassifyAction>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .classify_action(
            &id,
            &input.risk_class,
            &input.actor,
            &input.rationale,
            &input.idempotency_key,
        )
        .await
    {
        Ok(action) => (StatusCode::OK, Json(action)).into_response(),
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
async fn campaign_detail(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let campaign = match app.db.campaign(&id).await {
        Ok(campaign) => campaign,
        Err(error) => return api_error(error),
    };
    let authorization = app.db.campaign_authorization(&id).await.ok();
    let tasks = match app.db.campaign_tasks(&id).await {
        Ok(tasks) => tasks,
        Err(error) => return api_error(error),
    };
    let runs = app
        .db
        .runs()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|run| run.campaign_id == id)
        .collect::<Vec<_>>();
    let attempts = app.db.attempts().await.unwrap_or_default();
    let attempt_ids: std::collections::BTreeSet<String> =
        attempts.iter().map(|attempt| attempt.id.clone()).collect();
    let run_ids: std::collections::BTreeSet<String> =
        runs.iter().map(|run| run.id.clone()).collect();
    let attempts = attempts
        .into_iter()
        .filter(|attempt| run_ids.contains(&attempt.run_id))
        .collect::<Vec<_>>();
    let candidates = app
        .db
        .candidates()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|candidate| attempt_ids.contains(&candidate.attempt_id))
        .collect::<Vec<_>>();
    let events = app
        .db
        .events()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|event| {
            event.subject_id == id
                || event.subject_id.starts_with("lead")
                || tasks
                    .iter()
                    .any(|task| task.campaign_id == event.subject_id)
        })
        .collect::<Vec<_>>();
    Json(json!({
        "campaign": campaign,
        "authorization": authorization,
        "tasks": tasks,
        "runs": runs,
        "attempts": attempts,
        "candidates": candidates,
        "events": events
    }))
    .into_response()
}

async fn cancel_campaign(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let reason = if body.is_empty() {
        String::new()
    } else {
        match serde_json::from_slice::<CancelCampaignRequest>(&body) {
            Ok(input) => input.reason,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"invalid cancellation payload"})),
                )
                    .into_response();
            }
        }
    };
    match app.db.cancel_campaign(&id, "operator", &reason).await {
        Ok(campaign) => Json(campaign).into_response(),
        Err(error) => api_error(error),
    }
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
#[derive(Deserialize, Default)]
struct EventsQuery {
    after: Option<i64>,
    limit: Option<usize>,
}

async fn events(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if query.after.is_none() && query.limit.is_none() {
        return result_list(app.db.events().await);
    }
    let after = query.after.unwrap_or(0);
    if after < 0 {
        return api_error(Error::Invalid("event cursor"));
    }
    let limit = query
        .limit
        .unwrap_or(100)
        .clamp(1, factory_native::db::EVENTS_PAGE_MAX);
    result_list(app.db.events_after(after, limit).await)
}

async fn observability_status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(app.observability.status()).into_response()
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
        Ok(session)=>Json(json!({"worker_id":session.worker_id,"session_token":session_token,"created":session.created,"dispatch_supported":true})).into_response(),
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
        Ok(Some(mut assignment)) => match sign_assignment_grant(&app, &mut assignment) {
            Ok(()) => Json(json!({
                "dispatch_supported": true,
                "assignment": assignment
            }))
            .into_response(),
            Err(error) => api_error(error),
        },
        Ok(None) => Json(json!({
            "dispatch_supported": true,
            "assignment": null
        }))
        .into_response(),
        Err(error) => api_error(error),
    }
}

fn sign_assignment_grant(app: &App, assignment: &mut WorkerAssignment) -> Result<(), Error> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let expires_at_ms = now_ms
        .checked_add(30 * 60 * 1000)
        .ok_or(Error::Invalid("Attempt grant expiry overflow"))?;
    let claims = AttemptGrantClaims {
        attempt_id: assignment.lease.attempt_id.clone(),
        worker_id: assignment.lease.worker_id.clone(),
        generation: assignment.lease.generation,
        repository: assignment.repository.clone(),
        base_commit: assignment.action_spec.base_commit.clone(),
        action_spec_sha256: assignment.action_spec_sha256.clone(),
        expires_at_ms,
    };
    assignment.attempt_grant = factory_native::attempt_grant::sign(&app.attempt_grant_key, &claims)
        .map_err(|_| Error::Invalid("Attempt grant signing failed"))?;
    Ok(())
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
            60_000,
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

async fn supervisor_workspace_operation_valid(
    State(app): State<Arc<App>>,
    Json(input): Json<WorkspaceOperationValidation>,
) -> Response {
    let Ok(claims) = factory_native::attempt_grant::parse_operation(&input.grant) else {
        return StatusCode::CONFLICT.into_response();
    };
    if factory_native::attempt_grant::verify_operation(
        &app.attempt_grant_key,
        &input.grant,
        &claims,
        chrono::Utc::now().timestamp_millis(),
    )
    .is_err()
    {
        return StatusCode::CONFLICT.into_response();
    }
    match app
        .db
        .workspace_operation_authorization(
            &claims.attempt_id,
            &claims.worker_id,
            claims.generation,
            &claims.operation,
        )
        .await
    {
        Ok(authorization)
            if authorization.allowed_paths == claims.allowed_paths
                && authorization.checks == claims.checks =>
        {
            Json(json!({"valid": true})).into_response()
        }
        _ => StatusCode::CONFLICT.into_response(),
    }
}

async fn worker_workspace_operation(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<WorkspaceOperationRequest>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.workspace_id.is_empty()
        || input.workspace_id.len() > 128
        || input.attempt_id.is_empty()
        || input.generation <= 0
        || input.request_sha256.len() != 64
        || !input
            .request_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return api_error(Error::Invalid("workspace operation request"));
    }
    match app
        .db
        .workspace_operation_authorization(
            &input.attempt_id,
            &input.worker_id,
            input.generation,
            &input.operation,
        )
        .await
    {
        Ok(authorization) => {
            let now_ms = chrono::Utc::now().timestamp_millis();
            let expires_at_ms = now_ms.saturating_add(5_000);
            let claims = factory_native::attempt_grant::OperationGrantClaims {
                attempt_id: authorization.attempt_id,
                worker_id: authorization.worker_id,
                generation: authorization.generation,
                workspace_id: input.workspace_id,
                operation: input.operation,
                allowed_paths: authorization.allowed_paths,
                checks: authorization.checks,
                request_sha256: input.request_sha256,
                nonce: uuid::Uuid::new_v4().to_string(),
                expires_at_ms,
            };
            match factory_native::attempt_grant::sign_operation(&app.attempt_grant_key, &claims) {
                Ok(grant) => {
                    Json(json!({"grant": grant, "expires_at_ms": expires_at_ms})).into_response()
                }
                Err(_) => api_error(Error::Invalid("workspace operation grant signing")),
            }
        }
        Err(error) => api_error(error),
    }
}

async fn worker_provider_turn_authorize(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderTurnAuthorize>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .authorize_provider_turn(
            &input.session_record_id,
            &input.worker_id,
            input.generation,
            input.requested_output_tokens,
            &input.request_sha256,
            input.max_requests,
        )
        .await
    {
        Ok(authorization) => (StatusCode::CREATED, Json(authorization)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_turn_release(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderTurnRelease>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .release_provider_turn(&input.reservation_id, "worker released turn")
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_turn_bind(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderTurnBind>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .bind_provider_turn(&factory_native::db::ProviderTurnBindingInput {
            reservation_id: input.reservation_id,
            session_record_id: input.session_record_id,
            worker_id: input.worker_id,
            generation: input.generation,
            request_sha256: input.request_sha256,
            model: input.model,
            path: input.path,
            body_bytes: input.body_bytes,
            sequence: input.sequence,
        })
        .await
    {
        Ok(binding) => (StatusCode::OK, Json(binding)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn provider_turn_reservations(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.provider_turn_reservations(200).await)
}

async fn resolve_provider_turn(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<ResolveProviderTurn>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .resolve_provider_turn_reservation(&id, &input.actor, &input.reason)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_turn_settle(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderTurnSettle>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.usage.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.settle_provider_turn(&input.usage).await {
        Ok(usage) => Json(usage).into_response(),
        Err(error) => api_error(error),
    }
}

async fn provider_sessions(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.provider_sessions().await)
}

async fn worker_provider_intent(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<factory_native::db::NewProviderIntent>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.record_provider_intent(&input).await {
        Ok(record) => (StatusCode::CREATED, Json(record)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_active(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderActive>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .mark_provider_active(
            &input.session_record_id,
            &input.worker_id,
            input.generation,
            &input.provider_session_id,
            &input.provider_directory,
        )
        .await
    {
        Ok(record) => (StatusCode::OK, Json(record)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_tool_event(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<factory_native::db::ProviderToolEventInput>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.record_provider_tool_event(&input).await {
        Ok(record) => (StatusCode::CREATED, Json(record)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_cleanup(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderCleanup>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .record_provider_cleanup(
            &input.session_record_id,
            &input.worker_id,
            input.generation,
            &input.cleanup_receipt,
        )
        .await
    {
        Ok(record) => (StatusCode::OK, Json(record)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_state(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ProviderSessionRef>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.provider_session(&input.session_record_id).await {
        Ok(record)
            if record.worker_id == input.worker_id && record.generation == input.generation =>
        {
            Json(record).into_response()
        }
        Ok(_) => StatusCode::FORBIDDEN.into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_unresolved(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(worker_id) = query.get("worker_id") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !worker_session_authorized(&app, &headers, worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.unresolved_provider_sessions().await)
}

async fn worker_provider_terminal(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<factory_native::db::ProviderTerminal>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.record_provider_terminal(&input).await {
        Ok(record) => (StatusCode::OK, Json(record)).into_response(),
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

async fn worker_integrated_candidate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewIntegratedCandidate>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &input.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .record_integrated_candidate(&input, chrono::Utc::now().timestamp_millis())
        .await
    {
        Ok(candidate) => (StatusCode::CREATED, Json(candidate)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn model_calls(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<ModelCallListQuery>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    result_list(app.db.model_calls(limit).await)
}

async fn resolve_model_call(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<ResolveModelCall>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .resolve_ambiguous_model_call(&id, &input.actor, &input.reason)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => api_error(error),
    }
}

async fn model_call(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ModelCallHttpRequest>,
) -> Response {
    let role = input.role;
    let authenticated = match role {
        factory_native::db::ModelCallRole::SemanticReviewer => {
            quality_role(&app, &headers) == Some(QualityRole::Reviewer)
        }
        factory_native::db::ModelCallRole::Evaluation => authorized(&headers, &app.operator_token),
        factory_native::db::ModelCallRole::Lead => false,
    };
    if !authenticated {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if matches!(
        role,
        factory_native::db::ModelCallRole::SemanticReviewer
            | factory_native::db::ModelCallRole::Evaluation
    ) && input.scope_id != input.objective_id
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"model call scope must identify the objective"})),
        )
            .into_response();
    }
    let Some(gateway) = &app.model_gateway else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"model gateway is not configured"})),
        )
            .into_response();
    };
    match gateway
        .call(
            &app.db,
            factory_native::model_gateway::ModelCallRequest {
                role,
                principal_id: &input.principal_id,
                project_id: &input.project_id,
                objective_id: &input.objective_id,
                scope_id: &input.scope_id,
                purpose: &input.purpose,
                system_prompt: &input.system_prompt,
                user_prompt: &input.user_prompt,
                max_output_tokens: input.max_output_tokens,
            },
        )
        .await
    {
        Ok(result) => Json(json!({
            "reservationId": result.reservation_id,
            "provider": result.provider,
            "model": result.model,
            "requestSha256": result.request_sha256,
            "content": result.content,
            "inputTokens": result.input_tokens,
            "outputTokens": result.output_tokens,
            "totalTokens": result.total_tokens,
            "costMicros": result.cost_micros,
            "usageKnown": result.usage_known
        }))
        .into_response(),
        Err(factory_native::model_gateway::ModelGatewayError::Invalid(message)) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
        }
        Err(factory_native::model_gateway::ModelGatewayError::Database(error)) => api_error(error),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
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
        findings: input.findings,
        mechanism: input.mechanism,
        model_provider: input.model_provider,
        model_name: input.model_name,
        model_reservation_id: input.model_reservation_id,
        model_request_sha256: input.model_request_sha256,
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        total_tokens: input.total_tokens,
        cost_micros: input.cost_micros,
        usage_known: input.usage_known,
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
    let mut delegation = input.delegation;
    if delegation.content_hash.is_empty() {
        delegation.content_hash = delegation.compute_content_hash();
    }
    if delegation.autonomous_start && !app.lead_autonomous_start {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"autonomous delegation policy is not ratified or activated"})),
        )
            .into_response();
    }
    match app
        .db
        .create_standing_delegation(&delegation, &input.idempotency_key)
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

async fn assess_lead_request(
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
    let config = match app.db.project_config(&request.project_id).await {
        Ok(config) => config,
        Err(error) => return api_error(error),
    };
    let action = match app.db.action(&request.action_id).await {
        Ok(action) => action,
        Err(error) => return api_error(error),
    };
    let grant = match app.db.standing_delegation(&request.delegation_id).await {
        Ok(grant) => grant,
        Err(error) => return api_error(error),
    };
    if app.model_gateway.is_none() && !app.lead_fixture_assessor {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({"error":"no model gateway is authorized; configure FACTORY_MODEL_URL or set FACTORY_LEAD_FIXTURE_ASSESSOR=1 only for deterministic tests"})),
        )
            .into_response();
    }
    if request.state == factory_native::lead::LeadRequestState::Submitted
        && let Err(error) = app.db.begin_lead_assessment(&id).await
    {
        return api_error(error);
    }
    let action_binding = factory_native::lead::ActionBinding {
        id: action.id.clone(),
        project_id: action.project_id.clone(),
        status: factory_native::lead::ActionStatus::Pending,
    };
    let mechanism = if app.model_gateway.is_some() {
        "MODEL"
    } else {
        "FIXTURE"
    };
    let (mut assessment, mut proposal) = if let Some(gateway) = &app.model_gateway {
        match factory_native::lead_model::assess(
            gateway,
            &app.db,
            factory_native::lead_model::AssessmentRequest {
                request_id: &request.id,
                project: &config,
                action: &action_binding,
                action_spec: &action.spec,
                objective: &request.objective,
                grant: &grant,
                now: chrono::Utc::now(),
            },
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                if matches!(
                    error,
                    factory_native::lead::LeadError::TransientAssessment(_)
                ) {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error": error.to_string()})),
                    )
                        .into_response();
                }
                let _ = app.db.block_lead_request(&id, &error.to_string()).await;
                return api_error(error.into());
            }
        }
    } else {
        match factory_native::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &action_binding,
            &action.spec,
            &request.objective,
            &grant,
            chrono::Utc::now(),
        ) {
            Ok(value) => value,
            Err(error) => {
                let _ = app.db.block_lead_request(&id, &error.to_string()).await;
                return api_error(error.into());
            }
        }
    };
    let requested_units = if app.lead_fixture_assessor {
        env::var("FACTORY_LEAD_FIXTURE_UNITS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 8)
    } else {
        1
    };
    if requested_units > 1 {
        let fanin = env::var("FACTORY_LEAD_FIXTURE_MODE").ok().as_deref() == Some("fanin");
        proposal.budget.max_attempts = 1;
        proposal.max_sessions = (1 + requested_units) as u32;
        proposal.work_units = (1..=requested_units)
            .map(|index| factory_native::lead::LeadWorkUnit {
                id: format!("unit-{index}"),
                kind: factory_native::lead::WorkUnitKind::Implementation,
                description: format!("Unit {index}"),
                depends_on: if fanin && index > 2 {
                    vec!["unit-1".into(), "unit-2".into()]
                } else {
                    Vec::new()
                },
                allowed_paths: vec![format!("src/unit-{index}.txt")],
                capabilities: config.capabilities.clone(),
                acceptance_criteria: Vec::new(),
                negative_cases: Vec::new(),
            })
            .collect();
        assessment.proposal = Some(proposal.clone());
    }
    let input = factory_native::lead::AssessmentInput {
        project_id: request.project_id.clone(),
        action_id: request.action_id.clone(),
        request_id: request.id.clone(),
        objective: request.objective.clone(),
        allowed_evidence_refs: std::collections::BTreeSet::from(["repository-files".to_string()]),
    };
    let assessment_record = match app
        .db
        .record_lead_assessment(
            &id,
            mechanism,
            &assessment,
            &input,
            &format!("{mechanism}-assessment:{id}"),
        )
        .await
    {
        Ok(record) => record,
        Err(error) => return api_error(error),
    };
    let proposal_record = match app
        .db
        .record_lead_proposal(&id, &proposal, &format!("{mechanism}-proposal:{id}"))
        .await
    {
        Ok(record) => record,
        Err(error) => return api_error(error),
    };
    (
        StatusCode::OK,
        Json(json!({
            "assessment": assessment_record,
            "proposal": proposal_record
        })),
    )
        .into_response()
}

async fn freeze_lead_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let plan = match app.db.freeze_lead_plan(&id, &format!("freeze:{id}")).await {
        Ok(plan) => plan,
        Err(error) => return api_error(error),
    };
    let request = match app.db.mark_lead_ready(&id).await {
        Ok(request) => request,
        Err(error) => return api_error(error),
    };
    (
        StatusCode::OK,
        Json(json!({"plan": plan, "request": request})),
    )
        .into_response()
}

async fn start_lead_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !app.lead_autonomous_start {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"autonomous start policy is not activated"})),
        )
            .into_response();
    }
    match app
        .db
        .start_lead_campaign(&id, &format!("start:{id}"))
        .await
    {
        Ok((campaign, run, attempt)) => (
            StatusCode::CREATED,
            Json(json!({"campaign":campaign,"run":run,"attempt":attempt})),
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
        provider: input.provider,
        model: input.model,
        provider_mode: input.provider_mode,
        delegation_id: input.delegation_id,
        max_sessions: input.max_sessions,
        max_turns: input.max_turns,
        max_input_tokens: input.max_input_tokens,
        max_output_tokens: input.max_output_tokens,
        max_cost_micros: input.max_cost_micros,
        deadline_secs: input.deadline_secs,
        allowed_tools: input.allowed_tools,
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

async fn workers(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.workers().await)
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
    if !authorized(&headers, &app.operator_token) && quality_role(&app, &headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.quality_receipts(&id).await)
}

async fn quality_candidate_inspection(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) && quality_role(&app, &headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.candidate_inspection(&id).await {
        Ok(inspection) => Json(inspection).into_response(),
        Err(error) => api_error(error),
    }
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

    #[test]
    fn bind_mode_requires_explicit_container_opt_in() {
        assert!(validate_bind("127.0.0.1:8765", false).is_ok());
        assert!(validate_bind("0.0.0.0:8765", false).is_err());
        assert!(validate_bind("0.0.0.0:8765", true).is_ok());
        assert!(validate_bind("[::]:8765", true).is_err());
        assert!(validate_bind("203.0.113.4:8765", true).is_err());
    }

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
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
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
                model: "deepseek-v4.1-flash".into(),
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
        let config_hash = db.project_config(&project.id).await.unwrap().config_hash;
        let app = Arc::new(App {
            db,
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
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
                    "model": "deepseek-v4.1-flash",
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
                    "project_config_hash": config_hash,
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
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
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
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
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
