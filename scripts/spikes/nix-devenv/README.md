# WP0 Nix/devenv environment-compiler spike

**Status:** spike only. This directory is not part of the installed Factory
runtime, does not replace the canonical Compose development image, and is not
certification of Nix, devenv, Playwright or the Factory. Recorded evidence:
[`docs/evidence/nix-devenv-spike-2026-11-10.md`](../../../docs/evidence/nix-devenv-spike-2026-11-10.md).

The spike proves that a pinned Nix flake can materialize:

- Node 22.23.4, npm 10.9.9 and Playwright 1.64.1 with the matching Chromium
  browser revision; and
- Rust 1.90.0, Cargo 1.90.0 and Clippy 0.1.90;

inside rootless Podman with no host Nix, Node, npm, Cargo or Playwright
installation. It also records a devenv configuration and lock, but the flake is
the fully executed path in the recorded evidence.

## Prerequisites

- Rootless Podman with a working user namespace. `run.sh` verifies
  `podman info --format '{{.Host.Security.Rootless}}'` is `true` and fails
  otherwise.
- Network access for the first (cold) run. The first run fetches the pinned
  flake inputs and binary cache paths.
- No host Nix, Node, npm, Cargo, Playwright, `mise`, `asdf` or `devenv`.

The container uses Podman named volumes for `/nix` and `/cache`. `run.sh`
rejects `NIX_VOLUME`/`CACHE_VOLUME` values that contain `/` or start with `.`,
so a host path cannot be passed as a bind mount. `NETWORK` accepts only
`bridge` or `none`; `host`, `container:` and custom networks are rejected. The
container uses no host Podman socket, privileged mode or added Linux
capability.

## Commands

Run from this directory:

```sh
# Build the spike image and run both trivial projects (warm if volumes exist).
./run.sh all

# Remove the Nix store and cache volumes, then run cold.
RESET=1 ./run.sh all

# One project at a time.
./run.sh node
./run.sh rust

# Warm run with the network disabled. Requires a completed cold run.
OFFLINE=1 NETWORK=none ./run.sh all

# Generate CycloneDX/CSV SBOMs into the cache volume.
./run.sh sbom
podman volume inspect wp0-spike-cache \
  --format '{{.Mountpoint}}/sbom'

# Validate the wrapper's boundary checks without building or running.
CHECK_ONLY=1 ./run.sh

# Record image, volume, closure and cached-rebuild measurements.
./measure.sh
```

The wrapper builds `localhost/wp0-nix-devenv-spike:2026-11-10` from the local
`Containerfile`. Override `IMAGE`, `NIX_VOLUME`, `CACHE_VOLUME`, `NETWORK`,
`OFFLINE` or `RESET` with environment variables if needed. Volume overrides
must be named Podman volumes; network overrides must be `bridge` or `none`.

## Layout

| Path | Purpose |
|---|---|
| `Containerfile` | Pinned Nix base image plus spike sources and entrypoint |
| `flake.nix`, `flake.lock` | Pinned nixpkgs and rust-overlay inputs; `node`, `rust`, `tools` devShells |
| `devenv.nix`, `devenv.yaml`, `devenv.lock` | Equivalent devenv declaration and locked inputs |
| `projects/node` | Node 22 + npm + Playwright/Chromium trivial test and browser timing |
| `projects/rust` | Rust 1.90 trivial library test with a crates.io dependency |
| `run-spike.sh` | In-container entrypoint (`versions`, `node`, `rust`, `sbom`, `all`) |
| `run.sh` | Host wrapper using rootless Podman and named volumes, with boundary checks |
| `measure.sh` | Quick image/volume/closure/cached-rebuild measurement log |
| `evidence` | Cold/warm/offline logs, measurements, SBOMs, devenv info and summaries |

## Pins

| Input | Revision/version |
|---|---|
| Nix base image | `docker.io/nixos/nix:2.28.3` at `sha256:e623d73af9cac82d1b50784c83e0cf2a4b83bfd2cfe8d5b67809a2fc94e043ac` |
| nixpkgs | `4fe25e12eb9f4c744966dd3de5a05246cf098601` |
| rust-overlay | `89750a2ef1c6a44db9f4251b16c23deffdcfddff` |
| devenv modules (locked) | `cachix/devenv@4a1138b49c3b0503c214e4b6e59102dbdd3282ca` |
| Node | 22.23.4 from nixpkgs (the repository's `.nvmrc` pins 22.23.2) |
| Playwright | 1.64.1 from nixpkgs (matches the repository pin) |
| Rust | 1.90.0 from rust-overlay (matches `rust-toolchain.toml`) |

## Limits

- The test projects are deliberately trivial. The full Factory workspace,
  `cargo xtask check`, SQLx CLI, `cargo-audit`, bubblewrap-boundary tests,
  Office builds and the Compose topology are **not** exercised.
- Chromium runs with `--no-sandbox` because the spike container runs as
  (mapped) root. This is not a browser-sandbox claim.
- The nixpkgs package versions are revision-bound. Exact Node patch pinning
  requires a matching nixpkgs revision or a custom package/overlay.
- The devenv path was evaluated and locked, but a cold `devenv shell` activation
  did not finish within the spike budget; see the evidence document.
- License metadata was recorded for Nix, nixpkgs and devenv, but this spike
  does not determine legal compatibility. The generated SBOMs show GPL/LGPL
  components in the Linux userland and tool closures, and some components lack
  license entries or SPDX identifiers. Distribution obligations need a separate
  Factory license review and owner-approved overrides where metadata is
  missing.
- No paid service, Devenv Cloud account, Cachix account or paid inference was
  used. `devenv` uses the public `devenv.cachix.org` binary cache by default;
  that cache is optional and not a paid service.
