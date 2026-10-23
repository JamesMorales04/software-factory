//! Trusted local Sandbox Supervisor HTTP service.
//!
//! This is a candidate `TRUSTED_LOCAL` supervisor component, **not certified
//! isolation**. It accepts a narrow fixture-execution contract from a trusted
//! local caller, then delegates to [`factory_native::sandbox::RootlessPodman`]
//! so that only hardened, allowlisted rootless Podman fixtures run. Callers
//! cannot supply Podman flags, mounts, sockets, networks, environment variables
//! or an executable outside the requested `argv`.
//!
//! Failures are closed: the service refuses to start without a sufficiently
//! long bearer token, a private workspace root, a non-empty image allowlist and
//! a confirmed rootless Podman engine. There is no fallback to host execution,
//! Docker, rootful Podman or host networking, and the service exposes no
//! provider, GitHub or Core credentials and no Podman socket.
//!
//! Fixtures receive only the named workspace mount. Container `HOME` and
//! `TMPDIR` are fixed to `/tmp`; host `HOME`, runtime sockets and ambient host
//! environment are not forwarded into the container.
//!
//! Residual risks remain blockers for isolation certification: fixtures share
//! the supervisor user's UID, so same-UID workspace replacement races are not
//! eliminated, and credential confinement has not been independently verified.
//! Loopback binding, a bearer token and container hardening are not equivalent
//! to OS-level isolation.

use std::{
    collections::BTreeSet,
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use factory_native::sandbox::{MAX_RUNTIME, RootlessPodman, SandboxError, is_valid_workspace_name};
use serde::{Deserialize, Serialize};
use serde_json::json;

const DEFAULT_BIND: &str = "127.0.0.1:8766";
const MIN_TOKEN_BYTES: usize = 32;
const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_ARGV_ITEMS: usize = 256;
const MAX_ARG_BYTES: usize = 16 * 1024;
const MAX_ARGV_BYTES: usize = 64 * 1024;

const OTHER_SECRET_VARIABLES: [&str; 5] = [
    "FACTORY_OPERATOR_TOKEN",
    "FACTORY_WORKER_BOOTSTRAP_TOKEN",
    "FACTORY_WORKER_REGISTRATION_KEY",
    "FACTORY_VERIFIER_TOKEN",
    "FACTORY_REVIEWER_TOKEN",
];

struct App {
    token: String,
    workspace_root: PathBuf,
    allowed_images: BTreeSet<String>,
    max_timeout: Duration,
    sandbox: RootlessPodman,
    run_gate: tokio::sync::Mutex<()>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteRequest {
    workspace_name: String,
    image: String,
    argv: Vec<String>,
    timeout_secs: u64,
}

#[derive(Serialize)]
struct ExecuteResponse {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("factory_native=info")
        .init();
    let token =
        env::var("FACTORY_SUPERVISOR_TOKEN").map_err(|_| "FACTORY_SUPERVISOR_TOKEN required")?;
    validate_supervisor_token(&token)?;
    ensure_distinct_from_other_roles(&token)?;
    let workspace_root = env::var("FACTORY_SUPERVISOR_WORKSPACE_ROOT")
        .map_err(|_| "FACTORY_SUPERVISOR_WORKSPACE_ROOT required")?;
    let workspace_root = Path::new(&workspace_root).canonicalize()?;
    let images = parse_images(
        &env::var("FACTORY_SUPERVISOR_IMAGES").map_err(|_| "FACTORY_SUPERVISOR_IMAGES required")?,
    )?;
    let sandbox = RootlessPodman::new(&workspace_root, images.iter().cloned(), MAX_RUNTIME)?;
    sandbox.preflight().await?;
    let bind = env::var("FACTORY_SUPERVISOR_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    let addr: SocketAddr = bind.parse()?;
    if !addr.ip().is_loopback() {
        return Err("sandbox supervisor may only bind to a loopback address".into());
    }
    let app = Arc::new(App {
        token,
        workspace_root,
        allowed_images: images,
        max_timeout: MAX_RUNTIME,
        sandbox,
        run_gate: tokio::sync::Mutex::new(()),
    });
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "Factory sandbox supervisor listening");
    axum::serve(listener, build_router(app)).await?;
    Ok(())
}

fn build_router(app: Arc<App>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/execute", post(execute))
        .route("/v1/reconcile", post(reconcile))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(app)
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({"healthy": true}))
}

async fn execute(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<ExecuteRequest>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(message) = validate_execute(&app, &input) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response();
    }
    let workspace = app.workspace_root.join(&input.workspace_name);
    let timeout = Duration::from_secs(input.timeout_secs);
    let _run_guard = app.run_gate.lock().await;
    let sandbox = match RootlessPodman::new(
        &app.workspace_root,
        app.allowed_images.iter().cloned(),
        timeout,
    ) {
        Ok(sandbox) => sandbox,
        Err(error) => return sandbox_error(error),
    };
    match sandbox
        .run_fixture_capture(&workspace, &input.image, &input.argv)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(ExecuteResponse {
                exit_code: result.exit_code,
                stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
                stdout_truncated: result.stdout_truncated,
                stderr_truncated: result.stderr_truncated,
            }),
        )
            .into_response(),
        Err(error) => sandbox_error(error),
    }
}

