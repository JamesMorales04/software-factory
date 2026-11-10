//! Technology skill-pack requirements and deterministic freshness checks.
//!
//! A skill pack is a source-cited engineering reference. It is advisory: it
//! never grants tools or authority. A stale or missing pack blocks
//! `FORMAL_SPEC` planning until it is refreshed or explicitly waived by the
//! owner through the existing decision mechanism.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agentic::StackProfile;
use crate::planning::{SkillPack, SkillPlan};

mod catalog;

pub use catalog::{
    BUILTIN_LAST_VERIFIED_AT, BUILTIN_REVIEW_AFTER, BUILTIN_TECHNOLOGIES, builtin_skill_pack,
};

pub const SKILL_PACK_REVIEW_DAYS: i64 = 90;
pub const MAX_SKILL_PACK_STATUSES: usize = 256;

/// Effective freshness of a technology pack. `blocking` states prevent a
/// `FORMAL_SPEC` plan from proceeding until the pack is refreshed or the owner
/// explicitly waives it through the existing decision mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SkillPackState {
    /// A usable, in-date pack exists (persisted or builtin).
    Fresh,
    /// A persisted pack is past `review_after` and no newer builtin source
    /// exists; it cannot be refreshed without human research.
    Due,
    /// A persisted pack is due and a newer builtin revision with a different
    /// content hash can be installed immutably.
    Refreshable,
    /// The builtin catalog entry is past `review_after` or invalid and no
    /// usable persisted revision exists.
    Stale,
    /// Neither a persisted revision nor a builtin pack exists.
    Missing,
}

