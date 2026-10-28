//! Bounded, read-only file projections for a registered Project repository.
//!
//! Every projection is addressed by an exact Git commit object ID. The module
//! reads Git objects directly and never follows a checked-out symlink, a
//! gitlink, a host path or a mutable branch. Repository identity is revalidated
//! against the frozen Project binding before any object is returned.
//!
//! Git remains the only documentation source: this module does not create a
//! second documentation store and grants no write or execution authority.

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde::Serialize;

use crate::repository::{self, RepositoryError};

/// Maximum number of immediate tree entries returned by one projection.
pub const MAX_TREE_ENTRIES: usize = 500;
/// Maximum blob size returned as text content.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Maximum repository-relative path accepted from a caller.
pub const MAX_PATH_BYTES: usize = 1_000;
/// Maximum number of branch refs returned by the revision projection.
pub const MAX_REFS: usize = 200;

const MAX_GIT_STDERR_BYTES: usize = 512;

const PROVENANCE: &str = "Core read-only Git projection of the registered Project repository";

#[derive(Debug, thiserror::Error)]
pub enum ProjectFilesError {
    #[error("invalid repository browse request: {0}")]
    Invalid(String),
    #[error("repository object was not found")]
    NotFound,
    #[error("project repository binding is stale or does not match the registered identity")]
    Stale,
    #[error("git {command}: {stderr}")]
    Git { command: String, stderr: String },
    #[error("repository browse I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRevision {
    pub commit: String,
    pub tree: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TreeEntryKind {
    File,
    Directory,
    Symlink,
    Submodule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTreeEntry {
    pub name: String,
    pub path: String,
    pub kind: TreeEntryKind,
    pub mode: String,
    pub object_id: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTreeProjection {
    pub project_id: String,
    pub revision: ProjectRevision,
    pub path: String,
    pub entries: Vec<ProjectTreeEntry>,
    pub truncated: bool,
    pub skipped_entries: usize,
    pub provenance: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentOmission {
    Binary,
    TooLarge,
    NotAFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileProjection {
    pub project_id: String,
    pub revision: ProjectRevision,
    pub path: String,
    pub name: String,
    pub kind: TreeEntryKind,
    pub mode: String,
    pub object_id: String,
    pub size: Option<u64>,
    pub content: Option<String>,
    pub omitted: Option<ContentOmission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_count: Option<u64>,
    pub provenance: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectRefKind {
    Branch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub name: String,
    pub kind: ProjectRefKind,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRefsProjection {
    pub project_id: String,
    pub base_commit: String,
    pub head: Option<String>,
    pub refs: Vec<ProjectRef>,
    pub truncated: bool,
    pub provenance: &'static str,
}

struct RawTreeEntry {
    name: String,
    mode: String,
    kind: TreeEntryKind,
    object_id: String,
    size: Option<u64>,
}

/// List the immediate entries of a directory at an exact commit.
pub fn list_project_files(
    project_id: &str,
    repository: &Path,
    expected_identity: &str,
    base_commit: &str,
    requested_ref: Option<&str>,
    requested_path: &str,
) -> Result<ProjectTreeProjection, ProjectFilesError> {
    validate_tree_path(requested_path, true)?;
    let (repository, revision) =
        resolve_revision(repository, expected_identity, base_commit, requested_ref)?;
    let (raw_entries, truncated) = if requested_path.is_empty() {
        list_tree_records(&repository, &revision.tree)?
    } else {
        let entry = exact_entry(&repository, &revision.commit, requested_path)?;
        if entry.kind != TreeEntryKind::Directory {
            return Err(ProjectFilesError::Invalid(
                "path is not a repository directory".into(),
            ));
        }
        list_tree_records(&repository, &entry.object_id)?
    };
    let mut entries = Vec::with_capacity(raw_entries.len());
    let mut skipped_entries = 0usize;
    for entry in raw_entries {
        if !is_safe_entry_name(&entry.name) {
            skipped_entries += 1;
            continue;
        }
        let path = if requested_path.is_empty() {
            entry.name.clone()
        } else {
            format!("{requested_path}/{}", entry.name)
        };
        if path.len() > MAX_PATH_BYTES {
            skipped_entries += 1;
            continue;
        }
        entries.push(ProjectTreeEntry {
            name: entry.name,
            path,
            kind: entry.kind,
            mode: entry.mode,
            object_id: entry.object_id,
            size: entry.size,
        });
    }
    Ok(ProjectTreeProjection {
        project_id: project_id.to_owned(),
        revision,
        path: requested_path.to_owned(),
        entries,
        truncated,
        skipped_entries,
        provenance: PROVENANCE,
    })
}

/// Read one bounded UTF-8 file projection at an exact commit.
pub fn read_project_file(
    project_id: &str,
    repository: &Path,
    expected_identity: &str,
    base_commit: &str,
    requested_ref: Option<&str>,
    requested_path: &str,
) -> Result<ProjectFileProjection, ProjectFilesError> {
    validate_tree_path(requested_path, false)?;
    let (repository, revision) =
        resolve_revision(repository, expected_identity, base_commit, requested_ref)?;
    let entry = exact_entry(&repository, &revision.commit, requested_path)?;
    let name = entry
        .name
        .rsplit('/')
        .next()
        .unwrap_or(&entry.name)
        .to_owned();
    let mut projection = ProjectFileProjection {
        project_id: project_id.to_owned(),
        revision,
        path: requested_path.to_owned(),
        name,
        kind: entry.kind,
        mode: entry.mode,
        object_id: entry.object_id,
        size: entry.size,
        content: None,
        omitted: None,
        line_count: None,
        provenance: PROVENANCE,
    };
    match entry.kind {
        TreeEntryKind::Directory => Err(ProjectFilesError::Invalid(
            "path is a repository directory, not a file".into(),
        )),
        TreeEntryKind::Symlink | TreeEntryKind::Submodule => {
            projection.omitted = Some(ContentOmission::NotAFile);
            Ok(projection)
        }
        TreeEntryKind::File => {
            let size = entry.size.unwrap_or(0);
            if size > MAX_FILE_BYTES {
                projection.omitted = Some(ContentOmission::TooLarge);
                return Ok(projection);
            }
            let blob = run_git(&repository, &["cat-file", "blob", &projection.object_id])?;
            if blob.stdout.len() as u64 != size {
                return Err(ProjectFilesError::Git {
                    command: "git cat-file blob".into(),
                    stderr: "blob length differs from the tree listing".into(),
                });
            }
            if blob.stdout.len() as u64 > MAX_FILE_BYTES {
                projection.omitted = Some(ContentOmission::TooLarge);
                return Ok(projection);
            }
            if blob.stdout.contains(&0) {
                projection.omitted = Some(ContentOmission::Binary);
                return Ok(projection);
            }
            let Ok(content) = String::from_utf8(blob.stdout) else {
                projection.omitted = Some(ContentOmission::Binary);
                return Ok(projection);
            };
            projection.line_count = Some(content.lines().count() as u64);
            projection.content = Some(content);
            Ok(projection)
        }
    }
}

/// List bounded branch refs for the revision selector. Browsing still requires
/// the exact commit object ID returned here.
pub fn list_project_refs(
    project_id: &str,
    repository: &Path,
    expected_identity: &str,
    base_commit: &str,
) -> Result<ProjectRefsProjection, ProjectFilesError> {
    validate_commit_id(base_commit)?;
    let repository = bind_repository(repository, expected_identity)?;
    ensure_commit(&repository, base_commit)?;
    let output = run_git(
        &repository,
        &[
            "for-each-ref",
            "--count=201",
            "--format=%(refname:short)%00%(objectname)%00%(objecttype)",
            "refs/heads",
        ],
    )?;
    let mut refs = Vec::new();
    let mut truncated = false;
    for line in output.stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if refs.len() == MAX_REFS {
            truncated = true;
            break;
        }
        let fields: Vec<&[u8]> = line.split(|byte| *byte == 0).collect();
        if fields.len() != 3 {
            return Err(ProjectFilesError::Invalid(
                "Git branch listing has an unexpected shape".into(),
            ));
        }
        let name = std::str::from_utf8(fields[0])
            .map_err(|_| ProjectFilesError::Invalid("Git branch name is not UTF-8".into()))?;
        let commit = std::str::from_utf8(fields[1])
            .map_err(|_| ProjectFilesError::Invalid("Git branch object is not UTF-8".into()))?;
        let object_type = std::str::from_utf8(fields[2])
            .map_err(|_| ProjectFilesError::Invalid("Git branch type is not UTF-8".into()))?;
        if !is_safe_ref_name(name) || !is_object_id(commit) || object_type != "commit" {
            continue;
        }
        refs.push(ProjectRef {
            name: name.to_owned(),
            kind: ProjectRefKind::Branch,
            commit: commit.to_ascii_lowercase(),
        });
    }
    let head = run_git(&repository, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| is_safe_ref_name(name));
    Ok(ProjectRefsProjection {
        project_id: project_id.to_owned(),
        base_commit: base_commit.to_ascii_lowercase(),
        head,
        refs,
        truncated,
        provenance: PROVENANCE,
    })
}

fn bind_repository(
    repository: &Path,
    expected_identity: &str,
) -> Result<PathBuf, ProjectFilesError> {
    let inspected = repository::inspect(repository, &[]).map_err(map_repository_error)?;
    if expected_identity.trim().is_empty()
        || !inspected
            .identity
            .eq_ignore_ascii_case(expected_identity.trim())
    {
        return Err(ProjectFilesError::Stale);
    }
    Ok(inspected.path)
}

fn resolve_revision(
    repository: &Path,
    expected_identity: &str,
    base_commit: &str,
    requested_ref: Option<&str>,
) -> Result<(PathBuf, ProjectRevision), ProjectFilesError> {
    validate_commit_id(base_commit)?;
    let requested = requested_ref.unwrap_or("").trim();
    let commit = if requested.is_empty() {
        base_commit
    } else {
        validate_commit_id(requested)?;
        requested
    };
    let repository = bind_repository(repository, expected_identity)?;
    ensure_commit(&repository, commit)?;
    let requested_commit = commit;
    let commit = run_git_text(
        &repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{requested_commit}^{{commit}}"),
        ],
    )?;
    if !commit.eq_ignore_ascii_case(requested_commit) {
        return Err(ProjectFilesError::Invalid(
            "ref did not resolve to an exact commit object".into(),
        ));
    }
    let tree = run_git_text(
        &repository,
        &["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
    )?;
    if !is_object_id(&tree) {
        return Err(ProjectFilesError::Invalid(
            "commit did not resolve to a Git tree object".into(),
        ));
    }
    Ok((
        repository,
        ProjectRevision {
            commit: commit.to_ascii_lowercase(),
            tree: tree.to_ascii_lowercase(),
        },
    ))
}

fn ensure_commit(repository: &Path, commit: &str) -> Result<(), ProjectFilesError> {
    if is_object_id(commit)
        && run_git(
            repository,
            &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        )
        .is_ok()
    {
        Ok(())
    } else {
        Err(ProjectFilesError::NotFound)
    }
}

fn list_tree_records(
    repository: &Path,
    tree: &str,
) -> Result<(Vec<RawTreeEntry>, bool), ProjectFilesError> {
    if !is_object_id(tree) {
        return Err(ProjectFilesError::Invalid(
            "tree projection requires an exact Git tree ID".into(),
        ));
    }
    let mut command = hardened_git();
    command
        .arg("-C")
        .arg(repository)
        .args(["--literal-pathspecs", "ls-tree", "-z", "-l", tree])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| ProjectFilesError::Git {
        command: "git ls-tree".into(),
        stderr: "stdout pipe is unavailable".into(),
    })?;
    let mut reader = BufReader::new(stdout);
    let mut record = Vec::new();
    let mut entries = Vec::new();
    let mut truncated = false;
    loop {
        record.clear();
        let read = reader.read_until(0, &mut record)?;
        if read == 0 {
            break;
        }
        if record.last() == Some(&0) {
            record.pop();
        }
        if record.is_empty() {
            continue;
        }
        if entries.len() == MAX_TREE_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(parse_tree_record(&record)?);
    }
    drop(reader);
    if truncated {
        let _ = child.kill();
    }
    let output = child.wait_with_output()?;
    if !truncated && !output.status.success() {
        return Err(ProjectFilesError::Git {
            command: "git ls-tree".into(),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    Ok((entries, truncated))
}

fn exact_entry(
    repository: &Path,
    commit: &str,
    path: &str,
) -> Result<RawTreeEntry, ProjectFilesError> {
    if !is_object_id(commit) {
        return Err(ProjectFilesError::Invalid(
            "entry lookup requires an exact Git commit ID".into(),
        ));
    }
    let output = run_git(
        repository,
        &[
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            "-l",
            commit,
            "--",
            path,
        ],
    )?;
    let records: Vec<&[u8]> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect();
    if records.len() != 1 {
        return Err(if records.is_empty() {
            ProjectFilesError::NotFound
        } else {
            ProjectFilesError::Invalid("path does not identify one repository entry".into())
        });
    }
    let entry = parse_tree_record(records[0])?;
    if entry.name != path {
        return Err(ProjectFilesError::NotFound);
    }
    Ok(entry)
}

fn parse_tree_record(record: &[u8]) -> Result<RawTreeEntry, ProjectFilesError> {
    let tab = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| ProjectFilesError::Invalid("Git tree entry has no path".into()))?;
    let metadata = std::str::from_utf8(&record[..tab])
        .map_err(|_| ProjectFilesError::Invalid("Git tree metadata is not UTF-8".into()))?;
    let name = std::str::from_utf8(&record[tab + 1..])
        .map_err(|_| ProjectFilesError::Invalid("Git tree path is not UTF-8".into()))?
        .to_owned();
    if name.is_empty() || name.contains('\0') {
        return Err(ProjectFilesError::Invalid(
            "Git tree entry has an invalid path".into(),
        ));
    }
    let fields: Vec<&str> = metadata.split_whitespace().collect();
    if fields.len() != 4 {
        return Err(ProjectFilesError::Invalid(
            "Git tree entry has an unexpected shape".into(),
        ));
    }
    let mode = fields[0];
    let object_type = fields[1];
    let object_id = fields[2];
    if !is_object_id(object_id) {
        return Err(ProjectFilesError::Invalid(
            "Git tree entry has a malformed object ID".into(),
        ));
    }
    let kind = match (mode, object_type) {
        ("120000", "blob") => TreeEntryKind::Symlink,
        ("160000", "commit") => TreeEntryKind::Submodule,
        (_, "blob") => TreeEntryKind::File,
        (_, "tree") => TreeEntryKind::Directory,
        _ => {
            return Err(ProjectFilesError::Invalid(format!(
                "Git tree entry has unsupported mode {mode} and type {object_type}"
            )));
        }
    };
    let size = match kind {
        TreeEntryKind::File | TreeEntryKind::Symlink => Some(
            fields[3]
                .parse::<u64>()
                .map_err(|_| ProjectFilesError::Invalid("Git blob size is invalid".into()))?,
        ),
        _ => None,
    };
    Ok(RawTreeEntry {
        name,
        mode: mode.to_owned(),
        kind,
        object_id: object_id.to_ascii_lowercase(),
        size,
    })
}

fn validate_tree_path(path: &str, allow_root: bool) -> Result<(), ProjectFilesError> {
    if path.is_empty() {
        return if allow_root {
            Ok(())
        } else {
            Err(ProjectFilesError::Invalid("a file path is required".into()))
        };
    }
    if path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|component| {
            component.is_empty()
                || component == "."
                || component == ".."
                || component == ".git"
                || component.eq_ignore_ascii_case(".git")
        })
    {
        return Err(ProjectFilesError::Invalid(
            "path is not a safe repository-relative path".into(),
        ));
    }
    Ok(())
}

fn validate_commit_id(value: &str) -> Result<(), ProjectFilesError> {
    if is_object_id(value) {
        Ok(())
    } else {
        Err(ProjectFilesError::Invalid(
            "ref must be a full 40- or 64-character Git commit ID".into(),
        ))
    }
}

fn is_safe_entry_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && name != "."
        && name != ".."
        && !name.eq_ignore_ascii_case(".git")
}

fn is_safe_ref_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.starts_with('-')
        && !name.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains("//")
        && !name.starts_with('/')
        && !name.ends_with('/')
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn run_git(repository: &Path, args: &[&str]) -> Result<Output, ProjectFilesError> {
    let output = hardened_git()
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(ProjectFilesError::Git {
            command: format!("git {}", args.join(" ")),
            stderr: bounded_stderr(&output.stderr),
        });
    }
    Ok(output)
}

fn run_git_text(repository: &Path, args: &[&str]) -> Result<String, ProjectFilesError> {
    let output = run_git(repository, args)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| ProjectFilesError::Invalid("Git returned non-UTF-8 output".into()))
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

fn map_repository_error(error: RepositoryError) -> ProjectFilesError {
    match error {
        RepositoryError::IdentityMismatch => ProjectFilesError::Stale,
        RepositoryError::Invalid(message) => ProjectFilesError::Invalid(message),
        RepositoryError::Git { command, stderr } => ProjectFilesError::Git { command, stderr },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BINARY_FILE: &str = "assets/logo.bin";
    const LARGE_FILE: &str = "assets/large.txt";

    struct TempRepo {
        root: PathBuf,
        path: PathBuf,
        identity: String,
        base_commit: String,
    }

    impl TempRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "factory-project-files-{}-{nonce}",
                std::process::id()
            ));
            let path = root.join("repo");
            fs::create_dir_all(path.join("src")).unwrap();
            fs::create_dir_all(path.join("docs")).unwrap();
            fs::create_dir_all(path.join("assets")).unwrap();
            fs::write(
                path.join("README.md"),
                "# Fixture\n\nA documentation fixture.\n",
            )
            .unwrap();
            fs::write(
                path.join("src/main.rs"),
                "fn main() {\n    println!(\"hello\");\n}\n",
            )
            .unwrap();
            fs::write(path.join("docs/guide.md"), "# Guide\n\n- one\n- two\n").unwrap();
            fs::write(path.join("empty.txt"), "").unwrap();
            fs::write(path.join(BINARY_FILE), b"binary\0payload").unwrap();
            fs::write(path.join("assets/invalid.bin"), b"\xff\xfe\xfd").unwrap();
            fs::write(
                path.join(LARGE_FILE),
                vec![b'x'; MAX_FILE_BYTES as usize + 1],
            )
            .unwrap();
            git(&path, &["init", "-q", "-b", "main"]);
            git(&path, &["config", "user.name", "Fixture"]);
            git(&path, &["config", "user.email", "fixture@example.invalid"]);
            #[cfg(unix)]
            std::os::unix::fs::symlink("../README.md", path.join("docs/link.md")).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file("../README.md", path.join("docs/link.md")).unwrap();
            git(&path, &["add", "--all"]);
            git(&path, &["commit", "-qm", "fixture"]);
            let identity = repository::inspect(&path, &[]).unwrap().identity;
            let base_commit = git(&path, &["rev-parse", "HEAD"]);
            Self {
                root,
                path,
                identity,
                base_commit,
            }
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

    #[test]
    fn lists_root_and_nested_tree_with_exact_revision() {
        let fixture = TempRepo::new();
        let root = list_project_files(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "",
        )
        .unwrap();
        assert_eq!(root.project_id, "project-1");
        assert_eq!(root.revision.commit, fixture.base_commit);
        assert_eq!(root.revision.tree.len(), 40);
        assert!(!root.truncated);
        assert_eq!(root.entries.len(), 5);
        let readme = root
            .entries
            .iter()
            .find(|entry| entry.name == "README.md")
            .unwrap();
        assert_eq!(readme.kind, TreeEntryKind::File);
        assert_eq!(readme.path, "README.md");
        assert!(readme.size.is_some_and(|size| size > 0));
        assert!(
            root.entries
                .iter()
                .any(|entry| entry.name == "src" && entry.kind == TreeEntryKind::Directory)
        );

        let nested = list_project_files(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            Some(&fixture.base_commit),
            "src",
        )
        .unwrap();
        assert_eq!(nested.path, "src");
        assert_eq!(nested.entries.len(), 1);
        assert_eq!(nested.entries[0].path, "src/main.rs");
    }

    #[test]
    fn reads_utf8_file_and_reports_binary_large_symlink_and_directory() {
        let fixture = TempRepo::new();
        let file = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "src/main.rs",
        )
        .unwrap();
        assert_eq!(file.kind, TreeEntryKind::File);
        assert_eq!(
            file.content.as_deref(),
            Some("fn main() {\n    println!(\"hello\");\n}\n")
        );
        assert_eq!(file.line_count, Some(3));
        assert!(file.omitted.is_none());

        let binary = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            BINARY_FILE,
        )
        .unwrap();
        assert_eq!(binary.omitted, Some(ContentOmission::Binary));
        assert!(binary.content.is_none());

        let invalid_utf8 = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "assets/invalid.bin",
        )
        .unwrap();
        assert_eq!(invalid_utf8.omitted, Some(ContentOmission::Binary));
        assert!(invalid_utf8.content.is_none());

        let empty = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "empty.txt",
        )
        .unwrap();
        assert_eq!(empty.content.as_deref(), Some(""));
        assert_eq!(empty.line_count, Some(0));
        assert!(empty.omitted.is_none());

