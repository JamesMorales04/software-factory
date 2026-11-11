# WP1-B environment builder evidence — 2026-11-10

**Candidate:** WP1-B branch `feat/environment-builder`, baseline `05c78b2`, Core
interface `e9be02c` (WP1-A branch `feat/environment-compiler-core`).
**Security:** `TRUSTED_LOCAL`. Not isolation certification. No paid inference
was used.

This record separates what actually ran from what did not. A partial spike
measurement is not a builder receipt.

## 1. Deterministic tests

| Check | Result |
|---|---|
| `python3 -m py_compile deploy/environment-builder/entrypoint.py deploy/environment-builder/environment_builder/*.py` | PASS |
| `bash -n scripts/environment-builder.sh` | PASS |
| `python3 -m unittest discover -s deploy/environment-builder/tests -v` | PASS — 17 tests |

Covered: secret-shaped file rejection, `builtins.exec`, unpinned fetch,
pinned fetch acceptance, unknown/denied license, unfree without/with exact
override, network policy, sandbox policy, resource bounds, argv checks, receipt
binding, failed receipt blockers, deterministic CycloneDX SBOM, transitive
`NOASSERTION`, and the Core-shaped license inventory.

## 2. Containerized builder entrypoint

A local test image was derived from the already-present
`localhost/factory-builder-test:packages` image (Nix 2.28.3 + devenv 1.11.2 +
Node 22 + Python 3 + sbomnix) with the WP1-B entrypoint copied in. This proves
the Python entrypoint and descriptor path; it is **not** the
`deploy/environment-builder/Containerfile` image and is **not** a reproducible
builder release.

| Command | Result |
|---|---|
| `podman run … factory-environment-builder descriptor` | PASS — `READY`, `x86_64-linux`, image/digest supplied |
| `podman run … factory-environment-builder self-test` | PASS — Nix `2.28.3`, devenv `1.11.2`, schema 1 |

## 3. Typed failure evidence

The wrapper was run rootless with `--network none`, non-root UID 10001,
`keep-id`, bounded CPU/memory/PIDs, no host socket and a read-only project mount.

| Case | Result |
|---|---|
| missing image/digest | `ENVIRONMENT_BUILDER_UNAVAILABLE`, exit 2 |
| denied license (`AGPL-3.0-only`) | `ENVIRONMENT_LICENSE_DENIED`, exit 2 |
| plan build network `BROKER` | `ENVIRONMENT_NETWORK_DENIED`, exit 2 |
| CPU bound 64 | `ENVIRONMENT_RESOURCE_LIMIT`, exit 2 |
| offline build with an empty closure cache | `ENVIRONMENT_NETWORK_DENIED` (“the offline build needs a store path that the broker did not prefetch”), exit 2 |

The offline failure occurred after the spec/plan/files/pins/licenses/sandbox
validation passed. It demonstrates that the build phase does not silently enable
network or fall back to a language image.

## 4. Real Nix/devenv environment build

A spike run of the real `devenv shell` for the Node+Playwright and Rust
environment completed in about 155 seconds inside
`localhost/factory-builder-test:packages` before the WP1-B time box. That is a
developer observation only:

- no WP1-B `EnvironmentBuildReceipt` was emitted;
- the closure export was interrupted by a cache-permission/time-box issue;
- the full `deploy/environment-builder/Containerfile` image was not built;
- no offline Node+Playwright or Rust check ran against a builder receipt;
- Playwright Chromium was not launched through the WP1-B runner.

Therefore the following are **NOT_TESTED**:

- cold rootless Podman build of `deploy/environment-builder/Containerfile`;
- Node+Playwright environment build with a successful receipt;
- Rust environment build with a successful receipt;
- warm-cache repeat and stable `outputSha256`;
- Playwright Chromium launch through the builder/factory-browser images;
- SBOM digest from a real closure;
- license block with a real package set (only deterministic unit cases ran);
- Compose/runtime-broker integration.

## 5. Host-tool independence

The builder container used Nix, devenv, Node, Python and its project tools from
`/nix/store`; host `nix` and host `playwright` are absent on this host. Host
Node/Cargo were not invoked. The wrapper itself is a development/CI harness and
uses host `bash`/`podman`/`python3`; the installed product must invoke the same
image through the runtime broker, not the wrapper.

## 6. Artifacts

`summary.json` records the observed commands and statuses in machine-readable
form. Raw container output is bounded and was not committed to keep the
repository free of large logs.
