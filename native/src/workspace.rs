//! Factory-owned disposable Git worktrees for individual Attempts.
//!
//! A Git worktree is a source-control boundary only. This module does not
//! provide process, filesystem, network, or credential isolation.

use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("git command failed ({command}): {stderr}")]
    Git { command: String, stderr: String },
    #[error("invalid workspace configuration: {0}")]
    Invalid(String),
    #[error("attempt workspace cleanup failed: {0}")]
    Cleanup(String),
}

#[derive(Debug, Clone)]
pub struct WorkspaceAllocator {
    repository: PathBuf,
    workspace_root: PathBuf,
}

#[derive(Debug)]
pub struct AttemptWorkspace {
    repository: PathBuf,
    path: PathBuf,
    attempt_id: String,
    base_commit: String,
    cleaned: bool,
}

/// Commit and tree identities are immutable Git object IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateRevision {
    pub commit_sha: String,
    pub tree_sha: String,
}

impl WorkspaceAllocator {
    /// Construct an allocator for a registered repository and Factory-owned
    /// workspace directory. The workspace directory must not be inside the
    /// registered repository.
    pub fn new(
        registered_repo: impl AsRef<Path>,
        workspace_root: impl AsRef<Path>,
    ) -> Result<Self, WorkspaceError> {
        let repository = registered_repo.as_ref().canonicalize()?;
        if !repository.is_dir() {
            return Err(WorkspaceError::Invalid(
                "registered repository is not a directory".into(),
            ));
        }
        let top = git(
            &repository,
            [OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
        )?;
        let top = String::from_utf8(top.stdout).map_err(|error| {
            WorkspaceError::Invalid(format!("Git returned a non-UTF-8 repository path: {error}"))
        })?;
        let top = top.strip_suffix('\n').unwrap_or(&top);
        let top = PathBuf::from(top);
        let top = top.canonicalize()?;
        if top != repository {
            return Err(WorkspaceError::Invalid(
                "registered path must be the Git repository root".into(),
            ));
        }

        let requested_root = absolute_path(workspace_root.as_ref())?;
        reject_symlink_components(&requested_root)?;
        fs::create_dir_all(&requested_root)?;
        let workspace_root = requested_root.canonicalize()?;
        if workspace_root.starts_with(&repository) {
            return Err(WorkspaceError::Invalid(
                "workspace root cannot be inside the registered repository".into(),
            ));
        }
        Ok(Self {
            repository,
            workspace_root,
        })
    }

    /// Allocate a detached worktree pinned to a frozen commit for one Attempt.
    pub fn allocate(
        &self,
        attempt_id: &str,
        frozen_base: &str,
    ) -> Result<AttemptWorkspace, WorkspaceError> {
        validate_attempt_id(attempt_id)?;
        let base_commit = resolve_commit(&self.repository, frozen_base)?;
        let path = self.workspace_root.join(attempt_id);
        if path.exists() || fs::symlink_metadata(&path).is_ok() {
            return Err(WorkspaceError::Invalid(format!(
                "attempt workspace already exists: {attempt_id}"
            )));
        }
        if let Err(error) = git(
            &self.repository,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                path.as_os_str(),
                OsStr::new(&base_commit),
            ],
        ) {
            // Git can create a worktree before reporting a later failure.
            let cleanup = git(
                &self.repository,
                [
                    "worktree",
                    "remove",
                    "--force",
                    path.to_string_lossy().as_ref(),
                ],
            );
            let _ = fs::remove_dir_all(&path);
            if cleanup.is_err() || path.exists() || worktree_registered(&self.repository, &path)? {
                return Err(WorkspaceError::Cleanup(format!(
                    "allocation failed ({error}); partial worktree cleanup could not be confirmed"
                )));
            }
            return Err(error);
        }
        Ok(AttemptWorkspace {
            repository: self.repository.clone(),
            path,
            attempt_id: attempt_id.into(),
            base_commit,
            cleaned: false,
        })
    }
}

impl AttemptWorkspace {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
    pub fn base_commit(&self) -> &str {
        &self.base_commit
    }

