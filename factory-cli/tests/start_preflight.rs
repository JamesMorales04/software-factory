use std::{
    fs,
    io::{ErrorKind, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "factory-cli-start-preflight-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("temp mode");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_private(path: &Path, content: &str) {
    fs::write(path, content).expect("write env file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("env mode");
}

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).expect("write executable");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("executable mode");
}

fn fake_podman(temp: &TempDir) -> (PathBuf, PathBuf) {
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("fake podman directory");
    let marker = temp.path().join("podman-called");
    write_executable(
        &bin.join("podman"),
        "#!/bin/sh\n\
         printf '%s\\n' \"$*\" >> \"$FACTORY_TEST_PODMAN_MARKER\"\n\
         if [ \"$1\" = \"--version\" ]; then echo 'podman version 6.1.4'; exit 0; fi\n\
         if [ \"$1\" = \"info\" ]; then echo true; exit 0; fi\n\
         if [ \"$1 $2\" = \"container exists\" ]; then exit 1; fi\n\
         exit 0\n",
    );
    (bin, marker)
}

fn fake_running_podman(temp: &TempDir) -> (PathBuf, PathBuf) {
    let bin = temp.path().join("running-bin");
    fs::create_dir(&bin).expect("fake podman directory");
    let marker = temp.path().join("podman-called");
    write_executable(
        &bin.join("podman"),
        "#!/bin/sh\n\
         printf '%s\\n' \"$*\" >> \"$FACTORY_TEST_PODMAN_MARKER\"\n\
         case \"$1\" in\n\
           --version) echo 'podman version 6.1.4'; exit 0;;\n\
           info) echo true; exit 0;;\n\
           inspect)\n\
             case \"$3\" in\n\
               '{{.State.Status}}') echo running; exit 0;;\n\
               *core-model-env-fingerprint*) echo old-fingerprint; exit 0;;\n\
             esac\n\
             exit 0;;\n\
           run) echo test-container-id; exit 0;;\n\
           *) exit 0;;\n\
         esac\n",
    );
    (bin, marker)
}

fn serve_health(listener: TcpListener, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_millis(500)))
                        .expect("health read timeout");
                    let mut request = [0_u8; 4096];
                    let _ = stream.read(&mut request);
                    let body = "{\"healthy\":true}";
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .expect("health response");
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("health accept: {error}"),
            }
        }
    })
}

fn factory_command(temp: &TempDir, bin: &Path, marker: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_factory"));
    command
        .env_clear()
        .env("PATH", bin)
        .env("HOME", temp.path())
        .env("FACTORY_ENV_FILE", temp.path().join("core.env"))
        .env("FACTORY_CORE_MODEL_ENV_FILE", temp.path().join("model.env"))
        .env(
            "FACTORY_PROVIDER_ENV_FILE",
            temp.path().join("provider.env"),
        )
        .env("FACTORY_TEST_PODMAN_MARKER", marker);
    command
}

fn run_start(temp: &TempDir, bin: &Path, marker: &Path) -> std::process::Output {
    factory_command(temp, bin, marker)
        .arg("start")
        .output()
        .expect("run factory start")
}

#[test]
fn start_fails_closed_before_podman_when_the_core_model_file_is_invalid() {
    let temp = TempDir::new();
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\nFACTORY_MODEL_TOKEN=short\n",
    );
    let (bin, marker) = fake_podman(&temp);
    let output = run_start(&temp, &bin, &marker);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FACTORY_MODEL_TOKEN"), "{stderr}");
    assert!(!stderr.contains("FACTORY_MODEL_TOKEN=short"), "{stderr}");
    assert!(!marker.exists(), "podman must not run for invalid config");
}

#[test]
fn start_fails_closed_before_podman_when_the_provider_file_is_invalid() {
    let temp = TempDir::new();
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n",
    );
    write_private(
        &temp.path().join("provider.env"),
        "FACTORY_PROVIDER_SANDBOX=bwrap\nFACTORY_UNKNOWN_KEY=1\n",
    );
    let (bin, marker) = fake_podman(&temp);
    let output = run_start(&temp, &bin, &marker);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FACTORY_UNKNOWN_KEY"), "{stderr}");
    assert!(!marker.exists(), "podman must not run for invalid config");
}

