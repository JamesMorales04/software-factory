use std::{
    thread,
    time::{Duration, Instant},
};

use crate::{
    config,
    container::{self, CORE_MODEL_ENV_LABEL, RunSpec, StartAction, VolumeAction},
    core_client::get_health,
    core_url::CoreUrl,
    failure::Failure,
    managed_env, podman, runtime,
    tokens::{self, TokenFileState},
};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const START_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) fn run() -> u8 {
    match execute() {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("factory start: {}", failure.message);
            failure.code
        }
    }
}

fn execute() -> Result<(), Failure> {
    let raw_url = config::core_url().map_err(Failure::usage)?;
    let url = CoreUrl::parse(&raw_url).map_err(|error| {
        Failure::usage(format!("invalid FACTORY_CORE_URL {raw_url:?}: {error}"))
    })?;
    if !url.is_ipv4_localhost() {
        return Err(Failure::usage(
            "factory start publishes only to 127.0.0.1; FACTORY_CORE_URL must use 127.0.0.1 or localhost",
        ));
    }
    let port = url.port();

    let image = config::image().map_err(Failure::usage)?;
    podman::validate_image_reference(&image).map_err(Failure::usage)?;
    let name = config::container_name().map_err(Failure::usage)?;
    podman::validate_container_name(&name).map_err(Failure::usage)?;
    let volume = config::volume().map_err(Failure::usage)?;
    podman::validate_volume_name(&volume).map_err(Failure::usage)?;
    let token_file = config::token_file().map_err(Failure::usage)?;
    let project_roots = config::project_roots().map_err(Failure::usage)?;
    let core_model = managed_env::load_core_model().map_err(Failure::usage)?;
    let provider = managed_env::load_provider().map_err(Failure::usage)?;
    if provider.is_some() {
        if !config::host_execution().map_err(Failure::usage)? {
            return Err(Failure::usage(
                "FACTORY_PROVIDER_ENV_FILE requires FACTORY_HOST_EXECUTION=1 so the host Worker can receive it",
            ));
        }
        if project_roots.is_empty() {
            return Err(Failure::usage(
                "FACTORY_PROVIDER_ENV_FILE requires at least one FACTORY_PROJECT_ROOTS entry so factory start launches the host Worker",
            ));
        }
    }
    let env_file = token_file
        .to_str()
        .ok_or_else(|| {
            Failure::usage(format!(
                "token env file path {} is not valid UTF-8",
                token_file.display()
            ))
        })?
        .to_owned();

    println!("Factory start");
    println!("Image: {image}");
    println!("Container: {name}");
    println!("Data volume: {volume}");
    if let Some(core_model) = &core_model {
        println!("Core model env file: {}", core_model.path().display());
        println!("Core model route: {}", core_model.summary());
    } else {
        println!("Core model route: not configured");
    }
    if let Some(provider) = &provider {
        println!("Worker provider env file: {}", provider.path().display());
        println!("Worker provider profile: {}", provider.summary());
    } else {
        println!("Worker provider profile: not configured");
    }

    podman::version().map_err(Failure::operation)?;
    match podman::rootless() {
        Ok(true) => {}
        Ok(false) => {
            return Err(Failure::usage(
                "Podman is rootful; rootless Podman is required for this start path",
            ));
        }
        Err(error) => return Err(Failure::operation(error)),
    }
    match podman::image_exists(&image) {
        Ok(true) => {}
        Ok(false) => {
            return Err(Failure::usage(format!(
                "OCI image {image} is not present; build it with: podman build --network=host -t {image} -f deploy/Containerfile.core ."
            )));
        }
        Err(error) => return Err(Failure::operation(error)),
    }

    let token_state = tokens::ensure(&token_file).map_err(Failure::usage)?;
    println!("Token file: {}", token_file.display());

    let state = podman::container_state(&name).map_err(Failure::operation)?;
    let expected_fingerprint = core_model
        .as_ref()
        .map(managed_env::CoreModelEnv::fingerprint)
        .unwrap_or("unset");
    let current_fingerprint = if state.is_some() {
        podman::container_label(&name, CORE_MODEL_ENV_LABEL).map_err(Failure::operation)?
    } else {
        None
    };
    let action = container::start_decision(
        state.as_deref(),
        token_state == TokenFileState::Created,
        current_fingerprint.as_deref(),
        expected_fingerprint,
    );
    if action == StartAction::Replace && state.as_deref() == Some("running") {
        if token_state == TokenFileState::Created {
            println!("Token env file was created; recreating the container to apply new tokens");
        }
        if current_fingerprint.as_deref() != Some(expected_fingerprint) {
            println!(
                "Core model env fingerprint {expected_fingerprint} differs from the running container ({}); recreating",
                current_fingerprint.as_deref().unwrap_or("unset")
            );
        }
    }
    match action {
        StartAction::ReuseRunning => {
            println!("Container {name}: already running");
        }
        StartAction::Replace => {
            println!(
                "Container {name}: replacing existing state {}",
                state.as_deref().unwrap_or("unknown")
            );
            ensure_volume(&volume)?;
            replace_container(&name, state.as_deref())?;
            run_container(
                &name,
                &image,
                &volume,
                &env_file,
                core_model.as_ref(),
                port,
                &project_roots,
            )?;
        }
        StartAction::Create => {
            ensure_volume(&volume)?;
            run_container(
                &name,
                &image,
                &volume,
                &env_file,
                core_model.as_ref(),
                port,
                &project_roots,
            )?;
        }
    }

    println!("Office URL: http://127.0.0.1:{port}");
    wait_for_health(&url, &name)?;
    let host = runtime::start(&token_file, &raw_url, &project_roots, provider.as_ref())?;
    if host.enabled {
        println!("Host execution: enabled");
        println!("Sandbox Supervisor: {}", host.supervisor_url);
        println!("Host runtime state: {}", host.runtime_dir.display());
        if let Some(provider) = &provider {
            println!("Worker provider config: active ({})", provider.summary());
        } else {
            println!("Worker provider config: not configured");
        }
    } else {
        println!(
            "Host execution: disabled (set FACTORY_PROJECT_ROOTS to an explicit project root)"
        );
        println!("Worker provider config: inactive");
    }
    Ok(())
}

