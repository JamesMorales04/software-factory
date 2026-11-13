# RUSTSEC-2026-0459 — SQLite advisory resolved by a vendored sqlx-sqlite patch

**Status:** RESOLVED (2026-11-13)
**Resolution candidate:** `a8017cc1a210b1965c80dc673b8d0dc3131f307c`
**Scope:** the local-first, TRUSTED_LOCAL developer-preview product. This is not
an isolation or certification claim.

## Advisory

- `RUSTSEC-2026-0459` / `CVE-2026-119375` / `GHSA-72w2-rjgg-q729`
- `libsqlite3-sys` 0.30.1, pulled by `sqlx-sqlite` 0.8.6, was affected.
- Affected `libsqlite3-sys` 0.6.0–0.38.2; patched in 0.39.0 (SQLite 3.54.0) and
  0.39.1 (SQLite 3.54.1).
- The bundled SQLite could return uninitialized page-cache memory when a process
  opened a database together with a sibling `-wal` file whose page size differs.
  The attack vector is local and requires the victim to open the crafted pair.

## Original wait decision (2026-11-09)

The Product Owner temporarily accepted the blocker and merged the agentic-flow
PR while keeping `cargo audit` unsuppressed. At that time:

- `sqlx-sqlite` 0.8.6 and 0.9.0 both declared
  `libsqlite3-sys >=0.30.1, <0.38.0`; Cargo could not select 0.39.x.
- `sqlx 0.9.0` required Rust 1.94 while this project pins Rust 1.90.
- Vendoring `sqlx-sqlite` was identified as a possible future fix but was not
  authorized by that decision.

## Resolution (2026-11-13)

The project now uses a manifest-only vendor patch of the published
`sqlx-sqlite` 0.8.6 crate:

- `vendor/sqlx-sqlite/` is byte-for-byte the published 0.8.6 source except for
  `Cargo.toml`, which changes `libsqlite3-sys` from `0.30.1` to
  `>=0.30.1, <0.41.0`. `vendor/sqlx-sqlite/PATCH.md` records the source
  checksum, exact change and removal condition.
- The workspace `[patch.crates-io]` entry selects the vendored crate; the lockfile
  resolves `libsqlite3-sys` **0.39.1** from crates.io.
- No `cargo-audit` or `cargo-deny` advisory ignore was added. `cargo audit
  --file Cargo.lock` exits 0 with only the separately allowed `rustix` warning,
  and `cargo deny check licenses bans sources` passes.
- The `sqlx-sqlite` Rust source and the Factory SQL/query code are unchanged.

Verification at the resolution candidate:

- `cargo fmt --all -- --check` and strict workspace Clippy — clean.
- `cargo test --workspace --locked --features test-support` with the CI
  bwrap/provider skips — 1398 passed, 0 failed, 4 ignored.
- `cargo build --workspace --locked` and `cargo doc --workspace --no-deps
  --locked` — clean.
- `cargo audit --file Cargo.lock` — exit 0; `cargo deny check licenses bans
  sources` — licenses, bans and sources OK.
- Native architecture check and parity JSON — PASS. Office quality is covered by
  the exact-head `Scenario runner and Office quality` CI lane.

## Risk assessment

Core opens its own SQLite database under the Factory data directory. It does not
open project repositories as SQLite databases. Exploitation required a local
actor able to write a sibling `-wal` file with a mismatched page size and a
Factory process that later opened that exact pair. The product is TRUSTED_LOCAL;
the host and its local users are already inside the trust boundary, and
`SEC-01-ISOLATED` is out of scope under ADR-0035.

## Exit criteria

1. A patched `libsqlite3-sys` is selected without suppressing the audit gate —
   **met** via the vendor manifest patch and `libsqlite3-sys` 0.39.1.
2. `cargo audit --file Cargo.lock` is clean — **met**.
3. Full Rust, Office, architecture, parity and smoke checks pass — **met** for
   the candidate above.
4. This document is updated and CI no longer fails at the audit step — **met**.

**Removal condition:** remove `vendor/sqlx-sqlite` and the `[patch.crates-io]`
entry when a published `sqlx-sqlite` release permits
`libsqlite3-sys >=0.39.0` and can be adopted with the pinned Rust 1.90 toolchain.
