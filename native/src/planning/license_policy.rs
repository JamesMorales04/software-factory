//! Declarative licence catalog and context policy shared with the builder.
//!
//! `license_catalog.json` classifies known identifiers (family, state,
//! obligations, provenance, aliases). `license_profile.json` is the versioned
//! context policy. Classification is not acceptance; the profile decides.
//! The transitional `legacy-current` profile reproduces the pre-existing
//! selected/closure verdicts. The evaluator is deterministic and never consumes
//! model output.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const CATALOG_JSON: &str =
    include_str!("../../../deploy/environment-builder/environment_builder/license_catalog.json");
const PROFILE_JSON: &str =
    include_str!("../../../deploy/environment-builder/environment_builder/license_profile.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CatalogDocument {
    schema: u32,
    revision: String,
    spdx_license_list_version: String,
    source_url: String,
    sha256: String,
    entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub family: String,
    pub state: String,
    pub spdx: bool,
    #[serde(default)]
    pub obligations: Vec<String>,
    #[serde(default)]
    pub provenance: Vec<String>,
    pub source_url: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LicenseCatalog {
    pub revision: String,
    pub spdx_license_list_version: String,
    pub source_url: String,
    pub sha256: String,
    pub entries: BTreeMap<String, CatalogEntry>,
    pub alias_index: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ProfileDocument {
    schema: u32,
    revision: String,
    mode: String,
    families_configured: bool,
    signed_by: String,
    signed_at: String,
    sha256: String,
    denied_ids: Vec<String>,
    contexts: BTreeMap<String, ContextPolicy>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ContextPolicy {
    #[serde(default)]
    pub allowed_ids: Vec<String>,
    #[serde(default)]
    pub accepted_families: Vec<String>,
    #[serde(default)]
    pub obligations: Vec<String>,
    #[serde(default)]
    pub inherits: Option<String>,
    #[serde(default)]
    pub evaluated: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct LicenseProfile {
    pub revision: String,
    pub mode: String,
    pub families_configured: bool,
    pub signed_by: String,
    pub signed_at: String,
    pub sha256: String,
    pub denied_ids: BTreeSet<String>,
    pub contexts: BTreeMap<String, ContextPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseContext {
    Selected,
    Closure,
    Build,
    DevTest,
    RuntimeDistributed,
}

impl LicenseContext {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Closure => "closure",
            Self::Build => "build",
            Self::DevTest => "devTest",
            Self::RuntimeDistributed => "runtimeDistributed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseDecision {
    Allowed,
    Denied,
    Unknown,
    NotEvaluated,
}

impl LicenseDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Unknown => "unknown",
            Self::NotEvaluated => "notEvaluated",
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical(value: &serde_json::Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string(value).expect("canonical JSON is serializable")
    )
}

fn is_known_family(value: &str) -> bool {
    matches!(
        value,
        "permissive"
            | "weak-copyleft"
            | "strong-copyleft"
            | "network-copyleft"
            | "source-available"
            | "public-domain"
    )
}

impl LicenseCatalog {
    pub fn from_json(raw: &str) -> Result<Self, String> {
        let document: CatalogDocument = serde_json::from_str(raw)
            .map_err(|error| format!("invalid licence catalog JSON: {error}"))?;
        if document.schema != 1 {
            return Err("unsupported licence catalog schema".into());
        }
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| format!("invalid licence catalog JSON: {error}"))?;
        let entries_value = value
            .get("entries")
            .cloned()
            .ok_or_else(|| "licence catalog entries are missing".to_owned())?;
        if sha256_hex(canonical(&entries_value).as_bytes()) != document.sha256 {
            return Err("licence catalog hash is inconsistent".into());
        }
        let mut entries = BTreeMap::new();
        let mut alias_index = BTreeMap::new();
        for entry in document.entries {
            if entry.id.is_empty() || entries.contains_key(&entry.id) {
                return Err("licence catalog ids are invalid or duplicated".into());
            }
            if !is_known_family(&entry.family) {
                return Err(format!(
                    "licence catalog family is invalid for {}",
                    entry.id
                ));
            }
            for name in std::iter::once(&entry.id).chain(entry.aliases.iter()) {
                if alias_index.insert(name.clone(), entry.id.clone()).is_some() {
                    return Err(format!("licence catalog alias {name:?} is duplicated"));
                }
            }
            entries.insert(entry.id.clone(), entry);
        }
        Ok(Self {
            revision: document.revision,
            spdx_license_list_version: document.spdx_license_list_version,
            source_url: document.source_url,
            sha256: document.sha256,
            entries,
            alias_index,
        })
    }

    pub fn contains(&self, identifier: &str) -> bool {
        self.entries.contains_key(identifier)
    }
}

impl LicenseProfile {
    pub fn from_json(raw: &str) -> Result<Self, String> {
        let document: ProfileDocument = serde_json::from_str(raw)
            .map_err(|error| format!("invalid licence profile JSON: {error}"))?;
        if document.schema != 1 {
            return Err("unsupported licence profile schema".into());
        }
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| format!("invalid licence profile JSON: {error}"))?;
        let denied = value
            .get("deniedIds")
            .cloned()
            .ok_or_else(|| "licence profile deniedIds are missing".to_owned())?;
        let contexts = value
            .get("contexts")
            .cloned()
            .ok_or_else(|| "licence profile contexts are missing".to_owned())?;
        let payload = serde_json::json!({"deniedIds": denied, "contexts": contexts});
        if sha256_hex(canonical(&payload).as_bytes()) != document.sha256 {
            return Err("licence profile hash is inconsistent".into());
        }
        Ok(Self {
            revision: document.revision,
            mode: document.mode,
            families_configured: document.families_configured,
            signed_by: document.signed_by,
            signed_at: document.signed_at,
            sha256: document.sha256,
            denied_ids: document.denied_ids.into_iter().collect(),
            contexts: document.contexts,
        })
    }

    pub fn b_active(&self) -> bool {
        self.mode != "legacy-current" && self.families_configured
    }
}

pub fn default_catalog() -> &'static LicenseCatalog {
    static CATALOG: OnceLock<LicenseCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        LicenseCatalog::from_json(CATALOG_JSON)
            .expect("the shared licence catalog is valid and hash-bound")
    })
}

