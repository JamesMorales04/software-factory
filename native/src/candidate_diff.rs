//! Bounded, read-only file-body diff projection for an exact Candidate.
//!
//! The projection is addressed only by full Git object IDs: the recorded base
//! commit and the recorded Candidate commit/tree. It revalidates the Project
//! repository identity before reading any object and never accepts a host path
//! from the caller.
//!
//! Bounds are deliberate: changed files are capped, each patch is capped, the
//! total patch budget is capped, binary blobs and files above the text budget
//! are omitted, and symlinks/gitlinks are never followed. This module performs
//! no writes and grants no execution authority.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde::Serialize;

use crate::repository::{self, RepositoryError};

/// Maximum changed files returned in one projection.
pub const MAX_DIFF_FILES: usize = 200;
/// Maximum blob size whose text is rendered as a patch.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Maximum bytes of one file patch.
pub const MAX_FILE_PATCH_BYTES: usize = 128 * 1024;
/// Maximum total bytes of all file patches in one projection.
pub const MAX_TOTAL_PATCH_BYTES: usize = 512 * 1024;
/// Maximum repository-relative path length.
pub const MAX_PATH_BYTES: usize = 1_000;

const MAX_GIT_STDERR_BYTES: usize = 512;

const PROVENANCE: &str =
    "Core read-only Git file diff between the frozen base and the exact Candidate; no execution";

