use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Extension, Path as AxumPath, Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
};
use factory_native::lead::{LeadRequestState, StandingDelegation};
use factory_native::quality::{EvidenceKind, GateOutcome, ReceiptResult};
use factory_native::{
    attempt_grant::AttemptGrantClaims,
    db::{
        Database, Error, NewCampaignAuthorization, NewCandidate, NewIntegratedCandidate,
        NewProjectConfig, QualityReceiptInput, QualityRole, WorkerAssignment, WorkerReport,
    },
};
use serde::Deserialize;
use serde_json::json;

struct SetupState {
    failures: u32,
    blocked_until_ms: i64,
}

struct App {
    db: Database,
    operator_token: String,
    enrollment_code_file: Option<PathBuf>,
    projects_root: Option<PathBuf>,
    allow_insecure_git: bool,
    setup: Mutex<SetupState>,
    worker_bootstrap_token: String,
    verifier_token: String,
    reviewer_token: String,
    lead_fixture_assessor: bool,
    lead_autonomous_start: bool,
    model_gateway: Option<factory_native::model_gateway::ModelGateway>,
    master_key: Option<factory_native::provider_secrets::ProviderSecretKey>,
    attempt_grant_key: Vec<u8>,
    observability: factory_native::observability::ExporterHandle,
}

const SESSION_COOKIE: &str = "factory_session";
const CSRF_COOKIE: &str = "factory_csrf";
const CSRF_HEADER: &str = "x-factory-csrf";

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
    session_record_id: String,
    worker_id: String,
    generation: i64,
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

impl AuthorizeCampaign {
    fn into_new(self) -> NewCampaignAuthorization {
        NewCampaignAuthorization {
            authorization_id: self.authorization_id,
            risk_class: self.risk_class,
            execution_mode: self.execution_mode,
            actor: self.actor,
            rationale: self.rationale,
            authorized_paths: self.authorized_paths,
            max_attempts: self.max_attempts,
            provider: self.provider,
            model: self.model,
            provider_mode: self.provider_mode,
            delegation_id: self.delegation_id,
            max_sessions: self.max_sessions,
            max_turns: self.max_turns,
            max_input_tokens: self.max_input_tokens,
            max_output_tokens: self.max_output_tokens,
            max_cost_micros: self.max_cost_micros,
            deadline_secs: self.deadline_secs,
            allowed_tools: self.allowed_tools,
        }
    }
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
struct ApproveLeadRequest {
    expected_request_hash: String,
    expected_plan_hash: String,
    expected_spec_sha256: String,
    expected_config_hash: String,
    expected_delegation_hash: String,
    #[serde(default)]
    profile_revision_id: Option<String>,
    #[serde(default)]
    profile_hash: Option<String>,
    #[serde(default)]
    principal_label: Option<String>,
    rationale: String,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    provider_authorization: Option<AuthorizeCampaign>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WithdrawLeadRequest {
    reason: String,
    #[serde(default)]
    principal_label: Option<String>,
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
    match env::args().nth(1).as_deref() {
        Some("issue-bootstrap-code") => return issue_bootstrap_code(false).await,
        Some("issue-recovery-code") => return issue_bootstrap_code(true).await,
        Some(_) => return Err("unknown factory-core command".into()),
        None => {}
    }
    tracing_subscriber::fmt()
        .with_env_filter("factory_native=info")
        .with_writer(factory_native::telemetry::RedactingMakeWriter)
        .init();
    let database = env::var("FACTORY_DATABASE").unwrap_or_else(|_| "factory-native.sqlite".into());
    let operator_token = factory_native::secrets::required("FACTORY_OPERATOR_TOKEN")?;
    let worker_bootstrap_token =
        factory_native::secrets::required("FACTORY_WORKER_BOOTSTRAP_TOKEN")?;
    let verifier_token = factory_native::secrets::required("FACTORY_VERIFIER_TOKEN")?;
    let reviewer_token = factory_native::secrets::required("FACTORY_REVIEWER_TOKEN")?;
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
    let attempt_grant_key = factory_native::secrets::required("FACTORY_ATTEMPT_GRANT_KEY")?;
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
    let enrollment_code_file = env::var("FACTORY_ENROLLMENT_CODE_FILE")
        .ok()
        .map(PathBuf::from);
    let projects_root = env::var("FACTORY_PROJECTS_ROOT").ok().map(PathBuf::from);
    if projects_root
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err("FACTORY_PROJECTS_ROOT must be absolute".into());
    }
    let allow_insecure_git = env::var("FACTORY_ALLOW_INSECURE_GIT").as_deref() == Ok("1");
    let master_key = match env::var("FACTORY_MASTER_KEY_FILE") {
        Ok(path) => {
            Some(factory_native::provider_secrets::ProviderSecretKey::from_file(Path::new(&path))?)
        }
        Err(_) => None,
    };
    let app = Arc::new(App {
        db,
        operator_token,
        enrollment_code_file,
        projects_root,
        allow_insecure_git,
        setup: Mutex::new(SetupState {
            failures: 0,
            blocked_until_ms: 0,
        }),
        worker_bootstrap_token,
        verifier_token,
        reviewer_token,
        lead_fixture_assessor: env::var("FACTORY_LEAD_FIXTURE_ASSESSOR").ok().as_deref()
            == Some("1"),
        lead_autonomous_start: env::var("FACTORY_ENABLE_AUTONOMOUS_START").ok().as_deref()
            == Some("1"),
        model_gateway: model_gateway_from_env()?,
        master_key,
        attempt_grant_key: attempt_grant_key.into_bytes(),
        observability,
    });
    let router = build_router(Arc::clone(&app));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "native Factory Core listening");
    let office_access_bind = env::var("FACTORY_OFFICE_ACCESS_BIND").ok();
    if let Some(bind) = office_access_bind {
        let office_addr: SocketAddr = validate_bind(&bind, container_mode)?;
        let office_listener = tokio::net::TcpListener::bind(office_addr).await?;
        let office_router = build_office_access_router(
            Arc::clone(&app),
            env::var("FACTORY_OFFICE_DIR").ok().map(PathBuf::from),
        );
        tracing::info!(%office_addr, "native Factory Office access listener listening");
        tokio::select! {
            result = axum::serve(listener, router).with_graceful_shutdown(shutdown_signal()) => result?,
            result = axum::serve(office_listener, office_router).with_graceful_shutdown(shutdown_signal()) => result?,
        }
    } else {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
    }
    app.observability.shutdown().await;
    app.db.close().await;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BootstrapCodeRecord {
    code_sha256: String,
    expires_at_ms: i64,
}

async fn issue_bootstrap_code(recovery: bool) -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};

    let database = env::var("FACTORY_DATABASE").unwrap_or_else(|_| "factory-native.sqlite".into());
    let db = Database::open(Path::new(&database)).await?;
    let owner_available = db.owner_bootstrap_available().await?;
    if !owner_available && !recovery {
        println!("OWNER_ALREADY_ENROLLED");
        db.close().await;
        return Ok(());
    }
    let path = PathBuf::from(
        env::var("FACTORY_ENROLLMENT_CODE_FILE").unwrap_or_else(|_| "/data/enrollment-code".into()),
    );
    let code = uuid::Uuid::new_v4().simple().to_string();
    let expires_at_ms = chrono::Utc::now().timestamp_millis() + 5 * 60 * 1000;
    let record = BootstrapCodeRecord {
        code_sha256: factory_native::device_session::sha256_hex(&code),
        expires_at_ms,
    };
    let serialized = serde_json::to_vec(&record)?;
    let temporary = path.with_extension(format!(
        "bootstrap-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&serialized)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, &path)?;
    let used_marker = path.with_file_name("enrollment-used");
    if used_marker.exists() {
        std::fs::remove_file(used_marker)?;
    }
    if recovery {
        db.reset_owner_bootstrap("local-recovery", chrono::Utc::now().timestamp_millis())
            .await?;
    }
    println!("{code}");
    db.close().await;
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
    let api =
        api_router(Arc::clone(&app)).merge(factory_native::device_pairing::routes(app.db.clone()));
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
    router.layer(from_fn_with_state(app, browser_auth))
}

fn build_office_access_router(app: Arc<App>, office_directory: Option<PathBuf>) -> Router {
    let api = office_api_router(Arc::clone(&app));
    let nested = api.clone();
    let mut router = Router::new().merge(api).nest("/core-api", nested);
    if let Some(directory) = office_directory.filter(|directory| directory.is_dir()) {
        let index = directory.join("index.html");
        router = router.fallback_service(
            tower_http::services::ServeDir::new(directory)
                .not_found_service(tower_http::services::ServeFile::new(index)),
        );
    } else {
        router = router.fallback(office_api_route_not_found);
    }
    router.layer(from_fn_with_state(app, browser_auth))
}

fn office_api_router(app: Arc<App>) -> Router {
    let api = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"healthy":true,"version":"0.1.0"})) }),
        )
        .route("/auth/bootstrap", post(auth_bootstrap))
        .route("/auth/session", get(auth_session))
        .route("/auth/logout", post(auth_logout))
        .route("/auth/logout-all", post(auth_logout_all))
        .route("/auth/rotate", post(auth_rotate))
        .route("/setup/status", get(setup_status))
        .route("/settings/providers", get(provider_settings_list))
        .route(
            "/settings/runtime",
            get(runtime_settings).put(update_runtime_settings),
        )
        .route(
            "/settings/providers/{provider}",
            axum::routing::put(update_provider_settings),
        )
        .route(
            "/settings/providers/{provider}/credential",
            delete(revoke_provider_credential),
        )
        .route(
            "/settings/providers/{provider}/test",
            post(test_provider_connection),
        )
        .route("/settings/providers/{provider}/audit", get(provider_audit))
        .route("/projects", get(projects).post(register_project))
        .route(
            "/projects/{id}/config",
            get(project_config).put(update_project_config),
        )
        .route("/actions", get(actions).post(create_action))
        .route("/office/overview", get(office_overview))
        .route("/office/decisions", get(office_decisions))
        .route("/office/participants", get(office_participants))
        .route(
            "/provider/sessions/{session_record_id}/tool_events",
            get(provider_session_tool_events),
        )
        .route("/campaigns", get(campaigns).post(create_campaign))
        .route("/campaigns/{id}", get(campaign_detail))
        .route(
            "/campaigns/{id}/candidates/{candidate_id}/diff",
            get(candidate_diff),
        )
        .route("/campaigns/{id}/cancel", post(cancel_campaign))
        .route("/campaigns/{id}/authorization", get(campaign_authorization))
        .route("/campaigns/{id}/authorize", post(authorize_campaign))
        .route("/campaigns/{id}/runs", post(create_campaign_run))
        .route("/runs", get(runs))
        .route("/runs/{id}/attempts", post(create_run_attempt))
        .route("/attempts", get(attempts))
        .route("/events", get(events))
        .route("/observability", get(observability_status))
        .route(
            "/provider/turn_reservations/budget",
            get(provider_turn_reservation_budget),
        )
        .route("/provider/sessions", get(provider_sessions))
        .route("/model/calls/unresolved", get(unresolved_model_calls))
        .route("/model/calls", get(model_calls))
        .route("/model/calls/{id}/resolve", post(resolve_model_call))
        .route("/quality/candidates/{id}/receipts", get(quality_receipts))
        .route("/quality/candidates/{id}/gate", get(quality_gate))
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
        .route("/lead/requests/{id}/approve", post(approve_lead_request))
        .route("/lead/requests/{id}/withdraw", post(withdraw_lead_request))
        .route("/candidates", get(candidates))
        .route("/workers", get(workers))
        .with_state(Arc::clone(&app))
        .merge(factory_native::device_pairing::routes(app.db.clone()));

    api.route("/worker", any(office_route_not_found))
        .route("/worker/{*path}", any(office_route_not_found))
        .route("/review", any(office_route_not_found))
        .route("/review/{*path}", any(office_route_not_found))
        .route("/supervisor", any(office_route_not_found))
        .route("/supervisor/{*path}", any(office_route_not_found))
        .route("/quality/receipts", any(office_route_not_found))
}

async fn office_route_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn office_api_route_not_found() -> Response {
    let mut response = StatusCode::NOT_FOUND.into_response();
    response.headers_mut().insert(
        "x-factory-office-route-miss",
        axum::http::HeaderValue::from_static("true"),
    );
    response
}

fn api_router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"healthy":true,"version":"0.1.0"})) }),
        )
        .route("/auth/bootstrap", post(auth_bootstrap))
        .route("/auth/session", get(auth_session))
        .route("/auth/logout", post(auth_logout))
        .route("/auth/logout-all", post(auth_logout_all))
        .route("/auth/rotate", post(auth_rotate))
        .route("/setup/status", get(setup_status))
        .route("/quality/pending", get(pending_quality))
        .route("/settings/providers", get(provider_settings_list))
        .route(
            "/settings/runtime",
            get(runtime_settings).put(update_runtime_settings),
        )
        .route("/review/config", get(review_config))
        .route(
            "/settings/providers/{provider}",
            axum::routing::put(update_provider_settings),
        )
        .route(
            "/settings/providers/{provider}/credential",
            delete(revoke_provider_credential),
        )
        .route(
            "/settings/providers/{provider}/test",
            post(test_provider_connection),
        )
        .route("/settings/providers/{provider}/audit", get(provider_audit))
        .route("/worker/provider_profile", get(worker_provider_profile))
        .route("/projects", get(projects).post(register_project))
        .route(
            "/projects/{id}/config",
            get(project_config).put(update_project_config),
        )
        .route("/actions", get(actions).post(create_action))
        .route("/actions/{id}/classify", post(classify_action))
        .route("/office/overview", get(office_overview))
        .route("/office/decisions", get(office_decisions))
        .route("/office/participants", get(office_participants))
        .route(
            "/provider/sessions/{session_record_id}/tool_events",
            get(provider_session_tool_events),
        )
        .route("/campaigns", get(campaigns).post(create_campaign))
        .route("/campaigns/{id}", get(campaign_detail))
        .route(
            "/campaigns/{id}/candidates/{candidate_id}/diff",
            get(candidate_diff),
        )
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
            "/provider/turn_reservations/budget",
            get(provider_turn_reservation_budget),
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
        .route("/model/calls/unresolved", get(unresolved_model_calls))
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
        .route("/lead/requests/{id}/approve", post(approve_lead_request))
        .route("/lead/requests/{id}/withdraw", post(withdraw_lead_request))
        .route("/campaigns/{id}/authorize", post(authorize_campaign))
        .route("/campaigns/{id}/authorization", get(campaign_authorization))
        .route("/campaigns/{id}/runs", post(create_campaign_run))
        .route("/runs/{id}/attempts", post(create_run_attempt))
        .route("/candidates", get(candidates))
        .route("/workers", get(workers))
        .with_state(app)
}