async fn reconcile(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let _run_guard = app.run_gate.lock().await;
    match app.sandbox.reconcile_orphans().await {
        Ok(removed) => (StatusCode::OK, Json(json!({"removed": removed}))).into_response(),
        Err(error) => sandbox_error(error),
    }
}

fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
}

fn validate_execute(app: &App, input: &ExecuteRequest) -> Result<(), String> {
    if !is_valid_workspace_name(&input.workspace_name) {
        return Err("workspace_name must be a direct child directory name".into());
    }
    if !app.allowed_images.contains(&input.image) {
        return Err("image is not in the configured exact allowlist".into());
    }
    if input.argv.is_empty() || input.argv[0].is_empty() {
        return Err("argv must contain an executable".into());
    }
    if input.argv.len() > MAX_ARGV_ITEMS {
        return Err(format!("argv must contain at most {MAX_ARGV_ITEMS} items"));
    }
    let mut total_bytes = 0usize;
    for argument in &input.argv {
        if argument.len() > MAX_ARG_BYTES {
            return Err(format!(
                "each argv item must be at most {MAX_ARG_BYTES} bytes"
            ));
        }
        if argument.as_bytes().contains(&0) {
            return Err("argv must not contain NUL bytes".into());
        }
        total_bytes = total_bytes.saturating_add(argument.len());
    }
    if total_bytes > MAX_ARGV_BYTES {
        return Err(format!("argv must total at most {MAX_ARGV_BYTES} bytes"));
    }
    if input.timeout_secs == 0 || input.timeout_secs > app.max_timeout.as_secs() {
        return Err(format!(
            "timeout_secs must be between 1 and {}",
            app.max_timeout.as_secs()
        ));
    }
    Ok(())
}