#[derive(Debug, thiserror::Error)]
pub enum CandidateDiffError {
    #[error("invalid candidate diff request: {0}")]
    Invalid(String),
    #[error("candidate diff revision was not found")]
    NotFound,
    #[error("project repository binding is stale or does not match the registered identity")]
    Stale,
    #[error("candidate identity does not match the recorded revision: {0}")]
    IdentityMismatch(String),
    #[error("git {command}: {stderr}")]
    Git { command: String, stderr: String },
    #[error("candidate diff I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffRevision {
    pub commit: String,
    pub tree: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CandidateFileStatus {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiffOmission {
    Binary,
    TooLarge,
    NotAFile,
    NonUtf8,
    Budget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateFileDiff {
    pub path: String,
    pub status: CandidateFileStatus,
    pub old_mode: String,
    pub new_mode: String,
    pub additions: u64,
    pub deletions: u64,
    pub binary: bool,
    pub omitted: Option<DiffOmission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub patch_truncated: bool,
    pub patch_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateFileDiffProjection {
    pub project_id: String,
    pub base: DiffRevision,
    pub candidate: DiffRevision,
    pub files: Vec<CandidateFileDiff>,
    pub returned_files: usize,
    pub files_truncated: bool,
    pub total_additions: u64,
    pub total_deletions: u64,
    pub patch_bytes: usize,
    pub patch_truncated: bool,
    pub provenance: &'static str,
}

struct RawChange {
    path: String,
    old_mode: String,
    new_mode: String,
    old_oid: String,
    new_oid: String,
    status: CandidateFileStatus,
}

#[derive(Debug, Clone, Copy)]
struct NumStat {
    additions: u64,
    deletions: u64,
    binary: bool,
}

/// Project the bounded file diff for one exact Candidate.
#[allow(clippy::too_many_arguments)]
pub fn project_candidate_diff(
    project_id: &str,
    repository: &Path,
    expected_identity: &str,
    base_commit: &str,
    candidate_commit: &str,
    candidate_tree_sha: &str,
) -> Result<CandidateFileDiffProjection, CandidateDiffError> {
    validate_object_id(base_commit, "base commit")?;
    validate_object_id(candidate_commit, "candidate commit")?;
    validate_object_id(candidate_tree_sha, "candidate tree")?;
    if base_commit.eq_ignore_ascii_case(candidate_commit) {
        return Err(CandidateDiffError::Invalid(
            "candidate commit equals the recorded base commit".into(),
        ));
    }
    let repository = bind_repository(repository, expected_identity)?;
    let base_tree = resolve_commit_tree(&repository, base_commit, None)?;
    let candidate_tree =
        resolve_commit_tree(&repository, candidate_commit, Some(candidate_tree_sha))?;
    ensure_ancestor(&repository, base_commit, candidate_commit)?;

    let (raw_changes, files_truncated) = raw_changes(&repository, base_commit, candidate_commit)?;
    let paths: Vec<&str> = raw_changes
        .iter()
        .map(|change| change.path.as_str())
        .collect();
    let numstat = numstat_for_paths(&repository, base_commit, candidate_commit, &paths)?;
    let sizes = object_sizes(
        &repository,
        raw_changes
            .iter()
            .flat_map(|change| [change.old_oid.as_str(), change.new_oid.as_str()])
            .filter(|object| !is_zero_object(object)),
    )?;

    let mut files = Vec::with_capacity(raw_changes.len());
    let mut patch_bytes = 0usize;
    let mut patch_truncated = false;
    for change in raw_changes {
        let stat = numstat
            .get(&change.path)
            .copied()
            .ok_or_else(|| CandidateDiffError::Invalid("diff statistics are incomplete".into()))?;
        let mut file = CandidateFileDiff {
            path: change.path.clone(),
            status: change.status,
            old_mode: change.old_mode.clone(),
            new_mode: change.new_mode.clone(),
            additions: stat.additions,
            deletions: stat.deletions,
            binary: stat.binary,
            omitted: None,
            patch: None,
            patch_truncated: false,
            patch_bytes: 0,
        };
        if is_not_a_file(&change.old_mode, &change.new_mode) {
            file.omitted = Some(DiffOmission::NotAFile);
            files.push(file);
            continue;
        }
        if stat.binary {
            file.omitted = Some(DiffOmission::Binary);
            files.push(file);
            continue;
        }
        let too_large = [&change.old_oid, &change.new_oid]
            .into_iter()
            .filter(|object| !is_zero_object(object))
            .filter_map(|object| sizes.get(object).copied())
            .any(|size| size > MAX_FILE_BYTES);
        if too_large {
            file.omitted = Some(DiffOmission::TooLarge);
            files.push(file);
            continue;
        }
        let remaining = MAX_TOTAL_PATCH_BYTES.saturating_sub(patch_bytes);
        if remaining == 0 {
            file.omitted = Some(DiffOmission::Budget);
            patch_truncated = true;
            files.push(file);
            continue;
        }
        let patch_output = file_patch(&repository, base_commit, candidate_commit, &change.path)?;
        let limit = remaining.min(MAX_FILE_PATCH_BYTES);
        let (patch, truncated) = bounded_patch(&patch_output, limit)?;
        patch_bytes = patch_bytes.saturating_add(patch.len());
        if truncated {
            patch_truncated = true;
            file.patch_truncated = true;
        }
        if patch.is_empty() && !patch_output.is_empty() {
            file.omitted = Some(DiffOmission::Budget);
        } else if patch.is_empty() {
            // A pure mode change still has a valid, tiny patch header. Empty
            // output here means Git produced no renderable body.
            file.omitted = Some(DiffOmission::Budget);
        } else {
            file.patch_bytes = patch.len();
            file.patch = Some(patch);
        }
        files.push(file);
    }

    let total_additions = files.iter().map(|file| file.additions).sum();
    let total_deletions = files.iter().map(|file| file.deletions).sum();
    Ok(CandidateFileDiffProjection {
        project_id: project_id.to_owned(),
        base: DiffRevision {
            commit: base_commit.to_ascii_lowercase(),
            tree: base_tree,
        },
        candidate: DiffRevision {
            commit: candidate_commit.to_ascii_lowercase(),
            tree: candidate_tree,
        },
        returned_files: files.len(),
        files,
        files_truncated,
        total_additions,
        total_deletions,
        patch_bytes,
        patch_truncated,
        provenance: PROVENANCE,
    })
}

/// True when `path` is a safe repository-relative path for display.
pub fn is_safe_diff_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.chars().any(char::is_control)
        && path != ".git"
        && !path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        && !path
            .split('/')
            .any(|component| component.eq_ignore_ascii_case(".git"))
}

fn bind_repository(
    repository: &Path,
    expected_identity: &str,
) -> Result<PathBuf, CandidateDiffError> {
    let inspected = repository::inspect(repository, &[]).map_err(map_repository_error)?;
    if expected_identity.trim().is_empty()
        || !inspected
            .identity
            .eq_ignore_ascii_case(expected_identity.trim())
    {
        return Err(CandidateDiffError::Stale);
    }
    Ok(inspected.path)
}

fn resolve_commit_tree(
    repository: &Path,
    commit: &str,
    expected_tree: Option<&str>,
) -> Result<String, CandidateDiffError> {
    if !is_object_id(commit) {
        return Err(CandidateDiffError::Invalid(
            "revision must be a full Git commit ID".into(),
        ));
    }
    if run_git(
        repository,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
    )
    .is_err()
    {
        return Err(CandidateDiffError::NotFound);
    }
    let resolved = run_git_text(
        repository,
        &["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
    )?;
    if !resolved.eq_ignore_ascii_case(commit) {
        return Err(CandidateDiffError::IdentityMismatch(
            "revision did not resolve to the exact recorded commit".into(),
        ));
    }
    let tree = run_git_text(
        repository,
        &["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
    )?;
    if !is_object_id(&tree) {
        return Err(CandidateDiffError::Invalid(
            "commit did not resolve to a Git tree".into(),
        ));
    }
    if let Some(expected) = expected_tree
        && !tree.eq_ignore_ascii_case(expected)
    {
        return Err(CandidateDiffError::IdentityMismatch(
            "recorded candidate tree differs from the commit tree".into(),
        ));
    }
    Ok(tree.to_ascii_lowercase())
}

fn ensure_ancestor(
    repository: &Path,
    base_commit: &str,
    candidate_commit: &str,
) -> Result<(), CandidateDiffError> {
    let output = hardened_git()
        .arg("-C")
        .arg(repository)
        .args(["merge-base", "--is-ancestor", base_commit, candidate_commit])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(1) => Err(CandidateDiffError::IdentityMismatch(
            "candidate is not descended from the recorded base commit".into(),
        )),
        _ => Err(CandidateDiffError::Git {
            command: "git merge-base --is-ancestor".into(),
            stderr: bounded_stderr(&output.stderr),
        }),
    }
}

fn raw_changes(
    repository: &Path,
    base_commit: &str,
    candidate_commit: &str,
) -> Result<(Vec<RawChange>, bool), CandidateDiffError> {
    let mut command = hardened_git();
    command
        .arg("-C")
        .arg(repository)
        .args([
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--raw",
            "--no-abbrev",
            "-z",
            base_commit,
            candidate_commit,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| CandidateDiffError::Git {
        command: "git diff --raw".into(),
        stderr: "stdout pipe is unavailable".into(),
    })?;
    let mut reader = BufReader::new(stdout);
    let mut metadata = Vec::new();
    let mut path = Vec::new();
    let mut changes = Vec::new();
    let mut truncated = false;
    loop {
        metadata.clear();
        if reader.read_until(0, &mut metadata)? == 0 {
            break;
        }
        if metadata.last() == Some(&0) {
            metadata.pop();
        }
        if metadata.is_empty() {
            continue;
        }
        path.clear();
        if reader.read_until(0, &mut path)? == 0 {
            return Err(CandidateDiffError::Invalid(
                "raw Git diff is missing a path".into(),
            ));
        }
        if path.last() == Some(&0) {
            path.pop();
        }
        if changes.len() == MAX_DIFF_FILES {
            truncated = true;
            break;
        }
        changes.push(parse_raw_change(&metadata, &path)?);
    }
    drop(reader);
    if truncated {
        let _ = child.kill();
    }
    let output = child.wait_with_output()?;
    if !truncated && !output.status.success() {
        return Err(CandidateDiffError::Git {
            command: "git diff --raw".into(),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    Ok((changes, truncated))
}

fn parse_raw_change(metadata: &[u8], path: &[u8]) -> Result<RawChange, CandidateDiffError> {
    let path = std::str::from_utf8(path)
        .map_err(|_| CandidateDiffError::Invalid("changed path is not UTF-8".into()))?;
    if !is_safe_diff_path(path) {
        return Err(CandidateDiffError::Invalid(format!(
            "candidate changed an unsafe path: {path:?}"
        )));
    }
    let metadata = std::str::from_utf8(metadata)
        .map_err(|_| CandidateDiffError::Invalid("raw Git metadata is not UTF-8".into()))?;
    let fields: Vec<&str> = metadata
        .trim_start_matches(':')
        .split_whitespace()
        .collect();
    if fields.len() != 5 {
        return Err(CandidateDiffError::Invalid(
            "raw Git diff has an unexpected shape".into(),
        ));
    }
    let old_mode = fields[0];
    let new_mode = fields[1];
    let old_oid = fields[2];
    let new_oid = fields[3];
    if !is_object_id(old_oid) || !is_object_id(new_oid) {
        return Err(CandidateDiffError::Invalid(
            "raw Git diff has a malformed object ID".into(),
        ));
    }
    let status = match fields[4] {
        "A" => CandidateFileStatus::Added,
        "D" => CandidateFileStatus::Deleted,
        "M" | "T" => CandidateFileStatus::Modified,
        other => {
            return Err(CandidateDiffError::Invalid(format!(
                "unsupported Git diff status {other:?}"
            )));
        }
    };
    if old_mode.len() != 6
        || new_mode.len() != 6
        || !old_mode.bytes().all(|byte| byte.is_ascii_digit())
        || !new_mode.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(CandidateDiffError::Invalid(
            "raw Git diff has a malformed file mode".into(),
        ));
    }
    Ok(RawChange {
        path: path.to_owned(),
        old_mode: old_mode.to_owned(),
        new_mode: new_mode.to_owned(),
        old_oid: old_oid.to_ascii_lowercase(),
        new_oid: new_oid.to_ascii_lowercase(),
        status,
    })
}

fn numstat_for_paths(
    repository: &Path,
    base_commit: &str,
    candidate_commit: &str,
    paths: &[&str],
) -> Result<HashMap<String, NumStat>, CandidateDiffError> {
    if paths.is_empty() {
        return Ok(HashMap::new());
    }
    let mut args = vec![
        "diff".to_owned(),
        "--no-ext-diff".to_owned(),
        "--no-renames".to_owned(),
        "--numstat".to_owned(),
        "-z".to_owned(),
        base_commit.to_owned(),
        candidate_commit.to_owned(),
        "--".to_owned(),
    ];
    args.extend(paths.iter().map(|path| (*path).to_owned()));
    let output = hardened_git()
        .arg("-C")
        .arg(repository)
        .args(&args)
        .output()?;
    if !output.status.success() {
        return Err(CandidateDiffError::Git {
            command: "git diff --numstat".into(),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    let mut stats = HashMap::with_capacity(paths.len());
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let first = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| CandidateDiffError::Invalid("numstat record has no tab".into()))?;
        let second = record[first + 1..]
            .iter()
            .position(|byte| *byte == b'\t')
            .map(|offset| first + 1 + offset)
            .ok_or_else(|| CandidateDiffError::Invalid("numstat record has one tab".into()))?;
        let additions = std::str::from_utf8(&record[..first])
            .map_err(|_| CandidateDiffError::Invalid("numstat addition count is invalid".into()))?;
        let deletions = std::str::from_utf8(&record[first + 1..second])
            .map_err(|_| CandidateDiffError::Invalid("numstat deletion count is invalid".into()))?;
        let path = std::str::from_utf8(&record[second + 1..])
            .map_err(|_| CandidateDiffError::Invalid("numstat path is not UTF-8".into()))?;
        if !is_safe_diff_path(path) {
            return Err(CandidateDiffError::Invalid(format!(
                "numstat reported an unsafe path: {path:?}"
            )));
        }
        let binary = additions == "-" && deletions == "-";
        let additions = if binary {
            0
        } else {
            additions.parse::<u64>().map_err(|_| {
                CandidateDiffError::Invalid("numstat addition count is invalid".into())
            })?
        };
        let deletions = if binary {
            0
        } else {
            deletions.parse::<u64>().map_err(|_| {
                CandidateDiffError::Invalid("numstat deletion count is invalid".into())
            })?
        };
        stats.insert(
            path.to_owned(),
            NumStat {
                additions,
                deletions,
                binary,
            },
        );
    }
    Ok(stats)
}

fn object_sizes<'a>(
    repository: &Path,
    objects: impl Iterator<Item = &'a str>,
) -> Result<HashMap<String, u64>, CandidateDiffError> {
    let mut ids: Vec<&str> = objects.collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut command = hardened_git();
    command
        .arg("-C")
        .arg(repository)
        .args([
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| CandidateDiffError::Git {
                command: "git cat-file".into(),
                stderr: "stdin pipe is unavailable".into(),
            })?;
        for id in &ids {
            stdin.write_all(id.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(CandidateDiffError::Git {
            command: "git cat-file --batch-check".into(),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    let mut sizes = HashMap::with_capacity(ids.len());
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(line)
            .map_err(|_| CandidateDiffError::Invalid("cat-file output is not UTF-8".into()))?;
        let fields: Vec<&str> = text.split_whitespace().collect();
        if fields.len() != 3 {
            return Err(CandidateDiffError::Invalid(
                "cat-file output has an unexpected shape".into(),
            ));
        }
        let size = fields[2]
            .parse::<u64>()
            .map_err(|_| CandidateDiffError::Invalid("cat-file size is invalid".into()))?;
        sizes.insert(fields[0].to_ascii_lowercase(), size);
    }
    Ok(sizes)
}

fn file_patch(
    repository: &Path,
    base_commit: &str,
    candidate_commit: &str,
    path: &str,
) -> Result<Vec<u8>, CandidateDiffError> {
    let output = hardened_git()
        .arg("-C")
        .arg(repository)
        .args([
            "diff",
            "--no-ext-diff",
            "--no-renames",
            "--no-color",
            "--unified=3",
            base_commit,
            candidate_commit,
            "--",
            path,
        ])
        .output()?;
    if !output.status.success() {
        return Err(CandidateDiffError::Git {
            command: "git diff --patch".into(),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    Ok(output.stdout)
}

fn bounded_patch(bytes: &[u8], limit: usize) -> Result<(String, bool), CandidateDiffError> {
    if bytes.len() <= limit {
        let patch = std::str::from_utf8(bytes)
            .map_err(|_| CandidateDiffError::Invalid("candidate patch is not UTF-8".into()))?;
        return Ok((patch.to_owned(), false));
    }
    let mut end = limit;
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    let text = std::str::from_utf8(&bytes[..end])
        .map_err(|_| CandidateDiffError::Invalid("candidate patch is not UTF-8".into()))?;
    let cut = text
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(text.len());
    Ok((text[..cut].to_owned(), true))
}

fn is_not_a_file(old_mode: &str, new_mode: &str) -> bool {
    matches!(old_mode, "120000" | "160000") || matches!(new_mode, "120000" | "160000")
}

fn validate_object_id(value: &str, what: &str) -> Result<(), CandidateDiffError> {
    if is_object_id(value) {
        Ok(())
    } else {
        Err(CandidateDiffError::Invalid(format!(
            "{what} must be a full 40- or 64-character Git object ID"
        )))
    }
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_zero_object(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte == b'0')
}

fn run_git(repository: &Path, args: &[&str]) -> Result<Output, CandidateDiffError> {
    let output = hardened_git()
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(CandidateDiffError::Git {
            command: format!("git {}", args.join(" ")),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    Ok(output)
}

fn run_git_text(repository: &Path, args: &[&str]) -> Result<String, CandidateDiffError> {
    let output = run_git(repository, args)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| CandidateDiffError::Invalid("Git returned non-UTF-8 output".into()))
}

fn hardened_git() -> Command {
    let mut command = Command::new("git");
    command
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE");
    command
}

fn bounded_stderr(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim()
        .chars()
        .take(MAX_GIT_STDERR_BYTES)
        .collect()
}

fn map_repository_error(error: RepositoryError) -> CandidateDiffError {
    match error {
        RepositoryError::IdentityMismatch => CandidateDiffError::Stale,
        RepositoryError::Invalid(message) => CandidateDiffError::Invalid(message),
        RepositoryError::Git { command, stderr } => CandidateDiffError::Git { command, stderr },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempRepo {
        root: PathBuf,
        path: PathBuf,
        identity: String,
        base_commit: String,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "factory-candidate-diff-{name}-{}-{nonce}",
                std::process::id()
            ));
            let path = root.join("repo");
            fs::create_dir_all(path.join("src")).unwrap();
            fs::create_dir_all(path.join("assets")).unwrap();
            fs::write(path.join("src/modify.txt"), "one\ntwo\nthree\n").unwrap();
            fs::write(path.join("src/delete.txt"), "delete me\n").unwrap();
            fs::write(path.join("assets/picture.bin"), b"base\0binary").unwrap();
            fs::write(path.join("assets/large.txt"), "small\n").unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink("modify.txt", path.join("src/link.txt")).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file("modify.txt", path.join("src/link.txt")).unwrap();
            git(&path, &["init", "-q", "-b", "main"]);
            git(&path, &["config", "user.name", "Fixture"]);
            git(&path, &["config", "user.email", "fixture@example.invalid"]);
            git(&path, &["add", "--all"]);
            git(&path, &["commit", "-qm", "base"]);
            let identity = repository::inspect(&path, &[]).unwrap().identity;
            let base_commit = git(&path, &["rev-parse", "HEAD"]);
            Self {
                root,
                path,
                identity,
                base_commit,
            }
        }

        fn commit_all(&self, message: &str) -> String {
            git(&self.path, &["add", "--all"]);
            git(&self.path, &["commit", "-qm", message]);
            git(&self.path, &["rev-parse", "HEAD"])
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
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

    fn project(fixture: &TempRepo, commit: &str) -> CandidateFileDiffProjection {
        project_candidate_diff(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            commit,
            &git(&fixture.path, &["rev-parse", &format!("{commit}^{{tree}}")]),
        )
        .unwrap()
    }

    #[test]
    fn projects_status_counts_patch_and_exact_revisions() {
        let fixture = TempRepo::new("status");
        fs::write(
            fixture.path.join("src/modify.txt"),
            "one\nTWO\nthree\nfour\n",
        )
        .unwrap();
        fs::remove_file(fixture.path.join("src/delete.txt")).unwrap();
        fs::write(fixture.path.join("src/added.txt"), "new\nfile\n").unwrap();
        let commit = fixture.commit_all("candidate");
        let projection = project(&fixture, &commit);
        assert_eq!(projection.project_id, "project-1");
        assert_eq!(projection.base.commit, fixture.base_commit);
        assert_eq!(projection.candidate.commit, commit);
        assert_eq!(
            projection.candidate.tree,
            git(&fixture.path, &["rev-parse", &format!("{commit}^{{tree}}")])
        );
        assert!(!projection.files_truncated);
        assert_eq!(projection.returned_files, 3);
        let added = projection
            .files
            .iter()
            .find(|file| file.path == "src/added.txt")
            .unwrap();
        assert_eq!(added.status, CandidateFileStatus::Added);
        assert_eq!((added.additions, added.deletions), (2, 0));
        assert!(added.patch.as_deref().unwrap().contains("+new"));
        let deleted = projection
            .files
            .iter()
            .find(|file| file.path == "src/delete.txt")
            .unwrap();
        assert_eq!(deleted.status, CandidateFileStatus::Deleted);
        assert_eq!((deleted.additions, deleted.deletions), (0, 1));
        assert!(deleted.patch.as_deref().unwrap().contains("-delete me"));
        let modified = projection
            .files
            .iter()
            .find(|file| file.path == "src/modify.txt")
            .unwrap();
        assert_eq!(modified.status, CandidateFileStatus::Modified);
        assert_eq!((modified.additions, modified.deletions), (2, 1));
        assert!(modified.patch.as_deref().unwrap().contains("-two"));
        assert!(modified.patch.as_deref().unwrap().contains("+TWO"));
        assert_eq!(projection.total_additions, 4);
        assert_eq!(projection.total_deletions, 2);
        assert_eq!(
            projection.patch_bytes,
            projection
                .files
                .iter()
                .map(|f| f.patch_bytes)
                .sum::<usize>()
        );
        assert!(
            !projection
                .provenance
                .contains(&fixture.root.to_string_lossy().to_string())
        );
    }

    #[test]
    fn omits_binary_large_and_symlink_entries() {
        let fixture = TempRepo::new("omission");
        fs::write(
            fixture.path.join("assets/picture.bin"),
            b"candidate\0binary",
        )
        .unwrap();
        fs::write(
            fixture.path.join("assets/large.txt"),
            vec![b'x'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        #[cfg(unix)]
        {
            fs::remove_file(fixture.path.join("src/link.txt")).unwrap();
            std::os::unix::fs::symlink("delete.txt", fixture.path.join("src/link.txt")).unwrap();
        }
        let commit = fixture.commit_all("omissions");
        let projection = project(&fixture, &commit);
        let binary = projection
            .files
            .iter()
            .find(|file| file.path == "assets/picture.bin")
            .unwrap();
        assert!(binary.binary);
        assert_eq!(binary.omitted, Some(DiffOmission::Binary));
        assert!(binary.patch.is_none());
        let large = projection
            .files
            .iter()
            .find(|file| file.path == "assets/large.txt")
            .unwrap();
        assert_eq!(large.omitted, Some(DiffOmission::TooLarge));
        assert!(large.patch.is_none());
        #[cfg(unix)]
        {
            let link = projection
                .files
                .iter()
                .find(|file| file.path == "src/link.txt")
                .unwrap();
            assert_eq!(link.omitted, Some(DiffOmission::NotAFile));
            assert!(link.patch.is_none());
            assert_eq!((link.additions, link.deletions), (1, 1));
        }
    }

    #[test]
    fn rejects_stale_identity_wrong_tree_and_missing_revisions() {
        let fixture = TempRepo::new("identity");
        fs::write(fixture.path.join("src/modify.txt"), "changed\n").unwrap();
        let commit = fixture.commit_all("candidate");
        let tree = git(&fixture.path, &["rev-parse", &format!("{commit}^{{tree}}")]);

        let wrong_tree = project_candidate_diff(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            &commit,
            &"0".repeat(40),
        );
        assert!(matches!(
            wrong_tree,
            Err(CandidateDiffError::IdentityMismatch(_))
        ));

        let stale = project_candidate_diff(
            "project-1",
            &fixture.path,
            &"a".repeat(64),
            &fixture.base_commit,
            &commit,
            &tree,
        );
        assert!(matches!(stale, Err(CandidateDiffError::Stale)));

        let missing = project_candidate_diff(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            &"0".repeat(40),
            &tree,
        );
        assert!(matches!(missing, Err(CandidateDiffError::NotFound)));

        let same = project_candidate_diff(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            &fixture.base_commit,
            &tree,
        );
        assert!(matches!(same, Err(CandidateDiffError::Invalid(_))));

        let short = project_candidate_diff(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit[..12],
            &commit,
            &tree,
        );
        assert!(matches!(short, Err(CandidateDiffError::Invalid(_))));
    }

    #[test]
    fn rejects_a_candidate_not_descended_from_the_base() {
        let fixture = TempRepo::new("lineage");
        git(&fixture.path, &["checkout", "-q", "--orphan", "orphan"]);
        fs::write(fixture.path.join("src/orphan.txt"), "orphan\n").unwrap();
        let commit = fixture.commit_all("orphan candidate");
        git(&fixture.path, &["checkout", "-q", "main"]);
        let tree = git(&fixture.path, &["rev-parse", &format!("{commit}^{{tree}}")]);
        let projection = project_candidate_diff(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            &commit,
            &tree,
        );
        assert!(
            matches!(projection, Err(CandidateDiffError::IdentityMismatch(_))),
            "{projection:?}"
        );
    }

    #[test]
    fn bounds_file_count_and_patch_bytes() {
        let fixture = TempRepo::new("bounds");
        fs::create_dir_all(fixture.path.join("many")).unwrap();
        for index in 0..MAX_DIFF_FILES + 5 {
            fs::write(
                fixture.path.join(format!("many/file-{index:03}.txt")),
                format!("{index}\n"),
            )
            .unwrap();
        }
        let commit = fixture.commit_all("many files");
        let projection = project(&fixture, &commit);
        assert!(projection.files_truncated);
        assert_eq!(projection.files.len(), MAX_DIFF_FILES);
        assert!(projection.files.iter().all(|file| file.patch.is_some()));

        let fixture = TempRepo::new("patch-bound");
        let mut large = String::new();
        for index in 0..12_000 {
            large.push_str(&format!("line {index:05} with some bounded content\n"));
        }
        fs::write(fixture.path.join("src/generated.txt"), large).unwrap();
        let commit = fixture.commit_all("large patch");
        let projection = project(&fixture, &commit);
        let generated = projection
            .files
            .iter()
            .find(|file| file.path == "src/generated.txt")
            .unwrap();
        assert!(generated.patch_truncated);
        assert!(generated.patch_bytes <= MAX_FILE_PATCH_BYTES);
        assert!(projection.patch_bytes <= MAX_TOTAL_PATCH_BYTES);
        assert!(projection.patch_truncated);
    }

    #[test]
    fn rejects_unsafe_and_control_paths() {
        for path in [
            "",
            "/etc/passwd",
            "../outside",
            "src/../../outside",
            ".git/config",
            "src//main.rs",
            "src/",
            "src/./main.rs",
            "src\\main.rs",
            "src/ma\nin.rs",
            "assets/.GIT/config",
        ] {
            assert!(!is_safe_diff_path(path), "{path:?}");
        }
        for path in ["README.md", "src/main.rs", "docs/a b.md"] {
            assert!(is_safe_diff_path(path), "{path:?}");
        }
    }

    #[test]
    fn detects_recorded_modes_and_rejects_unsupported_status() {
        let raw = |mode: &str, oid: &str, status: &str| {
            format!(":{mode} {mode} {oid} {oid} {status}").into_bytes()
        };
        let oid = "a".repeat(40);
        let change = parse_raw_change(&raw("100644", &oid, "M"), b"src/main.rs").unwrap();
        assert_eq!(change.status, CandidateFileStatus::Modified);
        assert!(is_not_a_file("120000", "120000"));
        assert!(is_not_a_file("160000", "160000"));
        assert!(!is_not_a_file("100644", "100755"));
        let unsupported = parse_raw_change(&raw("100644", &oid, "U"), b"src/main.rs");
        assert!(matches!(unsupported, Err(CandidateDiffError::Invalid(_))));
    }
}
