//! Controlled Git import into Factory-managed project storage.
//!
//! Container deployments never accept an arbitrary host path. A Project is
//! imported from an HTTP(S) Git URL into the Factory-owned projects volume and
//! then registered by its managed path. Hooks, submodules, LFS smudging and
//! terminal prompts are disabled.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::{process::Command, time};

const MAX_URL_BYTES: usize = 2048;
const CLONE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("repository URL is invalid: {0}")]
    Invalid(String),
    #[error("repository import failed: {0}")]
    Git(String),
    #[error("repository import I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub fn is_remote_repository(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

pub fn validate_url(value: &str, allow_insecure_http: bool) -> Result<(), ImportError> {
    if value.len() > MAX_URL_BYTES
        || value.trim() != value
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || value.contains('@')
        || value.starts_with('-')
    {
        return Err(ImportError::Invalid(
            "repository URL has an unsafe shape".into(),
        ));
    }
    let url = reqwest::Url::parse(value)
        .map_err(|_| ImportError::Invalid("repository URL is not absolute".into()))?;
    if url.scheme() != "https" && !(allow_insecure_http && url.scheme() == "http") {
        return Err(ImportError::Invalid("repository URL must use HTTPS".into()));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(ImportError::Invalid(
            "repository URL must not contain credentials".into(),
        ));
    }
    if url.host_str().is_none() || url.query().is_some() || url.fragment().is_some() {
        return Err(ImportError::Invalid(
            "repository URL must be a clean HTTP(S) origin path".into(),
        ));
    }
    Ok(())
}

pub async fn import(
    url: &str,
    root: &Path,
    allow_insecure_http: bool,
) -> Result<PathBuf, ImportError> {
    validate_url(url, allow_insecure_http)?;
    if !root.is_absolute() {
        return Err(ImportError::Invalid(
            "managed project root must be absolute".into(),
        ));
    }
    std::fs::create_dir_all(root)?;
    let digest = Sha256::digest(url.as_bytes());
    let slug: String = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let destination = root.join(format!("project-{slug}"));
    match std::fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(ImportError::Invalid(
                "managed project destination is a symlink".into(),
            ));
        }
        Ok(metadata) if metadata.is_dir() => {
            if destination.join(".git").is_dir() {
                return Ok(destination);
            }
            return Err(ImportError::Invalid(
                "managed project destination is not a Git repository".into(),
            ));
        }
        Ok(_) => {
            return Err(ImportError::Invalid(
                "managed project destination is not a directory".into(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ImportError::Io(error)),
    }
    let temporary = root.join(format!(".import-{}", uuid::Uuid::new_v4()));
    let result = clone(url, &temporary).await;
    match result {
        Ok(()) => {
            if let Err(error) = std::fs::rename(&temporary, &destination) {
                let _ = std::fs::remove_dir_all(&temporary);
                return Err(ImportError::Io(error));
            }
            Ok(destination)
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&temporary);
            Err(error)
        }
    }
}

async fn clone(url: &str, destination: &Path) -> Result<(), ImportError> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("protocol.file.allow=never")
        .arg("clone")
        .arg("--quiet")
        .arg("--no-hardlinks")
        .arg("--no-recurse-submodules")
        .arg("--")
        .arg(url)
        .arg(destination)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = time::timeout(CLONE_TIMEOUT, command.output())
        .await
        .map_err(|_| ImportError::Git("clone timed out".into()))?
        .map_err(ImportError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(
            stderr
                .chars()
                .take(512)
                .collect::<String>()
                .trim()
                .to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_clean_https_urls() {
        assert!(validate_url("https://example.com/org/repo.git", false).is_ok());
        assert!(validate_url("http://example.com/org/repo.git", false).is_err());
        assert!(validate_url("http://example.com/org/repo.git", true).is_ok());
    }

    #[test]
    fn rejects_credentials_flags_and_whitespace() {
        for invalid in [
            "https://user:pass@example.com/repo.git",
            "https://user@example.com/repo.git",
            "--upload-pack=evil",
            "https://example.com/repo.git?x=1",
            " https://example.com/repo.git",
            "https://example.com/repo .git",
            "file:///tmp/repo",
        ] {
            assert!(validate_url(invalid, false).is_err(), "{invalid}");
        }
    }
}
