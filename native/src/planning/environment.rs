//! Agnostic project-environment compiler.
//!
//! Core turns committed project conventions into a reviewed, licensed, pinned
//! Nix/devenv [`EnvironmentPlan`] and freezes it as an immutable
//! [`EnvironmentSpec`]. The module performs deterministic validation and no
//! execution. A model proposal is untrusted input; the Lead review is a
//! recorded planning decision that grants no execution, credential or license
//! authority. The Nix/devenv builder is a separate port implemented by the
//! builder work package.
//!
//! Decision mechanisms: discovery and validation are `DETERMINISTIC`; an
//! optional model proposal is `MODEL` input that never becomes authority; the
//! end-to-end compile/review/freeze path is `HYBRID`.
//!
//! Security: `TRUSTED_LOCAL`. Generated Nix/devenv files and the builder are
//! untrusted project input. A container is not isolation certification.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::lead::SpecCheck;
use crate::project_import::{git_blob, git_tree, is_commit_id};

pub const ENVIRONMENT_SCHEMA_VERSION: u32 = 1;
pub const ENVIRONMENT_POLICY_REVISION: &str = "nix-devenv-environment-v1";
pub const LICENSE_POLICY_REVISION: &str = "nix-devenv-license-v1";
pub const DEFAULT_PLATFORM: &str = "x86_64-linux";
/// Core default nixpkgs revision used only when neither the project nor the
/// request supplies a pin. It is an exact NixOS/nixpkgs commit, not a branch or
/// floating channel. Source: NixOS/nixpkgs commit 12e847a3f104db49190ac9564eb56acaf9e94783
/// (observed 2026-11-11); WP1-B updates this together with its builder image.
pub const DEFAULT_ENVIRONMENT_NIXPKGS: &str =
    "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";
pub const MAX_CONVENTION_FILES: usize = 256;
pub const MAX_CONVENTION_COMMANDS: usize = 64;
pub const MAX_CONVENTION_CONFLICTS: usize = 32;
pub const MAX_TOOLCHAINS: usize = 64;
pub const MAX_PACKAGES: usize = 512;
pub const MAX_SERVICES: usize = 32;
pub const MAX_COMMANDS: usize = 32;
pub const MAX_INPUTS: usize = 64;
pub const MAX_BROWSERS: usize = 16;
pub const MAX_ARGV: usize = 64;
pub const MAX_ARG_CHARS: usize = 2_000;
pub const MAX_FILE_BYTES: usize = 512 * 1024;
pub const MAX_REVIEW_RATIONALE_CHARS: usize = 4_000;
pub const MAX_PRINCIPAL_CHARS: usize = 200;
const MAX_SHORT_CHARS: usize = 500;
pub const LEAD_PRINCIPAL_PREFIX: &str = "lead:";

const MIN_CPU: u16 = 1;
const MAX_CPU: u16 = 16;
const MIN_MEMORY_MIB: u32 = 512;
const MAX_MEMORY_MIB: u32 = 65_536;
const MIN_DISK_MIB: u32 = 1_024;
const MAX_DISK_MIB: u32 = 262_144;
const MIN_PIDS: u32 = 64;
const MAX_PIDS: u32 = 4_096;
const MIN_TIMEOUT_SECS: u32 = 30;
const MAX_TIMEOUT_SECS: u32 = 7_200;

/// Stable typed failure codes. `as_str` is the API value; the enum is not a
/// free-form string so Core cannot accidentally publish a new blocker.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentBlockerCode {
    EnvironmentSpecInvalid,
    EnvironmentBuildFailed,
    EnvironmentBuilderUnavailable,
    EnvironmentLicenseBlocked,
    EnvironmentPlanRequired,
    EnvironmentConventionsMissing,
    EnvironmentConventionsConflict,
    EnvironmentConventionsStale,
    EnvironmentPlanInvalid,
    EnvironmentUnsupported,
    EnvironmentPinMissing,
    EnvironmentLicenseDenied,
    EnvironmentLicenseUnknown,
    EnvironmentUnfreeRequiresOwner,
    EnvironmentPaidServiceRequired,
    EnvironmentLeadApprovalRequired,
    EnvironmentOwnerApprovalRequired,
    EnvironmentSecretDetected,
    EnvironmentSandboxDenied,
    EnvironmentNetworkDenied,
    EnvironmentResourceLimit,
    EnvironmentBuildTimeout,
    EnvironmentHashMismatch,
    EnvironmentNotFrozen,
    EnvironmentSuperseded,
}

impl EnvironmentBlockerCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnvironmentSpecInvalid => "ENVIRONMENT_SPEC_INVALID",
            Self::EnvironmentBuildFailed => "ENVIRONMENT_BUILD_FAILED",
            Self::EnvironmentBuilderUnavailable => "ENVIRONMENT_BUILDER_UNAVAILABLE",
            Self::EnvironmentLicenseBlocked => "ENVIRONMENT_LICENSE_BLOCKED",
            Self::EnvironmentPlanRequired => "ENVIRONMENT_PLAN_REQUIRED",
            Self::EnvironmentConventionsMissing => "ENVIRONMENT_CONVENTIONS_MISSING",
            Self::EnvironmentConventionsConflict => "ENVIRONMENT_CONVENTIONS_CONFLICT",
            Self::EnvironmentConventionsStale => "ENVIRONMENT_CONVENTIONS_STALE",
            Self::EnvironmentPlanInvalid => "ENVIRONMENT_PLAN_INVALID",
            Self::EnvironmentUnsupported => "ENVIRONMENT_UNSUPPORTED",
            Self::EnvironmentPinMissing => "ENVIRONMENT_PIN_MISSING",
            Self::EnvironmentLicenseDenied => "ENVIRONMENT_LICENSE_DENIED",
            Self::EnvironmentLicenseUnknown => "ENVIRONMENT_LICENSE_UNKNOWN",
            Self::EnvironmentUnfreeRequiresOwner => "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
            Self::EnvironmentPaidServiceRequired => "ENVIRONMENT_PAID_SERVICE_REQUIRED",
            Self::EnvironmentLeadApprovalRequired => "ENVIRONMENT_LEAD_APPROVAL_REQUIRED",
            Self::EnvironmentOwnerApprovalRequired => "ENVIRONMENT_OWNER_APPROVAL_REQUIRED",
            Self::EnvironmentSecretDetected => "ENVIRONMENT_SECRET_DETECTED",
            Self::EnvironmentSandboxDenied => "ENVIRONMENT_SANDBOX_DENIED",
            Self::EnvironmentNetworkDenied => "ENVIRONMENT_NETWORK_DENIED",
            Self::EnvironmentResourceLimit => "ENVIRONMENT_RESOURCE_LIMIT",
            Self::EnvironmentBuildTimeout => "ENVIRONMENT_BUILD_TIMEOUT",
            Self::EnvironmentHashMismatch => "ENVIRONMENT_HASH_MISMATCH",
            Self::EnvironmentNotFrozen => "ENVIRONMENT_NOT_FROZEN",
            Self::EnvironmentSuperseded => "ENVIRONMENT_SUPERSEDED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentError {
    pub code: EnvironmentBlockerCode,
    pub message: String,
}

impl EnvironmentError {
    pub fn new(code: EnvironmentBlockerCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: bounded_error_message(message.into()),
        }
    }

    pub const fn code_str(&self) -> &'static str {
        self.code.as_str()
    }
}

impl std::fmt::Display for EnvironmentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for EnvironmentError {}

fn bounded_error_message(value: String) -> String {
    value.chars().take(1_000).collect()
}

fn invalid(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentSpecInvalid, message)
}

fn plan_invalid(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentPlanInvalid, message)
}

fn pin_missing(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentPinMissing, message)
}

fn license_blocked(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentLicenseBlocked, message)
}

fn sandbox_denied(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentSandboxDenied, message)
}

fn network_denied(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentNetworkDenied, message)
}

fn lead_approval_required(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(
        EnvironmentBlockerCode::EnvironmentLeadApprovalRequired,
        message,
    )
}

