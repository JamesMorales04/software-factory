//! Repository engineering automation. Xtask coordinates existing tools; it is
//! not a product runtime, policy engine or test framework.

use std::env;
use std::process::{Command, ExitCode};

mod check;
mod e2e;
mod governance;
mod process;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }
    let result = match (args[0].as_str(), args.get(1).map(String::as_str)) {
        ("check", None) => check::check(),
        ("test", None) => check::test(),
        ("migrate", None) => check::migrate(),
        ("security", None) => check::security(),
        ("validate-architecture", None) => governance::architecture(),
        ("governance", None) => governance::governance(),
        ("e2e", Some("recovery")) => e2e::recovery(),
        ("e2e", Some("dispatch")) => e2e::dispatch(),
        ("e2e", Some(_)) => e2e::unsupported(args.get(1).map(String::as_str).unwrap_or_default()),
        ("e2e", None) => e2e::list(),
        _ => {
            eprintln!("unknown xtask command: {}", args.join(" "));
            print_help();
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "Software Factory engineering automation

Usage: cargo xtask <command>

Commands:
  check                   fmt --check, strict Clippy, tests, build, docs, audit
  test                    cargo test --workspace --locked
  migrate                 verify migrations against a disposable SQLite database
  security                sandbox isolation tests and repository hygiene audit
  validate-architecture   native architecture and capability consistency
  governance              architecture consistency plus legacy inventory
  e2e recovery            process-level Core restart and durable-state recovery
  e2e dispatch            fixture Campaign, Worker, candidate, receipts and gate
  e2e                     list available deterministic E2E suites

Xtask coordinates existing tools. It never starts a paid provider session and
is not part of the installed Factory runtime."
    );
}

pub(crate) fn cargo() -> Command {
    Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}