fn sandbox_error(error: SandboxError) -> Response {
    tracing::warn!(error = %error, "sandbox supervisor request failed");
    let status = match &error {
        SandboxError::Invalid(_) => StatusCode::BAD_REQUEST,
        SandboxError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        SandboxError::TimedOut => StatusCode::GATEWAY_TIMEOUT,
        SandboxError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        SandboxError::Io(_)
        | SandboxError::Failed { .. }
        | SandboxError::Reader(_)
        | SandboxError::Ledger(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error": error.to_string()}))).into_response()
}

fn validate_supervisor_token(token: &str) -> Result<(), String> {
    if token.len() < MIN_TOKEN_BYTES {
        return Err(format!(
            "FACTORY_SUPERVISOR_TOKEN must be at least {MIN_TOKEN_BYTES} bytes"
        ));
    }
    Ok(())
}

fn ensure_distinct_from_other_roles(token: &str) -> Result<(), String> {
    for variable in OTHER_SECRET_VARIABLES {
        if env::var(variable).is_ok_and(|other| other == token) {
            return Err(format!(
                "FACTORY_SUPERVISOR_TOKEN must differ from {variable}"
            ));
        }
    }
    Ok(())
}

fn parse_images(raw: &str) -> Result<BTreeSet<String>, String> {
    let mut images = BTreeSet::new();
    for part in raw.split(',') {
        let image = part.trim();
        if image.is_empty() {
            return Err(
                "FACTORY_SUPERVISOR_IMAGES must be a non-empty comma-separated allowlist".into(),
            );
        }
        images.insert(image.to_owned());
    }
    Ok(images)
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::fs;
    use tower::ServiceExt;
    use uuid::Uuid;

    const TEST_TOKEN: &str = "supervisor-token-0123456789abcdef0123456789abcdef";

    struct TestFixture {
        app: Arc<App>,
        root: PathBuf,
    }

    impl Drop for TestFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn fixture() -> TestFixture {
        let root = std::env::temp_dir().join(format!("factory-supervisor-test-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let images: BTreeSet<String> = ["alpine:3.20".to_owned()].into();
        let sandbox = RootlessPodman::new(&root, images.iter().cloned(), MAX_RUNTIME).unwrap();
        TestFixture {
            app: Arc::new(App {
                token: TEST_TOKEN.into(),
                workspace_root: root.clone(),
                allowed_images: images,
                max_timeout: MAX_RUNTIME,
                sandbox,
                run_gate: tokio::sync::Mutex::new(()),
            }),
            root,
        }
    }

    fn execute_request(
        workspace_name: &str,
        image: &str,
        argv: &[&str],
        timeout_secs: u64,
    ) -> ExecuteRequest {
        ExecuteRequest {
            workspace_name: workspace_name.into(),
            image: image.into(),
            argv: argv.iter().map(|value| (*value).to_owned()).collect(),
            timeout_secs,
        }
    }

    fn execute_json(
        workspace_name: &str,
        image: &str,
        argv: &[&str],
        timeout_secs: u64,
    ) -> serde_json::Value {
        json!({
            "workspace_name": workspace_name,
            "image": image,
            "argv": argv,
            "timeout_secs": timeout_secs
        })
    }

    fn json_request(uri: &str, token: Option<&str>, body: &serde_json::Value) -> Request<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn status(router: &Router, request: Request<Body>) -> StatusCode {
        router.clone().oneshot(request).await.unwrap().status()
    }

    #[tokio::test]
    async fn health_is_public_and_minimal() {
        let fixture = fixture();
        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let response = build_router(fixture.app.clone())
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value, json!({"healthy": true}));
    }

    #[tokio::test]
    async fn execute_and_reconcile_require_the_exact_bearer_token() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let body = execute_json("workspace", "alpine:3.20", &["/bin/true"], 30);

        assert_eq!(
            status(&router, json_request("/v1/execute", None, &body)).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(&router, json_request("/v1/execute", Some("wrong"), &body)).await,
            StatusCode::UNAUTHORIZED
        );
        let wrong_scheme = Request::builder()
            .method("POST")
            .uri("/v1/execute")
            .header("content-type", "application/json")
            .header("authorization", format!("Token {TEST_TOKEN}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(
            status(&router, wrong_scheme).await,
            StatusCode::UNAUTHORIZED
        );
        let lowercase_scheme = Request::builder()
            .method("POST")
            .uri("/v1/execute")
            .header("content-type", "application/json")
            .header("authorization", format!("bearer {TEST_TOKEN}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        assert_eq!(
            status(&router, lowercase_scheme).await,
            StatusCode::UNAUTHORIZED
        );

        assert_eq!(
            status(&router, json_request("/v1/reconcile", None, &json!({}))).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(
                &router,
                json_request("/v1/reconcile", Some("wrong"), &json!({}))
            )
            .await,
            StatusCode::UNAUTHORIZED
        );

        let invalid = execute_json("..", "alpine:3.20", &["/bin/true"], 30);
        assert_eq!(
            status(
                &router,
                json_request("/v1/execute", Some(TEST_TOKEN), &invalid)
            )
            .await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn execute_rejects_invalid_workspace_image_argv_and_timeout() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let cases = [
            execute_json("../escape", "alpine:3.20", &["/bin/true"], 30),
            execute_json("nested/child", "alpine:3.20", &["/bin/true"], 30),
            execute_json("", "alpine:3.20", &["/bin/true"], 30),
            execute_json("workspace", "ubuntu:latest", &["/bin/true"], 30),
            execute_json("workspace", "alpine:3.20x", &["/bin/true"], 30),
            execute_json("workspace", "alpine:3.20", &[], 30),
            execute_json("workspace", "alpine:3.20", &[""], 30),
            execute_json("workspace", "alpine:3.20", &["/bin/true"], 0),
            execute_json(
                "workspace",
                "alpine:3.20",
                &["/bin/true"],
                MAX_RUNTIME.as_secs() + 1,
            ),
        ];
        for body in cases {
            assert_eq!(
                status(
                    &router,
                    json_request("/v1/execute", Some(TEST_TOKEN), &body)
                )
                .await,
                StatusCode::BAD_REQUEST,
                "unexpected status for {body}"
            );
        }
    }

    #[test]
    fn validation_accepts_the_inclusive_timeout_bounds_and_exact_allowlist() {
        let fixture = fixture();
        let app = &fixture.app;
        assert!(
            validate_execute(
                app,
                &execute_request("workspace", "alpine:3.20", &["/bin/true"], 1)
            )
            .is_ok()
        );
        assert!(
            validate_execute(
                app,
                &execute_request(
                    "workspace",
                    "alpine:3.20",
                    &["/bin/true"],
                    MAX_RUNTIME.as_secs()
                )
            )
            .is_ok()
        );
        assert!(
            validate_execute(
                app,
                &execute_request("workspace", "alpine:3.20", &["/bin/true"], 0)
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_unknown_fields_malformed_json_wrong_content_type_and_oversized_bodies() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let unknown = json!({
            "workspace_name": "workspace",
            "image": "alpine:3.20",
            "argv": ["/bin/true"],
            "timeout_secs": 30,
            "network": "host"
        });
        assert_eq!(
            status(
                &router,
                json_request("/v1/execute", Some(TEST_TOKEN), &unknown)
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY
        );

        let malformed = Request::builder()
            .method("POST")
            .uri("/v1/execute")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::from("{"))
            .unwrap();
        assert_eq!(status(&router, malformed).await, StatusCode::BAD_REQUEST);

        let wrong_content_type = Request::builder()
            .method("POST")
            .uri("/v1/execute")
            .header("content-type", "text/plain")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::from(
                execute_json("workspace", "alpine:3.20", &["/bin/true"], 30).to_string(),
            ))
            .unwrap();
        assert_eq!(
            status(&router, wrong_content_type).await,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );

        let oversized = json!({
            "workspace_name": "workspace",
            "image": "alpine:3.20",
            "argv": ["x".repeat(MAX_BODY_BYTES + 1)],
            "timeout_secs": 30
        });
        assert_eq!(
            status(
                &router,
                json_request("/v1/execute", Some(TEST_TOKEN), &oversized)
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn rejects_unknown_routes_and_wrong_methods() {
        let fixture = fixture();
        let router = build_router(fixture.app.clone());
        let unknown = Request::builder()
            .method("POST")
            .uri("/v1/execute/..%2Fescape")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {TEST_TOKEN}"))
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(status(&router, unknown).await, StatusCode::NOT_FOUND);

        let wrong_method = Request::builder()
            .uri("/v1/execute")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status(&router, wrong_method).await,
            StatusCode::METHOD_NOT_ALLOWED
        );

        let health_post = Request::builder()
            .method("POST")
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            status(&router, health_post).await,
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    fn configuration_parsing_is_non_empty_and_exact() {
        let parsed = parse_images("alpine:3.20, ubuntu:24.04").unwrap();
        assert_eq!(
            parsed,
            BTreeSet::from(["alpine:3.20".to_owned(), "ubuntu:24.04".to_owned()])
        );
        assert!(parse_images("").is_err());
        assert!(parse_images("alpine:3.20,,ubuntu:24.04").is_err());
        assert!(validate_supervisor_token(&"x".repeat(MIN_TOKEN_BYTES - 1)).is_err());
        assert!(validate_supervisor_token(&"x".repeat(MIN_TOKEN_BYTES)).is_ok());
    }
}