impl App {
    async fn current_model_gateway(&self) -> Option<factory_native::model_gateway::ModelGateway> {
        if let Some(gateway) = &self.model_gateway {
            return Some(gateway.clone());
        }
        let master_key = self.master_key.as_ref()?;
        let settings = match self.db.default_provider_settings().await {
            Ok(settings) => settings,
            Err(factory_native::db::Error::NotFound) => return None,
            Err(error) => {
                tracing::warn!(%error, "provider settings could not be loaded");
                return None;
            }
        };
        if !settings.enabled || settings.kind != "opencode" {
            return None;
        }
        let credential =
            match master_key.decrypt(&settings.credential_ciphertext, &settings.credential_nonce) {
                Ok(credential) => String::from_utf8(credential).ok()?,
                Err(error) => {
                    tracing::warn!(%error, "provider credential could not be decrypted");
                    return None;
                }
            };
        factory_native::telemetry::register_secret(&credential);
        let allow_loopback_http =
            env::var("FACTORY_ALLOW_TEST_UPSTREAM").ok().as_deref() == Some("1");
        let mut lead_policy =
            policy_from_env("FACTORY_LEAD_MODEL", 128_000, 8_192, 300_000).ok()?;
        let mut reviewer_policy =
            policy_from_env("FACTORY_REVIEW_MODEL", 400_000, 8_192, 1_000_000).ok()?;
        let mut evaluation_policy =
            policy_from_env("FACTORY_EVALUATION_MODEL", 400_000, 8_192, 2_000_000).ok()?;
        let mut global_policy =
            policy_from_env("FACTORY_MODEL_GLOBAL", 2_000_000, 100_000, 10_000_000).ok()?;
        for policy in [
            &mut lead_policy,
            &mut reviewer_policy,
            &mut evaluation_policy,
            &mut global_policy,
        ] {
            policy.max_input_tokens = policy.max_input_tokens.min(settings.max_input_tokens);
            policy.max_output_tokens = policy.max_output_tokens.min(settings.max_output_tokens);
            policy.max_cost_micros = policy.max_cost_micros.min(settings.max_cost_micros);
        }
        let config = factory_native::model_gateway::ModelGatewayConfig {
            base_url: settings.base_url,
            credential,
            provider: settings.provider,
            model: settings.model,
            timeout: std::time::Duration::from_secs(
                env_u64("FACTORY_MODEL_TIMEOUT_SECS", 60).ok()?,
            ),
            input_micros_per_million: settings.input_micros_per_million,
            output_micros_per_million: settings.output_micros_per_million,
            lead_policy,
            reviewer_policy,
            evaluation_policy,
            global_policy,
            allow_loopback_http,
        };
        match factory_native::model_gateway::ModelGateway::new(config) {
            Ok(gateway) => Some(gateway),
            Err(error) => {
                tracing::warn!(%error, "provider settings did not produce a valid model gateway");
                None
            }
        }
    }
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
    let token = factory_native::secrets::required("FACTORY_MODEL_TOKEN")?;
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

/// Core-owned identity for an authenticated operator request. A browser device
/// session is a distinct principal; the shared bearer token is a single local
/// operator principal. A display label is audit metadata only.
fn authenticated_operator(
    session: Option<&factory_native::db::DeviceSessionRecord>,
) -> (String, Option<String>) {
    match session {
        Some(device) => (format!("device:{}", device.id), Some(device.name.clone())),
        None => ("operator-token".into(), None),
    }
}

async fn browser_auth(State(app): State<Arc<App>>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let session_token = match cookie_value(request.headers(), SESSION_COOKIE) {
        Ok(token) => token,
        Err(()) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let has_origin = request.headers().contains_key(header::ORIGIN);
    let auth_route = path == "/auth"
        || path.starts_with("/auth/")
        || path == "/core-api/auth"
        || path.starts_with("/core-api/auth/");
    let trusted = trusted_origins();
    let secure_cookie =
        factory_native::device_session::secure_cookie_context(request.headers(), &trusted);
    {
        let test_host_absent = cfg!(test) && !request.headers().contains_key(header::HOST);
        if !test_host_absent && !request_host_allowed(request.headers(), &trusted) {
            return StatusCode::BAD_REQUEST.into_response();
        }
        if let Some(origin) = request.headers().get(header::ORIGIN) {
            let Ok(origin) = origin.to_str() else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if !origin_allowed(origin, request.headers(), &trusted) {
                return StatusCode::FORBIDDEN.into_response();
            }
        } else if !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        ) && (auth_route || session_token.is_some())
        {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    let had_session = session_token.is_some();
    let mut discard_invalid_session_cookie = false;
    if let Some(token) = session_token {
        if request.headers().contains_key(header::AUTHORIZATION) {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let session = match app
            .db
            .device_session_by_token_hash(
                &factory_native::device_session::sha256_hex(&token),
                chrono::Utc::now().timestamp_millis(),
            )
            .await
        {
            Ok(Some(session)) => Some(session),
            Ok(None)
                if matches!(
                    *request.method(),
                    Method::GET | Method::HEAD | Method::OPTIONS
                ) =>
            {
                // A stale or revoked cookie must not block the Office shell or
                // public reads. Protected handlers still reject without an
                // injected operator credential, and the browser is told to
                // discard the invalid cookie so the app can show its gate.
                discard_invalid_session_cookie = true;
                None
            }
            Ok(None)
                if *request.method() == Method::POST
                    && matches!(path, "/auth/bootstrap" | "/core-api/auth/bootstrap") =>
            {
                None
            }
            Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
            Err(error) => return api_error(error),
        };
        if let Some(session) = session {
            if !matches!(
                *request.method(),
                Method::GET | Method::HEAD | Method::OPTIONS
            ) {
                let csrf_cookie = match cookie_value(request.headers(), CSRF_COOKIE) {
                    Ok(Some(value)) => value,
                    _ => return StatusCode::FORBIDDEN.into_response(),
                };
                let csrf_header = request
                    .headers()
                    .get(CSRF_HEADER)
                    .and_then(|value| value.to_str().ok());
                if csrf_header != Some(csrf_cookie.as_str()) {
                    return StatusCode::FORBIDDEN.into_response();
                }
                let matches = app
                    .db
                    .device_session_csrf_matches(
                        &session.id,
                        &factory_native::device_session::sha256_hex(&csrf_cookie),
                        chrono::Utc::now().timestamp_millis(),
                    )
                    .await;
                match matches {
                    Ok(true) => {}
                    Ok(false) => return StatusCode::FORBIDDEN.into_response(),
                    Err(error) => return api_error(error),
                }
            }
            if let Err(error) = app
                .db
                .touch_device_session(&session.id, chrono::Utc::now().timestamp_millis())
                .await
            {
                return api_error(error);
            }
            let authorization = match axum::http::HeaderValue::from_str(&format!(
                "Bearer {}",
                app.operator_token
            )) {
                Ok(value) => value,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            request
                .headers_mut()
                .insert(header::AUTHORIZATION, authorization);
            request.extensions_mut().insert(session);
        }
    }
    let mut response = next.run(request).await;
    if discard_invalid_session_cookie {
        factory_native::device_session::clear_cookies(&mut response, secure_cookie);
    }
    if auth_route || has_origin || had_session {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        );
        response.headers_mut().insert(
            header::REFERRER_POLICY,
            axum::http::HeaderValue::from_static("no-referrer"),
        );
    }
    response
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Result<Option<String>, ()> {
    let mut found = None;
    for header_value in headers.get_all(header::COOKIE) {
        let Ok(raw) = header_value.to_str() else {
            return Err(());
        };
        for part in raw.split(';') {
            let Some((key, value)) = part.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if found.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(());
                }
                found = Some(value.to_owned());
            }
        }
    }
    Ok(found)
}

fn trusted_origins() -> Vec<String> {
    env::var("FACTORY_TRUSTED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(str::to_owned)
        .collect()
}

fn loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]"
}

fn request_host_allowed(headers: &HeaderMap, trusted: &[String]) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    if loopback_host(authority.host()) {
        return true;
    }
    if authority.host() == "core" && authority.port_u16() == Some(8765) {
        return true;
    }
    trusted.iter().any(|origin| {
        origin.parse::<axum::http::Uri>().ok().is_some_and(|uri| {
            uri.scheme_str() == Some("https")
                && uri
                    .authority()
                    .is_some_and(|allowed| authority_matches(allowed, &authority, "https"))
        })
    })
}

fn origin_allowed(origin: &str, headers: &HeaderMap, trusted: &[String]) -> bool {
    let Ok(uri) = origin.parse::<axum::http::Uri>() else {
        return false;
    };
    if uri.path() != "" && uri.path() != "/" || uri.query().is_some() {
        return false;
    }
    let Some(scheme) = uri.scheme_str() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    let request_host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok());
    let same_authority =
        request_host.is_some_and(|host| authority_matches(&host, authority, scheme));
    if !same_authority {
        return false;
    }
    if scheme == "http" && loopback_host(authority.host()) {
        return true;
    }
    scheme == "https" && trusted.iter().any(|allowed| allowed == origin)
}

fn authority_matches(
    left: &axum::http::uri::Authority,
    right: &axum::http::uri::Authority,
    scheme: &str,
) -> bool {
    if !left.host().eq_ignore_ascii_case(right.host()) {
        return false;
    }
    let default_port = match scheme {
        "https" => 443,
        "http" => 80,
        _ => return false,
    };
    left.port_u16().unwrap_or(default_port) == right.port_u16().unwrap_or(default_port)
}