pub fn default_profile() -> &'static LicenseProfile {
    static PROFILE: OnceLock<LicenseProfile> = OnceLock::new();
    PROFILE.get_or_init(|| {
        LicenseProfile::from_json(PROFILE_JSON)
            .expect("the shared licence profile is valid and hash-bound")
    })
}

pub fn default_catalog_sha256() -> &'static str {
    &default_catalog().sha256
}

pub fn default_profile_sha256() -> &'static str {
    &default_profile().sha256
}

pub fn binding_sha256(catalog: &LicenseCatalog, profile: &LicenseProfile) -> String {
    let payload = serde_json::json!({
        "catalogSha256": catalog.sha256,
        "profileSha256": profile.sha256,
    });
    sha256_hex(canonical(&payload).as_bytes())
}

pub fn default_binding_sha256() -> String {
    binding_sha256(default_catalog(), default_profile())
}

fn resolve_context(
    profile: &LicenseProfile,
    context: LicenseContext,
) -> Result<&ContextPolicy, String> {
    let mut current = context.as_str();
    for _ in 0..8 {
        let policy = profile
            .contexts
            .get(current)
            .ok_or_else(|| format!("licence profile has no context {current:?}"))?;
        if let Some(parent) = &policy.inherits {
            current = parent.as_str();
            continue;
        }
        return Ok(policy);
    }
    Err("licence profile context inheritance cycle".into())
}

