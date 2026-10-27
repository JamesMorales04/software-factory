//! Narrow trusted runtime broker for the dedicated Factory execution engine.
//!
//! The broker owns the only interface to the nested Podman engine. It accepts
//! typed, policy-bounded operations from the trusted Supervisor, verifier and
//! provider runtime: create/remove named volumes, import/export workspace
//! archives, run an ephemeral network-disabled child container, and manage a
//! provider session container. It never exposes the Podman socket or CLI and
//! never accepts arbitrary Podman arguments, host paths or images.
//!
//! The nested engine runs inside the rootless outer container's user
//! namespace. It is a candidate TRUSTED_LOCAL boundary, not certified
//! isolation. See docs/security/container-runtime.md.

use std::{collections::BTreeSet, env, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time,
};

const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_ARGV_ITEMS: usize = 256;
const MAX_ARG_BYTES: usize = 16 * 1024;
const MAX_ARGV_BYTES: usize = 64 * 1024;
const MAX_ENV_ITEMS: usize = 32;
const MAX_ENV_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_EXECUTION_RUNTIME: Duration = Duration::from_secs(30 * 60);
const MAX_PROVIDER_RUNTIME: Duration = Duration::from_secs(60 * 60);
const MANAGED_LABEL: &str = "io.software-factory.managed=true";
const WORKSPACE_LABEL: &str = "io.software-factory.workspace";

const PROVIDER_ENV_KEYS: [&str; 19] = [
    "FACTORY_PROVIDER_SOCKET",
    "FACTORY_PROVIDER_TCP",
    "FACTORY_EGRESS_LISTEN_PORT",
    "FACTORY_EGRESS_SOCKET",
    "OPENAI_API_KEY",
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_SERVER_PASSWORD",
    "CODEX_HOME",
    "HOME",
    "TMPDIR",
    "PATH",
    "FACTORY_PROVIDER_KIND",
    "FACTORY_PROVIDER_MODEL",
    "FACTORY_FIXTURE_PROVIDER_DIR",
    "FACTORY_FIXTURE_FILE",
    "FACTORY_FIXTURE_CONTENT",
    "FACTORY_FIXTURE_PROVIDER_MODEL",
    "FACTORY_FIXTURE_PROVIDER_USE_EGRESS",
    "LANG",
];