fn api_error(error: Error) -> Response {
    let status = match error {
        Error::Invalid(_) | Error::Repository(_) | Error::Delegation(_) => StatusCode::BAD_REQUEST,
        Error::Conflict(_) | Error::Constraint(_) | Error::Withdrawn => StatusCode::CONFLICT,
        Error::AlreadyStarted { .. } => StatusCode::CONFLICT,
        Error::Forbidden(_) => StatusCode::FORBIDDEN,
        Error::NotFound => StatusCode::NOT_FOUND,
        Error::Busy => StatusCode::SERVICE_UNAVAILABLE,
        Error::Sqlx(_) | Error::Migrate(_) | Error::File(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    if let Error::AlreadyStarted {
        campaign_id,
        campaign_status,
    } = &error
    {
        return (
            status,
            Json(json!({
                "error": error.to_string(),
                "code": "LEAD_ALREADY_STARTED",
                "campaign_id": campaign_id,
                "campaign_status": campaign_status,
            })),
        )
            .into_response();
    }
    (status, Json(json!({"error":error.to_string()}))).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct EnrollmentRequest {
    code: String,
    #[serde(default)]
    device_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingQualityQuery {
    role: String,
}

fn default_max_turns() -> u32 {
    4
}
fn default_max_input_tokens() -> u64 {
    60_000
}
fn default_max_output_tokens() -> u64 {
    12_000
}
fn default_max_cost_micros() -> u64 {
    500_000
}
fn default_input_rate() -> u64 {
    150_000
}
fn default_output_rate() -> u64 {
    600_000
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ProviderSettingsInput {
    model: String,
    base_url: String,
    #[serde(default)]
    credential: Option<String>,
    enabled: bool,
    is_default: bool,
    #[serde(default = "default_max_turns")]
    max_turns: u32,
    #[serde(default = "default_max_input_tokens")]
    max_input_tokens: u64,
    #[serde(default = "default_max_output_tokens")]
    max_output_tokens: u64,
    #[serde(default = "default_max_cost_micros")]
    max_cost_micros: u64,
    #[serde(default = "default_input_rate")]
    input_micros_per_million: u64,
    #[serde(default = "default_output_rate")]
    output_micros_per_million: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RuntimeSettingsInput {
    semantic_review_enabled: bool,
    semantic_review_max_output_tokens: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerProviderQuery {
    provider: String,
    model: String,
    worker_id: String,
    attempt_id: String,
    generation: i64,
}

async fn runtime_settings(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.factory_settings().await {
        Ok(settings) => Json(settings).into_response(),
        Err(error) => api_error(error),
    }
}

async fn update_runtime_settings(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<RuntimeSettingsInput>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app
        .db
        .update_factory_settings(
            input.semantic_review_enabled,
            input.semantic_review_max_output_tokens,
            "office-operator",
        )
        .await
    {
        Ok(settings) => Json(settings).into_response(),
        Err(error) => api_error(error),
    }
}

async fn review_config(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let authorized = token
        .is_some_and(|token| constant_time_eq(token.as_bytes(), app.reviewer_token.as_bytes()));
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let settings = match app.db.factory_settings().await {
        Ok(settings) => settings,
        Err(error) => return api_error(error),
    };
    let gateway = app.current_model_gateway().await;
    let mechanism = if settings.semantic_review_enabled && gateway.is_some() {
        "MODEL"
    } else {
        "DETERMINISTIC"
    };
    Json(json!({
        "mechanism": mechanism,
        "maxOutputTokens": settings.semantic_review_max_output_tokens,
        "reason": if mechanism == "MODEL" {
            "operator enabled semantic review and a default provider is configured"
        } else if settings.semantic_review_enabled {
            "semantic review is enabled but no model gateway is configured"
        } else {
            "semantic review is disabled"
        }
    }))
    .into_response()
}

async fn pending_quality(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<PendingQualityQuery>,
) -> Response {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let authorized = match (query.role.as_str(), token) {
        ("verifier", Some(token)) => {
            constant_time_eq(token.as_bytes(), app.verifier_token.as_bytes())
        }
        ("reviewer", Some(token)) => {
            constant_time_eq(token.as_bytes(), app.reviewer_token.as_bytes())
        }
        _ => false,
    };
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.pending_quality_candidates(&query.role).await {
        Ok(candidates) => Json(json!({"candidates": candidates})).into_response(),
        Err(error) => api_error(error),
    }
}

async fn setup_status(State(app): State<Arc<App>>) -> Response {
    let required = bootstrap_available(&app).await;
    Json(json!({
        "required": required,
        "version": "0.1.0"
    }))
    .into_response()
}

async fn auth_session(
    State(app): State<Arc<App>>,
    session: Option<Extension<factory_native::db::DeviceSessionRecord>>,
) -> Response {
    if let Some(Extension(device)) = session {
        return Json(json!({
            "authenticated": true,
            "device": device,
        }))
        .into_response();
    }
    let bootstrap_available = bootstrap_available(&app).await;
    Json(json!({
        "authenticated": false,
        "bootstrapAvailable": bootstrap_available,
    }))
    .into_response()
}

async fn bootstrap_available(app: &App) -> bool {
    let Some(path) = app.enrollment_code_file.as_ref() else {
        return false;
    };
    if !matches!(app.db.owner_bootstrap_available().await, Ok(true)) {
        return false;
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BootstrapCodeRecord>(&bytes).ok())
        .is_some_and(|record| record.expires_at_ms > chrono::Utc::now().timestamp_millis())
}

async fn auth_bootstrap(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<EnrollmentRequest>,
) -> Response {
    let Some(path) = app.enrollment_code_file.as_ref() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"first-run setup is not enabled"})),
        )
            .into_response();
    };
    if input.code.trim().is_empty() || input.code.len() > 256 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"enrollment code is invalid"})),
        )
            .into_response();
    }
    if !matches!(app.db.owner_bootstrap_available().await, Ok(true)) {
        return StatusCode::CONFLICT.into_response();
    }
    let now = chrono::Utc::now().timestamp_millis();
    {
        let Ok(state) = app.setup.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if state.blocked_until_ms > now {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":"too many enrollment attempts; wait and retry"})),
            )
                .into_response();
        }
    }
    let expected = match std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BootstrapCodeRecord>(&bytes).ok())
    {
        Some(record) if record.expires_at_ms > now => record.code_sha256,
        Some(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"enrollment code has expired"})),
            )
                .into_response();
        }
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"enrollment is not available"})),
            )
                .into_response();
        }
    };
    let submitted = input.code.trim();
    if expected.is_empty()
        || !constant_time_eq(
            factory_native::device_session::sha256_hex(submitted).as_bytes(),
            expected.as_bytes(),
        )
    {
        if let Ok(mut state) = app.setup.lock() {
            state.failures = state.failures.saturating_add(1);
            if state.failures >= 8 {
                state.failures = 0;
                state.blocked_until_ms = now + 60_000;
            }
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"enrollment code is invalid"})),
        )
            .into_response();
    }
    let device_name = input.device_name.as_deref().unwrap_or("This device").trim();
    if device_name.is_empty() || device_name.len() > 120 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let issued = match factory_native::device_session::issue_owner(&app.db, device_name, now).await
    {
        Ok(issued) => issued,
        Err(error) => return api_error(error),
    };
    if let Err(error) = std::fs::remove_file(path) {
        tracing::error!(%error, "enrollment code could not be invalidated");
        let _ = app
            .db
            .revoke_device_session(&issued.session_id, "bootstrap-failed", now)
            .await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"enrollment could not be completed"})),
        )
            .into_response();
    }
    let used = path.with_file_name("enrollment-used");
    if let Err(error) = std::fs::write(&used, b"used\n") {
        tracing::error!(%error, "enrollment completion marker could not be written");
        let _ = app
            .db
            .revoke_device_session(&issued.session_id, "bootstrap-failed", now)
            .await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"enrollment could not be completed"})),
        )
            .into_response();
    }
    if let Ok(mut state) = app.setup.lock() {
        state.failures = 0;
        state.blocked_until_ms = 0;
    }
    factory_native::telemetry::register_secret(&issued.session_token);
    factory_native::telemetry::register_secret(&issued.csrf_token);
    let secure =
        factory_native::device_session::secure_cookie_context(&headers, &trusted_origins());
    let mut response = Json(json!({
        "authenticated": true,
        "device": issued.device,
    }))
    .into_response();
    factory_native::device_session::set_cookies(&mut response, &issued, secure);
    response
}

async fn auth_logout(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Extension(session): Extension<factory_native::db::DeviceSessionRecord>,
) -> Response {
    match app
        .db
        .revoke_device_session(
            &session.id,
            &session.id,
            chrono::Utc::now().timestamp_millis(),
        )
        .await
    {
        Ok(_) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            factory_native::device_session::clear_cookies(
                &mut response,
                factory_native::device_session::secure_cookie_context(&headers, &trusted_origins()),
            );
            response
        }
        Err(error) => api_error(error),
    }
}

async fn auth_logout_all(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Extension(session): Extension<factory_native::db::DeviceSessionRecord>,
) -> Response {
    match app
        .db
        .revoke_all_device_sessions(&session.id, chrono::Utc::now().timestamp_millis())
        .await
    {
        Ok(_) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            factory_native::device_session::clear_cookies(
                &mut response,
                factory_native::device_session::secure_cookie_context(&headers, &trusted_origins()),
            );
            response
        }
        Err(error) => api_error(error),
    }
}

async fn auth_rotate(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Extension(session): Extension<factory_native::db::DeviceSessionRecord>,
) -> Response {
    let issued = match factory_native::device_session::rotate(
        &app.db,
        &session,
        &session.id,
        chrono::Utc::now().timestamp_millis(),
    )
    .await
    {
        Ok(Some(issued)) => issued,
        Ok(None) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(error) => return api_error(error),
    };
    factory_native::telemetry::register_secret(&issued.session_token);
    factory_native::telemetry::register_secret(&issued.csrf_token);
    let mut response = Json(json!({"authenticated":true,"device":issued.device})).into_response();
    factory_native::device_session::set_cookies(
        &mut response,
        &issued,
        factory_native::device_session::secure_cookie_context(&headers, &trusted_origins()),
    );
    response
}

fn provider_view(
    settings: Option<&factory_native::db::ProviderSettingsRecord>,
    catalog: &factory_native::provider_catalog::ProviderCatalogEntry,
) -> serde_json::Value {
    json!({
        "provider": catalog.provider,
        "kind": catalog.kind,
        "displayName": catalog.display_name,
        "defaultModel": catalog.default_model,
        "defaultBaseUrl": catalog.default_base_url,
        "authModes": catalog.auth_modes,
        "chatgptOauth": catalog.chatgpt_oauth,
        "status": catalog.status,
        "note": catalog.note,
        "configured": settings.is_some_and(|settings| !settings.credential_ciphertext.is_empty()),
        "enabled": settings.is_some_and(|settings| settings.enabled),
        "isDefault": settings.is_some_and(|settings| settings.is_default),
        "model": settings.map(|settings| settings.model.clone()).unwrap_or_else(|| catalog.default_model.into()),
        "baseUrl": settings.map(|settings| settings.base_url.clone()).unwrap_or_else(|| catalog.default_base_url.into()),
        "credentialFingerprint": settings.map(|settings| settings.credential_fingerprint.clone()).unwrap_or_default(),
        "revision": settings.map(|settings| settings.revision).unwrap_or(0),
        "updatedAt": settings.map(|settings| settings.updated_at.clone()).unwrap_or_default(),
        "maxTurns": settings.map(|settings| settings.max_turns).unwrap_or(default_max_turns()),
        "maxInputTokens": settings.map(|settings| settings.max_input_tokens).unwrap_or(default_max_input_tokens()),
        "maxOutputTokens": settings.map(|settings| settings.max_output_tokens).unwrap_or(default_max_output_tokens()),
        "maxCostMicros": settings.map(|settings| settings.max_cost_micros).unwrap_or(default_max_cost_micros()),
        "inputMicrosPerMillion": settings.map(|settings| settings.input_micros_per_million).unwrap_or(default_input_rate()),
        "outputMicrosPerMillion": settings.map(|settings| settings.output_micros_per_million).unwrap_or(default_output_rate()),
    })
}

async fn provider_settings_list(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let stored = match app.db.provider_settings_all().await {
        Ok(stored) => stored,
        Err(error) => return api_error(error),
    };
    let providers: Vec<serde_json::Value> = factory_native::provider_catalog::CATALOG
        .iter()
        .map(|catalog| {
            provider_view(
                stored
                    .iter()
                    .find(|settings| settings.provider == catalog.provider),
                catalog,
            )
        })
        .collect();
    Json(json!({"providers":providers})).into_response()
}

async fn update_provider_settings(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
    Json(input): Json<ProviderSettingsInput>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(catalog) = factory_native::provider_catalog::entry(&provider) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"unknown provider"})),
        )
            .into_response();
    };
    let allow_loopback_http = env::var("FACTORY_ALLOW_TEST_UPSTREAM").ok().as_deref() == Some("1");
    if let Err(error) = factory_native::provider_catalog::validate_model(&input.model)
        .and_then(|()| {
            factory_native::provider_catalog::validate_base_url(
                &input.base_url,
                allow_loopback_http,
            )
        })
        .and_then(|()| {
            factory_native::provider_catalog::validate_budget(
                input.max_turns,
                input.max_input_tokens,
                input.max_output_tokens,
                input.max_cost_micros,
            )
        })
    {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response();
    }
    if input.input_micros_per_million > 10_000_000
        || input.output_micros_per_million > 10_000_000
        || (input.input_micros_per_million == 0 && input.output_micros_per_million == 0)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"provider rates are outside the supported bound"})),
        )
            .into_response();
    }
    let Some(master_key) = app.master_key.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"provider credential protection is not configured"})),
        )
            .into_response();
    };
    let existing = match app.db.provider_settings(&provider).await {
        Ok(settings) => Some(settings),
        Err(factory_native::db::Error::NotFound) => None,
        Err(error) => return api_error(error),
    };
    let encrypted = match input
        .credential
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(credential) => {
            if let Err(error) = factory_native::provider_catalog::validate_credential(credential) {
                return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response();
            }
            match master_key.encrypt(credential.as_bytes()) {
                Ok(encrypted) => encrypted,
                Err(error) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": error})),
                    )
                        .into_response();
                }
            }
        }
        None => match existing.as_ref() {
            Some(settings) if !settings.credential_ciphertext.is_empty() => {
                factory_native::provider_secrets::EncryptedCredential {
                    ciphertext: settings.credential_ciphertext.clone(),
                    nonce: settings.credential_nonce.clone(),
                    fingerprint: settings.credential_fingerprint.clone(),
                }
            }
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"an initial credential is required"})),
                )
                    .into_response();
            }
        },
    };
    let write = factory_native::db::ProviderSettingsWrite {
        provider: provider.clone(),
        kind: catalog.kind.into(),
        model: input.model,
        base_url: input.base_url.trim_end_matches('/').to_owned(),
        auth_mode: "api_key".into(),
        credential_ciphertext: encrypted.ciphertext,
        credential_nonce: encrypted.nonce,
        credential_fingerprint: encrypted.fingerprint,
        enabled: input.enabled,
        is_default: input.is_default,
        max_turns: input.max_turns,
        max_input_tokens: input.max_input_tokens,
        max_output_tokens: input.max_output_tokens,
        max_cost_micros: input.max_cost_micros,
        input_micros_per_million: input.input_micros_per_million,
        output_micros_per_million: input.output_micros_per_million,
    };
    match app
        .db
        .save_provider_settings(&write, "office-operator")
        .await
    {
        Ok(settings) => Json(provider_view(Some(&settings), catalog)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn revoke_provider_credential(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(catalog) = factory_native::provider_catalog::entry(&provider) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"unknown provider"})),
        )
            .into_response();
    };
    match app
        .db
        .clear_provider_credential(&provider, "office-operator")
        .await
    {
        Ok(settings) => Json(provider_view(Some(&settings), catalog)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn test_provider_connection(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(_catalog) = factory_native::provider_catalog::entry(&provider) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"unknown provider"})),
        )
            .into_response();
    };
    let settings = match app.db.provider_settings(&provider).await {
        Ok(settings) => settings,
        Err(factory_native::db::Error::NotFound) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"provider is not configured"})),
            )
                .into_response();
        }
        Err(error) => return api_error(error),
    };
    let Some(master_key) = app.master_key.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"provider credential protection is not configured"})),
        )
            .into_response();
    };
    let credential =
        match master_key.decrypt(&settings.credential_ciphertext, &settings.credential_nonce) {
            Ok(credential) => match String::from_utf8(credential) {
                Ok(credential) => credential,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error":"stored credential is not valid text"})),
                    )
                        .into_response();
                }
            },
            Err(error) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": error})),
                )
                    .into_response();
            }
        };
    factory_native::telemetry::register_secret(&credential);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
            )
                .into_response();
        }
    };
    let url = format!("{}/models", settings.base_url.trim_end_matches('/'));
    let response = client.get(&url).bearer_auth(&credential).send().await;
    let (status, models, detail) = match response {
        Ok(response) => {
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                ("DENIED", Vec::new(), format!("HTTP {status}"))
            } else if status == reqwest::StatusCode::NOT_FOUND
                || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
            {
                ("UNSUPPORTED", Vec::new(), format!("HTTP {status}"))
            } else if status.is_success() {
                let body: serde_json::Value = response.json().await.unwrap_or_default();
                let models: Vec<String> = body
                    .get("data")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
                            .take(100)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                ("AUTHENTICATED", models, "model list retrieved".into())
            } else {
                ("UNREACHABLE", Vec::new(), format!("HTTP {status}"))
            }
        }
        Err(error) => ("UNREACHABLE", Vec::new(), error.to_string()),
    };
    let model_available = models.iter().any(|model| model == &settings.model);
    let _ = app
        .db
        .record_provider_test(&provider, "office-operator", &detail)
        .await;
    Json(json!({
        "provider": provider,
        "status": status,
        "models": models,
        "model": settings.model,
        "modelAvailable": model_available,
        "detail": detail
    }))
    .into_response()
}

