//! Deterministic OpenSpec repository projection.
//!
//! Core renders an `openspec/changes/<change-id>/` folder from frozen typed
//! planning artifacts. The projection is idempotent, revision-bound and never a
//! second authority: edited markdown is draft context until Core validates and
//! freezes a new revision. This module performs no I/O and no model calls.
//!
//! A change is projected as a single capability named after the change id. The
//! `design` field carries dependency and ADR notes; the stack and architecture
//! summaries are rendered separately.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const OPENSPEC_CHANGES_DIR: &str = "openspec/changes";
pub const OPENSPEC_ARCHIVE_DIR: &str = "openspec/changes/archive";

pub const MAX_CHANGE_ID_CHARS: usize = 64;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_WHY_CHARS: usize = 8_000;
pub const MAX_SUMMARY_CHARS: usize = 8_000;
pub const MAX_DESIGN_CHARS: usize = 32_000;
pub const MAX_LIST_ITEMS: usize = 32;
pub const MAX_LIST_ITEM_CHARS: usize = 1_000;
pub const MAX_REQUIREMENTS: usize = 64;
pub const MAX_ID_CHARS: usize = 64;
pub const MAX_REQUIREMENT_STATEMENT_CHARS: usize = 4_000;
pub const MAX_SCENARIOS: usize = 16;
pub const MAX_SCENARIO_CHARS: usize = 2_000;
pub const MAX_TASKS: usize = 96;
pub const MAX_TASK_TITLE_CHARS: usize = 300;
pub const MAX_TASK_PATHS: usize = 32;
pub const MAX_PATH_CHARS: usize = 512;
pub const MAX_EVIDENCE_ITEMS: usize = 16;
pub const MAX_EVIDENCE_CHARS: usize = 2_000;
pub const MAX_TOTAL_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 512 * 1024;
pub const MAX_TOTAL_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenSpecError {
    #[error("invalid OpenSpec change: {0}")]
    InvalidChange(String),
    #[error("invalid OpenSpec artifact: {0}")]
    InvalidArtifact(String),
    #[error("OpenSpec artifact exceeds its bound: {0}")]
    TooLarge(String),
}

/// The typed input to the OpenSpec renderer. This is not a model contract; Core
/// validates and freezes it before rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecChange {
    pub change_id: String,
    pub title: String,
    pub why: String,
    #[serde(default)]
    pub what_changes: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    pub requirements: Vec<OpenSpecRequirement>,
    #[serde(default)]
    pub design: String,
    pub tasks: Vec<OpenSpecTask>,
    #[serde(default)]
    pub stack_summary: String,
    #[serde(default)]
    pub architecture_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecRequirement {
    pub id: String,
    pub statement: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub negative: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecSpecArtifact {
    pub capability: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecArtifactHash {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecManifest {
    pub change_id: String,
    pub revision: u32,
    pub artifacts: Vec<OpenSpecArtifactHash>,
}

impl OpenSpecManifest {
    pub fn hash(&self, path: &str) -> Option<&str> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.path.as_str() == path)
            .map(|artifact| artifact.sha256.as_str())
    }

    /// A stable digest of the complete specs delta. Individual spec files are
    /// already hashed; this binds the ordered set for `OpenSpecProjection`.
    pub fn specs_sha256(&self) -> Option<String> {
        let specs: Vec<&OpenSpecArtifactHash> = self
            .artifacts
            .iter()
            .filter(|artifact| artifact.path.starts_with("specs/"))
            .collect();
        if specs.is_empty() {
            return None;
        }
        let mut hasher = Sha256::new();
        for spec in specs {
            hasher.update(spec.path.as_bytes());
            hasher.update([0]);
            hasher.update(spec.sha256.as_bytes());
            hasher.update([0]);
        }
        let digest = hasher.finalize();
        let mut output = String::with_capacity(64);
        for byte in digest.iter() {
            output.push_str(&format!("{byte:02x}"));
        }
        Some(output)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecArtifacts {
    pub change_id: String,
    pub revision: u32,
    pub proposal: String,
    pub specs: Vec<OpenSpecSpecArtifact>,
    pub design: Option<String>,
    pub tasks: String,
    pub manifest: OpenSpecManifest,
}

/// One rendered file in the read model consumed by the Compose validation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecRenderedFile {
    pub path: String,
    pub sha256: String,
    pub content: String,
}

/// The exact hashes Core stores for the latest durable projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecProjectionHashes {
    pub proposal_sha256: String,
    pub specs_sha256: String,
    pub design_sha256: String,
    pub tasks_sha256: String,
}

/// A request-bound, read-only projection of the latest OpenSpec revision.
///
/// This is a transport model for the operator-authenticated validation
/// endpoint. It is not authority: Core still owns the stored projection and the
/// rendered files remain a projection until Core freezes a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecReadModel {
    pub request_id: String,
    pub change_id: String,
    pub change_path: String,
    pub revision: u32,
    pub validated: bool,
    pub archived: bool,
    pub artifact_content_sha256: String,
    pub hashes: OpenSpecProjectionHashes,
    pub manifest: OpenSpecManifest,
    pub files: Vec<OpenSpecRenderedFile>,
}

impl OpenSpecArtifacts {
    /// The hashed artifacts in deterministic write order. `manifest.json` is
    /// deliberately absent because the manifest cannot hash itself.
    pub fn files(&self) -> Vec<(&str, &str)> {
        let mut files = Vec::with_capacity(3 + self.specs.len());
        files.push(("proposal.md", self.proposal.as_str()));
        for spec in &self.specs {
            files.push((spec.path.as_str(), spec.content.as_str()));
        }
        if let Some(design) = self.design.as_deref() {
            files.push(("design.md", design));
        }
        files.push(("tasks.md", self.tasks.as_str()));
        files
    }

    pub fn artifact(&self, path: &str) -> Option<&str> {
        match path {
            "proposal.md" => Some(self.proposal.as_str()),
            "tasks.md" => Some(self.tasks.as_str()),
            "design.md" => self.design.as_deref(),
            _ => self
                .specs
                .iter()
                .find(|spec| spec.path.as_str() == path)
                .map(|spec| spec.content.as_str()),
        }
    }

    pub fn manifest_json(&self) -> String {
        let mut json =
            serde_json::to_string_pretty(&self.manifest).unwrap_or_else(|_| "{}".to_owned());
        json.push('\n');
        json
    }

    pub fn validate(&self) -> Result<(), OpenSpecError> {
        validate_artifacts(self)
    }

    /// Project the validated artifacts into the durable Core record. The caller
    /// supplies the Core-owned `validated` bit; this method never claims
    /// acceptance on its own.
    pub fn projection(
        &self,
        validated: bool,
    ) -> Result<crate::planning::OpenSpecProjection, OpenSpecError> {
        validate_artifacts(self)?;
        let proposal = self.manifest.hash("proposal.md").ok_or_else(|| {
            OpenSpecError::InvalidArtifact("proposal hash is missing from the manifest".into())
        })?;
        let tasks = self.manifest.hash("tasks.md").ok_or_else(|| {
            OpenSpecError::InvalidArtifact("tasks hash is missing from the manifest".into())
        })?;
        let specs = self.manifest.specs_sha256().ok_or_else(|| {
            OpenSpecError::InvalidArtifact("specs hash is missing from the manifest".into())
        })?;
        let design = self
            .manifest
            .hash("design.md")
            .unwrap_or_default()
            .to_owned();
        let projection = crate::planning::OpenSpecProjection {
            change_id: self.change_id.clone(),
            revision: self.revision,
            proposal_sha256: proposal.to_owned(),
            specs_sha256: specs,
            design_sha256: design,
            tasks_sha256: tasks.to_owned(),
            validated,
            archived: false,
        };
        projection
            .validate()
            .map_err(OpenSpecError::InvalidArtifact)?;
        Ok(projection)
    }
}

/// Build the exact read model for the latest persisted projection.
///
/// The projection record and the stored artifact JSON must describe the same
/// change, revision and hashes. A mismatch fails closed; Core never serves a
/// stale artifact as if it were the current projection.
pub fn read_model(
    request_id: &str,
    projection: &crate::planning::OpenSpecProjection,
    artifact_content_sha256: &str,
    artifacts: &OpenSpecArtifacts,
) -> Result<OpenSpecReadModel, OpenSpecError> {
    if request_id.is_empty()
        || request_id.chars().count() > 200
        || request_id.chars().any(char::is_control)
    {
        return Err(OpenSpecError::InvalidArtifact(
            "request id is required, bounded and control-free".into(),
        ));
    }
    projection
        .validate()
        .map_err(OpenSpecError::InvalidArtifact)?;
    validate_artifacts(artifacts)?;
    if artifacts.change_id != projection.change_id || artifacts.revision != projection.revision {
        return Err(OpenSpecError::InvalidArtifact(
            "stored artifact does not match the projected change revision".into(),
        ));
    }
    if artifact_content_sha256.len() != 64
        || !artifact_content_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OpenSpecError::InvalidArtifact(
            "stored artifact hash must be a lowercase SHA-256".into(),
        ));
    }
    let expected = artifacts.projection(projection.validated)?;
    if expected.change_id != projection.change_id
        || expected.revision != projection.revision
        || expected.proposal_sha256 != projection.proposal_sha256
        || expected.specs_sha256 != projection.specs_sha256
        || expected.design_sha256 != projection.design_sha256
        || expected.tasks_sha256 != projection.tasks_sha256
    {
        return Err(OpenSpecError::InvalidArtifact(
            "stored artifact hashes do not match the durable projection".into(),
        ));
    }

    let mut files = Vec::with_capacity(artifacts.files().len());
    for (path, content) in artifacts.files() {
        let sha256 = artifacts.manifest.hash(path).ok_or_else(|| {
            OpenSpecError::InvalidArtifact(format!("artifact {path} is missing its manifest hash"))
        })?;
        files.push(OpenSpecRenderedFile {
            path: path.to_owned(),
            sha256: sha256.to_owned(),
            content: content.to_owned(),
        });
    }

    Ok(OpenSpecReadModel {
        request_id: request_id.to_owned(),
        change_id: projection.change_id.clone(),
        change_path: change_path(&projection.change_id)?,
        revision: projection.revision,
        validated: projection.validated,
        archived: projection.archived,
        artifact_content_sha256: artifact_content_sha256.to_owned(),
        hashes: OpenSpecProjectionHashes {
            proposal_sha256: projection.proposal_sha256.clone(),
            specs_sha256: projection.specs_sha256.clone(),
            design_sha256: projection.design_sha256.clone(),
            tasks_sha256: projection.tasks_sha256.clone(),
        },
        manifest: artifacts.manifest.clone(),
        files,
    })
}

