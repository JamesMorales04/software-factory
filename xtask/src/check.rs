use std::env;
use std::process::Command;

use crate::cargo;
use crate::process::run;

fn cargo_stream(args: &[&str]) -> Result<(), String> {
    let mut command = cargo();
    command.args(args);
    run(command)
}

pub(crate) fn check() -> Result<(), String> {
    cargo_stream(&["fmt", "--all", "--", "--check"])?;
    cargo_stream(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--locked",
        "--",
        "-D",
        "warnings",
    ])?;
    cargo_stream(&["test", "--workspace", "--locked"])?;
    cargo_stream(&["build", "--workspace", "--locked", "--bins"])?;
    cargo_stream(&["doc", "--workspace", "--no-deps", "--locked"])?;
    let mut audit = cargo();
    audit.args(["audit", "--file", "Cargo.lock"]);
    run(audit).map_err(|error| {
        format!("{error}\nInstall the pinned cargo-audit tool in the development container.")
    })
}

pub(crate) fn test() -> Result<(), String> {
    cargo_stream(&["test", "--workspace", "--locked"])
}

/// Containerized development check. The development container does not receive
/// the host user-namespace/mount capability required by bubblewrap, so the
/// host-boundary provider tests are reserved for the host Rust lane and the
/// real Compose/provider lane. Everything else runs inside the pinned image.
pub(crate) fn check_container() -> Result<(), String> {
    cargo_stream(&["fmt", "--all", "--", "--check"])?;
    cargo_stream(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--locked",
        "--",
        "-D",
        "warnings",
    ])?;
    cargo_stream(&[
        "test",
        "--workspace",
        "--locked",
        "--",
        "--skip",
        "provider_backed_campaign_reaches_accepted_gate_with_source_and_tools",
        "--skip",
        "model_lead_start_dispatches_through_supervisor_to_accepted_gate",
        "--skip",
        "codex_fixture_runs_a_mediated_tool_turn_and_records_core_accounting",
        "--skip",
        "real_bwrap_hides_host_home_repositories_sockets_and_network",
        "--skip",
        "confined_provider_reaches_only_the_scoped_egress_broker",
        "--skip",
        "sandbox_kills_a_provider_that_exceeds_the_session_quota",
        "--skip",
        "actual_opencode_executable_uses_the_egress_broker",
        "--skip",
        "confined_opencode_reaches_the_pinned_egress_broker_without_a_host_credential",
        "--skip",
        "actual_codex_executable_uses_the_egress_broker",
    ])?;
    cargo_stream(&["build", "--workspace", "--locked", "--bins"])?;
    cargo_stream(&["doc", "--workspace", "--no-deps", "--locked"])?;
    let mut audit = cargo();
    audit.args(["audit", "--file", "Cargo.lock"]);
    run(audit).map_err(|error| {
        format!("{error}\nInstall the pinned cargo-audit tool in the development container.")
    })
}

pub(crate) fn migrate() -> Result<(), String> {
    let database = env::temp_dir().join(format!(
        "factory-migration-check-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&database);
    let url = format!("sqlite:{}", database.display());
    eprintln!("verifying migrations against {url}");
    let mut create = Command::new("sqlx");
    create
        .env("DATABASE_URL", &url)
        .arg("database")
        .arg("create");
    run(create).map_err(|error| {
        format!("{error}\nInstall pinned sqlx-cli 0.8.6 in the development container.")
    })?;
    let mut migrate = Command::new("sqlx");
    migrate
        .env("DATABASE_URL", &url)
        .arg("migrate")
        .arg("run")
        .arg("--source")
        .arg("migrations");
    run(migrate)?;
    let _ = std::fs::remove_file(database);
    Ok(())
}

pub(crate) fn office() -> Result<(), String> {
    for script in ["typecheck", "lint", "build"] {
        let mut command = Command::new("npm");
        command.args(["run", script]).current_dir("office");
        run(command).map_err(|error| {
            format!("{error}\nInstall Office dependencies with `npm ci` in office/.")
        })?;
    }
    Ok(())
}

pub(crate) fn security() -> Result<(), String> {
    cargo_stream(&["test", "--workspace", "--locked", "sandbox::"])?;
    let mut hygiene = cargo();
    hygiene.args([
        "run",
        "--locked",
        "--bin",
        "factory-repo-hygiene",
        "--",
        "--mode",
        "audit",
    ]);
    run(hygiene)
}