fn ensure_volume(volume: &str) -> Result<(), Failure> {
    match container::volume_action(podman::volume_exists(volume).map_err(Failure::operation)?) {
        VolumeAction::Reuse => println!("Data volume {volume}: reused"),
        VolumeAction::Create => {
            let args = vec!["volume".to_owned(), "create".to_owned(), volume.to_owned()];
            match podman::run(&args) {
                Ok(_) => println!("Data volume {volume}: created"),
                Err(error) => {
                    if podman::volume_exists(volume).map_err(Failure::operation)? {
                        println!("Data volume {volume}: reused");
                    } else {
                        return Err(Failure::operation(error));
                    }
                }
            }
        }
    }
    Ok(())
}

fn replace_container(name: &str, state: Option<&str>) -> Result<(), Failure> {
    if matches!(state, Some("running" | "paused")) {
        let args = vec![
            "stop".to_owned(),
            "--time".to_owned(),
            "10".to_owned(),
            name.to_owned(),
        ];
        podman::run(&args).map_err(Failure::operation)?;
    }
    let args = vec!["rm".to_owned(), "--force".to_owned(), name.to_owned()];
    podman::run(&args).map_err(Failure::operation)?;
    Ok(())
}

fn run_container(
    name: &str,
    image: &str,
    volume: &str,
    env_file: &str,
    core_model: Option<&managed_env::CoreModelEnv>,
    host_port: u16,
    project_roots: &[std::path::PathBuf],
) -> Result<(), Failure> {
    let core_model_env_file = core_model
        .map(|config| {
            config.path().to_str().ok_or_else(|| {
                Failure::usage(format!(
                    "Core model env file path {} is not valid UTF-8",
                    config.path().display()
                ))
            })
        })
        .transpose()?;
    let core_model_fingerprint = core_model
        .map(managed_env::CoreModelEnv::fingerprint)
        .unwrap_or("unset");
    let args = container::run_args(&RunSpec {
        name,
        image,
        volume,
        env_file,
        core_model_env_file,
        core_model_fingerprint,
        host_port,
        project_roots,
    });
    let container_id = podman::run(&args).map_err(Failure::operation)?;
    let short_id = container_id.chars().take(12).collect::<String>();
    if short_id.is_empty() {
        println!("Container {name}: started");
    } else {
        println!("Container {name}: started ({short_id})");
    }
    Ok(())
}

fn wait_for_health(url: &CoreUrl, name: &str) -> Result<(), Failure> {
    let deadline = Instant::now() + START_TIMEOUT;
    let mut last_problem;
    loop {
        last_problem = match get_health(url, HEALTH_TIMEOUT) {
            Ok(response) if response.status == 200 => match podman::container_state(name) {
                Ok(Some(state)) if state == "running" => {
                    println!("Core health: HTTP 200");
                    return Ok(());
                }
                Ok(state) => {
                    format!("health 200 but container state is {state:?}")
                }
                Err(error) => error,
            },
            Ok(response) => format!("HTTP {}", response.status),
            Err(error) => error.to_string(),
        };

        match podman::container_state(name).map_err(Failure::operation)? {
            Some(state) if matches!(state.as_str(), "exited" | "dead" | "stopped") => {
                return Err(Failure::operation(format!(
                    "container {name} is {state} before Core became healthy ({last_problem}); run 'factory logs' for details"
                )));
            }
            None => {
                return Err(Failure::operation(format!(
                    "container {name} disappeared before Core became healthy"
                )));
            }
            Some(_) => {}
        }

        if Instant::now() >= deadline {
            return Err(Failure::operation(format!(
                "Core did not become healthy within {} seconds ({last_problem}); run 'factory logs' for details",
                START_TIMEOUT.as_secs()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}
