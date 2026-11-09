# RUSTSEC-2026-0459 — accepted wait for a patched sqlx release

**Status:** owner-authorized temporary security blocker; tracked for upgrade
**Decision date:** 2026-11-09
**Owner decision:** wait for a new sqlx release that permits the patched
`libsqlite3-sys`, document the blocker, and merge the agentic-flow PR with the
audit check failing only on this advisory.
**Scope:** the local-first, TRUSTED_LOCAL developer-preview product. This is not
an isolation or certification claim.

## Advisory

- `RUSTSEC-2026-0459` / `CVE-2026-119375` / `GHSA-72w2-rjgg-q729`
- `libsqlite3-sys` 0.30.1, pulled by `sqlx-sqlite` 0.8.6 (the project's current
  sqlx line)
- Affected `libsqlite3-sys` 0.6.0–0.38.2; patched in 0.39.0 (SQLite 3.54.0) and
  0.39.1 (SQLite 3.54.1)
- The bundled SQLite can return uninitialized page-cache memory when a process
  opens a database together with a sibling `-wal` file whose page size differs.
  The attack vector is local and requires the victim to open the crafted pair.

## Why the project has not upgraded yet

- `sqlx-sqlite` 0.8.6 and 0.9.0 both declare
  `libsqlite3-sys >=0.30.1, <0.38.0`; Cargo cannot select 0.39.x.
- The current sqlx `main` uses `libsqlite3-sys 0.37.0`, which is still affected.
- The published `msqlx-sqlite 0.9.0-msqlx.2` fork keeps the same `<0.38.0` bound.
- `sqlx 0.9.0` requires Rust 1.94; the project pins Rust 1.90.0. Upgrading the
  toolchain would not fix the advisory by itself while sqlx keeps the old bound.
- Vendoring `sqlx-sqlite` with a relaxed bound is a possible future fix but was
  not authorized in this decision.

## Risk assessment for this product

- Core opens its own SQLite database under the Factory data directory. It does
  not open project repositories as SQLite databases.
- Exploitation requires a local actor able to write a sibling `-wal` file with a
  mismatched page size and a Factory process that later opens that exact pair.
- The product is TRUSTED_LOCAL; the host and its local users are already inside
  the trust boundary, and `SEC-01-ISOLATED` is out of scope under ADR-0035.
- The advisory is medium severity (CVSS 6.8). The decision is a temporary
  dependency risk, not a claim that the bundled SQLite is safe.

## Exit criteria

1. A sqlx release that allows `libsqlite3-sys >=0.39.0` (or an owner-approved
   vendor patch) is available.
2. The dependency is upgraded, `Cargo.lock` is refreshed, and
   `cargo audit --file Cargo.lock` is clean.
3. Full Rust, Office, architecture, parity and real smoke checks pass.
4. This document is updated to `RESOLVED` and the CI audit gate is green.

Until then, `cargo audit` is intentionally not suppressed. The `rust` and
`container-dev` CI jobs fail at that step; all other gates remain required. The
owner accepted the known failing check for the agentic-flow merge on
2026-11-09. Do not add a permanent audit ignore without a new owner decision.
