use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::{
    config, core_client::get_health, core_url::CoreUrl, failure::Failure, managed_env, tokens,
};

const START_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const SUPERVISOR_TOKEN_FILE: &str = "supervisor.token";
const SUPERVISOR_PID_FILE: &str = "supervisor.pid";
const WORKER_PID_FILE: &str = "worker.pid";
const SUPERVISOR_LOG: &str = "supervisor.log";
const WORKER_LOG: &str = "worker.log";

pub(crate) struct HostRuntime {
    pub(crate) enabled: bool,
    pub(crate) supervisor_url: String,
    pub(crate) runtime_dir: PathBuf,
}

pub(crate) fn start(
    token_file: &Path,
    core_url: &str,
    roots: &[PathBuf],
    provider: Option<&managed_env::ProviderEnv>,
) -> Result<HostRuntime, Failure> {
    let runtime_dir = runtime_dir(token_file);
    let supervisor_url = config::supervisor_url().map_err(Failure::usage)?;
    let url = CoreUrl::parse(&supervisor_url)
        .map_err(|error| Failure::usage(format!("invalid FACTORY_SUPERVISOR_URL: {error}")))?;
    if !url.is_ipv4_localhost() {
        return Err(Failure::usage(
            "FACTORY_SUPERVISOR_URL must use a loopback address",
        ));
    }
    if !config::host_execution().map_err(Failure::usage)? || roots.is_empty() {
        return Ok(HostRuntime {
            enabled: false,
            supervisor_url,
            runtime_dir,
        });
    }
    prepare_runtime_dir(&runtime_dir).map_err(Failure::usage)?;
    let supervisor_token = load_or_create_supervisor_token(&runtime_dir).map_err(Failure::usage)?;
    let bootstrap =
        tokens::read(token_file, "FACTORY_WORKER_BOOTSTRAP_TOKEN").map_err(Failure::usage)?;
    let attempt_grant_key =
        tokens::read(token_file, "FACTORY_ATTEMPT_GRANT_KEY").map_err(Failure::usage)?;
    let capability_key =
        tokens::read(token_file, "FACTORY_SUPERVISOR_CAPABILITY_KEY").map_err(Failure::usage)?;
    let bin_dir = binary_dir()?;
    let supervisor_bin = bin_dir.join("factory-supervisor");
    let worker_bin = bin_dir.join("factory-worker");
    for binary in [&supervisor_bin, &worker_bin] {
        if !binary.is_file() {
            return Err(Failure::usage(format!(
                "host execution binary is missing: {}; set FACTORY_BIN_DIR",
                binary.display()
            )));
        }
    }
    let workspace_root = config::supervisor_workspace_root().map_err(Failure::usage)?;
    let state_root = config::supervisor_state_root().map_err(Failure::usage)?;
    for root in [&workspace_root, &state_root] {
        fs::create_dir_all(root).map_err(|error| {
            Failure::operation(format!("cannot create {}: {error}", root.display()))
        })?;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|error| {
            Failure::operation(format!(
                "cannot restrict permissions on {}: {error}",
                root.display()
            ))
        })?;
    }
    let roots_value = roots
        .iter()
        .map(|root| root.to_string_lossy())
        .collect::<Vec<_>>()
        .join(",");
    let images = config::supervisor_images().map_err(Failure::usage)?;
    let fixture_image = config::worker_fixture_image().map_err(Failure::usage)?;
    let supervisor_port = url.port();

    stop_process(&runtime_dir.join(WORKER_PID_FILE), &worker_bin)?;
    stop_process(&runtime_dir.join(SUPERVISOR_PID_FILE), &supervisor_bin)?;

    let supervisor_log = open_log(&runtime_dir.join(SUPERVISOR_LOG)).map_err(Failure::operation)?;
    let mut supervisor = Command::new(&supervisor_bin);
    supervisor
        .arg0("factory-supervisor")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("FACTORY_SUPERVISOR_TOKEN", &supervisor_token)
        .env("FACTORY_CORE_URL", core_url)
        .env("FACTORY_ATTEMPT_GRANT_KEY", &attempt_grant_key)
        .env("FACTORY_SUPERVISOR_CAPABILITY_KEY", &capability_key)
        .env(
            "FACTORY_SUPERVISOR_BIND",
            format!("127.0.0.1:{supervisor_port}"),
        )
        .env("FACTORY_SUPERVISOR_WORKSPACE_ROOT", &workspace_root)
        .env("FACTORY_SUPERVISOR_STATE_ROOT", &state_root)
        .env("FACTORY_SUPERVISOR_PROJECT_ROOTS", roots_value)
        .env("FACTORY_SUPERVISOR_IMAGES", &images)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(supervisor_log));
    copy_container_environment(&mut supervisor);
    supervisor.process_group(0);
    let supervisor_child = supervisor.spawn().map_err(|error| {
        Failure::operation(format!(
            "cannot start {}: {error}",
            supervisor_bin.display()
        ))
    })?;
    write_pid(
        &runtime_dir.join(SUPERVISOR_PID_FILE),
        supervisor_child.id(),
    )
    .map_err(Failure::operation)?;
    wait_for_health(&url, &runtime_dir.join(SUPERVISOR_LOG))?;
    let supervisor_exe =
        fs::read_link(format!("/proc/{}/exe", supervisor_child.id())).map_err(|error| {
            Failure::operation(format!("cannot verify Supervisor process: {error}"))
        })?;
    if !process_alive(supervisor_child.id()) || !same_binary(&supervisor_exe, &supervisor_bin) {
        return Err(Failure::operation(
            "Supervisor health did not come from the spawned Supervisor process",
        ));
    }

    let worker_log = open_log(&runtime_dir.join(WORKER_LOG)).map_err(Failure::operation)?;
    let mut worker = Command::new(&worker_bin);
    worker
        .arg0("factory-worker")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("FACTORY_CORE_URL", core_url)
        .env("FACTORY_WORKER_BOOTSTRAP_TOKEN", bootstrap)
        .env("FACTORY_WORKER_ID", "factory-host-worker")
        .env(
            "FACTORY_WORKER_REGISTRATION_KEY",
            "factory-host-worker-registration-v1",
        )
        .env("FACTORY_SUPERVISOR_URL", &supervisor_url)
        .env("FACTORY_SUPERVISOR_TOKEN", supervisor_token)
        .env("FACTORY_WORKER_FIXTURE_IMAGE", fixture_image)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(worker_log));
    copy_container_environment(&mut worker);
    apply_provider_environment(&mut worker, provider);
    worker.process_group(0);
    let worker_child = worker.spawn().map_err(|error| {
        Failure::operation(format!("cannot start {}: {error}", worker_bin.display()))
    })?;
    write_pid(&runtime_dir.join(WORKER_PID_FILE), worker_child.id()).map_err(Failure::operation)?;
    thread::sleep(Duration::from_millis(200));
    if !process_alive(worker_child.id()) {
        return Err(Failure::operation(format!(
            "Worker exited during startup; see {}",
            runtime_dir.join(WORKER_LOG).display()
        )));
    }
    Ok(HostRuntime {
        enabled: true,
        supervisor_url,
        runtime_dir,
    })
}