/// Verify the exact stored artifact JSON before decoding it.
///
/// The durable record stores both the JSON bytes and their SHA-256. A row whose
/// digest no longer matches its bytes is drift or corruption; Core fails closed
/// instead of serving or archiving it.
pub fn parse_stored_artifact(
    content_json: &str,
    stored_sha256: &str,
) -> Result<OpenSpecArtifacts, OpenSpecError> {
    if stored_sha256.len() != 64
        || !stored_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OpenSpecError::InvalidArtifact(
            "stored artifact hash must be a lowercase SHA-256".into(),
        ));
    }
    if sha256_hex(content_json.as_bytes()) != stored_sha256 {
        return Err(OpenSpecError::InvalidArtifact(
            "stored artifact JSON does not match its stored hash".into(),
        ));
    }
    let artifacts: OpenSpecArtifacts = serde_json::from_str(content_json).map_err(|_| {
        OpenSpecError::InvalidArtifact("stored artifact JSON is not decodable".into())
    })?;
    validate_artifacts(&artifacts)?;
    Ok(artifacts)
}

/// The exact Core-owned archive transition result.
///
/// Archiving is a projection transition only. It never accepts a candidate,
/// closes a finding or grants delivery authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct OpenSpecArchiveOutcome {
    pub request_id: String,
    pub change_id: String,
    pub revision: u32,
    pub archive_path: String,
    pub archived: bool,
    pub already_archived: bool,
    pub artifact_content_sha256: String,
    pub hashes: OpenSpecProjectionHashes,
}

/// Build the archive response from the exact read model that passed drift and
/// hash verification.
pub fn archive_outcome(
    model: &OpenSpecReadModel,
    already_archived: bool,
) -> Result<OpenSpecArchiveOutcome, OpenSpecError> {
    if !model.validated {
        return Err(OpenSpecError::InvalidArtifact(
            "only a validated OpenSpec projection can be archived".into(),
        ));
    }
    Ok(OpenSpecArchiveOutcome {
        request_id: model.request_id.clone(),
        change_id: model.change_id.clone(),
        revision: model.revision,
        archive_path: archive_path(&model.change_id)?,
        archived: true,
        already_archived,
        artifact_content_sha256: model.artifact_content_sha256.clone(),
        hashes: model.hashes.clone(),
    })
}

impl OpenSpecChange {
    pub fn validate(&self) -> Result<(), OpenSpecError> {
        validate_change(self)
    }
}

/// A safe OpenSpec change id is a bounded lowercase slug. `archive` is reserved
/// because it is the archive directory name.
pub fn is_valid_change_id(value: &str) -> bool {
    is_lowercase_slug(value, MAX_CHANGE_ID_CHARS) && value != "archive"
}

pub fn change_path(change_id: &str) -> Result<String, OpenSpecError> {
    validate_change_id(change_id)?;
    Ok(format!("{OPENSPEC_CHANGES_DIR}/{change_id}"))
}

pub fn archive_path(change_id: &str) -> Result<String, OpenSpecError> {
    validate_change_id(change_id)?;
    Ok(format!("{OPENSPEC_ARCHIVE_DIR}/{change_id}"))
}

pub fn validate_change(change: &OpenSpecChange) -> Result<(), OpenSpecError> {
    validate_change_id(&change.change_id)?;
    validate_line("title", &change.title, MAX_TITLE_CHARS, true)?;
    validate_multiline("why", &change.why, MAX_WHY_CHARS, true)?;
    validate_list(
        "what changes",
        &change.what_changes,
        MAX_LIST_ITEMS,
        MAX_LIST_ITEM_CHARS,
        false,
    )?;
    validate_list(
        "non-goals",
        &change.non_goals,
        MAX_LIST_ITEMS,
        MAX_LIST_ITEM_CHARS,
        false,
    )?;
    validate_multiline("design", &change.design, MAX_DESIGN_CHARS, false)?;
    validate_multiline(
        "stack summary",
        &change.stack_summary,
        MAX_SUMMARY_CHARS,
        false,
    )?;
    validate_multiline(
        "architecture summary",
        &change.architecture_summary,
        MAX_SUMMARY_CHARS,
        false,
    )?;

    if change.requirements.is_empty() || change.requirements.len() > MAX_REQUIREMENTS {
        return Err(OpenSpecError::InvalidChange(format!(
            "requirements must contain 1-{MAX_REQUIREMENTS} entries"
        )));
    }
    let mut requirement_ids = BTreeSet::new();
    let mut all_ids = BTreeSet::new();
    for requirement in &change.requirements {
        if !is_safe_identifier(&requirement.id) {
            return Err(OpenSpecError::InvalidChange(
                "requirement id must be a safe bounded identifier".into(),
            ));
        }
        if !requirement_ids.insert(requirement.id.as_str()) {
            return Err(OpenSpecError::InvalidChange(format!(
                "duplicate requirement id {}",
                requirement.id
            )));
        }
        if !all_ids.insert(requirement.id.as_str()) {
            return Err(OpenSpecError::InvalidChange(format!(
                "duplicate id {} across requirements and tasks",
                requirement.id
            )));
        }
        validate_line(
            "requirement statement",
            &requirement.statement,
            MAX_REQUIREMENT_STATEMENT_CHARS,
            true,
        )?;
        validate_list(
            "requirement acceptance",
            &requirement.acceptance,
            MAX_SCENARIOS,
            MAX_SCENARIO_CHARS,
            true,
        )?;
        validate_list(
            "requirement negative",
            &requirement.negative,
            MAX_SCENARIOS,
            MAX_SCENARIO_CHARS,
            true,
        )?;
    }

    if change.tasks.is_empty() || change.tasks.len() > MAX_TASKS {
        return Err(OpenSpecError::InvalidChange(format!(
            "tasks must contain 1-{MAX_TASKS} entries"
        )));
    }
    let mut task_ids = BTreeSet::new();
    for task in &change.tasks {
        if !is_safe_identifier(&task.id) {
            return Err(OpenSpecError::InvalidChange(
                "task id must be a safe bounded identifier".into(),
            ));
        }
        if !task_ids.insert(task.id.as_str()) {
            return Err(OpenSpecError::InvalidChange(format!(
                "duplicate task id {}",
                task.id
            )));
        }
        if !all_ids.insert(task.id.as_str()) {
            return Err(OpenSpecError::InvalidChange(format!(
                "duplicate id {} across requirements and tasks",
                task.id
            )));
        }
        validate_line("task title", &task.title, MAX_TASK_TITLE_CHARS, true)?;
        if task.paths.is_empty() || task.paths.len() > MAX_TASK_PATHS {
            return Err(OpenSpecError::InvalidChange(format!(
                "task {} must contain 1-{MAX_TASK_PATHS} paths",
                task.id
            )));
        }
        let mut paths = BTreeSet::new();
        for path in &task.paths {
            validate_project_path("task path", path)?;
            if !paths.insert(path.as_str()) {
                return Err(OpenSpecError::InvalidChange(format!(
                    "task {} contains duplicate path {path}",
                    task.id
                )));
            }
        }
        validate_list(
            "task acceptance",
            &task.acceptance,
            MAX_SCENARIOS,
            MAX_SCENARIO_CHARS,
            true,
        )?;
        validate_list(
            "task evidence",
            &task.evidence,
            MAX_EVIDENCE_ITEMS,
            MAX_EVIDENCE_CHARS,
            true,
        )?;
    }

    if total_input_bytes(change) > MAX_TOTAL_INPUT_BYTES {
        return Err(OpenSpecError::InvalidChange(format!(
            "change input exceeds {MAX_TOTAL_INPUT_BYTES} bytes"
        )));
    }
    Ok(())
}

/// Render revision 1 of the deterministic OpenSpec change folder.
pub fn render(change: &OpenSpecChange) -> Result<OpenSpecArtifacts, OpenSpecError> {
    render_revision(change, 1)
}

/// Render a revision-bound projection. Core owns the revision; the renderer
/// never derives it from content or time.
pub fn render_revision(
    change: &OpenSpecChange,
    revision: u32,
) -> Result<OpenSpecArtifacts, OpenSpecError> {
    if revision == 0 {
        return Err(OpenSpecError::InvalidChange(
            "OpenSpec revision must be positive".into(),
        ));
    }
    validate_change(change)?;

    let proposal = render_proposal(change, revision);
    let specs = vec![OpenSpecSpecArtifact {
        capability: change.change_id.clone(),
        path: format!("specs/{}/spec.md", change.change_id),
        content: render_spec(change),
    }];
    let design = has_design(change).then(|| render_design(change, revision));
    let tasks = render_tasks(change, revision);
    let manifest = build_manifest(
        &change.change_id,
        revision,
        &proposal,
        &specs,
        design.as_deref(),
        &tasks,
    );
    let artifacts = OpenSpecArtifacts {
        change_id: change.change_id.clone(),
        revision,
        proposal,
        specs,
        design,
        tasks,
        manifest,
    };
    validate_artifact_bounds(&artifacts)?;
    Ok(artifacts)
}

