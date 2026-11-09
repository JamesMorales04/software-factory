//! Dependency planning, vendor/cache and brokered acquisition contracts.
//!
//! This module is a deterministic contract/builder layer. It validates
//! dependency plans, builds exact command vectors, describes the brokered-fetch
//! boundary and verifies a candidate workspace's lockfile, toolchain, vendor
//! tree and Cargo source replacement. It never performs network or process I/O;
//! Core owns execution, authorization and evidence persistence.
//!
//! Decision mechanism: `DETERMINISTIC`. Version, source, evidence and command
//! validation are mechanical checks. A model may propose a dependency plan, but
//! it cannot grant approval, skip evidence or execute a command.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::planning::{
    DependencyItem as PlanningDependencyItem, DependencyPlan as PlanningDependencyPlan,
    DependencyStrategy,
};

/// Version of the dependency contract emitted by this module.
pub const DEPENDENCY_SCHEMA_VERSION: u32 = 1;

/// Registries a brokered fetch may contact unless the plan explicitly widens
/// the allowlist in a reviewed Core policy. Matching normalizes the
/// `registry+`/`sparse+` prefix and a trailing slash.
pub const DEFAULT_ALLOWED_REGISTRIES: &[&str] = &[
    "https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// Commands that would mutate dependency state or reach the network. They are
/// never valid in an offline check command.
const ONLINE_COMMAND_TOKENS: &[&str] = &[
    "fetch", "update", "add", "install", "publish", "search", "login", "logout", "owner", "yank",
    "online",
];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DependencyError {
    #[error("invalid dependency plan: {0}")]
    Invalid(String),
    #[error("dependency evidence is missing `{0}`")]
    MissingEvidence(&'static str),
    #[error("dependency `{name}` uses wildcard version `{version}`")]
    WildcardVersion { name: String, version: String },
    #[error("git dependency `{0}` requires explicit owner approval")]
    UnapprovedGitDependency(String),
    #[error("dependency check is online: `{0}`")]
    OnlineCheck(String),
    #[error("dependency command is invalid: `{0}`")]
    InvalidCommand(String),
    #[error("brokered fetch registry is not allowlisted: `{0}`")]
    RegistryNotAllowlisted(String),
    #[error("brokered fetch may not carry credentials")]
    CredentialsNotAllowed,
    #[error("brokered vendor write must be atomic")]
    NonAtomicVendorWrite,
    #[error("dependency workspace verification failed: {0}")]
    Workspace(String),
    #[error("dependency evidence does not match the candidate: {0}")]
    EvidenceMismatch(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DependencyKind {
    Rust,
    Node,
    Python,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DependencyAcquisition {
    VendorCache,
    BrokeredFetch,
    Both,
    NotRequired,
}

impl DependencyAcquisition {
    pub fn uses_vendor(self) -> bool {
        matches!(self, Self::VendorCache | Self::Both)
    }

    pub fn uses_broker(self) -> bool {
        matches!(self, Self::BrokeredFetch | Self::Both)
    }
}

impl From<DependencyStrategy> for DependencyAcquisition {
    fn from(strategy: DependencyStrategy) -> Self {
        match strategy {
            DependencyStrategy::NotRequired => Self::NotRequired,
            DependencyStrategy::VendorCache => Self::VendorCache,
            DependencyStrategy::BrokeredFetch => Self::BrokeredFetch,
            DependencyStrategy::Both => Self::Both,
        }
    }
}

impl From<DependencyAcquisition> for DependencyStrategy {
    fn from(acquisition: DependencyAcquisition) -> Self {
        match acquisition {
            DependencyAcquisition::NotRequired => Self::NotRequired,
            DependencyAcquisition::VendorCache => Self::VendorCache,
            DependencyAcquisition::BrokeredFetch => Self::BrokeredFetch,
            DependencyAcquisition::Both => Self::Both,
        }
    }
}

/// Candidate-bound supply-chain evidence. Hashes are SHA-256 hex digests and
/// references point at immutable artifacts, never at mutable prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyEvidence {
    pub lockfile_sha256: String,
    pub vendor_sha256: String,
    pub sbom_ref: String,
    pub sbom_sha256: String,
    pub license_report_ref: String,
    pub license_report_sha256: String,
    pub advisory_report_ref: String,
    pub advisory_report_sha256: String,
}

impl DependencyEvidence {
    pub fn validate(&self, acquisition: DependencyAcquisition) -> Result<(), DependencyError> {
        if acquisition == DependencyAcquisition::NotRequired {
            return Ok(());
        }
        validate_sha256("lockfile", &self.lockfile_sha256)?;
        validate_sha256("vendor", &self.vendor_sha256)?;
        for (name, reference, sha256) in [
            ("sbom", self.sbom_ref.as_str(), self.sbom_sha256.as_str()),
            (
                "license",
                self.license_report_ref.as_str(),
                self.license_report_sha256.as_str(),
            ),
            (
                "advisory",
                self.advisory_report_ref.as_str(),
                self.advisory_report_sha256.as_str(),
            ),
        ] {
            if reference.trim().is_empty() {
                return Err(DependencyError::MissingEvidence(name));
            }
            if !is_safe_relative_path(reference) {
                return Err(DependencyError::Workspace(format!(
                    "{name} evidence reference must be a safe relative path"
                )));
            }
            validate_sha256(name, sha256)?;
        }
        Ok(())
    }
}

/// One Rust dependency copied from the planning contract. The builder preserves
/// the planning item identity so the evidence and the executed commands refer
/// to the same dependency set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RustDependency {
    pub name: String,
    pub version: String,
    pub source: String,
    pub license: String,
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub advisory_status: String,
}

impl From<&PlanningDependencyItem> for RustDependency {
    fn from(item: &PlanningDependencyItem) -> Self {
        Self {
            name: item.name.clone(),
            version: item.version.clone(),
            source: item.source.clone(),
            license: item.license.clone(),
            checksum: item.checksum.clone(),
            advisory_status: item.advisory_status.clone(),
        }
    }
}

impl RustDependency {
    pub fn validate(&self, owner_approval: Option<&str>) -> Result<(), DependencyError> {
        if self.name.trim().is_empty()
            || self.version.trim().is_empty()
            || self.source.trim().is_empty()
            || self.license.trim().is_empty()
            || self.advisory_status.trim().is_empty()
        {
            return Err(DependencyError::Invalid(format!(
                "dependency `{}` requires name, version, source, license and advisory evidence",
                self.name
            )));
        }
        if self.version.contains('*') {
            return Err(DependencyError::WildcardVersion {
                name: self.name.clone(),
                version: self.version.clone(),
            });
        }
        if is_git_source(&self.source) {
            if !is_approved(owner_approval) {
                return Err(DependencyError::UnapprovedGitDependency(self.name.clone()));
            }
        } else if !is_local_source(&self.source) {
            validate_sha256("dependency checksum", &self.checksum)?;
        }
        Ok(())
    }
}

/// Exact, deterministic command vectors for the Rust dependency lifecycle.
/// Acquisition commands may reach the network; every check command is offline
/// or uses an explicitly bounded local data source. The exact vectors below are
/// the approved policy: deny uses `--offline`, audit uses `--no-fetch`, SBOM is
/// lockfile-only, and vet uses `--locked --frozen`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RustDependencyCommands {
    pub vendor: Vec<String>,
    pub build: Vec<String>,
    pub test: Vec<String>,
    pub frozen: Vec<String>,
    pub deny: Vec<String>,
    pub audit: Vec<String>,
    pub sbom: Vec<String>,
    pub vet: Option<Vec<String>>,
}

impl RustDependencyCommands {
    pub fn for_paths(vendor_path: &str, vet_required: bool) -> Self {
        Self {
            vendor: vec![
                "cargo".into(),
                "vendor".into(),
                "--locked".into(),
                vendor_path.into(),
            ],
            build: vec![
                "cargo".into(),
                "build".into(),
                "--locked".into(),
                "--offline".into(),
            ],
            test: vec![
                "cargo".into(),
                "test".into(),
                "--locked".into(),
                "--offline".into(),
            ],
            frozen: vec!["cargo".into(), "check".into(), "--frozen".into()],
            deny: vec![
                "cargo".into(),
                "deny".into(),
                "--offline".into(),
                "check".into(),
            ],
            audit: vec!["cargo".into(), "audit".into(), "--no-fetch".into()],
            sbom: vec![
                "cargo".into(),
                "cyclonedx".into(),
                "--format".into(),
                "json".into(),
            ],
            vet: vet_required.then(|| {
                vec![
                    "cargo".into(),
                    "vet".into(),
                    "--locked".into(),
                    "--frozen".into(),
                ]
            }),
        }
    }

    pub fn validate(&self, vendor_path: &str, vet_required: bool) -> Result<(), DependencyError> {
        if !command_eq(&self.vendor, &["cargo", "vendor", "--locked", vendor_path]) {
            return Err(DependencyError::InvalidCommand(format!(
                "vendor command must be `cargo vendor --locked {vendor_path}`"
            )));
        }

        let expected = Self::for_paths(vendor_path, vet_required);
        for (name, actual, expected_command, require_offline) in [
            ("build", &self.build, &expected.build, true),
            ("test", &self.test, &expected.test, true),
            ("frozen", &self.frozen, &expected.frozen, true),
            ("deny", &self.deny, &expected.deny, true),
            ("audit", &self.audit, &expected.audit, false),
            ("sbom", &self.sbom, &expected.sbom, false),
        ] {
            validate_check_command(name, actual, require_offline)?;
            if actual != expected_command {
                return Err(DependencyError::InvalidCommand(format!(
                    "{name} command must be `{}`",
                    expected_command.join(" ")
                )));
            }
        }

        match (&self.vet, vet_required) {
            (None, true) => Err(DependencyError::InvalidCommand(
                "cargo vet is required".into(),
            )),
            (Some(command), _) => {
                validate_check_command("vet", command, true)?;
                if !command_eq(command, &["cargo", "vet", "--locked", "--frozen"]) {
                    return Err(DependencyError::InvalidCommand(
                        "vet command must be `cargo vet`".into(),
                    ));
                }
                Ok(())
            }
            (None, false) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RustDependencyPlan {
    pub manifest_path: String,
    pub lockfile_path: String,
    pub toolchain_path: String,
    pub vendor_path: String,
    pub cargo_config_path: String,
    pub source_replacement: String,
    pub dependencies: Vec<RustDependency>,
    #[serde(default)]
    pub vet_required: bool,
    pub commands: RustDependencyCommands,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyWorkspaceEvidence {
    pub lockfile_sha256: String,
    pub vendor_sha256: String,
    pub source_replacement: String,
}

impl RustDependencyPlan {
    /// Candidate-bound verification that the workspace actually contains the
    /// lockfile, toolchain, vendor tree and Cargo source replacement the plan
    /// claims. This is read-only and fails closed on missing or unsafe paths.
    pub fn validate_workspace(
        &self,
        root: &Path,
    ) -> Result<DependencyWorkspaceEvidence, DependencyError> {
        self.validate(None)?;
        let root = root.canonicalize().map_err(|error| {
            DependencyError::Workspace(format!("workspace root is unavailable: {error}"))
        })?;
        for (name, relative) in [
            ("manifest", &self.manifest_path),
            ("lockfile", &self.lockfile_path),
            ("toolchain", &self.toolchain_path),
            ("cargo config", &self.cargo_config_path),
        ] {
            let path = safe_join(&root, relative)?;
            if !path.is_file() {
                return Err(DependencyError::Workspace(format!(
                    "{name} is missing: {}",
                    path.display()
                )));
            }
        }
        let vendor = safe_join(&root, &self.vendor_path)?;
        if !vendor.is_dir() {
            return Err(DependencyError::Workspace(format!(
                "vendor directory is missing: {}",
                vendor.display()
            )));
        }
        let config = std::fs::read_to_string(safe_join(&root, &self.cargo_config_path)?)
            .map_err(|error| DependencyError::Workspace(format!("cargo config: {error}")))?;
        let replacement = parse_cargo_source_replacement(&config, &self.vendor_path)?;
        let lockfile = std::fs::read(safe_join(&root, &self.lockfile_path)?)
            .map_err(|error| DependencyError::Workspace(format!("lockfile: {error}")))?;
        let lockfile_sha256 = hex_sha256(&lockfile);
        let vendor_sha256 = hash_tree(&vendor)?;
        Ok(DependencyWorkspaceEvidence {
            lockfile_sha256,
            vendor_sha256,
            source_replacement: format!("crates-io -> {replacement} -> {}", self.vendor_path),
        })
    }

    /// Verify the candidate workspace against persisted supply-chain evidence.
    /// The lockfile, vendor tree, SBOM, license report and advisory report must
    /// exist under the candidate root and match the recorded SHA-256 digests.
    pub fn validate_workspace_evidence(
        &self,
        root: &Path,
        evidence: &DependencyEvidence,
        acquisition: DependencyAcquisition,
    ) -> Result<DependencyWorkspaceEvidence, DependencyError> {
        evidence.validate(acquisition)?;
        if acquisition == DependencyAcquisition::NotRequired {
            return Ok(DependencyWorkspaceEvidence {
                lockfile_sha256: String::new(),
                vendor_sha256: String::new(),
                source_replacement: String::new(),
            });
        }
        let workspace = self.validate_workspace(root)?;
        if workspace.lockfile_sha256 != evidence.lockfile_sha256 {
            return Err(DependencyError::EvidenceMismatch("lockfile".into()));
        }
        if workspace.vendor_sha256 != evidence.vendor_sha256 {
            return Err(DependencyError::EvidenceMismatch("vendor".into()));
        }
        let root = root.canonicalize().map_err(|error| {
            DependencyError::Workspace(format!("workspace root is unavailable: {error}"))
        })?;
        for (name, reference, expected) in [
            ("sbom", &evidence.sbom_ref, &evidence.sbom_sha256),
            (
                "license",
                &evidence.license_report_ref,
                &evidence.license_report_sha256,
            ),
            (
                "advisory",
                &evidence.advisory_report_ref,
                &evidence.advisory_report_sha256,
            ),
        ] {
            let path = safe_join(&root, reference)?;
            let bytes = std::fs::read(&path).map_err(|error| {
                DependencyError::Workspace(format!("{name} evidence is unavailable: {error}"))
            })?;
            if hex_sha256(&bytes) != *expected {
                return Err(DependencyError::EvidenceMismatch(name.into()));
            }
        }
        Ok(workspace)
    }
}

/// Parse the minimal Cargo source-replacement shape the plan requires. The
/// parser is deliberately strict: duplicate sections/keys, unsupported syntax
/// and a replacement that does not resolve to the planned vendor directory all
/// fail closed.
fn parse_cargo_source_replacement(
    config: &str,
    expected_vendor: &str,
) -> Result<String, DependencyError> {
    use std::collections::BTreeMap;

    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for raw in config.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line.starts_with("[[") {
                return Err(DependencyError::Workspace(
                    "Cargo config has an unsupported table header".into(),
                ));
            }
            let name = line[1..line.len() - 1].trim();
            if name.is_empty() || sections.contains_key(name) {
                return Err(DependencyError::Workspace(
                    "Cargo config has an empty or duplicate table".into(),
                ));
            }
            sections.insert(name.to_string(), BTreeMap::new());
            current = Some(name.to_string());
            continue;
        }
        let Some(section) = current.as_ref() else {
            return Err(DependencyError::Workspace(
                "Cargo config has a key outside a table".into(),
            ));
        };
        let (key, value) = line.split_once('=').ok_or_else(|| {
            DependencyError::Workspace("Cargo config has an invalid key/value line".into())
        })?;
        let key = key.trim();
        if key.is_empty() {
            return Err(DependencyError::Workspace(
                "Cargo config has an empty key".into(),
            ));
        }
        let value = parse_toml_string(value.trim())?;
        if sections
            .get_mut(section)
            .expect("section exists")
            .insert(key.to_string(), value)
            .is_some()
        {
            return Err(DependencyError::Workspace(
                "Cargo config has a duplicate key".into(),
            ));
        }
    }
    let crates_io = sections.get("source.crates-io").ok_or_else(|| {
        DependencyError::Workspace("Cargo config does not define [source.crates-io]".into())
    })?;
    let replacement = crates_io.get("replace-with").ok_or_else(|| {
        DependencyError::Workspace("Cargo config does not replace crates.io".into())
    })?;
    let target = sections
        .get(&format!("source.{replacement}"))
        .ok_or_else(|| {
            DependencyError::Workspace("Cargo config replacement source is not defined".into())
        })?;
    let directory = target.get("directory").ok_or_else(|| {
        DependencyError::Workspace("Cargo config replacement source has no directory".into())
    })?;
    if directory != expected_vendor {
        return Err(DependencyError::Workspace(
            "Cargo config replacement directory does not match the planned vendor path".into(),
        ));
    }
    Ok(replacement.clone())
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

fn parse_toml_string(value: &str) -> Result<String, DependencyError> {
    let mut characters = value.chars();
    let quote = characters
        .next()
        .ok_or_else(|| DependencyError::Workspace("Cargo config has an empty value".into()))?;
    if quote != '"' && quote != '\'' {
        return Err(DependencyError::Workspace(
            "Cargo config values must be quoted strings".into(),
        ));
    }
    if !value.ends_with(quote) || value.len() < 2 {
        return Err(DependencyError::Workspace(
            "Cargo config has an unterminated string".into(),
        ));
    }
    let inner = &value[1..value.len() - 1];
    if inner.contains(quote) || inner.chars().any(|character| character.is_control()) {
        return Err(DependencyError::Workspace(
            "Cargo config string is not a simple quoted value".into(),
        ));
    }
    if quote == '\'' {
        return Ok(inner.to_string());
    }
    let mut parsed = String::with_capacity(inner.len());
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            match character {
                '"' => parsed.push('"'),
                '\\' => parsed.push('\\'),
                _ => {
                    return Err(DependencyError::Workspace(
                        "Cargo config uses an unsupported escape".into(),
                    ));
                }
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            parsed.push(character);
        }
    }
    if escaped {
        return Err(DependencyError::Workspace(
            "Cargo config ends with an incomplete escape".into(),
        ));
    }
    Ok(parsed)
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, DependencyError> {
    if !is_safe_relative_path(relative) {
        return Err(DependencyError::Workspace(format!(
            "unsafe dependency path: {relative}"
        )));
    }
    Ok(root.join(relative))
}

fn hash_tree(root: &Path) -> Result<String, DependencyError> {
    let mut entries = Vec::new();
    collect_files(root, root, &mut entries)?;
    entries.sort();
    let mut hasher = Sha256::new();
    for (relative, bytes) in entries {
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), DependencyError> {
    let mut children = std::fs::read_dir(directory)
        .map_err(|error| DependencyError::Workspace(format!("vendor directory: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| DependencyError::Workspace(format!("vendor entry: {error}")))?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| DependencyError::Workspace(format!("vendor metadata: {error}")))?;
        if metadata.file_type().is_symlink() {
            return Err(DependencyError::Workspace(format!(
                "vendor contains a symbolic link: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_files(root, &path, entries)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| DependencyError::Workspace("vendor path escaped root".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path)
                .map_err(|error| DependencyError::Workspace(format!("vendor file: {error}")))?;
            entries.push((relative, bytes));
        }
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl RustDependencyPlan {
    pub fn new(dependencies: Vec<RustDependency>) -> Self {
        let vendor_path = "vendor".to_string();
        Self {
            manifest_path: "Cargo.toml".into(),
            lockfile_path: "Cargo.lock".into(),
            toolchain_path: "rust-toolchain.toml".into(),
            vendor_path: vendor_path.clone(),
            cargo_config_path: ".cargo/config.toml".into(),
            source_replacement: default_source_replacement(&vendor_path),
            dependencies,
            vet_required: false,
            commands: RustDependencyCommands::for_paths(&vendor_path, false),
        }
    }

    pub fn with_vendor_path(mut self, vendor_path: impl Into<String>) -> Self {
        let vendor_path = vendor_path.into();
        self.vendor_path = vendor_path.clone();
        self.source_replacement = default_source_replacement(&vendor_path);
        self.commands = RustDependencyCommands::for_paths(&vendor_path, self.vet_required);
        self
    }

    pub fn with_source_replacement(mut self, source_replacement: impl Into<String>) -> Self {
        self.source_replacement = source_replacement.into();
        self
    }

    pub fn with_vet_required(mut self, vet_required: bool) -> Self {
        self.vet_required = vet_required;
        self.commands = RustDependencyCommands::for_paths(&self.vendor_path, vet_required);
        self
    }

    pub fn commands(&self) -> &RustDependencyCommands {
        &self.commands
    }

    pub fn validate(&self, owner_approval: Option<&str>) -> Result<(), DependencyError> {
        validate_required_path("manifest", &self.manifest_path, "Cargo.toml")?;
        validate_required_path("lockfile", &self.lockfile_path, "Cargo.lock")?;
        validate_required_path("toolchain", &self.toolchain_path, "rust-toolchain.toml")?;
        validate_required_path("cargo config", &self.cargo_config_path, "config.toml")?;
        if !is_safe_relative_path(&self.vendor_path) {
            return Err(DependencyError::Invalid(
                "vendor path must be a safe relative path".into(),
            ));
        }
        validate_source_replacement(&self.source_replacement, &self.vendor_path)?;
        for dependency in &self.dependencies {
            dependency.validate(owner_approval)?;
        }
        self.commands.validate(&self.vendor_path, self.vet_required)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VendorWriteMode {
    Atomic,
    InPlace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CredentialMode {
    None,
    Ambient,
}

/// Network-enabled acquisition boundary. It is intentionally credential-free,
/// registry-allowlisted, lockfile-bound and check-free. The vendor write is
/// modeled as atomic so an aborted fetch cannot leave a half-written tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BrokeredFetch {
    pub registry: String,
    pub allowed_registries: Vec<String>,
    pub lockfile_path: String,
    pub lockfile_sha256: String,
    pub vendor_path: String,
    pub write_mode: VendorWriteMode,
    pub credentials: CredentialMode,
    pub execute_checks: bool,
    pub fetch_command: Vec<String>,
    pub vendor_command: Vec<String>,
}

impl BrokeredFetch {
    pub fn new(
        registry: impl Into<String>,
        lockfile_path: impl Into<String>,
        lockfile_sha256: impl Into<String>,
        vendor_path: impl Into<String>,
    ) -> Self {
        let vendor_path = vendor_path.into();
        Self {
            registry: registry.into(),
            allowed_registries: DEFAULT_ALLOWED_REGISTRIES
                .iter()
                .map(|registry| (*registry).to_string())
                .collect(),
            lockfile_path: lockfile_path.into(),
            lockfile_sha256: lockfile_sha256.into(),
            vendor_command: vec![
                "cargo".into(),
                "vendor".into(),
                "--locked".into(),
                vendor_path.clone(),
            ],
            vendor_path,
            write_mode: VendorWriteMode::Atomic,
            credentials: CredentialMode::None,
            execute_checks: false,
            fetch_command: vec!["cargo".into(), "fetch".into(), "--locked".into()],
        }
    }

    pub fn validate(&self) -> Result<(), DependencyError> {
        if self.registry.trim().is_empty() {
            return Err(DependencyError::Invalid(
                "brokered fetch registry is required".into(),
            ));
        }
        if self.allowed_registries.is_empty()
            || self
                .allowed_registries
                .iter()
                .any(|registry| registry.trim().is_empty())
        {
            return Err(DependencyError::Invalid(
                "brokered fetch requires a non-empty registry allowlist".into(),
            ));
        }
        let registry = normalize_registry(&self.registry);
        if !self
            .allowed_registries
            .iter()
            .any(|allowed| normalize_registry(allowed) == registry)
        {
            return Err(DependencyError::RegistryNotAllowlisted(
                self.registry.clone(),
            ));
        }

        validate_required_path("lockfile", &self.lockfile_path, "Cargo.lock")?;
        validate_sha256("lockfile", &self.lockfile_sha256)?;
        if !is_safe_relative_path(&self.vendor_path) {
            return Err(DependencyError::Invalid(
                "vendor path must be a safe relative path".into(),
            ));
        }
        if self.write_mode != VendorWriteMode::Atomic {
            return Err(DependencyError::NonAtomicVendorWrite);
        }
        if self.credentials != CredentialMode::None {
            return Err(DependencyError::CredentialsNotAllowed);
        }
        if self.execute_checks {
            return Err(DependencyError::Invalid(
                "brokered fetch may not execute checks".into(),
            ));
        }
        if !command_eq(&self.fetch_command, &["cargo", "fetch", "--locked"]) {
            return Err(DependencyError::InvalidCommand(
                "fetch command must be `cargo fetch --locked`".into(),
            ));
        }
        if !command_eq(
            &self.vendor_command,
            &["cargo", "vendor", "--locked", self.vendor_path.as_str()],
        ) {
            return Err(DependencyError::InvalidCommand(format!(
                "vendor command must be `cargo vendor --locked {}`",
                self.vendor_path
            )));
        }
        Ok(())
    }
}

/// A dependency plan that preserves the existing planning contract and adds the
/// deterministic execution/evidence detail WP4 needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyPlan {
    pub schema_version: u32,
    pub planning: PlanningDependencyPlan,
    pub kind: DependencyKind,
    pub acquisition: DependencyAcquisition,
    pub evidence: DependencyEvidence,
    #[serde(default)]
    pub rust: Option<RustDependencyPlan>,
    #[serde(default)]
    pub brokered_fetch: Option<BrokeredFetch>,
}

impl DependencyPlan {
    pub fn from_planning(
        planning: &PlanningDependencyPlan,
        kind: DependencyKind,
        evidence: DependencyEvidence,
        brokered_fetch: Option<BrokeredFetch>,
    ) -> Result<Self, DependencyError> {
        let mut builder = DependencyPlanBuilder::new(planning, kind, evidence);
        if let Some(fetch) = brokered_fetch {
            builder = builder.brokered_fetch(fetch);
        }
        builder.build()
    }

    pub fn commands(&self) -> Option<&RustDependencyCommands> {
        self.rust.as_ref().map(RustDependencyPlan::commands)
    }

    pub fn validate(&self) -> Result<(), DependencyError> {
        if self.schema_version != DEPENDENCY_SCHEMA_VERSION {
            return Err(DependencyError::Invalid(
                "unsupported dependency schema version".into(),
            ));
        }
        self.planning.validate().map_err(DependencyError::Invalid)?;
        if self.acquisition != DependencyAcquisition::from(self.planning.strategy) {
            return Err(DependencyError::Invalid(
                "dependency acquisition must match the planning strategy".into(),
            ));
        }
        if self.acquisition == DependencyAcquisition::NotRequired {
            if !self.planning.items.is_empty() {
                return Err(DependencyError::Invalid(
                    "a not-required dependency plan cannot list dependencies".into(),
                ));
            }
            if self.rust.is_some() || self.brokered_fetch.is_some() {
                return Err(DependencyError::Invalid(
                    "a not-required dependency plan cannot carry acquisition contracts".into(),
                ));
            }
            return Ok(());
        }

        self.evidence.validate(self.acquisition)?;
        for item in &self.planning.items {
            validate_planning_item(item, self.planning.owner_approval.as_deref())?;
        }

        match self.kind {
            DependencyKind::Rust => {
                let rust = self.rust.as_ref().ok_or_else(|| {
                    DependencyError::Invalid(
                        "a Rust dependency plan requires a RustDependencyPlan".into(),
                    )
                })?;
                rust.validate(self.planning.owner_approval.as_deref())?;
                validate_rust_items_match(&self.planning.items, &rust.dependencies)?;

                if self.acquisition.uses_broker() {
                    let fetch = self.brokered_fetch.as_ref().ok_or_else(|| {
                        DependencyError::Invalid(
                            "brokered fetch acquisition requires a BrokeredFetch contract".into(),
                        )
                    })?;
                    fetch.validate()?;
                    if fetch.lockfile_path != rust.lockfile_path {
                        return Err(DependencyError::Invalid(
                            "brokered fetch lockfile must match the Rust plan".into(),
                        ));
                    }
                    if fetch.lockfile_sha256 != self.evidence.lockfile_sha256 {
                        return Err(DependencyError::Invalid(
                            "brokered fetch lockfile hash must match the dependency evidence"
                                .into(),
                        ));
                    }
                    if fetch.vendor_path != rust.vendor_path {
                        return Err(DependencyError::Invalid(
                            "brokered fetch vendor path must match the Rust plan".into(),
                        ));
                    }
                } else if self.brokered_fetch.is_some() {
                    return Err(DependencyError::Invalid(
                        "a vendor-cache plan must not carry a brokered fetch contract".into(),
                    ));
                }
            }
            DependencyKind::Node | DependencyKind::Python | DependencyKind::Other => {
                if self.rust.is_some() {
                    return Err(DependencyError::Invalid(
                        "only a Rust dependency plan may carry Rust commands".into(),
                    ));
                }
                if self.brokered_fetch.is_some() {
                    return Err(DependencyError::Invalid(
                        "the brokered fetch contract is Rust-specific".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

pub struct DependencyPlanBuilder<'a> {
    planning: &'a PlanningDependencyPlan,
    kind: DependencyKind,
    evidence: DependencyEvidence,
    rust: Option<RustDependencyPlan>,
    brokered_fetch: Option<BrokeredFetch>,
}

impl<'a> DependencyPlanBuilder<'a> {
    pub fn new(
        planning: &'a PlanningDependencyPlan,
        kind: DependencyKind,
        evidence: DependencyEvidence,
    ) -> Self {
        Self {
            planning,
            kind,
            evidence,
            rust: None,
            brokered_fetch: None,
        }
    }

    pub fn rust(mut self, rust: RustDependencyPlan) -> Self {
        self.rust = Some(rust);
        self
    }

    pub fn brokered_fetch(mut self, brokered_fetch: BrokeredFetch) -> Self {
        self.brokered_fetch = Some(brokered_fetch);
        self
    }

    pub fn build(self) -> Result<DependencyPlan, DependencyError> {
        let rust = self.rust.or_else(|| {
            (self.kind == DependencyKind::Rust
                && self.planning.strategy != DependencyStrategy::NotRequired)
                .then(|| {
                    RustDependencyPlan::new(
                        self.planning
                            .items
                            .iter()
                            .map(RustDependency::from)
                            .collect(),
                    )
                })
        });
        let plan = DependencyPlan {
            schema_version: DEPENDENCY_SCHEMA_VERSION,
            planning: self.planning.clone(),
            kind: self.kind,
            acquisition: self.planning.strategy.into(),
            evidence: self.evidence,
            rust,
            brokered_fetch: self.brokered_fetch,
        };
        plan.validate()?;
        Ok(plan)
    }
}

fn validate_planning_item(
    item: &PlanningDependencyItem,
    owner_approval: Option<&str>,
) -> Result<(), DependencyError> {
    if item.name.trim().is_empty()
        || item.version.trim().is_empty()
        || item.source.trim().is_empty()
        || item.license.trim().is_empty()
        || item.advisory_status.trim().is_empty()
    {
        return Err(DependencyError::Invalid(format!(
            "dependency `{}` requires name, version, source, license and advisory evidence",
            item.name
        )));
    }
    if item.version.contains('*') {
        return Err(DependencyError::WildcardVersion {
            name: item.name.clone(),
            version: item.version.clone(),
        });
    }
    if is_git_source(&item.source) && !is_approved(owner_approval) {
        return Err(DependencyError::UnapprovedGitDependency(item.name.clone()));
    }
    Ok(())
}

fn validate_rust_items_match(
    items: &[PlanningDependencyItem],
    dependencies: &[RustDependency],
) -> Result<(), DependencyError> {
    let expected: Vec<RustDependency> = items.iter().map(RustDependency::from).collect();
    if expected != dependencies {
        return Err(DependencyError::Invalid(
            "Rust dependency plan must mirror the planning dependency items".into(),
        ));
    }
    Ok(())
}

fn validate_check_command(
    name: &str,
    command: &[String],
    require_offline: bool,
) -> Result<(), DependencyError> {
    if command.first().map(String::as_str) != Some("cargo") {
        return Err(DependencyError::InvalidCommand(format!(
            "{name} must invoke cargo"
        )));
    }
    if command.iter().any(|token| is_online_token(token)) {
        return Err(DependencyError::OnlineCheck(command.join(" ")));
    }
    if require_offline {
        let offline = command
            .iter()
            .any(|token| token == "--offline" || token == "--frozen" || token == "--no-fetch");
        if !offline {
            return Err(DependencyError::OnlineCheck(command.join(" ")));
        }
    }
    Ok(())
}

fn is_online_token(token: &str) -> bool {
    ONLINE_COMMAND_TOKENS.contains(&token.trim_start_matches('-'))
}

fn validate_required_path(
    field: &'static str,
    value: &str,
    expected_file: &str,
) -> Result<(), DependencyError> {
    if !is_safe_relative_path(value) || !has_file_name(value, expected_file) {
        return Err(DependencyError::Invalid(format!(
            "{field} path must be a safe relative path ending in {expected_file}"
        )));
    }
    Ok(())
}

fn validate_source_replacement(
    source_replacement: &str,
    vendor_path: &str,
) -> Result<(), DependencyError> {
    parse_cargo_source_replacement(source_replacement, vendor_path).map_err(|_| {
        DependencyError::Invalid(
            "source replacement must map crates-io to the vendor directory".into(),
        )
    })?;
    Ok(())
}

fn validate_sha256(field: &'static str, value: &str) -> Result<(), DependencyError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DependencyError::Invalid(format!(
            "{field} must be a SHA-256 hex digest"
        )));
    }
    Ok(())
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !value.contains(':')
        && !value.contains('"')
        && !value.contains('\'')
        && value.chars().all(|character| !character.is_control())
        && value
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn has_file_name(value: &str, expected: &str) -> bool {
    value.rsplit(['/', '\\']).next() == Some(expected)
}

fn is_approved(owner_approval: Option<&str>) -> bool {
    owner_approval.is_some_and(|approval| !approval.trim().is_empty())
}

fn is_git_source(source: &str) -> bool {
    let source = source.trim().to_ascii_lowercase();
    source.starts_with("git+")
        || source.starts_with("git:")
        || source.starts_with("git=")
        || source.starts_with("git ")
        || source.contains("git =")
        || source.contains("git=")
}

fn is_local_source(source: &str) -> bool {
    let source = source.trim().to_ascii_lowercase();
    source.starts_with("path+")
        || source.starts_with("path:")
        || source.starts_with("path=")
        || source == "workspace"
        || source.starts_with("workspace+")
}

fn normalize_registry(value: &str) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix("registry+")
        .or_else(|| value.strip_prefix("sparse+"))
        .unwrap_or(value);
    value.trim_end_matches('/').to_ascii_lowercase()
}

fn command_eq(command: &[String], expected: &[&str]) -> bool {
    command.len() == expected.len()
        && command
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.as_str() == *expected)
}

fn default_source_replacement(vendor_path: &str) -> String {
    format!(
        "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"{vendor_path}\"\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planning_item() -> PlanningDependencyItem {
        PlanningDependencyItem {
            name: "serde".into(),
            version: "1.0.226".into(),
            source: "registry+https://github.com/rust-lang/crates.io-index".into(),
            license: "MIT OR Apache-2.0".into(),
            checksum: "a".repeat(64),
            advisory_status: "no-known-advisories".into(),
            sbom_ref: "artifacts/sbom.json".into(),
        }
    }

    fn planning_plan(strategy: DependencyStrategy) -> PlanningDependencyPlan {
        let items = if strategy == DependencyStrategy::NotRequired {
            Vec::new()
        } else {
            vec![planning_item()]
        };
        PlanningDependencyPlan {
            strategy,
            items,
            cache_ref: if strategy == DependencyStrategy::NotRequired {
                String::new()
            } else {
                "vendor".into()
            },
            sbom_ref: if strategy == DependencyStrategy::NotRequired {
                String::new()
            } else {
                "artifacts/sbom.json".into()
            },
            owner_approval: None,
        }
    }

    fn evidence() -> DependencyEvidence {
        DependencyEvidence {
            lockfile_sha256: "b".repeat(64),
            vendor_sha256: "c".repeat(64),
            sbom_ref: "artifacts/sbom.json".into(),
            sbom_sha256: "d".repeat(64),
            license_report_ref: "artifacts/licenses.json".into(),
            license_report_sha256: "e".repeat(64),
            advisory_report_ref: "artifacts/advisories.json".into(),
            advisory_report_sha256: "f".repeat(64),
        }
    }

    fn rust_plan() -> RustDependencyPlan {
        RustDependencyPlan::new(vec![RustDependency::from(&planning_item())])
    }

    #[test]
    fn workspace_evidence_requires_lock_toolchain_vendor_and_source_replacement() {
        let root = std::env::temp_dir().join(format!("factory-deps-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".cargo")).unwrap();
        std::fs::create_dir_all(root.join("vendor")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
        std::fs::write(root.join("rust-toolchain.toml"), "[toolchain]\n").unwrap();
        std::fs::write(
            root.join(".cargo/config.toml"),
            "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("artifacts")).unwrap();
        std::fs::write(root.join("vendor/dep.txt"), "crate").unwrap();
        std::fs::write(root.join("artifacts/sbom.json"), "sbom").unwrap();
        std::fs::write(root.join("artifacts/licenses.json"), "licenses").unwrap();
        std::fs::write(root.join("artifacts/advisories.json"), "advisories").unwrap();
        let workspace = RustDependencyPlan::new(Vec::new())
            .validate_workspace(&root)
            .unwrap();
        assert_eq!(workspace.lockfile_sha256.len(), 64);
        assert_eq!(workspace.vendor_sha256.len(), 64);
        let mut evidence = DependencyEvidence {
            lockfile_sha256: workspace.lockfile_sha256.clone(),
            vendor_sha256: workspace.vendor_sha256.clone(),
            sbom_ref: "artifacts/sbom.json".into(),
            sbom_sha256: hex_sha256(b"sbom"),
            license_report_ref: "artifacts/licenses.json".into(),
            license_report_sha256: hex_sha256(b"licenses"),
            advisory_report_ref: "artifacts/advisories.json".into(),
            advisory_report_sha256: hex_sha256(b"advisories"),
        };
        RustDependencyPlan::new(Vec::new())
            .validate_workspace_evidence(&root, &evidence, DependencyAcquisition::VendorCache)
            .unwrap();
        evidence.vendor_sha256 = "d".repeat(64);
        assert!(matches!(
            RustDependencyPlan::new(Vec::new()).validate_workspace_evidence(
                &root,
                &evidence,
                DependencyAcquisition::VendorCache
            ),
            Err(DependencyError::EvidenceMismatch(_))
        ));
        std::fs::remove_file(root.join("Cargo.lock")).unwrap();
        assert!(
            RustDependencyPlan::new(Vec::new())
                .validate_workspace(&root)
                .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cargo_source_replacement_rejects_wrong_or_ambiguous_targets() {
        let valid = "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"vendor\"\n";
        assert_eq!(
            parse_cargo_source_replacement(valid, "vendor").unwrap(),
            "vendored-sources"
        );
        let wrong_directory = "[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = \"elsewhere\"\n";
        assert!(parse_cargo_source_replacement(wrong_directory, "vendor").is_err());
        let missing_target = "[source.crates-io]\nreplace-with = \"missing\"\n";
        assert!(parse_cargo_source_replacement(missing_target, "vendor").is_err());
        let duplicate = "[source.crates-io]\nreplace-with = \"a\"\nreplace-with = \"b\"\n";
        assert!(parse_cargo_source_replacement(duplicate, "vendor").is_err());
    }

    fn brokered_fetch() -> BrokeredFetch {
        BrokeredFetch::new(
            "registry+https://github.com/rust-lang/crates.io-index",
            "Cargo.lock",
            "b".repeat(64),
            "vendor",
        )
    }

    fn plan(strategy: DependencyStrategy) -> DependencyPlan {
        let planning = planning_plan(strategy);
        let fetch = matches!(
            strategy,
            DependencyStrategy::BrokeredFetch | DependencyStrategy::Both
        )
        .then(brokered_fetch);
        DependencyPlan::from_planning(&planning, DependencyKind::Rust, evidence(), fetch)
            .expect("valid plan")
    }

    #[test]
    fn valid_rust_plan_builds_exact_offline_commands() {
        let plan = plan(DependencyStrategy::VendorCache);
        assert!(plan.validate().is_ok());
        assert_eq!(plan.acquisition, DependencyAcquisition::VendorCache);
        assert_eq!(
            plan.planning,
            planning_plan(DependencyStrategy::VendorCache)
        );

        let commands = plan.commands().expect("rust commands");
        assert_eq!(
            commands.vendor,
            vec!["cargo", "vendor", "--locked", "vendor"]
        );
        assert_eq!(
            commands.build,
            vec!["cargo", "build", "--locked", "--offline"]
        );
        assert_eq!(
            commands.test,
            vec!["cargo", "test", "--locked", "--offline"]
        );
        assert_eq!(commands.frozen, vec!["cargo", "check", "--frozen"]);
        assert_eq!(commands.deny, vec!["cargo", "deny", "--offline", "check"]);
        assert_eq!(commands.audit, vec!["cargo", "audit", "--no-fetch"]);
        assert_eq!(
            commands.sbom,
            vec!["cargo", "cyclonedx", "--format", "json"]
        );
        assert_eq!(commands.vet, None);
    }

    #[test]
    fn builder_derives_the_default_rust_plan_from_planning_items() {
        let planning = planning_plan(DependencyStrategy::VendorCache);
        let plan = DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
            .build()
            .expect("default rust plan");
        assert_eq!(
            plan.commands().expect("rust commands").build,
            vec!["cargo", "build", "--locked", "--offline"]
        );
        assert_eq!(
            plan.rust.expect("rust plan").dependencies,
            vec![RustDependency::from(&planning_item())]
        );
    }

    #[test]
    fn optional_cargo_vet_builds_a_deterministic_command() {
        let planning = planning_plan(DependencyStrategy::VendorCache);
        let rust = rust_plan().with_vet_required(true);
        let plan = DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
            .rust(rust)
            .build()
            .expect("valid plan with vet");
        assert_eq!(
            plan.commands().expect("rust commands").vet,
            Some(vec![
                "cargo".into(),
                "vet".into(),
                "--locked".into(),
                "--frozen".into(),
            ])
        );
    }

    #[test]
    fn rust_plan_requires_lockfile_and_toolchain() {
        let planning = planning_plan(DependencyStrategy::VendorCache);

        let mut rust = rust_plan();
        rust.lockfile_path = String::new();
        assert!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
                .rust(rust)
                .build()
                .is_err()
        );

        let mut rust = rust_plan();
        rust.toolchain_path = "rust-toolchain".into();
        assert!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
                .rust(rust)
                .build()
                .is_err()
        );
    }

    #[test]
    fn rust_plan_requires_vendor_path_and_source_replacement() {
        let planning = planning_plan(DependencyStrategy::VendorCache);

        let mut rust = rust_plan();
        rust.vendor_path = String::new();
        assert!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
                .rust(rust)
                .build()
                .is_err()
        );

        let mut rust = rust_plan();
        rust.source_replacement = "[source.crates-io]\nreplace-with = \"missing\"\n".into();
        assert!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
                .rust(rust)
                .build()
                .is_err()
        );
    }

    #[test]
    fn missing_lockfile_vendor_and_sbom_evidence_are_rejected() {
        let planning = planning_plan(DependencyStrategy::VendorCache);

        let mut missing_lockfile = evidence();
        missing_lockfile.lockfile_sha256.clear();
        assert!(matches!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, missing_lockfile, None),
            Err(DependencyError::Invalid(_))
        ));

        let mut missing_vendor = evidence();
        missing_vendor.vendor_sha256.clear();
        assert!(matches!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, missing_vendor, None),
            Err(DependencyError::Invalid(_))
        ));

        let mut missing_brokered_vendor = evidence();
        missing_brokered_vendor.vendor_sha256.clear();
        assert!(matches!(
            DependencyPlan::from_planning(
                &planning_plan(DependencyStrategy::BrokeredFetch),
                DependencyKind::Rust,
                missing_brokered_vendor,
                Some(brokered_fetch())
            ),
            Err(DependencyError::Invalid(_))
        ));

        let mut missing_sbom = evidence();
        missing_sbom.sbom_ref.clear();
        assert_eq!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, missing_sbom, None)
                .expect_err("missing sbom"),
            DependencyError::MissingEvidence("sbom")
        );

        let mut missing_license = evidence();
        missing_license.license_report_ref.clear();
        assert_eq!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, missing_license, None)
                .expect_err("missing license"),
            DependencyError::MissingEvidence("license")
        );

        let mut missing_advisory = evidence();
        missing_advisory.advisory_report_ref.clear();
        assert_eq!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, missing_advisory, None)
                .expect_err("missing advisory"),
            DependencyError::MissingEvidence("advisory")
        );
    }

    #[test]
    fn wildcard_versions_are_rejected() {
        let mut planning = planning_plan(DependencyStrategy::VendorCache);
        planning.items[0].version = "1.*".into();
        assert!(matches!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, evidence(), None),
            Err(DependencyError::WildcardVersion { .. })
        ));
    }

    #[test]
    fn git_dependencies_require_explicit_owner_approval() {
        let mut planning = planning_plan(DependencyStrategy::VendorCache);
        planning.items[0].source = "git+https://github.com/example/repo".into();
        planning.items[0].checksum.clear();

        assert!(matches!(
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, evidence(), None),
            Err(DependencyError::UnapprovedGitDependency(name)) if name == "serde"
        ));

        planning.owner_approval = Some("owner-1".into());
        let approved =
            DependencyPlan::from_planning(&planning, DependencyKind::Rust, evidence(), None)
                .expect("approved git dependency");
        assert!(approved.validate().is_ok());
    }

    #[test]
    fn online_check_commands_are_rejected() {
        let planning = planning_plan(DependencyStrategy::VendorCache);

        let mut rust = rust_plan();
        rust.commands.build = vec!["cargo".into(), "build".into()];
        assert!(matches!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
                .rust(rust)
                .build(),
            Err(DependencyError::OnlineCheck(_))
        ));

        let mut rust = rust_plan();
        rust.commands.audit = vec!["cargo".into(), "update".into()];
        assert!(matches!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Rust, evidence())
                .rust(rust)
                .build(),
            Err(DependencyError::OnlineCheck(_))
        ));
    }

    #[test]
    fn brokered_fetch_requires_allowlist_lockfile_and_atomic_write() {
        let valid = brokered_fetch();
        assert!(valid.validate().is_ok());
        assert_eq!(valid.fetch_command, vec!["cargo", "fetch", "--locked"]);
        assert_eq!(
            valid.vendor_command,
            vec!["cargo", "vendor", "--locked", "vendor"]
        );

        let mut unlisted = brokered_fetch();
        unlisted.registry = "https://example.invalid/index".into();
        assert!(matches!(
            unlisted.validate(),
            Err(DependencyError::RegistryNotAllowlisted(_))
        ));

        let mut missing_lockfile = brokered_fetch();
        missing_lockfile.lockfile_path = "Cargo.toml".into();
        assert!(missing_lockfile.validate().is_err());

        let mut missing_hash = brokered_fetch();
        missing_hash.lockfile_sha256.clear();
        assert!(missing_hash.validate().is_err());

        let mut non_atomic = brokered_fetch();
        non_atomic.write_mode = VendorWriteMode::InPlace;
        assert_eq!(
            non_atomic.validate().expect_err("non-atomic"),
            DependencyError::NonAtomicVendorWrite
        );

        let mut unlocked = brokered_fetch();
        unlocked.fetch_command = vec!["cargo".into(), "fetch".into()];
        assert!(unlocked.validate().is_err());
    }

    #[test]
    fn brokered_fetch_rejects_credentials_and_check_execution() {
        let mut credentialed = brokered_fetch();
        credentialed.credentials = CredentialMode::Ambient;
        assert_eq!(
            credentialed.validate().expect_err("credentials"),
            DependencyError::CredentialsNotAllowed
        );

        let mut checks = brokered_fetch();
        checks.execute_checks = true;
        assert!(matches!(
            checks.validate(),
            Err(DependencyError::Invalid(message)) if message.contains("checks")
        ));
    }

    #[test]
    fn both_acquisition_requires_matching_broker_contract() {
        let plan = plan(DependencyStrategy::Both);
        assert!(plan.validate().is_ok());
        assert!(plan.brokered_fetch.is_some());

        let mut fetch = brokered_fetch();
        fetch.vendor_path = "cache".into();
        fetch.vendor_command = vec![
            "cargo".into(),
            "vendor".into(),
            "--locked".into(),
            "cache".into(),
        ];
        assert!(matches!(
            DependencyPlan::from_planning(
                &planning_plan(DependencyStrategy::Both),
                DependencyKind::Rust,
                evidence(),
                Some(fetch)
            ),
            Err(DependencyError::Invalid(_))
        ));
    }

    #[test]
    fn not_required_plan_needs_no_evidence_or_commands() {
        let plan = DependencyPlan::from_planning(
            &planning_plan(DependencyStrategy::NotRequired),
            DependencyKind::Rust,
            DependencyEvidence {
                lockfile_sha256: String::new(),
                vendor_sha256: String::new(),
                sbom_ref: String::new(),
                sbom_sha256: String::new(),
                license_report_ref: String::new(),
                license_report_sha256: String::new(),
                advisory_report_ref: String::new(),
                advisory_report_sha256: String::new(),
            },
            None,
        )
        .expect("not-required plan");
        assert!(plan.validate().is_ok());
        assert!(plan.rust.is_none());
        assert!(plan.commands().is_none());
    }

    #[test]
    fn non_rust_plans_reject_rust_commands_and_brokers() {
        let planning = planning_plan(DependencyStrategy::VendorCache);
        assert!(matches!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Node, evidence())
                .rust(rust_plan())
                .build(),
            Err(DependencyError::Invalid(_))
        ));

        assert!(matches!(
            DependencyPlanBuilder::new(&planning, DependencyKind::Python, evidence())
                .brokered_fetch(brokered_fetch())
                .build(),
            Err(DependencyError::Invalid(_))
        ));
    }
}
