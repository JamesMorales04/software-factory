use std::process::Command;

use crate::cargo;

pub(crate) fn run(mut command: Command) -> Result<(), String> {
    let display = format!("{command:?}");
    eprintln!("+ {display}");
    let status = command
        .status()
        .map_err(|error| format!("failed to start {display}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed with {status}: {display}"))
    }
}

pub(crate) fn cargo_run(args: &[&str]) -> Result<(), String> {
    let mut command = cargo();
    command.args(args);
    run(command)
}
