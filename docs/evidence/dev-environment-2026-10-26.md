# Developer environment acceptance (2026-10-26)

**Status:** **PASS** for the pinned Rust developer-container image and workspace
tooling. Browser-dependent integration tests and bubblewrap provider E2E tests
are **NOT_TESTED in the container** because Chromium and bubblewrap are
deliberately not part of the developer image; they passed on the host/CI lanes
recorded elsewhere.

## Image and toolchain

| Item | Value |
|---|---|
| Image | `localhost/factory-devcontainer-test:latest` |
| Image ID | `967acc53a3f057d20c5ab1e2a30df92ce4103a5d845002d58d662f8bc74c5471` |
| Built from | `.devcontainer/Containerfile` at the campaign branch |
| Rust | 1.90.0 (`1159e78c4`) |
| Cargo | 1.90.0 |
| Clippy | 0.1.90 |
| rustfmt | 1.8.0-stable |
| rust-analyzer | 1.90.0 (`1159e78`) |
| rust-src | present at `$(rustc --print sysroot)/lib/rustlib/src/rust/library` |
| Node / npm | 22.23.2 / 10.9.8 |
| SQLx CLI | 0.8.6 |
| Go | absent (intended) |

The image build ran the pinned toolchain, Node, SQLx and absence-of-Go checks
successfully.

## Workspace commands

Inside the image, with the repository mounted read-only and a fresh container
target directory:

```sh
cargo check --workspace --locked
cargo test --workspace --locked --lib --bins
rust-analyzer analysis-stats .
```

- `cargo check --workspace --locked`: PASS.
- `cargo test --workspace --locked --lib --bins`: PASS, 323 tests passed, 0
  failed, 2 ignored.
- `rust-analyzer analysis-stats .`: completed over 3,480 items in 48.36 s,
  including inference and MIR checks; no analysis failure was reported.
- The container has no host-specific editor paths; `.devcontainer` is
  repository-relative and mounts the workspace as `/workspaces/software-factory`.

## Deliberate container limits

The full integration suite is not claimed inside the developer image:

- `native/tests/browser.rs` returned `NOT_TESTED` because no pinned Chromium or
  `PLAYWRIGHT_BROWSERS_PATH` is present; the real-Chromium lane is a separate CI
  job and was exercised for the Office journey.
- `e2e_fixture_dispatch` provider cases exited 127 because `bwrap` is not
  installed in the developer image; the provider boundary tests require the
  dedicated bubblewrap installation used by CI and the host acceptance run.

These are environment-scope limits, not Rust compile or unit-test failures.