#[test]
fn start_fails_closed_before_podman_when_provider_config_has_no_host_worker() {
    let temp = TempDir::new();
    let binary = temp.path().join("opencode");
    let bridge = temp.path().join("provider-bridge");
    let egress = temp.path().join("egress-bridge");
    for path in [&binary, &bridge, &egress] {
        write_executable(path, "#!/bin/sh\n");
    }
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n",
    );
    write_private(
        &temp.path().join("provider.env"),
        &format!(
            "FACTORY_PROVIDER_SANDBOX=bwrap\n\
             FACTORY_PROVIDER_KIND=opencode\n\
             FACTORY_PROVIDER_BINARY={}\n\
             FACTORY_PROVIDER_BRIDGE={}\n\
             FACTORY_PROVIDER_EGRESS_BRIDGE={}\n\
             FACTORY_PROVIDER_UPSTREAM_URL=https://opencode.ai/inference/go/openai/v1\n\
             FACTORY_PROVIDER_UPSTREAM_TOKEN=super-secret-upstream-token\n",
            binary.display(),
            bridge.display(),
            egress.display()
        ),
    );
    let (bin, marker) = fake_podman(&temp);
    let output = run_start(&temp, &bin, &marker);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FACTORY_PROJECT_ROOTS"), "{stderr}");
    assert!(!stderr.contains("super-secret"), "{stderr}");
    assert!(
        !marker.exists(),
        "podman must not run for inactive provider config"
    );
}

#[test]
fn doctor_fails_closed_on_an_invalid_core_model_file_without_printing_secrets() {
    let temp = TempDir::new();
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n\
         FACTORY_UNKNOWN_KEY=1\n",
    );
    let (bin, marker) = fake_podman(&temp);
    let mut command = factory_command(&temp, &bin, &marker);
    command.env_remove("FACTORY_PROVIDER_ENV_FILE");
    let output = command.arg("doctor").output().expect("run factory doctor");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FAIL    Core model env"), "{stdout}");
    assert!(stdout.contains("FACTORY_UNKNOWN_KEY"), "{stdout}");
    assert!(stdout.contains("doctor: FAIL"), "{stdout}");
    assert!(!stdout.contains("super-secret-model-token"), "{stdout}");
}

#[test]
fn doctor_accepts_a_valid_core_model_file_and_redacts_its_token() {
    let temp = TempDir::new();
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n\
         FACTORY_MODEL_PROVIDER=opencode-go\n\
         FACTORY_MODEL_NAME=deepseek-v4.1-flash\n",
    );
    let (bin, marker) = fake_podman(&temp);
    let mut command = factory_command(&temp, &bin, &marker);
    command.env_remove("FACTORY_PROVIDER_ENV_FILE");
    let output = command.arg("doctor").output().expect("run factory doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS    Core model env"), "{stdout}");
    assert!(stdout.contains("doctor: PASS"), "{stdout}");
    assert!(!stdout.contains("super-secret-model-token"), "{stdout}");
}

#[test]
fn status_shows_redacted_route_profiles_without_the_secret() {
    let temp = TempDir::new();
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n\
         FACTORY_MODEL_PROVIDER=opencode-go\n\
         FACTORY_MODEL_NAME=deepseek-v4.1-flash\n",
    );
    let (bin, marker) = fake_podman(&temp);
    let mut command = factory_command(&temp, &bin, &marker);
    command.env_remove("FACTORY_PROVIDER_ENV_FILE");
    let output = command.arg("status").output().expect("run factory status");
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Core model route:"), "{stdout}");
    assert!(stdout.contains("provider=opencode-go"), "{stdout}");
    assert!(
        stdout.contains("Worker provider profile: not configured"),
        "{stdout}"
    );
    assert!(!stdout.contains("super-secret-model-token"), "{stdout}");
}

