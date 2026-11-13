# Vendored `sqlx-sqlite` 0.8.6 dependency-bound patch

**Source:** `sqlx-sqlite` 0.8.6 from crates.io
(`c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`).
**Scope:** manifest-only change. No Rust source file is modified.

## Change

`libsqlite3-sys` dependency:

```diff
-version = "0.30.1"
+version = ">=0.30.1, <0.41.0"
```

## Reason

`RUSTSEC-2026-0459` / `CVE-2026-119375` affects bundled SQLite in
`libsqlite3-sys` through 0.38.2 and is fixed in 0.39.0/0.39.1. Both
`sqlx-sqlite` 0.8.6 and 0.9.0 pin `libsqlite3-sys <0.38.0`, so Cargo cannot
select the patched release. The upstream `sqlx` main branch raised the bound
for its unreleased 0.9.x line; 0.9.0 itself still requires Rust 1.94 while this
project pins Rust 1.90.

This vendor patch changes only the manifest bound. The `sqlx-sqlite` 0.8.6
source remains byte-for-byte the published crate. The patched build selects
`libsqlite3-sys` 0.39.1 and is checked by `cargo audit`.

## Removal condition

Remove this directory and the `[patch.crates-io]` entry when a published
`sqlx-sqlite` release permits `libsqlite3-sys >=0.39.0` and the workspace can
upgrade to it without changing the pinned Rust toolchain.