impl SkillPackState {
    pub const fn is_blocking(self) -> bool {
        matches!(self, Self::Due | Self::Stale | Self::Missing)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPackSummary {
    pub version: String,
    pub content_sha256: String,
    pub last_verified_at: String,
    pub review_after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPackStatus {
    pub technology: String,
    pub state: SkillPackState,
    pub blocking: bool,
    pub persisted: Option<SkillPackSummary>,
    pub builtin: Option<SkillPackSummary>,
    pub reason: String,
}

fn pack_content_sha256(pack: &SkillPack) -> String {
    let bytes = serde_json::to_vec(pack).expect("skill pack is serializable");
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn summary(pack: &SkillPack) -> SkillPackSummary {
    SkillPackSummary {
        version: pack.version.clone(),
        content_sha256: pack_content_sha256(pack),
        last_verified_at: pack.last_verified_at.clone(),
        review_after: pack.review_after.clone(),
    }
}

fn timestamp(value: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

fn builtin_is_usable(pack: &SkillPack, now: &str) -> bool {
    pack.validate().is_ok() && !pack.is_stale(now)
}

/// Classify one required technology from the latest persisted revision and the
/// curated builtin catalog. Callers must pass persisted packs in append order so
/// the last matching revision is the newest immutable record.
pub fn classify_technology(
    technology: &str,
    persisted: &[SkillPack],
    now: &str,
) -> SkillPackStatus {
    let builtin = builtin_skill_pack(technology);
    let persisted_pack = persisted
        .iter()
        .filter(|pack| pack.technology.eq_ignore_ascii_case(technology))
        .next_back();

    let persisted_summary = persisted_pack.map(summary);
    let builtin_summary = builtin.as_ref().map(summary);
    let persisted_fresh =
        persisted_pack.is_some_and(|pack| pack.validate().is_ok() && !pack.is_stale(now));
    let builtin_usable = builtin
        .as_ref()
        .is_some_and(|pack| builtin_is_usable(pack, now));

    let (state, reason) = if persisted_fresh {
        (
            SkillPackState::Fresh,
            "the latest persisted revision is valid and before review_after".to_string(),
        )
    } else if persisted_pack.is_none() {
        match builtin.as_ref() {
            Some(_) if builtin_usable => (
                SkillPackState::Fresh,
                "no persisted revision exists and the builtin pack is current".to_string(),
            ),
            Some(_) => (
                SkillPackState::Stale,
                "the builtin pack is past review_after or invalid".to_string(),
            ),
            None => (
                SkillPackState::Missing,
                "no persisted revision or builtin pack exists".to_string(),
            ),
        }
    } else if let Some(builtin) = builtin.as_ref() {
        let newer_than_persisted = persisted_pack.is_some_and(|persisted| {
            match (
                timestamp(&builtin.last_verified_at),
                timestamp(&persisted.last_verified_at),
            ) {
                (Some(builtin_verified), Some(persisted_verified)) => {
                    builtin_verified > persisted_verified
                }
                _ => false,
            }
        });
        let different_content = persisted_pack.is_some_and(|persisted| {
            pack_content_sha256(builtin) != pack_content_sha256(persisted)
        });
        if builtin_usable && newer_than_persisted && different_content {
            (
                SkillPackState::Refreshable,
                "the persisted revision is due and the builtin revision is newer with different content"
                    .to_string(),
            )
        } else if builtin_usable {
            (
                SkillPackState::Due,
                "the persisted revision is due and the builtin pack has no newer, different revision"
                    .to_string(),
            )
        } else {
            (
                SkillPackState::Stale,
                "the builtin pack is past review_after or invalid".to_string(),
            )
        }
    } else {
        (
            SkillPackState::Due,
            "the persisted revision is due and no builtin refresh source exists".to_string(),
        )
    };

    SkillPackStatus {
        technology: technology.to_string(),
        state,
        blocking: state.is_blocking(),
        persisted: persisted_summary,
        builtin: builtin_summary,
        reason,
    }
}

/// Classify a bounded, de-duplicated technology list. The input order is
/// preserved so callers can keep a deterministic presentation order.
pub fn classify_skill_packs(
    technologies: &[String],
    persisted: &[SkillPack],
    now: &str,
) -> Vec<SkillPackStatus> {
    technologies
        .iter()
        .take(MAX_SKILL_PACK_STATUSES)
        .map(|technology| classify_technology(technology, persisted, now))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillPlanError {
    #[error("skill pack is required for: {0}")]
    Missing(String),
    #[error("skill pack is stale for: {0}")]
    Stale(String),
    #[error("invalid skill plan: {0}")]
    Invalid(String),
}

/// Technologies evidenced by the detected stack. Names are normalized to lower
/// case and de-duplicated so the same technology cannot require two packs.
pub fn required_technologies(stack: &StackProfile) -> Vec<String> {
    let mut technologies = Vec::new();
    for technology in stack
        .languages
        .iter()
        .chain(stack.frameworks.iter())
        .chain(stack.package_managers.iter())
        .chain(stack.build_tools.iter())
        .chain(stack.data.iter())
        .chain(stack.test_frameworks.iter())
        .chain(stack.browser.iter())
        .chain(stack.infra.iter())
    {
        let normalized = technology.trim().to_ascii_lowercase();
        if !normalized.is_empty() && !technologies.contains(&normalized) {
            technologies.push(normalized);
        }
    }
    technologies.sort();
    technologies
}

/// Build a skill plan from fresh persisted packs and curated official-source
/// packs. Unknown technologies and stale knowledge fail closed instead of
/// receiving invented guidance. A refreshable pack is proposed as a new
/// immutable revision; its review dates are never extended in place.
pub fn build_skill_plan(
    stack: &StackProfile,
    persisted: &[SkillPack],
    now: &str,
) -> Result<SkillPlan, SkillPlanError> {
    let needs = required_technologies(stack);
    let statuses = classify_skill_packs(&needs, persisted, now);
    let mut existing = Vec::new();
    let mut proposed = Vec::new();
    for status in statuses {
        match status.state {
            SkillPackState::Fresh if status.persisted.is_some() => {
                existing.push(status.technology);
            }
            SkillPackState::Fresh | SkillPackState::Refreshable => {
                let pack = builtin_skill_pack(&status.technology)
                    .ok_or_else(|| SkillPlanError::Missing(status.technology.clone()))?;
                if pack.validate().is_err() || pack.is_stale(now) {
                    return Err(SkillPlanError::Stale(status.technology));
                }
                proposed.push(pack);
            }
            SkillPackState::Due | SkillPackState::Stale => {
                return Err(SkillPlanError::Stale(status.technology));
            }
            SkillPackState::Missing => {
                return Err(SkillPlanError::Missing(status.technology));
            }
        }
    }
    let plan = SkillPlan {
        needs,
        existing,
        proposed,
        owner: "Librarian".into(),
    };
    plan.validate().map_err(SkillPlanError::Invalid)?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::StackProfile;

    fn rust_stack() -> StackProfile {
        StackProfile {
            languages: vec!["rust".into()],
            package_managers: vec!["cargo".into()],
            build_tools: vec!["cargo".into()],
            ..StackProfile::default()
        }
    }

    #[test]
    fn builtin_packs_are_fresh_source_cited_and_valid() {
        let plan = build_skill_plan(&rust_stack(), &[], "2026-11-09T00:00:00Z").unwrap();
        assert!(plan.needs.contains(&"rust".into()));
        assert!(plan.proposed.iter().any(|pack| pack.technology == "rust"));
        for pack in &plan.proposed {
            pack.validate().unwrap();
            assert!(!pack.source_urls.is_empty());
        }
        assert!(matches!(
            build_skill_plan(&rust_stack(), &[], "2028-01-01T00:00:00Z"),
            Err(SkillPlanError::Stale(_))
        ));
    }

    #[test]
    fn unknown_technology_fails_closed() {
        let stack = StackProfile {
            languages: vec!["brainfuck".into()],
            ..StackProfile::default()
        };
        assert!(matches!(
            build_skill_plan(&stack, &[], "2026-11-09T00:00:00Z"),
            Err(SkillPlanError::Missing(_))
        ));
    }

    fn persisted(technology: &str, last_verified_at: &str, review_after: &str) -> SkillPack {
        let mut pack = builtin_skill_pack(technology).unwrap();
        pack.last_verified_at = last_verified_at.into();
        pack.review_after = review_after.into();
        pack.best_practices
            .push("persisted revision content marker".into());
        pack
    }

    #[test]
    fn classification_covers_fresh_due_refreshable_stale_and_missing() {
        let now = "2026-11-09T00:00:00Z";
        let builtin = builtin_skill_pack("rust").unwrap();

        let fresh = classify_technology("rust", std::slice::from_ref(&builtin), now);
        assert_eq!(fresh.state, SkillPackState::Fresh);
        assert!(!fresh.blocking);

        let due = classify_technology(
            "legacy-tool",
            &[{
                let mut pack = builtin.clone();
                pack.technology = "legacy-tool".into();
                pack.last_verified_at = "2026-01-01T00:00:00Z".into();
                pack.review_after = "2026-02-01T00:00:00Z".into();
                pack
            }],
            now,
        );
        assert_eq!(due.state, SkillPackState::Due);
        assert!(due.blocking);

        let refreshable = classify_technology(
            "rust",
            &[persisted(
                "rust",
                "2026-01-01T00:00:00Z",
                "2026-04-01T00:00:00Z",
            )],
            now,
        );
        assert_eq!(refreshable.state, SkillPackState::Refreshable);
        assert!(!refreshable.blocking);

        let stale = classify_technology("rust", &[], "2028-01-01T00:00:00Z");
        assert_eq!(stale.state, SkillPackState::Stale);
        assert!(stale.blocking);

        let missing = classify_technology("brainfuck", &[], now);
        assert_eq!(missing.state, SkillPackState::Missing);
        assert!(missing.blocking);
    }

    #[test]
    fn catalog_is_bounded_source_cited_and_valid() {
        let mut seen = std::collections::BTreeSet::new();
        for technology in BUILTIN_TECHNOLOGIES {
            assert!(seen.insert(*technology), "duplicate catalog entry");
            let pack = builtin_skill_pack(technology)
                .unwrap_or_else(|| panic!("missing builtin pack for {technology}"));
            pack.validate()
                .unwrap_or_else(|error| panic!("invalid pack for {technology}: {error}"));
            assert!(!pack.best_practices.is_empty());
            assert!(!pack.security_notes.is_empty());
            assert!(!pack.commands.is_empty());
            assert!(
                pack.source_urls
                    .iter()
                    .all(|url| url.starts_with("https://")),
                "pack for {technology} must cite HTTPS sources"
            );
        }
    }

    #[test]
    fn broad_detected_stack_is_fully_covered_by_the_catalog() {
        let root =
            std::env::temp_dir().join(format!("factory-skill-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("migrations")).unwrap();
        std::fs::create_dir_all(root.join("k8s")).unwrap();
        std::fs::create_dir_all(root.join("terraform")).unwrap();
        std::fs::create_dir_all(root.join(".github/workflows")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[dependencies]
axum = "1"
actix-web = "4"
rocket = "0.5"
sqlx = "0.8"
serde = "1"
tracing = "0.1"
reqwest = "0.12"
hyper = "1"
tonic = "0.12"
diesel = "2"
tauri = "2"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"packageManager":"pnpm@9.0.0","dependencies":{"react":"1","vite":"1","vue":"3","svelte":"4","@angular/core":"18","electron":"30","@tauri-apps/api":"2"},"devDependencies":{"vitest":"1","@playwright/test":"1"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        std::fs::write(root.join("pom.xml"), "<project/>").unwrap();
        std::fs::write(root.join("build.gradle.kts"), "plugins { kotlin(\"jvm\") }").unwrap();
        std::fs::write(root.join("App.csproj"), "<Project/>").unwrap();
        std::fs::write(root.join("Gemfile"), "gem \"rails\"\n").unwrap();
        std::fs::write(
            root.join("composer.json"),
            r#"{"require":{"laravel/framework":"11"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("Package.swift"), "// swift-tools-version: 6.0\n").unwrap();
        std::fs::write(root.join("deno.json"), "{}\n").unwrap();
        std::fs::write(
            root.join("compose.yaml"),
            "services:\n  db:\n    image: postgres:16\n  cache:\n    image: redis:7\n  sql:\n    image: mysql:8\n",
        )
        .unwrap();
        std::fs::write(root.join("Dockerfile"), "FROM scratch\n").unwrap();
        std::fs::write(root.join("playwright.config.ts"), "export default {}\n").unwrap();

        let stack = crate::agentic::detect_stack(&root);
        let required = required_technologies(&stack);
        assert!(
            required.len() >= 20,
            "unexpectedly narrow detection: {required:?}"
        );
        for technology in &required {
            assert!(
                builtin_skill_pack(technology).is_some(),
                "detected technology {technology} has no catalog pack"
            );
        }
        for expected in [
            "java",
            "gradle",
            "kotlin",
            "csharp",
            "dotnet",
            "ruby",
            "rails",
            "php",
            "laravel",
            "swift",
            "vue",
            "svelte",
            "angular",
            "pnpm",
            "deno",
            "postgresql",
            "mysql",
            "redis",
            "kubernetes",
            "terraform",
            "github-actions",
            "tauri",
            "electron",
            "actix-web",
            "rocket",
            "serde",
            "tracing",
            "reqwest",
            "hyper",
            "tonic",
        ] {
            assert!(
                required.contains(&expected.to_string()),
                "expected {expected} in detected stack {required:?}"
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