fn normalized_atoms(value: &str) -> Vec<String> {
    value
        .replace(['(', ')'], " ")
        .replace(" AND ", " ")
        .replace(" OR ", " ")
        .replace(" WITH ", " ")
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Deterministic verdict for one context. No model input is accepted.
pub fn evaluate(
    value: &str,
    context: LicenseContext,
    catalog: &LicenseCatalog,
    profile: &LicenseProfile,
) -> LicenseDecision {
    let value = value.trim();
    if value.is_empty() || matches!(value, "NONE" | "NOASSERTION" | "UNLICENSED" | "UNKNOWN") {
        return LicenseDecision::Unknown;
    }
    let policy = match resolve_context(profile, context) {
        Ok(policy) => policy,
        Err(_) => return LicenseDecision::Unknown,
    };
    if policy.evaluated == Some(false) {
        return LicenseDecision::NotEvaluated;
    }
    let canonical_value = catalog
        .alias_index
        .get(value)
        .map(String::as_str)
        .unwrap_or(value);
    if profile.denied_ids.contains(canonical_value) {
        return LicenseDecision::Denied;
    }
    if policy
        .allowed_ids
        .iter()
        .any(|id| id.as_str() == canonical_value)
    {
        return LicenseDecision::Allowed;
    }
    let atoms = normalized_atoms(value);
    if atoms.is_empty() {
        return LicenseDecision::Unknown;
    }
    for atom in atoms {
        if atom.starts_with("LLVM-exception") {
            continue;
        }
        let identifier = catalog
            .alias_index
            .get(&atom)
            .map(String::as_str)
            .unwrap_or(atom.as_str());
        if policy
            .allowed_ids
            .iter()
            .any(|id| id.as_str() == identifier)
        {
            continue;
        }
        let Some(entry) = catalog.entries.get(identifier) else {
            return LicenseDecision::Unknown;
        };
        if entry.state != "active" {
            return LicenseDecision::Unknown;
        }
        if policy
            .accepted_families
            .iter()
            .any(|family| family == &entry.family)
        {
            continue;
        }
        return LicenseDecision::Unknown;
    }
    LicenseDecision::Allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOWED_IDS: &[&str] = &[
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
    const CLOSURE_ONLY_IDS: &[&str] = &[
        "BSD-4-Clause",
        "BSD-4-Clause-UC",
        "GPL-2.0-only",
        "GPL-2.0-or-later",
        "GPL-3.0-only",
        "GPL-3.0-or-later",
        "LGPL-2.0-only",
        "LGPL-2.0-or-later",
        "NCSA",
        "OpenSSL",
        "PostgreSQL",
        "Python-2.0",
        "curl",
        "publicDomain",
    ];
    const DENIED_IDS: &[&str] = &[
        "AGPL-3.0-only",
        "AGPL-3.0-or-later",
        "BUSL-1.1",
        "Commons-Clause",
        "SSPL-1.0",
    ];
    const UTIL_LINUX: &str = "GPL-2.0-only AND GPL-2.0-or-later AND GPL-3.0-or-later \
        AND LGPL-2.1-or-later AND BSD-3-Clause AND BSD-4-Clause-UC AND publicDomain";

    #[test]
    fn shared_catalog_and_profile_are_hash_bound() {
        let catalog = default_catalog();
        let profile = default_profile();
        assert_eq!(catalog.entries.len(), 35);
        assert_eq!(
            catalog.sha256,
            "725ba589f774e2f1415534e2ee37157ae9fa1ecacfc956a462529d167297cd36"
        );
        assert_eq!(
            profile.sha256,
            "da0449205259166f6b880fba083d70d275b0fa1862efb5c04c5a4e74f253a15f"
        );
        assert_eq!(
            default_binding_sha256(),
            "4905b68839cd0ed5420a25a0115664aba2342836420e04089ad4761a558f2b7f"
        );
        assert_eq!(profile.mode, "legacy-current");
        assert!(!profile.b_active());
        assert_eq!(
            catalog.alias_index.get("Public Domain"),
            Some(&"publicDomain".to_owned())
        );
    }

    #[test]
    fn missing_context_and_personal_profile_fail_closed_or_activate() {
        let mut profile = default_profile().clone();
        profile.contexts.remove("closure");
        assert_eq!(
            evaluate("MIT", LicenseContext::Closure, default_catalog(), &profile),
            LicenseDecision::Unknown
        );
        profile.mode = "personal".into();
        profile.families_configured = true;
        assert!(profile.b_active());
    }

    #[test]
    fn legacy_matrix_preserves_selected_closure_and_unmodelled_contexts() {
        let catalog = default_catalog();
        let profile = default_profile();
        for identifier in ALLOWED_IDS {
            assert_eq!(
                evaluate(identifier, LicenseContext::Selected, catalog, profile),
                LicenseDecision::Allowed
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::Closure, catalog, profile),
                LicenseDecision::Allowed
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::Build, catalog, profile),
                LicenseDecision::Allowed
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::DevTest, catalog, profile),
                LicenseDecision::Allowed
            );
        }
        for identifier in CLOSURE_ONLY_IDS {
            assert_eq!(
                evaluate(identifier, LicenseContext::Selected, catalog, profile),
                LicenseDecision::Unknown
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::Closure, catalog, profile),
                LicenseDecision::Allowed
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::Build, catalog, profile),
                LicenseDecision::Allowed
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::DevTest, catalog, profile),
                LicenseDecision::Allowed
            );
        }
        for identifier in DENIED_IDS {
            for context in [
                LicenseContext::Selected,
                LicenseContext::Closure,
                LicenseContext::Build,
                LicenseContext::DevTest,
            ] {
                assert_eq!(
                    evaluate(identifier, context, catalog, profile),
                    LicenseDecision::Denied
                );
            }
        }
        for identifier in catalog.entries.keys() {
            assert_eq!(
                evaluate(
                    identifier,
                    LicenseContext::RuntimeDistributed,
                    catalog,
                    profile
                ),
                LicenseDecision::NotEvaluated
            );
        }
        assert_eq!(
            evaluate(UTIL_LINUX, LicenseContext::Closure, catalog, profile),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(UTIL_LINUX, LicenseContext::Selected, catalog, profile),
            LicenseDecision::Unknown
        );
        assert_eq!(
            evaluate("Python-2.0", LicenseContext::Closure, catalog, profile),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate("Python-2.0", LicenseContext::Selected, catalog, profile),
            LicenseDecision::Unknown
        );
        assert_eq!(
            evaluate("Public Domain", LicenseContext::Closure, catalog, profile),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate("Public Domain", LicenseContext::Selected, catalog, profile),
            LicenseDecision::Unknown
        );
    }

    #[test]
    fn real_33_entry_inventory_passes_the_closure_context() {
        #[derive(Deserialize)]
        struct Fixture {
            count: usize,
            entries: Vec<FixtureEntry>,
        }
        #[derive(Deserialize)]
        struct FixtureEntry {
            license: String,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../deploy/environment-builder/tests/fixtures/closure-license-inventory-33.json"
        ))
        .unwrap();
        assert_eq!(fixture.count, 33);
        for entry in &fixture.entries {
            assert_eq!(
                evaluate(
                    &entry.license,
                    LicenseContext::Closure,
                    default_catalog(),
                    default_profile()
                ),
                LicenseDecision::Allowed,
                "{}",
                entry.license
            );
        }
    }

    #[test]
    fn cross_language_matrix_fixture_matches_the_shared_data() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Fixture {
            catalog_sha256: String,
            profile_sha256: String,
            contexts: BTreeMap<String, BTreeMap<String, String>>,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../deploy/environment-builder/tests/fixtures/license-context-matrix.json"
        ))
        .unwrap();
        assert_eq!(fixture.catalog_sha256, default_catalog().sha256);
        assert_eq!(fixture.profile_sha256, default_profile().sha256);
        for (context_name, row) in &fixture.contexts {
            let context = match context_name.as_str() {
                "selected" => LicenseContext::Selected,
                "closure" => LicenseContext::Closure,
                "build" => LicenseContext::Build,
                "devTest" => LicenseContext::DevTest,
                "runtimeDistributed" => LicenseContext::RuntimeDistributed,
                other => panic!("unknown context {other}"),
            };
            for (identifier, expected) in row {
                assert_eq!(
                    evaluate(identifier, context, default_catalog(), default_profile()).as_str(),
                    expected,
                    "{context_name}/{identifier}"
                );
            }
        }
    }

    #[test]
    fn malformed_catalog_schema_fails_closed() {
        for malformed in ["true", "false", "1.0", "\"1\"", "null", "-1", "0"] {
            let raw =
                CATALOG_JSON.replacen("\"schema\": 1", &format!("\"schema\": {malformed}"), 1);
            assert!(LicenseCatalog::from_json(&raw).is_err(), "{malformed}");
        }
        LicenseCatalog::from_json(CATALOG_JSON).unwrap();
    }
}