async fn provider_audit(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.provider_settings_audit(&provider).await {
        Ok(audit) => Json(json!({"audit": audit})).into_response(),
        Err(error) => api_error(error),
    }
}

async fn worker_provider_profile(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<WorkerProviderQuery>,
) -> Response {
    if !worker_session_authorized(&app, &headers, &query.worker_id).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (provider, model) = match app
        .db
        .assigned_provider_route(
            &query.worker_id,
            &query.attempt_id,
            query.generation,
            chrono::Utc::now().timestamp_millis(),
        )
        .await
    {
        Ok(route) => route,
        Err(error) => return api_error(error),
    };
    if provider != query.provider || model != query.model {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"provider profile does not match the live authorized assignment"})),
        )
            .into_response();
    }
    let settings = match app.db.provider_settings(&query.provider).await {
        Ok(settings) => settings,
        Err(factory_native::db::Error::NotFound) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"provider is not configured"})),
            )
                .into_response();
        }
        Err(error) => return api_error(error),
    };
    if !settings.enabled
        || settings.model != query.model
        || settings.credential_ciphertext.is_empty()
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"provider route is not enabled or does not match the assignment"})),
        )
            .into_response();
    }
    let Some(master_key) = app.master_key.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"provider credential protection is not configured"})),
        )
            .into_response();
    };
    let credential =
        match master_key.decrypt(&settings.credential_ciphertext, &settings.credential_nonce) {
            Ok(credential) => match String::from_utf8(credential) {
                Ok(credential) => credential,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            },
            Err(error) => {
                tracing::warn!(%error, "worker provider credential could not be decrypted");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
    factory_native::telemetry::register_secret(&credential);
    Json(json!({
        "provider": settings.provider,
        "kind": settings.kind,
        "model": settings.model,
        "baseUrl": settings.base_url,
        "credential": credential,
        "maxTurns": settings.max_turns,
        "maxInputTokens": settings.max_input_tokens,
        "maxOutputTokens": settings.max_output_tokens,
        "maxCostMicros": settings.max_cost_micros
    }))
    .into_response()
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
    let repository = match resolve_project_input(&app, &input.repository).await {
        Ok(path) => path,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response();
        }
    };
    match app
        .db
        .register_project(&repository, &input.idempotency_key)
        .await
    {
        Ok(project) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(error) => api_error(error),
    }
}

async fn resolve_project_input(app: &App, requested: &str) -> Result<PathBuf, String> {
    let Some(root) = app.projects_root.as_ref() else {
        return Ok(PathBuf::from(requested));
    };
    if factory_native::project_import::is_remote_repository(requested) {
        return factory_native::project_import::import(requested, root, app.allow_insecure_git)
            .await
            .map_err(|error| error.to_string());
    }
    let path = PathBuf::from(requested);
    if !path.is_absolute() {
        return Err(
            "managed project registration requires an HTTPS Git URL or an absolute path inside the managed projects root"
                .into(),
        );
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("managed projects root is unavailable: {error}"))?;
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("managed project path is unavailable: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("project path is outside the managed projects root".into());
    }
    Ok(canonical)
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
fn count_statuses<T, F>(items: &[T], key: F) -> BTreeMap<String, usize>
where
    F: Fn(&T) -> &str,
{
    let mut counts = BTreeMap::new();
    for item in items {
        *counts.entry(key(item).to_owned()).or_insert(0) += 1;
    }
    counts
}

fn campaign_map(
    runs: &[factory_native::db::Run],
    attempts: &[factory_native::db::Attempt],
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let run_campaign: BTreeMap<String, String> = runs
        .iter()
        .map(|run| (run.id.clone(), run.campaign_id.clone()))
        .collect();
    let attempt_campaign: BTreeMap<String, String> = attempts
        .iter()
        .filter_map(|attempt| {
            run_campaign
                .get(&attempt.run_id)
                .map(|campaign| (attempt.id.clone(), campaign.clone()))
        })
        .collect();
    (run_campaign, attempt_campaign)
}

fn decision_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "attention" => 1,
        _ => 2,
    }
}

async fn build_decisions(app: &App) -> Result<Vec<serde_json::Value>, Error> {
    let campaigns = app.db.campaigns().await?;
    let runs = app.db.runs().await?;
    let attempts = app.db.attempts().await?;
    let candidates = app.db.candidates().await?;
    let lead_requests = app.db.lead_requests().await?;
    let sessions = app.db.provider_sessions().await?;
    let model_calls = app.db.unresolved_model_calls().await?;
    let actions = app.db.actions().await?;
    let (run_campaign, attempt_campaign) = campaign_map(&runs, &attempts);
    let mut items = Vec::new();

    for campaign in campaigns
        .iter()
        .filter(|campaign| campaign.status == "BLOCKED_AUTHORIZATION")
    {
        items.push(json!({
            "id": format!("campaign-authorization:{}", campaign.id),
            "kind": "campaign.authorization",
            "severity": "attention",
            "actionable": true,
            "title": "Campaign awaiting authorization",
            "detail": format!("Campaign {} is blocked pending an explicit authorization decision.", campaign.id),
            "target": "/campaigns",
            "subject_id": campaign.id,
            "updated_at": campaign.created_at,
        }));
    }

    for request in lead_requests
        .iter()
        .filter(|request| request.state == LeadRequestState::Ready)
    {
        items.push(json!({
            "id": format!("lead-ready:{}", request.id),
            "kind": "lead.ready",
            "severity": "attention",
            "actionable": true,
            "title": "Frozen Lead plan ready",
            "detail": format!("Lead request {} has a frozen plan and needs an explicit start decision.", request.id),
            "target": "/lead",
            "subject_id": request.id,
            "updated_at": request.updated_at,
        }));
    }

    for candidate in &candidates {
        let Some(gate) = app.db.gate_for_candidate(&candidate.id).await? else {
            continue;
        };
        let needs_attention = matches!(
            gate.outcome,
            GateOutcome::RepairRequired
                | GateOutcome::Blocked
                | GateOutcome::HumanRequired
                | GateOutcome::NotTested
        );
        if !needs_attention {
            continue;
        }
        let campaign = attempt_campaign
            .get(&candidate.attempt_id)
            .cloned()
            .unwrap_or_default();
        let target = if campaign.is_empty() {
            "/campaigns".to_owned()
        } else {
            format!("/campaigns/{campaign}")
        };
        items.push(json!({
            "id": format!("candidate-gate:{}", candidate.id),
            "kind": "quality.gate",
            "severity": if matches!(gate.outcome, GateOutcome::HumanRequired | GateOutcome::Blocked) { "critical" } else { "attention" },
            "actionable": true,
            "title": format!("Quality Gate: {}", match gate.outcome {
                GateOutcome::Accepted => "ACCEPTED",
                GateOutcome::RepairRequired => "REPAIR_REQUIRED",
                GateOutcome::Blocked => "BLOCKED",
                GateOutcome::HumanRequired => "HUMAN_REQUIRED",
                GateOutcome::NotTested => "NOT_TESTED",
            }),
            "detail": format!("Candidate {} for Campaign {} needs operator attention. {}", candidate.id, if campaign.is_empty() { "unknown" } else { &campaign }, gate.reasons.join("; ")),
            "target": target,
            "subject_id": candidate.id,
            "updated_at": gate.decided_at,
        }));
    }

    for call in &model_calls {
        items.push(json!({
            "id": format!("model-call:{}", call.reservation_id),
            "kind": "model.usage-unresolved",
            "severity": "critical",
            "actionable": true,
            "title": "Unresolved model reservation",
            "detail": format!("{} model call {} is {}. The reservation remains unavailable for reuse until an operator releases it; it is not measured spend.", call.provider, call.reservation_id, call.state),
            "target": "/runtime",
            "subject_id": call.reservation_id,
            "updated_at": call.updated_at,
            "action": {
                "kind": "resolve_model_call",
                "model_call_id": call.reservation_id,
                "label": "Release reservation"
            }
        }));
    }

    for session in sessions.iter().filter(|session| {
        matches!(
            session.state.as_str(),
            "INTENT" | "ACTIVE" | "CANCEL_REQUESTED" | "ORPHANED"
        )
    }) {
        items.push(json!({
            "id": format!("provider-session:{}", session.session_record_id),
            "kind": "provider.unresolved-session",
            "severity": "attention",
            "actionable": false,
            "title": "Provider session requires monitoring",
            "detail": format!("Provider session {} for Attempt {} is {}. External termination may be unconfirmed.", session.session_record_id, session.attempt_id, session.state),
            "target": "/runtime",
            "subject_id": session.session_record_id,
            "updated_at": session.updated_at,
        }));
    }

    for run in runs
        .iter()
        .filter(|run| matches!(run.status.as_str(), "FAILED" | "REJECTED"))
    {
        let campaign = run_campaign.get(&run.id).cloned().unwrap_or_default();
        items.push(json!({
            "id": format!("run:{}", run.id),
            "kind": "run.failed",
            "severity": "attention",
            "actionable": false,
            "title": "Run failed",
            "detail": format!("Run {} for Campaign {} is {}.", run.id, if campaign.is_empty() { "unknown" } else { &campaign }, run.status),
            "target": if campaign.is_empty() { "/campaigns".to_owned() } else { format!("/campaigns/{campaign}") },
            "subject_id": run.id,
            "updated_at": run.created_at,
        }));
    }

    for action in actions
        .iter()
        .filter(|action| action.status == "HUMAN_REQUIRED")
    {
        items.push(json!({
            "id": format!("action:{}", action.id),
            "kind": "action.human-required",
            "severity": "attention",
            "actionable": false,
            "title": "Action is HUMAN_REQUIRED",
            "detail": format!("Action {} needs review outside the Inbox; no Inbox mutation is exposed.", action.id),
            "target": format!("/actions/{}", action.id),
            "subject_id": action.id,
            "updated_at": action.created_at,
        }));
    }

    let mut seen = BTreeSet::new();
    items.retain(|item| {
        let id = item["id"].as_str().unwrap_or_default().to_owned();
        seen.insert(id)
    });
    items.sort_by(|left, right| {
        let left_rank = decision_rank(left["severity"].as_str().unwrap_or("info"));
        let right_rank = decision_rank(right["severity"].as_str().unwrap_or("info"));
        left_rank.cmp(&right_rank).then_with(|| {
            right["updated_at"]
                .as_str()
                .unwrap_or("")
                .cmp(left["updated_at"].as_str().unwrap_or(""))
        })
    });
    Ok(items)
}

async fn office_overview(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match build_overview(&app).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => api_error(error),
    }
}

