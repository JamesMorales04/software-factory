use std::io::{self, IsTerminal, Write};

use crate::{config, failure::Failure, podman};

pub(crate) fn run(purge: bool) -> u8 {
    match execute(purge) {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("factory stop: {}", failure.message);
            failure.code
        }
    }
}

fn execute(purge: bool) -> Result<(), Failure> {
    let name = config::container_name().map_err(Failure::usage)?;
    podman::validate_container_name(&name).map_err(Failure::usage)?;
    let volume = config::volume().map_err(Failure::usage)?;
    podman::validate_volume_name(&volume).map_err(Failure::usage)?;

    println!("Factory stop");
    println!("Container: {name}");
    println!("Data volume: {volume}");

    let purge_confirmed = if purge {
        confirm_purge(&volume).map_err(Failure::operation)?
    } else {
        false
    };

    match podman::container_state(&name).map_err(Failure::operation)? {
        Some(state) => {
            if matches!(state.as_str(), "running" | "paused") {
                let args = vec![
                    "stop".to_owned(),
                    "--time".to_owned(),
                    "10".to_owned(),
                    name.clone(),
                ];
                podman::run(&args).map_err(Failure::operation)?;
                println!("Container {name}: stopped");
            }
            let args = vec!["rm".to_owned(), "--force".to_owned(), name.clone()];
            podman::run(&args).map_err(Failure::operation)?;
            println!("Container {name}: removed");
        }
        None => println!("Container {name}: not present"),
    }

    if purge {
        if purge_confirmed {
            purge_volume(&volume)?;
        } else {
            println!("Data volume {volume}: kept (purge was not confirmed)");
        }
    } else {
        println!("Data volume {volume}: kept");
    }
    Ok(())
}

fn purge_volume(volume: &str) -> Result<(), Failure> {
    if podman::volume_exists(volume).map_err(Failure::operation)? {
        let args = vec!["volume".to_owned(), "rm".to_owned(), volume.to_owned()];
        podman::run(&args).map_err(Failure::operation)?;
        println!("Data volume {volume}: removed");
    } else {
        println!("Data volume {volume}: not present");
    }
    Ok(())
}

fn confirm_purge(volume: &str) -> Result<bool, String> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    print!(
        "This permanently deletes data volume '{volume}' and the Factory database.\nType 'purge' to confirm: "
    );
    io::stdout()
        .flush()
        .map_err(|error| format!("cannot write confirmation prompt: {error}"))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| format!("cannot read confirmation: {error}"))?;
    Ok(is_confirmation(&input))
}

fn is_confirmation(input: &str) -> bool {
    input.trim() == "purge"
}

#[cfg(test)]
mod tests {
    use super::is_confirmation;

    #[test]
    fn requires_the_exact_purge_confirmation() {
        assert!(is_confirmation("purge"));
        assert!(is_confirmation("  purge\n"));
        assert!(!is_confirmation(""));
        assert!(!is_confirmation("yes"));
        assert!(!is_confirmation("purge please"));
    }
}