        let large = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            LARGE_FILE,
        )
        .unwrap();
        assert_eq!(large.omitted, Some(ContentOmission::TooLarge));
        assert!(large.content.is_none());

        let symlink = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "docs/link.md",
        )
        .unwrap();
        assert_eq!(symlink.kind, TreeEntryKind::Symlink);
        assert_eq!(symlink.omitted, Some(ContentOmission::NotAFile));
        assert!(symlink.content.is_none());

        let directory = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "src",
        );
        assert!(matches!(directory, Err(ProjectFilesError::Invalid(_))));
    }

    #[test]
    fn rejects_traversal_absolute_and_git_paths() {
        let fixture = TempRepo::new();
        for path in [
            "../etc/passwd",
            "/etc/passwd",
            "src/../../outside",
            ".git/config",
            "src//main.rs",
            "src/",
            "src/./main.rs",
            "src\\main.rs",
            "src/ma\nin.rs",
        ] {
            let result = read_project_file(
                "project-1",
                &fixture.path,
                &fixture.identity,
                &fixture.base_commit,
                None,
                path,
            );
            assert!(
                matches!(result, Err(ProjectFilesError::Invalid(_))),
                "{path}: {result:?}"
            );
        }
        let missing = read_project_file(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "src/missing.rs",
        );
        assert!(matches!(missing, Err(ProjectFilesError::NotFound)));
        let file_as_directory = list_project_files(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
            None,
            "README.md",
        );
        assert!(matches!(
            file_as_directory,
            Err(ProjectFilesError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_unsafe_or_unknown_refs_and_stale_identity() {
        let fixture = TempRepo::new();
        for reference in [
            "main".to_owned(),
            "--help".to_owned(),
            fixture.base_commit[..12].to_owned(),
            "0".repeat(40),
        ] {
            let result = list_project_files(
                "project-1",
                &fixture.path,
                &fixture.identity,
                &fixture.base_commit,
                Some(&reference),
                "",
            );
            assert!(
                matches!(
                    result,
                    Err(ProjectFilesError::Invalid(_)) | Err(ProjectFilesError::NotFound)
                ),
                "{reference}: {result:?}"
            );
        }
        let stale = list_project_files(
            "project-1",
            &fixture.path,
            &"a".repeat(64),
            &fixture.base_commit,
            None,
            "",
        );
        assert!(matches!(stale, Err(ProjectFilesError::Stale)));
    }

    #[test]
    fn refs_are_bounded_and_expose_only_commit_identity() {
        let fixture = TempRepo::new();
        let refs = list_project_refs(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &fixture.base_commit,
        )
        .unwrap();
        assert_eq!(refs.base_commit, fixture.base_commit);
        assert_eq!(refs.head.as_deref(), Some("main"));
        assert_eq!(refs.refs.len(), 1);
        assert_eq!(refs.refs[0].name, "main");
        assert_eq!(refs.refs[0].kind, ProjectRefKind::Branch);
        assert_eq!(refs.refs[0].commit, fixture.base_commit);
        assert!(!refs.truncated);
        let serialized = serde_json::to_string(&refs).unwrap();
        assert!(!serialized.contains(&fixture.root.to_string_lossy().to_string()));
    }

    #[test]
    fn tree_listing_is_bounded_at_max_entries() {
        let fixture = TempRepo::new();
        fs::create_dir_all(fixture.path.join("many")).unwrap();
        for index in 0..MAX_TREE_ENTRIES + 1 {
            fs::write(
                fixture.path.join(format!("many/file-{index:03}.txt")),
                format!("{index}\n"),
            )
            .unwrap();
        }
        git(&fixture.path, &["add", "--all"]);
        git(&fixture.path, &["commit", "-qm", "many"]);
        let commit = git(&fixture.path, &["rev-parse", "HEAD"]);
        let listing = list_project_files(
            "project-1",
            &fixture.path,
            &fixture.identity,
            &commit,
            Some(&commit),
            "many",
        )
        .unwrap();
        assert!(listing.truncated);
        assert_eq!(listing.entries.len(), MAX_TREE_ENTRIES);
    }

    #[test]
    fn tree_parser_skips_unsupported_names() {
        let record =
            |name: &str| format!("100644 blob {}      3\t{name}\0", "a".repeat(40)).into_bytes();
        let mut bytes = record("one.txt");
        bytes.extend(record("with/slash"));
        bytes.extend(record("with\nnewline"));
        bytes.extend(record("two.txt"));
        let mut reader = BufReader::new(bytes.as_slice());
        let mut entries = Vec::new();
        let mut buffer = Vec::new();
        while reader.read_until(0, &mut buffer).unwrap() > 0 {
            if buffer.last() == Some(&0) {
                buffer.pop();
            }
            if buffer.is_empty() {
                continue;
            }
            entries.push(parse_tree_record(&buffer).unwrap());
            buffer.clear();
        }
        assert_eq!(entries.len(), 4);
        assert!(is_safe_entry_name(&entries[0].name));
        assert!(!is_safe_entry_name(&entries[1].name));
        assert!(!is_safe_entry_name(&entries[2].name));
        assert!(is_safe_entry_name(&entries[3].name));
    }
}
