use crate::process::cargo_run;
use std::process::Command;

const SUITES: &[(&str, &str)] = &[
    (
        "recovery",
        "process-level Core restart and durable Project/Action recovery",
    ),
    (
        "dispatch",
        "fixture Campaign authorization, Worker lease, candidate, receipts and gate",
    ),
    (
        "negatives",
        "deterministic scope, verification, authorization and lease negatives",
    ),
    (
        "provider",
        "deterministic fake-provider Attempt through Core, Worker, Supervisor and gate",
    ),
    (
        "intake",
        "unified request, policy, assessment, freeze and operator approval with a deterministic fake model",
    ),
    (
        "intake-full",
        "unified intake through Worker, candidate, verifier, semantic reviewer, gate and Core restart",
    ),
];

pub(crate) fn list() -> Result<(), String> {
    println!("Deterministic E2E suites (no provider session):");
    for (name, description) in SUITES {
        println!("  {name:12} {description}");
    }
    println!("\nRun with: cargo xtask e2e <suite>");
    Ok(())
}

pub(crate) fn recovery() -> Result<(), String> {
    cargo_run(&[
        "test",
        "--workspace",
        "--locked",
        "--test",
        "e2e_core_recovery",
    ])
}

pub(crate) fn dispatch() -> Result<(), String> {
    cargo_run(&[
        "test",
        "--workspace",
        "--locked",
        "--test",
        "e2e_fixture_dispatch",
    ])
}

fn office_commands() -> [Command; 2] {
    let mut build = crate::cargo();
    build.args(["build", "--bins", "--locked"]);

    let mut browser = Command::new("node");
    browser.args(["office/e2e-browser/rust-journey.mjs"]);
    [build, browser]
}

pub(crate) fn office() -> Result<(), String> {
    let [build, browser] = office_commands();
    crate::process::run(build)?;
    crate::process::run(browser).map_err(|error| {
        format!("{error}\nInstall Office browser dependencies with `npm ci` in office/e2e-browser.")
    })
}

pub(crate) fn intake() -> Result<(), String> {
    cargo_run(&["build", "--bins", "--locked"])?;
    let mut command = std::process::Command::new("node");
    command.args(["office/e2e-browser/intake-journey.mjs"]);
    crate::process::run(command).map_err(|error| {
        format!("{error}\nInstall Office browser dependencies with `npm ci` in office/e2e-browser.")
    })
}

pub(crate) fn intake_full() -> Result<(), String> {
    cargo_run(&["build", "--bins", "--locked"])?;
    let mut command = std::process::Command::new("node");
    command.args(["office/e2e-browser/intake-journey.mjs"]);
    command.env("FACTORY_INTAKE_FULL", "1");
    crate::process::run(command).map_err(|error| {
        format!("{error}\nInstall Office browser dependencies with `npm ci` in office/e2e-browser.")
    })
}

pub(crate) fn provider() -> Result<(), String> {
    cargo_run(&[
        "test",
        "--workspace",
        "--locked",
        "--test",
        "e2e_fixture_dispatch",
        "provider_backed_campaign_reaches_accepted_gate_with_source_and_tools",
    ])
}

pub(crate) fn negatives() -> Result<(), String> {
    cargo_run(&[
        "test",
        "--workspace",
        "--locked",
        "--test",
        "e2e_dispatch_negative",
    ])
}

pub(crate) fn unsupported(name: &str) -> Result<(), String> {
    Err(format!(
        "E2E suite {name:?} is not implemented. Available suites: {}",
        SUITES
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::office_commands;

    #[test]
    fn office_journey_builds_candidate_binaries_before_launching_browser() {
        let [build, browser] = office_commands();

        assert_eq!(
            build.get_args().collect::<Vec<_>>(),
            ["build", "--bins", "--locked"]
        );
        assert_eq!(browser.get_program(), "node");
        assert_eq!(
            browser.get_args().collect::<Vec<_>>(),
            ["office/e2e-browser/rust-journey.mjs"]
        );
    }
}