pub(crate) fn stop(token_file: &Path) -> Result<(), Failure> {
    let runtime_dir = runtime_dir(token_file);
    let bin_dir = binary_dir()?;
    stop_process(
        &runtime_dir.join(WORKER_PID_FILE),
        &bin_dir.join("factory-worker"),
    )?;
    stop_process(
        &runtime_dir.join(SUPERVISOR_PID_FILE),
        &bin_dir.join("factory-supervisor"),
    )?;
    Ok(())
}

pub(crate) fn status(token_file: &Path) -> Result<(bool, bool), Failure> {
    let runtime_dir = runtime_dir(token_file);
    let bin_dir = binary_dir()?;
    Ok((
        pid_status(
            &runtime_dir.join(SUPERVISOR_PID_FILE),
            &bin_dir.join("factory-supervisor"),
        )?,
        pid_status(
            &runtime_dir.join(WORKER_PID_FILE),
            &bin_dir.join("factory-worker"),
        )?,
    ))
}

fn runtime_dir(token_file: &Path) -> PathBuf {
    token_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("runtime")
}

fn prepare_runtime_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{} is not a real directory", path.display()));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("cannot restrict {}: {error}", path.display()))?;
    }
    Ok(())
}

fn load_or_create_supervisor_token(dir: &Path) -> Result<String, String> {
    let path = dir.join(SUPERVISOR_TOKEN_FILE);
    match fs::read_to_string(&path) {
        Ok(value) => {
            let value = value.trim().to_owned();
            if value.len() < 32 || value.len() > 256 {
                return Err("supervisor token file has an invalid value".into());
            }
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err("supervisor token file must be a private regular file".into());
            }
            Ok(value)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let token = tokens::generate_token()?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
            file.write_all(format!("{token}\n").as_bytes())
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            file.sync_all()
                .map_err(|error| format!("cannot flush {}: {error}", path.display()))?;
            Ok(token)
        }
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn binary_dir() -> Result<PathBuf, Failure> {
    if let Some(value) = std::env::var_os("FACTORY_BIN_DIR") {
        let value = value
            .into_string()
            .map_err(|_| Failure::usage("FACTORY_BIN_DIR is not valid UTF-8"))?;
        if value.is_empty() {
            return Err(Failure::usage("FACTORY_BIN_DIR is empty"));
        }
        return Ok(PathBuf::from(value));
    }
    let executable = std::env::current_exe().map_err(|error| {
        Failure::operation(format!("cannot locate the factory executable: {error}"))
    })?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Failure::operation("factory executable has no parent directory"))
}

fn open_log(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))
}

