use std::time::Duration;

use crate::{
    config, core_client::get_health, core_url::CoreUrl, failure::Failure, podman, report::Check,
    text::one_line,
};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn run() -> u8 {
    match execute() {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("factory status: {}", failure.message);
            failure.code
        }
    }
}

fn execute() -> Result<u8, Failure> {
    let name = config::container_name().map_err(Failure::usage)?;
    podman::validate_container_name(&name).map_err(Failure::usage)?;
    let volume = config::volume().map_err(Failure::usage)?;
    podman::validate_volume_name(&volume).map_err(Failure::usage)?;
    let image = config::image().map_err(Failure::usage)?;
    podman::validate_image_reference(&image).map_err(Failure::usage)?;
    let raw_url = config::core_url().map_err(Failure::usage)?;
    let url = CoreUrl::parse(&raw_url).map_err(|error| {
        Failure::usage(format!("invalid FACTORY_CORE_URL {raw_url:?}: {error}"))
    })?;

    println!("Factory status");
    println!("Container: {name}");
    println!("Data volume: {volume}");
    println!("Image: {image}");
    println!("Core URL: {url}");
    if let Ok(path) = config::token_file() {
        println!("Token file: {}", path.display());
    }

    let podman_available = match podman::version() {
        Ok(version) => {
            Check::pass("Podman executable", version).print();
            true
        }
        Err(error) => {
            Check::blocked("Podman executable", error).print();
            false
        }
    };

    let mut container_running = false;
    if podman_available {
        match podman::rootless() {
            Ok(true) => Check::pass("Podman rootless", "true").print(),
            Ok(false) => Check::fail("Podman rootless", "false").print(),
            Err(error) => Check::fail("Podman rootless", error).print(),
        }

        match podman::container_state(&name) {
            Ok(Some(state)) => {
                container_running = state == "running";
                if container_running {
                    Check::pass("Container state", state).print();
                } else {
                    Check::fail("Container state", state).print();
                }
            }
            Ok(None) => Check::fail("Container state", "not present").print(),
            Err(error) => Check::fail("Container state", error).print(),
        }

        match podman::image_exists(&image) {
            Ok(true) => Check::pass("OCI runtime image", format!("{image} is present")).print(),
            Ok(false) => {
                Check::fail("OCI runtime image", format!("{image} is not present")).print()
            }
            Err(error) => Check::fail("OCI runtime image", error).print(),
        }
    } else {
        Check::blocked("Podman rootless", "podman executable is unavailable").print();
        Check::fail("Container state", "podman executable is unavailable").print();
        Check::blocked("OCI runtime image", "podman executable is unavailable").print();
    }

    let core_healthy = match get_health(&url, HEALTH_TIMEOUT) {
        Ok(response) if response.status == 200 => {
            Check::pass(
                "Core health",
                format!("HTTP 200 {}", one_line(&response.body, 300)),
            )
            .print();
            true
        }
        Ok(response) => {
            Check::fail("Core health", format!("HTTP {}", response.status)).print();
            false
        }
        Err(error) => {
            Check::fail("Core health", error.to_string()).print();
            false
        }
    };

    if container_running && core_healthy {
        Ok(0)
    } else {
        Ok(1)
    }
}
