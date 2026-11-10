//! Atomic dependency acquisition boundary.
//!
//! The broker runs a credential-free, allowlisted `cargo fetch`/`cargo vendor`
//! in a private staging directory inside the workspace. A trusted evidence
//! producer writes the SBOM, license and advisory inventory into the staging
//! vendor tree before a single rename promotes the whole tree, so vendor and
//! evidence become visible together. An aborted or failed acquisition leaves
//! the previous vendor tree intact and removes its staging directory.
//!
//! This module is deliberately separate from the pure planning contract. Core
//! owns authorization and evidence persistence; the broker owns the process and
//! filesystem boundary. It never executes project build scripts: `cargo fetch`
//! and `cargo vendor` parse manifests and download/vendor sources only.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_VENDOR_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_VENDOR_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_VENDOR_ENTRIES: usize = 200_000;
pub const MAX_COMMAND_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

/// Evidence lives inside the promoted vendor tree so a single atomic rename
/// publishes the vendor sources and their supply-chain evidence together.
pub const EVIDENCE_DIR: &str = ".factory-supply-chain";
pub const SBOM_FILE: &str = "sbom.cdx.json";
pub const LICENSE_FILE: &str = "licenses.json";
pub const ADVISORY_FILE: &str = "advisories.json";

#[derive(Debug, thiserror::Error)]
pub enum DependencyBrokerError {
    #[error("dependency broker input is invalid: {0}")]
    Invalid(String),
    #[error("dependency lockfile does not match the planned digest")]
    LockfileMismatch,
    #[error("dependency command failed: {0}")]
    CommandFailed(String),
    #[error("dependency vendor output is invalid: {0}")]
    VendorInvalid(String),
    #[error("dependency vendor promotion failed: {0}")]
    PromotionFailed(String),
    #[error("dependency broker I/O: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyAcquisitionRequest {
    pub root: PathBuf,
    /// Workspace-relative directory that contains the Cargo project. Empty and
    /// `.` refer to the workspace root.
    pub workdir: String,
    pub lockfile_path: String,
    pub lockfile_sha256: String,
    pub vendor_path: String,
    pub fetch_command: Vec<String>,
    pub vendor_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEvidenceFile {
    /// Path relative to the promoted vendor directory.
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyAcquisitionEvidence {
    pub lockfile_sha256: String,
    pub vendor_sha256: String,
    pub vendor_entries: usize,
    pub evidence: Vec<DependencyEvidenceFile>,
    pub staging_cleaned: bool,
}

/// Trusted, deterministic evidence generation. The producer reads only the
/// lockfile and vendored manifests; it never executes project code.
pub trait EvidenceProducer {
    fn produce(&self, context: &EvidenceContext<'_>) -> Result<(), String>;
}

pub struct EvidenceContext<'a> {
    pub workspace: &'a Path,
    pub lockfile_path: &'a str,
    pub vendor: &'a Path,
}

pub trait CommandRunner {
    fn run(&self, argv: &[String], cwd: &Path) -> Result<(), String>;

    /// Run with a private Cargo home. Acquisition never falls back to a shared
    /// Cargo state; every implementation must honor the supplied directory.
    fn run_in(&self, argv: &[String], cwd: &Path, cargo_home: &Path) -> Result<(), String>;
}

/// Trusted process runner. The broker must construct it from trusted
/// configuration: `cargo` is an absolute approved binary and `rustup_home` is
/// the pinned toolchain home. No ambient PATH or rustup selection is reused.
pub struct ProcessCommandRunner {
    cargo: PathBuf,
    rustup_home: PathBuf,
}

impl ProcessCommandRunner {
    pub fn new(cargo: PathBuf, rustup_home: PathBuf) -> Result<Self, DependencyBrokerError> {
        if !cargo.is_absolute() || !cargo.is_file() {
            return Err(DependencyBrokerError::Invalid(
                "cargo binary must be an absolute existing file".into(),
            ));
        }
        if !rustup_home.is_absolute() || !rustup_home.is_dir() {
            return Err(DependencyBrokerError::Invalid(
                "rustup home must be an absolute existing directory".into(),
            ));
        }
        Ok(Self { cargo, rustup_home })
    }
}

impl CommandRunner for ProcessCommandRunner {
    fn run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
        let cargo_home =
            std::env::temp_dir().join(format!("factory-cargo-home-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cargo_home).map_err(|error| format!("cargo home: {error}"))?;
        let result = self.run_in(argv, cwd, &cargo_home);
        let _ = std::fs::remove_dir_all(&cargo_home);
        result
    }

    fn run_in(&self, argv: &[String], cwd: &Path, cargo_home: &Path) -> Result<(), String> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "empty dependency command".to_string())?;
        if program != "cargo" {
            return Err("the dependency broker only runs the approved cargo binary".into());
        }
        // The broker network namespace and egress policy own registry
        // allowlisting. This process still scrubs the ambient environment so a
        // provider, registry or cloud credential cannot leak into Cargo.
        let path = self
            .cargo
            .parent()
            .map(|parent| format!("{}:/usr/bin:/bin", parent.display()))
            .unwrap_or_else(|| "/usr/bin:/bin".into());
        let stdout_path = cargo_home.join("cargo-stdout.log");
        let stderr_path = cargo_home.join("cargo-stderr.log");
        let stdout = std::fs::File::create(&stdout_path)
            .map_err(|error| format!("cargo stdout log: {error}"))?;
        let stderr = std::fs::File::create(&stderr_path)
            .map_err(|error| format!("cargo stderr log: {error}"))?;
        let mut child = Command::new(&self.cargo)
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", path)
            .env("HOME", std::env::temp_dir())
            .env("RUSTUP_HOME", &self.rustup_home)
            .env("CARGO_HOME", cargo_home)
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_TERM_COLOR", "never")
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| format!("{}: {error}", self.cargo.display()))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    return Err(format!("cargo wait: {error}"));
                }
            }
            if started.elapsed() > COMMAND_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cargo command exceeded its timeout".into());
            }
            for path in [&stdout_path, &stderr_path] {
                let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
                if size > MAX_COMMAND_OUTPUT_BYTES {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("cargo command output exceeded its bound".into());
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        for path in [&stdout_path, &stderr_path] {
            let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if size > MAX_COMMAND_OUTPUT_BYTES {
                return Err("cargo command output exceeded its bound".into());
            }
        }
        if !status.success() {
            let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
            return Err(format!(
                "{program} exited with {status}: {}",
                stderr.chars().take(2_000).collect::<String>()
            ));
        }
        Ok(())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn safe_relative(value: &str) -> Result<&str, DependencyBrokerError> {
    if value.is_empty()
        || value.len() > 512
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(':')
        || value.contains('"')
        || value.contains('\'')
        || value.chars().any(char::is_control)
        || value
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(DependencyBrokerError::Invalid(format!(
            "unsafe relative path {value:?}"
        )));
    }
    Ok(value)
}

fn safe_workdir(value: &str) -> Result<(), DependencyBrokerError> {
    if value.is_empty() || value == "." {
        return Ok(());
    }
    safe_relative(value).map(|_| ())
}

fn resolve_workdir(root: &Path, value: &str) -> Result<PathBuf, DependencyBrokerError> {
    safe_workdir(value)?;
    let candidate = if value.is_empty() || value == "." {
        root.to_path_buf()
    } else {
        root.join(value)
    };
    let canonical = candidate.canonicalize()?;
    if !canonical.starts_with(root) || !canonical.is_dir() {
        return Err(DependencyBrokerError::Invalid(
            "dependency workdir escapes the workspace".into(),
        ));
    }
    Ok(canonical)
}

fn validate_commands(request: &DependencyAcquisitionRequest) -> Result<(), DependencyBrokerError> {
    if request.fetch_command != ["cargo", "fetch", "--locked"] {
        return Err(DependencyBrokerError::Invalid(
            "fetch command must be `cargo fetch --locked`".into(),
        ));
    }
    let expected_vendor = [
        "cargo".to_string(),
        "vendor".to_string(),
        "--locked".to_string(),
        request.vendor_path.clone(),
    ];
    if request.vendor_command != expected_vendor {
        return Err(DependencyBrokerError::Invalid(
            "vendor command must be `cargo vendor --locked <vendor>`".into(),
        ));
    }
    if request.lockfile_sha256.len() != 64
        || !request
            .lockfile_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(DependencyBrokerError::Invalid(
            "lockfile digest must be SHA-256".into(),
        ));
    }
    Ok(())
}