#[derive(Clone)]
struct App {
    token: String,
    podman: PathBuf,
    images: Arc<BTreeSet<String>>,
    provider_images: Arc<BTreeSet<String>>,
    provider_kind: String,
    provider_session_root: PathBuf,
    import_images: Vec<(String, PathBuf)>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(
                    "factory_native=info,factory_runtime_broker=info",
                )
            }),
        )
        .with_writer(factory_native::telemetry::RedactingMakeWriter)
        .init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let token = factory_native::secrets::required("FACTORY_RUNTIME_BROKER_TOKEN")?;
    if token.len() < 32 {
        return Err("FACTORY_RUNTIME_BROKER_TOKEN must be at least 32 bytes".into());
    }
    factory_native::telemetry::register_secret(&token);
    let podman = env::var("FACTORY_PODMAN_BIN").unwrap_or_else(|_| "/usr/bin/podman".into());
    let podman = PathBuf::from(podman);
    if !podman.is_absolute() || !podman.is_file() {
        return Err("FACTORY_PODMAN_BIN must be an absolute existing file".into());
    }
    let images = parse_images(
        env::var("FACTORY_RUNTIME_IMAGES").map_err(|_| "FACTORY_RUNTIME_IMAGES required")?,
    )?;
    let provider_images = match env::var("FACTORY_RUNTIME_PROVIDER_IMAGES") {
        Ok(value) if !value.trim().is_empty() => parse_images(value)?,
        _ => BTreeSet::new(),
    };
    let provider_kind = env::var("FACTORY_RUNTIME_PROVIDER_KIND").unwrap_or_default();
    if !matches!(
        provider_kind.as_str(),
        "" | "opencode" | "codex" | "fixture"
    ) {
        return Err("FACTORY_RUNTIME_PROVIDER_KIND is unsupported".into());
    }
    if provider_kind.is_empty() != provider_images.is_empty() {
        return Err(
            "FACTORY_RUNTIME_PROVIDER_IMAGES and FACTORY_RUNTIME_PROVIDER_KIND must be set together"
                .into(),
        );
    }
    let provider_session_root = PathBuf::from(
        env::var("FACTORY_RUNTIME_PROVIDER_SESSION_ROOT")
            .unwrap_or_else(|_| "/provider-sessions".into()),
    );
    if !provider_session_root.is_absolute() {
        return Err("FACTORY_RUNTIME_PROVIDER_SESSION_ROOT must be absolute".into());
    }
    let import_images =
        parse_import_images(env::var("FACTORY_RUNTIME_IMPORT_IMAGES").unwrap_or_default())?;
    let bind = env::var("FACTORY_BIND").unwrap_or_else(|_| "0.0.0.0:8770".into());
    let addr: std::net::SocketAddr = bind.parse()?;
    let container_mode = env::var("FACTORY_CONTAINER_MODE").as_deref() == Ok("1");
    if !(addr.ip().is_loopback()
        || (container_mode && addr.ip().is_unspecified() && addr.is_ipv4()))
    {
        return Err("runtime broker requires loopback or explicit container IPv4 binding".into());
    }

    let app = Arc::new(App {
        token,
        podman,
        images: Arc::new(images),
        provider_images: Arc::new(provider_images),
        provider_kind,
        provider_session_root,
        import_images,
    });
    app.preflight().await?;
    app.remove_managed_containers().await;
    app.prepull_images().await?;

    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/volumes", post(create_volume))
        .route("/v1/volumes/{volume}", axum::routing::delete(remove_volume))
        .route(
            "/v1/volumes/{volume}/import",
            post(import_volume).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES)),
        )
        .route("/v1/volumes/{volume}/export", get(export_volume))
        .route("/v1/containers/run", post(run_container))
        .route(
            "/v1/containers/{container}",
            axum::routing::delete(remove_container),
        )
        .route("/v1/containers/{container}/status", get(container_status))
        .route("/v1/providers/start", post(start_provider))
        .layer(DefaultBodyLimit::max(MAX_JSON_BYTES))
        .with_state(Arc::clone(&app));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "Factory runtime broker listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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

fn parse_images(raw: String) -> Result<BTreeSet<String>, String> {
    let mut images = BTreeSet::new();
    for image in raw.split(',') {
        let image = image.trim();
        if image.is_empty()
            || image.starts_with('-')
            || image.chars().any(char::is_whitespace)
            || image.contains("://")
        {
            return Err("image allowlist contains an invalid reference".into());
        }
        images.insert(image.to_owned());
    }
    if images.is_empty() {
        return Err("image allowlist must not be empty".into());
    }
    Ok(images)
}