#[test]
fn start_replaces_a_running_core_container_with_a_changed_model_fingerprint() {
    let temp = TempDir::new();
    let model_path = temp.path().join("model.env");
    write_private(
        &model_path,
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n",
    );
    let (bin, marker) = fake_running_podman(&temp);
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("health listener");
    let port = listener.local_addr().expect("health address").port();
    let stop = Arc::new(AtomicBool::new(false));
    let health = serve_health(listener, Arc::clone(&stop));
    let mut command = factory_command(&temp, &bin, &marker);
    command
        .env("FACTORY_CORE_URL", format!("http://127.0.0.1:{port}"))
        .env("FACTORY_TEST_PODMAN_MARKER", &marker)
        .env_remove("FACTORY_PROVIDER_ENV_FILE");
    let output = command.arg("start").output().expect("run factory start");
    stop.store(true, Ordering::Relaxed);
    health.join().expect("health server");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("recreating"), "{stdout}");
    assert!(stdout.contains("Core health: HTTP 200"), "{stdout}");
    assert!(!stdout.contains("super-secret-model-token"), "{stdout}");

    let calls = fs::read_to_string(&marker).expect("podman calls");
    let run = calls
        .lines()
        .find(|line| line.starts_with("run "))
        .expect("container run call");
    assert!(run.contains("--env-file"), "{run}");
    assert!(run.contains(model_path.to_str().unwrap()), "{run}");
    assert!(run.contains("factory.core-model-env-fingerprint="), "{run}");
    assert!(!run.contains("old-fingerprint"), "{run}");
    assert!(!run.contains("super-secret-model-token"), "{run}");
    assert!(
        calls.lines().any(|line| line.starts_with("rm --force ")),
        "{calls}"
    );
}

#[test]
fn doctor_accepts_a_codex_provider_file_with_explicit_rates() {
    let temp = TempDir::new();
    let binary = temp.path().join("codex");
    let egress = temp.path().join("egress-bridge");
    for path in [&binary, &egress] {
        write_executable(path, "#!/bin/sh\n");
    }
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n",
    );
    write_private(
        &temp.path().join("provider.env"),
        &format!(
            "FACTORY_PROVIDER_SANDBOX=bwrap\n\
             FACTORY_PROVIDER_KIND=codex\n\
             FACTORY_PROVIDER_BINARY={}\n\
             FACTORY_PROVIDER_EGRESS_BRIDGE={}\n\
             FACTORY_PROVIDER_UPSTREAM_URL=https://api.openai.com/v1\n\
             FACTORY_PROVIDER_UPSTREAM_TOKEN=super-secret-upstream-token\n\
             FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION=100000\n\
             FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION=200000\n",
            binary.display(),
            egress.display()
        ),
    );
    let (bin, marker) = fake_podman(&temp);
    let roots = temp.path().join("roots");
    fs::create_dir(&roots).expect("project roots");
    let output = factory_command(&temp, &bin, &marker)
        .env("FACTORY_HOST_EXECUTION", "1")
        .env("FACTORY_PROJECT_ROOTS", &roots)
        .env("FACTORY_BIN_DIR", temp.path())
        .arg("doctor")
        .output()
        .expect("run factory doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PASS    Worker provider env"), "{stdout}");
    assert!(!stdout.contains("super-secret-upstream-token"), "{stdout}");
}

#[test]
fn doctor_rejects_a_codex_provider_file_without_cost_rates() {
    let temp = TempDir::new();
    let binary = temp.path().join("codex");
    let egress = temp.path().join("egress-bridge");
    for path in [&binary, &egress] {
        write_executable(path, "#!/bin/sh\n");
    }
    write_private(
        &temp.path().join("model.env"),
        "FACTORY_MODEL_URL=https://provider.example/v1\n\
         FACTORY_MODEL_TOKEN=super-secret-model-token\n",
    );
    write_private(
        &temp.path().join("provider.env"),
        &format!(
            "FACTORY_PROVIDER_SANDBOX=bwrap\n\
             FACTORY_PROVIDER_KIND=codex\n\
             FACTORY_PROVIDER_BINARY={}\n\
             FACTORY_PROVIDER_EGRESS_BRIDGE={}\n\
             FACTORY_PROVIDER_UPSTREAM_URL=https://api.openai.com/v1\n\
             FACTORY_PROVIDER_UPSTREAM_TOKEN=super-secret-upstream-token\n",
            binary.display(),
            egress.display()
        ),
    );
    let (bin, marker) = fake_podman(&temp);
    let output = factory_command(&temp, &bin, &marker)
        .arg("doctor")
        .output()
        .expect("run factory doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FAIL    Worker provider env"), "{stdout}");
    assert!(
        stdout.contains("FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION"),
        "{stdout}"
    );
    assert!(!stdout.contains("super-secret-upstream-token"), "{stdout}");
}