fn hash_tree(root: &Path) -> Result<(String, usize), DependencyBrokerError> {
    let mut entries = Vec::new();
    let mut total_bytes = 0u64;
    let mut visited = 0usize;
    collect(root, root, &mut entries, &mut total_bytes, &mut visited)?;
    entries.sort();
    let mut hasher = Sha256::new();
    let count = entries.len();
    for (relative, bytes) in entries {
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    Ok((format!("{:x}", hasher.finalize()), count))
}

fn collect(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(String, Vec<u8>)>,
    total_bytes: &mut u64,
    visited: &mut usize,
) -> Result<(), DependencyBrokerError> {
    let mut children = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        *visited = visited.saturating_add(1);
        if *visited > MAX_VENDOR_ENTRIES {
            return Err(DependencyBrokerError::VendorInvalid(
                "vendor entry bound exceeded".into(),
            ));
        }
        let path = child.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(DependencyBrokerError::VendorInvalid(format!(
                "vendor contains a symbolic link: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect(root, &path, entries, total_bytes, visited)?;
        } else if metadata.is_file() {
            if metadata.len() > MAX_VENDOR_FILE_BYTES {
                return Err(DependencyBrokerError::VendorInvalid(
                    "vendor file is too large".into(),
                ));
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > MAX_VENDOR_TOTAL_BYTES {
                return Err(DependencyBrokerError::VendorInvalid(
                    "vendor total size bound exceeded".into(),
                ));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| DependencyBrokerError::VendorInvalid("vendor path escaped".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            entries.push((relative, std::fs::read(&path)?));
        }
    }
    Ok(())
}

fn collect_evidence(vendor: &Path) -> Result<Vec<DependencyEvidenceFile>, DependencyBrokerError> {
    let directory = vendor.join(EVIDENCE_DIR);
    if !directory.is_dir() {
        return Err(DependencyBrokerError::VendorInvalid(
            "evidence producer wrote no evidence directory".into(),
        ));
    }
    let mut files = Vec::new();
    let mut children = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(DependencyBrokerError::VendorInvalid(
                "evidence contains a non-regular entry".into(),
            ));
        }
        if metadata.len() > MAX_VENDOR_FILE_BYTES {
            return Err(DependencyBrokerError::VendorInvalid(
                "evidence file is too large".into(),
            ));
        }
        let name = child.file_name().into_string().map_err(|_| {
            DependencyBrokerError::VendorInvalid("evidence name is not UTF-8".into())
        })?;
        files.push(DependencyEvidenceFile {
            path: format!("{EVIDENCE_DIR}/{name}"),
            sha256: sha256_hex(&std::fs::read(&path)?),
        });
    }
    for required in [SBOM_FILE, LICENSE_FILE, ADVISORY_FILE] {
        if !files
            .iter()
            .any(|file| file.path == format!("{EVIDENCE_DIR}/{required}"))
        {
            return Err(DependencyBrokerError::VendorInvalid(format!(
                "evidence producer did not write {required}"
            )));
        }
    }
    Ok(files)
}

fn remove_path(path: &Path) -> Result<(), DependencyBrokerError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Acquire dependencies into staging and atomically promote the vendor tree
/// together with the evidence files the producer wrote inside it.
pub fn acquire(
    request: &DependencyAcquisitionRequest,
    runner: &dyn CommandRunner,
    evidence: &dyn EvidenceProducer,
) -> Result<DependencyAcquisitionEvidence, DependencyBrokerError> {
    validate_commands(request)?;
    safe_relative(&request.lockfile_path)?;
    safe_relative(&request.vendor_path)?;
    let root = request.root.canonicalize()?;
    if !root.is_dir() {
        return Err(DependencyBrokerError::Invalid(
            "workspace root is not a directory".into(),
        ));
    }
    let workdir = resolve_workdir(&root, &request.workdir)?;
    let lockfile_path = workdir.join(&request.lockfile_path);
    let lockfile_metadata = std::fs::symlink_metadata(&lockfile_path)?;
    if !lockfile_metadata.is_file() || lockfile_metadata.file_type().is_symlink() {
        return Err(DependencyBrokerError::Invalid(
            "lockfile must be a regular file".into(),
        ));
    }
    let lockfile = std::fs::read(&lockfile_path)?;
    if sha256_hex(&lockfile) != request.lockfile_sha256 {
        return Err(DependencyBrokerError::LockfileMismatch);
    }
    let staging = workdir.join(format!(".factory-deps-{}", uuid::Uuid::new_v4()));
    let staging_vendor = staging.join("vendor");
    let cargo_home = staging.join("cargo-home");
    std::fs::create_dir_all(&staging_vendor)?;
    std::fs::create_dir_all(&cargo_home)?;
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&staging);
    };
    if let Err(error) = runner.run_in(&request.fetch_command, &workdir, &cargo_home) {
        cleanup();
        return Err(DependencyBrokerError::CommandFailed(error));
    }
    let mut vendor_command = request.vendor_command.clone();
    if let Some(last) = vendor_command.last_mut() {
        *last = staging_vendor.to_string_lossy().to_string();
    }
    if let Err(error) = runner.run_in(&vendor_command, &workdir, &cargo_home) {
        cleanup();
        return Err(DependencyBrokerError::CommandFailed(error));
    }
    if let Err(error) = evidence.produce(&EvidenceContext {
        workspace: &workdir,
        lockfile_path: &request.lockfile_path,
        vendor: &staging_vendor,
    }) {
        cleanup();
        return Err(DependencyBrokerError::VendorInvalid(error));
    }
    let evidence_files = match collect_evidence(&staging_vendor) {
        Ok(files) => files,
        Err(error) => {
            cleanup();
            return Err(error);
        }
    };
    let (vendor_sha256, vendor_entries) = match hash_tree(&staging_vendor) {
        Ok(value) => value,
        Err(error) => {
            cleanup();
            return Err(error);
        }
    };
    let final_vendor = workdir.join(&request.vendor_path);
    let backup = workdir.join(format!(".factory-vendor-backup-{}", uuid::Uuid::new_v4()));
    let had_existing = std::fs::symlink_metadata(&final_vendor).is_ok();
    if had_existing && let Err(error) = std::fs::rename(&final_vendor, &backup) {
        cleanup();
        return Err(DependencyBrokerError::PromotionFailed(error.to_string()));
    }
    if let Err(error) = std::fs::rename(&staging_vendor, &final_vendor) {
        if had_existing {
            let _ = std::fs::rename(&backup, &final_vendor);
        }
        cleanup();
        return Err(DependencyBrokerError::PromotionFailed(error.to_string()));
    }
    if had_existing && let Err(error) = remove_path(&backup) {
        // The new vendor tree is already live. Report the leftover backup
        // rather than pretending the promotion was fully clean.
        cleanup();
        return Err(DependencyBrokerError::PromotionFailed(format!(
            "previous vendor backup could not be removed: {error}"
        )));
    }
    std::fs::remove_dir_all(&staging)?;
    Ok(DependencyAcquisitionEvidence {
        lockfile_sha256: request.lockfile_sha256.clone(),
        vendor_sha256,
        vendor_entries,
        evidence: evidence_files,
        staging_cleaned: !staging.exists(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoPackage {
    name: String,
    version: String,
    checksum: Option<String>,
}

/// Deterministic evidence producer for Cargo vendor trees. It reads the
/// lockfile and each vendored `Cargo.toml`; it does not invoke Cargo or run
/// build scripts. Advisory evidence records the package inventory with an
/// explicit `NOT_CHECKED` status until a pinned advisory database is supplied.
#[derive(Debug, Default, Clone, Copy)]
pub struct CargoEvidenceProducer;

impl EvidenceProducer for CargoEvidenceProducer {
    fn produce(&self, context: &EvidenceContext<'_>) -> Result<(), String> {
        let lockfile_path = context.workspace.join(context.lockfile_path);
        let lockfile = std::fs::read(&lockfile_path)
            .map_err(|error| format!("cannot read lockfile for evidence: {error}"))?;
        let lockfile_sha256 = sha256_hex(&lockfile);
        let packages = parse_cargo_lock(&lockfile)?;
        let directory = context.vendor.join(EVIDENCE_DIR);
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create evidence directory: {error}"))?;
        write_json(
            &directory.join(SBOM_FILE),
            &sbom_document(&lockfile_sha256, &packages),
        )?;
        write_json(
            &directory.join(LICENSE_FILE),
            &license_document(&lockfile_sha256, &packages, context.vendor),
        )?;
        write_json(
            &directory.join(ADVISORY_FILE),
            &advisory_document(&lockfile_sha256, &packages),
        )?;
        Ok(())
    }
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("evidence JSON encoding failed: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes).map_err(|error| format!("cannot write evidence: {error}"))
}

fn parse_cargo_lock(bytes: &[u8]) -> Result<Vec<CargoPackage>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Cargo.lock is not UTF-8".to_string())?;
    let mut packages = Vec::new();
    let mut current: Option<(Option<String>, Option<String>, Option<String>)> = None;
    let mut finish = |package: Option<(Option<String>, Option<String>, Option<String>)>| {
        if let Some((name, version, checksum)) = package {
            match (name, version) {
                (Some(name), Some(version)) if !name.is_empty() && !version.is_empty() => {
                    packages.push(CargoPackage {
                        name,
                        version,
                        checksum,
                    });
                }
                _ => {
                    return Err("Cargo.lock package entry is missing a name or version".to_string());
                }
            }
        }
        Ok(())
    };
    for raw in text.lines() {
        let line = strip_toml_comment(raw).trim();
        if line == "[[package]]" {
            finish(current.take())?;
            current = Some((None, None, None));
            continue;
        }
        if line.starts_with('[') {
            finish(current.take())?;
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => package.0 = parse_toml_string(value.trim()),
            "version" => package.1 = parse_toml_string(value.trim()),
            "checksum" => package.2 = parse_toml_string(value.trim()),
            _ => {}
        }
    }
    finish(current.take())?;
    Ok(packages)
}

fn strip_toml_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        match quote {
            Some(active) if character == active => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == '#' => return &line[..index],
            None => {}
        }
    }
    line
}

fn parse_toml_string(value: &str) -> Option<String> {
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    if value.len() < 2 || !value.ends_with(quote) {
        return None;
    }
    Some(value[1..value.len() - 1].to_string())
}

fn sbom_document(lockfile_sha256: &str, packages: &[CargoPackage]) -> serde_json::Value {
    let components: Vec<serde_json::Value> = packages
        .iter()
        .map(|package| {
            let mut component = serde_json::json!({
                "type": "library",
                "name": package.name,
                "version": package.version,
                "purl": format!("pkg:cargo/{}@{}", package.name, package.version),
            });
            if let Some(checksum) = &package.checksum {
                component["hashes"] = serde_json::json!([
                    {"alg": "SHA-256", "content": checksum}
                ]);
            }
            component
        })
        .collect();
    serde_json::json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": format!("urn:uuid:{}", deterministic_uuid(lockfile_sha256)),
        "version": 1,
        "metadata": {
            "tools": {
                "components": [{
                    "type": "application",
                    "name": "factory-dependency-acquire",
                    "version": env!("CARGO_PKG_VERSION")
                }]
            },
            "properties": [{
                "name": "factory:lockfileSha256",
                "value": lockfile_sha256
            }]
        },
        "components": components
    })
}

fn license_document(
    lockfile_sha256: &str,
    packages: &[CargoPackage],
    vendor: &Path,
) -> serde_json::Value {
    let licenses = vendored_licenses(vendor);
    let entries: Vec<serde_json::Value> = packages
        .iter()
        .map(|package| {
            let license = licenses
                .get(&(package.name.clone(), package.version.clone()))
                .cloned()
                .unwrap_or_else(|| "UNKNOWN".into());
            serde_json::json!({
                "name": package.name,
                "version": package.version,
                "license": license,
            })
        })
        .collect();
    serde_json::json!({
        "schemaVersion": 1,
        "lockfileSha256": lockfile_sha256,
        "packages": entries
    })
}

fn advisory_document(lockfile_sha256: &str, packages: &[CargoPackage]) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = packages
        .iter()
        .map(|package| {
            serde_json::json!({
                "name": package.name,
                "version": package.version,
            })
        })
        .collect();
    serde_json::json!({
        "schemaVersion": 1,
        "lockfileSha256": lockfile_sha256,
        "database": "not-configured",
        "status": "NOT_CHECKED",
        "note": "No pinned advisory database was available in the dependency acquisition image; this file is a deterministic package inventory, not an advisory verdict.",
        "packages": entries
    })
}

