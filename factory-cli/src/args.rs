use std::fmt;

const HELP: &str = "\
Software Factory CLI

Usage:
  factory <command> [options]
  factory --help

Commands:
  start            Create or reuse the data volume, start the Core container
                   and wait for Core /health through the loopback publication
  stop [--purge]   Stop and remove the Core container; --purge also removes the
                   data volume after explicit confirmation
  logs             Follow the Core container logs
  status           Show container state and Core health
  doctor           Check host runtime prerequisites for the container start path

NOT_IMPLEMENTED commands (exit non-zero):
  install
  restart
  update
  dev

Configuration:
  FACTORY_CORE_URL       Loopback HTTP origin (default http://127.0.0.1:8765)
  FACTORY_IMAGE          OCI runtime image (default localhost/software-factory-core:dev)
  FACTORY_CONTAINER_NAME Container name (default factory-core)
  FACTORY_VOLUME         Data volume name (default factory-core-data)
  FACTORY_ENV_FILE       Private token env file (default
                         $XDG_CONFIG_HOME/factory/core.env)

Security:
  Start publishes only 127.0.0.1, drops all capabilities, uses a read-only root
  filesystem, and never mounts the Podman socket. Container checks do not
  certify isolation. Tokens are generated into the private env file and are
  never printed.
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    Install,
    Restart,
    Update,
    Dev,
}

impl Lifecycle {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "install" => Some(Self::Install),
            "restart" => Some(Self::Restart),
            "update" => Some(Self::Update),
            "dev" => Some(Self::Dev),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Restart => "restart",
            Self::Update => "update",
            Self::Dev => "dev",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Doctor,
    Start,
    Stop { purge: bool },
    Logs,
    Status,
    Help,
    NotImplemented(Lifecycle),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseError(String);

impl ParseError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) fn parse<I, S>(args: I) -> Result<Command, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|argument| argument.as_ref().to_owned())
        .collect();
    let Some(first) = args.first() else {
        return Ok(Command::Help);
    };
    let options = &args[1..];
    match first.as_str() {
        "-h" | "--help" | "help" => {
            require_no_options(options)?;
            Ok(Command::Help)
        }
        "doctor" => {
            require_no_options(options)?;
            Ok(Command::Doctor)
        }
        "start" => {
            require_no_options(options)?;
            Ok(Command::Start)
        }
        "stop" => parse_stop(options),
        "logs" => {
            require_no_options(options)?;
            Ok(Command::Logs)
        }
        "status" => {
            require_no_options(options)?;
            Ok(Command::Status)
        }
        other => match Lifecycle::parse(other) {
            Some(command) => {
                require_no_options(options)?;
                Ok(Command::NotImplemented(command))
            }
            None => Err(ParseError::new(format!(
                "unknown command '{other}'; run 'factory --help'"
            ))),
        },
    }
}

fn parse_stop(options: &[String]) -> Result<Command, ParseError> {
    let mut purge = false;
    for option in options {
        match option.as_str() {
            "--purge" if !purge => purge = true,
            "--purge" => return Err(ParseError::new("duplicate option '--purge'")),
            other => {
                return Err(ParseError::new(format!(
                    "unexpected argument '{other}' for 'factory stop'; usage: factory stop [--purge]"
                )));
            }
        }
    }
    Ok(Command::Stop { purge })
}

fn require_no_options(options: &[String]) -> Result<(), ParseError> {
    match options.first() {
        Some(option) => Err(ParseError::new(format!(
            "unexpected argument '{option}'; run 'factory --help'"
        ))),
        None => Ok(()),
    }
}

pub(crate) fn print_help() {
    print!("{HELP}");
}

#[cfg(test)]
mod tests {
    use super::{Command, Lifecycle, parse};

    #[test]
    fn parses_implemented_commands() {
        assert_eq!(parse(["doctor"]), Ok(Command::Doctor));
        assert_eq!(parse(["start"]), Ok(Command::Start));
        assert_eq!(parse(["logs"]), Ok(Command::Logs));
        assert_eq!(parse(["status"]), Ok(Command::Status));
        assert_eq!(parse(["stop"]), Ok(Command::Stop { purge: false }));
        assert_eq!(
            parse(["stop", "--purge"]),
            Ok(Command::Stop { purge: true })
        );
    }

    #[test]
    fn parses_help_forms() {
        assert_eq!(parse(std::iter::empty::<&str>()), Ok(Command::Help));
        assert_eq!(parse(["--help"]), Ok(Command::Help));
        assert_eq!(parse(["-h"]), Ok(Command::Help));
        assert_eq!(parse(["help"]), Ok(Command::Help));
    }

    #[test]
    fn parses_unimplemented_lifecycle_commands() {
        for (name, expected) in [
            ("install", Lifecycle::Install),
            ("restart", Lifecycle::Restart),
            ("update", Lifecycle::Update),
            ("dev", Lifecycle::Dev),
        ] {
            assert_eq!(
                parse([name]),
                Ok(Command::NotImplemented(expected)),
                "command {name}"
            );
        }
    }

    #[test]
    fn rejects_unknown_and_extra_arguments() {
        assert!(parse(["launch"]).is_err());
        assert!(parse(["doctor", "extra"]).is_err());
        assert!(parse(["--help", "extra"]).is_err());
        assert!(parse(["stop", "--purge", "--purge"]).is_err());
        assert!(parse(["stop", "--force"]).is_err());
        assert!(parse(["logs", "extra"]).is_err());
    }
}
