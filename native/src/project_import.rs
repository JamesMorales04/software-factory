//! Controlled Git import into Factory-managed project storage and
//! evidence-based toolchain discovery.
//!
//! Container deployments never accept an arbitrary host path. A Project is
//! imported from an HTTP(S) Git URL into the Factory-owned projects volume and
//! then registered by its managed path. Hooks, submodules, LFS smudging,
//! redirects, proxy environment and terminal prompts are disabled.
//!
//! GitHub imports never accept a user-supplied clone URL or token. The
//! credential is the existing encrypted Core connection, the URL is derived
//! from validated owner/repository coordinates, and the token reaches `git`
//! only through an ephemeral environment configuration, never argv or logs.
//!
//! Discovery reads bounded manifests from the exact recorded commit. It is
//! advisory evidence; it never grants a Capability or authorizes a check.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{process::Command, time};

use crate::lead::{Capability, CheckKind, SpecCheck, validate_mixed_checks};

const MAX_URL_BYTES: usize = 2048;
const CLONE_TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_PROJECT_NAME_CHARS: usize = 200;
const MAX_MANIFESTS: usize = 120;
const MAX_TREE_BYTES: usize = 8 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_TREE_ENTRIES: usize = 100_000;
const MAX_DISCOVERY_CHECK_TIMEOUT_SECS: u32 = 300;
const PROVENANCE: &str = "git-tree-manifest-scan-v1";

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("project import input is invalid: {0}")]
    Invalid(String),
    #[error("repository import failed: {0}")]
    Git(String),
    #[error("repository import I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("repository discovery failed: {0}")]
    Discovery(String),
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

/// Resolve an explicit local import only inside the operator-approved root.
/// Canonicalization rejects `..`, symlink targets and any other path that
/// escapes the root. A missing root fails closed: no arbitrary host path.
pub fn approved_local_path(root: Option<&Path>, requested: &str) -> Result<PathBuf, ImportError> {
    let root = root.ok_or_else(|| {
        ImportError::Invalid(
            "local import requires an operator-approved FACTORY_PROJECTS_ROOT".into(),
        )
    })?;
    if requested.trim() != requested
        || requested.is_empty()
        || requested.len() > MAX_URL_BYTES
        || requested.chars().any(char::is_control)
    {
        return Err(ImportError::Invalid(
            "local repository path has an unsafe shape".into(),
        ));
    }
    let requested = PathBuf::from(requested);
    if !requested.is_absolute() {
        return Err(ImportError::Invalid(
            "local repository path must be absolute inside the managed root".into(),
        ));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        ImportError::Invalid(format!("managed projects root is unavailable: {error}"))
    })?;
    if !canonical_root.is_dir() {
        return Err(ImportError::Invalid(
            "managed projects root is not a directory".into(),
        ));
    }
    let canonical = requested.canonicalize().map_err(|error| {
        ImportError::Invalid(format!("local repository path is unavailable: {error}"))
    })?;
    if !canonical.starts_with(&canonical_root) {
        return Err(ImportError::Invalid(
            "local repository path is outside the managed projects root".into(),
        ));
    }
    Ok(canonical)
}

/// Validate a local repository path after Git resolves its actual top level.
/// `approved_local_path` checks the requested directory; Git can still resolve
/// a parent worktree outside the approved root if that directory is nested in
/// one. Registration must revalidate the resolved repository root.
pub fn approved_local_repository(
    root: Option<&Path>,
    requested: &str,
) -> Result<PathBuf, ImportError> {
    let canonical = approved_local_path(root, requested)?;
    let root = root.ok_or_else(|| {
        ImportError::Invalid(
            "local import requires an operator-approved FACTORY_PROJECTS_ROOT".into(),
        )
    })?;
    let canonical_root = root.canonicalize().map_err(|error| {
        ImportError::Invalid(format!("managed projects root is unavailable: {error}"))
    })?;
    let path = canonical
        .to_str()
        .ok_or_else(|| ImportError::Invalid("local repository path encoding".into()))?;
    let output = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--show-toplevel"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|error| {
            ImportError::Invalid(format!("Git top-level resolution failed: {error}"))
        })?;
    if !output.status.success() {
        return Err(ImportError::Invalid(
            "path is not inside a Git worktree".into(),
        ));
    }
    let top = String::from_utf8(output.stdout)
        .map_err(|_| ImportError::Invalid("Git top-level output encoding".into()))?;
    let top = top
        .strip_suffix('\n')
        .ok_or_else(|| ImportError::Invalid("Git top-level output terminator".into()))?;
    let top = PathBuf::from(top).canonicalize().map_err(|error| {
        ImportError::Invalid(format!("Git top-level path is unavailable: {error}"))
    })?;
    if !top.starts_with(&canonical_root) {
        return Err(ImportError::Invalid(
            "the Git worktree root is outside the managed projects root".into(),
        ));
    }
    Ok(canonical)
}

pub async fn import(
    url: &str,
    root: &Path,
    allow_insecure_http: bool,
) -> Result<PathBuf, ImportError> {
    validate_url(url, allow_insecure_http)?;
    import_url(url, root, allow_insecure_http, None).await
}

