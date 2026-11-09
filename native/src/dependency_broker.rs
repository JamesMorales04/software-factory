//! Atomic dependency acquisition boundary.
//!
//! The broker runs a credential-free, registry-allowlisted fetch and vendor in
//! a staging directory. It verifies the lockfile and vendor tree, then promotes
//! the vendor directory with a rename/swap. An aborted or failed acquisition
//! leaves the previous vendor tree intact and removes its staging directory.
//!
//! This module is deliberately separate from the pure planning contract. Core
//! owns authorization and evidence persistence; the broker owns the process and
//! filesystem boundary.

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

pub const MAX_VENDOR_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_VENDOR_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_VENDOR_ENTRIES: usize = 200_000;

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
    pub lockfile_path: String,
    pub lockfile_sha256: String,
    pub vendor_path: String,
    pub fetch_command: Vec<String>,
    pub vendor_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyAcquisitionEvidence {
    pub lockfile_sha256: String,
    pub vendor_sha256: String,
    pub vendor_entries: usize,
    pub staging_cleaned: bool,
}

pub trait CommandRunner {
    fn run(&self, argv: &[String], cwd: &Path) -> Result<(), String>;
}

pub struct ProcessCommandRunner;

impl CommandRunner for ProcessCommandRunner {
    fn run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "empty dependency command".to_string())?;
        // The broker network namespace and egress policy own registry
        // allowlisting. This process still scrubs the ambient environment so a
        // provider, registry or cloud credential cannot leak into Cargo.
        let output = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env(
                "PATH",
                std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()),
            )
            .env("HOME", std::env::temp_dir())
            .env(
                "CARGO_HOME",
                std::env::temp_dir().join("factory-dependency-cargo-home"),
            )
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .map_err(|error| format!("{program}: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "{program} exited with {}: {}",
                output.status,
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
    collect(root, root, &mut entries, &mut total_bytes)?;
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
) -> Result<(), DependencyBrokerError> {
    let mut children = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        if entries.len() >= MAX_VENDOR_ENTRIES {
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
            collect(root, &path, entries, total_bytes)?;
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

/// Acquire dependencies into staging and atomically promote the vendor tree.
pub fn acquire(
    request: &DependencyAcquisitionRequest,
    runner: &dyn CommandRunner,
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
    let lockfile = std::fs::read(root.join(&request.lockfile_path))?;
    if sha256_hex(&lockfile) != request.lockfile_sha256 {
        return Err(DependencyBrokerError::LockfileMismatch);
    }
    let staging = root.join(format!(".factory-deps-{}", uuid::Uuid::new_v4()));
    let staging_vendor = staging.join("vendor");
    std::fs::create_dir_all(&staging_vendor)?;
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&staging);
    };
    if let Err(error) = runner.run(&request.fetch_command, &root) {
        cleanup();
        return Err(DependencyBrokerError::CommandFailed(error));
    }
    let mut vendor_command = request.vendor_command.clone();
    if let Some(last) = vendor_command.last_mut() {
        *last = staging_vendor.to_string_lossy().to_string();
    }
    if let Err(error) = runner.run(&vendor_command, &root) {
        cleanup();
        return Err(DependencyBrokerError::CommandFailed(error));
    }
    let (vendor_sha256, vendor_entries) = match hash_tree(&staging_vendor) {
        Ok(value) => value,
        Err(error) => {
            cleanup();
            return Err(error);
        }
    };
    let final_vendor = root.join(&request.vendor_path);
    let backup = root.join(format!(".factory-vendor-backup-{}", uuid::Uuid::new_v4()));
    let had_existing = final_vendor.exists();
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
    if had_existing {
        std::fs::remove_dir_all(&backup)?;
    }
    std::fs::remove_dir_all(&staging)?;
    Ok(DependencyAcquisitionEvidence {
        lockfile_sha256: request.lockfile_sha256.clone(),
        vendor_sha256,
        vendor_entries,
        staging_cleaned: !staging.exists(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRunner {
        fail: bool,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, argv: &[String], _cwd: &Path) -> Result<(), String> {
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

    fn request(root: &Path) -> DependencyAcquisitionRequest {
        let lockfile = b"version = 4\n";
        std::fs::write(root.join("Cargo.lock"), lockfile).unwrap();
        DependencyAcquisitionRequest {
            root: root.to_path_buf(),
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

    #[test]
    fn acquisition_promotes_atomically_and_cleans_staging() {
        let root =
            std::env::temp_dir().join(format!("factory-deps-broker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
        let request = request(&root);
        let evidence = acquire(&request, &FakeRunner { fail: false }).unwrap();
        assert_eq!(evidence.vendor_entries, 1);
        assert!(evidence.staging_cleaned);
        assert_eq!(
            std::fs::read_to_string(root.join("vendor/crate.txt")).unwrap(),
            "vendored"
        );
        assert!(!root.join("vendor/old.txt").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_acquisition_preserves_the_previous_vendor_tree() {
        let root =
            std::env::temp_dir().join(format!("factory-deps-broker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("vendor/old.txt"), "old").unwrap();
        let request = request(&root);
        assert!(acquire(&request, &FakeRunner { fail: true }).is_err());
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
        let root =
            std::env::temp_dir().join(format!("factory-deps-broker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut request = request(&root);
        request.lockfile_sha256 = "0".repeat(64);
        assert!(matches!(
            acquire(&request, &FakeRunner { fail: false }),
            Err(DependencyBrokerError::LockfileMismatch)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
