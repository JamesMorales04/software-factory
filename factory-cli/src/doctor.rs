use std::time::Duration;

use crate::{
    config, core_client::get_health, core_url::CoreUrl, managed_env, podman, report::Check,
    text::one_line,
};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn run() -> u8 {
    println!("Factory doctor");
    let mut checks = Vec::new();
    let core_url_is_explicit = config::core_url_is_explicit();

    let core_url = match config::core_url() {
        Ok(raw) => match CoreUrl::parse(&raw) {
            Ok(url) => {
                checks.push(Check::pass("Core URL", url.to_string()));
                Some(url)
            }
            Err(error) => {
                checks.push(Check::fail(
                    "Core URL",
                    format!("FACTORY_CORE_URL is invalid: {error}"),
                ));
                None
            }
        },
        Err(error) => {
            checks.push(Check::fail("Core URL", error));
            None
        }
    };

    match managed_env::load_core_model() {
        Ok(Some(configured)) => checks.push(Check::pass(
            "Core model env",
            format!("{}; {}", configured.path().display(), configured.summary()),
        )),
        Ok(None) => checks.push(Check::pass("Core model env", "not configured")),
        Err(error) => checks.push(Check::fail("Core model env", error)),
    }

    match managed_env::load_provider() {
        Ok(Some(configured)) => {
            let host_ready = config::host_execution().unwrap_or(false)
                && config::project_roots()
                    .map(|roots| !roots.is_empty())
                    .unwrap_or(false);
            if host_ready {
                checks.push(Check::pass(
                    "Worker provider env",
                    format!("{}; {}", configured.path().display(), configured.summary()),
                ));
            } else {
                checks.push(Check::fail(
                    "Worker provider env",
                    format!(
                        "{} is configured but FACTORY_HOST_EXECUTION=1 and an explicit FACTORY_PROJECT_ROOTS entry are required",
                        configured.path().display()
                    ),
                ));
            }
        }
        Ok(None) => checks.push(Check::pass("Worker provider env", "not configured")),
        Err(error) => checks.push(Check::fail("Worker provider env", error)),
    }

    let podman_available = match podman::version() {
        Ok(version) => {
            checks.push(Check::pass("Podman executable", version));
            true
        }
        Err(error) => {
            checks.push(Check::fail("Podman executable", error));
            false
        }
    };

    if podman_available {
        match podman::rootless() {
            Ok(true) => checks.push(Check::pass("Podman rootless", "true")),
            Ok(false) => checks.push(Check::fail(
                "Podman rootless",
                "rootless=false; rootful Podman is not a supported start path",
            )),
            Err(error) => checks.push(Check::fail("Podman rootless", error)),
        }
        check_image(&mut checks);
    } else {
        checks.push(Check::blocked(
            "Podman rootless",
            "podman executable is unavailable",
        ));
        checks.push(Check::blocked(
            "OCI runtime image",
            "podman executable is unavailable",
        ));
    }

    if let Some(url) = core_url {
        let name = if core_url_is_explicit {
            "Core health"
        } else {
            "Core health [informational]"
        };
        let check = match get_health(&url, HEALTH_TIMEOUT) {
            Ok(response) if response.status == 200 => {
                Check::pass(name, format!("HTTP 200 {}", one_line(&response.body, 300)))
            }
            Ok(response) => Check::fail(name, format!("HTTP {}", response.status)),
            Err(error) => Check::fail(name, error.to_string()),
        };
        checks.push(if core_url_is_explicit {
            check
        } else {
            check.informational()
        });
    }

    for check in &checks {
        check.print();
    }

    let required_failures = checks
        .iter()
        .filter(|check| check.is_required_failure())
        .count();
    if required_failures == 0 {
        println!("doctor: PASS (required start prerequisites are satisfied)");
        0
    } else {
        println!("doctor: FAIL ({required_failures} required prerequisite check(s) failed)");
        1
    }
}

fn check_image(checks: &mut Vec<Check>) {
    let image = match config::image() {
        Ok(image) => image,
        Err(error) => {
            checks.push(Check::fail("OCI runtime image", error));
            return;
        }
    };
    if let Err(error) = podman::validate_image_reference(&image) {
        checks.push(Check::fail("OCI runtime image", error));
        return;
    }
    match podman::image_exists(&image) {
        Ok(true) => checks.push(Check::pass(
            "OCI runtime image",
            format!("{image} is present"),
        )),
        Ok(false) => checks.push(Check::fail(
            "OCI runtime image",
            format!("{image} is not present"),
        )),
        Err(error) => checks.push(Check::fail("OCI runtime image", error)),
    }
}
