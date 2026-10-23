use std::{env, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use factory_native::db::{Database, Error};
use serde::Deserialize;
use serde_json::json;

struct App {
    db: Database,
    operator_token: String,
    worker_token: String,
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
struct Claim {
    worker_id: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("factory_native=info")
        .init();
    let database = env::var("FACTORY_DATABASE").unwrap_or_else(|_| "factory-native.sqlite".into());
    let operator_token =
        env::var("FACTORY_OPERATOR_TOKEN").map_err(|_| "FACTORY_OPERATOR_TOKEN required")?;
    let worker_token =
        env::var("FACTORY_WORKER_TOKEN").map_err(|_| "FACTORY_WORKER_TOKEN required")?;
    if operator_token.is_empty() || worker_token.is_empty() || operator_token == worker_token {
        return Err("operator and worker tokens must be distinct, nonempty".into());
    }
    let bind = env::var("FACTORY_BIND").unwrap_or_else(|_| "127.0.0.1:8765".into());
    let addr: SocketAddr = bind.parse()?;
    if !addr.ip().is_loopback() {
        return Err("remote binding requires a separate authenticated transport design".into());
    }
    let app = Arc::new(App {
        db: Database::open(Path::new(&database))?,
        operator_token,
        worker_token,
    });
    let router = Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"healthy":true,"version":"0.1.0"})) }),
        )
        .route("/projects", get(projects).post(register_project))
        .route("/actions", get(actions).post(create_action))
        .route("/events", get(events))
        .route("/worker/hello", post(worker_hello))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "native Factory Core listening");
    axum::serve(listener, router).await?;
    Ok(())
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {token}").as_str())
}

fn api_error(error: Error) -> Response {
    let status = match error {
        Error::Invalid(_) | Error::Repository(_) => StatusCode::BAD_REQUEST,
        Error::NotFound => StatusCode::NOT_FOUND,
        Error::Sql(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
    {
        Ok(project) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(error) => api_error(error),
    }
}
async fn projects(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.projects())
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
    match app.db.create_action(
        &input.project_id,
        &input.objective,
        &input.provider,
        &input.model,
        &input.idempotency_key,
    ) {
        Ok(action) => (StatusCode::CREATED, Json(action)).into_response(),
        Err(error) => api_error(error),
    }
}
async fn actions(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.actions())
}
async fn events(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.operator_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    result_list(app.db.events())
}
async fn worker_hello(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Claim>,
) -> Response {
    if !authorized(&headers, &app.worker_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if input.worker_id.trim().is_empty() {
        return api_error(Error::Invalid("worker_id"));
    }
    // No execution authority, frozen ActionSpec, or sandbox grant exists in this slice.
    // A worker can authenticate, but Core cannot dispatch any Action yet.
    Json(json!({"authenticated":true,"dispatch_supported":false})).into_response()
}
fn result_list<T: serde::Serialize>(result: Result<Vec<T>, Error>) -> Response {
    match result {
        Ok(items) => Json(items).into_response(),
        Err(error) => api_error(error),
    }
}
