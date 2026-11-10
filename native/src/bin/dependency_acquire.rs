//! Credential-free dependency acquisition entrypoint.
//!
//! This binary runs inside the dedicated dependency image with only the
//! workspace volume mounted. It validates the allowlisted registry and exact
//! lockfile digest, runs `cargo fetch --locked`/`cargo vendor --locked` in a
//! private staging directory, writes deterministic SBOM/license/advisory
//! evidence, and promotes the vendor tree atomically. It never executes
//! project build scripts and never receives provider, registry or cloud
//! credentials.

use std::path::PathBuf;

use factory_native::dependencies::{BrokeredFetch, DEFAULT_ALLOWED_REGISTRIES};
use factory_native::dependency_broker::{
    self, ADVISORY_FILE, CargoEvidenceProducer, DependencyAcquisitionEvidence,
    DependencyAcquisitionRequest, DependencyEvidenceFile, EVIDENCE_DIR, LICENSE_FILE,
    ProcessCommandRunner, SBOM_FILE,
};
use factory_native::dependency_service::DependencyAcquireReceipt;

struct Args {
    root: PathBuf,
    workdir: String,
    lockfile: String,
    lockfile_sha256: String,
    vendor: String,
    registry: String,
}

fn main() {
    match run() {
        Ok(receipt) => {
            match serde_json::to_writer(std::io::stdout(), &receipt) {
                Ok(()) => {}
                Err(error) => fail(&format!("cannot encode dependency receipt: {error}")),
            }
            println!();
        }
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> ! {
    let message = factory_native::telemetry::redact(message);
    eprintln!(
        "factory-dependency-acquire: {}",
        message.chars().take(2_000).collect::<String>()
    );
    std::process::exit(1);
}

fn run() -> Result<DependencyAcquireReceipt, String> {
    let args = parse_args(std::env::args().skip(1))?;
    let fetch = BrokeredFetch::new(
        args.registry.clone(),
        args.lockfile.clone(),
        args.lockfile_sha256.clone(),
        args.vendor.clone(),
    );
    fetch
        .validate()
        .map_err(|error| format!("dependency acquisition contract rejected: {error}"))?;
    if !DEFAULT_ALLOWED_REGISTRIES
        .iter()
        .any(|registry| registry.trim().eq_ignore_ascii_case(args.registry.trim()))
    {
        return Err("dependency registry is not in the image allowlist".into());
    }
    let allowed: Vec<String> = DEFAULT_ALLOWED_REGISTRIES
        .iter()
        .map(|registry| (*registry).to_string())
        .collect();
    let lockfile_path = lockfile_path(&args.root, &args.workdir, &args.lockfile)?;
    let lockfile = std::fs::read(&lockfile_path)
        .map_err(|error| format!("dependency lockfile is unavailable: {error}"))?;
    dependency_broker::validate_lockfile_sources(&lockfile, &allowed)?;
    let cargo = std::env::var("FACTORY_DEPENDENCY_CARGO")
        .unwrap_or_else(|_| "/usr/local/cargo/bin/cargo".into());
    let rustup_home = std::env::var("FACTORY_DEPENDENCY_RUSTUP_HOME")
        .unwrap_or_else(|_| "/usr/local/rustup".into());
    let runner = ProcessCommandRunner::new(PathBuf::from(cargo), PathBuf::from(rustup_home))
        .map_err(|error| format!("dependency toolchain is unavailable: {error}"))?;
    let request = DependencyAcquisitionRequest {
        root: args.root,
        workdir: args.workdir,
        lockfile_path: args.lockfile,
        lockfile_sha256: args.lockfile_sha256,
        vendor_path: args.vendor,
        fetch_command: vec!["cargo".into(), "fetch".into(), "--locked".into()],
        vendor_command: fetch.vendor_command,
    };
    let evidence = dependency_broker::acquire(&request, &runner, &CargoEvidenceProducer)
        .map_err(|error| format!("dependency acquisition failed: {error}"))?;
    receipt(&request, &evidence)
}

fn lockfile_path(root: &std::path::Path, workdir: &str, lockfile: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let lockfile = std::path::Path::new(lockfile);
    if lockfile.is_absolute()
        || lockfile
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("--lockfile must be a safe relative path".into());
    }
    if workdir.is_empty() || workdir == "." {
        return Ok(root.join(lockfile));
    }
    let workdir = std::path::Path::new(workdir);
    if workdir.is_absolute()
        || workdir
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("--workdir must be a safe relative path".into());
    }
    Ok(root.join(workdir).join(lockfile))
}

