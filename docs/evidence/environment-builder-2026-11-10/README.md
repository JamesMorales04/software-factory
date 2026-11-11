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

Spec hash: `0630003b7be75e3b54ce9cbf0d3099f284abd093b874c80588bb5c3db236bdff`.
Builder image: `localhost/software-factory-environment-builder:dev@sha256:3407adf8c56cf090fe18fc3801388a603adfdaa45d347e841063f886cfc6d1da`.
Store path: `/nix/store/wyrgjl56z5nyq18clgnj1fkqr6vlvbzj-devenv-shell-env`.
Closure entries: 483.

| Operation | Result |
|---|---|
| Final image build | `PASS`; 6.44 GB; Nix 2.28.3, devenv 1.11.2, Node 22.20.0, Chromium 143.0.7499.169 |
| Descriptor / self-test | `PASS`; `READY`; exact image digest |
| Offline build receipt | `SUCCEEDED`; `outputSha256=43cf7804da91ec0c09360d11b0af40bc730c734520e7c46c7e24259f1704ebb1`; 4 749 ms |
| Warm repeat build | `SUCCEEDED`; same spec and same `outputSha256`; 4 733 ms |
| Node + Playwright check (`npm test`, real Chromium) | `PASSED`; exit 0; 6 425 ms |
| Rust check (`cargo test --offline`) | `PASSED`; exit 0; 5 697 ms |
| Sandbox attestation | complete: non-root, no secrets, no host network/socket/mount, Nix sandbox, pure, bounded resources |
| Frozen SBOM hash | `5f2a9b5b3ec0d7dbfea7c890af8fbb0f1ef267b3f26bf6b0c0a195fb3521539e` |
| CycloneDX document hash | `33f74497c4edc09d6a6c4132b6922a6c02693268352760cfe2e328787e7b8eb5` |
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
`devenv-tasks`/source inputs without re-downloading. Nix signature
verification stayed enabled with the pinned `cache.nixos.org` and
`devenv.cachix.org` keys; there is no global `require-sigs=false` and no
unsigned local closure import.

Writable host paths are confined to the cache root: the run directory and the
Nix store cache must resolve inside it, and the read-only workspace is rejected
if it is `/`, `$HOME`, the repository root, the container engine store or
another protected host path. Disk is a post-hoc `du` bound against the frozen
`diskMiB`; `disk.exact=false` and the enforcement is recorded in
`disk-usage.json`. It is not claimed as an exact quota.

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
| non-internal fetch network (e.g. `factory-egress`) | `ENVIRONMENT_NETWORK_DENIED`, exit 2 |
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
form. The directory also contains the exact `spec-plan-request.json` and
`run-request.json`, the CycloneDX `sbom.cdx.json`, closure manifest, license
inventory, final and warm-repeat receipts/evidence, `disk-usage.json`, raw
Node/Rust `check.log` files and the in-tree `egress-proxy.py` used for the run
(`sha256 66d31ec13fdf23ae35ba3eb75b31f1986104d47c037a1c1405f68329027d353d`).
`descriptor.json`, `self-test.json`, `image-inspect.json` and
`signature-config.txt` record the verified image identity and Nix signature
configuration. `build.log`, `node-check.log`, `rust-check.log` and
`egress-proxy.log` are bounded. These files allow the receipt hashes and
durations to be recomputed from the candidate. `MANIFEST.sha256` hashes every
evidence file except itself.