impl App {
    async fn preflight(&self) -> Result<(), Box<dyn std::error::Error>> {
        let uid_map = std::fs::read_to_string("/proc/self/uid_map").unwrap_or_default();
        if has_host_root_mapping(&uid_map) {
            return Err("runtime broker must run inside a rootless outer user namespace".into());
        }
        let output = self
            .podman_command(["--version"])
            .output()
            .await
            .map_err(|error| format!("podman is unavailable: {error}"))?;
        if !output.status.success() {
            return Err(format!("podman --version failed: {}", stderr(&output)).into());
        }
        let output = self
            .podman_command(["info", "--format", "json"])
            .output()
            .await
            .map_err(|error| format!("podman info failed: {error}"))?;
        if !output.status.success() {
            return Err(format!("podman info failed: {}", stderr(&output)).into());
        }
        let info: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("podman info returned invalid JSON: {error}"))?;
        if info
            .pointer("/host/security/rootless")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        {
            return Err(
                "nested runtime engine must be a rootful engine inside the rootless outer container"
                    .into(),
            );
        }
        Ok(())
    }

    async fn prepull_images(&self) -> Result<(), Box<dyn std::error::Error>> {
        for image in self.images.iter() {
            let exists = self
                .podman_output(["image", "exists", image.as_str()])
                .await?;
            if exists.status.success() {
                continue;
            }
            let output = time::timeout(
                Duration::from_secs(900),
                self.podman_command(["pull", "--quiet", image.as_str()])
                    .output(),
            )
            .await
            .map_err(|_| format!("pulling {image} timed out"))??;
            if !output.status.success() {
                return Err(format!("pulling {image} failed: {}", stderr(&output)).into());
            }
        }
        for (tag, path) in &self.import_images {
            if !path.is_file() {
                return Err(format!("image bundle {} is missing", path.display()).into());
            }
            let bundle_path = path.clone();
            let digest = tokio::task::spawn_blocking(move || bundle_digest(&bundle_path))
                .await
                .map_err(|error| format!("bundle hash task failed: {error}"))??;
            let existing = self
                .podman_output([
                    "image",
                    "inspect",
                    "--format",
                    "{{ index .Labels \"io.software-factory.bundle-sha256\" }}",
                    tag.as_str(),
                ])
                .await;
            if let Ok(existing) = existing
                && existing.status.success()
                && String::from_utf8_lossy(&existing.stdout).trim() == digest
            {
                continue;
            }
            let label = format!("LABEL io.software-factory.bundle-sha256={digest}");
            let stable = PathBuf::from("/var/lib/containers/tmp/provider-bundle.tar");
            let source = path.clone();
            let stable_for_copy = stable.clone();
            tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                std::fs::copy(&source, &stable_for_copy)?;
                Ok(())
            })
            .await
            .map_err(|error| format!("bundle copy task failed: {error}"))?
            .map_err(|error| format!("bundle copy failed: {error}"))?;
            let output = time::timeout(
                Duration::from_secs(900),
                self.podman_command([
                    "import",
                    "--change",
                    "ENV PATH=/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin",
                    "--change",
                    "ENTRYPOINT [\"/usr/local/bin/factory-provider-entrypoint\"]",
                    "--change",
                    "WORKDIR /session",
                    "--change",
                    label.as_str(),
                    stable.to_string_lossy().as_ref(),
                    tag.as_str(),
                ])
                .output(),
            )
            .await
            .map_err(|_| format!("importing {tag} timed out"))??;
            let _ = std::fs::remove_file(&stable);
            if !output.status.success() {
                let existing = self
                    .podman_output([
                        "image",
                        "inspect",
                        "--format",
                        "{{ index .Labels \"io.software-factory.bundle-sha256\" }}",
                        tag.as_str(),
                    ])
                    .await;
                if !existing.is_ok_and(|existing| {
                    existing.status.success()
                        && String::from_utf8_lossy(&existing.stdout).trim() == digest
                }) {
                    return Err(format!("importing {tag} failed: {}", stderr(&output)).into());
                }
            }
        }
        for image in self.provider_images.iter() {
            let output = self
                .podman_output(["image", "exists", image.as_str()])
                .await?;
            if !output.status.success() {
                return Err(format!("provider image {image} is not available").into());
            }
        }
        Ok(())
    }

    async fn remove_managed_containers(&self) {
        let output = self
            .podman_command([
                "ps",
                "-a",
                "--filter",
                &format!("label={MANAGED_LABEL}"),
                "--format",
                "{{.Names}}",
            ])
            .output()
            .await;
        let Ok(output) = output else {
            tracing::warn!("could not list managed containers during reconciliation");
            return;
        };
        if !output.status.success() {
            tracing::warn!("managed container listing failed during reconciliation");
            return;
        }
        let names: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|name| valid_container_name(name))
            .map(str::to_owned)
            .collect();
        for name in names {
            if let Err(error) = self
                .podman_output(["rm", "--force", "--time=0", name.as_str()])
                .await
            {
                tracing::warn!(%name, %error, "could not remove reconciled container");
            }
        }
    }

    fn podman_command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new(&self.podman);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .env("TMPDIR", "/var/lib/containers/tmp")
            .env("STORAGE_DRIVER", "vfs")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    async fn podman_output<I, S>(&self, args: I) -> Result<std::process::Output, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        time::timeout(COMMAND_TIMEOUT, self.podman_command(args).output())
            .await
            .map_err(|_| "podman command timed out".to_owned())?
            .map_err(|error| format!("podman command failed: {error}"))
    }

    async fn run_checked<I, S>(&self, args: I, operation: &str) -> Result<Vec<u8>, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let output = self.podman_output(args).await?;
        if !output.status.success() {
            return Err(format!("{operation} failed: {}", stderr(&output)));
        }
        Ok(output.stdout)
    }
}

