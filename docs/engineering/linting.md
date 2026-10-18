# Formatting, linting and static analysis

**Policy:** every implemented language must have an automated formatter, lint/static-analysis checks and a type/build check before its first production PR. CI executes the same pinned commands used locally. Warnings are actionable, not decorative.

## Proposed toolchain (activate only for languages actually selected)

| Surface | Required default | Additional gate / conditions |
|---|---|---|
| Go (candidate Core/Worker) | `gofmt`, `go vet`, `go test` | `golangci-lint` with a pinned supported release and curated analyzers; `go test -race` for affected concurrency code |
| Rust (alternative Core/Worker) | `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` | Select Rust **instead of** Go after TECH-01, not both by default; `cargo audit` for dependency review when chosen |
| TypeScript / React / Paseo adapter | TypeScript `tsc --noEmit`, Biome formatter/linter (`biome check`) | React-specific checks; use ESLint + `typescript-eslint` only if Biome lacks a required rule; avoid overlapping tools |
| Markdown documentation | `markdownlint-cli2` | Automated relative-link and code-fence validation; check external links in a bounded, reliable lane |
| Shell scripts (if introduced) | `shellcheck` and `shfmt` | Test important scripts in disposable environments |
| YAML / JSON / GitHub Actions (if introduced) | Schema validation and formatting | `actionlint` for GitHub Actions workflows |

Versions and configurations must be pinned in repository manifests/lockfiles or explicit tool manifests when introduced. Do not claim commands exist yet: **F0 currently contains no application toolchain or CI certification**.

### TypeScript policy

Prefer Biome for fast formatting and general linting plus `tsc` for type checking. If semantic React hooks or security rules require ESLint, install only the missing focused plugin/rules, document why, and avoid duplicate formatters. Avoid unchecked `any`; provide explicit boundary types and schema validation for untrusted payloads.

### Go / Rust decision

ADR-0005 remains unresolved; establish baseline configurations for the selected language after the bounded Go-versus-Rust feasibility probe. Do not create or maintain two full build pipelines simply to preserve flexibility.

## Gate rules

- Formatting, linting, type/build checks and scoped tests must pass before final acceptance. Add integration/E2E and race checks when relevant.
- No blanket `noqa`, `nolint`, `eslint-disable`, `@ts-ignore` or Clippy allow-lists. Scoped suppression requires a reason, ownership and an issue or review reference if persistent.
- A failing mandatory check is `FAIL`, not `NOT_APPLICABLE`; a tool unavailable due to environment is `BLOCKED` until resolved. No disabling checks to green CI.
- Changes to linter configuration and thresholds require review of the quality impact, and an ADR if they materially weaken project policy.
- Generated code may have explicit path-scoped exemptions when justified; source generators and generated outputs must still pass required security and compilation checks.
- Maintain exact commands, versions, result artifacts and cache identities in PRs and CI evidence. Docs-only changes should eventually use a dedicated lightweight docs quality lane.