/// Validate the exact rendered artifacts. This is a deterministic projection
/// check, not an acceptance decision; Core remains authoritative.
pub fn validate_artifacts(artifacts: &OpenSpecArtifacts) -> Result<(), OpenSpecError> {
    if !is_valid_change_id(&artifacts.change_id) {
        return Err(OpenSpecError::InvalidArtifact(
            "artifact change id must be a safe lowercase slug".into(),
        ));
    }
    if artifacts.revision == 0 {
        return Err(OpenSpecError::InvalidArtifact(
            "artifact revision must be positive".into(),
        ));
    }
    validate_artifact_bounds(artifacts)?;
    if artifacts.specs.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(
            "at least one spec artifact is required".into(),
        ));
    }
    let spec_links: Vec<&str> = artifacts
        .specs
        .iter()
        .map(|spec| spec.path.as_str())
        .collect();
    validate_proposal(&artifacts.proposal, &spec_links, artifacts.design.is_some())?;

    let mut requirements = Vec::new();
    let mut spec_paths = BTreeSet::new();
    for spec in &artifacts.specs {
        validate_spec_path(spec)?;
        if !spec_paths.insert(spec.path.as_str()) {
            return Err(OpenSpecError::InvalidArtifact(
                "duplicate spec artifact path".into(),
            ));
        }
        validate_spec_headings(&spec.content, &spec.capability)?;
        requirements.extend(parse_spec(&spec.content)?);
    }
    let mut requirement_ids = BTreeSet::new();
    for requirement in &requirements {
        if !requirement_ids.insert(requirement.id.clone()) {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "duplicate requirement id {}",
                requirement.id
            )));
        }
    }
    let mut scenario_ids = BTreeSet::new();
    for requirement in &requirements {
        for scenario in requirement
            .acceptance
            .iter()
            .chain(requirement.negative.iter())
        {
            if !scenario_ids.insert(scenario.clone()) {
                return Err(OpenSpecError::InvalidArtifact(format!(
                    "duplicate scenario id {scenario}"
                )));
            }
        }
    }

    if let Some(design) = artifacts.design.as_deref() {
        validate_design(design)?;
    }
    validate_tasks_headings(&artifacts.tasks)?;
    let tasks = parse_tasks(&artifacts.tasks)?;
    let traceability = parse_traceability(&artifacts.tasks)?;
    validate_traceability(&requirements, &tasks, &traceability)?;
    validate_manifest(artifacts)?;
    Ok(())
}

fn validate_change_id(change_id: &str) -> Result<(), OpenSpecError> {
    if is_valid_change_id(change_id) {
        Ok(())
    } else {
        Err(OpenSpecError::InvalidChange(format!(
            "change id must be a lowercase slug of 1-{MAX_CHANGE_ID_CHARS} characters, start and end with an alphanumeric, and must not be 'archive'"
        )))
    }
}

fn is_lowercase_slug(value: &str, max_len: usize) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > max_len {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return false;
    }
    let mut previous_hyphen = false;
    for byte in bytes {
        match *byte {
            b'a'..=b'z' | b'0'..=b'9' => previous_hyphen = false,
            b'-' if !previous_hyphen => previous_hyphen = true,
            _ => return false,
        }
    }
    true
}

fn is_safe_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_ID_CHARS {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return false;
    }
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.'))
    {
        return false;
    }
    !value.contains("..")
}

fn validate_line(
    field: &str,
    value: &str,
    max_chars: usize,
    required: bool,
) -> Result<(), OpenSpecError> {
    if required && value.trim().is_empty() {
        return Err(OpenSpecError::InvalidChange(format!("{field} is required")));
    }
    if value.chars().count() > max_chars {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} exceeds {max_chars} characters"
        )));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\t') {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} must be a single line"
        )));
    }
    validate_control_and_secrets(field, value)
}

fn validate_multiline(
    field: &str,
    value: &str,
    max_chars: usize,
    required: bool,
) -> Result<(), OpenSpecError> {
    if required && value.trim().is_empty() {
        return Err(OpenSpecError::InvalidChange(format!("{field} is required")));
    }
    if value.chars().count() > max_chars {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} exceeds {max_chars} characters"
        )));
    }
    validate_control_and_secrets(field, value)
}

fn validate_control_and_secrets(field: &str, value: &str) -> Result<(), OpenSpecError> {
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} contains a control character"
        )));
    }
    if contains_secret_shaped_value(value) {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} contains a secret-shaped value"
        )));
    }
    Ok(())
}

fn validate_list(
    field: &str,
    items: &[String],
    max_items: usize,
    max_chars: usize,
    required: bool,
) -> Result<(), OpenSpecError> {
    if required && items.is_empty() {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} must contain at least one entry"
        )));
    }
    if items.len() > max_items {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} exceeds {max_items} entries"
        )));
    }
    for item in items {
        validate_line(field, item, max_chars, true)?;
    }
    Ok(())
}

fn validate_project_path(field: &str, path: &str) -> Result<(), OpenSpecError> {
    if path.is_empty() || path.chars().count() > MAX_PATH_CHARS {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} must be a bounded project-relative path"
        )));
    }
    if path.trim() != path
        || path.starts_with('/')
        || path.starts_with('~')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} must be a project-relative path without traversal"
        )));
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} must not be a Windows absolute path"
        )));
    }
    if path.contains("://") || path.chars().any(char::is_control) {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} must be a plain project-relative path"
        )));
    }
    if is_secret_shaped_path(path) {
        return Err(OpenSpecError::InvalidChange(format!(
            "{field} looks like a secret-bearing path"
        )));
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component == ".git"
            || component.trim() != component
        {
            return Err(OpenSpecError::InvalidChange(format!(
                "{field} must not contain empty, dot, dot-dot or .git components"
            )));
        }
        if component
            .chars()
            .any(|character| matches!(character, ':' | '*' | '?' | '"' | '<' | '>' | '|' | '`'))
        {
            return Err(OpenSpecError::InvalidChange(format!(
                "{field} contains a character that is unsafe in a portable path"
            )));
        }
    }
    Ok(())
}

fn is_secret_shaped_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or_default();
    file.starts_with(".env")
        || matches!(
            file,
            "id_rsa"
                | "id_ed25519"
                | "credentials"
                | "credentials.json"
                | "secrets"
                | "secrets.json"
        )
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|extension| lower.ends_with(extension))
}

fn total_input_bytes(change: &OpenSpecChange) -> usize {
    let mut total = change.change_id.len()
        + change.title.len()
        + change.why.len()
        + change.design.len()
        + change.stack_summary.len()
        + change.architecture_summary.len();
    for item in &change.what_changes {
        total += item.len();
    }
    for item in &change.non_goals {
        total += item.len();
    }
    for requirement in &change.requirements {
        total += requirement.id.len() + requirement.statement.len();
        for item in &requirement.acceptance {
            total += item.len();
        }
        for item in &requirement.negative {
            total += item.len();
        }
    }
    for task in &change.tasks {
        total += task.id.len() + task.title.len();
        for path in &task.paths {
            total += path.len();
        }
        for item in &task.acceptance {
            total += item.len();
        }
        for item in &task.evidence {
            total += item.len();
        }
    }
    total
}

fn has_design(change: &OpenSpecChange) -> bool {
    !change.design.trim().is_empty()
        || !change.stack_summary.trim().is_empty()
        || !change.architecture_summary.trim().is_empty()
}

fn render_proposal(change: &OpenSpecChange, revision: u32) -> String {
    let mut output = String::new();
    output.push_str("# ");
    output.push_str(change.title.trim());
    output.push_str("\n\n");
    output.push_str(&format!(
        "Change: `{}` (revision {revision})\n\n",
        change.change_id
    ));
    output.push_str("## Why\n\n");
    output.push_str(&normalize_multiline(&change.why));
    output.push_str("\n\n## What Changes\n\n");
    push_bullets(&mut output, &change.what_changes, "None recorded.");
    output.push_str("\n## Non-Goals\n\n");
    push_bullets(&mut output, &change.non_goals, "None recorded.");
    output.push_str("\n## Artifacts\n\n");
    output.push_str(&format!(
        "- [Specification](specs/{}/spec.md)\n",
        change.change_id
    ));
    if has_design(change) {
        output.push_str("- [Design](design.md)\n");
    }
    output.push_str("- [Tasks](tasks.md)\n");
    output.push_str("\n---\n");
    output.push_str(
        "Core is authoritative. This OpenSpec change is a deterministic projection; edited markdown is draft context until Core validates and freezes a new revision.\n",
    );
    output
}