async fn office_decisions(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match build_decisions(&app).await {
        Ok(items) => {
            Json(json!({"items": items, "as_of": chrono::Utc::now().to_rfc3339()})).into_response()
        }
        Err(error) => api_error(error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParticipantsQuery {
    role: Option<String>,
    provider: Option<String>,
    status: Option<String>,
    campaign_id: Option<String>,
    project_id: Option<String>,
    participant_id: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolEventsQuery {
    after_sequence: Option<i64>,
    limit: Option<usize>,
}

fn filter_text(
    value: Option<String>,
    field: &'static str,
    max: usize,
) -> Result<Option<String>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Invalid(field));
    }
    Ok(Some(value.to_owned()))
}

fn participants_query(
    query: ParticipantsQuery,
) -> Result<factory_native::db::ParticipantQuery, Error> {
    let role = filter_text(query.role, "participant role", 32)?.map(|value| value.to_uppercase());
    if let Some(role) = &role
        && !factory_native::db::participants::is_valid_participant_role(role)
    {
        return Err(Error::Invalid("participant role"));
    }
    let status =
        filter_text(query.status, "participant status", 64)?.map(|value| value.to_uppercase());
    if let Some(status) = &status
        && !factory_native::db::participants::is_valid_participant_status(status)
    {
        return Err(Error::Invalid("participant status"));
    }
    let provider = filter_text(query.provider, "participant provider", 128)?;
    let campaign_id = filter_text(query.campaign_id, "participant campaign", 256)?;
    let project_id = filter_text(query.project_id, "participant project", 256)?;
    let participant_id = filter_text(query.participant_id, "participant identity", 256)?;
    let limit = query.limit.unwrap_or(100);
    if !(1..=factory_native::db::participants::PARTICIPANT_PAGE_MAX).contains(&limit) {
        return Err(Error::Invalid("participant limit"));
    }
    let offset = query.offset.unwrap_or(0);
    if offset > factory_native::db::participants::PARTICIPANT_OFFSET_MAX {
        return Err(Error::Invalid("participant offset"));
    }
    Ok(factory_native::db::ParticipantQuery {
        role,
        provider,
        status,
        campaign_id,
        project_id,
        participant_id,
        limit,
        offset,
    })
}

async fn office_participants(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(query): Query<ParticipantsQuery>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match participants_query(query) {
        Ok(query) => match app.db.participants(&query).await {
            Ok(page) => Json(page).into_response(),
            Err(error) => api_error(error),
        },
        Err(error) => api_error(error),
    }
}

async fn provider_session_tool_events(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(session_record_id): AxumPath<String>,
    Query(query): Query<ToolEventsQuery>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if session_record_id.trim().is_empty()
        || session_record_id.len() > 256
        || session_record_id.chars().any(char::is_control)
    {
        return api_error(Error::Invalid("provider session identity"));
    }
    let after_sequence = query.after_sequence.unwrap_or(0);
    if after_sequence < 0 {
        return api_error(Error::Invalid("tool event cursor"));
    }
    let limit = query.limit.unwrap_or(100);
    if !(1..=factory_native::db::participants::TOOL_EVENT_PAGE_MAX).contains(&limit) {
        return api_error(Error::Invalid("tool event limit"));
    }
    match app
        .db
        .provider_tool_event_projection(&session_record_id, after_sequence, limit)
        .await
    {
        Ok(projection) => Json(projection).into_response(),
        Err(error) => api_error(error),
    }
}

async fn build_overview(app: &App) -> Result<serde_json::Value, Error> {
    let projects = app.db.projects().await?;
    let actions = app.db.actions().await?;
    let campaigns = app.db.campaigns().await?;
    let runs = app.db.runs().await?;
    let attempts = app.db.attempts().await?;
    let candidates = app.db.candidates().await?;
    let sessions = app.db.provider_sessions().await?;
    let workers = app.db.workers().await?;
    let model_calls = app.db.model_calls(200).await?;
    let unresolved_model_calls = app.db.unresolved_model_calls().await?;
    let (unmeasured_model_calls, unresolved_model_bound) =
        app.db.unresolved_model_call_budget().await?;
    let lead_requests = app.db.lead_requests().await?;
    let events = app.db.events().await?;
    let (_, attempt_campaign) = campaign_map(&runs, &attempts);
    let mut recent_accepted = Vec::new();
    let mut gate_outcomes: BTreeMap<String, usize> = BTreeMap::new();
    for candidate in &candidates {
        let Some(gate) = app.db.gate_for_candidate(&candidate.id).await? else {
            continue;
        };
        let label = match gate.outcome {
            GateOutcome::Accepted => "ACCEPTED",
            GateOutcome::RepairRequired => "REPAIR_REQUIRED",
            GateOutcome::Blocked => "BLOCKED",
            GateOutcome::HumanRequired => "HUMAN_REQUIRED",
            GateOutcome::NotTested => "NOT_TESTED",
        };
        *gate_outcomes.entry(label.to_owned()).or_insert(0) += 1;
        if gate.outcome == GateOutcome::Accepted {
            let campaign = attempt_campaign
                .get(&candidate.attempt_id)
                .cloned()
                .unwrap_or_default();
            recent_accepted.push(json!({
                "candidate_id": candidate.id,
                "campaign_id": campaign,
                "commit_sha": candidate.commit_sha,
                "decided_at": gate.decided_at,
                "outcome": "ACCEPTED",
            }));
        }
    }
    recent_accepted.sort_by(|left, right| {
        right["decided_at"]
            .as_str()
            .unwrap_or("")
            .cmp(left["decided_at"].as_str().unwrap_or(""))
    });
    recent_accepted.truncate(5);

    let decisions = build_decisions(app).await?;
    let actionable_decisions = decisions
        .iter()
        .filter(|item| item["actionable"].as_bool().unwrap_or(false))
        .count();
    let authorized_campaigns = campaigns
        .iter()
        .filter(|campaign| campaign.status == "AUTHORIZED")
        .count();
    let campaigns_awaiting_authorization = campaigns
        .iter()
        .filter(|campaign| campaign.status == "BLOCKED_AUTHORIZATION")
        .count();
    let failed_campaigns = campaigns
        .iter()
        .filter(|campaign| campaign.status == "FAILED")
        .count();
    let failed_runs = runs
        .iter()
        .filter(|run| matches!(run.status.as_str(), "FAILED" | "REJECTED"))
        .count();
    let blocked_actions = actions
        .iter()
        .filter(|action| {
            matches!(
                action.status.as_str(),
                "HUMAN_REQUIRED" | "FAILED" | "BLOCKED"
            )
        })
        .count();
    let gate_blocked = gate_outcomes
        .iter()
        .filter(|(outcome, _)| {
            matches!(
                outcome.as_str(),
                "REPAIR_REQUIRED" | "BLOCKED" | "HUMAN_REQUIRED" | "NOT_TESTED"
            )
        })
        .map(|(_, count)| count)
        .sum::<usize>();
    let unresolved_sessions = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.state.as_str(),
                "INTENT" | "ACTIVE" | "CANCEL_REQUESTED" | "ORPHANED"
            )
        })
        .count();
    let active_sessions = sessions
        .iter()
        .filter(|session| session.state == "ACTIVE")
        .count();
    let unresolved_model_call_count = unresolved_model_calls.len();
    let unmeasured_sessions = sessions
        .iter()
        .filter(|session| !session.usage_known)
        .count();
    let unmeasured_bound_cost: u64 = sessions
        .iter()
        .filter(|session| !session.usage_known)
        .map(|session| session.cost_micros)
        .sum::<u64>()
        + unresolved_model_bound;
    let measured_session_cost: u64 = sessions
        .iter()
        .filter(|session| session.usage_known)
        .map(|session| session.cost_micros)
        .sum();
    let measured_session_tokens: u64 = sessions
        .iter()
        .filter(|session| session.usage_known)
        .map(|session| session.total_tokens)
        .sum();
    let measured_model_cost: u64 = model_calls
        .iter()
        .filter(|call| call.usage_known)
        .map(|call| call.cost_micros)
        .sum();
    let measured_model_tokens: u64 = model_calls
        .iter()
        .filter(|call| call.usage_known)
        .map(|call| call.total_tokens)
        .sum();
    let recent_events: Vec<_> = events.iter().rev().take(12).cloned().collect();
    let recent_decisions: Vec<_> = decisions.iter().take(5).cloned().collect();

    Ok(json!({
        "as_of": chrono::Utc::now().to_rfc3339(),
        "source": "Core durable snapshots; no client-side invented state",
        "counts": {
            "projects": projects.len(),
            "actions": actions.len(),
            "campaigns": campaigns.len(),
            "runs": runs.len(),
            "attempts": attempts.len(),
            "candidates": candidates.len(),
            "workers": workers.len(),
            "lead_requests": lead_requests.len(),
            "authorized_campaigns": authorized_campaigns,
            "campaigns_awaiting_authorization": campaigns_awaiting_authorization,
            "active_agents": active_sessions,
            "unresolved_sessions": unresolved_sessions,
            "unresolved_model_calls": unresolved_model_call_count,
            "pending_decisions": actionable_decisions,
            "blocked_signals": blocked_actions + failed_campaigns + failed_runs + gate_blocked + campaigns_awaiting_authorization,
        },
        "actions_by_status": count_statuses(&actions, |action| action.status.as_str()),
        "campaigns_by_status": count_statuses(&campaigns, |campaign| campaign.status.as_str()),
        "runs_by_status": count_statuses(&runs, |run| run.status.as_str()),
        "gate_outcomes": gate_outcomes,
        "budget": {
            "measured_session_cost_micros": measured_session_cost,
            "measured_session_tokens": measured_session_tokens,
            "measured_model_cost_micros": measured_model_cost,
            "measured_model_tokens": measured_model_tokens,
            "unmeasured_sessions": unmeasured_sessions,
            "unmeasured_model_calls": unmeasured_model_calls,
            "unmeasured_bound_cost_micros": unmeasured_bound_cost,
            "model_calls_sampled": model_calls.len(),
            "note": "Measured totals include usage-known records only; measured model-call totals cover the latest 200 calls. Unresolved reservations are counted from the full ledger. The unmeasured bound is a reservation ceiling, not observed spend. Not a billing statement; unknown usage is not zero."
        },
        "runtime": {
            "core_healthy": true,
            "workers": workers.len(),
            "active_agents": active_sessions,
            "unresolved_sessions": unresolved_sessions,
            "unresolved_model_calls": unresolved_model_call_count,
            "observability": app.observability.status(),
        },
        "recent_accepted": recent_accepted,
        "recent_decisions": recent_decisions,
        "recent_events": recent_events,
    }))
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
    let runs = match app.db.runs().await {
        Ok(runs) => runs
            .into_iter()
            .filter(|run| run.campaign_id == id)
            .collect::<Vec<_>>(),
        Err(error) => return api_error(error),
    };
    let run_ids: std::collections::BTreeSet<String> =
        runs.iter().map(|run| run.id.clone()).collect();
    let attempts = match app.db.attempts().await {
        Ok(attempts) => attempts
            .into_iter()
            .filter(|attempt| run_ids.contains(&attempt.run_id))
            .collect::<Vec<_>>(),
        Err(error) => return api_error(error),
    };
    let attempt_ids: std::collections::BTreeSet<String> =
        attempts.iter().map(|attempt| attempt.id.clone()).collect();
    let candidates = match app.db.candidates().await {
        Ok(candidates) => candidates
            .into_iter()
            .filter(|candidate| attempt_ids.contains(&candidate.attempt_id))
            .collect::<Vec<_>>(),
        Err(error) => return api_error(error),
    };
    let lead_requests = match app.db.lead_requests().await {
        Ok(requests) => requests,
        Err(error) => return api_error(error),
    };
    let related_requests = lead_requests
        .iter()
        .filter(|request| {
            request.campaign_id.as_deref() == Some(id.as_str())
                || tasks.iter().any(|task| task.request_id == request.id)
        })
        .collect::<Vec<_>>();
    let events = match app.db.events().await {
        Ok(events) => events,
        Err(error) => return api_error(error),
    };
    let mut related_subjects: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::from([id.clone()]);
    for request in &related_requests {
        related_subjects.insert(request.id.clone());
        related_subjects.insert(request.delegation_id.clone());
    }
    if let Some(authorization) = &authorization
        && !authorization.delegation_id.is_empty()
    {
        related_subjects.insert(authorization.delegation_id.clone());
    }
    for task in &tasks {
        related_subjects.insert(task.task_id.clone());
        related_subjects.insert(task.request_id.clone());
    }
    for run in &runs {
        related_subjects.insert(run.id.clone());
    }
    for attempt in &attempts {
        related_subjects.insert(attempt.id.clone());
    }
    for candidate in &candidates {
        related_subjects.insert(candidate.id.clone());
    }
    let events = events
        .into_iter()
        .filter(|event| related_subjects.contains(&event.subject_id))
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

