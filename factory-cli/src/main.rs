mod args;
mod config;
mod container;
mod core_client;
mod core_url;
mod doctor;
mod failure;
mod logs;
mod managed_env;
mod podman;
mod report;
mod runtime;
mod start;
mod status;
mod stop;
mod text;
mod tokens;

use std::{env, process::ExitCode};

use args::Command;

fn main() -> ExitCode {
    let arguments = match env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| "argument is not valid UTF-8".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("factory: {error}");
            return ExitCode::from(2);
        }
    };

    match args::parse(arguments) {
        Ok(Command::Help) => {
            args::print_help();
            ExitCode::SUCCESS
        }
        Ok(Command::Doctor) => ExitCode::from(doctor::run()),
        Ok(Command::Start) => ExitCode::from(start::run()),
        Ok(Command::Stop { purge }) => ExitCode::from(stop::run(purge)),
        Ok(Command::Logs) => ExitCode::from(logs::run()),
        Ok(Command::Status) => ExitCode::from(status::run()),
        Ok(Command::NotImplemented(command)) => {
            eprintln!("factory {}: NOT_IMPLEMENTED", command.as_str());
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("factory: {error}");
            ExitCode::from(2)
        }
    }
}
