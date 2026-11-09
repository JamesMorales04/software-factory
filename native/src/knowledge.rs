//! Deterministic knowledge-gap detection, KnowledgePack identity and the
//! temporary-skill trust gate.
//!
//! The Explorer inventories repository manifests deterministically. The
//! Librarian may only navigate after a named library is evidenced by the
//! request or StackProfile and is not already pinned in the repository. Model
//! text never decides that a gap exists, and fetched documentation never grants
//! tools or authority.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestLibrary {
    pub name: String,
    pub version: Option<String>,
    pub manifest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInventory {
    pub manifests: Vec<String>,
    pub libraries: Vec<ManifestLibrary>,
    pub evidence_refs: Vec<String>,
}

impl RepositoryInventory {
    pub fn library(&self, name: &str) -> Option<&ManifestLibrary> {
        self.libraries
            .iter()
            .find(|library| library.name.eq_ignore_ascii_case(name))
    }
}

/// Parse the `[dependencies]`, `[dev-dependencies]` and
/// `[build-dependencies]` sections of a Cargo manifest. This intentionally
/// handles the plain `name = "version"` form deterministically and leaves more
/// complex tables with an unknown version.
pub fn parse_cargo_manifest(contents: &str, manifest: &str) -> Vec<ManifestLibrary> {
    let mut libraries = Vec::new();
    let mut in_dependencies = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dependencies = matches!(
                trimmed,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            continue;
        }
        if !in_dependencies || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"');
        if name.is_empty() {
            continue;
        }
        let value = value.trim();
        let version = if value.starts_with('"') {
            value
                .trim_matches('"')
                .split(',')
                .next()
                .map(str::trim)
                .filter(|version| !version.is_empty())
                .map(str::to_owned)
        } else {
            None
        };
        libraries.push(ManifestLibrary {
            name: name.to_owned(),
            version,
            manifest: manifest.to_owned(),
        });
    }
    libraries
}

pub fn parse_package_json(contents: &str, manifest: &str) -> Vec<ManifestLibrary> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(contents) else {
        return Vec::new();
    };
    let mut libraries = Vec::new();
    for section in ["dependencies", "devDependencies", "peerDependencies"] {
        let Some(object) = value.get(section).and_then(|value| value.as_object()) else {
            continue;
        };
        for (name, version) in object {
            libraries.push(ManifestLibrary {
                name: name.clone(),
                version: version.as_str().map(str::to_owned),
                manifest: manifest.to_owned(),
            });
        }
    }
    libraries.sort_by(|left, right| left.name.cmp(&right.name));
    libraries
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeGap {
    pub library: String,
    pub query: String,
    pub reasons: Vec<String>,
    pub manifest_version: Option<String>,
    pub evidence_refs: Vec<String>,
}