    /// Return the clean committed HEAD only when it is a single commit directly
    /// based on the frozen base. Unrelated, merge, and unchanged revisions are
    /// not CandidateRevisions.
    pub fn candidate_revision(&self) -> Result<CandidateRevision, WorkspaceError> {
        if self.cleaned || !self.path.is_dir() {
            return Err(WorkspaceError::Invalid(
                "attempt workspace is unavailable".into(),
            ));
        }
        let status = git(
            &self.path,
            [
                OsStr::new("status"),
                OsStr::new("--porcelain=v1"),
                OsStr::new("--untracked-files=all"),
            ],
        )?;
        if !status.stdout.is_empty() {
            return Err(WorkspaceError::Invalid(
                "candidate worktree has uncommitted or untracked changes".into(),
            ));
        }
        let data = git(
            &self.path,
            [
                OsStr::new("show"),
                OsStr::new("-s"),
                OsStr::new("--format=%H%n%T"),
                OsStr::new("HEAD"),
            ],
        )?;
        let output = String::from_utf8_lossy(&data.stdout).into_owned();
        let mut lines = output.lines().map(str::to_owned);
        let commit_sha = lines
            .next()
            .ok_or_else(|| WorkspaceError::Invalid("Git returned no candidate commit".into()))?;
        let tree_sha = lines
            .next()
            .ok_or_else(|| WorkspaceError::Invalid("Git returned no candidate tree".into()))?;
        if !is_object_id(&commit_sha) || !is_object_id(&tree_sha) {
            return Err(WorkspaceError::Invalid(
                "Git returned malformed candidate identity".into(),
            ));
        }
        if commit_sha.eq_ignore_ascii_case(&self.base_commit) {
            return Err(WorkspaceError::Invalid(
                "candidate contains no committed change from the frozen base".into(),
            ));
        }
        let ancestry = git(
            &self.path,
            [
                OsStr::new("rev-list"),
                OsStr::new("--parents"),
                OsStr::new("-n"),
                OsStr::new("1"),
                OsStr::new("HEAD"),
            ],
        )?;
        let ancestry = String::from_utf8(ancestry.stdout).map_err(|error| {
            WorkspaceError::Invalid(format!(
                "Git returned non-UTF-8 candidate ancestry: {error}"
            ))
        })?;
        let mut ids = ancestry.split_whitespace();
        let resolved_commit = ids.next().unwrap_or_default();
        let parent = ids.next();
        if !resolved_commit.eq_ignore_ascii_case(&commit_sha)
            || parent.is_none_or(|parent| !parent.eq_ignore_ascii_case(&self.base_commit))
            || ids.next().is_some()
        {
            return Err(WorkspaceError::Invalid(
                "candidate must be a single commit whose only parent is the frozen base".into(),
            ));
        }
        Ok(CandidateRevision {
            commit_sha,
            tree_sha,
        })
    }

    /// Remove the worktree and its Git registration. Errors are returned so
    /// callers can persist cleanup failures and retry using the same handle.
    pub fn cleanup(&mut self) -> Result<(), WorkspaceError> {
        if !self.cleaned {
            let out = git(
                &self.repository,
                [
                    OsStr::new("worktree"),
                    OsStr::new("remove"),
                    OsStr::new("--force"),
                    self.path.as_os_str(),
                ],
            );
            if let Err(error) = out {
                return Err(WorkspaceError::Cleanup(error.to_string()));
            }
            if self.path.exists() || fs::symlink_metadata(&self.path).is_ok() {
                return Err(WorkspaceError::Cleanup(format!(
                    "Git reported removal but path still exists: {}",
                    self.path.display()
                )));
            }
            if worktree_registered(&self.repository, &self.path)? {
                return Err(WorkspaceError::Cleanup(format!(
                    "Git worktree registration remains for {}",
                    self.path.display()
                )));
            }
            self.cleaned = true;
        }
        Ok(())
    }
}

