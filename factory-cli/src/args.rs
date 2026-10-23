use std::fmt;

const HELP: &str = "\
Software Factory CLI

Usage:
  factory <command>
  factory --help

Implemented commands:
  doctor    Check host runtime prerequisites for the container start path
  status    Query the Factory Core /health endpoint

NOT_IMPLEMENTED lifecycle commands (exit non-zero):
  install
  start
  stop
  restart
  logs
  update
  dev

Configuration:
  FACTORY_CORE_URL  Loopback HTTP origin (default http://127.0.0.1:8765)
  FACTORY_IMAGE     OCI runtime image (default localhost/software-factory-core:dev)

Container checks do not certify isolation.
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lifecycle {
    Install,
    Start,
    Stop,
    Restart,
    Logs,
    Update,
    Dev,
}

impl Lifecycle {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "install" => Some(Self::Install),
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            "logs" => Some(Self::Logs),
            "update" => Some(Self::Update),
            "dev" => Some(Self::Dev),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Logs => "logs",
            Self::Update => "update",
            Self::Dev => "dev",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    Doctor,
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
    if args.len() > 1 {
        return Err(ParseError::new(format!(
            "unexpected argument '{}'; run 'factory --help'",
            args[1]
        )));
    }
    match first.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "doctor" => Ok(Command::Doctor),
        "status" => Ok(Command::Status),
        other => match Lifecycle::parse(other) {
            Some(command) => Ok(Command::NotImplemented(command)),
            None => Err(ParseError::new(format!(
                "unknown command '{other}'; run 'factory --help'"
            ))),
        },
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
        assert_eq!(parse(["status"]), Ok(Command::Status));
    }

    #[test]
    fn parses_help_forms() {
        assert_eq!(parse(std::iter::empty::<&str>()), Ok(Command::Help));
        assert_eq!(parse(["--help"]), Ok(Command::Help));
        assert_eq!(parse(["-h"]), Ok(Command::Help));
        assert_eq!(parse(["help"]), Ok(Command::Help));
    }

    #[test]
    fn parses_lifecycle_commands_as_not_implemented() {
        for (name, expected) in [
            ("install", Lifecycle::Install),
            ("start", Lifecycle::Start),
            ("stop", Lifecycle::Stop),
            ("restart", Lifecycle::Restart),
            ("logs", Lifecycle::Logs),
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
    }
}
