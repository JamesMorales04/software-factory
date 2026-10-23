use std::{
    thread,
    time::{Duration, Instant},
};

use crate::{
    config,
    container::{self, RunSpec, StartAction, VolumeAction},
    core_client::get_health,
    core_url::CoreUrl,
    failure::Failure,
    podman,
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
    let action = if state.as_deref() == Some("running") && token_state == TokenFileState::Created {
        println!("Token env file was created; recreating the container to apply new tokens");
        StartAction::Replace
    } else {
        container::start_action(state.as_deref())
    };
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
            run_container(&name, &image, &volume, &env_file, port)?;
        }
        StartAction::Create => {
            ensure_volume(&volume)?;
            run_container(&name, &image, &volume, &env_file, port)?;
        }
    }

    println!("Office URL: http://127.0.0.1:{port}");
    wait_for_health(&url, &name)?;
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
    host_port: u16,
) -> Result<(), Failure> {
    let args = container::run_args(&RunSpec {
        name,
        image,
        volume,
        env_file,
        host_port,
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
            Ok(response) if response.status == 200 => {
                println!("Core health: HTTP 200");
                return Ok(());
            }
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