fn render_spec(change: &OpenSpecChange) -> String {
    let mut output = String::new();
    // OpenSpec archives the first `## ` section onward. A title or metadata
    // preamble above the delta is not carried into `openspec/specs/` and is a
    // strict-validation warning. The proposal and design already carry the
    // change identity and revision.
    output.push_str("## ADDED Requirements\n\n");
    for requirement in &change.requirements {
        output.push_str(&format!("### Requirement: {}\n\n", requirement.id));
        output.push_str(&normalize_multiline(&requirement.statement));
        output.push_str("\n\n");
        for (index, scenario) in requirement.acceptance.iter().enumerate() {
            output.push_str(&format!(
                "#### Scenario: {}-ACC-{:02}\n\n",
                requirement.id,
                index + 1
            ));
            output.push_str("- **WHEN** the acceptance case is exercised\n");
            output.push_str(&format!("- **THEN** {}\n\n", one_line(scenario)));
        }
        for (index, scenario) in requirement.negative.iter().enumerate() {
            output.push_str(&format!(
                "#### Scenario: {}-NEG-{:02}\n\n",
                requirement.id,
                index + 1
            ));
            output.push_str("- **WHEN** the negative case is exercised\n");
            output.push_str(&format!("- **THEN** {}\n\n", one_line(scenario)));
        }
    }
    output.push_str("---\n");
    output.push_str(
        "Core is authoritative. Requirement and scenario identifiers are deterministic projections of the frozen specification.\n",
    );
    output
}

fn render_design(change: &OpenSpecChange, revision: u32) -> String {
    let mut output = String::new();
    output.push_str(&format!("# Design: {}\n\n", change.title.trim()));
    output.push_str(&format!(
        "Change: `{}` (revision {revision})\n\n",
        change.change_id
    ));
    output.push_str("## Stack\n\n");
    push_paragraph(
        &mut output,
        &change.stack_summary,
        "No stack decision recorded.",
    );
    output.push_str("\n## Architecture\n\n");
    push_paragraph(
        &mut output,
        &change.architecture_summary,
        "No architecture decision recorded.",
    );
    output.push_str("\n## Design Notes\n\n");
    push_paragraph(
        &mut output,
        &change.design,
        "No additional design notes recorded.",
    );
    output.push_str("\n## Dependencies\n\n");
    output.push_str(
        "This projection does not add, remove or approve a dependency. Dependency decisions are recorded in the design notes above and in the stack decision.\n",
    );
    output.push_str("\n## ADRs\n\n");
    output.push_str(
        "This projection does not create or approve an ADR. ADR references are recorded in the design notes above and in the architecture summary.\n",
    );
    output
}

fn render_tasks(change: &OpenSpecChange, revision: u32) -> String {
    let mut output = String::new();
    output.push_str(&format!("# Tasks: {}\n\n", change.title.trim()));
    output.push_str(&format!(
        "Change: `{}` (revision {revision})\n\n",
        change.change_id
    ));
    output.push_str("## 1. Implementation\n\n");
    for (task_index, task) in change.tasks.iter().enumerate() {
        let task_id = format!("1.{}", task_index + 1);
        output.push_str(&format!("- [ ] {task_id} — {}\n", task.title.trim()));
        output.push_str("  - Paths: ");
        for (index, path) in task.paths.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            output.push('`');
            output.push_str(path);
            output.push('`');
        }
        output.push('\n');
        output.push_str("  - Acceptance:\n");
        for acceptance in &task.acceptance {
            output.push_str("    - ");
            output.push_str(acceptance.trim());
            output.push('\n');
        }
        output.push_str("  - Evidence:\n");
        for evidence in &task.evidence {
            output.push_str("    - ");
            output.push_str(evidence.trim());
            output.push('\n');
        }
        let references = requirement_refs(change, task);
        output.push_str("  - Requirements: ");
        if references.is_empty() {
            output.push_str("unmapped");
        } else {
            output.push_str(&references.join(", "));
        }
        output.push('\n');
    }
    output.push_str("\n## Traceability\n\n");
    output.push_str("| Requirement | Tasks |\n");
    output.push_str("|---|---|\n");
    for requirement in &change.requirements {
        let tasks: Vec<String> = change
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| requirement_refs(change, task).contains(&requirement.id.as_str()))
            .map(|(index, _)| format!("1.{}", index + 1))
            .collect();
        output.push_str("| ");
        output.push_str(&requirement.id);
        output.push_str(" | ");
        if tasks.is_empty() {
            output.push_str("unmapped");
        } else {
            output.push_str(&tasks.join(", "));
        }
        output.push_str(" |\n");
    }
    output.push_str("\n---\n");
    output.push_str(
        "A checked box is not acceptance. Core validates the task, checks and evidence; this projection never grants completion.\n",
    );
    output
}

fn push_bullets(output: &mut String, items: &[String], empty: &str) {
    if items.is_empty() {
        output.push_str("- ");
        output.push_str(empty);
        output.push('\n');
        return;
    }
    for item in items {
        output.push_str("- ");
        output.push_str(item.trim());
        output.push('\n');
    }
}

fn push_paragraph(output: &mut String, value: &str, empty: &str) {
    let normalized = normalize_multiline(value);
    if normalized.is_empty() {
        output.push_str(empty);
    } else {
        output.push_str(&normalized);
    }
    output.push('\n');
}

fn normalize_multiline(value: &str) -> String {
    if value.trim().is_empty() {
        return String::new();
    }
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_owned()
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn requirement_refs<'a>(change: &'a OpenSpecChange, task: &OpenSpecTask) -> Vec<&'a str> {
    let mut references = Vec::new();
    for requirement in &change.requirements {
        let referenced = contains_identifier(&task.title, &requirement.id)
            || task
                .acceptance
                .iter()
                .any(|item| contains_identifier(item, &requirement.id))
            || task
                .evidence
                .iter()
                .any(|item| contains_identifier(item, &requirement.id));
        if referenced {
            references.push(requirement.id.as_str());
        }
    }
    references
}

fn contains_identifier(haystack: &str, identifier: &str) -> bool {
    if identifier.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(relative) = haystack[start..].find(identifier) {
        let index = start + relative;
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + identifier.len()..].chars().next();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
        };
        if boundary(before) && boundary(after) {
            return true;
        }
        start = index + identifier.len();
    }
    false
}

fn build_manifest(
    change_id: &str,
    revision: u32,
    proposal: &str,
    specs: &[OpenSpecSpecArtifact],
    design: Option<&str>,
    tasks: &str,
) -> OpenSpecManifest {
    let mut artifacts = Vec::with_capacity(3 + specs.len());
    artifacts.push(OpenSpecArtifactHash {
        path: "proposal.md".into(),
        sha256: sha256_hex(proposal.as_bytes()),
    });
    for spec in specs {
        artifacts.push(OpenSpecArtifactHash {
            path: spec.path.clone(),
            sha256: sha256_hex(spec.content.as_bytes()),
        });
    }
    if let Some(design) = design {
        artifacts.push(OpenSpecArtifactHash {
            path: "design.md".into(),
            sha256: sha256_hex(design.as_bytes()),
        });
    }
    artifacts.push(OpenSpecArtifactHash {
        path: "tasks.md".into(),
        sha256: sha256_hex(tasks.as_bytes()),
    });
    OpenSpecManifest {
        change_id: change_id.into(),
        revision,
        artifacts,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest.iter() {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn validate_artifact_bounds(artifacts: &OpenSpecArtifacts) -> Result<(), OpenSpecError> {
    let mut total = 0;
    for (name, content) in artifacts.files() {
        if content.is_empty() {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "artifact {name} is empty"
            )));
        }
        if content.len() > MAX_ARTIFACT_BYTES {
            return Err(OpenSpecError::TooLarge(format!(
                "artifact {name} exceeds {MAX_ARTIFACT_BYTES} bytes"
            )));
        }
        if content.contains('\0')
            || content
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "artifact {name} contains a control character"
            )));
        }
        if contains_secret_shaped_value(content) {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "artifact {name} contains a secret-shaped value"
            )));
        }
        total += content.len();
    }
    if total > MAX_TOTAL_ARTIFACT_BYTES {
        return Err(OpenSpecError::TooLarge(format!(
            "rendered artifacts exceed {MAX_TOTAL_ARTIFACT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_proposal(
    content: &str,
    spec_paths: &[&str],
    has_design: bool,
) -> Result<(), OpenSpecError> {
    let first = content.lines().next().unwrap_or_default();
    if first
        .strip_prefix("# ")
        .is_none_or(|title| title.trim().is_empty())
    {
        return Err(OpenSpecError::InvalidArtifact(
            "proposal.md is missing its title heading".into(),
        ));
    }
    for heading in ["## Why", "## What Changes", "## Non-Goals", "## Artifacts"] {
        if !content.lines().any(|line| line == heading) {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "proposal.md is missing the {heading} heading"
            )));
        }
    }
    if !spec_paths
        .iter()
        .any(|path| content.contains(&format!("]({path})")))
    {
        return Err(OpenSpecError::InvalidArtifact(
            "proposal.md is missing a link to a rendered specification".into(),
        ));
    }
    if !content.contains("](tasks.md)") {
        return Err(OpenSpecError::InvalidArtifact(
            "proposal.md is missing the tasks link".into(),
        ));
    }
    if has_design && !content.contains("](design.md)") {
        return Err(OpenSpecError::InvalidArtifact(
            "proposal.md is missing the design link".into(),
        ));
    }
    if !has_design && content.contains("](design.md)") {
        return Err(OpenSpecError::InvalidArtifact(
            "proposal.md links a design that was not rendered".into(),
        ));
    }
    Ok(())
}

fn validate_spec_path(spec: &OpenSpecSpecArtifact) -> Result<(), OpenSpecError> {
    if !is_lowercase_slug(&spec.capability, MAX_CHANGE_ID_CHARS) {
        return Err(OpenSpecError::InvalidArtifact(
            "spec capability must be a lowercase slug".into(),
        ));
    }
    let expected = format!("specs/{}/spec.md", spec.capability);
    if spec.path != expected {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "spec path must be {expected}"
        )));
    }
    Ok(())
}

