# Formatting, linting and static analysis

**Policy:** every implemented language must have an automated formatter, lint/static-analysis checks and a type/build check before its first production PR. CI executes the same pinned commands used locally. Warnings are actionable, not decorative.

## Proposed toolchain (activate only for languages actually selected)

| Surface | Required default | Additional gate / conditions |
|---|---|---|
| Rust Core/Worker/tools (required) | `cargo fmt --check`, strict Clippy, `cargo test`, `cargo build`, `cargo doc`, `cargo audit` | Pinned Rust 1.90.0 and locked Cargo; see [rust-standards](rust-standards.md) |
| TypeScript / React Office | TypeScript `tsc --noEmit`, Biome formatter/linter (`biome check`) | React-specific checks; use ESLint + `typescript-eslint` only if Biome lacks a required rule; avoid overlapping tools |
| Markdown documentation | `markdownlint-cli2` | Automated relative-link and code-fence validation; check external links in a bounded, reliable lane |
| Shell scripts (if introduced) | `shellcheck` and `shfmt` | Test important scripts in disposable environments |
| YAML / JSON / GitHub Actions (if introduced) | Schema validation and formatting | `actionlint` for GitHub Actions workflows |

Versions and configurations must be pinned in repository manifests/lockfiles or explicit tool manifests when introduced. Do not claim commands exist yet: **F0 currently contains no application toolchain or CI certification**.

### TypeScript policy

Prefer Biome for fast formatting and general linting plus `tsc` for type checking. If semantic React hooks or security rules require ESLint, install only the missing focused plugin/rules, document why, and avoid duplicate formatters. Avoid unchecked `any`; provide explicit boundary types and schema validation for untrusted payloads.

### Selected Rust toolchain

[ADR-0018](../architecture/decisions/ADR-0018-native-rust-replacement.md) ratifies Rust for Core, Workers, provider adapters, persistence and governance tools. Rust 1.90.0 is pinned by `rust-toolchain.toml`; dependencies are locked by `Cargo.lock`. These commands are required and run on the exact candidate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
cargo doc --workspace --no-deps --locked
cargo audit --file Cargo.lock
```

The RustSec audit tool/version is pinned in CI. `cargo-nextest` and `cargo-deny` may be evaluated before adoption; do not add them speculatively. This language decision does not waive SEC-01 isolation.

### Historical Go toolchain

[ADR-0005](../architecture/decisions/ADR-0005-go-core-worker-language.md) is superseded by ADR-0018. Legacy Go files, workflows and lint commands remain in the tree only until the Rust replacement and its acceptance evidence are complete; they are not the target toolchain. Do not add new Go code or extend Go CI.

## Gate rules

- Formatting, linting, type/build checks and scoped tests must pass before final acceptance. Add integration/E2E and race checks when relevant.
- No blanket `noqa`, `nolint`, `eslint-disable`, `@ts-ignore` or Clippy allow-lists. Scoped suppression requires a reason, ownership and an issue or review reference if persistent.
- A failing mandatory check is `FAIL`, not `NOT_APPLICABLE`; a tool unavailable due to environment is `BLOCKED` until resolved. No disabling checks to green CI.
- Changes to linter configuration and thresholds require review of the quality impact, and an ADR if they materially weaken project policy.
- Generated code may have explicit path-scoped exemptions when justified; source generators and generated outputs must still pass required security and compilation checks.
- Maintain exact commands, versions, result artifacts and cache identities in PRs and CI evidence. Docs-only changes should eventually use a dedicated lightweight docs quality lane.
