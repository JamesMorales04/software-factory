use std::time::Duration;

use crate::{
    config, container::CORE_MODEL_ENV_LABEL, core_client::get_health, core_url::CoreUrl,
    failure::Failure, managed_env, podman, report::Check, runtime, text::one_line,
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

    let mut managed_config_ok = true;
    let core_model = match managed_env::load_core_model() {
        Ok(Some(configured)) => {
            println!("Core model env file: {}", configured.path().display());
            println!("Core model route: {}", configured.summary());
            Some(configured)
        }
        Ok(None) => {
            println!("Core model route: not configured");
            None
        }
        Err(error) => {
            Check::fail("Core model env", error).print();
            managed_config_ok = false;
            None
        }
    };
    let mut provider_configured = false;
    let mut provider_invalid = false;
    let provider = match managed_env::load_provider() {
        Ok(Some(configured)) => {
            provider_configured = true;
            println!("Worker provider env file: {}", configured.path().display());
            println!("Worker provider profile: {}", configured.summary());
            Some(configured)
        }
        Ok(None) => {
            println!("Worker provider profile: not configured");
            None
        }
        Err(error) => {
            Check::fail("Worker provider env", error).print();
            managed_config_ok = false;
            provider_invalid = true;
            None
        }
    };

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

    let mut host_services_running = false;
    let mut host_services_expected = false;
    let mut worker_running = false;
    if let Ok(roots) = config::project_roots() {
        host_services_expected = !roots.is_empty() && config::host_execution().unwrap_or(false);
        println!("Project roots: {}", roots.len());
        if let Ok(token_file) = config::token_file() {
            match runtime::status(&token_file) {
                Ok((supervisor, worker)) => {
                    worker_running = worker;
                    host_services_running = supervisor && worker;
                    if host_services_expected {
                        if supervisor {
                            Check::pass("Sandbox Supervisor", "running").print();
                        } else {
                            Check::fail("Sandbox Supervisor", "not running").print();
                        }
                        if worker {
                            Check::pass("Host Worker", "running").print();
                        } else {
                            Check::fail("Host Worker", "not running").print();
                        }
                    } else if supervisor || worker {
                        Check::blocked("Host execution", "services are running but disabled")
                            .print();
                    } else {
                        Check::pass("Host execution", "disabled").print();
                    }
                }
                Err(error) => {
                    Check::fail("Host execution", error.message).print();
                    host_services_running = false;
                }
            }
        }
    }

    let provider_ok = if provider_invalid {
        false
    } else if provider_configured {
        match &provider {
            Some(_) if !host_services_expected => {
                Check::fail(
                    "Worker provider config",
                    "configured but host execution is disabled or FACTORY_PROJECT_ROOTS is empty",
                )
                .print();
                false
            }
            Some(provider) if worker_running => {
                Check::pass(
                    "Worker provider config",
                    format!("active ({})", provider.summary()),
                )
                .print();
                true
            }
            Some(_) => {
                Check::fail(
                    "Worker provider config",
                    "configured but host Worker is not running",
                )
                .print();
                false
            }
            None => false,
        }
    } else {
        Check::pass("Worker provider config", "not configured").print();
        true
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
                match podman::container_label(&name, CORE_MODEL_ENV_LABEL) {
                    Ok(current) => {
                        let expected = core_model
                            .as_ref()
                            .map(managed_env::CoreModelEnv::fingerprint)
                            .unwrap_or("unset");
                        if current.as_deref() == Some(expected) {
                            Check::pass(
                                "Core model config",
                                format!("container fingerprint={expected}"),
                            )
                            .print();
                        } else {
                            Check::fail(
                                "Core model config",
                                format!(
                                    "container fingerprint={} but configured fingerprint={expected}; run 'factory start' to recreate",
                                    current.as_deref().unwrap_or("unset")
                                ),
                            )
                            .print();
                            managed_config_ok = false;
                        }
                    }
                    Err(error) => {
                        Check::fail("Core model config", error).print();
                        managed_config_ok = false;
                    }
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

    let host_ok = !host_services_expected || host_services_running;
    if container_running && core_healthy && host_ok && provider_ok && managed_config_ok {
        Ok(0)
    } else {
        Ok(1)
    }
}
