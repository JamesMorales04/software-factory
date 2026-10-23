use std::time::Duration;

use crate::{
    config, core_client::get_health, core_url::CoreUrl, podman, report::Check, text::one_line,
};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn run() -> u8 {
    let raw = match config::core_url() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("factory status: {error}");
            return 2;
        }
    };
    let url = match CoreUrl::parse(&raw) {
        Ok(url) => url,
        Err(error) => {
            eprintln!("factory status: invalid FACTORY_CORE_URL: {error}");
            return 2;
        }
    };

    println!("Core URL: {url}");
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

    print_podman_summary();

    if core_healthy { 0 } else { 1 }
}

fn print_podman_summary() {
    let version = match podman::version() {
        Ok(version) => version,
        Err(error) => {
            Check::blocked("Podman", error).print();
            Check::blocked("OCI runtime image", "podman executable is unavailable").print();
            return;
        }
    };
    Check::pass("Podman", version).print();

    match podman::rootless() {
        Ok(true) => Check::pass("Podman rootless", "true").print(),
        Ok(false) => Check::fail("Podman rootless", "false").print(),
        Err(error) => Check::fail("Podman rootless", error).print(),
    }

    let image = match config::image() {
        Ok(image) => image,
        Err(error) => {
            Check::fail("OCI runtime image", error).print();
            return;
        }
    };
    if let Err(error) = podman::validate_image_reference(&image) {
        Check::fail("OCI runtime image", error).print();
        return;
    }
    match podman::image_exists(&image) {
        Ok(true) => Check::pass("OCI runtime image", format!("{image} is present")).print(),
        Ok(false) => Check::fail("OCI runtime image", format!("{image} is not present")).print(),
        Err(error) => Check::fail("OCI runtime image", error).print(),
    }
}
