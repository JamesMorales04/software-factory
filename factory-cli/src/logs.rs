use crate::{config, failure::Failure, podman};

pub(crate) fn run() -> u8 {
    match execute() {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("factory logs: {}", failure.message);
            failure.code
        }
    }
}

fn execute() -> Result<u8, Failure> {
    let name = config::container_name().map_err(Failure::usage)?;
    podman::validate_container_name(&name).map_err(Failure::usage)?;

    match podman::container_exists(&name).map_err(Failure::operation)? {
        true => podman::logs_follow(&name)
            .map(|code| u8::try_from(code).unwrap_or(1))
            .map_err(Failure::operation),
        false => Err(Failure::operation(format!(
            "container {name} is not present; run 'factory start' first"
        ))),
    }
}