fn worktree_registered(repo: &Path, path: &Path) -> Result<bool, WorkspaceError> {
    let out = git(
        repo,
        [
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
        ],
    )?;
    let expected = format!("worktree {}", path.display());
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line == expected))
}

fn resolve_commit(repo: &Path, value: &str) -> Result<String, WorkspaceError> {
    if !is_object_id(value) {
        return Err(WorkspaceError::Invalid(
            "frozen base must be a full Git object ID".into(),
        ));
    }
    let expression = format!("{value}^{{commit}}");
    let out = git(
        repo,
        [
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new(&expression),
        ],
    )?;
    let output = String::from_utf8(out.stdout).map_err(|error| {
        WorkspaceError::Invalid(format!("Git returned a non-UTF-8 frozen base ID: {error}"))
    })?;
    let commit = output.strip_suffix('\n').unwrap_or(&output);
    if !is_object_id(commit) {
        return Err(WorkspaceError::Invalid(
            "frozen base did not resolve to a commit".into(),
        ));
    }
    if !commit.eq_ignore_ascii_case(value) {
        return Err(WorkspaceError::Invalid(
            "frozen base object ID is not the exact commit ID (tag peeling is not allowed)".into(),
        ));
    }
    Ok(commit.to_owned())
}

fn git<I, S>(cwd: &Path, args: I) -> Result<Output, WorkspaceError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args.into_iter().map(|s| s.as_ref().to_owned()).collect();
    let display = args
        .iter()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let output = Command::new("git").args(&args).current_dir(cwd).output()?;
    if !output.status.success() {
        return Err(WorkspaceError::Git {
            command: display,
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    Ok(output)
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn validate_attempt_id(value: &str) -> Result<(), WorkspaceError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(WorkspaceError::Invalid(
            "attempt ID must contain only ASCII letters, digits, '-' or '_'".into(),
        ));
    }
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn reject_symlink_components(path: &Path) -> Result<(), WorkspaceError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                current.push("..");
            }
            Component::Normal(part) => current.push(part),
        }
        if let Ok(metadata) = fs::symlink_metadata(&current)
            && metadata.file_type().is_symlink()
        {
            return Err(WorkspaceError::Invalid(format!(
                "workspace path contains symlink component: {}",
                current.display()
            )));
        }
    }
    Ok(())
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
                "factory-workspace-test-{}-{nonce}",
                std::process::id()
            ));
            Self::new_at(path)
        }
        fn new_at(path: PathBuf) -> Self {
            fs::create_dir_all(&path).unwrap();
            git(&path, ["init", "-q"]).unwrap();
            git(
                &path,
                ["config", "user.email", "factory-test@example.invalid"],
            )
            .unwrap();
            git(&path, ["config", "user.name", "Factory Test"]).unwrap();
            fs::write(path.join("seed.txt"), "base\n").unwrap();
            git(&path, ["add", "seed.txt"]).unwrap();
            git(&path, ["commit", "-qm", "base"]).unwrap();
            Self(path)
        }
        fn head(&self) -> String {
            String::from_utf8(git(&self.0, ["rev-parse", "HEAD"]).unwrap().stdout)
                .unwrap()
                .trim()
                .into()
        }
    }
    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn allocates_frozen_worktree_and_extracts_candidate_identity_after_commit() {
        let repo = TempRepo::new();
        let root = std::env::temp_dir().join(format!("factory-attempts-{}", repo.head()));
        let _ = fs::remove_dir_all(&root);
        let allocator = WorkspaceAllocator::new(&repo.0, &root).unwrap();
        let mut attempt = allocator.allocate("attempt_1", &repo.head()).unwrap();
        assert_eq!(attempt.base_commit(), repo.head());
        assert_eq!(
            fs::read_to_string(attempt.path().join("seed.txt")).unwrap(),
            "base\n"
        );
        fs::write(attempt.path().join("seed.txt"), "candidate\n").unwrap();
        assert!(
            attempt.candidate_revision().is_err(),
            "dirty workspace must not be candidate"
        );
        git(attempt.path(), ["add", "seed.txt"]).unwrap();
        git(attempt.path(), ["commit", "-qm", "candidate"]).unwrap();
        let candidate = attempt.candidate_revision().unwrap();
        assert_ne!(candidate.commit_sha, repo.head());
        assert!(is_object_id(&candidate.tree_sha));
        attempt.cleanup().unwrap();
        assert!(!root.join("attempt_1").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_path_traversal_symlinks_and_non_commit_ids() {
        let repo = TempRepo::new();
        let parent = std::env::temp_dir().join(format!("factory-symlink-{}", repo.head()));
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(&parent).unwrap();
        let target = parent.join("target");
        fs::create_dir(&target).unwrap();
        let link = parent.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &link).unwrap();
        assert!(WorkspaceAllocator::new(&repo.0, link.join("worktrees")).is_err());
        let allocator = WorkspaceAllocator::new(&repo.0, parent.join("safe")).unwrap();
        assert!(allocator.allocate("../escape", &repo.head()).is_err());
        assert!(allocator.allocate("valid", "main").is_err());
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn accepts_registered_repository_path_ending_in_space() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repo = TempRepo::new_at(
            std::env::temp_dir().join(format!("factory-workspace-trailing-space-{nonce} ")),
        );
        let root = std::env::temp_dir().join(format!("factory-space-attempts-{nonce}"));
        let allocator = WorkspaceAllocator::new(&repo.0, &root).unwrap();
        let mut attempt = allocator.allocate("space_path", &repo.head()).unwrap();
        assert_eq!(
            fs::read_to_string(attempt.path().join("seed.txt")).unwrap(),
            "base\n"
        );
        attempt.cleanup().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_annotated_tag_object_oid_as_frozen_commit() {
        let repo = TempRepo::new();
        git(&repo.0, ["tag", "-a", "release", "-m", "release"]).unwrap();
        let tag_oid = String::from_utf8(
            git(&repo.0, ["rev-parse", "refs/tags/release"])
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let root = std::env::temp_dir().join(format!("factory-tag-attempts-{}", repo.head()));
        let allocator = WorkspaceAllocator::new(&repo.0, &root).unwrap();
        assert!(allocator.allocate("tag_oid", &tag_oid).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn candidate_rejects_no_change_unrelated_and_merge_commits() {
        let repo = TempRepo::new();
        let base = repo.head();
        let root = std::env::temp_dir().join(format!("factory-ancestry-attempts-{base}"));
        let _ = fs::remove_dir_all(&root);
        let allocator = WorkspaceAllocator::new(&repo.0, &root).unwrap();

        let mut unchanged = allocator.allocate("unchanged", &base).unwrap();
        assert!(unchanged.candidate_revision().is_err());
        unchanged.cleanup().unwrap();

        let tree = String::from_utf8(
            git(&repo.0, ["rev-parse", &format!("{base}^{{tree}}")])
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let unrelated = String::from_utf8(
            git(&repo.0, ["commit-tree", &tree, "-m", "unrelated"])
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let mut unrelated_attempt = allocator.allocate("unrelated", &base).unwrap();
        git(unrelated_attempt.path(), ["reset", "--hard", &unrelated]).unwrap();
        assert!(unrelated_attempt.candidate_revision().is_err());
        unrelated_attempt.cleanup().unwrap();

        fs::write(repo.0.join("side.txt"), "side\n").unwrap();
        git(&repo.0, ["add", "side.txt"]).unwrap();
        git(&repo.0, ["commit", "-qm", "side"]).unwrap();
        let side = repo.head();
        let merge = String::from_utf8(
            git(
                &repo.0,
                [
                    "commit-tree",
                    &tree,
                    "-p",
                    &base,
                    "-p",
                    &side,
                    "-m",
                    "merge",
                ],
            )
            .unwrap()
            .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let mut merge_attempt = allocator.allocate("merge", &base).unwrap();
        git(merge_attempt.path(), ["reset", "--hard", &merge]).unwrap();
        assert!(merge_attempt.candidate_revision().is_err());
        merge_attempt.cleanup().unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
