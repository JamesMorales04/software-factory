use crate::process::cargo_run;

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