fn apply_provider_environment(command: &mut Command, provider: Option<&managed_env::ProviderEnv>) {
    if let Some(provider) = provider {
        command.envs(provider.environment());
    }
}

fn copy_container_environment(command: &mut Command) {
    for key in [
        "HOME",
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "TZ",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn wait_for_health(url: &CoreUrl, log: &Path) -> Result<(), Failure> {
    let deadline = Instant::now() + START_TIMEOUT;
    let mut last;
    loop {
        match get_health(url, Duration::from_secs(2)) {
            Ok(response) if response.status == 200 => return Ok(()),
            Ok(response) => last = format!("HTTP {}", response.status),
            Err(error) => last = error.to_string(),
        }
        if Instant::now() >= deadline {
            let log = fs::read_to_string(log).unwrap_or_default();
            return Err(Failure::operation(format!(
                "Sandbox Supervisor did not become healthy ({last}); log:\n{log}"
            )));
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn write_pid(path: &Path, pid: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    writeln!(file, "{pid}").map_err(|error| format!("cannot write pid: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot flush pid file: {error}"))
}

fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}/stat")).is_file()
}

fn pid_status(path: &Path, expected_binary: &Path) -> Result<bool, Failure> {
    let Some(pid) = read_pid(path) else {
        return Ok(false);
    };
    if !process_alive(pid) {
        return Ok(false);
    }
    let executable = fs::read_link(format!("/proc/{pid}/exe")).map_err(|error| {
        Failure::operation(format!("cannot inspect host service {pid}: {error}"))
    })?;
    Ok(same_binary(&executable, expected_binary))
}

fn stop_process(pid_path: &Path, expected: &Path) -> Result<(), Failure> {
    let Some(pid) = read_pid(pid_path) else {
        return Ok(());
    };
    if !process_alive(pid) {
        let _ = fs::remove_file(pid_path);
        return Ok(());
    }
    let executable = match fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(executable) => executable,
        Err(_) => {
            let _ = fs::remove_file(pid_path);
            return Ok(());
        }
    };
    if !same_binary(&executable, expected) {
        return Err(Failure::operation(format!(
            "refusing to stop process {pid}: expected {}, found {}",
            expected.display(),
            executable.display()
        )));
    }
    signal(pid, "-TERM")?;
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        if !process_alive(pid) {
            let _ = fs::remove_file(pid_path);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    signal(pid, "-KILL")?;
    thread::sleep(Duration::from_millis(200));
    let _ = fs::remove_file(pid_path);
    Ok(())
}

fn same_binary(actual: &Path, expected: &Path) -> bool {
    let actual = actual.to_string_lossy();
    let actual = actual.strip_suffix(" (deleted)").unwrap_or(&actual);
    Path::new(actual) == expected
}

fn signal(pid: u32, signal: &str) -> Result<(), Failure> {
    let status = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status()
        .map_err(|error| Failure::operation(format!("cannot signal process {pid}: {error}")))?;
    if !status.success() {
        return Err(Failure::operation(format!(
            "cannot signal process {pid}: kill exited with {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "factory-runtime-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn creates_a_private_stable_supervisor_token() {
        let temp = Temp::new();
        let first = load_or_create_supervisor_token(&temp.0).unwrap();
        let second = load_or_create_supervisor_token(&temp.0).unwrap();
        assert_eq!(first, second);
        assert!(first.len() >= 64);
        let metadata = fs::metadata(temp.0.join(SUPERVISOR_TOKEN_FILE)).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn rejects_symlinked_runtime_directories() {
        let temp = Temp::new();
        let target = temp.0.join("target");
        fs::create_dir(&target).unwrap();
        let link = temp.0.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(prepare_runtime_dir(&link).is_err());
    }

    #[test]
    fn applies_provider_values_only_to_the_requested_command() {
        use std::ffi::OsStr;

        let provider = managed_env::provider_for_test(&[
            ("FACTORY_PROVIDER_KIND", "opencode"),
            (
                "FACTORY_PROVIDER_UPSTREAM_TOKEN",
                "super-secret-upstream-token",
            ),
        ]);
        let mut worker = Command::new("factory-worker");
        worker.env_clear();
        apply_provider_environment(&mut worker, Some(&provider));
        let worker_environment = worker.get_envs().collect::<Vec<_>>();
        assert!(worker_environment.iter().any(|(key, value)| {
            *key == OsStr::new("FACTORY_PROVIDER_KIND") && *value == Some(OsStr::new("opencode"))
        }));
        assert!(worker_environment.iter().any(|(key, value)| {
            *key == OsStr::new("FACTORY_PROVIDER_UPSTREAM_TOKEN")
                && *value == Some(OsStr::new("super-secret-upstream-token"))
        }));

        let mut supervisor = Command::new("factory-supervisor");
        supervisor.env_clear();
        apply_provider_environment(&mut supervisor, None);
        assert_eq!(supervisor.get_envs().count(), 0);
    }
}