fn validate_spec_headings(content: &str, capability: &str) -> Result<(), OpenSpecError> {
    if !is_lowercase_slug(capability, MAX_CHANGE_ID_CHARS) {
        return Err(OpenSpecError::InvalidArtifact(
            "spec capability must be a lowercase slug".into(),
        ));
    }
    if content.lines().next() != Some("## ADDED Requirements") {
        return Err(OpenSpecError::InvalidArtifact(
            "spec must start with the ADDED Requirements section".into(),
        ));
    }
    Ok(())
}

fn validate_design(content: &str) -> Result<(), OpenSpecError> {
    let first = content.lines().next().unwrap_or_default();
    if first
        .strip_prefix("# Design: ")
        .is_none_or(|title| title.trim().is_empty())
    {
        return Err(OpenSpecError::InvalidArtifact(
            "design.md is missing its title heading".into(),
        ));
    }
    for heading in [
        "## Stack",
        "## Architecture",
        "## Design Notes",
        "## Dependencies",
        "## ADRs",
    ] {
        if !content.lines().any(|line| line == heading) {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "design.md is missing the {heading} heading"
            )));
        }
    }
    Ok(())
}

fn validate_tasks_headings(content: &str) -> Result<(), OpenSpecError> {
    let first = content.lines().next().unwrap_or_default();
    if first
        .strip_prefix("# Tasks: ")
        .is_none_or(|title| title.trim().is_empty())
    {
        return Err(OpenSpecError::InvalidArtifact(
            "tasks.md is missing its title heading".into(),
        ));
    }
    if !content.lines().any(|line| {
        line.strip_prefix("## 1. ")
            .is_some_and(|title| !title.trim().is_empty())
    }) {
        return Err(OpenSpecError::InvalidArtifact(
            "tasks.md is missing its numeric task section".into(),
        ));
    }
    if !content.lines().any(|line| line == "## Traceability") {
        return Err(OpenSpecError::InvalidArtifact(
            "tasks.md is missing the Traceability heading".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Default)]
struct ParsedRequirement {
    id: String,
    acceptance: Vec<String>,
    negative: Vec<String>,
}

fn parse_spec(content: &str) -> Result<Vec<ParsedRequirement>, OpenSpecError> {
    let mut requirements = Vec::new();
    let mut current: Option<ParsedRequirement> = None;
    for line in content.lines() {
        if let Some(id) = line.strip_prefix("### Requirement: ") {
            if let Some(requirement) = current.take() {
                finish_requirement(requirement, &mut requirements)?;
            }
            let id = id.trim().to_owned();
            if !is_safe_identifier(&id) {
                return Err(OpenSpecError::InvalidArtifact(
                    "spec contains an unsafe requirement id".into(),
                ));
            }
            current = Some(ParsedRequirement {
                id,
                ..Default::default()
            });
            continue;
        }
        let Some(requirement) = current.as_mut() else {
            continue;
        };
        if let Some(scenario) = line.strip_prefix("#### Scenario: ") {
            let id = scenario.trim();
            if !is_safe_identifier(id) {
                return Err(OpenSpecError::InvalidArtifact(
                    "spec contains an unsafe scenario id".into(),
                ));
            }
            let acceptance = id.contains("-ACC-");
            let negative = id.contains("-NEG-");
            if acceptance == negative {
                return Err(OpenSpecError::InvalidArtifact(
                    "scenario id must identify exactly one of ACC or NEG".into(),
                ));
            }
            if acceptance {
                requirement.acceptance.push(id.to_owned());
            } else {
                requirement.negative.push(id.to_owned());
            }
        }
    }
    if let Some(requirement) = current {
        finish_requirement(requirement, &mut requirements)?;
    }
    if requirements.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(
            "spec contains no requirements".into(),
        ));
    }
    Ok(requirements)
}

fn finish_requirement(
    requirement: ParsedRequirement,
    requirements: &mut Vec<ParsedRequirement>,
) -> Result<(), OpenSpecError> {
    if requirement.acceptance.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "requirement {} has no acceptance scenario",
            requirement.id
        )));
    }
    if requirement.negative.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "requirement {} has no negative scenario",
            requirement.id
        )));
    }
    for (index, scenario) in requirement.acceptance.iter().enumerate() {
        let expected = format!("{}-ACC-{:02}", requirement.id, index + 1);
        if scenario != &expected {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "requirement {} acceptance scenario is not deterministic",
                requirement.id
            )));
        }
    }
    for (index, scenario) in requirement.negative.iter().enumerate() {
        let expected = format!("{}-NEG-{:02}", requirement.id, index + 1);
        if scenario != &expected {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "requirement {} negative scenario is not deterministic",
                requirement.id
            )));
        }
    }
    requirements.push(requirement);
    Ok(())
}

#[derive(Debug, Default)]
struct ParsedTask {
    id: String,
    has_paths: bool,
    has_acceptance: bool,
    acceptance_items: usize,
    has_evidence: bool,
    evidence_items: usize,
    requirements: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskSection {
    None,
    Acceptance,
    Evidence,
}

fn parse_tasks(content: &str) -> Result<Vec<ParsedTask>, OpenSpecError> {
    let mut tasks = Vec::new();
    let mut current: Option<ParsedTask> = None;
    let mut section = TaskSection::None;
    let mut in_traceability = false;
    for line in content.lines() {
        if line == "## Traceability" {
            in_traceability = true;
            if let Some(task) = current.take() {
                finish_task(task, &mut tasks)?;
            }
            continue;
        }
        if in_traceability {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("- [ ] ")
            .or_else(|| line.strip_prefix("- [x] "))
        {
            if let Some(task) = current.take() {
                finish_task(task, &mut tasks)?;
            }
            let id = rest
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
            if rest[id.len()..].trim().is_empty() {
                return Err(OpenSpecError::InvalidArtifact(
                    "task checkbox is missing its title".into(),
                ));
            }
            current = Some(ParsedTask {
                id,
                ..Default::default()
            });
            section = TaskSection::None;
            continue;
        }
        let Some(task) = current.as_mut() else {
            continue;
        };
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("- Paths:") {
            task.has_paths = !value.trim().is_empty();
            section = TaskSection::None;
        } else if trimmed == "- Acceptance:" {
            task.has_acceptance = true;
            section = TaskSection::Acceptance;
        } else if trimmed == "- Evidence:" {
            task.has_evidence = true;
            section = TaskSection::Evidence;
        } else if let Some(value) = trimmed.strip_prefix("- Requirements:") {
            task.requirements = parse_requirement_refs(value)?;
            section = TaskSection::None;
        } else if let Some(item) = trimmed.strip_prefix("- ")
            && !item.trim().is_empty()
        {
            match section {
                TaskSection::Acceptance => task.acceptance_items += 1,
                TaskSection::Evidence => task.evidence_items += 1,
                TaskSection::None => {}
            }
        }
    }
    if let Some(task) = current {
        finish_task(task, &mut tasks)?;
    }
    if tasks.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(
            "tasks.md contains no task checkboxes".into(),
        ));
    }
    Ok(tasks)
}

fn finish_task(task: ParsedTask, tasks: &mut Vec<ParsedTask>) -> Result<(), OpenSpecError> {
    if !is_safe_identifier(&task.id) {
        return Err(OpenSpecError::InvalidArtifact(
            "tasks.md contains an unsafe task id".into(),
        ));
    }
    if !task.has_paths {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "task {} is missing its paths",
            task.id
        )));
    }
    if !task.has_acceptance || task.acceptance_items == 0 {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "task {} has no acceptance statements",
            task.id
        )));
    }
    if !task.has_evidence || task.evidence_items == 0 {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "task {} has no evidence statements",
            task.id
        )));
    }
    if task.requirements.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(format!(
            "task {} is not traced to a requirement",
            task.id
        )));
    }
    tasks.push(task);
    Ok(())
}

fn parse_requirement_refs(value: &str) -> Result<Vec<String>, OpenSpecError> {
    let value = value.trim();
    if value == "unmapped" {
        return Ok(Vec::new());
    }
    if value.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(
            "task requirements line is empty".into(),
        ));
    }
    let mut references = Vec::new();
    let mut seen = BTreeSet::new();
    for item in value.split(',') {
        let id = item.trim();
        if !is_safe_identifier(id) {
            return Err(OpenSpecError::InvalidArtifact(
                "task requirements line contains an unsafe requirement id".into(),
            ));
        }
        if seen.insert(id.to_owned()) {
            references.push(id.to_owned());
        }
    }
    Ok(references)
}

fn parse_traceability(content: &str) -> Result<BTreeMap<String, Vec<String>>, OpenSpecError> {
    let mut rows = BTreeMap::new();
    let mut in_table = false;
    for line in content.lines() {
        if line == "## Traceability" {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        if cells.len() != 4 {
            continue;
        }
        let requirement = cells[1];
        let tasks = cells[2];
        if requirement.is_empty()
            || requirement == "Requirement"
            || requirement.chars().all(|character| character == '-')
        {
            continue;
        }
        if !is_safe_identifier(requirement) {
            return Err(OpenSpecError::InvalidArtifact(
                "traceability contains an unsafe requirement id".into(),
            ));
        }
        if rows.contains_key(requirement) {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "traceability contains duplicate requirement {requirement}"
            )));
        }
        let task_ids = if tasks == "unmapped" {
            Vec::new()
        } else {
            tasks
                .split(',')
                .map(str::trim)
                .filter(|task| !task.is_empty())
                .map(str::to_owned)
                .collect()
        };
        for task_id in &task_ids {
            if !is_safe_identifier(task_id) {
                return Err(OpenSpecError::InvalidArtifact(
                    "traceability contains an unsafe task id".into(),
                ));
            }
        }
        rows.insert(requirement.to_owned(), task_ids);
    }
    if rows.is_empty() {
        return Err(OpenSpecError::InvalidArtifact(
            "tasks.md contains no traceability rows".into(),
        ));
    }
    Ok(rows)
}

