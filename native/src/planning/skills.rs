//! Technology skill-pack requirements and deterministic freshness checks.
//!
//! A skill pack is a source-cited engineering reference. It is advisory: it
//! never grants tools or authority. A stale or missing pack blocks
//! `FORMAL_SPEC` planning until it is refreshed or explicitly waived by the
//! owner through the existing decision mechanism.

use crate::agentic::StackProfile;
use crate::planning::{SkillPack, SkillPlan};

pub const SKILL_PACK_REVIEW_DAYS: i64 = 90;
pub const BUILTIN_LAST_VERIFIED_AT: &str = "2026-11-09T00:00:00Z";
pub const BUILTIN_REVIEW_AFTER: &str = "2027-02-07T00:00:00Z";

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
/// packs. Unknown technologies fail closed instead of receiving invented
/// guidance.
pub fn build_skill_plan(
    stack: &StackProfile,
    persisted: &[SkillPack],
    now: &str,
) -> Result<SkillPlan, SkillPlanError> {
    let needs = required_technologies(stack);
    let mut existing = Vec::new();
    let mut proposed = Vec::new();
    for technology in &needs {
        let fresh = persisted
            .iter()
            .filter(|pack| pack.technology.eq_ignore_ascii_case(technology))
            .find(|pack| pack.validate().is_ok() && !pack.is_stale(now));
        if fresh.is_some() {
            existing.push(technology.clone());
            continue;
        }
        let stale = persisted
            .iter()
            .any(|pack| pack.technology.eq_ignore_ascii_case(technology) && pack.is_stale(now));
        match builtin_skill_pack(technology) {
            Some(pack) => proposed.push(pack),
            None if stale => return Err(SkillPlanError::Stale(technology.clone())),
            None => return Err(SkillPlanError::Missing(technology.clone())),
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

fn pack(
    technology: &str,
    sources: &[&str],
    best_practices: &[&str],
    anti_patterns: &[&str],
    security_notes: &[&str],
    commands: &[&str],
) -> SkillPack {
    SkillPack {
        technology: technology.into(),
        version: "current".into(),
        source_urls: sources.iter().map(|source| (*source).to_string()).collect(),
        last_verified_at: BUILTIN_LAST_VERIFIED_AT.into(),
        review_after: BUILTIN_REVIEW_AFTER.into(),
        best_practices: best_practices
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        anti_patterns: anti_patterns
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        commands: commands.iter().map(|value| (*value).to_string()).collect(),
        security_notes: security_notes
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        deprecations: Vec::new(),
        license: "Official documentation terms; verify upstream license.".into(),
        provenance: "Factory curated pack built from the cited official documentation.".into(),
        content_sha256: String::new(),
    }
}

/// Curated packs for the technologies the Factory supports. These are not a
/// substitute for refreshing upstream documentation; `review_after` forces a
/// periodic review.
pub fn builtin_skill_pack(technology: &str) -> Option<SkillPack> {
    let pack = match technology.to_ascii_lowercase().as_str() {
        "rust" => pack(
            "rust",
            &[
                "https://doc.rust-lang.org/book/",
                "https://doc.rust-lang.org/cargo/",
                "https://rust-lang.github.io/rust-clippy/",
            ],
            &[
                "Prefer ownership and explicit error types over panics in library code.",
                "Use `cargo fmt`, Clippy with `-D warnings`, and tests as deterministic checks.",
            ],
            &[
                "Do not add `unsafe` without a documented safety argument.",
                "Do not hide fallible operations behind `unwrap` in production paths.",
            ],
            &["Treat dependency advisories as blocking findings."],
            &[
                "cargo fmt --check",
                "cargo clippy -- -D warnings",
                "cargo test",
            ],
        ),
        "cargo" => pack(
            "cargo",
            &["https://doc.rust-lang.org/cargo/"],
            &[
                "Commit `Cargo.lock` for applications and use `--locked` in checks.",
                "Vendor dependencies for offline, reproducible builds.",
            ],
            &["Do not let a check command resolve new dependencies implicitly."],
            &["Verify registry and source replacement before an offline build."],
            &["cargo metadata --locked", "cargo vendor --locked vendor"],
        ),
        "typescript" => pack(
            "typescript",
            &["https://www.typescriptlang.org/docs/"],
            &[
                "Keep strict type checking enabled for application code.",
                "Prefer explicit domain types at API boundaries.",
            ],
            &["Do not silence errors with `any` or blanket `ts-ignore`."],
            &["Treat untrusted JSON as `unknown` until validated."],
            &["npm run typecheck"],
        ),
        "react" => pack(
            "react",
            &["https://react.dev/"],
            &[
                "Keep state close to the component that owns it.",
                "Use accessible semantic elements before adding ARIA.",
            ],
            &["Do not derive authoritative state from presentation state."],
            &["Render untrusted text as text, never as raw HTML."],
            &["npm test"],
        ),
        "vite" => pack(
            "vite",
            &["https://vite.dev/guide/"],
            &["Keep build-time environment values non-secret."],
            &["Do not expose server credentials through `VITE_` variables."],
            &["Treat client bundles as public artifacts."],
            &["npm run build"],
        ),
        "playwright" => pack(
            "playwright",
            &["https://playwright.dev/docs/intro"],
            &[
                "Prefer role and label locators over CSS selectors.",
                "Test the real application state and observable outcomes.",
            ],
            &["Do not use fixed sleeps as synchronization."],
            &["Never store credentials in test source or traces."],
            &["npx playwright test"],
        ),
        "sqlx" => pack(
            "sqlx",
            &["https://docs.rs/sqlx/latest/sqlx/"],
            &[
                "Use versioned migrations and explicit transactions.",
                "Bind parameters instead of interpolating SQL.",
            ],
            &["Do not run unchecked schema changes against production data."],
            &["Treat database errors as typed, recoverable outcomes."],
            &["cargo test"],
        ),
        "axum" => pack(
            "axum",
            &["https://docs.rs/axum/latest/axum/"],
            &[
                "Keep handlers thin and domain decisions in typed services.",
                "Return explicit status codes and typed error bodies.",
            ],
            &["Do not trust request bodies before validation."],
            &["Enforce authorization in the service boundary."],
            &["cargo test"],
        ),
        "tokio" => pack(
            "tokio",
            &["https://tokio.rs/"],
            &[
                "Bound concurrency and cancel work on shutdown.",
                "Never hold a lock across an await unless the design requires it.",
            ],
            &["Do not spawn unbounded background tasks."],
            &["Propagate cancellation to external calls."],
            &["cargo test"],
        ),
        "python" => pack(
            "python",
            &["https://docs.python.org/3/"],
            &[
                "Use type hints at module and public function boundaries.",
                "Keep dependencies pinned for reproducible environments.",
            ],
            &["Do not use mutable default arguments."],
            &["Validate untrusted input before parsing or executing it."],
            &["python3 -m pytest"],
        ),
        "fastapi" => pack(
            "fastapi",
            &["https://fastapi.tiangolo.com/"],
            &[
                "Declare request and response models explicitly.",
                "Keep authorization checks in dependencies or services.",
            ],
            &["Do not return raw exception text to clients."],
            &["Treat generated OpenAPI as a contract to review."],
            &["python3 -m pytest"],
        ),
        "pytest" => pack(
            "pytest",
            &["https://docs.pytest.org/"],
            &["Prefer fixtures with explicit scope and cleanup."],
            &["Do not depend on test execution order."],
            &["Never embed live credentials in fixtures."],
            &["python3 -m pytest"],
        ),
        "node" => pack(
            "node",
            &["https://nodejs.org/docs/latest/api/"],
            &["Pin the runtime version in the repository."],
            &["Do not run untrusted scripts with ambient credentials."],
            &["Treat the runtime as a public client for network calls."],
            &["npm test"],
        ),
        "npm" => pack(
            "npm",
            &["https://docs.npmjs.com/"],
            &["Commit the lockfile and use `npm ci` for deterministic installs."],
            &["Do not install packages from unreviewed registries."],
            &["Review install scripts before execution."],
            &["npm ci"],
        ),
        "sqlite" => pack(
            "sqlite",
            &["https://www.sqlite.org/docs.html"],
            &["Use WAL mode and explicit transactions for concurrent writers."],
            &["Do not rely on implicit column affinity for validation."],
            &["Treat database files as sensitive local state."],
            &["cargo test"],
        ),
        "migrations" => pack(
            "migrations",
            &["https://www.sqlite.org/lang_altertable.html"],
            &[
                "Use versioned, forward-only migrations.",
                "Test migrations against a fresh database and an upgraded database.",
            ],
            &["Do not edit an applied migration in place."],
            &["Treat destructive schema changes as owner-reserved."],
            &["cargo test"],
        ),
        "go" => pack(
            "go",
            &["https://go.dev/doc/"],
            &["Use `go test ./...` and `go vet ./...` as deterministic checks."],
            &["Do not ignore returned errors."],
            &["Pin module versions and verify checksums."],
            &["go test ./..."],
        ),
        "podman-compose" | "podman" | "container" | "docker" => pack(
            "container",
            &[
                "https://docs.podman.io/",
                "https://podman-desktop.io/docs/compose",
            ],
            &[
                "Run rootless with an explicit network and least privilege.",
                "Pin image digests for reproducible deployments.",
            ],
            &[
                "Do not mount the host Podman socket or use privileged mode.",
                "Do not add host networking as a shortcut.",
            ],
            &["Treat container images and mounts as a trust boundary."],
            &["podman compose config"],
        ),
        "next" => pack(
            "next",
            &["https://nextjs.org/docs"],
            &["Keep server-only secrets out of client bundles."],
            &["Validate route inputs on the server."],
            &["Treat server actions as authenticated endpoints."],
            &["npm run build"],
        ),
        "vitest" => pack(
            "vitest",
            &["https://vitest.dev/guide/"],
            &["Keep tests deterministic and independent."],
            &["Do not depend on shared mutable module state."],
            &["Never commit live credentials to test fixtures."],
            &["npx vitest run"],
        ),
        "jest" => pack(
            "jest",
            &["https://jestjs.io/docs/getting-started"],
            &["Keep tests deterministic and independent."],
            &["Do not depend on execution order."],
            &["Never commit live credentials to test fixtures."],
            &["npx jest --runInBand"],
        ),
        "clap" => pack(
            "clap",
            &["https://docs.rs/clap/latest/clap/"],
            &["Declare arguments with typed parsers and help text."],
            &["Return non-zero exit codes for invalid input."],
            &["Do not echo secrets in error output."],
            &["cargo test"],
        ),
        "ruff" => pack(
            "ruff",
            &["https://docs.astral.sh/ruff/"],
            &["Run lint and format checks in CI."],
            &["Do not blanket-disable rules without a scoped rationale."],
            &["Treat lint findings as review findings."],
            &["python3 -m ruff check ."],
        ),
        "uv" => pack(
            "uv",
            &["https://docs.astral.sh/uv/"],
            &["Commit the lockfile and use locked syncs."],
            &["Do not resolve new dependencies during a frozen check."],
            &["Verify package sources before installation."],
            &["uv sync --locked"],
        ),
        _ => return None,
    };
    Some(pack)
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
}
