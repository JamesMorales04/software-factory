# Rust editor setup

**Status:** implementation guidance for the pinned developer toolchain. It does
not certify a specific editor build, extension release or container image.

The repository pins Rust `1.90.0` in `rust-toolchain.toml` with `clippy`,
`rustfmt`, `rust-analyzer` and `rust-src`. The development container installs
the same components; both use the repository `Cargo.lock`.

## One-time setup

```sh
rustup toolchain install 1.90.0 --profile minimal \
  --component clippy,rustfmt,rust-analyzer,rust-src
rustup component add clippy rustfmt rust-analyzer rust-src --toolchain 1.90.0
rustup which rust-analyzer
rust-analyzer --version
```

`rustup which rust-analyzer` must print a path under the pinned 1.90.0
toolchain. `rust-src` provides standard-library source navigation.

## VS Code and Code OSS

1. Install the `rust-lang.rust-analyzer` extension. The committed development
   container declares it in `.devcontainer/devcontainer.json`.
2. Open the repository root, not a parent directory or a copied source tree.
3. The extension reads `rust-toolchain.toml`; do not add a sysroot or
   `rust-analyzer.cargo.targetDir` override.
4. To use the pinned component server explicitly, set
   `rust-analyzer.server.path` in your **user or workspace** settings to the
   output of `rustup which rust-analyzer`, then restart the language server.
   The extension's bundled server is also acceptable when its version and the
   toolchain are updated together.
5. Validate with `code --list-extensions --show-versions | grep rust-lang.rust-analyzer`
   (or the Code OSS equivalent) and `rust-analyzer --version`.

Do not commit absolute language-server paths, a global `settings.json`, or
host-specific editor configuration. The devcontainer extension list is the only
committed editor integration.

## Validation gates

After changing toolchain or editor configuration:

```sh
rustup component list --installed --toolchain 1.90.0
cargo +1.90.0 check --workspace --locked
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.90.0 test --workspace --locked
```

If a component is unavailable for the pinned release, do not substitute a newer
toolchain silently. Report it and update the pin with a tested, justified
change.

## Tested state (2026-10-26)

The committed development-container image was rebuilt and exercised locally:
Rust/Cargo 1.90.0, Clippy 0.1.90, rustfmt 1.8.0, rust-analyzer 1.90.0,
`rust-src`, Node 22.23.2, SQLx 0.8.6, `cargo check --workspace --locked`,
323 unit tests (`--lib --bins`) and `rust-analyzer analysis-stats .` completed.
Browser and bubblewrap integration tests are intentionally outside that image.
See the [developer environment acceptance](../evidence/dev-environment-2026-10-26.md).
No host-specific editor path is committed.