fn stderr(output: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    text.chars()
        .take(1024)
        .collect::<String>()
        .trim()
        .to_owned()
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "healthy": true,
        "isolation_certified": false,
        "engine_exposed": false,
        "network": "none"
    }))
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(value) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    constant_time_eq(value.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right) {
        diff |= left ^ right;
    }
    diff == 0
}

fn valid_volume_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("factory-ws-") else {
        return false;
    };
    rest.len() == 36
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn valid_container_name(name: &str) -> bool {
    let prefixes = ["factory-exec-", "factory-provider-"];
    prefixes.iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            rest.len() == 36
                && rest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        })
    })
}

fn valid_workspace_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_provider_session_subdir(value: &str) -> bool {
    value.len() >= 8
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn parse_import_images(raw: String) -> Result<Vec<(String, PathBuf)>, String> {
    let mut images = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((tag, path)) = entry.split_once('=') else {
            return Err("FACTORY_RUNTIME_IMPORT_IMAGES entries must be tag=/absolute/path".into());
        };
        let tag = tag.trim();
        let path = PathBuf::from(path.trim());
        if tag.is_empty()
            || tag.starts_with('-')
            || tag.chars().any(char::is_whitespace)
            || !path.is_absolute()
        {
            return Err("FACTORY_RUNTIME_IMPORT_IMAGES entry is invalid".into());
        }
        images.push((tag.to_owned(), path));
    }
    Ok(images)
}

fn has_host_root_mapping(map: &str) -> bool {
    let mut lines = map.lines().filter(|line| !line.trim().is_empty());
    let Some(_first) = lines.next() else {
        return true;
    };
    let mut any = false;
    for line in map.lines().filter(|line| !line.trim().is_empty()) {
        any = true;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 3 {
            return true;
        }
        if fields[0] == "0" && fields[1] == "0" {
            return true;
        }
    }
    !any
}

fn bundle_digest(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("cannot open image bundle: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read image bundle: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_argv(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() || argv.len() > MAX_ARGV_ITEMS {
        return Err("argv size is outside the allowed bound".into());
    }
    let mut total = 0usize;
    for argument in argv {
        if argument.is_empty() || argument.len() > MAX_ARG_BYTES {
            return Err("argv item is empty or too large".into());
        }
        total = total.saturating_add(argument.len());
    }
    if total > MAX_ARGV_BYTES {
        return Err("argv is too large".into());
    }
    Ok(())
}