/// Import a GitHub repository through the existing encrypted connection. The
/// caller supplies the already-decrypted short-lived token; Core derives the
/// clone URL from coordinates and never accepts a URL from Office.
pub async fn import_github(
    owner: &str,
    repo: &str,
    token: &str,
    root: &Path,
) -> Result<PathBuf, ImportError> {
    let url = github_clone_url(owner, repo)?;
    if token.len() < 16 || token.len() > 512 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ImportError::Invalid(
            "GitHub credential has an unsafe shape".into(),
        ));
    }
    import_url(&url, root, false, Some(token)).await
}

pub fn github_clone_url(owner: &str, repo: &str) -> Result<String, ImportError> {
    crate::github::validate_repo_coordinates(owner, repo)
        .map_err(|_| ImportError::Invalid("GitHub owner or repository is invalid".into()))?;
    Ok(format!("https://github.com/{owner}/{repo}.git"))
}

async fn import_url(
    url: &str,
    root: &Path,
    allow_insecure_http: bool,
    token: Option<&str>,
) -> Result<PathBuf, ImportError> {
    if !root.is_absolute() {
        return Err(ImportError::Invalid(
            "managed project root must be absolute".into(),
        ));
    }
    std::fs::create_dir_all(root)?;
    let canonical_root = root.canonicalize()?;
    let digest = Sha256::digest(url.as_bytes());
    let slug: String = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let destination = canonical_root.join(format!("project-{slug}"));
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
    let temporary = canonical_root.join(format!(".import-{}", uuid::Uuid::new_v4()));
    let result = clone(url, &temporary, allow_insecure_http, token).await;
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

struct GitCommandSpec {
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl GitCommandSpec {
    fn build(
        url: &str,
        destination: &Path,
        allow_insecure_http: bool,
        token: Option<&str>,
    ) -> Self {
        let mut args = vec![
            "-c".into(),
            "core.hooksPath=/dev/null".into(),
            "-c".into(),
            "protocol.file.allow=never".into(),
            "-c".into(),
            "protocol.ext.allow=never".into(),
            "clone".into(),
            "--quiet".into(),
            "--no-hardlinks".into(),
            "--no-recurse-submodules".into(),
            "--".into(),
            url.into(),
            destination.to_string_lossy().into_owned(),
        ];
        let mut config = vec![
            ("http.followRedirects".to_owned(), "false".to_owned()),
            ("credential.helper".to_owned(), String::new()),
            ("http.proxy".to_owned(), String::new()),
        ];
        if let Some(token) = token {
            config.push((
                "http.extraHeader".to_owned(),
                format!(
                    "Authorization: Basic {}",
                    base64_standard(format!("x-access-token:{token}").as_bytes())
                ),
            ));
        }
        let mut env = vec![
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_LFS_SKIP_SMUDGE".into(), "1".into()),
            (
                "GIT_ALLOW_PROTOCOL".into(),
                if allow_insecure_http {
                    "https:http".into()
                } else {
                    "https".into()
                },
            ),
            ("NO_PROXY".into(), "*".into()),
            ("no_proxy".into(), "*".into()),
            ("HTTP_PROXY".into(), String::new()),
            ("HTTPS_PROXY".into(), String::new()),
            ("ALL_PROXY".into(), String::new()),
            ("http_proxy".into(), String::new()),
            ("https_proxy".into(), String::new()),
            ("all_proxy".into(), String::new()),
            ("GIT_CONFIG_COUNT".into(), config.len().to_string()),
        ];
        for (index, (key, value)) in config.iter().enumerate() {
            env.push((format!("GIT_CONFIG_KEY_{index}"), key.clone()));
            env.push((format!("GIT_CONFIG_VALUE_{index}"), value.clone()));
        }
        args.shrink_to_fit();
        Self { args, env }
    }
}

async fn clone(
    url: &str,
    destination: &Path,
    allow_insecure_http: bool,
    token: Option<&str>,
) -> Result<(), ImportError> {
    let spec = GitCommandSpec::build(url, destination, allow_insecure_http, token);
    let mut command = Command::new("git");
    command
        .args(&spec.args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in spec.env {
        command.env(key, value);
    }
    let output = time::timeout(CLONE_TIMEOUT, command.output())
        .await
        .map_err(|_| ImportError::Git("clone timed out".into()))?
        .map_err(ImportError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut detail: String = stderr.chars().take(512).collect();
        if let Some(token) = token {
            detail = detail.replace(token, "<redacted>");
        }
        return Err(ImportError::Git(
            detail
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_owned(),
        ));
    }
    Ok(())
}

fn base64_standard(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(ALPHABET[(first >> 2) as usize] as char);
        encoded.push(ALPHABET[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(ALPHABET[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(ALPHABET[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredToolchain {
    pub id: String,
    pub label: String,
    pub manifests: Vec<String>,
    pub evidence: Vec<String>,
    pub suggested_capability: Option<Capability>,
    pub suggested_checks: Vec<SpecCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiscovery {
    pub base_commit: String,
    pub toolchains: Vec<DiscoveredToolchain>,
    pub suggested_capabilities: Vec<Capability>,
    pub suggested_checks: Vec<SpecCheck>,
    pub scanned_manifests: Vec<String>,
    pub truncated: bool,
    pub skipped_entries: usize,
    pub provenance: String,
    pub scanned_at: String,
}

/// Scan bounded known manifests at `base_commit` from the Git object store.
/// Working-tree changes, host paths and symlink targets never contribute.
pub fn discover(repository: &Path, base_commit: &str) -> Result<ProjectDiscovery, ImportError> {
    if !is_commit_id(base_commit) {
        return Err(ImportError::Discovery(
            "discovery base must be an exact commit id".into(),
        ));
    }
    let tree = git_tree(repository, base_commit)?;
    let mut manifests = Vec::new();
    for path in &tree.paths {
        if manifests.len() >= MAX_MANIFESTS {
            break;
        }
        if is_manifest_path(path) {
            manifests.push(path.clone());
        }
    }
    manifests.sort();
    manifests.dedup();
    let mut contents: BTreeMap<String, String> = BTreeMap::new();
    let mut skipped_entries = tree.skipped_entries;
    for path in &manifests {
        match git_blob(repository, base_commit, path) {
            Ok(content) => {
                contents.insert(path.clone(), content);
            }
            Err(_) => skipped_entries += 1,
        }
    }
    let has = |name: &str| first_manifest(&contents, name).is_some();
    let has_suffix = |suffix: &str| {
        contents
            .keys()
            .any(|path| path.to_lowercase().ends_with(&suffix.to_lowercase()))
    };
    let path_names = tree
        .paths
        .iter()
        .map(|path| path.to_lowercase())
        .collect::<Vec<_>>();

    let mut toolchains = Vec::new();
    if has("Cargo.toml") || has_suffix("rust-toolchain.toml") || has_suffix("rust-toolchain") {
        toolchains.push(rust_toolchain(&contents));
    }
    if has("package.json") {
        toolchains.push(node_toolchain(
            &contents,
            has("pnpm-lock.yaml"),
            has("yarn.lock"),
            has("bun.lockb") || has("bun.lock"),
            path_names
                .iter()
                .any(|path| path.contains("playwright.config")),
            path_names
                .iter()
                .any(|path| path.contains("cypress.config")),
        ));
    }
    if has("go.mod") {
        toolchains.push(go_toolchain(&contents));
    }
    if has("pyproject.toml")
        || has("setup.py")
        || has("setup.cfg")
        || has("requirements.txt")
        || has("Pipfile")
    {
        toolchains.push(python_toolchain(&contents));
    }
    if has_suffix(".csproj") || has_suffix(".fsproj") || has_suffix(".vbproj") || has_suffix(".sln")
    {
        toolchains.push(dotnet_toolchain(&contents));
    }
    if has("pom.xml") {
        toolchains.push(java_toolchain("maven", &contents));
    }
    if has("build.gradle") || has("build.gradle.kts") {
        toolchains.push(java_toolchain("gradle", &contents));
    }
    if has("composer.json") {
        toolchains.push(manifest_toolchain(
            "php",
            "PHP (Composer)",
            &contents,
            &["composer.json"],
            Some(Capability::GenericToolchain),
            vec![check(
                "php-composer-validate",
                CheckKind::Build,
                "Validate composer.json without publishing metadata",
                &["composer", "validate", "--no-check-publish"],
            )],
        ));
    }
    if has("Gemfile") {
        toolchains.push(ruby_toolchain(&contents));
    }
    if has("mix.exs") {
        toolchains.push(manifest_toolchain(
            "elixir",
            "Elixir (Mix)",
            &contents,
            &["mix.exs"],
            Some(Capability::GenericToolchain),
            vec![check(
                "elixir-mix-test",
                CheckKind::Test,
                "Run the Mix test suite",
                &["mix", "test"],
            )],
        ));
    }
    if has("pubspec.yaml") {
        let flutter = first_manifest(&contents, "pubspec.yaml")
            .is_some_and(|value| value.to_lowercase().contains("flutter"));
        toolchains.push(manifest_toolchain(
            if flutter { "flutter" } else { "dart" },
            if flutter {
                "Dart (Flutter)"
            } else {
                "Dart (pub)"
            },
            &contents,
            &["pubspec.yaml"],
            Some(Capability::GenericToolchain),
            vec![check(
                if flutter { "flutter-test" } else { "dart-test" },
                CheckKind::Test,
                "Run the Dart test suite",
                if flutter {
                    &["flutter", "test"]
                } else {
                    &["dart", "test"]
                },
            )],
        ));
    }
    if has("Package.swift") {
        toolchains.push(manifest_toolchain(
            "swift",
            "Swift Package Manager",
            &contents,
            &["Package.swift"],
            Some(Capability::GenericToolchain),
            vec![check(
                "swift-test",
                CheckKind::Test,
                "Run the Swift package tests",
                &["swift", "test"],
            )],
        ));
    }
    if has("CMakeLists.txt") {
        toolchains.push(manifest_toolchain(
            "cmake",
            "CMake",
            &contents,
            &["CMakeLists.txt"],
            Some(Capability::GenericToolchain),
            Vec::new(),
        ));
    }
    if has("Dockerfile") || has("compose.yaml") || has("compose.yml") || has("docker-compose.yml") {
        toolchains.push(container_toolchain(&contents));
    }
    let browser_evidence = first_manifest(&contents, "package.json")
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .map(package_browser_dependency)
        .unwrap_or(false)
        || path_names
            .iter()
            .any(|path| path.contains("playwright.config") || path.contains("cypress.config"));
    if browser_evidence && !toolchains.iter().any(|toolchain| toolchain.id == "browser") {
        toolchains.push(DiscoveredToolchain {
            id: "browser".into(),
            label: "Browser automation".into(),
            manifests: browser_manifest_paths(&contents),
            evidence: vec!["browser test configuration or dependency detected".into()],
            suggested_capability: Some(Capability::Browser),
            suggested_checks: browser_checks(&contents),
        });
    }

    let mut capabilities = BTreeSet::new();
    capabilities.insert(Capability::RepositoryEdit);
    for toolchain in &toolchains {
        if let Some(capability) = toolchain.suggested_capability {
            capabilities.insert(capability);
        }
    }
    let suggested_capabilities = capabilities.into_iter().collect::<Vec<_>>();
    let mut suggested_checks = Vec::new();
    for toolchain in &toolchains {
        for check in &toolchain.suggested_checks {
            if suggested_checks.len() >= 32 {
                break;
            }
            if suggested_checks
                .iter()
                .any(|existing: &SpecCheck| existing.id == check.id)
            {
                continue;
            }
            suggested_checks.push(check.clone());
        }
    }
    validate_mixed_checks(&suggested_checks)
        .map_err(|error| ImportError::Discovery(error.to_string()))?;

    Ok(ProjectDiscovery {
        base_commit: base_commit.to_owned(),
        toolchains,
        suggested_capabilities,
        suggested_checks,
        scanned_manifests: contents.keys().cloned().collect(),
        truncated: tree.truncated,
        skipped_entries,
        provenance: PROVENANCE.into(),
        scanned_at: Utc::now().to_rfc3339(),
    })
}

struct TreeScan {
    paths: Vec<String>,
    truncated: bool,
    skipped_entries: usize,
}

fn git_tree(repository: &Path, base_commit: &str) -> Result<TreeScan, ImportError> {
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .args([
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            "--full-tree",
            base_commit,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    let mut child = command
        .spawn()
        .map_err(|error| ImportError::Discovery(format!("git ls-tree could not start: {error}")))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| ImportError::Discovery("git ls-tree stdout was unavailable".into()))?;
    let mut buffer = Vec::new();
    stdout
        .by_ref()
        .take((MAX_TREE_BYTES + 1) as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| {
            ImportError::Discovery(format!("git ls-tree could not be read: {error}"))
        })?;
    drop(stdout);
    let mut truncated = buffer.len() > MAX_TREE_BYTES;
    if truncated {
        let _ = child.kill();
        let _ = child.wait();
        buffer.truncate(MAX_TREE_BYTES);
    } else {
        let status = child.wait().map_err(|error| {
            ImportError::Discovery(format!("git ls-tree could not finish: {error}"))
        })?;
        if !status.success() {
            return Err(ImportError::Discovery(
                "git ls-tree could not read the recorded commit".into(),
            ));
        }
    }
    if let Some(last) = buffer.iter().rposition(|byte| *byte == 0) {
        buffer.truncate(last + 1);
    } else {
        buffer.clear();
    }
    let mut paths = Vec::new();
    let mut skipped_entries = 0usize;
    for raw in buffer.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        if paths.len() >= MAX_TREE_ENTRIES {
            truncated = true;
            skipped_entries += 1;
            continue;
        }
        let Ok(path) = std::str::from_utf8(raw) else {
            skipped_entries += 1;
            continue;
        };
        if path.is_empty() || path.len() > 4_096 || path.chars().any(char::is_control) {
            skipped_entries += 1;
            continue;
        }
        paths.push(path.to_owned());
    }
    Ok(TreeScan {
        paths,
        truncated,
        skipped_entries,
    })
}

fn git_blob(repository: &Path, base_commit: &str, path: &str) -> Result<String, ImportError> {
    if path.contains('\0') || path.contains('\n') || path.starts_with('-') {
        return Err(ImportError::Discovery(
            "manifest path has an unsafe shape".into(),
        ));
    }
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(repository)
        .arg("show")
        .arg(format!("{base_commit}:{path}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    let mut child = command
        .spawn()
        .map_err(|error| ImportError::Discovery(format!("git show could not start: {error}")))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| ImportError::Discovery("git show stdout was unavailable".into()))?;
    let mut buffer = Vec::new();
    stdout
        .by_ref()
        .take((MAX_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut buffer)
        .map_err(|error| ImportError::Discovery(format!("git show could not be read: {error}")))?;
    drop(stdout);
    let oversized = buffer.len() > MAX_MANIFEST_BYTES;
    if oversized {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ImportError::Discovery(
            "manifest exceeds the bounded discovery size".into(),
        ));
    }
    let status = child
        .wait()
        .map_err(|error| ImportError::Discovery(format!("git show could not finish: {error}")))?;
    if !status.success() {
        return Err(ImportError::Discovery(
            "manifest could not be read from the recorded commit".into(),
        ));
    }
    String::from_utf8(buffer)
        .map_err(|_| ImportError::Discovery("manifest is not valid UTF-8 text".into()))
}

fn is_manifest_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lockb"
            | "bun.lock"
            | "go.mod"
            | "go.sum"
            | "pyproject.toml"
            | "setup.py"
            | "setup.cfg"
            | "requirements.txt"
            | "pipfile"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "composer.json"
            | "gemfile"
            | "gemfile.lock"
            | "mix.exs"
            | "pubspec.yaml"
            | "package.swift"
            | "cmakelists.txt"
            | "dockerfile"
            | "compose.yaml"
            | "compose.yml"
            | "docker-compose.yml"
            | "rust-toolchain.toml"
            | "rust-toolchain"
            | "global.json"
            | "playwright.config.ts"
            | "playwright.config.js"
            | "playwright.config.mjs"
            | "cypress.config.ts"
            | "cypress.config.js"
    ) || lower.ends_with(".csproj")
        || lower.ends_with(".fsproj")
        || lower.ends_with(".vbproj")
        || lower.ends_with(".sln")
}

fn check(id: &str, kind: CheckKind, description: &str, command: &[&str]) -> SpecCheck {
    SpecCheck {
        id: id.into(),
        kind,
        description: description.into(),
        command: command.iter().map(|value| (*value).to_owned()).collect(),
        expect_exit: 0,
        timeout_secs: MAX_DISCOVERY_CHECK_TIMEOUT_SECS,
    }
}

fn first_manifest<'a>(contents: &'a BTreeMap<String, String>, name: &str) -> Option<&'a String> {
    contents
        .iter()
        .find(|(path, _)| {
            path.rsplit('/')
                .next()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        })
        .map(|(_, value)| value)
}

fn manifest_paths(contents: &BTreeMap<String, String>, names: &[&str]) -> Vec<String> {
    contents
        .keys()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            names
                .iter()
                .any(|candidate| name.eq_ignore_ascii_case(candidate))
                || names.iter().any(|candidate| {
                    candidate.starts_with('.') && name.to_lowercase().ends_with(candidate)
                })
        })
        .cloned()
        .collect()
}

fn manifest_toolchain(
    id: &str,
    label: &str,
    contents: &BTreeMap<String, String>,
    manifest_names: &[&str],
    capability: Option<Capability>,
    checks: Vec<SpecCheck>,
) -> DiscoveredToolchain {
    DiscoveredToolchain {
        id: id.into(),
        label: label.into(),
        manifests: manifest_paths(contents, manifest_names),
        evidence: vec![format!("known {label} manifest detected")],
        suggested_capability: capability,
        suggested_checks: checks,
    }
}

fn rust_toolchain(contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    let manifests = manifest_paths(
        contents,
        &["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"],
    );
    let mut evidence = vec!["Cargo manifest detected".into()];
    if first_manifest(contents, "Cargo.toml").is_some_and(|value| value.contains("[workspace]")) {
        evidence.push("Cargo workspace declared".into());
    }
    DiscoveredToolchain {
        id: "rust".into(),
        label: "Rust (Cargo)".into(),
        manifests,
        evidence,
        suggested_capability: Some(Capability::GenericToolchain),
        suggested_checks: vec![
            check(
                "rust-format",
                CheckKind::Format,
                "Check Rust formatting without rewriting files",
                &["cargo", "fmt", "--all", "--", "--check"],
            ),
            check(
                "rust-clippy",
                CheckKind::Lint,
                "Run Clippy with warnings denied",
                &["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
            ),
            check(
                "rust-test",
                CheckKind::Test,
                "Run the Cargo test suite",
                &["cargo", "test", "--all-targets"],
            ),
        ],
    }
}

fn node_toolchain(
    contents: &BTreeMap<String, String>,
    pnpm: bool,
    yarn: bool,
    bun: bool,
    playwright_config: bool,
    cypress_config: bool,
) -> DiscoveredToolchain {
    let package = first_manifest(contents, "package.json")
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
    let (manager, run) = if pnpm {
        ("pnpm", vec!["pnpm", "run"])
    } else if yarn {
        ("yarn", vec!["yarn", "run"])
    } else if bun {
        ("bun", vec!["bun", "run"])
    } else {
        ("npm", vec!["npm", "run"])
    };
    let scripts = package
        .as_ref()
        .and_then(|value| value.get("scripts"))
        .and_then(serde_json::Value::as_object);
    let mut checks = Vec::new();
    if scripts.is_some_and(|value| value.contains_key("format:check")) {
        checks.push(check(
            "node-format",
            CheckKind::Format,
            "Run the repository's non-mutating format check script",
            &[run[0], run[1], "format:check"],
        ));
    }
    if scripts.is_some_and(|value| value.contains_key("lint")) {
        checks.push(check(
            "node-lint",
            CheckKind::Lint,
            "Run the repository's lint script",
            &[run[0], run[1], "lint"],
        ));
    }
    if scripts.is_some_and(|value| value.contains_key("test")) {
        checks.push(check(
            "node-test",
            CheckKind::Test,
            "Run the repository's test script",
            &[run[0], run[1], "test"],
        ));
    }
    if scripts.is_some_and(|value| value.contains_key("build")) {
        checks.push(check(
            "node-build",
            CheckKind::Build,
            "Run the repository's build script",
            &[run[0], run[1], "build"],
        ));
    }
    let manifests = manifest_paths(
        contents,
        &[
            "package.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "bun.lockb",
            "bun.lock",
        ],
    );
    let mut evidence = vec![format!("package.json detected; package manager {manager}")];
    if playwright_config {
        evidence.push("Playwright configuration detected".into());
    }
    if cypress_config {
        evidence.push("Cypress configuration detected".into());
    }
    DiscoveredToolchain {
        id: "node".into(),
        label: "Node.js".into(),
        manifests,
        evidence,
        suggested_capability: Some(Capability::NodeToolchain),
        suggested_checks: checks,
    }
}

fn go_toolchain(contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    DiscoveredToolchain {
        id: "go".into(),
        label: "Go modules".into(),
        manifests: manifest_paths(contents, &["go.mod", "go.sum"]),
        evidence: vec!["go.mod detected".into()],
        suggested_capability: Some(Capability::GenericToolchain),
        suggested_checks: vec![
            check(
                "go-vet",
                CheckKind::Lint,
                "Run Go static analysis on all packages",
                &["go", "vet", "./..."],
            ),
            check(
                "go-test",
                CheckKind::Test,
                "Run the project's Go unit tests",
                &["go", "test", "./..."],
            ),
            check(
                "go-build",
                CheckKind::Build,
                "Build the discovered Go packages",
                &["go", "build", "./..."],
            ),
        ],
    }
}

fn python_toolchain(contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    let mut evidence = Vec::new();
    let mut checks = Vec::new();
    let manifests = manifest_paths(
        contents,
        &[
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
            "Pipfile",
        ],
    );
    let lower = contents
        .values()
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    if lower.contains("pytest") {
        evidence.push("pytest configuration or dependency detected".into());
        checks.push(check(
            "python-pytest",
            CheckKind::Test,
            "Run the pytest suite quietly",
            &["python", "-m", "pytest", "-q"],
        ));
    }
    if lower.contains("ruff") {
        evidence.push("Ruff configuration or dependency detected".into());
        checks.push(check(
            "python-ruff",
            CheckKind::Lint,
            "Run Ruff checks",
            &["ruff", "check", "."],
        ));
    }
    if lower.contains("black") {
        evidence.push("Black configuration or dependency detected".into());
        checks.push(check(
            "python-black",
            CheckKind::Format,
            "Check Black formatting without rewriting files",
            &["black", "--check", "."],
        ));
    }
    if lower.contains("mypy") {
        evidence.push("mypy configuration or dependency detected".into());
        checks.push(check(
            "python-mypy",
            CheckKind::Lint,
            "Run the mypy type checks",
            &["mypy", "."],
        ));
    }
    if evidence.is_empty() {
        evidence.push("Python manifest detected; no single safe test command inferred".into());
    }
    DiscoveredToolchain {
        id: "python".into(),
        label: "Python".into(),
        manifests,
        evidence,
        suggested_capability: Some(Capability::GenericToolchain),
        suggested_checks: checks,
    }
}

fn dotnet_toolchain(contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    DiscoveredToolchain {
        id: "dotnet".into(),
        label: ".NET".into(),
        manifests: contents
            .keys()
            .filter(|path| {
                let lower = path.to_lowercase();
                lower.ends_with(".csproj")
                    || lower.ends_with(".fsproj")
                    || lower.ends_with(".vbproj")
                    || lower.ends_with(".sln")
                    || lower.ends_with("global.json")
            })
            .cloned()
            .collect(),
        evidence: vec![".NET project or solution manifest detected".into()],
        suggested_capability: Some(Capability::GenericToolchain),
        suggested_checks: vec![
            check(
                "dotnet-build",
                CheckKind::Build,
                "Build the .NET solution or project",
                &["dotnet", "build"],
            ),
            check(
                "dotnet-test",
                CheckKind::Test,
                "Run the .NET test projects",
                &["dotnet", "test"],
            ),
        ],
    }
}

fn java_toolchain(kind: &str, contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    let (id, label, manifests, checks) = if kind == "maven" {
        (
            "maven",
            "Java (Maven)",
            manifest_paths(contents, &["pom.xml"]),
            vec![
                check(
                    "maven-test",
                    CheckKind::Test,
                    "Run the Maven test phase",
                    &["mvn", "-q", "test"],
                ),
                check(
                    "maven-package",
                    CheckKind::Build,
                    "Package without running tests",
                    &["mvn", "-q", "-DskipTests", "package"],
                ),
            ],
        )
    } else {
        (
            "gradle",
            "JVM (Gradle)",
            manifest_paths(contents, &["build.gradle", "build.gradle.kts"]),
            vec![
                check(
                    "gradle-test",
                    CheckKind::Test,
                    "Run the Gradle test task",
                    &["./gradlew", "test"],
                ),
                check(
                    "gradle-build",
                    CheckKind::Build,
                    "Run the Gradle build task",
                    &["./gradlew", "build"],
                ),
            ],
        )
    };
    DiscoveredToolchain {
        id: id.into(),
        label: label.into(),
        manifests,
        evidence: vec![format!("{label} build manifest detected")],
        suggested_capability: Some(Capability::GenericToolchain),
        suggested_checks: checks,
    }
}

fn ruby_toolchain(contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    let mut checks = Vec::new();
    let mut evidence = vec!["Gemfile detected".into()];
    let gemfile = first_manifest(contents, "Gemfile")
        .cloned()
        .unwrap_or_default();
    if gemfile.to_lowercase().contains("rspec") {
        evidence.push("RSpec dependency detected".into());
        checks.push(check(
            "ruby-rspec",
            CheckKind::Test,
            "Run the RSpec suite",
            &["bundle", "exec", "rspec"],
        ));
    }
    if contents.keys().any(|path| path.ends_with("Rakefile")) {
        evidence.push("Rakefile detected".into());
        checks.push(check(
            "ruby-rake-test",
            CheckKind::Test,
            "Run the default Rake test task",
            &["bundle", "exec", "rake", "test"],
        ));
    }
    DiscoveredToolchain {
        id: "ruby".into(),
        label: "Ruby (Bundler)".into(),
        manifests: manifest_paths(contents, &["Gemfile", "Gemfile.lock"]),
        evidence,
        suggested_capability: Some(Capability::GenericToolchain),
        suggested_checks: checks,
    }
}

fn container_toolchain(contents: &BTreeMap<String, String>) -> DiscoveredToolchain {
    DiscoveredToolchain {
        id: "container".into(),
        label: "Container build".into(),
        manifests: manifest_paths(
            contents,
            &[
                "Dockerfile",
                "compose.yaml",
                "compose.yml",
                "docker-compose.yml",
            ],
        ),
        evidence: vec!["container manifest detected; no build authority is inferred".into()],
        suggested_capability: None,
        suggested_checks: Vec::new(),
    }
}

fn browser_manifest_paths(contents: &BTreeMap<String, String>) -> Vec<String> {
    contents
        .keys()
        .filter(|path| {
            let lower = path.to_lowercase();
            lower.contains("playwright.config") || lower.contains("cypress.config")
        })
        .cloned()
        .collect()
}

fn package_browser_dependency(package: serde_json::Value) -> bool {
    let mut names = Vec::new();
    for key in ["dependencies", "devDependencies"] {
        if let Some(values) = package.get(key).and_then(serde_json::Value::as_object) {
            names.extend(values.keys().cloned());
        }
    }
    names.iter().any(|name| {
        matches!(
            name.as_str(),
            "@playwright/test" | "playwright" | "cypress" | "puppeteer"
        )
    })
}

fn browser_checks(contents: &BTreeMap<String, String>) -> Vec<SpecCheck> {
    let has_playwright = contents
        .keys()
        .any(|path| path.to_lowercase().contains("playwright.config"))
        || first_manifest(contents, "package.json")
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .is_some_and(|package| {
                ["dependencies", "devDependencies"].iter().any(|key| {
                    package
                        .get(key)
                        .and_then(serde_json::Value::as_object)
                        .is_some_and(|values| {
                            values.contains_key("@playwright/test")
                                || values.contains_key("playwright")
                        })
                })
            });
    if has_playwright {
        vec![check(
            "browser-playwright",
            CheckKind::Browser,
            "Run the Playwright browser suite after confirming the worker image",
            &["npx", "--no-install", "playwright", "test"],
        )]
    } else {
        vec![check(
            "browser-cypress",
            CheckKind::Browser,
            "Run the Cypress browser suite after confirming the worker image",
            &["npx", "--no-install", "cypress", "run"],
        )]
    }
}

fn is_commit_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

    #[test]
    fn derives_only_github_clone_urls_from_validated_coordinates() {
        assert_eq!(
            github_clone_url("acme", "widgets").unwrap(),
            "https://github.com/acme/widgets.git"
        );
        assert!(github_clone_url("../acme", "widgets").is_err());
        assert!(github_clone_url("acme", "widgets.git").is_err());
        assert!(github_clone_url("acme", "widgets/extra").is_err());
    }

    #[test]
    fn github_clone_command_never_puts_the_token_in_argv() {
        let spec = GitCommandSpec::build(
            "https://github.com/acme/private-widgets.git",
            Path::new("/projects/.import-test"),
            false,
            Some("ghp_test_token_1234567890"),
        );
        assert!(!spec.args.iter().any(|arg| arg.contains("ghp_test")));
        assert!(!spec.args.iter().any(|arg| arg.contains("Authorization")));
        assert_eq!(
            spec.env
                .iter()
                .find(|(key, _)| key == "GIT_CONFIG_KEY_3")
                .map(|(_, value)| value.as_str()),
            Some("http.extraHeader")
        );
        let header = spec
            .env
            .iter()
            .find(|(key, _)| key == "GIT_CONFIG_VALUE_3")
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert!(header.starts_with("Authorization: Basic "));
        assert!(!header.contains("ghp_test_token_1234567890"));
        assert!(
            spec.args
                .iter()
                .any(|arg| arg == "https://github.com/acme/private-widgets.git")
        );
        assert!(
            spec.env.iter().any(|(key, value)| {
                key == "GIT_CONFIG_KEY_0" && value == "http.followRedirects"
            })
        );
        assert!(
            spec.env
                .iter()
                .any(|(key, value)| { key == "GIT_CONFIG_VALUE_0" && value == "false" })
        );
    }

    #[test]
    fn rejects_a_managed_root_nested_in_a_parent_git_worktree() {
        let parent = fixture_root("nested-worktree");
        git(&parent, &["init", "-q"]);
        let approved = parent.join("managed");
        std::fs::create_dir_all(&approved).unwrap();
        let requested = approved.join("checkout");
        std::fs::create_dir_all(&requested).unwrap();
        let error = approved_local_repository(Some(&approved), requested.to_str().unwrap())
            .expect_err("a parent worktree outside the approved root must be rejected");
        assert!(
            error
                .to_string()
                .contains("the Git worktree root is outside the managed projects root")
        );
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_standard(b""), "");
        assert_eq!(base64_standard(b"f"), "Zg==");
        assert_eq!(base64_standard(b"fo"), "Zm8=");
        assert_eq!(base64_standard(b"foo"), "Zm9v");
        assert_eq!(base64_standard(b"foobar"), "Zm9vYmFy");
    }

    fn fixture_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "factory-project-import-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn git(repository: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn repository_fixture(name: &str) -> (PathBuf, String) {
        let root = fixture_root(name);
        let repository = root.join("checkout");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        std::fs::write(
            repository.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(repository.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(
            repository.join("package.json"),
            r#"{"name":"fixture","scripts":{"test":"vitest run","lint":"biome check ."}}"#,
        )
        .unwrap();
        git(&repository, &["init", "-q"]);
        git(
            &repository,
            &["config", "user.email", "fixture@example.com"],
        );
        git(&repository, &["config", "user.name", "Fixture"]);
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "-q", "-m", "fixture"]);
        let commit = git(&repository, &["rev-parse", "HEAD"]);
        (root, commit)
    }

    #[test]
    fn local_import_is_confined_to_the_approved_root() {
        let root = fixture_root("root");
        let approved = root.join("approved");
        std::fs::create_dir_all(&approved).unwrap();
        let inside = approved.join("repo");
        std::fs::create_dir_all(&inside).unwrap();
        assert_eq!(
            approved_local_path(Some(&root), inside.to_str().unwrap()).unwrap(),
            inside.canonicalize().unwrap()
        );
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        assert!(approved_local_path(Some(&approved), outside.to_str().unwrap()).is_err());
        assert!(approved_local_path(None, inside.to_str().unwrap()).is_err());
        assert!(approved_local_path(Some(&root), "relative/repo").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn local_import_rejects_a_symlink_escape() {
        let root = fixture_root("symlink");
        let approved = root.join("approved");
        let outside = root.join("outside");
        std::fs::create_dir_all(&approved).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let link = approved.join("escape");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let error = approved_local_path(Some(&approved), link.to_str().unwrap()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside the managed projects root")
        );
    }

    #[test]
    fn discovery_is_bound_to_the_commit_and_maps_existing_capabilities() {
        let (root, commit) = repository_fixture("discover");
        let repository = root.join("checkout");
        let discovery = discover(&repository, &commit).unwrap();
        assert_eq!(discovery.base_commit, commit);
        assert_eq!(discovery.provenance, PROVENANCE);
        assert!(
            discovery
                .toolchains
                .iter()
                .any(|toolchain| toolchain.id == "rust")
        );
        assert!(
            discovery
                .toolchains
                .iter()
                .any(|toolchain| toolchain.id == "node")
        );
        assert!(
            discovery
                .suggested_capabilities
                .contains(&Capability::RepositoryEdit)
        );
        assert!(
            discovery
                .suggested_capabilities
                .contains(&Capability::GenericToolchain)
        );
        assert!(
            discovery
                .suggested_capabilities
                .contains(&Capability::NodeToolchain)
        );
        assert!(
            !discovery
                .suggested_capabilities
                .contains(&Capability::Browser)
        );
        assert!(discovery.suggested_checks.iter().any(|check| {
            check.id == "rust-test" && check.command == ["cargo", "test", "--all-targets"]
        }));
        assert!(
            discovery
                .suggested_checks
                .iter()
                .any(|check| check.id == "node-lint")
        );
        assert!(
            discovery
                .scanned_manifests
                .iter()
                .any(|manifest| manifest == "Cargo.toml")
        );
        // An uncommitted working-tree manifest is not evidence.
        std::fs::write(repository.join("go.mod"), "module example.com/drift\n").unwrap();
        let repeated = discover(&repository, &commit).unwrap();
        assert!(
            !repeated
                .toolchains
                .iter()
                .any(|toolchain| toolchain.id == "go")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsupported_toolchain_yields_only_repository_evidence() {
        let root = fixture_root("unsupported");
        let repository = root.join("checkout");
        std::fs::create_dir_all(&repository).unwrap();
        std::fs::write(repository.join("README.md"), "no manifest\n").unwrap();
        git(&repository, &["init", "-q"]);
        git(
            &repository,
            &["config", "user.email", "fixture@example.com"],
        );
        git(&repository, &["config", "user.name", "Fixture"]);
        git(&repository, &["add", "."]);
        git(&repository, &["commit", "-q", "-m", "fixture"]);
        let commit = git(&repository, &["rev-parse", "HEAD"]);
        let discovery = discover(&repository, &commit).unwrap();
        assert!(discovery.toolchains.is_empty());
        assert_eq!(
            discovery.suggested_capabilities,
            vec![Capability::RepositoryEdit]
        );
        assert!(discovery.suggested_checks.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_requires_an_exact_commit() {
        let root = fixture_root("commit");
        let error = discover(&root, "HEAD").unwrap_err();
        assert!(error.to_string().contains("exact commit"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