fn hash_mismatch(message: impl Into<String>) -> EnvironmentError {
    EnvironmentError::new(EnvironmentBlockerCode::EnvironmentHashMismatch, message)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_hash<T: Serialize>(value: &T) -> String {
    sha256_hex(&serde_json::to_vec(value).expect("environment canonical JSON serializes"))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded_text(name: &str, value: &str, max: usize) -> Result<(), EnvironmentError> {
    if value.trim().is_empty()
        || value.len() > max
        || value.chars().any(|character| character.is_control())
    {
        return Err(invalid(format!(
            "{name} is required, bounded and control-free"
        )));
    }
    Ok(())
}

fn bounded_list(name: &str, values: &[String], max: usize) -> Result<(), EnvironmentError> {
    if values.len() > max {
        return Err(invalid(format!("{name} exceeds its bound")));
    }
    for value in values {
        bounded_text(name, value, MAX_SHORT_CHARS)?;
    }
    Ok(())
}

fn exact_version(name: &str, value: &str) -> Result<(), EnvironmentError> {
    bounded_text(name, value, 64)?;
    let lower = value.to_ascii_lowercase();
    if value.chars().any(char::is_whitespace)
        || matches!(
            lower.as_str(),
            "latest" | "stable" | "nightly" | "current" | "*" | "master" | "main"
        )
        || value.starts_with('^')
        || value.starts_with('~')
        || value.contains(">=")
        || value.contains("<=")
    {
        return Err(pin_missing(format!(
            "{name} must be an exact version pin, not a floating range"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Committed conventions
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConventionPrecedence {
    CommittedEnvironment,
    VersionPin,
    Manifest,
    Lockfile,
    CiCommand,
    Documentation,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConventionFileKind {
    FlakeNix,
    FlakeLock,
    DevenvNix,
    DevenvYaml,
    DevenvLock,
    Devcontainer,
    Mise,
    ToolVersions,
    VersionPin,
    Manifest,
    Lockfile,
    CiWorkflow,
    Documentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExistingEnvironmentKind {
    Devenv,
    Flake,
    Devcontainer,
    None,
}

impl ExistingEnvironmentKind {
    pub const fn is_present(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConventionPin {
    pub name: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConventionFile {
    pub path: String,
    pub kind: ConventionFileKind,
    pub sha256: String,
    pub precedence: ConventionPrecedence,
    #[serde(default)]
    pub pins: Vec<ConventionPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConventionCommand {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    pub source: String,
}

fn default_cwd() -> String {
    ".".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConventionConflict {
    pub name: String,
    pub detail: String,
    pub sources: Vec<String>,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectConventions {
    pub schema_version: u32,
    pub project_id: String,
    pub base_commit: String,
    pub conventions_sha256: String,
    pub scanned_at: String,
    #[serde(default)]
    pub files: Vec<ConventionFile>,
    #[serde(default)]
    pub commands: Vec<ConventionCommand>,
    #[serde(default)]
    pub conflicts: Vec<ConventionConflict>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub skipped_entries: usize,
    pub provenance: String,
    pub existing_environment: ExistingEnvironmentKind,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConventionsContent<'a> {
    schema_version: u32,
    project_id: &'a str,
    base_commit: &'a str,
    files: &'a [ConventionFile],
    commands: &'a [ConventionCommand],
    conflicts: &'a [ConventionConflict],
    truncated: bool,
    skipped_entries: usize,
    existing_environment: ExistingEnvironmentKind,
}

impl ProjectConventions {
    pub fn compute_sha256(&self) -> String {
        canonical_hash(&ConventionsContent {
            schema_version: self.schema_version,
            project_id: &self.project_id,
            base_commit: &self.base_commit,
            files: &self.files,
            commands: &self.commands,
            conflicts: &self.conflicts,
            truncated: self.truncated,
            skipped_entries: self.skipped_entries,
            existing_environment: self.existing_environment,
        })
    }

    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid(
                "unsupported environment conventions schema version",
            ));
        }
        bounded_text("conventions project id", &self.project_id, 200)?;
        if !is_commit_id(&self.base_commit) {
            return Err(invalid("conventions base must be an exact commit id"));
        }
        if !is_sha256(&self.conventions_sha256) {
            return Err(invalid("conventions hash must be SHA-256"));
        }
        if self.conventions_sha256 != self.compute_sha256() {
            return Err(hash_mismatch("conventions content does not match its hash"));
        }
        if self.files.len() > MAX_CONVENTION_FILES
            || self.commands.len() > MAX_CONVENTION_COMMANDS
            || self.conflicts.len() > MAX_CONVENTION_CONFLICTS
        {
            return Err(invalid("conventions snapshot exceeds its bounds"));
        }
        if !self.conflicts.is_empty() {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentConventionsConflict,
                format!(
                    "conflicting committed conventions: {}",
                    self.conflicts
                        .iter()
                        .map(|conflict| conflict.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        Ok(())
    }

    pub fn has_file(&self, name: &str) -> bool {
        self.files
            .iter()
            .any(|file| file.path.eq_ignore_ascii_case(name))
    }

    pub fn pins_named(&self, name: &str) -> Vec<&ConventionPin> {
        self.files
            .iter()
            .flat_map(|file| file.pins.iter())
            .filter(|pin| pin.name.eq_ignore_ascii_case(name))
            .collect()
    }
}

/// Discover committed conventions from the exact Git commit. Working-tree
/// state, host paths and model text never contribute.
pub fn discover_project_conventions(
    project_id: &str,
    repository: &Path,
    base_commit: &str,
    now: DateTime<Utc>,
) -> Result<ProjectConventions, EnvironmentError> {
    bounded_text("conventions project id", project_id, 200)?;
    if !is_commit_id(base_commit) {
        return Err(invalid("conventions base must be an exact commit id"));
    }
    let tree = git_tree(repository, base_commit).map_err(|error| {
        EnvironmentError::new(
            EnvironmentBlockerCode::EnvironmentConventionsMissing,
            format!("the recorded commit could not be read: {error}"),
        )
    })?;
    let mut files = Vec::new();
    let mut commands = Vec::new();
    let mut skipped_entries = tree.skipped_entries;
    let mut truncated = tree.truncated;
    for path in &tree.paths {
        if files.len() >= MAX_CONVENTION_FILES {
            truncated = true;
            break;
        }
        let Some((kind, precedence)) = classify_convention_path(path) else {
            continue;
        };
        let content = match git_blob(repository, base_commit, path) {
            Ok(content) if content.len() <= MAX_FILE_BYTES => content,
            Ok(_) => {
                skipped_entries += 1;
                continue;
            }
            Err(_) => {
                skipped_entries += 1;
                continue;
            }
        };
        let mut pins = extract_pins(kind, path, &content);
        for pin in &mut pins {
            pin.value = pin.value.trim().to_owned();
            pin.name = pin.name.trim().to_ascii_lowercase();
        }
        pins.retain(|pin| !pin.value.is_empty());
        if kind == ConventionFileKind::CiWorkflow {
            for (index, argv) in parse_ci_commands(&content).into_iter().enumerate() {
                if commands.len() >= MAX_CONVENTION_COMMANDS {
                    truncated = true;
                    break;
                }
                commands.push(ConventionCommand {
                    name: format!("ci-{}-{index}", command_name(&argv)),
                    argv,
                    cwd: ".".into(),
                    source: path.clone(),
                });
            }
        }
        files.push(ConventionFile {
            path: path.clone(),
            kind,
            sha256: sha256_hex(content.as_bytes()),
            precedence,
            pins,
        });
    }
    let existing_environment = existing_environment(&files);
    let conflicts = detect_conflicts(&files);
    commands.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.name.cmp(&right.name))
    });
    commands.dedup_by(|left, right| left.argv == right.argv && left.source == right.source);
    let mut conventions = ProjectConventions {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        project_id: project_id.into(),
        base_commit: base_commit.into(),
        conventions_sha256: String::new(),
        scanned_at: now.to_rfc3339(),
        files,
        commands,
        conflicts,
        truncated,
        skipped_entries,
        provenance: "git-object-store".into(),
        existing_environment,
    };
    conventions.conventions_sha256 = conventions.compute_sha256();
    Ok(conventions)
}

fn classify_convention_path(path: &str) -> Option<(ConventionFileKind, ConventionPrecedence)> {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    let kind = match name {
        "flake.nix" => (
            ConventionFileKind::FlakeNix,
            ConventionPrecedence::CommittedEnvironment,
        ),
        "flake.lock" => (
            ConventionFileKind::FlakeLock,
            ConventionPrecedence::CommittedEnvironment,
        ),
        "devenv.nix" => (
            ConventionFileKind::DevenvNix,
            ConventionPrecedence::CommittedEnvironment,
        ),
        "devenv.yaml" => (
            ConventionFileKind::DevenvYaml,
            ConventionPrecedence::CommittedEnvironment,
        ),
        "devenv.lock" => (
            ConventionFileKind::DevenvLock,
            ConventionPrecedence::CommittedEnvironment,
        ),
        "devcontainer.json" if lower.contains(".devcontainer/") => (
            ConventionFileKind::Devcontainer,
            ConventionPrecedence::CommittedEnvironment,
        ),
        "mise.toml" | ".mise.toml" => (ConventionFileKind::Mise, ConventionPrecedence::VersionPin),
        ".tool-versions" => (
            ConventionFileKind::ToolVersions,
            ConventionPrecedence::VersionPin,
        ),
        ".nvmrc" | ".python-version" | "rust-toolchain.toml" | "rust-toolchain" | "global.json" => {
            (
                ConventionFileKind::VersionPin,
                ConventionPrecedence::VersionPin,
            )
        }
        "cargo.toml" | "package.json" | "go.mod" | "pyproject.toml" | "setup.py" | "setup.cfg"
        | "requirements.txt" | "pipfile" | "pom.xml" | "build.gradle" | "build.gradle.kts"
        | "composer.json" | "gemfile" | "mix.exs" | "pubspec.yaml" | "package.swift"
        | "cmakelists.txt" => (ConventionFileKind::Manifest, ConventionPrecedence::Manifest),
        "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "bun.lock"
        | "bun.lockb" | "go.sum" | "poetry.lock" | "uv.lock" | "pipfile.lock" | "composer.lock"
        | "gemfile.lock" | "mix.lock" | "pubspec.lock" | "package.resolved" => {
            (ConventionFileKind::Lockfile, ConventionPrecedence::Lockfile)
        }
        "agents.md" | "readme.md" | "readme" => (
            ConventionFileKind::Documentation,
            ConventionPrecedence::Documentation,
        ),
        _ if lower.ends_with(".csproj")
            || lower.ends_with(".fsproj")
            || lower.ends_with(".vbproj")
            || lower.ends_with(".sln") =>
        {
            (ConventionFileKind::Manifest, ConventionPrecedence::Manifest)
        }
        _ if lower.starts_with(".github/workflows/")
            && (lower.ends_with(".yml") || lower.ends_with(".yaml")) =>
        {
            (
                ConventionFileKind::CiWorkflow,
                ConventionPrecedence::CiCommand,
            )
        }
        ".gitlab-ci.yml" | ".circleci/config.yml" | "azure-pipelines.yml" | "jenkinsfile" => (
            ConventionFileKind::CiWorkflow,
            ConventionPrecedence::CiCommand,
        ),
        _ => return None,
    };
    Some(kind)
}

fn existing_environment(files: &[ConventionFile]) -> ExistingEnvironmentKind {
    let has = |kind: ConventionFileKind| files.iter().any(|file| file.kind == kind);
    if has(ConventionFileKind::DevenvNix)
        || has(ConventionFileKind::DevenvYaml)
        || has(ConventionFileKind::DevenvLock)
    {
        ExistingEnvironmentKind::Devenv
    } else if has(ConventionFileKind::FlakeNix) || has(ConventionFileKind::FlakeLock) {
        ExistingEnvironmentKind::Flake
    } else if has(ConventionFileKind::Devcontainer) {
        ExistingEnvironmentKind::Devcontainer
    } else {
        ExistingEnvironmentKind::None
    }
}

fn extract_pins(kind: ConventionFileKind, path: &str, content: &str) -> Vec<ConventionPin> {
    let mut pins = Vec::new();
    let source = path.to_owned();
    match kind {
        ConventionFileKind::VersionPin => {
            let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
            let value = match name.as_str() {
                "rust-toolchain.toml" => parse_toml_string(content, "channel"),
                "rust-toolchain" => first_meaningful_line(content),
                ".nvmrc" => first_meaningful_line(content),
                ".python-version" => first_meaningful_line(content),
                "global.json" => parse_json_path(content, &["sdk", "version"]),
                _ => None,
            };
            if let Some(value) = value {
                let pin_name = match name.as_str() {
                    "rust-toolchain.toml" | "rust-toolchain" => "rust",
                    ".nvmrc" => "node",
                    ".python-version" => "python",
                    "global.json" => "dotnet",
                    _ => return pins,
                };
                pins.push(ConventionPin {
                    name: pin_name.into(),
                    value,
                    source,
                });
            }
        }
        ConventionFileKind::ToolVersions | ConventionFileKind::Mise => {
            for (name, value) in parse_tool_versions(content) {
                pins.push(ConventionPin {
                    name: normalize_tool_name(&name),
                    value,
                    source: source.clone(),
                });
            }
        }
        ConventionFileKind::FlakeLock | ConventionFileKind::DevenvLock => {
            if let Some(value) = parse_lock_nixpkgs(content) {
                pins.push(ConventionPin {
                    name: "nixpkgs".into(),
                    value,
                    source,
                });
            }
        }
        ConventionFileKind::FlakeNix
        | ConventionFileKind::DevenvYaml
        | ConventionFileKind::DevenvNix => {
            if let Some(value) = parse_nixpkgs_reference(content) {
                pins.push(ConventionPin {
                    name: "nixpkgs".into(),
                    value,
                    source,
                });
            }
        }
        ConventionFileKind::Manifest => {
            let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
            match name.as_str() {
                "package.json" => {
                    if let Some(value) = parse_json_string(content, &["packageManager"]) {
                        let (manager, version) =
                            value.split_once('@').unwrap_or((value.as_str(), ""));
                        if !version.is_empty() {
                            pins.push(ConventionPin {
                                name: normalize_tool_name(manager),
                                value: version.to_owned(),
                                source,
                            });
                        }
                    }
                }
                "go.mod" => {
                    if let Some(value) = parse_go_version(content) {
                        pins.push(ConventionPin {
                            name: "go".into(),
                            value,
                            source,
                        });
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    pins
}

fn parse_toml_string(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

fn first_meaningful_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .map(str::to_owned)
}

fn parse_json_path(content: &str, path: &[&str]) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(content).ok()?;
    for key in path {
        value = value.get(*key)?.clone();
    }
    value.as_str().map(str::to_owned)
}

fn parse_json_string(content: &str, path: &[&str]) -> Option<String> {
    parse_json_path(content, path)
}

fn parse_tool_versions(content: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    let has_tools_section = content
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("[tools]"));
    let mut in_tools = !has_tools_section;
    for line in content.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_tools = line.eq_ignore_ascii_case("[tools]");
            continue;
        }
        if !in_tools {
            continue;
        }
        let (name, value) = if let Some((name, value)) = line.split_once('=') {
            (
                name.trim().trim_matches('"').to_owned(),
                value.trim().trim_matches('"').to_owned(),
            )
        } else {
            let mut parts = line.split_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            (name.to_owned(), parts.collect::<Vec<_>>().join(" "))
        };
        if name.is_empty() || value.is_empty() {
            continue;
        }
        values.push((name, value));
    }
    values
}

fn normalize_tool_name(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "nodejs" | "node" => "node".into(),
        "golang" | "go" => "go".into(),
        "python" | "python3" => "python".into(),
        "rust" | "rustc" => "rust".into(),
        "dotnet" | "dotnet-core" => "dotnet".into(),
        "ruby" => "ruby".into(),
        "java" | "temurin" | "openjdk" => "java".into(),
        "pnpm" | "npm" | "yarn" | "bun" => value.to_ascii_lowercase(),
        other => other.into(),
    }
}

fn parse_lock_nixpkgs(content: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let nodes = value.get("nodes")?.as_object()?;
    for (name, node) in nodes {
        let Some(locked) = node.get("locked") else {
            continue;
        };
        let Some(repo) = locked.get("repo").and_then(|value| value.as_str()) else {
            continue;
        };
        if repo != "nixpkgs" && !name.contains("nixpkgs") {
            continue;
        }
        let Some(owner) = locked.get("owner").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(rev) = locked.get("rev").and_then(|value| value.as_str()) else {
            continue;
        };
        if is_commit_id(rev) {
            return Some(format!("github:{owner}/{repo}/{rev}"));
        }
    }
    None
}

fn parse_nixpkgs_reference(content: &str) -> Option<String> {
    static REFERENCE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let regex = REFERENCE.get_or_init(|| {
        Regex::new(r"github:NixOS/nixpkgs/([0-9a-fA-F]{40}|[0-9a-fA-F]{64})")
            .expect("nixpkgs reference regex")
    });
    regex
        .find(content)
        .map(|matched| matched.as_str().to_owned())
}

fn parse_go_version(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("go ") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn parse_ci_commands(content: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    for line in content.lines() {
        let mut line = line.trim();
        if let Some(rest) = line.strip_prefix("- ") {
            line = rest.trim();
        }
        let Some(value) = line.strip_prefix("run:") else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty()
            || value.contains("&&")
            || value.contains(['$', '`', '|', '>', '<', ';', '&', '(', ')'])
        {
            continue;
        }
        let argv = value
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if argv.is_empty()
            || argv.len() > 16
            || argv
                .iter()
                .any(|arg| arg.len() > MAX_ARG_CHARS || arg.contains('\0'))
        {
            continue;
        }
        commands.push(argv);
    }
    commands
}

fn command_name(argv: &[String]) -> String {
    argv.first()
        .map(|value| {
            value
                .chars()
                .filter(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
                })
                .take(32)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "command".into())
}

fn detect_conflicts(files: &[ConventionFile]) -> Vec<ConventionConflict> {
    let mut by_name: BTreeMap<String, Vec<(&ConventionPin, ConventionPrecedence)>> =
        BTreeMap::new();
    for file in files {
        if file.precedence == ConventionPrecedence::Lockfile {
            continue;
        }
        for pin in &file.pins {
            by_name
                .entry(pin.name.clone())
                .or_default()
                .push((pin, file.precedence));
        }
    }
    let mut conflicts = Vec::new();
    for (name, pins) in by_name {
        let mut values = pins
            .iter()
            .map(|(pin, _)| pin.value.clone())
            .collect::<Vec<_>>();
        values.sort();
        values.dedup();
        if values.len() <= 1 {
            continue;
        }
        let mut sources = pins
            .iter()
            .map(|(pin, _)| pin.source.clone())
            .collect::<Vec<_>>();
        sources.sort();
        sources.dedup();
        conflicts.push(ConventionConflict {
            name,
            detail: "equally-specific committed pins disagree; Core will not choose one silently"
                .to_string(),
            sources,
            values,
        });
    }
    conflicts.sort_by(|left, right| left.name.cmp(&right.name));
    conflicts
}

// ---------------------------------------------------------------------------
// Requirements and plan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolchainRequirement {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PackageRequirement {
    pub name: String,
    pub version: String,
    pub license: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub attr: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub unfree: bool,
    #[serde(default)]
    pub attribution: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ServiceNetwork {
    #[default]
    Loopback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServiceRequirement {
    pub name: String,
    pub version: String,
    #[serde(default = "default_true")]
    pub ephemeral: bool,
    #[serde(default = "default_loopback")]
    pub network: ServiceNetwork,
    #[serde(default)]
    pub port: Option<u16>,
}

fn default_true() -> bool {
    true
}

fn default_loopback() -> ServiceNetwork {
    ServiceNetwork::Loopback
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CommandRequirement {
    pub argv: Vec<String>,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    #[serde(default = "default_command_timeout")]
    pub timeout_secs: u32,
    #[serde(default)]
    pub expect_exit: i32,
}

fn default_command_timeout() -> u32 {
    300
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BrowserRequirement {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NixInput {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExplicitEnvironmentConstraints {
    #[serde(default)]
    pub toolchains: Vec<ToolchainRequirement>,
    #[serde(default)]
    pub packages: Vec<PackageRequirement>,
    #[serde(default)]
    pub services: Vec<ServiceRequirement>,
    #[serde(default)]
    pub commands: BTreeMap<String, CommandRequirement>,
    #[serde(default)]
    pub nixpkgs: Option<String>,
    #[serde(default)]
    pub inputs: Vec<NixInput>,
    #[serde(default)]
    pub browsers: Vec<BrowserRequirement>,
    #[serde(default)]
    pub allow_unfree: bool,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

/// Operator-supplied environment constraints carried by a production intake
/// request. This is the supported production input path for a generated
/// environment; absent fields are resolved from committed conventions or the
/// builder's documented default pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RequestEnvironmentConstraints {
    #[serde(default)]
    pub nixpkgs: Option<String>,
    #[serde(default)]
    pub toolchains: Vec<ToolchainRequirement>,
    #[serde(default)]
    pub packages: Vec<PackageRequirement>,
    #[serde(default)]
    pub services: Vec<ServiceRequirement>,
    #[serde(default)]
    pub browsers: Vec<BrowserRequirement>,
    #[serde(default)]
    pub resources: Option<ResourceLimits>,
}

impl RequestEnvironmentConstraints {
    pub fn into_requirements(self) -> EnvironmentRequirements {
        EnvironmentRequirements {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            explicit_constraints: ExplicitEnvironmentConstraints {
                toolchains: self.toolchains,
                packages: self.packages,
                services: self.services,
                browsers: self.browsers,
                nixpkgs: self.nixpkgs,
                ..ExplicitEnvironmentConstraints::default()
            },
            proposal: None,
            allow_unfree: false,
            owner_overrides: Vec::new(),
            resource_limits: self.resources.unwrap_or_default(),
            network: NetworkPolicy::default(),
        }
    }
}

/// A model/Lead/Librarian proposal. Every field is untrusted and Core
/// revalidates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentProposal {
    #[serde(default)]
    pub toolchains: Vec<ToolchainRequirement>,
    #[serde(default)]
    pub packages: Vec<PackageRequirement>,
    #[serde(default)]
    pub services: Vec<ServiceRequirement>,
    #[serde(default)]
    pub commands: BTreeMap<String, CommandRequirement>,
    #[serde(default)]
    pub nixpkgs: Option<String>,
    #[serde(default)]
    pub inputs: Vec<NixInput>,
    #[serde(default)]
    pub browsers: Vec<BrowserRequirement>,
    #[serde(default)]
    pub allow_unfree: bool,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UnfreeOverride {
    pub id: String,
    pub package: String,
    pub version: String,
    pub license: String,
    #[serde(default)]
    pub plan_sha256: String,
    pub reason: String,
    pub decision_id: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentRequirements {
    pub schema_version: u32,
    #[serde(default)]
    pub explicit_constraints: ExplicitEnvironmentConstraints,
    #[serde(default)]
    pub proposal: Option<EnvironmentProposal>,
    #[serde(default)]
    pub allow_unfree: bool,
    #[serde(default)]
    pub owner_overrides: Vec<UnfreeOverride>,
    #[serde(default)]
    pub resource_limits: ResourceLimits,
    pub network: NetworkPolicy,
}

impl Default for EnvironmentRequirements {
    fn default() -> Self {
        Self {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            explicit_constraints: ExplicitEnvironmentConstraints::default(),
            proposal: None,
            allow_unfree: false,
            owner_overrides: Vec::new(),
            resource_limits: ResourceLimits::default(),
            network: NetworkPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ResourceLimits {
    pub cpu: u16,
    pub memory_mib: u32,
    pub disk_mib: u32,
    pub pids: u32,
    pub timeout_secs: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu: 2,
            memory_mib: 4_096,
            disk_mib: 10_240,
            pids: 512,
            timeout_secs: 1_800,
        }
    }
}

impl ResourceLimits {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if !(MIN_CPU..=MAX_CPU).contains(&self.cpu)
            || !(MIN_MEMORY_MIB..=MAX_MEMORY_MIB).contains(&self.memory_mib)
            || !(MIN_DISK_MIB..=MAX_DISK_MIB).contains(&self.disk_mib)
            || !(MIN_PIDS..=MAX_PIDS).contains(&self.pids)
            || !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs)
        {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentResourceLimit,
                "CPU, memory, disk, PID or wall-clock bound is outside the supported policy",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BuildNetwork {
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FetchNetwork {
    Broker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetworkPolicy {
    pub build: BuildNetwork,
    pub fetch: FetchNetwork,
    #[serde(default)]
    pub fetch_allowlist: Vec<String>,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            build: BuildNetwork::None,
            fetch: FetchNetwork::Broker,
            fetch_allowlist: Vec::new(),
        }
    }
}

impl NetworkPolicy {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if self.fetch_allowlist.len() > 32 {
            return Err(network_denied("fetch allowlist exceeds its bound"));
        }
        for entry in &self.fetch_allowlist {
            if entry.len() > 256
                || entry.contains('*')
                || entry.contains('@')
                || entry.chars().any(char::is_whitespace)
                || !(entry.starts_with("https://") || entry.starts_with("github:"))
            {
                return Err(network_denied(
                    "fetch allowlist entries must be credential-free HTTPS or pinned GitHub sources",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SandboxPolicy {
    pub sandbox: bool,
    pub impure: bool,
    #[serde(default)]
    pub host_mounts: Vec<String>,
    pub privileged: bool,
    pub host_network: bool,
    #[serde(default)]
    pub host_sockets: Vec<String>,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self {
            sandbox: true,
            impure: false,
            host_mounts: Vec::new(),
            privileged: false,
            host_network: false,
            host_sockets: Vec::new(),
        }
    }
}

impl SandboxPolicy {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if !self.sandbox
            || self.impure
            || self.privileged
            || self.host_network
            || !self.host_mounts.is_empty()
            || !self.host_sockets.is_empty()
        {
            return Err(sandbox_denied(
                "the builder must run with sandbox=true, impure=false, no host mount, socket, privilege or host network",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentPlanStatus {
    Draft,
    CoreValidated,
    LeadApproved,
    Frozen,
    Rejected,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentPlanSource {
    ProjectCommitted,
    Generated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentFileOrigin {
    Project,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentFile {
    pub path: String,
    pub sha256: String,
    pub origin: EnvironmentFileOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentNixPolicy {
    #[serde(default)]
    pub nixpkgs: String,
    #[serde(default)]
    pub inputs: Vec<NixInput>,
    pub allow_unfree: bool,
    pub sandbox: SandboxPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentBuilderDescriptor {
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default = "default_platform_string")]
    pub platform: String,
    #[serde(default = "default_builder_policy_revision")]
    pub policy_revision: String,
    /// Builder-pinned nixpkgs revision used when neither the request nor the
    /// committed conventions provide an explicit pin. This is infrastructure
    /// policy, never a language default.
    #[serde(default)]
    pub default_nixpkgs: String,
    pub status: EnvironmentBuilderStatus,
}

fn default_platform_string() -> String {
    DEFAULT_PLATFORM.into()
}

fn default_builder_policy_revision() -> String {
    ENVIRONMENT_POLICY_REVISION.into()
}

impl Default for EnvironmentBuilderDescriptor {
    fn default() -> Self {
        Self {
            image: String::new(),
            digest: String::new(),
            platform: default_platform_string(),
            policy_revision: default_builder_policy_revision(),
            default_nixpkgs: DEFAULT_ENVIRONMENT_NIXPKGS.into(),
            status: EnvironmentBuilderStatus::Unavailable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentBuilderStatus {
    Unavailable,
    Ready,
    Building,
    Succeeded,
    Failed,
}

impl EnvironmentBuilderDescriptor {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if !self.image.is_empty()
            && (self.image.len() > 512
                || self.image.contains('@')
                || self.image.chars().any(char::is_whitespace)
                || self.image.chars().any(|character| character.is_control()))
        {
            return Err(invalid("builder image reference is not a clean image name"));
        }
        if !self.digest.is_empty() && !is_digest(&self.digest) {
            return Err(invalid(
                "builder image digest must be a sha256: digest without a tag",
            ));
        }
        if self.platform != DEFAULT_PLATFORM {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentUnsupported,
                format!(
                    "builder platform {} is not supported by the current frozen policy",
                    self.platform
                ),
            ));
        }
        bounded_text("builder policy revision", &self.policy_revision, 128)?;
        if !self.default_nixpkgs.is_empty() {
            validate_nixpkgs_pin(&self.default_nixpkgs)?;
        }
        Ok(())
    }

    pub fn is_available(&self) -> bool {
        !self.image.is_empty()
            && is_digest(&self.digest)
            && matches!(
                self.status,
                EnvironmentBuilderStatus::Ready
                    | EnvironmentBuilderStatus::Building
                    | EnvironmentBuilderStatus::Succeeded
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentPlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub project_id: String,
    pub request_id: String,
    pub action_id: String,
    #[serde(default)]
    pub campaign_id: Option<String>,
    pub base_commit: String,
    pub conventions_sha256: String,
    pub conventions: ProjectConventions,
    pub risk_class: String,
    pub status: EnvironmentPlanStatus,
    pub revision: u32,
    pub source: EnvironmentPlanSource,
    #[serde(default)]
    pub toolchains: Vec<ToolchainRequirement>,
    #[serde(default)]
    pub packages: Vec<PackageRequirement>,
    #[serde(default)]
    pub services: Vec<ServiceRequirement>,
    #[serde(default)]
    pub commands: BTreeMap<String, CommandRequirement>,
    #[serde(default)]
    pub browsers: Vec<BrowserRequirement>,
    #[serde(default)]
    pub files: Vec<EnvironmentFile>,
    pub nix: EnvironmentNixPolicy,
    pub resources: ResourceLimits,
    pub network: NetworkPolicy,
    #[serde(default)]
    pub license_inventory: LicenseInventory,
    #[serde(default)]
    pub license_inventory_sha256: String,
    #[serde(default)]
    pub builder: EnvironmentBuilderDescriptor,
    #[serde(default)]
    pub resolution: Option<EnvironmentResolution>,
    #[serde(default)]
    pub lead_review: Option<EnvironmentPlanReview>,
    #[serde(default)]
    pub blockers: Vec<EnvironmentBlocker>,
    #[serde(default)]
    pub commands_rationale: String,
    pub plan_sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentBlocker {
    pub code: EnvironmentBlockerCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeadReviewDecision {
    Approved,
    ChangesRequested,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentReviewerRole {
    Lead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentLeadReviewModelDecision {
    Approve,
    ChangesRequested,
    Reject,
}

impl EnvironmentLeadReviewModelDecision {
    pub const fn as_lead_review(self) -> LeadReviewDecision {
        match self {
            Self::Approve => LeadReviewDecision::Approved,
            Self::ChangesRequested => LeadReviewDecision::ChangesRequested,
            Self::Reject => LeadReviewDecision::Rejected,
        }
    }
}

/// Typed Lead model decision for one exact EnvironmentPlan. The model may not
/// add fields, change the plan identity or approve a blocked plan; Core maps
/// the decision into the recorded planning review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentLeadReviewModel {
    pub schema_version: u32,
    pub request_id: String,
    pub action_id: String,
    pub plan_sha256: String,
    pub plan_revision: u32,
    pub risk_class: String,
    pub decision: EnvironmentLeadReviewModelDecision,
    pub rationale: String,
    #[serde(default)]
    pub constraints: Vec<String>,
}

impl EnvironmentLeadReviewModel {
    pub fn validate_for(
        &self,
        plan: &EnvironmentPlan,
        now: DateTime<Utc>,
    ) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid(
                "unsupported Lead environment review schema version",
            ));
        }
        if self.request_id != plan.request_id
            || self.action_id != plan.action_id
            || self.plan_sha256 != plan.plan_sha256
            || self.plan_revision != plan.revision
            || self.risk_class != plan.risk_class
        {
            return Err(hash_mismatch(
                "the Lead environment review does not bind the exact plan identity and risk",
            ));
        }
        bounded_text(
            "Lead environment review rationale",
            &self.rationale,
            MAX_REVIEW_RATIONALE_CHARS,
        )?;
        bounded_list("Lead environment constraints", &self.constraints, 16)?;
        if self.rationale.trim().len() < 8 {
            return Err(invalid(
                "the Lead environment review requires a bounded rationale",
            ));
        }
        let _ = now;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentReviewModelCall {
    pub mechanism: String,
    pub provider: String,
    pub model: String,
    pub reservation_id: String,
    pub request_sha256: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_micros: u64,
    #[serde(default)]
    pub usage_known: bool,
}

impl EnvironmentReviewModelCall {
    fn validate(&self) -> Result<(), EnvironmentError> {
        if self.mechanism != "MODEL" {
            return Err(lead_approval_required(
                "an environment Lead decision must be a recorded Lead model call",
            ));
        }
        bounded_text("Lead model provider", &self.provider, 128)?;
        bounded_text("Lead model name", &self.model, 128)?;
        bounded_text("Lead model reservation", &self.reservation_id, 128)?;
        if !is_sha256(&self.request_sha256) {
            return Err(invalid("Lead model call request digest must be SHA-256"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentPlanReview {
    pub schema_version: u32,
    pub review_id: String,
    pub request_id: String,
    pub action_id: String,
    pub plan_id: String,
    pub plan_sha256: String,
    pub plan_revision: u32,
    pub risk_class: String,
    pub reviewer_role: EnvironmentReviewerRole,
    pub principal_id: String,
    pub recorded_by: String,
    pub decision: LeadReviewDecision,
    pub rationale: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub decided_at: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub model_call: EnvironmentReviewModelCall,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanContent<'a> {
    schema_version: u32,
    project_id: &'a str,
    request_id: &'a str,
    action_id: &'a str,
    campaign_id: &'a Option<String>,
    base_commit: &'a str,
    conventions_sha256: &'a str,
    risk_class: &'a str,
    revision: u32,
    source: EnvironmentPlanSource,
    toolchains: &'a [ToolchainRequirement],
    packages: &'a [PackageRequirement],
    services: &'a [ServiceRequirement],
    commands: &'a BTreeMap<String, CommandRequirement>,
    browsers: &'a [BrowserRequirement],
    files: &'a [EnvironmentFile],
    nix: &'a EnvironmentNixPolicy,
    resources: ResourceLimits,
    network: &'a NetworkPolicy,
    license_inventory: &'a LicenseInventory,
    resolution: &'a Option<EnvironmentResolution>,
}

impl EnvironmentPlan {
    /// Canonical content hash. IDs, timestamps, transient status, validation
    /// blockers, builder availability and the review record are excluded. The
    /// override plan binding is excluded too, otherwise an override would hash
    /// the plan hash it is bound to.
    pub fn compute_sha256(&self) -> String {
        let mut packages = self.packages.clone();
        for package in &mut packages {
            package.attribution = package.attribution.trim().to_owned();
        }
        let mut inventory = self.license_inventory.clone();
        for override_record in &mut inventory.overrides {
            override_record.plan_sha256.clear();
        }
        canonical_hash(&PlanContent {
            schema_version: self.schema_version,
            project_id: &self.project_id,
            request_id: &self.request_id,
            action_id: &self.action_id,
            campaign_id: &self.campaign_id,
            base_commit: &self.base_commit,
            conventions_sha256: &self.conventions_sha256,
            risk_class: &self.risk_class,
            revision: self.revision,
            source: self.source,
            toolchains: &self.toolchains,
            packages: &packages,
            services: &self.services,
            commands: &self.commands,
            browsers: &self.browsers,
            files: &self.files,
            nix: &self.nix,
            resources: self.resources,
            network: &self.network,
            license_inventory: &inventory,
            resolution: &self.resolution,
        })
    }

    pub fn refresh_hash(&mut self) {
        self.plan_sha256 = self.compute_sha256();
    }

    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid("unsupported environment plan schema version"));
        }
        bounded_text("plan id", &self.plan_id, 64)?;
        bounded_text("plan project id", &self.project_id, 200)?;
        bounded_text("plan request id", &self.request_id, 200)?;
        bounded_text("plan action id", &self.action_id, 200)?;
        if let Some(campaign_id) = &self.campaign_id {
            bounded_text("plan campaign id", campaign_id, 200)?;
        }
        if !is_commit_id(&self.base_commit) {
            return Err(invalid("plan base must be an exact commit id"));
        }
        if !is_sha256(&self.conventions_sha256) || !is_sha256(&self.plan_sha256) {
            return Err(invalid("plan hashes must be SHA-256"));
        }
        self.conventions.validate()?;
        if self.conventions.project_id != self.project_id
            || self.conventions.base_commit != self.base_commit
            || self.conventions.conventions_sha256 != self.conventions_sha256
        {
            return Err(hash_mismatch(
                "the plan conventions snapshot does not match the plan identity",
            ));
        }
        if self.revision == 0 {
            return Err(invalid("plan revision must be positive"));
        }
        if !matches!(self.risk_class.as_str(), "R0" | "R1" | "R2" | "R3") {
            return Err(plan_invalid(
                "the environment plan must bind a valid risk class",
            ));
        }
        if self.compute_sha256() != self.plan_sha256 {
            return Err(hash_mismatch("plan content does not match its plan hash"));
        }
        if self.toolchains.len() > MAX_TOOLCHAINS
            || self.packages.len() > MAX_PACKAGES
            || self.services.len() > MAX_SERVICES
            || self.commands.len() > MAX_COMMANDS
            || self.browsers.len() > MAX_BROWSERS
            || self.files.len() > 16
        {
            return Err(invalid("environment plan exceeds its bounds"));
        }
        self.resources.validate()?;
        self.network.validate()?;
        self.nix.sandbox.validate()?;
        self.builder.validate()?;
        self.validate_nixpkgs()?;
        self.validate_inputs()?;
        self.validate_toolchains()?;
        self.validate_packages()?;
        self.validate_services()?;
        self.validate_commands()?;
        self.validate_files()?;
        if self.nix.allow_unfree {
            return Err(license_blocked(
                "allowUnfree must remain false; an unfree package requires an exact owner override",
            ));
        }
        if self.license_inventory.allow_unfree {
            return Err(license_blocked(
                "the license inventory cannot enable a global allowUnfree switch",
            ));
        }
        if self.license_inventory_sha256 != self.license_inventory.compute_sha256(&self.plan_id) {
            return Err(hash_mismatch(
                "license inventory content does not match its inventory hash",
            ));
        }
        if !self.license_inventory.blockers.is_empty() {
            return Err(license_blocked(
                self.license_inventory
                    .blockers
                    .first()
                    .map(|blocker| blocker.message.clone())
                    .unwrap_or_else(|| "the license inventory is blocked".into()),
            ));
        }
        if !self.blockers.is_empty() {
            return Err(invalid(
                self.blockers
                    .first()
                    .map(|blocker| blocker.message.clone())
                    .unwrap_or_else(|| "the plan has an unresolved blocker".into()),
            ));
        }
        if let Some(review) = &self.lead_review {
            review.validate_for(self, now)?;
        }
        Ok(())
    }

    fn validate_nixpkgs(&self) -> Result<(), EnvironmentError> {
        validate_nixpkgs_pin(&self.nix.nixpkgs)
    }

    fn validate_inputs(&self) -> Result<(), EnvironmentError> {
        if self.nix.inputs.len() > MAX_INPUTS {
            return Err(invalid("Nix inputs exceed their bound"));
        }
        let mut names = BTreeSet::new();
        for input in &self.nix.inputs {
            bounded_text("Nix input name", &input.name, 64)?;
            if !names.insert(input.name.as_str()) {
                return Err(invalid("duplicate Nix input name"));
            }
            if input.name.eq_ignore_ascii_case("nixpkgs") {
                return Err(invalid(
                    "nixpkgs is declared by the plan policy, not as an extra input",
                ));
            }
            validate_pinned_nix_input(&input.url, &input.sha256)?;
        }
        Ok(())
    }

    fn validate_toolchains(&self) -> Result<(), EnvironmentError> {
        let mut ids = BTreeSet::new();
        for toolchain in &self.toolchains {
            bounded_text("toolchain id", &toolchain.id, 64)?;
            exact_version("toolchain version", &toolchain.version)?;
            if !ids.insert(toolchain.id.to_ascii_lowercase()) {
                return Err(invalid("duplicate toolchain id"));
            }
            if let Some(source) = Some(toolchain.source.as_str()).filter(|value| !value.is_empty())
            {
                bounded_text("toolchain source", source, 256)?;
            }
            for evidence in &toolchain.evidence {
                bounded_text("toolchain evidence", evidence, 256)?;
            }
        }
        Ok(())
    }

    fn validate_packages(&self) -> Result<(), EnvironmentError> {
        let mut names = BTreeSet::new();
        for package in &self.packages {
            bounded_text("package name", &package.name, 200)?;
            exact_version("package version", &package.version)?;
            bounded_text("package license", &package.license, 200)?;
            if !package.source.is_empty()
                && package.source != "nixpkgs"
                && package
                    .source
                    .strip_prefix("nixpkgs:")
                    .is_none_or(|attr| attr.trim().is_empty())
            {
                return Err(pin_missing(
                    "selected packages must come from the pinned nixpkgs input",
                ));
            }
            if !package.sha256.is_empty() && !is_sha256(&package.sha256) {
                return Err(pin_missing("package hash must be SHA-256"));
            }
            if !package.attr.is_empty() {
                bounded_text("package nix attribute", &package.attr, 128)?;
            }
            if !names.insert(package.name.to_ascii_lowercase()) {
                return Err(invalid("duplicate package name"));
            }
        }
        Ok(())
    }

    fn validate_services(&self) -> Result<(), EnvironmentError> {
        let mut names = BTreeSet::new();
        for service in &self.services {
            bounded_text("service name", &service.name, 200)?;
            exact_version("service version", &service.version)?;
            if !service.ephemeral {
                return Err(invalid("services must be ephemeral"));
            }
            if service.network != ServiceNetwork::Loopback {
                return Err(network_denied("services must be loopback-only"));
            }
            if !names.insert(service.name.to_ascii_lowercase()) {
                return Err(invalid("duplicate service name"));
            }
        }
        Ok(())
    }

    fn validate_commands(&self) -> Result<(), EnvironmentError> {
        for (name, command) in &self.commands {
            bounded_text("command name", name, 64)?;
            validate_argv(&command.argv)?;
            if command.cwd != "."
                && (command.cwd.starts_with('/')
                    || command.cwd.starts_with('~')
                    || command.cwd.split('/').any(|part| part == "..")
                    || command.cwd.contains('\\'))
            {
                return Err(invalid(
                    "command working directory must stay inside the project root",
                ));
            }
            if command.timeout_secs == 0
                || command.timeout_secs > self.resources.timeout_secs
                || command.timeout_secs > MAX_TIMEOUT_SECS
            {
                return Err(EnvironmentError::new(
                    EnvironmentBlockerCode::EnvironmentResourceLimit,
                    "command timeout is outside the plan's resource policy",
                ));
            }
            if command.expect_exit < 0 || command.expect_exit > 255 {
                return Err(invalid("expected exit status must be between 0 and 255"));
            }
        }
        Ok(())
    }

    fn validate_files(&self) -> Result<(), EnvironmentError> {
        for file in &self.files {
            bounded_text("environment file path", &file.path, 256)?;
            if file.path.starts_with('/')
                || file.path.contains('\\')
                || file.path.split('/').any(|part| part == "..")
            {
                return Err(invalid(
                    "environment file path must stay inside the project",
                ));
            }
            if !is_sha256(&file.sha256) {
                return Err(invalid("environment file hash must be SHA-256"));
            }
            if let Some(content) = &file.content {
                if content.len() > MAX_FILE_BYTES {
                    return Err(invalid("environment file exceeds its byte bound"));
                }
                if sha256_hex(content.as_bytes()) != file.sha256 {
                    return Err(hash_mismatch(
                        "environment file content does not match its hash",
                    ));
                }
                validate_generated_nix(&file.path, content)?;
            } else if self.source == EnvironmentPlanSource::Generated {
                return Err(pin_missing(
                    "a generated environment file must include its validated content",
                ));
            }
        }
        if self.source == EnvironmentPlanSource::ProjectCommitted
            && self
                .files
                .iter()
                .any(|file| file.origin == EnvironmentFileOrigin::Generated)
        {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentConventionsConflict,
                "the project already commits an environment; the compiler must not generate a competing one",
            ));
        }
        Ok(())
    }

    pub fn require_core_validated(&self) -> Result<(), EnvironmentError> {
        if !matches!(
            self.status,
            EnvironmentPlanStatus::CoreValidated
                | EnvironmentPlanStatus::LeadApproved
                | EnvironmentPlanStatus::Frozen
        ) {
            return Err(plan_invalid(
                "the environment plan has not passed Core validation",
            ));
        }
        Ok(())
    }

    pub fn require_lead_approval(&self) -> Result<&EnvironmentPlanReview, EnvironmentError> {
        let review = self
            .lead_review
            .as_ref()
            .ok_or_else(|| lead_approval_required("no Lead environment plan review is recorded"))?;
        if review.decision != LeadReviewDecision::Approved {
            return Err(lead_approval_required(
                "the Lead environment plan review is not APPROVED",
            ));
        }
        Ok(review)
    }
}

fn validate_argv(argv: &[String]) -> Result<(), EnvironmentError> {
    if argv.is_empty() || argv.len() > MAX_ARGV {
        return Err(invalid("a command requires a bounded argv vector"));
    }
    for value in argv {
        if value.is_empty()
            || value.len() > MAX_ARG_CHARS
            || value.contains('\0')
            || value.contains('\n')
            || value.chars().any(char::is_control)
        {
            return Err(invalid("command argv contains an invalid value"));
        }
        if contains_secret_shape(value) {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentSecretDetected,
                "command argv contains a secret-shaped value",
            ));
        }
    }
    Ok(())
}

fn validate_nixpkgs_pin(value: &str) -> Result<(), EnvironmentError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(pin_missing("the plan has no pinned nixpkgs input"));
    }
    let Some(rest) = value.strip_prefix("github:NixOS/nixpkgs/") else {
        return Err(pin_missing(
            "nixpkgs must be pinned as github:NixOS/nixpkgs/<exact-revision>",
        ));
    };
    if !is_commit_id(rest) {
        return Err(pin_missing(
            "nixpkgs must pin an exact 40/64 character revision",
        ));
    }
    Ok(())
}

fn validate_pinned_nix_input(url: &str, sha256: &str) -> Result<(), EnvironmentError> {
    if url.len() > 512
        || url.chars().any(char::is_whitespace)
        || url.chars().any(char::is_control)
        || !url.starts_with("github:")
    {
        return Err(pin_missing(
            "Nix inputs must use a pinned github: owner/repository/revision source",
        ));
    }
    let rest = &url["github:".len()..];
    let parts = rest.split('/').collect::<Vec<_>>();
    if parts.len() != 3 || !is_commit_id(parts[2]) {
        return Err(pin_missing(
            "Nix input revisions must be exact 40/64 character commit ids",
        ));
    }
    if !sha256.is_empty() && !is_sha256(sha256) {
        return Err(pin_missing("Nix input hash must be SHA-256"));
    }
    Ok(())
}

fn validate_generated_nix(path: &str, content: &str) -> Result<(), EnvironmentError> {
    let lower = content.to_ascii_lowercase();
    for token in [
        "builtins.exec",
        "--impure",
        "import <nixpkgs>",
        "allowunfree = true",
        "allowunfree=true",
        "sandbox = false",
        "sandbox=false",
        "builtins.getenv",
        "fetchgit",
        "fetchtarball",
        "cachix",
        "devenv.sh/cloud",
        "/var/run/docker.sock",
        "/run/podman/podman.sock",
        "/nix/var/nix/daemon-socket",
        "--privileged",
    ] {
        if lower.contains(token) {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentSandboxDenied,
                format!(
                    "generated environment file {path} contains a forbidden construct: {token}"
                ),
            ));
        }
    }
    if lower.contains("extra-experimental-features") && lower.contains("impure") {
        return Err(sandbox_denied(
            "generated environment files cannot enable impure evaluation",
        ));
    }
    if contains_secret_shape(content) {
        return Err(EnvironmentError::new(
            EnvironmentBlockerCode::EnvironmentSecretDetected,
            format!("generated environment file {path} contains a secret-shaped value"),
        ));
    }
    Ok(())
}

fn contains_secret_shape(value: &str) -> bool {
    static SECRET: std::sync::OnceLock<Vec<Regex>> = std::sync::OnceLock::new();
    let patterns = SECRET.get_or_init(|| {
        vec![
            Regex::new(r"AKIA[0-9A-Z]{16}").expect("AWS key regex"),
            Regex::new(r"gh[pousr]_[A-Za-z0-9_]{20,}").expect("GitHub token regex"),
            Regex::new(r"github_pat_[A-Za-z0-9_]{20,}").expect("GitHub PAT regex"),
            Regex::new(r"sk-[A-Za-z0-9]{20,}").expect("provider key regex"),
            Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").expect("private key regex"),
            Regex::new(r#"(?i)(password|secret|token)\s*=\s*['"]?[A-Za-z0-9_\-]{12,}"#)
                .expect("secret assignment regex"),
        ]
    });
    patterns.iter().any(|pattern| pattern.is_match(value))
}

// ---------------------------------------------------------------------------
// License gate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LicenseEntry {
    pub name: String,
    pub version: String,
    pub license: String,
    pub source: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub unfree: bool,
    #[serde(default)]
    pub owner_override_id: Option<String>,
    #[serde(default)]
    pub attribution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LicenseInventory {
    pub schema_version: u32,
    #[serde(default)]
    pub policy_revision: String,
    #[serde(default)]
    pub allow_unfree: bool,
    #[serde(default)]
    pub entries: Vec<LicenseEntry>,
    #[serde(default)]
    pub overrides: Vec<UnfreeOverride>,
    #[serde(default)]
    pub blockers: Vec<EnvironmentBlocker>,
    #[serde(default)]
    pub inventory_sha256: String,
    #[serde(default)]
    pub sbom_sha256: String,
}

impl Default for LicenseInventory {
    fn default() -> Self {
        Self {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            policy_revision: LICENSE_POLICY_REVISION.into(),
            allow_unfree: false,
            entries: Vec::new(),
            overrides: Vec::new(),
            blockers: Vec::new(),
            inventory_sha256: String::new(),
            sbom_sha256: String::new(),
        }
    }
}

impl LicenseInventory {
    pub fn compute_sha256(&self, plan_id: &str) -> String {
        let mut overrides = self.overrides.clone();
        for override_record in &mut overrides {
            override_record.plan_sha256.clear();
        }
        canonical_hash(&(
            ENVIRONMENT_SCHEMA_VERSION,
            LICENSE_POLICY_REVISION,
            plan_id,
            false,
            &self.entries,
            &overrides,
        ))
    }

    pub fn compute_sbom_sha256(&self) -> String {
        canonical_hash(&self.entries)
    }

    pub fn refresh_hashes(&mut self, plan_id: &str) {
        self.inventory_sha256 = self.compute_sha256(plan_id);
        self.sbom_sha256 = self.compute_sbom_sha256();
    }
}

const ALLOWED_SPDX: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "ISC",
    "LGPL-2.1-only",
    "LGPL-2.1-or-later",
    "LGPL-3.0-only",
    "LGPL-3.0-or-later",
    "MIT",
    "MIT-0",
    "MPL-2.0",
    "Unlicense",
    "Zlib",
];

const DENIED_SPDX: &[&str] = &[
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "BUSL-1.1",
    "Commons-Clause",
    "SSPL-1.0",
];

fn validate_license_expression(value: &str) -> Result<(), EnvironmentError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(license_blocked(
            "a selected package has no license expression",
        ));
    }
    if matches!(value, "NONE" | "NOASSERTION" | "UNLICENSED" | "UNKNOWN") {
        return Err(license_blocked(format!(
            "the license expression {value} is unknown and blocks the plan"
        )));
    }
    if DENIED_SPDX.contains(&value) {
        return Err(license_blocked(format!(
            "the license expression {value} is denied by the frozen policy"
        )));
    }
    if ALLOWED_SPDX.contains(&value) {
        return Ok(());
    }
    // Permit simple SPDX AND/OR expressions where every atom is known.
    let normalized = value
        .replace(['(', ')'], " ")
        .replace(" AND ", " ")
        .replace(" OR ", " ")
        .replace(" WITH ", " ");
    let atoms = normalized.split_whitespace().collect::<Vec<_>>();
    if !atoms.is_empty()
        && atoms
            .iter()
            .all(|atom| ALLOWED_SPDX.contains(atom) || atom.starts_with("LLVM-exception"))
    {
        return Ok(());
    }
    Err(license_blocked(format!(
        "the license expression {value} is unknown and blocks the plan"
    )))
}

fn substrate_license_entries() -> Vec<LicenseEntry> {
    vec![
        LicenseEntry {
            name: "nix".into(),
            version: "pinned by builder receipt".into(),
            license: "LGPL-2.1-only".into(),
            source: "https://github.com/NixOS/nix/blob/master/COPYING".into(),
            sha256: String::new(),
            unfree: false,
            owner_override_id: None,
            attribution: "Nix is invoked as a separate LGPL-2.1 tool; Factory does not link Nix libraries. Preserve the license, notices and source/relink offer when distributing a modified Nix binary.".into(),
        },
        LicenseEntry {
            name: "nixpkgs".into(),
            version: "pinned by the environment plan".into(),
            license: "MIT".into(),
            source: "https://github.com/NixOS/nixpkgs/blob/master/COPYING".into(),
            sha256: String::new(),
            unfree: false,
            owner_override_id: None,
            attribution: "Copyright 2003-2026 Eelco Dolstra and the Nixpkgs/NixOS contributors. Preserve the MIT copyright and permission notice. Each selected package retains its own license.".into(),
        },
        LicenseEntry {
            name: "devenv".into(),
            version: "pinned by builder receipt".into(),
            license: "Apache-2.0".into(),
            source: "https://github.com/cachix/devenv/blob/main/LICENSE".into(),
            sha256: String::new(),
            unfree: false,
            owner_override_id: None,
            attribution: "Apache License 2.0. Retain notices and any NOTICE text, mark modified files and comply with the patent-termination terms. The repository LICENSE and Cargo.toml are authoritative.".into(),
        },
    ]
}

fn build_license_inventory(
    plan: &mut EnvironmentPlan,
    overrides: &[UnfreeOverride],
    now: DateTime<Utc>,
) -> Result<(), EnvironmentError> {
    let mut inventory = LicenseInventory {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        policy_revision: LICENSE_POLICY_REVISION.into(),
        allow_unfree: false,
        entries: substrate_license_entries(),
        overrides: overrides.to_vec(),
        blockers: Vec::new(),
        inventory_sha256: String::new(),
        sbom_sha256: String::new(),
    };
    if plan.nix.allow_unfree {
        inventory.blockers.push(EnvironmentBlocker {
            code: EnvironmentBlockerCode::EnvironmentLicenseBlocked,
            message:
                "allowUnfree must remain false; an unfree package requires an exact owner override"
                    .into(),
        });
    }
    for package in &plan.packages {
        if let Err(error) = validate_license_expression(&package.license) {
            inventory.blockers.push(EnvironmentBlocker {
                code: EnvironmentBlockerCode::EnvironmentLicenseBlocked,
                message: format!("{} {}: {}", package.name, package.version, error.message),
            });
            continue;
        }
        let mut owner_override_id = None;
        if package.unfree {
            let matching = overrides.iter().find(|value| {
                value.package.eq_ignore_ascii_case(&package.name)
                    && value.version == package.version
                    && value.license == package.license
                    && value.reason.trim().len() >= 8
                    && !value.decision_id.trim().is_empty()
            });
            let Some(override_record) = matching else {
                inventory.blockers.push(EnvironmentBlocker {
                    code: EnvironmentBlockerCode::EnvironmentLicenseBlocked,
                    message: format!(
                        "{} {} is unfree and has no exact authenticated owner override",
                        package.name, package.version
                    ),
                });
                continue;
            };
            let expires = DateTime::parse_from_rfc3339(&override_record.expires_at)
                .map_err(|_| license_blocked("an owner override expiry is not RFC3339"))?;
            if expires.with_timezone(&Utc) <= now {
                inventory.blockers.push(EnvironmentBlocker {
                    code: EnvironmentBlockerCode::EnvironmentLicenseBlocked,
                    message: format!(
                        "{} {} owner override expired at {}",
                        package.name, package.version, override_record.expires_at
                    ),
                });
                continue;
            }
            owner_override_id = Some(override_record.id.clone());
        }
        inventory.entries.push(LicenseEntry {
            name: package.name.clone(),
            version: package.version.clone(),
            license: package.license.clone(),
            source: if package.source.is_empty() {
                plan.nix.nixpkgs.clone()
            } else {
                package.source.clone()
            },
            sha256: package.sha256.clone(),
            unfree: package.unfree,
            owner_override_id,
            attribution: package.attribution.clone(),
        });
    }
    inventory.refresh_hashes(&plan.plan_id);
    plan.license_inventory = inventory;
    plan.license_inventory_sha256 = plan.license_inventory.inventory_sha256.clone();
    Ok(())
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

pub struct CompileEnvironmentInput<'a> {
    pub plan_id: String,
    pub project_id: &'a str,
    pub request_id: &'a str,
    pub action_id: &'a str,
    pub campaign_id: Option<&'a str>,
    pub revision: u32,
    pub risk_class: &'a str,
    pub conventions: &'a ProjectConventions,
    pub requirements: &'a EnvironmentRequirements,
    pub builder: EnvironmentBuilderDescriptor,
    pub now: DateTime<Utc>,
}

/// Deterministically combine explicit owner constraints, committed pins,
/// version files/CI and an optional untrusted proposal. Precedence is explicit
/// owner > committed environment > version pin/CI > manifest inference >
/// proposal. There is no language default.
pub fn derive_environment_requirements(
    conventions: &ProjectConventions,
    requirements: &EnvironmentRequirements,
) -> Result<EnvironmentRequirements, EnvironmentError> {
    conventions.validate()?;
    if requirements.schema_version != ENVIRONMENT_SCHEMA_VERSION {
        return Err(invalid(
            "unsupported environment requirements schema version",
        ));
    }
    if requirements.allow_unfree {
        return Err(license_blocked(
            "allowUnfree cannot be enabled globally; use an exact owner override",
        ));
    }
    let explicit = &requirements.explicit_constraints;
    let proposal = requirements.proposal.as_ref();
    let mut toolchains: BTreeMap<String, ToolchainRequirement> = BTreeMap::new();
    let mut commands: BTreeMap<String, CommandRequirement> = BTreeMap::new();
    let mut nixpkgs = explicit.nixpkgs.clone();
    let mut packages = explicit.packages.clone();
    let mut services = explicit.services.clone();
    let mut inputs = explicit.inputs.clone();
    let mut browsers = explicit.browsers.clone();
    let mut generated_files = explicit.files.clone();

    for toolchain in &explicit.toolchains {
        toolchains.insert(toolchain.id.to_ascii_lowercase(), toolchain.clone());
    }
    // Committed version pins are more specific than manifest inference.
    for pin in conventions.files.iter().flat_map(|file| file.pins.iter()) {
        if pin.name == "nixpkgs" {
            if nixpkgs.is_none() {
                nixpkgs = Some(pin.value.clone());
            }
            continue;
        }
        if toolchains.contains_key(&pin.name) {
            continue;
        }
        if pin.value.trim().is_empty() {
            continue;
        }
        exact_version("committed convention version", &pin.value)?;
        toolchains.insert(
            pin.name.clone(),
            ToolchainRequirement {
                id: pin.name.clone(),
                version: pin.value.clone(),
                source: pin.source.clone(),
                evidence: vec![pin.source.clone()],
            },
        );
    }
    // Manifest inference supplies language identity, never a version. An
    // existing committed environment may leave the toolchain list empty.
    let mut manifest_languages = BTreeSet::new();
    for file in &conventions.files {
        if file.kind != ConventionFileKind::Manifest {
            continue;
        }
        let name = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&file.path)
            .to_ascii_lowercase();
        let language = match name.as_str() {
            "cargo.toml" => "rust",
            "package.json" => "node",
            "go.mod" => "go",
            "pyproject.toml" | "setup.py" | "setup.cfg" | "requirements.txt" | "pipfile" => {
                "python"
            }
            "pom.xml" | "build.gradle" | "build.gradle.kts" => "java",
            "composer.json" => "php",
            "gemfile" => "ruby",
            "mix.exs" => "elixir",
            "pubspec.yaml" => "dart",
            "package.swift" => "swift",
            "cmakelists.txt" => "cmake",
            _ if name.ends_with(".csproj")
                || name.ends_with(".fsproj")
                || name.ends_with(".vbproj")
                || name.ends_with(".sln") =>
            {
                "dotnet"
            }
            _ => continue,
        };
        manifest_languages.insert(language.to_owned());
    }
    if let Some(proposal) = proposal {
        for toolchain in &proposal.toolchains {
            if toolchains.contains_key(&toolchain.id.to_ascii_lowercase()) {
                continue;
            }
            toolchains.insert(toolchain.id.to_ascii_lowercase(), toolchain.clone());
        }
        if nixpkgs.is_none() {
            nixpkgs = proposal.nixpkgs.clone();
        }
        if packages.is_empty() {
            packages = proposal.packages.clone();
        }
        if services.is_empty() {
            services = proposal.services.clone();
        }
        if inputs.is_empty() {
            inputs = proposal.inputs.clone();
        }
        if browsers.is_empty() {
            browsers = proposal.browsers.clone();
        }
        if generated_files.is_empty() {
            generated_files = proposal.files.clone();
        }
        for (name, command) in &proposal.commands {
            commands
                .entry(name.clone())
                .or_insert_with(|| command.clone());
        }
    }
    for command in &conventions.commands {
        commands
            .entry(command.name.clone())
            .or_insert_with(|| CommandRequirement {
                argv: command.argv.clone(),
                cwd: command.cwd.clone(),
                timeout_secs: default_command_timeout(),
                expect_exit: 0,
            });
    }
    for (name, command) in &explicit.commands {
        commands.insert(name.clone(), command.clone());
    }
    if conventions.existing_environment == ExistingEnvironmentKind::None {
        let missing = manifest_languages
            .iter()
            .filter(|language| !toolchains.contains_key(language.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(pin_missing(format!(
                "the project has {} conventions but no exact pinned version; provide an explicit environment requirement instead of silently selecting a toolchain",
                missing.join(", ")
            )));
        }
    }
    let Some(nixpkgs) = nixpkgs else {
        return Err(pin_missing(
            "the environment plan requires a committed or explicit pinned nixpkgs input",
        ));
    };
    if !nixpkgs.starts_with("github:NixOS/nixpkgs/") {
        return Err(pin_missing(
            "nixpkgs must be pinned as github:NixOS/nixpkgs/<exact-revision>",
        ));
    }
    let mut values = EnvironmentRequirements {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        explicit_constraints: ExplicitEnvironmentConstraints {
            toolchains: toolchains.into_values().collect(),
            packages,
            services,
            commands,
            nixpkgs: Some(nixpkgs),
            inputs,
            browsers,
            allow_unfree: false,
            files: generated_files,
        },
        proposal: None,
        allow_unfree: false,
        owner_overrides: requirements.owner_overrides.clone(),
        resource_limits: requirements.resource_limits,
        network: requirements.network.clone(),
    };
    values
        .explicit_constraints
        .toolchains
        .sort_by(|left, right| left.id.cmp(&right.id));
    values
        .explicit_constraints
        .packages
        .sort_by(|left, right| left.name.cmp(&right.name));
    values
        .explicit_constraints
        .services
        .sort_by(|left, right| left.name.cmp(&right.name));
    values
        .explicit_constraints
        .browsers
        .sort_by(|left, right| left.name.cmp(&right.name));
    Ok(values)
}

pub fn compile_environment_plan(
    input: CompileEnvironmentInput<'_>,
) -> Result<EnvironmentPlan, EnvironmentError> {
    bounded_text("plan id", &input.plan_id, 64)?;
    if input.revision == 0 {
        return Err(invalid("plan revision must be positive"));
    }
    input.conventions.validate()?;
    if input.conventions.project_id != input.project_id {
        return Err(hash_mismatch(
            "the conventions snapshot belongs to a different project",
        ));
    }
    let requirements = derive_environment_requirements(input.conventions, input.requirements)?;
    let explicit = &requirements.explicit_constraints;
    let source = if input.conventions.existing_environment.is_present() {
        EnvironmentPlanSource::ProjectCommitted
    } else {
        EnvironmentPlanSource::Generated
    };
    if source == EnvironmentPlanSource::ProjectCommitted {
        let committed = input
            .conventions
            .pins_named("nixpkgs")
            .into_iter()
            .map(|pin| pin.value.as_str())
            .collect::<BTreeSet<_>>();
        if let Some(explicit_nixpkgs) = explicit.nixpkgs.as_deref()
            && !committed.is_empty()
            && !committed.contains(explicit_nixpkgs)
        {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentConventionsConflict,
                "an explicit nixpkgs constraint differs from the committed environment; commit a new pin or request a new environment revision",
            ));
        }
    }
    let mut files = committed_environment_files(input.conventions);
    if source == EnvironmentPlanSource::Generated {
        let generated = if explicit.files.is_empty() {
            generate_environment_files(explicit)?
        } else {
            explicit.files.clone()
        };
        for (path, content) in generated {
            files.push(EnvironmentFile {
                path,
                sha256: sha256_hex(content.as_bytes()),
                origin: EnvironmentFileOrigin::Generated,
                content: Some(content),
            });
        }
    }
    let nixpkgs = explicit
        .nixpkgs
        .clone()
        .ok_or_else(|| pin_missing("the environment plan has no pinned nixpkgs input"))?;
    let mut plan = EnvironmentPlan {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        plan_id: input.plan_id,
        project_id: input.project_id.into(),
        request_id: input.request_id.into(),
        action_id: input.action_id.into(),
        campaign_id: input.campaign_id.map(str::to_owned),
        base_commit: input.conventions.base_commit.clone(),
        conventions_sha256: input.conventions.conventions_sha256.clone(),
        conventions: input.conventions.clone(),
        risk_class: input.risk_class.to_owned(),
        status: EnvironmentPlanStatus::Draft,
        revision: input.revision,
        source,
        toolchains: explicit.toolchains.clone(),
        packages: explicit.packages.clone(),
        services: explicit.services.clone(),
        commands: explicit.commands.clone(),
        browsers: explicit.browsers.clone(),
        files,
        nix: EnvironmentNixPolicy {
            nixpkgs,
            inputs: explicit.inputs.clone(),
            allow_unfree: false,
            sandbox: SandboxPolicy::default(),
        },
        resources: requirements.resource_limits,
        network: requirements.network.clone(),
        license_inventory: LicenseInventory::default(),
        license_inventory_sha256: String::new(),
        builder: input.builder,
        resolution: None,
        lead_review: None,
        blockers: Vec::new(),
        commands_rationale: if explicit.commands.is_empty() {
            "No build/test command was committed. Core will not invent a language-specific command; the Quality Profile remains responsible for required check kinds.".into()
        } else {
            "Commands come from committed CI or an explicit owner requirement.".into()
        },
        plan_sha256: String::new(),
        created_at: input.now.to_rfc3339(),
    };
    plan.refresh_hash();
    build_license_inventory(&mut plan, &requirements.owner_overrides, input.now)?;
    // The license inventory is part of plan content. Recompute once after the
    // inventory hash is attached, then bind each override to the resulting
    // plan hash. The override binding is excluded from the content hash.
    plan.refresh_hash();
    for override_record in &mut plan.license_inventory.overrides {
        if override_record.plan_sha256.is_empty() {
            override_record.plan_sha256 = plan.plan_sha256.clone();
        } else if override_record.plan_sha256 != plan.plan_sha256 {
            return Err(hash_mismatch(
                "an owner override is bound to a different environment plan",
            ));
        }
    }
    plan.license_inventory.refresh_hashes(&plan.plan_id);
    plan.license_inventory_sha256 = plan.license_inventory.inventory_sha256.clone();
    plan.refresh_hash();
    plan.validate(input.now)?;
    plan.status = EnvironmentPlanStatus::CoreValidated;
    Ok(plan)
}

fn committed_environment_files(conventions: &ProjectConventions) -> Vec<EnvironmentFile> {
    conventions
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.kind,
                ConventionFileKind::FlakeNix
                    | ConventionFileKind::FlakeLock
                    | ConventionFileKind::DevenvNix
                    | ConventionFileKind::DevenvYaml
                    | ConventionFileKind::DevenvLock
                    | ConventionFileKind::Devcontainer
            )
        })
        .map(|file| EnvironmentFile {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            origin: EnvironmentFileOrigin::Project,
            content: None,
        })
        .collect()
}

fn generate_environment_files(
    explicit: &ExplicitEnvironmentConstraints,
) -> Result<BTreeMap<String, String>, EnvironmentError> {
    let nixpkgs = explicit
        .nixpkgs
        .as_deref()
        .ok_or_else(|| pin_missing("a generated environment requires a pinned nixpkgs input"))?;
    let mut attributes = Vec::new();
    for package in &explicit.packages {
        let attr = if !package.attr.trim().is_empty() {
            package.attr.trim()
        } else {
            package.name.trim()
        };
        if !attr.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        }) {
            return Err(invalid(
                "a generated environment package attribute is not a Nix identifier",
            ));
        }
        attributes.push(attr.to_owned());
    }
    attributes.sort();
    attributes.dedup();
    let build_inputs = attributes
        .iter()
        .map(|attr| format!("pkgs.{attr}"))
        .collect::<Vec<_>>()
        .join(" ");
    let flake = format!(
        "{{\n  description = \"Factory frozen environment\";\n  inputs.nixpkgs.url = \"{nixpkgs}\";\n  outputs = {{ self, nixpkgs }}:\n    let\n      system = \"{DEFAULT_PLATFORM}\";\n      pkgs = import nixpkgs {{ inherit system; config.allowUnfree = false; }};\n    in {{\n      devShells.${{system}}.default = pkgs.mkShell {{ buildInputs = [ {build_inputs} ]; }};\n    }};\n}}\n"
    );
    let devenv = format!("{{ pkgs, ... }}: {{\n  packages = [ {build_inputs} ];\n}}\n");
    let mut files = BTreeMap::new();
    files.insert("flake.nix".into(), flake);
    files.insert("devenv.nix".into(), devenv);
    Ok(files)
}

// ---------------------------------------------------------------------------
// Lead review and freeze
// ---------------------------------------------------------------------------

impl EnvironmentPlanReview {
    pub fn validate_for(
        &self,
        plan: &EnvironmentPlan,
        now: DateTime<Utc>,
    ) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid("unsupported environment review schema version"));
        }
        bounded_text("environment review id", &self.review_id, 64)?;
        bounded_text("environment review request id", &self.request_id, 200)?;
        if self.request_id != plan.request_id || self.action_id != plan.action_id {
            return Err(hash_mismatch(
                "the environment review belongs to a different request or Action",
            ));
        }
        if self.plan_id != plan.plan_id
            || self.plan_sha256 != plan.plan_sha256
            || self.plan_revision != plan.revision
            || self.risk_class != plan.risk_class
        {
            return Err(hash_mismatch(
                "the environment review does not bind the current plan revision, hash and risk",
            ));
        }
        self.model_call.validate()?;
        bounded_list("environment review constraints", &self.constraints, 16)?;
        if self.reviewer_role != EnvironmentReviewerRole::Lead {
            return Err(lead_approval_required(
                "only a LEAD planning decision can approve an environment plan",
            ));
        }
        bounded_text(
            "environment review principal",
            &self.principal_id,
            MAX_PRINCIPAL_CHARS,
        )?;
        if !self.principal_id.starts_with(LEAD_PRINCIPAL_PREFIX) {
            return Err(lead_approval_required(
                "the environment review principal must be a Lead session identity",
            ));
        }
        bounded_text(
            "environment review recorder",
            &self.recorded_by,
            MAX_PRINCIPAL_CHARS,
        )?;
        bounded_text(
            "environment review rationale",
            &self.rationale,
            MAX_REVIEW_RATIONALE_CHARS,
        )?;
        let decided_at = DateTime::parse_from_rfc3339(&self.decided_at)
            .map_err(|_| invalid("environment review decision time is not RFC3339"))?;
        if decided_at.with_timezone(&Utc) > now + chrono::Duration::minutes(5) {
            return Err(invalid("environment review decision time is in the future"));
        }
        Ok(())
    }
}

pub struct RecordLeadReviewInput<'a> {
    pub review_id: String,
    pub principal_id: &'a str,
    pub recorded_by: &'a str,
    pub decision: LeadReviewDecision,
    pub rationale: &'a str,
    pub constraints: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub model_call: EnvironmentReviewModelCall,
    pub now: DateTime<Utc>,
}

pub fn record_lead_review(
    plan: &mut EnvironmentPlan,
    input: RecordLeadReviewInput<'_>,
) -> Result<EnvironmentPlanReview, EnvironmentError> {
    plan.validate(input.now)?;
    plan.require_core_validated()?;
    if plan.status == EnvironmentPlanStatus::Blocked {
        return Err(plan_invalid(
            "a blocked environment plan cannot be approved by a model or Lead",
        ));
    }
    let review = EnvironmentPlanReview {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        review_id: input.review_id,
        request_id: plan.request_id.clone(),
        action_id: plan.action_id.clone(),
        plan_id: plan.plan_id.clone(),
        plan_sha256: plan.plan_sha256.clone(),
        plan_revision: plan.revision,
        risk_class: plan.risk_class.clone(),
        reviewer_role: EnvironmentReviewerRole::Lead,
        principal_id: input.principal_id.into(),
        recorded_by: input.recorded_by.into(),
        decision: input.decision,
        rationale: input.rationale.into(),
        constraints: input.constraints,
        decided_at: input.now.to_rfc3339(),
        evidence_refs: input.evidence_refs,
        model_call: input.model_call,
    };
    review.validate_for(plan, input.now)?;
    plan.lead_review = Some(review.clone());
    plan.status = match input.decision {
        LeadReviewDecision::Approved => EnvironmentPlanStatus::LeadApproved,
        LeadReviewDecision::ChangesRequested | LeadReviewDecision::Rejected => {
            EnvironmentPlanStatus::Blocked
        }
    };
    Ok(review)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentSpec {
    pub schema_version: u32,
    pub spec_id: String,
    pub spec_sha256: String,
    pub project_id: String,
    pub request_id: String,
    pub action_id: String,
    #[serde(default)]
    pub campaign_id: Option<String>,
    pub base_commit: String,
    pub plan_id: String,
    pub plan_sha256: String,
    pub conventions_sha256: String,
    pub review_id: String,
    pub platform: String,
    pub nix_version: String,
    pub devenv_version: String,
    pub flake_lock_sha256: String,
    pub devenv_lock_sha256: String,
    pub license_inventory_sha256: String,
    pub sbom_sha256: String,
    pub builder_image: String,
    pub builder_image_digest: String,
    pub resource_policy: ResourceLimits,
    pub network_policy: NetworkPolicy,
    pub status: EnvironmentSpecStatus,
    pub frozen_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentSpecStatus {
    Frozen,
    Superseded,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpecContent<'a> {
    schema_version: u32,
    project_id: &'a str,
    request_id: &'a str,
    action_id: &'a str,
    campaign_id: &'a Option<String>,
    base_commit: &'a str,
    plan_id: &'a str,
    plan_sha256: &'a str,
    conventions_sha256: &'a str,
    review_id: &'a str,
    platform: &'a str,
    nix_version: &'a str,
    devenv_version: &'a str,
    flake_lock_sha256: &'a str,
    devenv_lock_sha256: &'a str,
    license_inventory_sha256: &'a str,
    sbom_sha256: &'a str,
    builder_image: &'a str,
    builder_image_digest: &'a str,
    resource_policy: ResourceLimits,
    network_policy: &'a NetworkPolicy,
}

impl EnvironmentSpec {
    pub fn compute_sha256(&self) -> String {
        canonical_hash(&SpecContent {
            schema_version: self.schema_version,
            project_id: &self.project_id,
            request_id: &self.request_id,
            action_id: &self.action_id,
            campaign_id: &self.campaign_id,
            base_commit: &self.base_commit,
            plan_id: &self.plan_id,
            plan_sha256: &self.plan_sha256,
            conventions_sha256: &self.conventions_sha256,
            review_id: &self.review_id,
            platform: &self.platform,
            nix_version: &self.nix_version,
            devenv_version: &self.devenv_version,
            flake_lock_sha256: &self.flake_lock_sha256,
            devenv_lock_sha256: &self.devenv_lock_sha256,
            license_inventory_sha256: &self.license_inventory_sha256,
            sbom_sha256: &self.sbom_sha256,
            builder_image: &self.builder_image,
            builder_image_digest: &self.builder_image_digest,
            resource_policy: self.resource_policy,
            network_policy: &self.network_policy,
        })
    }

    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid("unsupported environment spec schema version"));
        }
        bounded_text("spec id", &self.spec_id, 64)?;
        if !is_sha256(&self.spec_sha256)
            || !is_sha256(&self.plan_sha256)
            || !is_sha256(&self.conventions_sha256)
            || !is_sha256(&self.license_inventory_sha256)
            || !is_sha256(&self.sbom_sha256)
        {
            return Err(invalid("environment spec hashes must be SHA-256"));
        }
        if self.compute_sha256() != self.spec_sha256 {
            return Err(hash_mismatch(
                "environment spec content does not match its hash",
            ));
        }
        if self.status != EnvironmentSpecStatus::Frozen {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentNotFrozen,
                "the environment spec is not FROZEN",
            ));
        }
        if self.flake_lock_sha256.is_empty() && self.devenv_lock_sha256.is_empty() {
            return Err(pin_missing(
                "a frozen environment spec requires a committed flake or devenv lock hash",
            ));
        }
        for (name, value) in [
            ("flake lock", self.flake_lock_sha256.as_str()),
            ("devenv lock", self.devenv_lock_sha256.as_str()),
        ] {
            if !value.is_empty() && !is_sha256(value) {
                return Err(pin_missing(format!("{name} hash must be SHA-256")));
            }
        }
        if self.nix_version.trim().is_empty() || self.devenv_version.trim().is_empty() {
            return Err(pin_missing(
                "a frozen environment spec requires pinned Nix and devenv versions",
            ));
        }
        if !self.builder_image.is_empty() && !is_digest(&self.builder_image_digest) {
            return Err(invalid(
                "a builder image must be pinned by a sha256 digest in a frozen spec",
            ));
        }
        self.resource_policy.validate()?;
        self.network_policy.validate()?;
        Ok(())
    }
}

pub struct FreezeEnvironmentInput<'a> {
    pub spec_id: String,
    pub nix_version: &'a str,
    pub devenv_version: &'a str,
    pub now: DateTime<Utc>,
}

pub fn freeze_environment_spec(
    plan: &EnvironmentPlan,
    review: &EnvironmentPlanReview,
    input: FreezeEnvironmentInput<'_>,
) -> Result<EnvironmentSpec, EnvironmentError> {
    plan.validate(input.now)?;
    plan.require_core_validated()?;
    if plan.status != EnvironmentPlanStatus::LeadApproved {
        return Err(lead_approval_required(
            "Core cannot freeze an environment plan without a bound Lead APPROVED review",
        ));
    }
    review.validate_for(plan, input.now)?;
    if review.decision != LeadReviewDecision::Approved {
        return Err(lead_approval_required(
            "the Lead environment review did not approve the plan",
        ));
    }
    let Some(lead_review) = &plan.lead_review else {
        return Err(lead_approval_required(
            "the plan does not carry its Lead review record",
        ));
    };
    if lead_review.review_id != review.review_id
        || lead_review.plan_sha256 != plan.plan_sha256
        || lead_review.decision != LeadReviewDecision::Approved
    {
        return Err(lead_approval_required(
            "the plan's Lead review record does not match the approving review",
        ));
    }
    let flake_lock_sha256 = plan
        .files
        .iter()
        .find(|file| file.path.eq_ignore_ascii_case("flake.lock"))
        .map(|file| file.sha256.clone())
        .unwrap_or_default();
    let devenv_lock_sha256 = plan
        .files
        .iter()
        .find(|file| file.path.eq_ignore_ascii_case("devenv.lock"))
        .map(|file| file.sha256.clone())
        .unwrap_or_default();
    if flake_lock_sha256.is_empty() && devenv_lock_sha256.is_empty() {
        return Err(pin_missing(
            "the environment has no committed flake/devenv lock; the builder must resolve and pin it before freeze",
        ));
    }
    let mut spec = EnvironmentSpec {
        schema_version: ENVIRONMENT_SCHEMA_VERSION,
        spec_id: input.spec_id,
        spec_sha256: String::new(),
        project_id: plan.project_id.clone(),
        request_id: plan.request_id.clone(),
        action_id: plan.action_id.clone(),
        campaign_id: plan.campaign_id.clone(),
        base_commit: plan.base_commit.clone(),
        plan_id: plan.plan_id.clone(),
        plan_sha256: plan.plan_sha256.clone(),
        conventions_sha256: plan.conventions_sha256.clone(),
        review_id: review.review_id.clone(),
        platform: plan.builder.platform.clone(),
        nix_version: input.nix_version.into(),
        devenv_version: input.devenv_version.into(),
        flake_lock_sha256,
        devenv_lock_sha256,
        license_inventory_sha256: plan.license_inventory.inventory_sha256.clone(),
        sbom_sha256: plan.license_inventory.sbom_sha256.clone(),
        builder_image: plan.builder.image.clone(),
        builder_image_digest: plan.builder.digest.clone(),
        resource_policy: plan.resources,
        network_policy: plan.network.clone(),
        status: EnvironmentSpecStatus::Frozen,
        frozen_at: input.now.to_rfc3339(),
    };
    spec.spec_sha256 = spec.compute_sha256();
    spec.validate()?;
    Ok(spec)
}

// ---------------------------------------------------------------------------
// Builder port and check-runner selection (implemented by WP1-B)
// ---------------------------------------------------------------------------

pub type EnvironmentBuilderFuture<'a> = Pin<
    Box<
        dyn std::future::Future<Output = Result<EnvironmentBuildReceipt, EnvironmentError>>
            + Send
            + 'a,
    >,
>;

pub type EnvironmentResolutionFuture<'a> = Pin<
    Box<
        dyn std::future::Future<Output = Result<EnvironmentResolution, EnvironmentError>>
            + Send
            + 'a,
    >,
>;

/// Builder port for the Nix/devenv image and Node/Playwright runtime. The
/// builder runs outside Core with no Factory/provider/GitHub credentials, no
/// host network and no host socket. It receives an already frozen spec and must
/// return evidence bound to that exact hash.
pub trait EnvironmentBuilder {
    fn descriptor(&self) -> EnvironmentBuilderDescriptor;

    /// Resolve and pin a generated environment's lock data before freeze.
    /// Existing committed environments with a lock may skip this step.
    fn resolve<'a>(&'a self, plan: &'a EnvironmentPlan) -> EnvironmentResolutionFuture<'a>;

    fn build<'a>(
        &'a self,
        spec: &'a EnvironmentSpec,
        plan: &'a EnvironmentPlan,
    ) -> EnvironmentBuilderFuture<'a>;
}

pub type SharedEnvironmentBuilder = std::sync::Arc<dyn EnvironmentBuilder + Send + Sync>;

/// The default Core builder is truthful: until WP1-B supplies a digest-pinned
/// builder, generated environments cannot resolve locks or produce a receipt.
pub struct UnavailableEnvironmentBuilder;

impl EnvironmentBuilder for UnavailableEnvironmentBuilder {
    fn descriptor(&self) -> EnvironmentBuilderDescriptor {
        EnvironmentBuilderDescriptor::default()
    }

    fn resolve<'a>(&'a self, _plan: &'a EnvironmentPlan) -> EnvironmentResolutionFuture<'a> {
        Box::pin(async {
            Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentBuilderUnavailable,
                "no environment builder is configured; the generated environment cannot be resolved or frozen",
            ))
        })
    }

    fn build<'a>(
        &'a self,
        _spec: &'a EnvironmentSpec,
        _plan: &'a EnvironmentPlan,
    ) -> EnvironmentBuilderFuture<'a> {
        Box::pin(async {
            Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentBuilderUnavailable,
                "no environment builder is configured; no build receipt can be produced",
            ))
        })
    }
}

pub fn unavailable_environment_builder() -> SharedEnvironmentBuilder {
    std::sync::Arc::new(UnavailableEnvironmentBuilder)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentResolution {
    pub schema_version: u32,
    #[serde(default)]
    pub files: Vec<EnvironmentFile>,
    pub nix_version: String,
    pub devenv_version: String,
}

impl EnvironmentResolution {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid("unsupported environment resolution schema version"));
        }
        bounded_text("resolved Nix version", &self.nix_version, 64)?;
        bounded_text("resolved devenv version", &self.devenv_version, 64)?;
        if self.files.is_empty() {
            return Err(pin_missing(
                "the environment builder returned no lock files",
            ));
        }
        for file in &self.files {
            if !matches!(file.path.as_str(), "flake.lock" | "devenv.lock") {
                return Err(invalid(
                    "the environment builder may only resolve flake.lock or devenv.lock",
                ));
            }
            if !is_sha256(&file.sha256) {
                return Err(invalid("resolved lock hash must be SHA-256"));
            }
            if let Some(content) = &file.content
                && sha256_hex(content.as_bytes()) != file.sha256
            {
                return Err(hash_mismatch(
                    "resolved lock content does not match its hash",
                ));
            }
        }
        Ok(())
    }
}

/// Exact plan/spec/receipt set used by a Worker assignment or verifier
/// inspection. It is loaded from the Campaign binding, never from check text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentExecutionBinding {
    pub plan: EnvironmentPlan,
    pub spec: EnvironmentSpec,
    pub receipt: EnvironmentBuildReceipt,
}

impl EnvironmentExecutionBinding {
    pub fn validate(&self) -> Result<(), EnvironmentError> {
        self.plan.validate(Utc::now())?;
        self.spec.validate()?;
        self.receipt.validate_for(&self.spec)?;
        if self.spec.plan_sha256 != self.plan.plan_sha256 {
            return Err(hash_mismatch(
                "the execution spec does not belong to the execution plan",
            ));
        }
        if self.spec.builder_image.is_empty() || !is_digest(&self.spec.builder_image_digest) {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentBuilderUnavailable,
                "the frozen environment has no digest-pinned builder image",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EnvironmentBuildReceipt {
    pub schema_version: u32,
    pub receipt_id: String,
    pub spec_sha256: String,
    pub platform: String,
    pub builder_image: String,
    pub builder_image_digest: String,
    pub builder_policy_revision: String,
    pub status: EnvironmentBuilderStatus,
    #[serde(default)]
    pub store_path: String,
    #[serde(default)]
    pub output_sha256: String,
    #[serde(default)]
    pub sbom_sha256: String,
    #[serde(default)]
    pub log_ref: String,
    #[serde(default)]
    pub sandbox_attestation: SandboxAttestation,
    #[serde(default)]
    pub blockers: Vec<EnvironmentBlocker>,
    pub duration_ms: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SandboxAttestation {
    pub non_root: bool,
    pub no_secrets: bool,
    pub no_host_network: bool,
    pub no_host_socket: bool,
    pub no_host_mount: bool,
    pub sandbox: bool,
    pub impure: bool,
    pub bounded_resources: bool,
}

impl SandboxAttestation {
    pub fn complete(&self) -> bool {
        self.non_root
            && self.no_secrets
            && self.no_host_network
            && self.no_host_socket
            && self.no_host_mount
            && self.sandbox
            && !self.impure
            && self.bounded_resources
    }
}

impl EnvironmentBuildReceipt {
    pub fn validate_for(&self, spec: &EnvironmentSpec) -> Result<(), EnvironmentError> {
        if self.schema_version != ENVIRONMENT_SCHEMA_VERSION {
            return Err(invalid(
                "unsupported environment build receipt schema version",
            ));
        }
        bounded_text("build receipt id", &self.receipt_id, 64)?;
        if self.spec_sha256 != spec.spec_sha256 {
            return Err(hash_mismatch(
                "the build receipt is bound to a different environment spec",
            ));
        }
        if self.platform != spec.platform
            || self.builder_image != spec.builder_image
            || self.builder_image_digest != spec.builder_image_digest
        {
            return Err(hash_mismatch(
                "the build receipt builder identity does not match the frozen spec",
            ));
        }
        if self.status != EnvironmentBuilderStatus::Succeeded {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentBuildFailed,
                self.blockers
                    .first()
                    .map(|blocker| blocker.message.clone())
                    .unwrap_or_else(|| "the environment build did not succeed".into()),
            ));
        }
        if !self.sandbox_attestation.complete() {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentSandboxDenied,
                "the build receipt lacks a complete sandbox attestation",
            ));
        }
        if self.output_sha256.is_empty() || !is_sha256(&self.output_sha256) {
            return Err(EnvironmentError::new(
                EnvironmentBlockerCode::EnvironmentBuildFailed,
                "the build receipt has no content-addressed output hash",
            ));
        }
        if self.sbom_sha256 != spec.sbom_sha256 {
            return Err(hash_mismatch(
                "the build receipt SBOM hash does not match the frozen environment spec",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CheckExecutionTarget {
    pub check_id: String,
    pub environment_plan_id: String,
    pub environment_spec_sha256: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub timeout_secs: u32,
    pub expect_exit: i32,
    pub builder_image: String,
    pub builder_image_digest: String,
    pub network_policy: NetworkPolicy,
}

/// Resolve the exact environment for one check. A check without an environment
/// plan binding is refused; there is no implicit Rust or generic image fallback.
pub fn select_check_execution(
    check: &SpecCheck,
    plan: &EnvironmentPlan,
    spec: &EnvironmentSpec,
    receipt: &EnvironmentBuildReceipt,
) -> Result<CheckExecutionTarget, EnvironmentError> {
    let plan_id = check.environment_plan_id.as_deref().ok_or_else(|| {
        EnvironmentError::new(
            EnvironmentBlockerCode::EnvironmentPlanRequired,
            format!("check {} has no environment plan binding", check.id),
        )
    })?;
    if plan_id != plan.plan_id {
        return Err(hash_mismatch(format!(
            "check {} is bound to a different environment plan",
            check.id
        )));
    }
    let spec_sha256 = check.environment_spec_sha256.as_deref().ok_or_else(|| {
        EnvironmentError::new(
            EnvironmentBlockerCode::EnvironmentPlanRequired,
            format!("check {} has no frozen environment spec binding", check.id),
        )
    })?;
    if spec_sha256 != spec.spec_sha256 {
        return Err(hash_mismatch(format!(
            "check {} is bound to a different environment spec",
            check.id
        )));
    }
    spec.validate()?;
    plan.validate(Utc::now())?;
    if plan.status != EnvironmentPlanStatus::Frozen {
        return Err(EnvironmentError::new(
            EnvironmentBlockerCode::EnvironmentPlanRequired,
            "the check environment plan is not frozen",
        ));
    }
    if spec.plan_sha256 != plan.plan_sha256 {
        return Err(hash_mismatch(
            "the frozen spec does not belong to the check's plan",
        ));
    }
    if !spec.builder_image.is_empty()
        && (spec.builder_image != receipt.builder_image
            || spec.builder_image_digest != receipt.builder_image_digest)
    {
        return Err(hash_mismatch(
            "the check builder does not match the frozen environment spec",
        ));
    }
    receipt.validate_for(spec)?;
    validate_argv(&check.command)?;
    if check.command.is_empty() {
        return Err(invalid(format!(
            "check {} has an empty command vector",
            check.id
        )));
    }
    Ok(CheckExecutionTarget {
        check_id: check.id.clone(),
        environment_plan_id: plan.plan_id.clone(),
        environment_spec_sha256: spec.spec_sha256.clone(),
        argv: check.command.clone(),
        cwd: ".".into(),
        timeout_secs: check.timeout_secs,
        expect_exit: check.expect_exit,
        builder_image: spec.builder_image.clone(),
        builder_image_digest: spec.builder_image_digest.clone(),
        network_policy: spec.network_policy.clone(),
    })
}

/// A typed adapter used by the current verifier/Worker boundary until WP1-B
/// replaces the global image selection. It never returns a language image.
pub fn check_runner_blocker(
    check: &SpecCheck,
    plan: Option<&EnvironmentPlan>,
    spec: Option<&EnvironmentSpec>,
    receipt: Option<&EnvironmentBuildReceipt>,
) -> Option<EnvironmentBlocker> {
    let Some(plan) = plan else {
        return Some(EnvironmentBlocker {
            code: EnvironmentBlockerCode::EnvironmentPlanRequired,
            message: format!(
                "check {} has no environment plan; execution is refused rather than falling back to a language image",
                check.id
            ),
        });
    };
    let Some(spec) = spec else {
        return Some(EnvironmentBlocker {
            code: EnvironmentBlockerCode::EnvironmentPlanRequired,
            message: format!("check {} has no frozen environment spec", check.id),
        });
    };
    let Some(receipt) = receipt else {
        return Some(EnvironmentBlocker {
            code: EnvironmentBlockerCode::EnvironmentBuilderUnavailable,
            message: format!(
                "the environment builder has no successful receipt for check {}",
                check.id
            ),
        });
    };
    select_check_execution(check, plan, spec, receipt)
        .err()
        .map(|error| EnvironmentBlocker {
            code: error.code,
            message: error.message,
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn git(repository: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn repository(files: &[(&str, &str)]) -> (PathBuf, String) {
        let root =
            std::env::temp_dir().join(format!("factory-environment-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for (path, content) in files {
            let target = root.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(target, content).unwrap();
        }
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Fixture"]);
        git(&root, &["config", "user.email", "fixture@example.invalid"]);
        git(&root, &["add", "--all"]);
        git(&root, &["commit", "-qm", "base"]);
        let base = git(&root, &["rev-parse", "HEAD"]);
        (root, base)
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-10T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn builder() -> EnvironmentBuilderDescriptor {
        EnvironmentBuilderDescriptor {
            image: "factory/environment-builder".into(),
            digest: format!("sha256:{}", "a".repeat(64)),
            platform: DEFAULT_PLATFORM.into(),
            policy_revision: ENVIRONMENT_POLICY_REVISION.into(),
            default_nixpkgs: "github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into(),
            status: EnvironmentBuilderStatus::Ready,
        }
    }

    fn flake_lock(rev: &str) -> String {
        format!(
            r#"{{"nodes":{{"nixpkgs":{{"locked":{{"owner":"NixOS","repo":"nixpkgs","rev":"{rev}"}}}}}}}}"#
        )
    }

    fn requirements() -> EnvironmentRequirements {
        EnvironmentRequirements {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            explicit_constraints: ExplicitEnvironmentConstraints::default(),
            proposal: None,
            allow_unfree: false,
            owner_overrides: Vec::new(),
            resource_limits: ResourceLimits::default(),
            network: NetworkPolicy::default(),
        }
    }

    fn compile(
        repository: &Path,
        base: &str,
        requirements: &EnvironmentRequirements,
    ) -> EnvironmentPlan {
        let conventions =
            discover_project_conventions("project-1", repository, base, now()).unwrap();
        compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements,
            builder: builder(),
            now: now(),
        })
        .unwrap()
    }

    fn model_call() -> EnvironmentReviewModelCall {
        EnvironmentReviewModelCall {
            mechanism: "MODEL".into(),
            provider: "opencode-go".into(),
            model: "deepseek-v4.1-flash".into(),
            reservation_id: "reservation-1".into(),
            request_sha256: "d".repeat(64),
            input_tokens: 100,
            output_tokens: 20,
            total_tokens: 120,
            cost_micros: 1,
            usage_known: true,
        }
    }

    fn approve(plan: &mut EnvironmentPlan) -> EnvironmentPlanReview {
        record_lead_review(
            plan,
            RecordLeadReviewInput {
                review_id: "review-1".into(),
                principal_id: "lead:session-1",
                recorded_by: "core:lead-environment-review",
                decision: LeadReviewDecision::Approved,
                rationale:
                    "The plan matches the committed conventions and contains no unfree package.",
                constraints: Vec::new(),
                evidence_refs: vec!["plan-review:env".into()],
                model_call: model_call(),
                now: now(),
            },
        )
        .unwrap()
    }

    fn freeze(plan: &mut EnvironmentPlan, review: &EnvironmentPlanReview) -> EnvironmentSpec {
        let spec = freeze_environment_spec(
            plan,
            review,
            FreezeEnvironmentInput {
                spec_id: "spec-1".into(),
                nix_version: "2.28.3",
                devenv_version: "1.11.2",
                now: now(),
            },
        )
        .unwrap();
        plan.status = EnvironmentPlanStatus::Frozen;
        spec
    }

    fn receipt(spec: &EnvironmentSpec) -> EnvironmentBuildReceipt {
        EnvironmentBuildReceipt {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            receipt_id: "receipt-1".into(),
            spec_sha256: spec.spec_sha256.clone(),
            platform: spec.platform.clone(),
            builder_image: spec.builder_image.clone(),
            builder_image_digest: spec.builder_image_digest.clone(),
            builder_policy_revision: ENVIRONMENT_POLICY_REVISION.into(),
            status: EnvironmentBuilderStatus::Succeeded,
            store_path: "/nix/store/environment".into(),
            output_sha256: "b".repeat(64),
            sbom_sha256: spec.sbom_sha256.clone(),
            log_ref: "evidence/build.log".into(),
            sandbox_attestation: SandboxAttestation {
                non_root: true,
                no_secrets: true,
                no_host_network: true,
                no_host_socket: true,
                no_host_mount: true,
                sandbox: true,
                impure: false,
                bounded_resources: true,
            },
            blockers: Vec::new(),
            duration_ms: 1_000,
            created_at: now().to_rfc3339(),
        }
    }

    #[test]
    fn detects_rust_node_and_existing_devenv_conventions() {
        let (repository, base) = repository(&[
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.90.0\"\n"),
            ("Cargo.toml", "[package]\nname = \"widget\"\n"),
            ("Cargo.lock", "version = 4\n"),
            (".nvmrc", "22.14.0\n"),
            (
                "package.json",
                "{\"name\":\"widget\",\"packageManager\":\"pnpm@10.4.1\"}\n",
            ),
            (
                "devenv.nix",
                "{ pkgs, ... }: { packages = [ pkgs.ripgrep ]; }\n",
            ),
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            (
                ".github/workflows/ci.yml",
                "steps:\n  - run: cargo test --locked\n  - run: pnpm test\n",
            ),
        ]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        assert_eq!(
            conventions.existing_environment,
            ExistingEnvironmentKind::Devenv
        );
        assert!(
            conventions
                .pins_named("rust")
                .iter()
                .any(|pin| pin.value == "1.90.0")
        );
        assert!(
            conventions
                .pins_named("node")
                .iter()
                .any(|pin| pin.value == "22.14.0")
        );
        assert!(conventions.pins_named("nixpkgs").iter().any(|pin| {
            pin.value
                .ends_with("1111111111111111111111111111111111111111")
        }));
        assert_eq!(conventions.commands.len(), 2);
        assert!(conventions.conflicts.is_empty());
    }

    #[test]
    fn detects_an_existing_flake_without_generating_a_competing_environment() {
        let (repository, base) = repository(&[
            (
                "flake.nix",
                "{ inputs.nixpkgs.url = \"github:NixOS/nixpkgs/1111111111111111111111111111111111111111\"; }\n",
            ),
            (
                "flake.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
        ]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        assert_eq!(
            conventions.existing_environment,
            ExistingEnvironmentKind::Flake
        );
        let plan = compile(&repository, &base, &requirements());
        assert_eq!(plan.source, EnvironmentPlanSource::ProjectCommitted);
        assert!(
            plan.nix
                .nixpkgs
                .ends_with("1111111111111111111111111111111111111111")
        );
        assert!(
            plan.files
                .iter()
                .all(|file| file.origin == EnvironmentFileOrigin::Project)
        );
    }

    #[test]
    fn detects_multi_language_without_selecting_a_default() {
        let (repository, base) = repository(&[
            ("Cargo.toml", "[package]\nname = \"widget\"\n"),
            ("package.json", "{\"name\":\"widget\"}\n"),
            ("go.mod", "module example.com/widget\n\ngo 1.24.1\n"),
        ]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        assert_eq!(
            conventions.existing_environment,
            ExistingEnvironmentKind::None
        );
        let mut values = requirements();
        values.explicit_constraints.toolchains = vec![
            ToolchainRequirement {
                id: "rust".into(),
                version: "1.90.0".into(),
                source: "owner".into(),
                evidence: Vec::new(),
            },
            ToolchainRequirement {
                id: "node".into(),
                version: "22.14.0".into(),
                source: "owner".into(),
                evidence: Vec::new(),
            },
        ];
        values.explicit_constraints.nixpkgs =
            Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into());
        values.explicit_constraints.packages = vec![PackageRequirement {
            name: "ripgrep".into(),
            version: "14.1.1".into(),
            license: "MIT".into(),
            source: "nixpkgs".into(),
            attr: "ripgrep".into(),
            sha256: "c".repeat(64),
            unfree: false,
            attribution: "MIT ripgrep".into(),
        }];
        let plan = compile(&repository, &base, &values);
        assert_eq!(plan.toolchains.len(), 3);
        assert!(plan.toolchains.iter().any(|toolchain| toolchain.id == "go"));
        assert_eq!(plan.source, EnvironmentPlanSource::Generated);
        assert!(plan.files.iter().any(|file| file.path == "flake.nix"));
    }

    #[test]
    fn request_environment_constraints_reject_unknown_and_wrong_typed_fields() {
        assert!(serde_json::from_str::<RequestEnvironmentConstraints>(
            r#"{"nixpkgs":"github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d","typo":1}"#
        )
        .is_err());
        assert!(
            serde_json::from_str::<RequestEnvironmentConstraints>(r#"{"nixpkgs":42}"#).is_err()
        );
    }

    #[test]
    fn generated_environment_uses_the_documented_default_nixpkgs_pin() {
        let conventions = ProjectConventions {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            project_id: "project-1".into(),
            base_commit: "1".repeat(40),
            conventions_sha256: String::new(),
            scanned_at: now().to_rfc3339(),
            files: Vec::new(),
            commands: Vec::new(),
            conflicts: Vec::new(),
            truncated: false,
            skipped_entries: 0,
            provenance: "empty".into(),
            existing_environment: ExistingEnvironmentKind::None,
        };
        let mut conventions = conventions;
        conventions.conventions_sha256 = conventions.compute_sha256();
        let mut requirements = requirements();
        requirements.explicit_constraints.nixpkgs = Some(DEFAULT_ENVIRONMENT_NIXPKGS.into());
        let plan = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-default".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &requirements,
            builder: builder(),
            now: now(),
        })
        .unwrap();
        assert_eq!(plan.nix.nixpkgs, DEFAULT_ENVIRONMENT_NIXPKGS);
        assert!(
            plan.files
                .iter()
                .any(|file| file.path == "flake.nix" && file.content.is_some())
        );
        assert!(
            EnvironmentBuilderDescriptor::default()
                .default_nixpkgs
                .starts_with("github:NixOS/nixpkgs/")
        );
    }

    #[test]
    fn request_constraints_populate_generated_requirements_and_default_pin() {
        let constraints = RequestEnvironmentConstraints {
            nixpkgs: Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into()),
            packages: vec![PackageRequirement {
                name: "ripgrep".into(),
                version: "14.1.1".into(),
                license: "MIT".into(),
                source: "nixpkgs".into(),
                attr: "ripgrep".into(),
                sha256: "c".repeat(64),
                unfree: false,
                attribution: "MIT ripgrep".into(),
            }],
            resources: Some(ResourceLimits {
                cpu: 4,
                ..ResourceLimits::default()
            }),
            ..RequestEnvironmentConstraints::default()
        };
        let requirements = constraints.into_requirements();
        assert_eq!(
            requirements.explicit_constraints.nixpkgs.as_deref(),
            Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222")
        );
        assert_eq!(requirements.resource_limits.cpu, 4);
        let descriptor = EnvironmentBuilderDescriptor {
            default_nixpkgs: "github:NixOS/nixpkgs/3333333333333333333333333333333333333333".into(),
            ..builder()
        };
        descriptor.validate().unwrap();
    }

    #[test]
    fn refuses_a_project_with_no_conventions_and_no_explicit_selection() {
        let (repository, base) = repository(&[("README.md", "# Empty\n")]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        let error = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &requirements(),
            builder: builder(),
            now: now(),
        })
        .unwrap_err();
        assert_eq!(error.code, EnvironmentBlockerCode::EnvironmentPinMissing);
        assert!(!error.message.to_lowercase().contains("rust"));
        assert!(!error.message.to_lowercase().contains("node"));
    }

    #[test]
    fn equally_specific_conflicting_pins_fail_closed() {
        let (repository, base) = repository(&[
            (".nvmrc", "22.14.0\n"),
            (".tool-versions", "nodejs 20.19.0\n"),
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        assert_eq!(conventions.conflicts.len(), 1);
        assert_eq!(conventions.conflicts[0].name, "node");
        let error = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &requirements(),
            builder: builder(),
            now: now(),
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            EnvironmentBlockerCode::EnvironmentConventionsConflict
        );
    }

    #[test]
    fn lead_approval_is_required_and_bound_to_the_exact_plan() {
        let (repository, base) = repository(&[
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let mut plan = compile(&repository, &base, &requirements());
        assert_eq!(plan.status, EnvironmentPlanStatus::CoreValidated);
        let error = freeze_environment_spec(
            &plan,
            &EnvironmentPlanReview {
                schema_version: ENVIRONMENT_SCHEMA_VERSION,
                review_id: "review-1".into(),
                request_id: "request-1".into(),
                action_id: "action-1".into(),
                plan_id: "plan-1".into(),
                plan_sha256: plan.plan_sha256.clone(),
                plan_revision: 1,
                risk_class: "R0".into(),
                reviewer_role: EnvironmentReviewerRole::Lead,
                principal_id: "lead:session-1".into(),
                recorded_by: "core:lead-environment-review".into(),
                decision: LeadReviewDecision::Approved,
                rationale: "approved".into(),
                constraints: Vec::new(),
                decided_at: now().to_rfc3339(),
                evidence_refs: Vec::new(),
                model_call: model_call(),
            },
            FreezeEnvironmentInput {
                spec_id: "spec-1".into(),
                nix_version: "2.28.3",
                devenv_version: "1.11.2",
                now: now(),
            },
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            EnvironmentBlockerCode::EnvironmentLeadApprovalRequired
        );
        let review = approve(&mut plan);
        let spec = freeze(&mut plan, &review);
        assert_eq!(spec.plan_sha256, plan.plan_sha256);
        assert_eq!(spec.review_id, review.review_id);
        let wrong_risk = EnvironmentLeadReviewModel {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            request_id: plan.request_id.clone(),
            action_id: plan.action_id.clone(),
            plan_sha256: plan.plan_sha256.clone(),
            plan_revision: plan.revision,
            risk_class: "R1".into(),
            decision: EnvironmentLeadReviewModelDecision::Approve,
            rationale: "A different risk class must not approve this plan.".into(),
            constraints: Vec::new(),
        };
        assert_eq!(
            wrong_risk.validate_for(&plan, now()).unwrap_err().code,
            EnvironmentBlockerCode::EnvironmentHashMismatch
        );
        let wrong_plan = EnvironmentLeadReviewModel {
            plan_sha256: "f".repeat(64),
            ..wrong_risk.clone()
        };
        assert_eq!(
            wrong_plan.validate_for(&plan, now()).unwrap_err().code,
            EnvironmentBlockerCode::EnvironmentHashMismatch
        );
    }

    #[test]
    fn model_or_owner_review_cannot_bypass_core_validation() {
        let (repository, base) = repository(&[
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let mut plan = compile(&repository, &base, &requirements());
        plan.resources.cpu = 0;
        plan.refresh_hash();
        let mut review = EnvironmentPlanReview {
            schema_version: ENVIRONMENT_SCHEMA_VERSION,
            review_id: "review-1".into(),
            request_id: "request-1".into(),
            action_id: "action-1".into(),
            plan_id: "plan-1".into(),
            plan_sha256: plan.plan_sha256.clone(),
            plan_revision: 1,
            risk_class: "R0".into(),
            reviewer_role: EnvironmentReviewerRole::Lead,
            principal_id: "lead:session-1".into(),
            recorded_by: "core:lead-environment-review".into(),
            decision: LeadReviewDecision::Approved,
            rationale: "approved despite invalid resources".into(),
            constraints: Vec::new(),
            decided_at: now().to_rfc3339(),
            evidence_refs: Vec::new(),
            model_call: model_call(),
        };
        let error = record_lead_review(
            &mut plan,
            RecordLeadReviewInput {
                review_id: review.review_id.clone(),
                principal_id: &review.principal_id,
                recorded_by: &review.recorded_by,
                decision: review.decision,
                rationale: &review.rationale,
                constraints: Vec::new(),
                evidence_refs: Vec::new(),
                model_call: model_call(),
                now: now(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, EnvironmentBlockerCode::EnvironmentResourceLimit);
        review.decision = LeadReviewDecision::Approved;
        let _ = review;
    }

    #[test]
    fn owner_override_is_required_for_an_unfree_package_and_is_exact() {
        let (repository, base) = repository(&[
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let mut values = requirements();
        values.explicit_constraints.packages = vec![PackageRequirement {
            name: "proprietary-tool".into(),
            version: "1.0.0".into(),
            license: "MIT".into(),
            source: "nixpkgs".into(),
            attr: "proprietary-tool".into(),
            sha256: "c".repeat(64),
            unfree: true,
            attribution: "Proprietary Tool".into(),
        }];
        let error = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &discover_project_conventions("project-1", &repository, &base, now())
                .unwrap(),
            requirements: &values,
            builder: builder(),
            now: now(),
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            EnvironmentBlockerCode::EnvironmentLicenseBlocked
        );
        assert!(error.message.contains("unfree"));

        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        let mut draft = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &requirements(),
            builder: builder(),
            now: now(),
        })
        .unwrap();
        values.owner_overrides = vec![UnfreeOverride {
            id: "override-1".into(),
            package: "proprietary-tool".into(),
            version: "1.0.0".into(),
            license: "MIT".into(),
            plan_sha256: String::new(),
            reason: "Owner approved the exact proprietary package.".into(),
            decision_id: "decision-1".into(),
            expires_at: "2027-01-01T00:00:00Z".into(),
        }];
        let plan = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &values,
            builder: builder(),
            now: now(),
        })
        .unwrap();
        assert!(!plan.nix.allow_unfree);
        assert!(
            plan.license_inventory
                .entries
                .iter()
                .any(|entry| entry.name == "proprietary-tool"
                    && entry.owner_override_id.as_deref() == Some("override-1"))
        );
        assert_eq!(
            plan.license_inventory.overrides[0].plan_sha256,
            plan.plan_sha256
        );
        // The override is exact: another plan revision cannot reuse the bound
        // hash.
        draft.revision = 2;
        draft.refresh_hash();
        assert_ne!(draft.plan_sha256, plan.plan_sha256);
    }

    #[test]
    fn unknown_license_or_global_unfree_switch_is_blocked() {
        let (repository, base) = repository(&[
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        let mut values = requirements();
        values.explicit_constraints.packages = vec![PackageRequirement {
            name: "mystery".into(),
            version: "1.0.0".into(),
            license: "Unknown".into(),
            source: "nixpkgs".into(),
            attr: "mystery".into(),
            sha256: "c".repeat(64),
            unfree: false,
            attribution: String::new(),
        }];
        let error = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &values,
            builder: builder(),
            now: now(),
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            EnvironmentBlockerCode::EnvironmentLicenseBlocked
        );

        let mut values = requirements();
        values.allow_unfree = true;
        let error = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &values,
            builder: builder(),
            now: now(),
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            EnvironmentBlockerCode::EnvironmentLicenseBlocked
        );
        assert!(error.message.contains("allowUnfree"));
    }

    #[test]
    fn generated_files_reject_impure_execution_and_unpinned_fetches() {
        let (repository, base) = repository(&[("Cargo.toml", "[package]\nname=\"x\"\n")]);
        let conventions =
            discover_project_conventions("project-1", &repository, &base, now()).unwrap();
        let mut values = requirements();
        values.proposal = Some(EnvironmentProposal {
            nixpkgs: Some("github:NixOS/nixpkgs/2222222222222222222222222222222222222222".into()),
            files: BTreeMap::from([("flake.nix".into(), "import <nixpkgs> { }".into())]),
            ..EnvironmentProposal::default()
        });
        values.explicit_constraints.toolchains = vec![ToolchainRequirement {
            id: "rust".into(),
            version: "1.90.0".into(),
            source: "owner".into(),
            evidence: Vec::new(),
        }];
        values.explicit_constraints.commands.insert(
            "test".into(),
            CommandRequirement {
                argv: vec!["cargo".into(), "test".into()],
                cwd: ".".into(),
                timeout_secs: 300,
                expect_exit: 0,
            },
        );
        let error = compile_environment_plan(CompileEnvironmentInput {
            plan_id: "plan-1".into(),
            project_id: "project-1",
            request_id: "request-1",
            action_id: "action-1",
            campaign_id: None,
            revision: 1,
            risk_class: "R0",
            conventions: &conventions,
            requirements: &values,
            builder: builder(),
            now: now(),
        })
        .unwrap_err();
        assert_eq!(error.code, EnvironmentBlockerCode::EnvironmentSandboxDenied);
    }

    #[test]
    fn check_without_environment_binding_is_refused() {
        let (repository, base) = repository(&[
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let mut plan = compile(&repository, &base, &requirements());
        let review = approve(&mut plan);
        let spec = freeze(&mut plan, &review);
        let check = SpecCheck {
            id: "test".into(),
            kind: crate::lead::CheckKind::Test,
            description: "test".into(),
            command: vec!["cargo".into(), "test".into()],
            expect_exit: 0,
            timeout_secs: 60,
            environment_plan_id: None,
            environment_spec_sha256: None,
        };
        let error = select_check_execution(&check, &plan, &spec, &receipt(&spec)).unwrap_err();
        assert_eq!(error.code, EnvironmentBlockerCode::EnvironmentPlanRequired);
        let check = SpecCheck {
            environment_plan_id: Some(plan.plan_id.clone()),
            environment_spec_sha256: Some(spec.spec_sha256.clone()),
            ..check
        };
        let target = select_check_execution(&check, &plan, &spec, &receipt(&spec)).unwrap();
        assert_eq!(target.environment_spec_sha256, spec.spec_sha256);
    }

    #[test]
    fn builder_unavailable_or_failed_receipt_is_a_typed_blocker() {
        let (repository, base) = repository(&[
            (
                "devenv.lock",
                &flake_lock("1111111111111111111111111111111111111111"),
            ),
            ("devenv.nix", "{ pkgs, ... }: { }\n"),
        ]);
        let mut plan = compile(&repository, &base, &requirements());
        let review = approve(&mut plan);
        let spec = freeze(&mut plan, &review);
        let check = SpecCheck {
            id: "test".into(),
            kind: crate::lead::CheckKind::Test,
            description: "test".into(),
            command: vec!["cargo".into(), "test".into()],
            expect_exit: 0,
            timeout_secs: 60,
            environment_plan_id: Some(plan.plan_id.clone()),
            environment_spec_sha256: Some(spec.spec_sha256.clone()),
        };
        let blocker = check_runner_blocker(&check, Some(&plan), Some(&spec), None).unwrap();
        assert_eq!(
            blocker.code,
            EnvironmentBlockerCode::EnvironmentBuilderUnavailable
        );
        let mut failed = receipt(&spec);
        failed.status = EnvironmentBuilderStatus::Failed;
        let blocker =
            check_runner_blocker(&check, Some(&plan), Some(&spec), Some(&failed)).unwrap();
        assert_eq!(blocker.code, EnvironmentBlockerCode::EnvironmentBuildFailed);
    }
}