fn validate_env(env: &[(String, String)]) -> Result<(), String> {
    if env.len() > MAX_ENV_ITEMS {
        return Err("environment has too many entries".into());
    }
    let mut total = 0usize;
    let mut seen = BTreeSet::new();
    for (key, value) in env {
        if !PROVIDER_ENV_KEYS.contains(&key.as_str()) {
            return Err(format!("environment key {key:?} is not allowed"));
        }
        if !seen.insert(key.as_str()) {
            return Err("environment contains a duplicate key".into());
        }
        if value.len() > MAX_ARG_BYTES || value.contains('\0') {
            return Err("environment value is invalid".into());
        }
        total = total.saturating_add(key.len() + value.len());
    }
    if total > MAX_ENV_BYTES {
        return Err("environment is too large".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NewVolume {
    volume: String,
    workspace_id: String,
}

async fn create_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<NewVolume>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_volume_name(&input.volume) || !valid_workspace_id(&input.workspace_id) {
        return bad_request("invalid volume or workspace identity");
    }
    let label = format!("{WORKSPACE_LABEL}={}", input.workspace_id);
    match app
        .run_checked(
            [
                "volume",
                "create",
                "--label",
                &label,
                "--label",
                MANAGED_LABEL,
                input.volume.as_str(),
            ],
            "volume create",
        )
        .await
    {
        Ok(stdout) => (
            StatusCode::CREATED,
            Json(json!({
                "volume": input.volume,
                "created": true,
                "output": String::from_utf8_lossy(&stdout).trim()
            })),
        )
            .into_response(),
        Err(error) => operation_error(error),
    }
}

async fn remove_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(volume): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_volume_name(&volume) {
        return bad_request("invalid volume name");
    }
    let output = match app
        .podman_output(["volume", "rm", "--force", volume.as_str()])
        .await
    {
        Ok(output) => output,
        Err(error) => return operation_error(error),
    };
    if output.status.success() {
        return StatusCode::NO_CONTENT.into_response();
    }
    match app
        .podman_output(["volume", "exists", volume.as_str()])
        .await
    {
        Ok(exists) if exists.status.code() == Some(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => operation_error(format!("volume removal failed: {}", stderr(&output))),
        Err(error) => operation_error(error),
    }
}

async fn import_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(volume): AxumPath<String>,
    body: Bytes,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_volume_name(&volume) || body.is_empty() || body.len() > MAX_ARCHIVE_BYTES {
        return bad_request("invalid volume import");
    }
    let mut child = match app
        .podman_command(["volume", "import", volume.as_str(), "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return operation_error(format!("volume import could not start: {error}")),
    };
    let Some(mut stdin) = child.stdin.take() else {
        return operation_error("volume import stdin unavailable");
    };
    let bytes = body.to_vec();
    let writer = tokio::spawn(async move {
        stdin.write_all(&bytes).await?;
        stdin.shutdown().await
    });
    let output = match time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return operation_error(format!("volume import failed: {error}")),
        Err(_) => {
            let _ = app
                .podman_output(["volume", "rm", "--force", volume.as_str()])
                .await;
            return operation_error("volume import timed out");
        }
    };
    if let Err(error) = writer.await.unwrap_or_else(|error| {
        Err(std::io::Error::other(format!(
            "volume import writer task: {error}"
        )))
    }) {
        return operation_error(format!("volume import writer failed: {error}"));
    }
    if !output.status.success() {
        return operation_error(format!("volume import failed: {}", stderr(&output)));
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn export_volume(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(volume): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_volume_name(&volume) {
        return bad_request("invalid volume name");
    }
    let mut child = match app
        .podman_command(["volume", "export", volume.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return operation_error(format!("volume export could not start: {error}")),
    };
    let Some(stdout) = child.stdout.take() else {
        return operation_error("volume export stdout unavailable");
    };
    let Some(stderr) = child.stderr.take() else {
        return operation_error("volume export stderr unavailable");
    };
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_ARCHIVE_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STREAM_BYTES));
    let status = match time::timeout(COMMAND_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => return operation_error(format!("volume export failed: {error}")),
        Err(_) => {
            let _ = child.kill().await;
            return operation_error("volume export timed out");
        }
    };
    let (bytes, truncated) = match stdout_task.await {
        Ok(Ok(value)) => value,
        _ => return operation_error("volume export stdout could not be read"),
    };
    let (stderr_bytes, _) = match stderr_task.await {
        Ok(Ok(value)) => value,
        _ => return operation_error("volume export stderr could not be read"),
    };
    if !status.success() {
        return operation_error(format!(
            "volume export failed: {}",
            String::from_utf8_lossy(&stderr_bytes).trim()
        ));
    }
    if truncated {
        return operation_error("volume export exceeded the archive bound");
    }
    (
        StatusCode::OK,
        [("content-type", "application/x-tar")],
        bytes,
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RunContainer {
    volume: String,
    container: String,
    image: String,
    argv: Vec<String>,
    timeout_secs: u64,
}

async fn run_container(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<RunContainer>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_volume_name(&input.volume) || !valid_container_name(&input.container) {
        return bad_request("invalid volume or container identity");
    }
    if !app.images.contains(&input.image) {
        return bad_request("image is not allowlisted");
    }
    if let Err(error) = validate_argv(&input.argv) {
        return bad_request(&error);
    }
    if !(1..=MAX_EXECUTION_RUNTIME.as_secs()).contains(&input.timeout_secs) {
        return bad_request("execution timeout is outside the allowed bound");
    }
    tracing::info!(container=%input.container, volume=%input.volume, "child execution started");
    let result = run_child(&app, &input).await;
    match &result {
        Ok(output) => tracing::info!(
            container=%input.container,
            exit_code=?output.exit_code,
            "child execution finished"
        ),
        Err(error) => tracing::warn!(container=%input.container, %error, "child execution failed"),
    }
    match result {
        Ok(output) => Json(json!({
            "exitCode": output.exit_code,
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "stdoutTruncated": output.stdout_truncated,
            "stderrTruncated": output.stderr_truncated
        }))
        .into_response(),
        Err(error) => operation_error(error),
    }
}

struct ChildOutput {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

async fn run_child(app: &App, input: &RunContainer) -> Result<ChildOutput, String> {
    let timeout = Duration::from_secs(input.timeout_secs);
    let args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        format!("--name={}", input.container),
        format!("--label={MANAGED_LABEL}"),
        format!("--label={WORKSPACE_LABEL}={}", input.volume),
        "--pull=never".to_owned(),
        "--network=none".to_owned(),
        "--read-only".to_owned(),
        "--tmpfs=/tmp:rw,noexec,nosuid,size=64m".to_owned(),
        "--tmpfs=/run:rw,noexec,nosuid,size=16m".to_owned(),
        "--cap-drop=all".to_owned(),
        "--cap-add=NET_ADMIN".to_owned(),
        "--security-opt=no-new-privileges".to_owned(),
        "--ulimit=fsize=67108864".to_owned(),
        "--pids-limit=128".to_owned(),
        "--memory=1g".to_owned(),
        "--cpus=2".to_owned(),
        "--workdir=/workspace".to_owned(),
        "--env=HOME=/tmp".to_owned(),
        "--env=TMPDIR=/tmp".to_owned(),
        format!("--volume={}:/workspace:rw", input.volume),
        input.image.clone(),
    ];
    let mut command = app.podman_command(args.iter().map(String::as_str));
    for argument in &input.argv {
        command.arg(argument);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("child container could not start: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        return Err("child stdout unavailable".into());
    };
    let Some(stderr) = child.stderr.take() else {
        return Err("child stderr unavailable".into());
    };
    let stdout_task = tokio::spawn(read_bounded(stdout, MAX_STREAM_BYTES));
    let stderr_task = tokio::spawn(read_bounded(stderr, MAX_STREAM_BYTES));
    let status = match time::timeout(timeout + Duration::from_secs(15), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            stdout_task.abort();
            stderr_task.abort();
            return Err(format!("child container wait failed: {error}"));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = app
                .podman_output(["rm", "--force", "--time=0", input.container.as_str()])
                .await;
            stdout_task.abort();
            stderr_task.abort();
            return Err("child container execution timed out".into());
        }
    };
    let _ = app
        .podman_output(["rm", "--force", "--time=0", input.container.as_str()])
        .await;
    let (stdout, stdout_truncated) = stdout_task
        .await
        .map_err(|error| format!("child stdout reader: {error}"))?
        .map_err(|error| format!("child stdout reader: {error}"))?;
    let (stderr, stderr_truncated) = stderr_task
        .await
        .map_err(|error| format!("child stderr reader: {error}"))?
        .map_err(|error| format!("child stderr reader: {error}"))?;
    Ok(ChildOutput {
        exit_code: status.code(),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

async fn remove_container(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(container): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_container_name(&container) {
        return bad_request("invalid container name");
    }
    let output = match app
        .podman_output(["rm", "--force", "--time=0", container.as_str()])
        .await
    {
        Ok(output) => output,
        Err(error) => return operation_error(error),
    };
    if output.status.success() {
        return StatusCode::NO_CONTENT.into_response();
    }
    match app
        .podman_output(["container", "exists", container.as_str()])
        .await
    {
        Ok(exists) if exists.status.code() == Some(1) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => operation_error(format!("container removal failed: {}", stderr(&output))),
        Err(error) => operation_error(error),
    }
}

async fn container_status(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    AxumPath(container): AxumPath<String>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_container_name(&container) {
        return bad_request("invalid container name");
    }
    let output = match app
        .podman_output([
            "inspect",
            "--format",
            "{{.State.Status}} {{.State.ExitCode}}",
            container.as_str(),
        ])
        .await
    {
        Ok(output) => output,
        Err(error) => return operation_error(error),
    };
    if !output.status.success() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"container not found"})),
        )
            .into_response();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace();
    let status = parts.next().unwrap_or("unknown");
    let exit_code = parts
        .next()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(-1);
    Json(json!({"container":container,"status":status,"exitCode":exit_code})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StartProvider {
    container: String,
    session_subdir: String,
    timeout_secs: u64,
    env: Vec<(String, String)>,
}

async fn start_provider(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<StartProvider>,
) -> Response {
    if !authorized(&headers, &app.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !valid_container_name(&input.container) || !input.container.starts_with("factory-provider-")
    {
        return bad_request("invalid provider container identity");
    }
    if !valid_provider_session_subdir(&input.session_subdir) {
        return bad_request("invalid provider session directory");
    }
    if !(1..=MAX_PROVIDER_RUNTIME.as_secs()).contains(&input.timeout_secs) {
        return bad_request("provider timeout is outside the allowed bound");
    }
    if let Err(error) = validate_env(&input.env) {
        return bad_request(&error);
    }
    if app.provider_kind.is_empty() {
        return bad_request("provider runtime is not configured");
    }
    let Some(image) = app.provider_images.iter().next() else {
        return bad_request("provider image is not configured");
    };
    if app.provider_images.len() != 1 {
        return bad_request("provider image allowlist must contain exactly one image");
    }
    let session_source = app.provider_session_root.join(&input.session_subdir);
    let canonical = match session_source.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return bad_request(&format!(
                "provider session directory is unavailable: {error}"
            ));
        }
    };
    let canonical_root = match app.provider_session_root.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return operation_error(format!("provider session root is unavailable: {error}"));
        }
    };
    if !canonical.starts_with(&canonical_root) || !canonical.is_dir() {
        return bad_request("provider session directory escapes the configured root");
    }
    let mount = format!(
        "type=bind,source={},destination=/session,bind-propagation=rprivate",
        canonical.to_string_lossy()
    );
    let mut args: Vec<String> = vec![
        "run".into(),
        "--detach".into(),
        format!("--name={}", input.container),
        format!("--label={MANAGED_LABEL}"),
        "--pull=never".into(),
        "--network=none".into(),
        "--read-only".into(),
        "--tmpfs=/tmp:rw,noexec,nosuid,size=512m".into(),
        "--tmpfs=/run:rw,noexec,nosuid,size=16m".into(),
        "--cap-drop=all".into(),
        "--security-opt=no-new-privileges".into(),
        "--ulimit=fsize=67108864".into(),
        "--user=10001:10001".into(),
        "--pids-limit=256".into(),
        "--memory=4g".into(),
        "--cpus=2".into(),
        "--workdir=/session".into(),
        format!("--mount={mount}"),
    ];
    for (key, value) in &input.env {
        args.push(format!("--env={key}={value}"));
    }
    args.push(image.clone());
    let output = match time::timeout(
        COMMAND_TIMEOUT,
        app.podman_command(args.iter().map(String::as_str)).output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return operation_error(format!("provider start failed: {error}")),
        Err(_) => return operation_error("provider start timed out"),
    };
    if !output.status.success() {
        let _ = app
            .podman_output(["rm", "--force", "--time=0", input.container.as_str()])
            .await;
        return operation_error(format!("provider start failed: {}", stderr(&output)));
    }
    tracing::info!(container=%input.container, subdir=%input.session_subdir, "provider container started");
    let reaper = Arc::clone(&app);
    let reaper_container = input.container.clone();
    let reaper_seconds = input.timeout_secs;
    tokio::spawn(async move {
        time::sleep(Duration::from_secs(reaper_seconds)).await;
        if let Ok(exists) = reaper
            .podman_output(["container", "exists", reaper_container.as_str()])
            .await
            && exists.status.success()
        {
            tracing::warn!(container=%reaper_container, "provider container exceeded its lifetime; removing");
            let _ = reaper
                .podman_output(["rm", "--force", "--time=0", reaper_container.as_str()])
                .await;
        }
    });
    (
        StatusCode::CREATED,
        Json(json!({
            "container": input.container,
            "sessionSubdir": input.session_subdir,
            "image": image
        })),
    )
        .into_response()
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
}

fn operation_error(message: impl Into<String>) -> Response {
    let message = message.into();
    let message: String = message.chars().take(1024).collect();
    (StatusCode::BAD_GATEWAY, Json(json!({"error": message}))).into_response()
}

async fn read_bounded<R>(mut reader: R, limit: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        if captured.len() < limit {
            let remaining = limit - captured.len();
            captured.extend_from_slice(&chunk[..read.min(remaining)]);
            if read > remaining {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }
    Ok((captured, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rootful_outer_user_namespace_is_rejected() {
        assert!(has_host_root_mapping("         0          0 4294967295\n"));
        assert!(has_host_root_mapping("         0          0          1\n"));
        assert!(has_host_root_mapping("garbage\n"));
        assert!(has_host_root_mapping(""));
        assert!(!has_host_root_mapping(
            "         0       1000          1\n         1     100000      65536\n"
        ));
    }

    #[test]
    fn identities_are_strict() {
        assert!(valid_volume_name(
            "factory-ws-123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(!valid_volume_name("factory-ws-../../etc"));
        assert!(valid_container_name(
            "factory-exec-123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(valid_container_name(
            "factory-provider-123e4567-e89b-12d3-a456-426614174000"
        ));
        assert!(!valid_container_name("factory-provider-123"));
        assert!(valid_provider_session_subdir("provider-session-a"));
        assert!(!valid_provider_session_subdir("../escape"));
        assert!(valid_workspace_id("attempt-1"));
    }

    #[test]
    fn argv_and_environment_are_bounded() {
        assert!(validate_argv(&["true".into()]).is_ok());
        assert!(validate_argv(&[]).is_err());
        assert!(
            validate_argv(&["x".repeat(MAX_ARG_BYTES + 1)]).is_err(),
            "oversized argv item accepted"
        );
        assert!(validate_env(&[("HOME".into(), "/tmp".into())]).is_ok());
        assert!(validate_env(&[("LD_PRELOAD".into(), "/x".into())]).is_err());
    }

    #[test]
    fn image_allowlist_rejects_flags_and_empty_entries() {
        assert!(parse_images("docker.io/library/alpine:3.20".into()).is_ok());
        assert!(parse_images("--privileged".into()).is_err());
        assert!(parse_images("a,,b".into()).is_err());
        assert!(parse_images("a b".into()).is_err());
    }

    #[test]
    fn import_images_are_strict() {
        assert!(parse_import_images("factory-provider:dev=/bundles/provider.tar".into()).is_ok());
        assert!(parse_import_images("factory-provider:dev=relative.tar".into()).is_err());
        assert!(parse_import_images("no-equals".into()).is_err());
    }

    #[test]
    fn constant_time_compare_requires_exact_match() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
    }
}
