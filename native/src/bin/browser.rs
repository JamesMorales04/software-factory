//! `factory-browser`: explicit managed test-server and real-browser check
//! operations for a Worker or verifier.
//!
//! Linux-only managed server lifecycle. Browser tooling absence is reported as
//! `NOT_TESTED` (exit 3), never as a pass.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::ExitCode,
};

use factory_native::browser::{
    BrowserCheckConfig, BrowserCheckStatus, BrowserTooling, LoopbackEndpoint, ManagedTestServer,
    ManagedTestServerConfig, cleanup_state_dir, read_state, run_browser_check, state_path,
    stop_from_state,
};
use serde_json::json;
use uuid::Uuid;

const EXIT_FAILED: u8 = 1;
const EXIT_NOT_TESTED: u8 = 3;

struct Cli {
    flags: BTreeMap<String, String>,
    argv: Vec<String>,
}

impl Cli {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut flags = BTreeMap::new();
        let mut argv = Vec::new();
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            if argument == "--" {
                argv.extend(args);
                break;
            }
            let Some(name) = argument.strip_prefix("--") else {
                return Err(format!("unexpected positional argument: {argument}"));
            };
            let value = if BOOLEAN_FLAGS.contains(&name) {
                "1".to_owned()
            } else {
                args.next()
                    .ok_or_else(|| format!("missing value for --{name}"))?
            };
            if flags.insert(name.to_owned(), value).is_some() {
                return Err(format!("duplicate flag --{name}"));
            }
        }
        Ok(Self { flags, argv })
    }

    fn optional(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.optional(name)
            .ok_or_else(|| format!("missing required --{name}"))
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.optional(name).map(PathBuf::from)
    }

    fn number(&self, name: &str, default: u64) -> Result<u64, String> {
        match self.optional(name) {
            Some(value) => value
                .parse()
                .map_err(|_| format!("--{name} must be an unsigned integer")),
            None => Ok(default),
        }
    }

    fn reject_unknown(&self, allowed: &[&str]) -> Result<(), String> {
        let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
        for name in self.flags.keys() {
            if !allowed.contains(name.as_str()) {
                return Err(format!("unknown flag --{name}"));
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: factory-browser <run|server|check|start|stop|cleanup|tools> ...");
        return ExitCode::from(EXIT_FAILED);
    };
    let cli = match Cli::parse(args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    match command.as_str() {
        "run" => run(cli).await,
        "server" => server(cli).await,
        "check" => check(cli).await,
        "start" => start(cli).await,
        "stop" => stop(cli).await,
        "cleanup" => cleanup(cli).await,
        "tools" => tools(cli),
        other => {
            eprintln!("factory-browser: unknown command {other}");
            ExitCode::from(EXIT_FAILED)
        }
    }
}

async fn run(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(RUN_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let inputs = match run_inputs(&cli) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let server = match ManagedTestServer::start(inputs.config).await {
        Ok(server) => server,
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let report = run_browser_check(BrowserCheckConfig {
        endpoint: inputs.endpoint,
        path: cli.optional("url-path").unwrap_or("/").into(),
        selector: cli.optional("selector").map(str::to_owned),
        text: cli.optional("text").map(str::to_owned),
        timeout: inputs.timeout,
        profile_dir: inputs.profile_dir,
        evidence_path: inputs.evidence,
        screenshot: inputs.screenshot,
        tooling: inputs.tooling,
        server_pid: Some(server.pid()),
        server_process_group: Some(server.process_group()),
        server_start_time_ticks: Some(server.start_time_ticks()),
    })
    .await;
    let receipt = match server.stop().await {
        Ok(receipt) => serde_json::to_value(receipt).unwrap_or_default(),
        Err(error) => {
            print_json(&json!({
                "status": "FAILED",
                "detail": format!("managed server stop failed: {error}"),
                "browser": report,
            }));
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let exit = exit_for(report.status);
    print_json(&json!({"status": report.status, "browser": report, "server": receipt}));
    ExitCode::from(exit)
}

async fn server(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(SERVER_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let config = match server_inputs(&cli) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let server = match ManagedTestServer::start(config).await {
        Ok(server) => server,
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            return ExitCode::from(EXIT_FAILED);
        }
    };
    print_json(&json!({
        "status": "READY",
        "pid": server.pid(),
        "processGroup": server.process_group(),
        "endpoint": server.endpoint().url(),
        "stdoutLog": server.stdout_log(),
        "stderrLog": server.stderr_log(),
    }));
    wait_for_signal().await;
    match server.stop().await {
        Ok(receipt) => {
            print_json(&json!({"status": "STOPPED", "server": receipt}));
            ExitCode::SUCCESS
        }
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            ExitCode::from(EXIT_FAILED)
        }
    }
}

async fn check(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(CHECK_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let url = match cli.required("url") {
        Ok(value) => value,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let (endpoint, path) = match split_url(url) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let timeout = match cli.number("timeout-secs", 60) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let root =
        std::env::temp_dir().join(format!("factory-browser-check-{}", Uuid::new_v4().simple()));
    let profile_dir = cli
        .path("profile-dir")
        .unwrap_or_else(|| root.join("profile"));
    let evidence_path = cli
        .path("evidence")
        .unwrap_or_else(|| root.join("browser-evidence.json"));
    let report = run_browser_check(BrowserCheckConfig {
        endpoint,
        path,
        selector: cli.optional("selector").map(str::to_owned),
        text: cli.optional("text").map(str::to_owned),
        timeout: std::time::Duration::from_secs(timeout),
        profile_dir,
        evidence_path: evidence_path.clone(),
        screenshot: cli.path("screenshot"),
        tooling: tooling_from(&cli),
        server_pid: None,
        server_process_group: None,
        server_start_time_ticks: None,
    })
    .await;
    let exit = exit_for(report.status);
    print_json(&json!({
        "status": report.status,
        "browser": report,
        "evidencePath": evidence_path,
    }));
    ExitCode::from(exit)
}

async fn start(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(START_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let config = match server_inputs(&cli) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let state_dir = match cli.path("state-dir") {
        Some(path) => path,
        None => {
            eprintln!("factory-browser: missing required --state-dir");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let id = cli
        .optional("id")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("server-{}", Uuid::new_v4().simple()));
    match ManagedTestServer::start_detached(config, &state_dir, &id).await {
        Ok(state) => {
            print_json(&json!({
                "status": "READY",
                "stateFile": state_path(&state_dir, &id),
                "state": state,
            }));
            ExitCode::SUCCESS
        }
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            ExitCode::from(EXIT_FAILED)
        }
    }
}

async fn stop(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(STOP_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let state_file = match cli.path("state-file") {
        Some(path) => path,
        None => {
            eprintln!("factory-browser: missing required --state-file");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let state = match read_state(&state_file) {
        Ok(state) => state,
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let max_log_bytes = match cli.number("max-log-bytes", state.max_log_bytes) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("factory-browser: {error}");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    match stop_from_state(&state, max_log_bytes).await {
        Ok(receipt) => {
            let removed = match std::fs::remove_file(&state_file) {
                Ok(()) => true,
                Err(error) => error.kind() == std::io::ErrorKind::NotFound,
            };
            print_json(&json!({
                "status": "STOPPED",
                "stateRemoved": removed,
                "server": receipt,
            }));
            ExitCode::SUCCESS
        }
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            ExitCode::from(EXIT_FAILED)
        }
    }
}

async fn cleanup(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(CLEANUP_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let state_dir = match cli.path("state-dir") {
        Some(path) => path,
        None => {
            eprintln!("factory-browser: missing required --state-dir");
            return ExitCode::from(EXIT_FAILED);
        }
    };
    let stop_live = cli.flags.contains_key("stop-live");
    let remove_logs = !cli.flags.contains_key("keep-logs");
    match cleanup_state_dir(&state_dir, remove_logs, stop_live).await {
        Ok(summary) => {
            let exit = if summary.failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_FAILED)
            };
            print_json(&json!({"status": "CLEANED", "cleanup": summary}));
            exit
        }
        Err(error) => {
            print_json(&json!({"status": "FAILED", "detail": error.to_string()}));
            ExitCode::from(EXIT_FAILED)
        }
    }
}

fn tools(cli: Cli) -> ExitCode {
    if let Err(error) = cli.reject_unknown(TOOLS_FLAGS) {
        eprintln!("factory-browser: {error}");
        return ExitCode::from(EXIT_FAILED);
    }
    let tooling = tooling_from(&cli);
    let missing = tooling.missing();
    let value = json!({
        "status": if missing.is_some() { "NOT_TESTED" } else { "AVAILABLE" },
        "missing": missing,
        "node": tooling.node,
        "runner": tooling.runner,
        "playwrightCore": tooling.playwright_core,
        "chromiumExecutable": tooling.chromium_executable,
        "browsersPath": tooling.browsers_path,
    });
    print_json(&value);
    if value["missing"].is_null() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_NOT_TESTED)
    }
}

struct RunInputs {
    config: ManagedTestServerConfig,
    endpoint: LoopbackEndpoint,
    profile_dir: PathBuf,
    evidence: PathBuf,
    screenshot: Option<PathBuf>,
    tooling: BrowserTooling,
    timeout: std::time::Duration,
}

fn run_inputs(cli: &Cli) -> Result<RunInputs, String> {
    let config = server_inputs(cli)?;
    let endpoint = config
        .endpoint
        .clone()
        .ok_or_else(|| "run requires --endpoint so the browser target is authorized".to_owned())?;
    let log_dir = config.log_dir.clone();
    let profile_dir = cli
        .path("profile-dir")
        .unwrap_or_else(|| log_dir.join(format!("browser-profile-{}", Uuid::new_v4().simple())));
    let evidence = cli
        .path("evidence")
        .unwrap_or_else(|| log_dir.join("browser-evidence.json"));
    let timeout = std::time::Duration::from_secs(cli.number("browser-timeout-secs", 60)?);
    Ok(RunInputs {
        config,
        endpoint,
        profile_dir,
        evidence,
        screenshot: cli.path("screenshot"),
        tooling: tooling_from(cli),
        timeout,
    })
}

fn server_inputs(cli: &Cli) -> Result<ManagedTestServerConfig, String> {
    if cli.argv.is_empty() {
        return Err("server argv is required after `--`".into());
    }
    let cwd = cli
        .path("cwd")
        .ok_or_else(|| "missing required --cwd".to_owned())?;
    let log_dir = cli
        .path("log-dir")
        .ok_or_else(|| "missing required --log-dir".to_owned())?;
    let endpoint = match cli.optional("endpoint") {
        Some(value) => Some(LoopbackEndpoint::parse(value).map_err(|error| error.to_string())?),
        None => None,
    };
    let startup_timeout = std::time::Duration::from_secs(cli.number("startup-timeout-secs", 30)?);
    let max_log_bytes = cli.number("max-log-bytes", 1024 * 1024)?;
    Ok(ManagedTestServerConfig {
        argv: cli.argv.clone(),
        cwd,
        endpoint,
        ready_path: cli.optional("ready-path").unwrap_or("/").into(),
        startup_timeout,
        log_dir,
        max_log_bytes,
    })
}

fn tooling_from(cli: &Cli) -> BrowserTooling {
    let mut tooling = BrowserTooling::from_env();
    if let Some(value) = cli.path("node") {
        tooling.node = value;
    }
    if let Some(value) = cli.path("runner") {
        tooling.runner = value;
    }
    if let Some(value) = cli.path("playwright-core") {
        tooling.playwright_core = Some(value);
    }
    if let Some(value) = cli.path("chromium") {
        tooling.chromium_executable = Some(value);
    }
    if let Some(value) = cli.path("browsers-path") {
        tooling.browsers_path = Some(value);
    }
    tooling
}

fn split_url(value: &str) -> Result<(LoopbackEndpoint, String), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "url is not a valid URL".to_owned())?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("url must be a plain http://127.0.0.1:<port>/... URL".into());
    }
    let endpoint = LoopbackEndpoint::parse(&format!(
        "http://127.0.0.1:{}",
        url.port()
            .filter(|port| *port != 0)
            .ok_or_else(|| "url has no authorized port".to_owned())?
    ))
    .map_err(|error| error.to_string())?;
    Ok((endpoint, url.path().to_owned()))
}

fn exit_for(status: BrowserCheckStatus) -> u8 {
    match status {
        BrowserCheckStatus::Passed => 0,
        BrowserCheckStatus::NotTested => EXIT_NOT_TESTED,
        BrowserCheckStatus::Failed => EXIT_FAILED,
    }
}

fn print_json(value: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
    );
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).ok();
        let mut interrupt = signal(SignalKind::interrupt()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async { if let Some(signal) = terminate.as_mut() { signal.recv().await; } } => {}
            _ = async { if let Some(signal) = interrupt.as_mut() { signal.recv().await; } } => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

const TOOLING_FLAGS: &[&str] = &[
    "node",
    "runner",
    "playwright-core",
    "chromium",
    "browsers-path",
];
const SERVER_FLAGS: &[&str] = &[
    "cwd",
    "log-dir",
    "endpoint",
    "ready-path",
    "startup-timeout-secs",
    "max-log-bytes",
];
const RUN_FLAGS: &[&str] = &[
    "cwd",
    "log-dir",
    "endpoint",
    "ready-path",
    "startup-timeout-secs",
    "max-log-bytes",
    "url-path",
    "selector",
    "text",
    "browser-timeout-secs",
    "profile-dir",
    "evidence",
    "screenshot",
    "node",
    "runner",
    "playwright-core",
    "chromium",
    "browsers-path",
];
const CHECK_FLAGS: &[&str] = &[
    "url",
    "selector",
    "text",
    "timeout-secs",
    "profile-dir",
    "evidence",
    "screenshot",
    "node",
    "runner",
    "playwright-core",
    "chromium",
    "browsers-path",
];
const START_FLAGS: &[&str] = &[
    "state-dir",
    "id",
    "cwd",
    "log-dir",
    "endpoint",
    "ready-path",
    "startup-timeout-secs",
    "max-log-bytes",
];
const STOP_FLAGS: &[&str] = &["state-file", "max-log-bytes"];
const CLEANUP_FLAGS: &[&str] = &["state-dir", "stop-live", "keep-logs"];
const TOOLS_FLAGS: &[&str] = TOOLING_FLAGS;
const BOOLEAN_FLAGS: &[&str] = &["stop-live", "keep-logs"];