fn validate_traceability(
    requirements: &[ParsedRequirement],
    tasks: &[ParsedTask],
    rows: &BTreeMap<String, Vec<String>>,
) -> Result<(), OpenSpecError> {
    let requirement_ids: BTreeSet<&str> = requirements
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    let mut task_ids = BTreeSet::new();
    for task in tasks {
        if !task_ids.insert(task.id.as_str()) {
            return Err(OpenSpecError::InvalidArtifact(
                "tasks.md contains duplicate task ids".into(),
            ));
        }
        if task.requirements.is_empty() {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "task {} is not traced to a requirement",
                task.id
            )));
        }
        for reference in &task.requirements {
            if !requirement_ids.contains(reference.as_str()) {
                return Err(OpenSpecError::InvalidArtifact(format!(
                    "task {} references unknown requirement {reference}",
                    task.id
                )));
            }
        }
    }
    for requirement in requirements {
        let expected: Vec<&str> = tasks
            .iter()
            .filter(|task| task.requirements.contains(&requirement.id))
            .map(|task| task.id.as_str())
            .collect();
        if expected.is_empty() {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "requirement {} is not covered by any task",
                requirement.id
            )));
        }
        let row = rows.get(&requirement.id).ok_or_else(|| {
            OpenSpecError::InvalidArtifact(format!(
                "traceability is missing requirement {}",
                requirement.id
            ))
        })?;
        let row_tasks: Vec<&str> = row.iter().map(String::as_str).collect();
        if row_tasks != expected {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "traceability for {} does not match task references",
                requirement.id
            )));
        }
    }
    if rows.len() != requirements.len() {
        return Err(OpenSpecError::InvalidArtifact(
            "traceability contains unknown or missing requirements".into(),
        ));
    }
    Ok(())
}

fn validate_manifest(artifacts: &OpenSpecArtifacts) -> Result<(), OpenSpecError> {
    if artifacts.manifest.change_id != artifacts.change_id {
        return Err(OpenSpecError::InvalidArtifact(
            "manifest change id does not match the artifacts".into(),
        ));
    }
    if artifacts.manifest.revision != artifacts.revision {
        return Err(OpenSpecError::InvalidArtifact(
            "manifest revision does not match the artifacts".into(),
        ));
    }
    let mut expected = Vec::with_capacity(3 + artifacts.specs.len());
    expected.push("proposal.md".to_owned());
    for spec in &artifacts.specs {
        expected.push(spec.path.clone());
    }
    if artifacts.design.is_some() {
        expected.push("design.md".to_owned());
    }
    expected.push("tasks.md".to_owned());
    if artifacts.manifest.artifacts.len() != expected.len() {
        return Err(OpenSpecError::InvalidArtifact(
            "manifest does not contain every rendered artifact".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for (entry, path) in artifacts.manifest.artifacts.iter().zip(expected.iter()) {
        if &entry.path != path {
            return Err(OpenSpecError::InvalidArtifact(
                "manifest artifact order or path is not deterministic".into(),
            ));
        }
        if !seen.insert(entry.path.as_str()) {
            return Err(OpenSpecError::InvalidArtifact(
                "manifest contains a duplicate artifact path".into(),
            ));
        }
        if entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "manifest hash for {} is not a lowercase SHA-256",
                entry.path
            )));
        }
        let content = artifacts.artifact(&entry.path).ok_or_else(|| {
            OpenSpecError::InvalidArtifact(format!(
                "manifest references missing artifact {}",
                entry.path
            ))
        })?;
        if entry.sha256 != sha256_hex(content.as_bytes()) {
            return Err(OpenSpecError::InvalidArtifact(format!(
                "manifest hash for {} does not match the content",
                entry.path
            )));
        }
    }
    Ok(())
}

const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "api-key",
    "access_token",
    "auth_token",
    "private_key",
    "client_secret",
    "aws_secret_access_key",
    "credential",
    "credentials",
];

fn contains_secret_shaped_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("-----begin") && lower.contains("private key") {
        return true;
    }
    if contains_prefixed_token(&lower, "ghp_", 20)
        || contains_prefixed_token(&lower, "gho_", 20)
        || contains_prefixed_token(&lower, "ghu_", 20)
        || contains_prefixed_token(&lower, "ghs_", 20)
        || contains_prefixed_token(&lower, "ghr_", 20)
        || contains_prefixed_token(&lower, "github_pat_", 20)
        || contains_prefixed_token(&lower, "xoxb-", 10)
        || contains_prefixed_token(&lower, "xoxp-", 10)
        || contains_prefixed_token(&lower, "xoxa-", 10)
        || contains_prefixed_token(&lower, "xoxr-", 10)
        || contains_prefixed_token(&lower, "xoxs-", 10)
        || contains_prefixed_token(&lower, "sk-", 20)
        || contains_prefixed_token(value, "AIza", 35)
    {
        return true;
    }
    contains_aws_access_key(value)
        || contains_bearer_token(&lower)
        || contains_basic_auth(&lower)
        || contains_jwt(&lower)
        || contains_url_credentials(&lower)
        || contains_keyed_assignment(&lower)
}

fn contains_prefixed_token(haystack: &str, prefix: &str, min_tail: usize) -> bool {
    let mut start = 0;
    while let Some(relative) = haystack[start..].find(prefix) {
        let index = start + relative;
        let before = haystack[..index].chars().next_back();
        if before.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_') {
            start = index + prefix.len();
            continue;
        }
        let tail = &haystack[index + prefix.len()..];
        let count = tail
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric() || *character == '_' || *character == '-'
            })
            .count();
        if count >= min_tail {
            return true;
        }
        start = index + prefix.len();
    }
    false
}

fn contains_aws_access_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if bytes[index..].starts_with(b"AKIA") {
            let tail = &bytes[index + 4..];
            let count = tail
                .iter()
                .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                .count();
            if count >= 16 {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn contains_bearer_token(lower: &str) -> bool {
    let mut start = 0;
    while let Some(relative) = lower[start..].find("bearer ") {
        let index = start + relative + "bearer ".len();
        let tail = &lower[index..];
        let count = tail
            .chars()
            .take_while(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(*character, '-' | '_' | '.' | '~' | '+' | '/' | '=')
            })
            .count();
        if count >= 20 {
            return true;
        }
        start = index;
    }
    false
}

fn contains_basic_auth(lower: &str) -> bool {
    let mut start = 0;
    while let Some(relative) = lower[start..].find("basic ") {
        let index = start + relative;
        let prefix = lower[..index].trim_end();
        if prefix.ends_with("authorization:") || prefix.ends_with("authorization") {
            let tail = &lower[index + "basic ".len()..];
            let count = tail
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || matches!(*character, '+' | '/' | '=')
                })
                .count();
            if count >= 12 {
                return true;
            }
        }
        start = index + "basic ".len();
    }
    false
}

fn contains_jwt(lower: &str) -> bool {
    let mut start = 0;
    while let Some(relative) = lower[start..].find("eyj") {
        let index = start + relative;
        let rest = &lower[index..];
        let mut parts = rest.split('.');
        let header = parts.next().unwrap_or_default();
        let payload = parts.next().unwrap_or_default();
        let signature = parts.next().unwrap_or_default();
        if header.len() >= 20
            && payload.len() >= 10
            && signature.len() >= 10
            && [header, payload, signature].iter().all(|part| {
                part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '-' || character == '_'
                })
            })
        {
            return true;
        }
        start = index + 3;
    }
    false
}

fn contains_url_credentials(lower: &str) -> bool {
    for scheme in ["http://", "https://", "ssh://", "git://"] {
        let mut start = 0;
        while let Some(relative) = lower[start..].find(scheme) {
            let index = start + relative + scheme.len();
            let authority = lower[index..].split('/').next().unwrap_or_default();
            if let Some((userinfo, _host)) = authority.rsplit_once('@')
                && userinfo.contains(':')
                && !userinfo.starts_with('$')
            {
                return true;
            }
            start = index;
        }
    }
    false
}

fn contains_keyed_assignment(lower: &str) -> bool {
    for key in SECRET_KEYS {
        let mut start = 0;
        while let Some(relative) = lower[start..].find(key) {
            let index = start + relative;
            let after = lower[index + key.len()..].trim_start();
            let value = after
                .strip_prefix('=')
                .or_else(|| after.strip_prefix(':'))
                .map(|value| value.trim_start().trim_start_matches(['"', '\'']));
            if let Some(value) = value {
                let token: String = value
                    .chars()
                    .take_while(|character| {
                        !character.is_whitespace()
                            && !matches!(*character, ',' | ';' | '"' | '\'' | ')' | ']' | '}')
                    })
                    .collect();
                if token.len() >= 6 && !is_placeholder_value(&token) {
                    return true;
                }
            }
            start = index + key.len();
        }
    }
    false
}

