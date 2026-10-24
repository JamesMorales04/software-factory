# Rust dependency selection

**Status:** normative for first-party Rust infrastructure choices. Read with [Rust standards](rust-standards.md), [service placement](../architecture/service-placement-policy.md) and [structured model outputs](structured-model-outputs.md).

## Principle

Use a mature, maintained library for established infrastructure. Keep Factory
authorization, policy and domain invariants in Factory code. **Reuse
infrastructure mechanisms; own product invariants.**

A library may perform an operation. It never decides whether Factory is
authorized to perform it.

## Evaluation

Before adding a significant infrastructure dependency, record:

1. The capability actually needed.
2. Existing code/dependencies that may already solve it.
3. Realistic candidate crates.
4. Maintenance and release activity.
5. Security advisories and RustSec status.
6. License compatibility.
7. MSRV / Rust 1.90.0 compatibility.
8. Linux and Windows requirements.
9. Native/system dependencies and build complexity.
10. Transitive dependency and binary-size impact.
11. Runtime performance and failure behavior.
12. Fit with Factory security and authority boundaries.
13. Integration, equivalence and negative-test effort.
14. Maintainability compared with the current implementation.

Use exact versions in `Cargo.toml`/`Cargo.lock`; never wildcards. A spike is
required when the library's behavior determines a security or lifecycle
invariant. Keep the decision proportionate to risk.

## Decision statuses

`EVALUATED`, `ADOPTED`, `REJECTED_WITH_REASON`, `IMPLEMENTED`, `VERIFIED`,
`BLOCKED`, `NOT_TESTED`. A crate in `Cargo.toml` is not an operational claim.

## Boundaries

- Git/GitHub/Podman/filesystem libraries are adapters. Core still owns
  authorization, leases, fencing, candidate acceptance and delivery rights.
- A container-runtime API is not an isolation boundary. Rootless Podman must be
  explicitly configured; no fallback to rootful Podman, Docker or TCP.
- A capability-based filesystem API reduces pathname races; it does not provide
  process or credential isolation.
- Structured-output libraries generate/validate contracts. Model output remains
  untrusted and cannot grant authority.
- Do not add a second library for a capability already covered by Tokio, Axum,
  SQLx, reqwest, Serde, tracing or the existing selected tooling without a
  concrete benefit.
- Remove superseded custom code only after equivalence and negative tests pass.

## Review evidence

Record the candidate, crate version, Cargo/RustSec evidence, selected/rejected
alternatives and verification commands in the PR/handoff. Evaluate
`cargo-deny` for license/source/duplicate policy before adoption; keep
`cargo-audit` as the security baseline.
