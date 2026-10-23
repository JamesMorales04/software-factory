use crate::process::cargo_run;

pub(crate) fn architecture() -> Result<(), String> {
    cargo_run(&[
        "run",
        "--locked",
        "--bin",
        "factory-architecture-check",
        "--",
        "--root",
        ".",
    ])
}

pub(crate) fn governance() -> Result<(), String> {
    architecture()?;
    cargo_run(&[
        "run",
        "--locked",
        "--bin",
        "factory-repo-hygiene",
        "--",
        "--mode",
        "audit",
    ])
}