fn is_placeholder_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.is_empty()
        || lower.starts_with('$')
        || lower.starts_with('<')
        || lower.starts_with('{')
        || lower.starts_with("env(")
        || lower.starts_with("env:")
        || lower.starts_with("os.environ")
        || lower.starts_with("std::env")
        || matches!(
            lower.as_str(),
            "redacted"
                | "changeme"
                | "change-me"
                | "example"
                | "placeholder"
                | "required"
                | "none"
                | "null"
                | "true"
                | "false"
                | "todo"
                | "tbd"
                | "xxx"
                | "***"
                | "..."
        )
        || lower
            .chars()
            .all(|character| character == '*' || character == 'x' || character == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_change() -> OpenSpecChange {
        OpenSpecChange {
            change_id: "add-validator".into(),
            title: "Add a bounded validator".into(),
            why: "Invalid input currently reaches the parser.".into(),
            what_changes: vec![
                "Add a deterministic validator at the boundary.".into(),
                "Return a typed error for invalid input.".into(),
            ],
            non_goals: vec!["Do not change the public API.".into()],
            requirements: vec![
                OpenSpecRequirement {
                    id: "REQ-001".into(),
                    statement: "The validator SHALL reject invalid input.".into(),
                    acceptance: vec!["REQ-001 valid input is accepted.".into()],
                    negative: vec!["REQ-001 invalid input returns a typed error.".into()],
                },
                OpenSpecRequirement {
                    id: "REQ-002".into(),
                    statement: "The validator SHALL preserve valid input.".into(),
                    acceptance: vec!["REQ-002 valid input is unchanged.".into()],
                    negative: vec!["REQ-002 valid input is never rewritten.".into()],
                },
            ],
            design: "## Dependencies\n\nNo new dependencies.\n\n## ADRs\n\nADR-0001 records the boundary.".into(),
            tasks: vec![
                OpenSpecTask {
                    id: "T1".into(),
                    title: "Implement rejection".into(),
                    paths: vec!["src/validator.rs".into()],
                    acceptance: vec!["REQ-001 invalid input returns a typed error.".into()],
                    evidence: vec!["cargo test -p factory-native --lib validator".into()],
                },
                OpenSpecTask {
                    id: "T2".into(),
                    title: "Implement preservation".into(),
                    paths: vec!["tests/validator.rs".into()],
                    acceptance: vec!["REQ-002 valid input is unchanged.".into()],
                    evidence: vec!["cargo test -p factory-native --lib validator".into()],
                },
            ],
            stack_summary: "Rust 2024 with std only.".into(),
            architecture_summary: "A pure module at the input boundary.".into(),
        }
    }

    fn refresh_manifest(artifacts: &mut OpenSpecArtifacts) {
        artifacts.manifest = build_manifest(
            &artifacts.change_id,
            artifacts.revision,
            &artifacts.proposal,
            &artifacts.specs,
            artifacts.design.as_deref(),
            &artifacts.tasks,
        );
    }

    #[test]
    fn rendering_is_deterministic_and_has_required_sections() {
        let change = sample_change();
        let first = render(&change).expect("valid change");
        let second = render(&change).expect("valid change");
        assert_eq!(first, second);
        assert_eq!(first.change_id, change.change_id);
        assert_eq!(first.revision, 1);
        assert_eq!(first.specs.len(), 1);
        assert_eq!(first.specs[0].path, "specs/add-validator/spec.md");

        assert!(first.proposal.starts_with("# Add a bounded validator"));
        for heading in ["## Why", "## What Changes", "## Non-Goals", "## Artifacts"] {
            assert!(first.proposal.contains(heading));
        }
        assert!(first.proposal.contains("](specs/add-validator/spec.md)"));
        assert!(first.proposal.contains("](design.md)"));
        assert!(first.proposal.contains("](tasks.md)"));

        assert!(first.specs[0].content.contains("## ADDED Requirements"));
        assert!(first.specs[0].content.contains("### Requirement: REQ-001"));
        assert!(
            first.specs[0]
                .content
                .contains("#### Scenario: REQ-001-ACC-01")
        );
        assert!(
            first.specs[0]
                .content
                .contains("#### Scenario: REQ-001-NEG-01")
        );

        let design = first.design.as_deref().expect("design exists");
        for heading in [
            "## Stack",
            "## Architecture",
            "## Design Notes",
            "## Dependencies",
            "## ADRs",
        ] {
            assert!(design.contains(heading));
        }

        assert!(first.tasks.contains("- [ ] 1.1"));
        assert!(first.tasks.contains("- [ ] 1.2"));
        assert!(first.tasks.contains("Paths: `src/validator.rs`"));
        assert!(first.tasks.contains("Acceptance:"));
        assert!(first.tasks.contains("Evidence:"));
        assert!(first.tasks.contains("Requirements: REQ-001"));
        assert!(first.tasks.contains("| REQ-001 | 1.1 |"));
        assert!(first.tasks.contains("| REQ-002 | 1.2 |"));
        assert!(validate_artifacts(&first).is_ok());
    }

    #[test]
    fn manifest_hashes_bind_each_artifact() {
        let artifacts = render(&sample_change()).expect("valid change");
        let expected_paths = [
            "proposal.md",
            "specs/add-validator/spec.md",
            "design.md",
            "tasks.md",
        ];
        assert_eq!(artifacts.manifest.artifacts.len(), expected_paths.len());
        for (entry, path) in artifacts.manifest.artifacts.iter().zip(expected_paths) {
            assert_eq!(entry.path, path);
            let content = artifacts.artifact(path).expect("artifact exists");
            assert_eq!(entry.sha256, sha256_hex(content.as_bytes()));
            assert_eq!(entry.sha256.len(), 64);
            assert!(
                entry
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            );
        }
        assert_eq!(artifacts.manifest.change_id, artifacts.change_id);
        assert_eq!(artifacts.manifest.revision, artifacts.revision);

        let mut tampered = artifacts.clone();
        tampered.proposal.push_str("\n<!-- tampered -->\n");
        assert!(validate_artifacts(&tampered).is_err());

        let mut missing = artifacts.clone();
        missing.manifest.artifacts.pop();
        assert!(validate_artifacts(&missing).is_err());
    }

    #[test]
    fn files_are_ordered_and_manifest_json_is_stable() {
        let artifacts = render(&sample_change()).expect("valid change");
        let paths: Vec<&str> = artifacts
            .files()
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            paths,
            vec![
                "proposal.md",
                "specs/add-validator/spec.md",
                "design.md",
                "tasks.md"
            ]
        );
        let first = artifacts.manifest_json();
        let second = artifacts.manifest_json();
        assert_eq!(first, second);
        let parsed: serde_json::Value = serde_json::from_str(&first).expect("manifest JSON");
        assert_eq!(parsed["changeId"], "add-validator");
        assert_eq!(parsed["revision"], 1);
        assert_eq!(parsed["artifacts"].as_array().map(Vec::len), Some(4));
    }

    #[test]
    fn projection_matches_the_planning_contract() {
        let artifacts = render(&sample_change()).expect("valid change");
        let projection = artifacts.projection(true).expect("valid projection");
        assert_eq!(projection.change_id, "add-validator");
        assert_eq!(projection.revision, 1);
        assert!(projection.validated);
        assert!(!projection.archived);
        assert_eq!(
            projection.proposal_sha256,
            artifacts
                .manifest
                .hash("proposal.md")
                .expect("proposal hash")
        );
        assert_eq!(
            projection.tasks_sha256,
            artifacts.manifest.hash("tasks.md").expect("tasks hash")
        );
        assert_eq!(
            projection.design_sha256,
            artifacts.manifest.hash("design.md").expect("design hash")
        );
        assert_eq!(projection.specs_sha256.len(), 64);
        assert!(projection.validate().is_ok());
    }

    #[test]
    fn read_model_is_deterministic_and_binds_projection_hashes() {
        let artifacts = render(&sample_change()).expect("valid change");
        let projection = artifacts.projection(true).expect("valid projection");
        let artifact_sha = sha256_hex(serde_json::to_string(&artifacts).unwrap().as_bytes());
        let first = read_model("request-1", &projection, &artifact_sha, &artifacts).unwrap();
        let second = read_model("request-1", &projection, &artifact_sha, &artifacts).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.request_id, "request-1");
        assert_eq!(first.change_id, "add-validator");
        assert_eq!(first.change_path, "openspec/changes/add-validator");
        assert_eq!(first.revision, 1);
        assert!(first.validated);
        assert!(!first.archived);
        assert_eq!(first.files.len(), artifacts.files().len());
        for file in &first.files {
            assert_eq!(
                artifacts.manifest.hash(&file.path),
                Some(file.sha256.as_str())
            );
            assert_eq!(file.content, artifacts.artifact(&file.path).unwrap());
        }
        assert_eq!(first.manifest.change_id, "add-validator");
        assert_eq!(
            first.hashes.proposal_sha256,
            artifacts.manifest.hash("proposal.md").unwrap()
        );
        assert_eq!(
            first.hashes.specs_sha256,
            artifacts.manifest.specs_sha256().unwrap()
        );
    }

    #[test]
    fn read_model_rejects_stale_or_mismatched_records() {
        let artifacts = render(&sample_change()).expect("valid change");
        let projection = artifacts.projection(true).expect("valid projection");
        let artifact_sha = sha256_hex(serde_json::to_string(&artifacts).unwrap().as_bytes());

        let mut stale = projection.clone();
        stale.revision += 1;
        assert!(read_model("request-1", &stale, &artifact_sha, &artifacts).is_err());

        let mut different = projection.clone();
        different.tasks_sha256 = "0".repeat(64);
        assert!(read_model("request-1", &different, &artifact_sha, &artifacts).is_err());

        let mut tampered = artifacts.clone();
        tampered.tasks.push_str("\n<!-- tampered -->\n");
        assert!(read_model("request-1", &projection, &artifact_sha, &tampered).is_err());

        assert!(read_model("request-1", &projection, "not-a-hash", &artifacts).is_err());
        assert!(read_model("request\n1", &projection, &artifact_sha, &artifacts).is_err());
        assert!(read_model(&"r".repeat(201), &projection, &artifact_sha, &artifacts).is_err());
    }

    #[test]
    fn stored_artifact_digest_is_recomputed_before_decoding() {
        let artifacts = render(&sample_change()).expect("valid change");
        let content_json = serde_json::to_string(&artifacts).unwrap();
        let digest = sha256_hex(content_json.as_bytes());
        let parsed = parse_stored_artifact(&content_json, &digest).expect("exact artifact");
        assert_eq!(parsed, artifacts);

        let mut changed = content_json.clone();
        changed.push(' ');
        let error = parse_stored_artifact(&changed, &digest).expect_err("changed JSON");
        assert!(error.to_string().contains("does not match its stored hash"));

        let error = parse_stored_artifact(&content_json, &"0".repeat(64)).expect_err("wrong hash");
        assert!(error.to_string().contains("does not match its stored hash"));

        let error = parse_stored_artifact(&content_json, "not-a-hash").expect_err("malformed hash");
        assert!(error.to_string().contains("SHA-256"));

        let mut tampered = artifacts.clone();
        tampered.tasks.push_str("\n<!-- drift -->");
        let tampered_json = serde_json::to_string(&tampered).unwrap();
        let tampered_digest = sha256_hex(tampered_json.as_bytes());
        let error =
            parse_stored_artifact(&tampered_json, &tampered_digest).expect_err("stale manifest");
        assert!(error.to_string().contains("manifest hash"));
    }

    #[test]
    fn archive_outcome_requires_a_validated_projection_and_uses_the_archive_path() {
        let artifacts = render(&sample_change()).expect("valid change");
        let projection = artifacts.projection(true).expect("valid projection");
        let artifact_sha = sha256_hex(serde_json::to_string(&artifacts).unwrap().as_bytes());
        let model = read_model("request-1", &projection, &artifact_sha, &artifacts).unwrap();
        let first = archive_outcome(&model, false).unwrap();
        let replay = archive_outcome(&model, true).unwrap();
        assert_eq!(first.archive_path, "openspec/changes/archive/add-validator");
        assert!(first.archived);
        assert!(!first.already_archived);
        assert!(replay.already_archived);
        assert_eq!(first.hashes, replay.hashes);

        let mut unvalidated = projection.clone();
        unvalidated.validated = false;
        let model = read_model("request-1", &unvalidated, &artifact_sha, &artifacts).unwrap();
        let error = archive_outcome(&model, false).expect_err("unvalidated archive");
        assert!(error.to_string().contains("validated"));
    }

    #[test]
    fn traceability_gap_is_rejected() {
        let mut change = sample_change();
        change.tasks[1].acceptance = vec!["Valid input is unchanged.".into()];
        change.tasks[1].evidence = vec!["cargo test -p factory-native --lib validator".into()];
        let artifacts = render(&change).expect("structurally valid change");
        let error = validate_artifacts(&artifacts).expect_err("unmapped requirement");
        assert!(
            error.to_string().contains("traceability")
                || error.to_string().contains("not covered")
                || error.to_string().contains("not traced")
        );
    }

    #[test]
    fn unknown_requirement_reference_is_rejected() {
        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.tasks = artifacts
            .tasks
            .replace("Requirements: REQ-002", "Requirements: REQ-999");
        refresh_manifest(&mut artifacts);
        let error = validate_artifacts(&artifacts).expect_err("unknown requirement");
        assert!(error.to_string().contains("unknown requirement"));
    }

    #[test]
    fn tasks_require_paths_acceptance_and_evidence() {
        let mut change = sample_change();
        change.tasks[0].acceptance.clear();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.tasks[0].evidence.clear();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.tasks[0].paths.clear();
        assert!(render(&change).is_err());

        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.tasks = artifacts.tasks.replace(
            "  - Acceptance:\n    - REQ-001 invalid input returns a typed error.\n",
            "",
        );
        refresh_manifest(&mut artifacts);
        assert!(validate_artifacts(&artifacts).is_err());
    }

    #[test]
    fn unsafe_task_paths_are_rejected() {
        for path in [
            "/etc/passwd",
            "../secret",
            "src/../secret",
            "C:/Windows",
            "~/secret",
            "src\\secret",
            ".git/config",
            "src//file.rs",
            "src/./file.rs",
            ".env",
            "deploy/server.pem",
        ] {
            let mut change = sample_change();
            change.tasks[0].paths = vec![path.into()];
            assert!(render(&change).is_err(), "path {path:?} must be rejected");
        }
    }

    #[test]
    fn duplicate_requirement_and_task_ids_are_rejected() {
        let mut change = sample_change();
        change.requirements[1].id = "REQ-001".into();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.tasks[1].id = "T1".into();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.requirements[0].id = "T1".into();
        assert!(render(&change).is_err());
    }

    #[test]
    fn artifact_validation_rejects_duplicate_requirement_ids() {
        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.specs[0].content.push_str(
            "\n### Requirement: REQ-001\n\nDuplicate.\n\n#### Scenario: REQ-001-ACC-01\n\n- **WHEN** duplicate\n- **THEN** duplicate\n\n#### Scenario: REQ-001-NEG-01\n\n- **WHEN** duplicate\n- **THEN** duplicate\n",
        );
        refresh_manifest(&mut artifacts);
        let error = validate_artifacts(&artifacts).expect_err("duplicate requirement");
        assert!(error.to_string().contains("duplicate requirement"));
    }

    #[test]
    fn required_headings_are_validated() {
        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.proposal = artifacts.proposal.replace("## Why", "## Motivation");
        refresh_manifest(&mut artifacts);
        assert!(validate_artifacts(&artifacts).is_err());

        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.tasks = artifacts.tasks.replace("## Traceability", "## Coverage");
        refresh_manifest(&mut artifacts);
        assert!(validate_artifacts(&artifacts).is_err());

        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.specs[0].content = format!("# Preamble\n\n{}", artifacts.specs[0].content);
        refresh_manifest(&mut artifacts);
        let error = validate_artifacts(&artifacts).expect_err("spec preamble is rejected");
        assert!(error.to_string().contains("ADDED Requirements"));
    }

    #[test]
    fn design_is_omitted_when_no_design_content_exists() {
        let mut change = sample_change();
        change.design.clear();
        change.stack_summary.clear();
        change.architecture_summary.clear();
        let artifacts = render(&change).expect("valid change");
        assert!(artifacts.design.is_none());
        assert!(!artifacts.proposal.contains("](design.md)"));
        assert_eq!(artifacts.manifest.artifacts.len(), 3);
        assert!(artifacts.manifest.hash("design.md").is_none());
        assert!(validate_artifacts(&artifacts).is_ok());
        assert!(artifacts.projection(true).is_ok());
    }

    #[test]
    fn empty_required_fields_are_rejected() {
        let mut change = sample_change();
        change.title.clear();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.why.clear();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.requirements.clear();
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.tasks.clear();
        assert!(render(&change).is_err());
    }

    #[test]
    fn oversized_fields_are_rejected() {
        let mut change = sample_change();
        change.title = "x".repeat(MAX_TITLE_CHARS + 1);
        assert!(render(&change).is_err());

        let mut change = sample_change();
        change.tasks[0].paths = (0..=MAX_TASK_PATHS)
            .map(|index| format!("src/file-{index}.rs"))
            .collect();
        assert!(render(&change).is_err());
    }

    #[test]
    fn secret_shaped_values_are_rejected_without_echoing_them() {
        let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let mut change = sample_change();
        change.design = format!("Never embed {secret} in a plan.");
        let error = render(&change).expect_err("secret-shaped design");
        assert!(!error.to_string().contains(secret));
        assert!(error.to_string().contains("secret"));

        let mut change = sample_change();
        change.tasks[0].acceptance = vec!["The check rejects password=hunter2.".into()];
        let error = render(&change).expect_err("secret-shaped acceptance");
        assert!(!error.to_string().contains("hunter2"));

        assert!(!contains_secret_shaped_value(
            "No secrets are stored. The task-runner processes risk-assessment data."
        ));
    }

    #[test]
    fn artifact_validation_rejects_injected_secrets() {
        let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let mut artifacts = render(&sample_change()).expect("valid change");
        artifacts.proposal.push_str(&format!("\n{secret}\n"));
        refresh_manifest(&mut artifacts);
        let error = validate_artifacts(&artifacts).expect_err("injected secret");
        assert!(!error.to_string().contains(secret));
    }

    #[test]
    fn change_ids_and_archive_paths_are_safe() {
        assert!(is_valid_change_id("add-validator"));
        assert!(is_valid_change_id("v2"));
        assert!(is_valid_change_id("a"));
        let long = "a".repeat(MAX_CHANGE_ID_CHARS + 1);
        assert!(!is_valid_change_id(&long));
        for invalid in [
            "",
            "Add-Validator",
            "add/validator",
            "../validator",
            "add_validator",
            "-add",
            "add-",
            "add--validator",
            "archive",
            "add.validator",
            "add validator",
            "ädd",
        ] {
            assert!(!is_valid_change_id(invalid), "{invalid:?}");
        }
        assert_eq!(
            archive_path("add-validator").expect("safe change id"),
            "openspec/changes/archive/add-validator"
        );
        assert_eq!(
            change_path("add-validator").expect("safe change id"),
            "openspec/changes/add-validator"
        );
        assert!(archive_path("../validator").is_err());
        assert!(change_path("Add-Validator").is_err());
    }

    #[test]
    fn render_revision_is_bound_and_rejects_zero() {
        let artifacts = render_revision(&sample_change(), 7).expect("valid revision");
        assert_eq!(artifacts.revision, 7);
        assert_eq!(artifacts.manifest.revision, 7);
        assert!(artifacts.proposal.contains("revision 7"));
        assert!(
            artifacts.specs[0]
                .content
                .starts_with("## ADDED Requirements\n")
        );
        assert!(!artifacts.specs[0].content.contains("revision 7"));
        assert!(render_revision(&sample_change(), 0).is_err());
    }
}
