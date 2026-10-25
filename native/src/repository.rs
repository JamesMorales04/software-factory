//! Canonical local Git repository identity.
//!
//! Identity is derived from the symlink-resolved path and the repository's
//! root commit set. Relocated or symlinked checkouts of the same repository
//! share an identity; a different repository does not. This module only reads
//! Git metadata and grants no execution authority.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("repository path is invalid: {0}")]
    Invalid(String),
    #[error("git {command}: {stderr}")]
    Git { command: String, stderr: String },
    #[error("repository identity differs from the frozen project binding")]
    IdentityMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryIdentity {
    pub path: PathBuf,
    pub identity: String,
    pub roots: Vec<String>,
    pub base_tips: BTreeMap<String, String>,
    pub is_bare: bool,
}

pub fn inspect(path: &Path, base_refs: &[String]) -> Result<RepositoryIdentity, RepositoryError> {
    if path.as_os_str().is_empty() {
        return Err(RepositoryError::Invalid("path is required".into()));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| RepositoryError::Invalid(format!("path is not accessible: {error}")))?;
    if !canonical.is_dir() {
        return Err(RepositoryError::Invalid("path must be a directory".into()));
    }
    run_git(&canonical, &["rev-parse", "--git-dir"])
        .map_err(|_| RepositoryError::Invalid("path is not a local Git repository".into()))?;
    let bare = run_git(&canonical, &["rev-parse", "--is-bare-repository"])?;
    let is_bare = String::from_utf8_lossy(&bare.stdout).trim() == "true";
    let roots = run_git(&canonical, &["rev-list", "--max-parents=0", "HEAD"]).map_err(|_| {
        RepositoryError::Invalid("repository must have a committed HEAD before registration".into())
    })?;
    let mut roots: Vec<String> = String::from_utf8_lossy(&roots.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if roots.is_empty() {
        return Err(RepositoryError::Invalid(
            "repository has no root commit".into(),
        ));
    }
    roots.sort();
    let path_text = canonical
        .to_str()
        .ok_or_else(|| RepositoryError::Invalid("repository path is not UTF-8".into()))?;
    let mut digest = Sha256::new();
    digest.update(b"factory-git-identity-v2\n");
    digest.update(path_text.as_bytes());
    digest.update(b"\n");
    digest.update(roots.join("\n").as_bytes());
    let identity = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut base_tips = BTreeMap::new();
    for reference in base_refs {
        validate_ref(reference)?;
        let commit = run_git(
            &canonical,
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        )
        .map_err(|_| {
            RepositoryError::Invalid(format!(
                "authorized base ref {reference:?} does not resolve to a commit"
            ))
        })?;
        let commit = String::from_utf8_lossy(&commit.stdout).trim().to_owned();
        if !is_object_id(&commit) {
            return Err(RepositoryError::Invalid(format!(
                "authorized base ref {reference:?} returned a malformed commit"
            )));
        }
        base_tips.insert(reference.clone(), commit);
    }
    Ok(RepositoryIdentity {
        path: canonical,
        identity,
        roots,
        base_tips,
        is_bare,
    })
}

pub fn validate_base(
    repository: &RepositoryIdentity,
    base_revision: &str,
) -> Result<(), RepositoryError> {
    if repository
        .base_tips
        .values()
        .any(|tip| tip == base_revision)
    {
        return Ok(());
    }
    Err(RepositoryError::Invalid(format!(
        "base revision {base_revision} is not the tip of an authorized base ref"
    )))
}

pub fn revalidate_bound(
    path: &Path,
    expected_identity: &str,
    base_revision: &str,
) -> Result<(), RepositoryError> {
    if !is_object_id(base_revision) {
        return Err(RepositoryError::Invalid(
            "frozen base revision is not a full Git object ID".into(),
        ));
    }
    let repository = inspect(path, &[])?;
    if !repository.identity.eq_ignore_ascii_case(expected_identity) {
        return Err(RepositoryError::IdentityMismatch);
    }
    run_git(
        &repository.path,
        &["cat-file", "-e", &format!("{base_revision}^{{commit}}")],
    )
    .map_err(|_| {
        RepositoryError::Invalid(
            "frozen base revision is no longer present in the repository".into(),
        )
    })?;
    Ok(())
}

fn validate_ref(reference: &str) -> Result<(), RepositoryError> {
    let valid = !reference.is_empty()
        && reference.len() <= 256
        && !reference.starts_with('-')
        && !reference.starts_with('/')
        && !reference.ends_with('/')
        && !reference.contains("..")
        && !reference.contains("@{")
        && !reference.contains("//")
        && reference
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && reference
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'));
    if valid {
        Ok(())
    } else {
        Err(RepositoryError::Invalid(
            "authorized base ref has an unsafe shape".into(),
        ))
    }
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn run_git(path: &Path, args: &[&str]) -> Result<std::process::Output, RepositoryError> {
    let command = format!("git {}", args.join(" "));
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|error| RepositoryError::Git {
            command: command.clone(),
            stderr: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(RepositoryError::Git {
            command,
            stderr: bounded(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    Ok(output)
}

fn bounded(value: &str) -> String {
    value.trim().chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "factory-repository-test-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            git(&path, &["init", "-q", "-b", "main"]);
            fs::write(path.join("file.txt"), "one\n").unwrap();
            git(&path, &["add", "file.txt"]);
            git(
                &path,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.test",
                    "commit",
                    "-qm",
                    "one",
                ],
            );
            Self(path)
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(path: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn identity_is_path_independent_and_base_bound() {
        let repo = TempRepo::new();
        let inspected = inspect(&repo.0, &["main".into()]).unwrap();
        assert_eq!(inspected.identity.len(), 64);
        assert_eq!(inspected.roots.len(), 1);
        assert_eq!(
            inspected.base_tips["main"],
            git(&repo.0, &["rev-parse", "HEAD"])
        );
        validate_base(&inspected, &inspected.base_tips["main"]).unwrap();
        assert!(validate_base(&inspected, &"0".repeat(40)).is_err());

        let alias = std::env::temp_dir().join(format!(
            "factory-repository-alias-{}-{}",
            std::process::id(),
            inspected.identity
        ));
        let _ = fs::remove_file(&alias);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&repo.0, &alias).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&repo.0, &alias).unwrap();
        let aliased = inspect(&alias, &["main".into()]).unwrap();
        assert_eq!(aliased.identity, inspected.identity);
        assert_eq!(aliased.path, inspected.path);
        revalidate_bound(&repo.0, &inspected.identity, &inspected.base_tips["main"]).unwrap();
        assert!(revalidate_bound(&repo.0, &"a".repeat(64), &inspected.base_tips["main"]).is_err());
        let _ = fs::remove_file(alias);
    }

    #[test]
    fn rejects_unrelated_repositories_and_unsafe_refs() {
        let first = TempRepo::new();
        let second = TempRepo::new();
        let a = inspect(&first.0, &["main".into()]).unwrap();
        let b = inspect(&second.0, &["main".into()]).unwrap();
        assert_ne!(a.identity, b.identity);
        assert!(inspect(&first.0, &["does-not-exist".into()]).is_err());
        for reference in ["-c", "main..evil", "main@{1}", "main//x", "main/"] {
            assert!(
                inspect(&first.0, &[reference.into()]).is_err(),
                "{reference}"
            );
        }
        assert!(inspect(&std::env::temp_dir(), &["main".into()]).is_err());
    }
}
