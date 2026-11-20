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
        if document.mode != "legacy-current" && document.mode != "personal" {
            return Err("invalid licence profile mode".into());
        }
        for (name, config) in &document.contexts {
            if !matches!(
                name.as_str(),
                "selected" | "closure" | "build" | "devTest" | "runtimeDistributed"
            ) {
                return Err(format!("invalid licence profile context {name:?}"));
            }
            for family in &config.accepted_families {
                if !is_known_family(family) {
                    return Err(format!("unknown licence family {family:?}"));
                }
            }
            if let Some(parent) = &config.inherits
                && !matches!(
                    parent.as_str(),
                    "selected" | "closure" | "build" | "devTest" | "runtimeDistributed"
                )
            {
                return Err(format!("invalid licence profile inheritance in {name:?}"));
            }
        }
        let payload = serde_json::json!({
            "mode": document.mode.clone(),
            "familiesConfigured": document.families_configured,
            "deniedIds": denied,
            "contexts": contexts,
        });
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Atom(String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    With(Box<Expr>, String),
}

fn tokenize(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        match character {
            '(' | ')' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(character.to_string());
            }
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

struct ExpressionParser {
    tokens: Vec<String>,
    position: usize,
}

impl ExpressionParser {
    fn new(value: &str) -> Self {
        Self {
            tokens: tokenize(value),
            position: 0,
        }
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.position).map(String::as_str)
    }

    fn take(&mut self) -> Option<String> {
        let token = self.tokens.get(self.position).cloned();
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn parse(&mut self) -> Result<Expr, String> {
        let expression = self.parse_or()?;
        if self.position != self.tokens.len() {
            return Err("trailing licence expression tokens".into());
        }
        Ok(expression)
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut branches = vec![self.parse_and()?];
        while self.peek() == Some("OR") {
            self.position += 1;
            branches.push(self.parse_and()?);
        }
        Ok(if branches.len() == 1 {
            branches.remove(0)
        } else {
            Expr::Or(branches)
        })
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut factors = vec![self.parse_atom()?];
        loop {
            match self.peek() {
                Some("AND") => {
                    self.position += 1;
                    factors.push(self.parse_atom()?);
                }
                Some("OR") | Some(")") | None => break,
                // Adjacent SPDX atoms (and a WITH exception) are implicitly AND.
                Some(_) => factors.push(self.parse_atom()?),
            }
        }
        Ok(if factors.len() == 1 {
            factors.remove(0)
        } else {
            Expr::And(factors)
        })
    }

    fn parse_atom(&mut self) -> Result<Expr, String> {
        let base = match self.take().as_deref() {
            Some("(") => {
                let expression = self.parse_or()?;
                if self.take().as_deref() != Some(")") {
                    return Err("unbalanced licence expression parentheses".into());
                }
                expression
            }
            Some(")") | Some("AND") | Some("OR") | Some("WITH") | None => {
                return Err("invalid licence expression atom".into());
            }
            Some(atom) => Expr::Atom(atom.to_string()),
        };
        if self.peek() != Some("WITH") {
            return Ok(base);
        }
        self.position += 1;
        // SPDX WITH is a postfix on one licence id: base WITH exception.
        if !matches!(base, Expr::Atom(_)) {
            return Err("WITH must bind to a single licence id".into());
        }
        let exception = match self.take().as_deref() {
            Some(exception)
                if !matches!(exception, "(" | ")" | "AND" | "OR" | "WITH")
                    && !exception.starts_with('-') =>
            {
                exception.to_string()
            }
            _ => return Err("WITH requires an exception identifier".into()),
        };
        if self.peek() == Some("WITH") {
            return Err("nested WITH is invalid".into());
        }
        Ok(Expr::With(Box::new(base), exception))
    }
}

fn atom_decision(
    atom: &str,
    catalog: &LicenseCatalog,
    profile: &LicenseProfile,
    policy: &ContextPolicy,
) -> LicenseDecision {
    if atom == "LLVM-exception" || atom.ends_with("-exception") || atom.ends_with("-exception-2.0")
    {
        // An exception is never a standalone licence and is never silently
        // ignored; only the postfix WITH form can consume a recognised one.
        return LicenseDecision::Unknown;
    }
    let identifier = catalog
        .alias_index
        .get(atom)
        .map(String::as_str)
        .unwrap_or(atom);
    if profile.denied_ids.contains(identifier) {
        return LicenseDecision::Denied;
    }
    if policy
        .allowed_ids
        .iter()
        .any(|id| id.as_str() == identifier)
    {
        return LicenseDecision::Allowed;
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
        return LicenseDecision::Allowed;
    }
    LicenseDecision::Unknown
}

fn evaluate_expression(
    expression: &Expr,
    catalog: &LicenseCatalog,
    profile: &LicenseProfile,
    policy: &ContextPolicy,
) -> LicenseDecision {
    match expression {
        Expr::Atom(atom) => atom_decision(atom, catalog, profile, policy),
        Expr::And(factors) => {
            let mut unknown = false;
            let mut not_evaluated = false;
            for factor in factors {
                match evaluate_expression(factor, catalog, profile, policy) {
                    LicenseDecision::Denied => return LicenseDecision::Denied,
                    LicenseDecision::Unknown => unknown = true,
                    LicenseDecision::NotEvaluated => not_evaluated = true,
                    LicenseDecision::Allowed => {}
                }
            }
            if unknown {
                LicenseDecision::Unknown
            } else if not_evaluated {
                LicenseDecision::NotEvaluated
            } else {
                LicenseDecision::Allowed
            }
        }
        Expr::Or(branches) => {
            let mut denied = false;
            let mut unknown = false;
            let mut not_evaluated = false;
            for branch in branches {
                match evaluate_expression(branch, catalog, profile, policy) {
                    LicenseDecision::Allowed => return LicenseDecision::Allowed,
                    LicenseDecision::Denied => denied = true,
                    LicenseDecision::Unknown => unknown = true,
                    LicenseDecision::NotEvaluated => not_evaluated = true,
                }
            }
            // Unknown (including an exception-only branch) is never an allow;
            // a denied branch is reported only when no branch is unknown.
            if unknown {
                LicenseDecision::Unknown
            } else if denied {
                LicenseDecision::Denied
            } else if not_evaluated {
                LicenseDecision::NotEvaluated
            } else {
                LicenseDecision::Unknown
            }
        }
        Expr::With(base, exception) => {
            if !matches!(exception.as_str(), "LLVM-exception") {
                return LicenseDecision::Unknown;
            }
            match base.as_ref() {
                Expr::Atom(atom) if !atom.ends_with("-exception") => {
                    atom_decision(atom, catalog, profile, policy)
                }
                _ => LicenseDecision::Unknown,
            }
        }
    }
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
    if let Some(identifier) = catalog.alias_index.get(value) {
        return atom_decision(identifier, catalog, profile, policy);
    }
    let expression = match ExpressionParser::new(value).parse() {
        Ok(expression) => expression,
        Err(_) => return LicenseDecision::Unknown,
    };
    evaluate_expression(&expression, catalog, profile, policy)
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
        assert_eq!(catalog.entries.len(), 36);
        assert_eq!(
            catalog.sha256,
            "edef68e16ad7c465eb8a263d5a74e23c361b8aa7a7eb9186197699265c1e9a49"
        );
        assert_eq!(
            profile.sha256,
            "233c0a4498f6b0b04dac494d596b160766b80f64d3ad0ff020e8e4796ac22aac"
        );
        assert_eq!(
            default_binding_sha256(),
            "157242c0f25a05412364dab23449c674418d1e21f6568c454521d187fa225afd"
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
    fn or_expression_resolves_a_valid_branch_without_implicit_and() {
        let catalog = default_catalog();
        let profile = default_profile();
        assert_eq!(
            evaluate("MIT OR BUSL-1.1", LicenseContext::Closure, catalog, profile),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(
                "AGPL-3.0-only OR MIT",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(
                "AGPL-3.0-only AND MIT",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Denied
        );
        assert_eq!(
            evaluate(
                "AGPL-3.0-only OR BUSL-1.1",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Denied
        );
        assert_eq!(
            evaluate(
                "(MIT OR Apache-2.0) AND BSD-3-Clause",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(
                "MIT AND (Apache-2.0 OR BUSL-1.1)",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(
                "MIT OR LicenseRef-Unknown",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(
                "MIT AND LicenseRef-Unknown",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Unknown
        );
        assert_eq!(
            evaluate(
                "GFDL-1.3-or-later",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Unknown
        );
        assert_eq!(
            evaluate(
                "MIT WITH LLVM-exception",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Allowed
        );
        assert_eq!(
            evaluate(
                "MIT WITH Classpath-exception-2.0",
                LicenseContext::Closure,
                catalog,
                profile
            ),
            LicenseDecision::Unknown
        );
        assert_eq!(
            evaluate("MIT WITH", LicenseContext::Closure, catalog, profile),
            LicenseDecision::Unknown
        );
    }

    #[test]
    fn with_exception_is_postfix_and_exception_only_is_never_a_license() {
        let catalog = default_catalog();
        let profile = default_profile();
        for (expression, expected) in [
            ("MIT WITH LLVM-exception", LicenseDecision::Allowed),
            ("LLVM-exception", LicenseDecision::Unknown),
            ("BUSL-1.1 OR LLVM-exception", LicenseDecision::Unknown),
            ("AGPL-3.0-only OR LLVM-exception", LicenseDecision::Unknown),
            ("BUSL-1.1 WITH LLVM-exception", LicenseDecision::Denied),
            ("MIT WITH Classpath-exception-2.0", LicenseDecision::Unknown),
            ("MIT WITH", LicenseDecision::Unknown),
            ("WITH MIT", LicenseDecision::Unknown),
            (
                "(MIT OR Apache-2.0) WITH LLVM-exception",
                LicenseDecision::Unknown,
            ),
            (
                "MIT WITH LLVM-exception WITH LLVM-exception",
                LicenseDecision::Unknown,
            ),
        ] {
            assert_eq!(
                evaluate(expression, LicenseContext::Closure, catalog, profile),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn denied_atom_precedes_unknown_in_compound_expressions() {
        let catalog = default_catalog();
        let profile = default_profile();
        for expression in [
            "AGPL-3.0-only AND MIT",
            "MIT AND AGPL-3.0-only",
            "Unknown-Proprietary AND AGPL-3.0-only",
            "BUSL-1.1 AND MIT",
        ] {
            assert_eq!(
                evaluate(expression, LicenseContext::Selected, catalog, profile),
                LicenseDecision::Denied,
                "{expression}"
            );
            assert_eq!(
                evaluate(expression, LicenseContext::Closure, catalog, profile),
                LicenseDecision::Denied,
                "{expression}"
            );
        }
    }

    #[test]
    fn legacy_oracle_4dcd_matches_current_interpreters() {
        #[derive(Deserialize)]
        struct Oracle {
            selected: Vec<String>,
            closure: Vec<String>,
            denied: Vec<String>,
            compounds: BTreeMap<String, serde_json::Value>,
        }
        let oracle: Oracle = serde_json::from_str(include_str!(
            "../../../deploy/environment-builder/tests/fixtures/legacy-policy-oracle-4dcd.json"
        ))
        .unwrap();
        let catalog = default_catalog();
        let profile = default_profile();
        for identifier in &oracle.selected {
            assert_eq!(
                evaluate(identifier, LicenseContext::Selected, catalog, profile),
                LicenseDecision::Allowed,
                "{identifier}"
            );
        }
        for identifier in &oracle.closure {
            assert_eq!(
                evaluate(identifier, LicenseContext::Closure, catalog, profile),
                LicenseDecision::Allowed,
                "{identifier}"
            );
        }
        for identifier in &oracle.denied {
            assert_eq!(
                evaluate(identifier, LicenseContext::Selected, catalog, profile),
                LicenseDecision::Denied,
                "{identifier}"
            );
            assert_eq!(
                evaluate(identifier, LicenseContext::Closure, catalog, profile),
                LicenseDecision::Denied,
                "{identifier}"
            );
        }
        for (expression, expected) in &oracle.compounds {
            if let Some(value) = expected.get("selected").and_then(serde_json::Value::as_str) {
                assert_eq!(
                    evaluate(expression, LicenseContext::Selected, catalog, profile).as_str(),
                    value,
                    "{expression}/selected"
                );
                let closure = expected
                    .get("closure")
                    .and_then(serde_json::Value::as_str)
                    .unwrap();
                assert_eq!(
                    evaluate(expression, LicenseContext::Closure, catalog, profile).as_str(),
                    closure,
                    "{expression}/closure"
                );
            }
        }
    }

    #[test]
    fn authorized_f5_denied_atom_matches_the_oracle() {
        #[derive(Deserialize)]
        struct Oracle {
            compounds: BTreeMap<String, serde_json::Value>,
        }
        let oracle: Oracle = serde_json::from_str(include_str!(
            "../../../deploy/environment-builder/tests/fixtures/legacy-policy-oracle-4dcd.json"
        ))
        .unwrap();
        let catalog = default_catalog();
        let profile = default_profile();
        for (expression, expected) in &oracle.compounds {
            if let Some(value) = expected
                .get("authorizedF5")
                .and_then(serde_json::Value::as_str)
            {
                for context in [LicenseContext::Selected, LicenseContext::Closure] {
                    assert_eq!(
                        evaluate(expression, context, catalog, profile).as_str(),
                        value,
                        "{expression}/{context:?}"
                    );
                }
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