fn receipt(
    request: &DependencyAcquisitionRequest,
    evidence: &DependencyAcquisitionEvidence,
) -> Result<DependencyAcquireReceipt, String> {
    let image = std::env::var("FACTORY_DEPENDENCY_IMAGE").unwrap_or_else(|_| "unknown".into());
    let file = |name: &str| -> Result<(String, String), String> {
        let path = format!("{EVIDENCE_DIR}/{name}");
        let sha256 = evidence_sha(&evidence.evidence, &path)
            .ok_or_else(|| format!("dependency evidence {name} is missing"))?;
        Ok((format!("{}/{path}", request.vendor_path), sha256))
    };
    let (sbom_path, sbom_sha256) = file(SBOM_FILE)?;
    let (license_path, license_sha256) = file(LICENSE_FILE)?;
    let (advisory_path, advisory_sha256) = file(ADVISORY_FILE)?;
    Ok(DependencyAcquireReceipt {
        image,
        lockfile_sha256: evidence.lockfile_sha256.clone(),
        vendor_sha256: evidence.vendor_sha256.clone(),
        vendor_entries: evidence.vendor_entries,
        sbom_path,
        sbom_sha256,
        license_path,
        license_sha256,
        advisory_path,
        advisory_sha256,
        staging_cleaned: evidence.staging_cleaned,
    })
}

fn evidence_sha(evidence: &[DependencyEvidenceFile], path: &str) -> Option<String> {
    evidence
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.sha256.clone())
}

fn parse_args<I>(mut arguments: I) -> Result<Args, String>
where
    I: Iterator<Item = String>,
{
    let mut args = Args {
        root: PathBuf::from("/workspace"),
        workdir: ".".into(),
        lockfile: "Cargo.lock".into(),
        lockfile_sha256: String::new(),
        vendor: "vendor".into(),
        registry: DEFAULT_ALLOWED_REGISTRIES[1].to_string(),
    };
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} requires a value"))
        };
        match argument.as_str() {
            "--root" => args.root = PathBuf::from(value()?),
            "--workdir" => args.workdir = value()?,
            "--lockfile" => args.lockfile = value()?,
            "--lockfile-sha256" => args.lockfile_sha256 = value()?,
            "--vendor" => args.vendor = value()?,
            "--registry" => args.registry = value()?,
            other => return Err(format!("unsupported argument {other:?}")),
        }
    }
    if args.lockfile_sha256.len() != 64
        || !args
            .lockfile_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("--lockfile-sha256 must be a SHA-256 hex digest".into());
    }
    if !args.root.is_absolute() {
        return Err("--root must be an absolute path".into());
    }
    if args.workdir.is_empty() {
        args.workdir = ".".into();
    }
    if args.registry.trim().is_empty() {
        return Err("--registry is required".into());
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Result<Args, String> {
        parse_args(values.iter().map(|value| (*value).to_string()))
    }

    #[test]
    fn arguments_are_strict() {
        assert!(args(&["--lockfile-sha256", &"a".repeat(64)]).is_ok());
        assert!(args(&["--lockfile-sha256", "short"]).is_err());
        assert!(args(&["--unknown", "x"]).is_err());
        assert!(args(&["--lockfile"]).is_err());
        assert!(args(&["--root", "relative", "--lockfile-sha256", &"a".repeat(64)]).is_err());
    }

    #[test]
    fn a_registry_outside_the_allowlist_is_rejected() {
        let parsed = args(&[
            "--lockfile-sha256",
            &"a".repeat(64),
            "--registry",
            "https://example.invalid/index",
        ])
        .unwrap();
        let fetch = BrokeredFetch::new(
            parsed.registry,
            parsed.lockfile,
            parsed.lockfile_sha256,
            parsed.vendor,
        );
        assert!(fetch.validate().is_err());
    }

    #[test]
    fn default_bounds_match_the_service_contract() {
        assert!(
            factory_native::dependency_service::MAX_DEPENDENCY_TIMEOUT.as_secs()
                >= dependency_broker::COMMAND_TIMEOUT.as_secs()
        );
        assert_eq!(SBOM_FILE, "sbom.cdx.json");
        assert_eq!(LICENSE_FILE, "licenses.json");
        assert_eq!(ADVISORY_FILE, "advisories.json");
    }
}