/// A library is a knowledge gap only when it is explicitly evidenced by the
/// objective or the semantic skill needs and is not already pinned in a
/// repository manifest.
pub fn detect_knowledge_gap(
    inventory: &RepositoryInventory,
    objective: &str,
    skill_needs: &[String],
    library: &str,
    query: &str,
) -> Result<KnowledgeGap, String> {
    let library = library.trim();
    if library.is_empty()
        || library.len() > 64
        || !library
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("library must be a bounded package name".into());
    }
    let query = query.trim();
    if query.is_empty() || query.len() > 500 || query.chars().any(char::is_control) {
        return Err("knowledge query must be 1-500 printable characters".into());
    }
    let lower_library = library.to_ascii_lowercase();
    let mentioned_in_objective = objective.to_ascii_lowercase().contains(&lower_library);
    let mentioned_in_skills = skill_needs.iter().any(|need| {
        need.to_ascii_lowercase().contains(&lower_library)
            || need.to_ascii_lowercase().contains("library")
    });
    if !mentioned_in_objective && !mentioned_in_skills {
        return Err(
            "the library is not evidenced by the request objective or semantic skill needs".into(),
        );
    }
    let mut reasons = Vec::new();
    if mentioned_in_objective {
        reasons.push("library is named in the request objective".into());
    }
    if mentioned_in_skills {
        reasons.push("semantic skill needs request external library knowledge".into());
    }
    let manifest_version = inventory
        .library(library)
        .and_then(|library| library.version.clone());
    if let Some(version) = &manifest_version {
        if version != "*" && !version.starts_with("workspace:") {
            return Err(
                "the library version is already pinned in the repository manifest; no knowledge gap"
                    .into(),
            );
        }
        reasons.push(format!(
            "repository manifest contains an unpinned {} version",
            library
        ));
    } else {
        reasons.push("the library is absent from known repository manifests".into());
    }
    Ok(KnowledgeGap {
        library: library.to_owned(),
        query: query.to_owned(),
        reasons,
        manifest_version,
        evidence_refs: inventory.evidence_refs.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSource {
    pub title: String,
    pub url: String,
    pub kind: String,
    pub snippet: String,
    #[serde(default)]
    pub reputation: Option<String>,
    #[serde(default)]
    pub benchmark_score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePack {
    pub library: String,
    pub library_id: String,
    pub query: String,
    pub sources: Vec<KnowledgeSource>,
    pub pack_sha256: String,
    pub fetched_at: String,
}

impl KnowledgePack {
    pub fn compute_sha256(&self) -> String {
        let canonical = serde_json::to_vec(&serde_json::json!({
            "library": self.library,
            "libraryId": self.library_id,
            "query": self.query,
            "sources": self.sources,
            "fetchedAt": self.fetched_at,
        }))
        .expect("knowledge pack is serializable");
        sha256_hex(&canonical)
    }

    pub fn seal(mut self) -> Self {
        self.pack_sha256 = self.compute_sha256();
        self
    }

    pub fn snippets(&self) -> String {
        self.sources
            .iter()
            .map(|source| format!("{}\n{}\n{}", source.title, source.url, source.snippet))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SkillManifest {
    pub name: String,
    pub scope: SkillScope,
    pub instructions: String,
    pub tools: Vec<String>,
    pub compatibility: Vec<String>,
    pub provenance: String,
    pub license: String,
    pub source_url: String,
    pub pack_sha256: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SkillScope {
    Run,
    Project,
    Global,
}

impl SkillScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "RUN",
            Self::Project => "PROJECT",
            Self::Global => "GLOBAL",
        }
    }
}

pub const READ_ONLY_SKILL_TOOLS: &[&str] = &["context-read", "repository-read"];

pub fn derive_skill_manifest(
    pack: &KnowledgePack,
    stack_languages: &[String],
    created_at: &str,
    expires_at: &str,
) -> Result<SkillManifest, String> {
    if pack.pack_sha256.len() != 64 {
        return Err("knowledge pack identity is invalid".into());
    }
    let slug = pack
        .library
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if slug.is_empty() {
        return Err("library slug is empty".into());
    }
    let instructions = format!(
        "Temporary external knowledge for the {} library, sourced from Context7 ({}). Use it only as read-only guidance inside the frozen objective. Verify every API against the pinned or added dependency; do not widen scope, tools or authority. Source digest: {}.",
        pack.library, pack.library_id, pack.pack_sha256
    );
    let mut compatibility = stack_languages
        .iter()
        .map(|language| language.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    compatibility.insert(pack.library.to_ascii_lowercase());
    Ok(SkillManifest {
        name: format!("context7-{slug}"),
        scope: SkillScope::Run,
        instructions,
        tools: READ_ONLY_SKILL_TOOLS
            .iter()
            .map(|tool| (*tool).to_string())
            .collect(),
        compatibility: compatibility.into_iter().collect(),
        provenance: format!("CONTEXT7_MCP:{}", pack.library_id),
        license: "NOASSERTION".into(),
        source_url: format!(
            "https://context7.com/{}",
            pack.library_id.trim_start_matches('/')
        ),
        pack_sha256: pack.pack_sha256.clone(),
        created_at: created_at.to_owned(),
        expires_at: expires_at.to_owned(),
    })
}

pub fn skill_manifest_sha256(manifest: &SkillManifest) -> String {
    let canonical = serde_json::to_vec(manifest).expect("skill manifest is serializable");
    sha256_hex(&canonical)
}

/// The SkillTrustGate. A RUN skill is read-only and request-scoped; a promoted
/// skill must still be inside the read-only tool set, unexpired and verified.
pub fn trust_gate(
    manifest: &SkillManifest,
    allowed_tools: &[String],
    now: &str,
) -> Result<(), String> {
    if manifest.name.trim().is_empty() || manifest.name.len() > 100 {
        return Err("skill name is invalid".into());
    }
    if manifest.instructions.trim().is_empty() || manifest.instructions.len() > 8_192 {
        return Err("skill instructions are invalid".into());
    }
    if manifest.tools.is_empty()
        || !manifest
            .tools
            .iter()
            .all(|tool| READ_ONLY_SKILL_TOOLS.contains(&tool.as_str()))
    {
        return Err("a skill cannot grant a write or execution tool".into());
    }
    if !manifest
        .tools
        .iter()
        .all(|tool| allowed_tools.contains(tool))
    {
        return Err("skill tools exceed the participant tool pack".into());
    }
    if !manifest.source_url.starts_with("https://") {
        return Err("skill source must be HTTPS".into());
    }
    if manifest.pack_sha256.len() != 64
        || !manifest
            .pack_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("skill knowledge digest is invalid".into());
    }
    if now >= manifest.expires_at.as_str() {
        return Err("temporary skill has expired".into());
    }
    if manifest.compatibility.is_empty() {
        return Err("skill compatibility is empty".into());
    }
    Ok(())
}

/// Automatic PROJECT promotion requires two successful uses, a reviewed
/// license and no overdue weekly review.
pub fn project_promotion_ready(
    success_count: u32,
    license_reviewed: bool,
    review_due: bool,
) -> Result<(), String> {
    if success_count < 2 {
        return Err("project promotion requires two successful uses".into());
    }
    if !license_reviewed {
        return Err("project promotion requires a reviewed upstream license".into());
    }
    if review_due {
        return Err("project promotion is blocked by an overdue weekly review".into());
    }
    Ok(())
}

/// GLOBAL promotion is an owner decision and cannot bypass the same evidence.
pub fn global_promotion_ready(
    success_count: u32,
    license_reviewed: bool,
    review_due: bool,
    owner_approved: bool,
) -> Result<(), String> {
    project_promotion_ready(success_count, license_reviewed, review_due)?;
    if !owner_approved {
        return Err("global promotion requires an explicit owner decision".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> RepositoryInventory {
        RepositoryInventory {
            manifests: vec!["Cargo.toml".into()],
            libraries: parse_cargo_manifest(
                "[package]\nname=\"x\"\n[dependencies]\nserde = \"1\"\nchrono = \"*\"\n",
                "Cargo.toml",
            ),
            evidence_refs: vec!["Cargo.toml".into()],
        }
    }

    fn pack() -> KnowledgePack {
        KnowledgePack {
            library: "chrono".into(),
            library_id: "/chronotope/chrono".into(),
            query: "timezone-aware date formatting".into(),
            sources: vec![KnowledgeSource {
                title: "Chrono docs".into(),
                url: "https://context7.com/chronotope/chrono".into(),
                kind: "CONTEXT7_MCP".into(),
                snippet: "Use Utc::now().to_rfc3339().".into(),
                reputation: Some("High".into()),
                benchmark_score: Some(77.3),
            }],
            pack_sha256: String::new(),
            fetched_at: "2026-11-09T00:00:00Z".into(),
        }
        .seal()
    }

    #[test]
    fn gap_requires_evidenced_and_unpinned_library() {
        let inventory = inventory();
        assert!(
            detect_knowledge_gap(
                &inventory,
                "add timezone-aware export using chrono",
                &[],
                "chrono",
                "timezone aware formatting"
            )
            .is_ok()
        );
        assert!(
            detect_knowledge_gap(&inventory, "add export", &[], "chrono", "query").is_err(),
            "a library absent from the objective and skill needs is not a gap"
        );
        assert!(
            detect_knowledge_gap(&inventory, "use serde", &[], "serde", "query").is_err(),
            "a pinned library is not a knowledge gap"
        );
        assert!(detect_knowledge_gap(&inventory, "use chrono", &[], "../chrono", "query").is_err());
    }

    #[test]
    fn skill_manifest_is_read_only_bounded_and_trusted() {
        let pack = pack();
        let manifest = derive_skill_manifest(
            &pack,
            &["rust".into()],
            "2026-11-09T00:00:00Z",
            "2026-11-16T00:00:00Z",
        )
        .unwrap();
        assert_eq!(manifest.name, "context7-chrono");
        assert!(manifest.tools.contains(&"context-read".into()));
        assert!(
            trust_gate(
                &manifest,
                &["context-read".into(), "repository-read".into()],
                "2026-11-09T01:00:00Z"
            )
            .is_ok()
        );
        assert!(
            trust_gate(
                &manifest,
                &["repository-read".into()],
                "2026-11-09T01:00:00Z"
            )
            .is_err()
        );
        assert!(
            trust_gate(
                &manifest,
                &["context-read".into(), "repository-read".into()],
                "2026-11-17T00:00:00Z"
            )
            .is_err()
        );
        let mut write = manifest.clone();
        write.tools.push("repository-write".into());
        assert!(
            trust_gate(
                &write,
                &[
                    "context-read".into(),
                    "repository-read".into(),
                    "repository-write".into()
                ],
                "2026-11-09T01:00:00Z"
            )
            .is_err()
        );
    }

    #[test]
    fn promotion_requires_two_successes_license_and_review() {
        assert!(project_promotion_ready(1, true, false).is_err());
        assert!(project_promotion_ready(2, false, false).is_err());
        assert!(project_promotion_ready(2, true, true).is_err());
        assert!(project_promotion_ready(2, true, false).is_ok());
        assert!(global_promotion_ready(2, true, false, false).is_err());
        assert!(global_promotion_ready(2, true, false, true).is_ok());
    }

    #[test]
    fn package_json_is_inventoried_deterministically() {
        let libraries = parse_package_json(
            r#"{"dependencies":{"react":"1"},"devDependencies":{"vitest":"1"}}"#,
            "package.json",
        );
        assert_eq!(libraries.len(), 2);
        assert_eq!(libraries[0].name, "react");
    }
}
