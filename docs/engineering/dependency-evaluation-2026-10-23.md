# Dependency evaluation record — 2026-10-23

**Candidate context:** native Rust branch, Rust 1.90.0, SQLx SQLite, rootless Podman, hardened Git CLI. Versions checked with `cargo info` on 2026-10-23. This is an evaluation record, not an operational certificate.

| Area | Candidates | Decision | Rationale |
|---|---|---|---|
| JSON Schema | `schemars` 1.2.3, `jsonschema` 0.60.1 | **ADOPTED / IMPLEMENTED** | Rust-derived assessment schema, strict schema validation, provider keyword allowlist; 3 tests pass. |
| YAML parsing | `serde-saphyr` 2.0.0 | **EVALUATED / IMPLEMENTED** | Replaces `serde_yaml_ng` and its unmaintained `unsafe-libyaml` dependency (RUSTSEC-2024-0449) with maintained pure-Rust Serde deserialization and unknown-field rejection. Dual MIT/Apache-2.0, declared MSRV 1.89 (compatible with Rust 1.90). Focused registry and negative governance tests pass, and `cargo audit --file Cargo.lock` reports no advisories; Windows behavior is not separately exercised. |
| Git | `gix` 0.90 (MSRV 1.88), `git2` 0.21, hardened CLI | **RETAIN_CLI (evaluated)** | Current CLI is narrow, uses exact Git semantics and now disables hooks/replace/grafts/global config. A library migration needs a full worktree/lineage/cleanup equivalence suite; not justified in this increment. |
| Podman | `bollard` 0.21.1, `podman-api` 0.11, CLI | **RETAIN_CLI / SPIKE_REQUIRED** | Docker-compatible APIs may not expose every rootless Podman security control. The supervisor uses named-volume mounts but remains blocked on independent isolation and credential confinement; no API adoption without an explicit endpoint/authorization spike. |
| GitHub | `octocrab` 0.54.5 (MSRV 1.85) | **EVALUATED / ADOPT_LATER** | Suitable adapter candidate when delivery is implemented; Core delivery authority and token scope remain outside the library. |
| Filesystem | `cap-std` 4.0.3, `rustix` 1.1.5 | **EVALUATED / PARTIAL** | `create_new`, symlink-component checks and raw Git mode checks reduce fixture/write races. Full same-UID pathname-race elimination needs an anchored directory/container boundary; SEC-01 remains blocked. |
| Async lifecycle | `tokio-util` 0.7.20 | **EVALUATED / DEFER** | Current tasks are bounded and explicitly joined/aborted. Adopt `CancellationToken` when provider/supervisor lifecycle needs it. |
| SSE | existing bounded parser vs external crate | **RETAIN_CUSTOM** | Custom parser has byte-boundary, reconnect, malformed and size-limit tests. No replacement demonstrated better correctness. |
| Log redaction | `regex` 1.11.1 | **ADOPTED / IMPLEMENTED** | Already present transitively; direct use keeps the redaction rules declarative and testable. It masks registered role credentials plus bearer/keyed forms in Core/Supervisor log output. Redaction is best effort and does not authorize logging prompts, source bodies or secrets. |
| Dependency policy | `cargo-deny` 0.20.2 | **EVALUATE_BEFORE_ADOPTION** | Useful for license/source/duplicate policy; keep `cargo-audit` baseline and avoid overlapping unpinned tools. |
| OCI specs | `oci-spec` | **DEFER** | Normal Podman operations express current needs; revisit only for direct OCI spec manipulation. |

## Follow-up gates

- Podman client spike must prove explicit rootless endpoint selection, no rootful/Docker fallback, resource/mount/network controls, cancellation and orphan reconciliation before replacing the CLI.
- Git library migration must preserve exact commit/tree/parent identity, worktree cleanup, symlink/gitlink rejection and hardened config semantics.
- Structured output must be exercised against the actual installed Codex/OpenCode schema capabilities before a live turn; no paid session is authorized here.
- Windows support for every selected library/command remains `NOT_TESTED`.