async fn candidate_diff(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath((campaign_id, candidate_id)): AxumPath<(String, String)>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let campaign = match app.db.campaign(&campaign_id).await {
        Ok(campaign) => campaign,
        Err(error) => return api_error(error),
    };
    let candidate = match app.db.candidates().await {
        Ok(candidates) => candidates
            .into_iter()
            .find(|candidate| candidate.id == candidate_id),
        Err(error) => return api_error(error),
    };
    let Some(candidate) = candidate else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let attempt = match app.db.attempts().await {
        Ok(attempts) => attempts
            .into_iter()
            .find(|attempt| attempt.id == candidate.attempt_id),
        Err(error) => return api_error(error),
    };
    let Some(attempt) = attempt else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let run = match app.db.runs().await {
        Ok(runs) => runs.into_iter().find(|run| run.id == attempt.run_id),
        Err(error) => return api_error(error),
    };
    let Some(run) = run else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if run.campaign_id != campaign.id {
        return StatusCode::NOT_FOUND.into_response();
    }
    let action = match app.db.actions().await {
        Ok(actions) => actions
            .into_iter()
            .find(|action| action.id == campaign.action_id),
        Err(error) => return api_error(error),
    };
    let Some(action) = action else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tasks = match app.db.campaign_tasks(&campaign.id).await {
        Ok(tasks) => tasks,
        Err(error) => return api_error(error),
    };
    let base_commit = tasks
        .iter()
        .find(|task| task.candidate_id.as_deref() == Some(candidate.id.as_str()))
        .map(|task| task.base_commit.clone())
        .unwrap_or_else(|| action.spec.base_commit.clone());
    let repository = Path::new(&action.spec.repository);
    let changed_paths = match factory_native::workspace::changed_paths(
        repository,
        &base_commit,
        &candidate.commit_sha,
    ) {
        Ok(paths) => paths,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "candidate diff summary is unavailable for the recorded revision",
                    "candidate_id": candidate.id,
                })),
            )
                .into_response();
        }
    };
    let diff_stat =
        factory_native::workspace::diff_stat(repository, &base_commit, &candidate.commit_sha)
            .unwrap_or_else(|_| "diff stat unavailable".to_owned());
    let changed_paths_truncated = changed_paths.len() > 200;
    let changed_paths = changed_paths.into_iter().take(200).collect::<Vec<_>>();
    Json(json!({
        "candidate_id": candidate.id,
        "commit_sha": candidate.commit_sha,
        "tree_sha": candidate.tree_sha,
        "base_commit": base_commit,
        "changed_paths": changed_paths,
        "changed_paths_truncated": changed_paths_truncated,
        "diff_stat": diff_stat,
        "source": "Core read-only Git diff summary; file bodies are not exposed",
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
        .release_provider_turn(
            &input.reservation_id,
            &input.session_record_id,
            &input.worker_id,
            input.generation,
            "worker released turn",
        )
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

async fn provider_turn_reservation_budget(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match app.db.provider_turn_reservation_budget().await {
        Ok((reserved_turns, ceiling_micros)) => Json(json!({
            "reserved_turns": reserved_turns,
            "ceiling_micros": ceiling_micros,
        }))
        .into_response(),
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
    result_list(
        app.db
            .unresolved_provider_sessions_for_worker(Some(worker_id))
            .await,
    )
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

async fn unresolved_model_calls(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.unresolved_model_calls().await)
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
    if role == factory_native::db::ModelCallRole::SemanticReviewer {
        let settings = match app.db.factory_settings().await {
            Ok(settings) => settings,
            Err(error) => return api_error(error),
        };
        if !settings.semantic_review_enabled {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"semantic review is disabled by the operator"})),
            )
                .into_response();
        }
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
    let Some(gateway) = app.current_model_gateway().await else {
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

fn reviewer_receipt_mechanism_allowed(
    semantic_review_enabled: bool,
    gateway_configured: bool,
    mechanism: factory_native::db::ReviewMechanism,
) -> Result<(), &'static str> {
    if mechanism == factory_native::db::ReviewMechanism::Model {
        if !semantic_review_enabled {
            return Err("semantic review is disabled by the operator");
        }
        if !gateway_configured {
            return Err("semantic review requires a configured default model gateway");
        }
        return Ok(());
    }
    if semantic_review_enabled && gateway_configured {
        return Err("semantic review is enabled; reviewer must submit MODEL evidence");
    }
    Ok(())
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
    if role == QualityRole::Reviewer {
        let settings = match app.db.factory_settings().await {
            Ok(settings) => settings,
            Err(error) => return api_error(error),
        };
        let gateway_configured = app.current_model_gateway().await.is_some();
        if let Err(message) = reviewer_receipt_mechanism_allowed(
            settings.semantic_review_enabled,
            gateway_configured,
            input.mechanism,
        ) {
            let status = if message == "semantic review requires a configured default model gateway"
            {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::FORBIDDEN
            };
            return (status, Json(json!({"error": message}))).into_response();
        }
    }
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
    let current_gateway = app.current_model_gateway().await;
    if current_gateway.is_none() && !app.lead_fixture_assessor {
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
    let mechanism = if current_gateway.is_some() {
        "MODEL"
    } else {
        "FIXTURE"
    };
    let (mut assessment, mut proposal) = if let Some(gateway) = &current_gateway {
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
    let (plan, request) = match app
        .db
        .freeze_lead_plan_to_ready(&id, &format!("freeze:{id}"))
        .await
    {
        Ok(result) => result,
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
    body: Bytes,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let autonomy = match factory_native::db::ValidatedAutonomyContext::from_deployment_policy(
        app.lead_autonomous_start,
    ) {
        Ok(context) => context,
        Err(error) => return api_error(error),
    };
    let authorization = if body.is_empty() {
        None
    } else {
        match serde_json::from_slice::<AuthorizeCampaign>(&body) {
            Ok(input) => Some(input.into_new()),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"invalid campaign authorization payload"})),
                )
                    .into_response();
            }
        }
    };
    match app
        .db
        .start_lead_campaign(
            &id,
            &format!("start:{id}"),
            factory_native::db::StartAuthority::Autonomous(autonomy),
            None,
            authorization.as_ref(),
        )
        .await
    {
        Ok(result) => (
            StatusCode::CREATED,
            Json(json!({
                "campaign":result.campaign,
                "run":result.run,
                "attempt":result.attempt,
                "replayed":result.replayed
            })),
        )
            .into_response(),
        Err(error) => api_error(error),
    }
}

async fn approve_lead_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    session: Option<Extension<factory_native::db::DeviceSessionRecord>>,
    Json(input): Json<ApproveLeadRequest>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (principal_id, device_label) =
        authenticated_operator(session.as_ref().map(|Extension(device)| device));
    let principal_label = input.principal_label.as_deref().or(device_label.as_deref());
    let approval = match factory_native::db::AuthenticatedOperatorApproval::new(
        &principal_id,
        principal_label,
        &input.rationale,
    ) {
        Ok(approval) => approval,
        Err(error) => return api_error(error),
    };
    let expected = match factory_native::db::ExpectedStartBinding::new(
        &input.expected_request_hash,
        &input.expected_plan_hash,
        &input.expected_spec_sha256,
        &input.expected_config_hash,
        &input.expected_delegation_hash,
        input.profile_revision_id.as_deref(),
        input.profile_hash.as_deref(),
    ) {
        Ok(binding) => binding,
        Err(error) => return api_error(error),
    };
    let idempotency_key = input
        .idempotency_key
        .clone()
        .unwrap_or_else(|| format!("lead-approve:{id}:{}", input.expected_plan_hash));
    let provider_authorization = input
        .provider_authorization
        .map(AuthorizeCampaign::into_new);
    let result = app
        .db
        .start_lead_campaign(
            &id,
            &idempotency_key,
            factory_native::db::StartAuthority::Operator(approval),
            Some(&expected),
            provider_authorization.as_ref(),
        )
        .await;
    match result {
        Ok(result) => {
            let evidence = app.db.lead_start_evidence(&id).await.ok().flatten();
            (
                if result.replayed {
                    StatusCode::OK
                } else {
                    StatusCode::CREATED
                },
                Json(json!({
                    "campaign":result.campaign,
                    "run":result.run,
                    "attempt":result.attempt,
                    "replayed":result.replayed,
                    "evidence":evidence,
                })),
            )
                .into_response()
        }
        Err(error) => api_error(error),
    }
}