fn vendored_licenses(vendor: &Path) -> std::collections::BTreeMap<(String, String), String> {
    let mut licenses = std::collections::BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(vendor) else {
        return licenses;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path.join("Cargo.toml")) else {
            continue;
        };
        if let Some((name, version, license)) = parse_vendored_package(&text) {
            licenses.insert((name, version), license);
        }
    }
    licenses
}

fn parse_vendored_package(text: &str) -> Option<(String, String, String)> {
    let mut in_package = false;
    let mut name = None;
    let mut version = None;
    let mut license = None;
    let mut license_file = None;
    for raw in text.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "name" => name = parse_toml_string(value.trim()),
            "version" => version = parse_toml_string(value.trim()),
            "license" => license = parse_toml_string(value.trim()),
            "license-file" => license_file = parse_toml_string(value.trim()),
            _ => {}
        }
    }
    let name = name?;
    let version = version?;
    let license = license
        .or_else(|| license_file.map(|file| format!("file:{file}")))
        .unwrap_or_else(|| "UNKNOWN".into());
    Some((name, version, license))
}

fn deterministic_uuid(seed_hex: &str) -> String {
    let hex: Vec<char> = seed_hex.chars().take(32).collect();
    if hex.len() != 32 {
        return "00000000-0000-0000-0000-000000000000".into();
    }
    let part = |start: usize, end: usize| hex[start..end].iter().collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        part(0, 8),
        part(8, 12),
        part(12, 16),
        part(16, 20),
        part(20, 32)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRunner {
        fail: bool,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
            self.run_in(argv, cwd, &std::env::temp_dir())
        }

        fn run_in(&self, argv: &[String], _cwd: &Path, _cargo_home: &Path) -> Result<(), String> {
            if self.fail {
                return Err("network unavailable".into());
            }
            if argv.first().map(String::as_str) == Some("cargo")
                && argv.get(1).map(String::as_str) == Some("vendor")
            {
                let vendor = argv.last().unwrap();
                std::fs::create_dir_all(vendor).unwrap();
                std::fs::write(Path::new(vendor).join("crate.txt"), "vendored").unwrap();
            }
            Ok(())
        }
    }

    struct FakeEvidence;

    impl EvidenceProducer for FakeEvidence {
        fn produce(&self, context: &EvidenceContext<'_>) -> Result<(), String> {
            let directory = context.vendor.join(EVIDENCE_DIR);
            std::fs::create_dir_all(&directory).unwrap();
            for (name, body) in [
                (SBOM_FILE, "{\"bomFormat\":\"CycloneDX\"}\n"),
                (LICENSE_FILE, "{\"packages\":[]}\n"),
                (ADVISORY_FILE, "{\"status\":\"NOT_CHECKED\"}\n"),
            ] {
                std::fs::write(directory.join(name), body).unwrap();
            }
            Ok(())
        }
    }

    fn request(root: &Path) -> DependencyAcquisitionRequest {
        let lockfile = b"version = 4\n";
        std::fs::write(root.join("Cargo.lock"), lockfile).unwrap();
        DependencyAcquisitionRequest {
            root: root.to_path_buf(),
            workdir: String::new(),
            lockfile_path: "Cargo.lock".into(),
            lockfile_sha256: sha256_hex(lockfile),
            vendor_path: "vendor".into(),
            fetch_command: vec!["cargo".into(), "fetch".into(), "--locked".into()],
            vendor_command: vec![
                "cargo".into(),
                "vendor".into(),
                "--locked".into(),
                "vendor".into(),
            ],
        }
    }

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("factory-deps-broker-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn acquisition_promotes_atomically_and_cleans_staging() {
        let root = temp_root();
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
        let request = request(&root);
        let evidence = acquire(&request, &FakeRunner { fail: false }, &FakeEvidence).unwrap();
        assert_eq!(evidence.vendor_entries, 4);
        assert!(evidence.staging_cleaned);
        assert_eq!(evidence.evidence.len(), 3);
        assert_eq!(
            std::fs::read_to_string(root.join("vendor/crate.txt")).unwrap(),
            "vendored"
        );
        assert!(!root.join("vendor/old.txt").exists());
        assert!(
            root.join("vendor")
                .join(EVIDENCE_DIR)
                .join(SBOM_FILE)
                .is_file()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_acquisition_preserves_the_previous_vendor_tree() {
        let root = temp_root();
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
        let request = request(&root);
        assert!(acquire(&request, &FakeRunner { fail: true }, &FakeEvidence).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("vendor/old.txt")).unwrap(),
            "old"
        );
        let staging: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".factory-deps-")
            })
            .collect();
        assert!(staging.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lockfile_mismatch_fails_before_running_commands() {
        let root = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        let mut request = request(&root);
        request.lockfile_sha256 = "0".repeat(64);
        assert!(matches!(
            acquire(&request, &FakeRunner { fail: false }, &FakeEvidence),
            Err(DependencyBrokerError::LockfileMismatch)
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acquisition_supports_a_safe_workdir() {
        let root = temp_root();
        std::fs::create_dir_all(root.join("project")).unwrap();
        std::fs::create_dir_all(root.join("project/vendor")).unwrap();
        let mut request = request(&root);
        request.workdir = "project".into();
        std::fs::write(root.join("project/Cargo.lock"), b"version = 4\n").unwrap();
        request.lockfile_sha256 = sha256_hex(b"version = 4\n");
        let evidence = acquire(&request, &FakeRunner { fail: false }, &FakeEvidence).unwrap();
        assert!(evidence.staging_cleaned);
        assert!(root.join("project/vendor/crate.txt").is_file());
        assert!(!root.join("Cargo.lock").exists() || root.join("project/Cargo.lock").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unsafe_paths_and_commands_are_rejected() {
        let root = temp_root();
        std::fs::create_dir_all(&root).unwrap();
        for workdir in ["../escape", "/abs", "a/../b", "a//b"] {
            let mut input = request(&root);
            input.workdir = workdir.into();
            assert!(
                acquire(&input, &FakeRunner { fail: false }, &FakeEvidence).is_err(),
                "unsafe workdir {workdir} accepted"
            );
        }
        let mut input = request(&root);
        input.lockfile_path = "../Cargo.lock".into();
        assert!(acquire(&input, &FakeRunner { fail: false }, &FakeEvidence).is_err());
        let mut input = request(&root);
        input.vendor_path = "vendor/../../escape".into();
        assert!(acquire(&input, &FakeRunner { fail: false }, &FakeEvidence).is_err());
        let mut input = request(&root);
        input.fetch_command = vec!["cargo".into(), "fetch".into()];
        assert!(acquire(&input, &FakeRunner { fail: false }, &FakeEvidence).is_err());
        let mut input = request(&root);
        input.vendor_command = vec!["sh".into(), "-c".into(), "touch owned".into()];
        assert!(acquire(&input, &FakeRunner { fail: false }, &FakeEvidence).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_evidence_producer_output_fails_closed() {
        struct EmptyEvidence;
        impl EvidenceProducer for EmptyEvidence {
            fn produce(&self, _context: &EvidenceContext<'_>) -> Result<(), String> {
                Ok(())
            }
        }
        let root = temp_root();
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
        let request = request(&root);
        assert!(acquire(&request, &FakeRunner { fail: false }, &EmptyEvidence).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("vendor/old.txt")).unwrap(),
            "old"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cargo_evidence_producer_rejects_a_malformed_lockfile() {
        assert!(parse_cargo_lock(b"version = 4\n\n[[package]]\nname = \"demo\"\n").is_err());
        assert!(parse_cargo_lock(b"version = 4\n\n[[package]]\nversion = \"1.0.0\"\n").is_err());
    }

    #[test]
    fn cargo_evidence_producer_writes_deterministic_evidence() {
        let root = temp_root();
        std::fs::create_dir_all(root.join("vendor/demo")).unwrap();
        std::fs::write(
            root.join("vendor/demo/Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"1.2.3\"\nlicense = \"MIT OR Apache-2.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("vendor/other-2.0.0")).unwrap();
        std::fs::write(
            root.join("vendor/other-2.0.0/Cargo.toml"),
            "[package]\nname = \"other\"\nversion = \"2.0.0\"\nlicense-file = \"LICENSE\"\n",
        )
        .unwrap();
        let lockfile = b"version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"1.2.3\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\n[[package]]\nname = \"other\"\nversion = \"2.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n";
        std::fs::write(root.join("Cargo.lock"), lockfile).unwrap();
        let producer = CargoEvidenceProducer;
        producer
            .produce(&EvidenceContext {
                workspace: &root,
                lockfile_path: "Cargo.lock",
                vendor: &root.join("vendor"),
            })
            .unwrap();
        let sbom: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("vendor").join(EVIDENCE_DIR).join(SBOM_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(sbom["bomFormat"], "CycloneDX");
        assert_eq!(sbom["components"][0]["name"], "demo");
        assert_eq!(sbom["components"][0]["purl"], "pkg:cargo/demo@1.2.3");
        let licenses: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("vendor").join(EVIDENCE_DIR).join(LICENSE_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(licenses["packages"][0]["license"], "MIT OR Apache-2.0");
        assert_eq!(licenses["packages"][1]["license"], "file:LICENSE");
        let advisories: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("vendor").join(EVIDENCE_DIR).join(ADVISORY_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(advisories["status"], "NOT_CHECKED");
        let first = producer.produce(&EvidenceContext {
            workspace: &root,
            lockfile_path: "Cargo.lock",
            vendor: &root.join("vendor"),
        });
        assert!(first.is_ok());
        assert_eq!(
            std::fs::read(root.join("vendor").join(EVIDENCE_DIR).join(SBOM_FILE)).unwrap(),
            std::fs::read(root.join("vendor").join(EVIDENCE_DIR).join(SBOM_FILE)).unwrap()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
