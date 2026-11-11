# WP1-B environment builder evidence — 2026-11-11

**Candidate:** WP1-B branch `feat/environment-builder`, baseline `05c78b2`,
Core interface `e9be02c`. The exact SHA is the commit containing this record.
**Security:** `TRUSTED_LOCAL`. Not isolation certification. No paid inference
was used.

This record separates what actually ran from what did not. The positive environment build used the final
`deploy/environment-builder/Containerfile` image
(`localhost/software-factory-environment-builder:dev`,
`sha256:c1f26ceccd45451ce6085fc7df9811bfdaef3e002300b14a71c5f93594815824`).
It is a real rootless Podman image run with a verified image identity, not a
mock.

## 1. Deterministic tests and static checks

| Check | Result |
|---|---|
| `python3 -m py_compile deploy/environment-builder/entrypoint.py deploy/environment-builder/environment_builder/*.py scripts/environment-builder-fixture.py` | PASS |
| `bash -n scripts/environment-builder.sh` | PASS |
| `python3 -m unittest discover -s deploy/environment-builder/tests -v` | PASS — 20 tests |
| `git diff --check` | PASS |

Covered: secret-shaped file rejection, `builtins.exec`, unpinned fetch,
pinned fetch acceptance, unknown/denied license, unfree without/with exact
override, network policy, sandbox policy, resource bounds, argv checks, receipt
binding, failed receipt blockers, exact cgroup CPU/memory/PID bounds, fetch
network/proxy/allowlist enforcement, deterministic CycloneDX SBOM, transitive
`NOASSERTION`, SBOM-to-frozen-spec binding, and the Core-shaped license
inventory.

## 2. Positive Node+Playwright and Rust acceptance

Spec hash: `c10481f8acdfacc19711a8b0b5b20c4a8704f748f2396cd2a3b08bf93a42677e`.
Builder image: `localhost/software-factory-environment-builder:dev@sha256:c1f26ceccd45451ce6085fc7df9811bfdaef3e002300b14a71c5f93594815824`.
Store path: `/nix/store/wyrgjl56z5nyq18clgnj1fkqr6vlvbzj-devenv-shell-env`.
Closure entries: 483.

| Operation | Result |
|---|---|
| Final image build | `PASS`; 6.44 GB; Nix 2.28.3, devenv 1.11.2, Node 22.20.0, Chromium 143.0.7499.169 |
| Descriptor / self-test | `PASS`; `READY`; exact image digest |
| Offline build receipt | `SUCCEEDED`; `outputSha256=4e79552f01aaac949d60cf1037ab7c0c7c63c66be2561f3655337cd1a1ba02d1`; 7 354 ms |
| Warm repeat build | `SUCCEEDED`; same spec and same `outputSha256`; 4 936 ms |
| Node + Playwright check (`npm test`, real Chromium) | `PASSED`; exit 0; 8 683 ms |
| Rust check (`cargo test --offline`) | `PASSED`; exit 0; 5 224 ms |
| Sandbox attestation | complete: non-root, no secrets, no host network/socket/mount, Nix sandbox, pure, bounded resources |
| Frozen SBOM hash | `5f2a9b5b3ec0d7dbfea7c890af8fbb0f1ef267b3f26bf6b0c0a195fb3521539e` |
| CycloneDX document hash | `b2a0f3903c05c5487d4ef6c6cadee32b11afea27b7a29d18c8569aff1a9d8402` |
| SBOM matches frozen spec | `true` |

The Node check log contains a real Playwright run: `1 passed` for
`tests/smoke.spec.mjs` with Chromium. The Rust check compiled and ran the
`wp0_rust_spike` unit test.

The environment was fetched through the approved internal broker network
(`factory-egress-internal`) and a deny-by-default HTTP CONNECT proxy that
allowed only the frozen plan destinations (`cache.nixos.org`,
`devenv.cachix.org`, GitHub, npm, crates.io/rust static hosts). The build and
check phases ran with `--network none`.

The persistent store cache (`/nix` cache mount) was seeded from the verified
image once. This is why the warm build reuses the exact derivations and
`devenv-tasks`/source inputs without re-downloading.

## 3. Typed failure evidence

Rootless Podman runs used non-root UID 10001, `keep-id`, bounded
CPU/memory/PIDs, no host socket and a read-only project mount.

| Case | Result |
|---|---|
| missing image/digest | `ENVIRONMENT_BUILDER_UNAVAILABLE`, exit 2 |
| configured digest not the inspected image | `ENVIRONMENT_HASH_MISMATCH`, exit 2 |
| denied license (`AGPL-3.0-only`) | `ENVIRONMENT_LICENSE_DENIED`, exit 2 |
| plan build network `BROKER` | `ENVIRONMENT_NETWORK_DENIED`, exit 2 |
| fetch network `host` | `ENVIRONMENT_NETWORK_DENIED`, exit 2 |
| non-`none` fetch network without proxy | `ENVIRONMENT_NETWORK_DENIED`, exit 2 |
| CPU bound 64 | `ENVIRONMENT_RESOURCE_LIMIT`, exit 2 |
| offline build with an empty store cache | `ENVIRONMENT_NETWORK_DENIED`, exit 2 |

## 4. Remaining NOT_TESTED

The final builder image built and the positive receipt is bound to its exact
digest. The following remain untested:

- `deploy/factory-browser/Containerfile` build and its Chromium launch (the
  builder image itself launched real Chromium through Playwright);
- registry publication and registry-manifest digest pinning;
- Compose/runtime-broker invocation of the builder image;
- a check under the product Core authorization/Gate path;
- independent verification or acceptance.

## 5. Core version coordination

The builder provides Nix `2.28.3` and devenv `1.11.2`. WP1-A's default devenv
version at `e9be02c` is `1.10.0`. Core must set
`FACTORY_ENVIRONMENT_NIX_VERSION=2.28.3` and
`FACTORY_ENVIRONMENT_DEVENV_VERSION=1.11.2` when freezing, or the builder must
be rebuilt against `1.10.0`. The builder fails
`ENVIRONMENT_HASH_MISMATCH` with a direct version message; it does not claim a
different version.

## 6. Host-tool independence

The builder container used Nix, devenv, Node, Python, Rust and Chromium from
the Nix store/shared cache; host `nix` and host `playwright` are absent on this
host. Host Node/Cargo were not invoked. The wrapper itself is a development/CI
harness and uses host `bash`/`podman`/`python3`; the installed product must
invoke the same image through the runtime broker, not the wrapper.

## 7. Artifacts

`summary.json` records the observed commands and statuses in machine-readable
form. `build-receipt.json`, `builder-evidence.json`, `node-check-result.json`,
`rust-check-result.json` and `sbom-digest.json` are the bounded artifacts from
the final-image run. Raw container logs are bounded and were not committed.