async fn withdraw_lead_request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    session: Option<Extension<factory_native::db::DeviceSessionRecord>>,
    Json(input): Json<WithdrawLeadRequest>,
) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (principal_id, device_label) =
        authenticated_operator(session.as_ref().map(|Extension(device)| device));
    let principal_label = input.principal_label.as_deref().or(device_label.as_deref());
    match app
        .db
        .withdraw_lead_request(&id, &principal_id, principal_label, &input.reason)
        .await
    {
        Ok(record) => Json(record).into_response(),
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
    let withdrawal = app.db.lead_withdrawal(&id).await.ok().flatten();
    let evidence = app.db.lead_start_evidence(&id).await.ok().flatten();
    let execution_binding = match &request.campaign_id {
        Some(campaign_id) => app
            .db
            .campaign_execution_binding(campaign_id)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let projection = app.db.lead_request_projection(&id).await.ok();
    let model_calls = match app.db.lead_model_calls(&id).await {
        Ok(calls) => calls,
        Err(error) => return api_error(error),
    };
    Json(json!({
        "request": request,
        "assessment": assessment,
        "proposal": proposal,
        "plan": plan,
        "withdrawal": withdrawal,
        "start_evidence": evidence,
        "execution_binding": execution_binding,
        "projection": projection,
        "model_calls": model_calls
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
    let authorization = input.into_new();
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

    #[test]
    fn browser_host_and_origin_policy_rejects_forgeries() {
        let trusted = vec!["https://factory-office.example.ts.net".to_string()];
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_static("factory-office.example.ts.net"),
        );
        assert!(request_host_allowed(&headers, &trusted));
        assert!(origin_allowed(
            "https://factory-office.example.ts.net",
            &headers,
            &trusted
        ));
        assert!(!origin_allowed(
            "https://factory-office.example.ts.net/evil",
            &headers,
            &trusted
        ));
        assert!(!origin_allowed(
            "https://factory-office.example.ts.net?x=1",
            &headers,
            &trusted
        ));
        assert!(!origin_allowed("https://evil.example", &headers, &trusted));
        assert!(!origin_allowed(
            "http://factory-office.example.ts.net",
            &headers,
            &trusted
        ));

        headers.insert(header::HOST, HeaderValue::from_static("attacker.example"));
        assert!(!request_host_allowed(&headers, &trusted));

        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8893"));
        assert!(request_host_allowed(&headers, &trusted));
        assert!(origin_allowed("http://127.0.0.1:8893", &headers, &trusted));
        assert!(!origin_allowed("http://127.0.0.1:9999", &headers, &trusted));
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
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
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
    async fn office_projections_require_operator_and_return_core_shapes() {
        use tower::ServiceExt;
        let path = std::env::temp_dir().join(format!(
            "factory-core-office-projections-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = Database::open(&path).await.unwrap();
        let app = Arc::new(App {
            db,
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
        });
        let router = build_router(app);
        for uri in [
            "/office/overview",
            "/office/decisions",
            "/campaigns/missing/candidates/missing/diff",
            "/model/calls/unresolved",
            "/provider/turn_reservations/budget",
        ] {
            let request = axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
        }
        let request = axum::http::Request::builder()
            .uri("/office/overview")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let overview: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(overview["counts"]["projects"], 0);
        assert_eq!(overview["runtime"]["core_healthy"], true);
        assert!(
            overview["as_of"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        let request = axum::http::Request::builder()
            .uri("/office/decisions")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let decisions: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(decisions["items"].as_array().map(Vec::len), Some(0));
        let request = axum::http::Request::builder()
            .uri("/campaigns/missing/candidates/missing/diff")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        let request = axum::http::Request::builder()
            .uri("/model/calls/unresolved")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
        let request = axum::http::Request::builder()
            .uri("/provider/turn_reservations/budget")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let turn_budget: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(turn_budget["reserved_turns"], 0);
        assert_eq!(turn_budget["ceiling_micros"], 0);
        let _ = std::fs::remove_file(path);
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
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
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
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
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

    async fn test_app(path: &Path) -> Arc<App> {
        Arc::new(App {
            db: Database::open(path).await.unwrap(),
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
        })
    }

    #[tokio::test]
    async fn browser_bootstrap_issues_cookie_session_and_enforces_csrf_and_revocation() {
        use tower::ServiceExt;

        let database_path = std::env::temp_dir().join(format!(
            "factory-core-browser-session-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let code_path = std::env::temp_dir().join(format!(
            "factory-core-bootstrap-code-{}",
            uuid::Uuid::new_v4()
        ));
        let code = "one-time-owner-confirmation";
        std::fs::write(
            &code_path,
            serde_json::to_vec(&BootstrapCodeRecord {
                code_sha256: factory_native::device_session::sha256_hex(code),
                expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
            })
            .unwrap(),
        )
        .unwrap();
        let app = Arc::new(App {
            db: Database::open(&database_path).await.unwrap(),
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            master_key: None,
            enrollment_code_file: Some(code_path.clone()),
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
        });
        let router = build_router(app.clone());
        let wrong_code = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/bootstrap")
                    .header("host", "localhost:8765")
                    .header("origin", "http://localhost:8765")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"code":"wrong-code","deviceName":"Desktop"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong_code.status(), StatusCode::UNAUTHORIZED);
        let missing_origin = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/bootstrap")
                    .header("host", "localhost:8765")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"code":code,"deviceName":"Desktop"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN);
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/bootstrap")
                    .header("host", "localhost:8765")
                    .header("origin", "http://localhost:8765")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"code":code,"deviceName":"Desktop"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect();
        assert_eq!(cookies.len(), 2);
        assert!(cookies.iter().any(|value| value.contains("HttpOnly")));
        let session_cookie = cookies
            .iter()
            .find(|value| value.starts_with("factory_session="))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf_cookie = cookies
            .iter()
            .find(|value| value.starts_with("factory_csrf="))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let status = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/projects")
                    .header("host", "localhost:8765")
                    .header("cookie", &session_cookie)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status.status(), StatusCode::OK);

        let unauthenticated = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/projects")
                    .header("host", "localhost:8765")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let reviewer_route = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/review/config")
                    .header("host", "localhost:8765")
                    .header("cookie", &session_cookie)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reviewer_route.status(), StatusCode::UNAUTHORIZED);
        let bad_origin = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/projects")
                    .header("host", "localhost:8765")
                    .header("origin", "http://attacker.example")
                    .header("cookie", &session_cookie)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);
        let bad_host = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/projects")
                    .header("host", "attacker.example")
                    .header("cookie", &session_cookie)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_host.status(), StatusCode::BAD_REQUEST);
        let ambiguous_auth = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/projects")
                    .header("host", "localhost:8765")
                    .header("cookie", &session_cookie)
                    .header(
                        "authorization",
                        "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
                    )
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ambiguous_auth.status(), StatusCode::BAD_REQUEST);

        let denied = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/logout")
                    .header("host", "localhost:8765")
                    .header("origin", "http://localhost:8765")
                    .header("cookie", format!("{session_cookie}; {csrf_cookie}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let csrf = csrf_cookie.split_once('=').unwrap().1;
        let logout = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/logout")
                    .header("host", "localhost:8765")
                    .header("origin", "http://localhost:8765")
                    .header("cookie", format!("{session_cookie}; {csrf_cookie}"))
                    .header(CSRF_HEADER, csrf)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        let revoked = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/projects")
                    .header("host", "localhost:8765")
                    .header("cookie", &session_cookie)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
        let recovered = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/auth/session")
                    .header("host", "localhost:8765")
                    .header("cookie", format!("{session_cookie}; {csrf_cookie}"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(recovered.status(), StatusCode::OK);
        assert_eq!(
            recovered
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
        let body = axum::body::to_bytes(recovered.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(session["authenticated"], false);
        app.db
            .reset_owner_bootstrap("local-recovery", chrono::Utc::now().timestamp_millis())
            .await
            .unwrap();
        let recovery_code = "one-time-recovery-confirmation";
        std::fs::write(
            &code_path,
            serde_json::to_vec(&BootstrapCodeRecord {
                code_sha256: factory_native::device_session::sha256_hex(recovery_code),
                expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
            })
            .unwrap(),
        )
        .unwrap();
        let reenrolled = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/bootstrap")
                    .header("host", "localhost:8765")
                    .header("origin", "http://localhost:8765")
                    .header("content-type", "application/json")
                    .header("cookie", format!("{session_cookie}; {csrf_cookie}"))
                    .body(axum::body::Body::from(
                        serde_json::json!({
                            "code": recovery_code,
                            "deviceName": "Recovered workstation"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reenrolled.status(), StatusCode::OK);
        let issued_cookies = reenrolled
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(issued_cookies.len(), 2);
        assert!(issued_cookies.iter().any(|value| {
            value.starts_with("factory_session=") && !value.contains("Max-Age=0")
        }));
        assert!(
            issued_cookies.iter().any(|value| {
                value.starts_with("factory_csrf=") && !value.contains("Max-Age=0")
            })
        );
        let cookie_header = issued_cookies
            .iter()
            .map(|value| value.split(';').next().unwrap())
            .collect::<Vec<_>>()
            .join("; ");
        let body = axum::body::to_bytes(reenrolled.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(session["authenticated"], true);
        let persisted = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/auth/session")
                    .header("host", "localhost:8765")
                    .header("cookie", cookie_header)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(persisted.status(), StatusCode::OK);
        let body = axum::body::to_bytes(persisted.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(session["authenticated"], true);
        let replay = router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/auth/bootstrap")
                    .header("host", "localhost:8765")
                    .header("origin", "http://localhost:8765")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({
                            "code": recovery_code,
                            "deviceName": "Replay"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::CONFLICT);
        let _ = std::fs::remove_file(database_path);
        let _ = std::fs::remove_file(code_path);
    }

    #[tokio::test]
    async fn office_access_listener_serves_office_and_denies_control_routes() {
        use tower::ServiceExt;

        let database_path = std::env::temp_dir().join(format!(
            "factory-core-office-access-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let office_directory = std::env::temp_dir().join(format!(
            "factory-core-office-assets-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(office_directory.join("assets")).unwrap();
        std::fs::write(office_directory.join("index.html"), "office-shell").unwrap();
        std::fs::write(office_directory.join("assets/app.js"), "office-asset").unwrap();
        let app = test_app(&database_path).await;
        let office_router =
            build_office_access_router(Arc::clone(&app), Some(office_directory.clone()));
        let api_only_router = build_office_access_router(Arc::clone(&app), None);

        for (uri, expected) in [("/", "office-shell"), ("/assets/app.js", "office-asset")] {
            let response = office_router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(uri)
                        .header("host", "localhost:8768")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(body, expected);
        }

        let office_projection = office_router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/office/overview")
                    .header("host", "localhost:8768")
                    .header("authorization", "Bearer operator-token")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(office_projection.status(), StatusCode::OK);

        let office_get_routes = [
            "/core-api/health",
            "/core-api/auth/session",
            "/core-api/auth/access",
            "/core-api/auth/devices",
            "/core-api/auth/devices/audit",
            "/core-api/setup/status",
            "/core-api/settings/providers",
            "/core-api/settings/runtime",
            "/core-api/settings/providers/opencode/audit",
            "/core-api/projects",
            "/core-api/projects/missing/config",
            "/core-api/actions",
            "/core-api/office/overview",
            "/core-api/office/decisions",
            "/core-api/office/participants",
            "/core-api/provider/sessions/missing/tool_events",
            "/core-api/campaigns",
            "/core-api/campaigns/missing",
            "/core-api/campaigns/missing/candidates/missing/diff",
            "/core-api/campaigns/missing/authorization",
            "/core-api/runs",
            "/core-api/attempts",
            "/core-api/events",
            "/core-api/observability",
            "/core-api/provider/turn_reservations/budget",
            "/core-api/provider/sessions",
            "/core-api/model/calls",
            "/core-api/model/calls/unresolved",
            "/core-api/quality/candidates/missing/receipts",
            "/core-api/quality/candidates/missing/gate",
            "/core-api/delegations",
            "/core-api/lead/requests",
            "/core-api/lead/requests/missing",
            "/core-api/candidates",
            "/core-api/workers",
        ];
        for uri in office_get_routes {
            let response = api_only_router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(uri)
                        .header("host", "localhost:8768")
                        .header("authorization", "Bearer operator-token")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(
                response
                    .headers()
                    .get("x-factory-office-route-miss")
                    .is_none(),
                "Office route was not registered: {uri}"
            );
        }
        let office_write_routes = [
            ("PUT", "/core-api/settings/runtime"),
            ("PUT", "/core-api/settings/providers/opencode"),
            ("POST", "/core-api/settings/providers/unknown/test"),
            ("DELETE", "/core-api/settings/providers/unknown/credential"),
            ("POST", "/core-api/projects"),
            ("PUT", "/core-api/projects/missing/config"),
            ("POST", "/core-api/actions"),
            ("POST", "/core-api/campaigns"),
            ("POST", "/core-api/campaigns/missing/cancel"),
            ("POST", "/core-api/campaigns/missing/authorize"),
            ("POST", "/core-api/campaigns/missing/runs"),
            ("POST", "/core-api/runs/missing/attempts"),
            ("POST", "/core-api/delegations"),
            ("POST", "/core-api/delegations/missing"),
            ("POST", "/core-api/lead/requests"),
            ("POST", "/core-api/lead/requests/missing/assess"),
            ("POST", "/core-api/lead/requests/missing/freeze"),
            ("POST", "/core-api/lead/requests/missing/start"),
            ("POST", "/core-api/lead/requests/missing/approve"),
            ("POST", "/core-api/lead/requests/missing/withdraw"),
            ("POST", "/core-api/model/calls/missing/resolve"),
            ("POST", "/core-api/auth/bootstrap"),
            ("POST", "/core-api/auth/device-challenges"),
            ("POST", "/core-api/auth/device-challenges/missing/cancel"),
            ("POST", "/core-api/auth/device-claims/missing"),
            ("POST", "/core-api/auth/device-claims/missing/poll"),
            ("POST", "/core-api/auth/devices/missing/approve"),
            ("POST", "/core-api/auth/devices/missing/reject"),
            ("DELETE", "/core-api/auth/devices/missing"),
            ("POST", "/core-api/auth/logout"),
            ("POST", "/core-api/auth/logout-all"),
            ("POST", "/core-api/auth/rotate"),
        ];
        for (method, uri) in office_write_routes {
            let response = api_only_router
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("host", "localhost:8768")
                        .header("authorization", "Bearer operator-token")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(
                response
                    .headers()
                    .get("x-factory-office-route-miss")
                    .is_none(),
                "Office route was not registered: {method} {uri}"
            );
        }

        let denied = [
            (
                "/worker/register",
                "Bearer bootstrap-bootstrap-bootstrap-1234",
            ),
            (
                "/core-api/worker/register",
                "Bearer bootstrap-bootstrap-bootstrap-1234",
            ),
            (
                "/quality/receipts",
                "Bearer verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "/core-api/quality/receipts",
                "Bearer verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "/review/config",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "/core-api/review/config",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            (
                "/supervisor/workspace_operation_valid",
                "Bearer operator-token",
            ),
            (
                "/core-api/supervisor/workspace_operation_valid",
                "Bearer operator-token",
            ),
        ];
        for (uri, credential) in denied {
            let mut request = axum::http::Request::builder()
                .method(if uri.ends_with("/review/config") {
                    "GET"
                } else {
                    "POST"
                })
                .uri(uri)
                .header("host", "localhost:8768")
                .header("authorization", credential);
            let body = if uri.ends_with("/review/config") {
                axum::body::Body::empty()
            } else {
                request = request.header("content-type", "application/json");
                axum::body::Body::from("{}")
            };
            let response = office_router
                .clone()
                .oneshot(request.body(body).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        }

        let local_router = build_router(app);
        let local_worker_route = local_router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/worker/register")
                    .header("host", "localhost:8765")
                    .header("authorization", "Bearer bootstrap-bootstrap-bootstrap-1234")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(local_worker_route.status(), StatusCode::NOT_FOUND);

        drop(office_router);
        let _ = std::fs::remove_file(database_path);
        let _ = std::fs::remove_dir_all(office_directory);
    }

    #[tokio::test]
    async fn office_access_listener_pairs_and_revokes_devices_with_operator_sessions() {
        use tower::ServiceExt;
        let path = std::env::temp_dir().join(format!(
            "factory-core-office-pairing-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let app = test_app(&path).await;
        let owner = factory_native::device_session::issue_owner(
            &app.db,
            "Desktop",
            chrono::Utc::now().timestamp_millis(),
        )
        .await
        .unwrap();
        let session_cookie = format!("factory_session={}", owner.session_token);
        let csrf_cookie = format!("factory_csrf={}", owner.csrf_token);
        let both_cookies = format!("{session_cookie}; {csrf_cookie}");
        let router = build_office_access_router(Arc::clone(&app), None);

        let empty = || axum::body::Body::empty();
        let unauthenticated = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/auth/access")
                    .header("host", "localhost:8768")
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let access = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/auth/access")
                    .header("host", "localhost:8768")
                    .header("cookie", &session_cookie)
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(access.status(), StatusCode::OK);

        let devices = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/auth/devices")
                    .header("host", "localhost:8768")
                    .header("cookie", &session_cookie)
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(devices.status(), StatusCode::OK);
        let devices = axum::body::to_bytes(devices.into_body(), usize::MAX)
            .await
            .unwrap();
        let devices: Value = serde_json::from_slice(&devices).unwrap();
        assert_eq!(devices["devices"][0]["current"], true);

        let challenge = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/core-api/auth/device-challenges")
                    .header("host", "localhost:8768")
                    .header("origin", "http://localhost:8768")
                    .header("cookie", &both_cookies)
                    .header(CSRF_HEADER, &owner.csrf_token)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(challenge.status(), StatusCode::OK);
        let challenge = axum::body::to_bytes(challenge.into_body(), usize::MAX)
            .await
            .unwrap();
        let challenge: Value = serde_json::from_slice(&challenge).unwrap();
        let challenge_id = challenge["challengeId"].as_str().unwrap().to_owned();
        let verification_code = challenge["verificationCode"].as_str().unwrap().to_owned();

        let claim = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/core-api/auth/device-claims/{challenge_id}"))
                    .header("host", "localhost:8768")
                    .header("origin", "http://localhost:8768")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"deviceName":"Phone"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(claim.status(), StatusCode::OK);
        let claim = axum::body::to_bytes(claim.into_body(), usize::MAX)
            .await
            .unwrap();
        let claim: Value = serde_json::from_slice(&claim).unwrap();
        assert_eq!(claim["status"], "PENDING");
        let poll_token = claim["pollToken"].as_str().unwrap().to_owned();

        let approve = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/core-api/auth/devices/{challenge_id}/approve"))
                    .header("host", "localhost:8768")
                    .header("origin", "http://localhost:8768")
                    .header("cookie", &both_cookies)
                    .header(CSRF_HEADER, &owner.csrf_token)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"verificationCode": verification_code}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(approve.status(), StatusCode::OK);

        let poll = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/core-api/auth/device-claims/{challenge_id}/poll"))
                    .header("host", "localhost:8768")
                    .header("origin", "http://localhost:8768")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"pollToken": poll_token}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(poll.status(), StatusCode::OK);
        let phone_cookie = poll
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("factory_session="))
            .and_then(|value| value.split(';').next())
            .unwrap()
            .to_owned();
        let poll_body = axum::body::to_bytes(poll.into_body(), usize::MAX)
            .await
            .unwrap();
        let poll_body: Value = serde_json::from_slice(&poll_body).unwrap();
        assert_eq!(poll_body["status"], "APPROVED");

        let devices = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/auth/devices")
                    .header("host", "localhost:8768")
                    .header("cookie", &session_cookie)
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let devices = axum::body::to_bytes(devices.into_body(), usize::MAX)
            .await
            .unwrap();
        let devices: Value = serde_json::from_slice(&devices).unwrap();
        let phone = devices["devices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|device| device["name"] == "Phone")
            .expect("phone session is listed");
        assert_eq!(phone["current"], false);
        let phone_id = phone["id"].as_str().unwrap().to_owned();

        let revoke = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("DELETE")
                    .uri(format!("/core-api/auth/devices/{phone_id}"))
                    .header("host", "localhost:8768")
                    .header("origin", "http://localhost:8768")
                    .header("cookie", &both_cookies)
                    .header(CSRF_HEADER, &owner.csrf_token)
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoke.status(), StatusCode::NO_CONTENT);

        let revoked = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/auth/devices")
                    .header("host", "localhost:8768")
                    .header("cookie", &phone_cookie)
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);

        let audit = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/core-api/auth/devices/audit")
                    .header("host", "localhost:8768")
                    .header("cookie", &session_cookie)
                    .body(empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(audit.status(), StatusCode::OK);
        let audit = axum::body::to_bytes(audit.into_body(), usize::MAX)
            .await
            .unwrap();
        let audit = String::from_utf8(audit.to_vec()).unwrap();
        assert!(audit.contains("approved"));
        assert!(audit.contains("revoked"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn participant_routes_require_operator_credentials_and_validate_filters() {
        use tower::ServiceExt;
        let path = std::env::temp_dir().join(format!(
            "factory-core-participant-routes-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let router = build_router(test_app(&path).await);
        for uri in [
            "/office/participants",
            "/provider/sessions/missing/tool_events",
        ] {
            for bearer in [
                None,
                Some("Bearer wrong-token"),
                Some("Bearer bootstrap-bootstrap-bootstrap-1234"),
                Some("Bearer verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa"),
                Some("Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb"),
            ] {
                let mut request = axum::http::Request::builder().uri(uri);
                if let Some(bearer) = bearer {
                    request = request.header("authorization", bearer);
                }
                assert_eq!(
                    router
                        .clone()
                        .oneshot(request.body(axum::body::Body::empty()).unwrap())
                        .await
                        .unwrap()
                        .status(),
                    StatusCode::UNAUTHORIZED,
                    "{uri} with {bearer:?}"
                );
            }
        }

        for uri in [
            "/office/participants?role=ADMIN",
            "/office/participants?status=NOT_A_STATUS",
            "/office/participants?limit=0",
            "/office/participants?limit=201",
            "/office/participants?offset=10001",
            "/office/participants?campaign_id=",
            "/provider/sessions/missing/tool_events?after_sequence=-1",
            "/provider/sessions/missing/tool_events?limit=201",
        ] {
            let request = axum::http::Request::builder()
                .uri(uri)
                .header("authorization", "Bearer operator-token")
                .body(axum::body::Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                StatusCode::BAD_REQUEST,
                "{uri}"
            );
        }

        let request = axum::http::Request::builder()
            .uri("/office/participants?limit=1&role=implementer&status=active")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body["participants"].is_array());
        assert_eq!(body["page"]["limit"], 1);
        assert_eq!(
            body["page"]["order"],
            "last_observed_at DESC, participant_id ASC"
        );

        let request = axum::http::Request::builder()
            .uri("/provider/sessions/missing/tool_events")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::NOT_FOUND
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
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
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

    #[tokio::test]
    async fn semantic_review_toggle_is_authoritative_for_receipts() {
        use tower::ServiceExt;
        let path = std::env::temp_dir().join(format!(
            "factory-core-semantic-toggle-{}.sqlite",
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
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
        });
        let router = build_router(Arc::clone(&app));
        let disabled_model_call = axum::http::Request::builder()
            .method("POST")
            .uri("/model/calls")
            .header(
                "authorization",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({
                    "role": "semantic_reviewer",
                    "principalId": "reviewer",
                    "projectId": "project-1",
                    "objectiveId": "candidate-1",
                    "scopeId": "candidate-1",
                    "purpose": "semantic_review",
                    "systemPrompt": "system",
                    "userPrompt": "user",
                    "maxOutputTokens": 128
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            router
                .clone()
                .oneshot(disabled_model_call)
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );

        let config = axum::http::Request::builder()
            .uri("/review/config")
            .header(
                "authorization",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(config).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("DETERMINISTIC"));

        let enable = axum::http::Request::builder()
            .method("PUT")
            .uri("/settings/runtime")
            .header("authorization", "Bearer operator-token")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"semanticReviewEnabled":true,"semanticReviewMaxOutputTokens":2048}"#,
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(enable).await.unwrap().status(),
            StatusCode::OK
        );
        let enabled_model_call = axum::http::Request::builder()
            .method("POST")
            .uri("/model/calls")
            .header(
                "authorization",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({
                    "role": "semantic_reviewer",
                    "principalId": "reviewer",
                    "projectId": "project-1",
                    "objectiveId": "candidate-1",
                    "scopeId": "candidate-1",
                    "purpose": "semantic_review",
                    "systemPrompt": "system",
                    "userPrompt": "user",
                    "maxOutputTokens": 128
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            router
                .clone()
                .oneshot(enabled_model_call)
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        let receipt = axum::http::Request::builder()
            .method("POST")
            .uri("/quality/receipts")
            .header(
                "authorization",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({
                    "candidate_id": "candidate-1",
                    "receipt_id": "receipt-1",
                    "principal_id": "reviewer",
                    "actionSpecSha256": "a".repeat(64),
                    "commitSha": "b".repeat(40),
                    "treeSha": "c".repeat(40),
                    "result": "PASS",
                    "evidence": "EXECUTED",
                    "findings": [],
                    "mechanism": "MODEL"
                })
                .to_string(),
            ))
            .unwrap();
        let response = router.clone().oneshot(receipt).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let disable = axum::http::Request::builder()
            .method("PUT")
            .uri("/settings/runtime")
            .header("authorization", "Bearer operator-token")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                r#"{"semanticReviewEnabled":false,"semanticReviewMaxOutputTokens":2048}"#,
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(disable).await.unwrap().status(),
            StatusCode::OK
        );
        let disabled_receipt = axum::http::Request::builder()
            .method("POST")
            .uri("/quality/receipts")
            .header(
                "authorization",
                "Bearer reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb",
            )
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({
                    "candidate_id": "candidate-1",
                    "receipt_id": "receipt-2",
                    "principal_id": "reviewer",
                    "actionSpecSha256": "a".repeat(64),
                    "commitSha": "b".repeat(40),
                    "treeSha": "c".repeat(40),
                    "result": "PASS",
                    "evidence": "EXECUTED",
                    "findings": [],
                    "mechanism": "MODEL"
                })
                .to_string(),
            ))
            .unwrap();
        assert_eq!(
            router.oneshot(disabled_receipt).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        drop(app);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn enabled_semantic_review_requires_model_reviewer_evidence() {
        use factory_native::db::ReviewMechanism::{Deterministic, Model};
        assert!(reviewer_receipt_mechanism_allowed(true, true, Model).is_ok());
        assert!(
            reviewer_receipt_mechanism_allowed(true, true, Deterministic)
                .unwrap_err()
                .contains("must submit MODEL evidence")
        );
        assert!(reviewer_receipt_mechanism_allowed(true, false, Deterministic).is_ok());
        assert!(reviewer_receipt_mechanism_allowed(false, true, Deterministic).is_ok());
        assert!(reviewer_receipt_mechanism_allowed(false, false, Deterministic).is_ok());
        assert!(
            reviewer_receipt_mechanism_allowed(false, true, Model)
                .unwrap_err()
                .contains("disabled")
        );
        assert!(
            reviewer_receipt_mechanism_allowed(true, false, Model)
                .unwrap_err()
                .contains("configured default model gateway")
        );
    }

    fn approval_checks() -> Vec<factory_native::lead::SpecCheck> {
        use factory_native::lead::{CheckKind, SpecCheck};
        vec![
            SpecCheck {
                id: "format".into(),
                kind: CheckKind::Format,
                description: "format".into(),
                command: vec!["cargo".into(), "fmt".into(), "--check".into()],
                expect_exit: 0,
                timeout_secs: 60,
            },
            SpecCheck {
                id: "build".into(),
                kind: CheckKind::Build,
                description: "build".into(),
                command: vec!["cargo".into(), "build".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "test".into(),
                kind: CheckKind::Test,
                description: "test".into(),
                command: vec!["cargo".into(), "test".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "functional".into(),
                kind: CheckKind::Functional,
                description: "functional".into(),
                command: vec!["cargo".into(), "run".into()],
                expect_exit: 0,
                timeout_secs: 120,
            },
            SpecCheck {
                id: "negative".into(),
                kind: CheckKind::Negative,
                description: "negative".into(),
                command: vec!["cargo".into(), "run".into(), "--bad".into()],
                expect_exit: 1,
                timeout_secs: 60,
            },
        ]
    }

    async fn create_ready_approval_lead(
        db: &Database,
        project_id: &str,
        config_hash: &str,
        grant: &StandingDelegation,
        label: &str,
    ) -> (String, Value) {
        let checks = approval_checks();
        let allowed = vec!["src".to_string()];
        let action = db
            .create_action_with_spec(
                project_id,
                "Implement the approval endpoint.",
                "opencode",
                "deepseek-v4.1-flash",
                &format!("approval-action-{label}"),
                factory_native::db::ActionSpecInput {
                    allowed_paths: &allowed,
                    checks: &checks,
                },
            )
            .await
            .unwrap();
        let (request, _) = db
            .create_lead_request(
                project_id,
                &action.id,
                &grant.id,
                "Implement the approval endpoint.",
                config_hash,
                &format!("approval-request-{label}"),
            )
            .await
            .unwrap();
        db.begin_lead_assessment(&request.id).await.unwrap();
        let config = db.project_config(project_id).await.unwrap();
        let binding = factory_native::lead::ActionBinding {
            id: action.id.clone(),
            project_id: project_id.into(),
            status: factory_native::lead::ActionStatus::Pending,
        };
        let (assessment, proposal) = factory_native::lead::fixture_lead_assessment(
            &request.id,
            &config,
            &binding,
            &action.spec,
            &request.objective,
            grant,
            chrono::Utc::now(),
        )
        .unwrap();
        let input = factory_native::lead::AssessmentInput {
            project_id: project_id.into(),
            action_id: action.id.clone(),
            request_id: request.id.clone(),
            objective: request.objective.clone(),
            allowed_evidence_refs: std::collections::BTreeSet::from(["repository-files".into()]),
        };
        db.record_lead_assessment(
            &request.id,
            "FIXTURE",
            &assessment,
            &input,
            &format!("approval-assessment-{label}"),
        )
        .await
        .unwrap();
        db.record_lead_proposal(
            &request.id,
            &proposal,
            &format!("approval-proposal-{label}"),
        )
        .await
        .unwrap();
        let (plan, _) = db
            .freeze_lead_plan_to_ready(&request.id, &format!("approval-freeze-{label}"))
            .await
            .unwrap();
        (
            request.id,
            json!({
                "expected_request_hash": request.request_hash,
                "expected_plan_hash": plan.plan_hash,
                "expected_spec_sha256": action.spec_sha256,
                "expected_config_hash": config_hash,
                "expected_delegation_hash": grant.content_hash,
                "principal_label": "Alice",
                "rationale": "Reviewed the frozen plan.",
            }),
        )
    }

    #[tokio::test]
    async fn operator_approval_endpoint_is_authenticated_and_replay_safe() {
        use tower::ServiceExt;

        let path = std::env::temp_dir().join(format!(
            "factory-core-approval-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let repository = std::env::temp_dir().join(format!(
            "factory-core-approval-repo-{}",
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
        let db = Database::open(&path).await.unwrap();
        let project = db
            .register_project(&repository, "approval-project")
            .await
            .unwrap();
        let config = db
            .update_project_config(
                &project.id,
                &factory_native::db::NewProjectConfig {
                    expected_revision: 1,
                    allowed_paths: vec!["src".into()],
                    capabilities: vec![
                        factory_native::lead::Capability::RepositoryEdit,
                        factory_native::lead::Capability::GenericToolchain,
                    ],
                    quality_profile: factory_native::lead::QualityProfile::Generic,
                    active: true,
                },
            )
            .await
            .unwrap();
        let mut grant = StandingDelegation {
            id: "approval-grant".into(),
            principal_id: "owner".into(),
            audit_id: "audit".into(),
            project_id: project.id.clone(),
            revision: 1,
            content_hash: String::new(),
            objective_classes: vec![factory_native::lead::ObjectiveClass::Feature],
            allowed_paths: vec!["src".into()],
            capabilities: vec![
                factory_native::lead::Capability::RepositoryEdit,
                factory_native::lead::Capability::GenericToolchain,
            ],
            risk_classes: vec![factory_native::lead::RiskClass::R0],
            autonomous_start: false,
            autonomous_risk_classes: Vec::new(),
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
            expires_at: chrono::Utc::now() + chrono::Duration::days(1),
            revoked_at: None,
        };
        grant.content_hash = grant.compute_content_hash();
        db.create_standing_delegation(&grant, "approval-grant-key")
            .await
            .unwrap();
        let (request_id, expected) =
            create_ready_approval_lead(&db, &project.id, &config.config_hash, &grant, "first")
                .await;
        let (withdrawn_request, _) =
            create_ready_approval_lead(&db, &project.id, &config.config_hash, &grant, "second")
                .await;
        let app = Arc::new(App {
            db,
            operator_token: "operator-token".into(),
            worker_bootstrap_token: "bootstrap-bootstrap-bootstrap-1234".into(),
            verifier_token: "verifier-token-aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            reviewer_token: "reviewer-token-bbbbbbbbbbbbbbbbbbbbbbbb".into(),
            lead_fixture_assessor: false,
            lead_autonomous_start: false,
            model_gateway: None,
            master_key: None,
            enrollment_code_file: None,
            projects_root: None,
            allow_insecure_git: false,
            setup: Mutex::new(SetupState {
                failures: 0,
                blocked_until_ms: 0,
            }),
            attempt_grant_key: b"attempt-grant-key-0123456789abcdef".to_vec(),
            observability: factory_native::observability::ExporterHandle::disabled(),
        });
        let router = build_router(app.clone());
        let approve_uri = format!("/lead/requests/{request_id}/approve");
        let mut body = expected.clone();
        body["idempotency_key"] = json!("api-approval");
        let request_body = serde_json::to_vec(&body).unwrap();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&approve_uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(request_body.clone()))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&approve_uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer bootstrap-bootstrap-bootstrap-1234")
            .body(axum::body::Body::from(request_body.clone()))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let mut forged = body.clone();
        forged["actor"] = json!("forged-operator");
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&approve_uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(serde_json::to_vec(&forged).unwrap()))
            .unwrap();
        assert!(
            router
                .clone()
                .oneshot(request)
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&approve_uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(request_body.clone()))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let approved: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let campaign_id = approved["campaign"]["id"].as_str().unwrap().to_string();
        assert_eq!(approved["evidence"]["principal_id"], "operator-token");
        assert_eq!(approved["evidence"]["mode"], "OPERATOR");

        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&approve_uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(request_body))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut second_key = body.clone();
        second_key["idempotency_key"] = json!("api-approval-second");
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(&approve_uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(
                serde_json::to_vec(&second_key).unwrap(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let conflict: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(conflict["campaign_id"], campaign_id);

        let request = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/lead/requests/{request_id}/withdraw"))
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({"reason":"stop"})).unwrap(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let conflict: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(conflict["campaign_id"], campaign_id);

        let request = axum::http::Request::builder()
            .method("POST")
            .uri(format!("/lead/requests/{withdrawn_request}/withdraw"))
            .header("content-type", "application/json")
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({"reason":"stop","principal_label":"Alice"})).unwrap(),
            ))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let withdrawal: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(withdrawal["principal_id"], "operator-token");
        let request = axum::http::Request::builder()
            .uri(format!("/lead/requests/{withdrawn_request}"))
            .header("authorization", "Bearer operator-token")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let projection: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(projection["projection"], "WITHDRAWN");
        app.db.close().await;
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(repository);
    }
}
