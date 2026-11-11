# WP1-B environment builder evidence — 2026-11-11

**Candidate:** WP1-B branch `feat/environment-builder`, baseline `05c78b2`,
Core interface `e9be02c`. The exact SHA is the commit containing this record.
**Security:** `TRUSTED_LOCAL`. Not isolation certification. No paid inference
was used.

This record separates what actually ran from what did not. The positive
environment build used the final `deploy/environment-builder/Containerfile`
image under the unique WP1-B tag
`localhost/software-factory-environment-builder:wp1b-340fa54-r3`
(`sha256:8f7693fcdc1298bf4cc96190631ece69c13561a14533d8006cb707ba8c867cd7`,
image ID `sha256:fc2f984c…`). It is a real rootless Podman image run with a
verified image identity, not a mock. The shared `:dev` tag belongs to the
WP1-C Compose bundle and was not overwritten by this evidence run.

## 1. Deterministic tests and static checks

| Check | Result |
|---|---|
| `python3 -m py_compile deploy/environment-builder/entrypoint.py deploy/environment-builder/environment_builder/*.py scripts/environment-builder-fixture.py scripts/environment-builder-path-policy.py scripts/environment-builder-disk-policy.py` | PASS |
| `bash -n scripts/environment-builder.sh` | PASS |
| `python3 -m unittest discover -s deploy/environment-builder/tests -v` | PASS — 31 tests |
| `git diff --check` | PASS |

Covered: secret-shaped file rejection, `builtins.exec`, unpinned fetch,
pinned fetch acceptance, unknown/denied license, unfree without/with exact
override, network policy, sandbox policy, resource bounds, argv checks, receipt
binding, failed receipt blockers, exact cgroup CPU/memory/PID bounds, fetch
network/proxy/allowlist enforcement, protected cache/workspace descendants,
symlink/`..` escapes, rootful-Podman rejection, disk-overflow success-artifact
purge, wrapper nonzero-exit cleanup, deterministic CycloneDX SBOM, transitive
`NOASSERTION`, SBOM-to-frozen-spec binding, and the Core-shaped license
inventory.

## 2. Positive Node+Playwright and Rust acceptance

Spec hash: `cb365859f1f8bcb7cadedfe8f3188d7aa57e0dd8768cfbcfbc82e79a7add7d85`.
Builder image: `localhost/software-factory-environment-builder:wp1b-340fa54-r3@sha256:8f7693fcdc1298bf4cc96190631ece69c13561a14533d8006cb707ba8c867cd7`.
This is the WP1-B builder-image evidence tag, not the shared `:dev` tag used by
the WP1-C Compose bundle. The final Compose integration must rebuild this same
`deploy/environment-builder/Containerfile` under its own digest and validate
that separately.
Store path: `/nix/store/c2nf1xssr9jvifvw55v4dpf1hnp1qk5g-devenv-shell-env`.
Closure entries: 481.

| Operation | Result |
|---|---|
| Final image build | `PASS`; 6.44 GB; Nix 2.28.3, devenv 1.11.2, Node 22.20.0, Chromium 143.0.7499.169 |
| Descriptor / self-test | `PASS`; `READY`; exact image digest |
| Offline build receipt | `SUCCEEDED`; `outputSha256=12dfb45e5adf93454e35c846472d8101d7a52eec0438aaa95e867636f7f7c23a`; 5 304 ms |
| Warm repeat build | `SUCCEEDED`; same spec and same `outputSha256`; 6 702 ms |
| Node + Playwright check (`npm test`, real Chromium) | `PASSED`; exit 0; 9 742 ms |
| Rust check (`cargo test --offline`) | `PASSED`; exit 0; 5 681 ms |
| Sandbox attestation | complete: non-root, no secrets, no host network/socket/mount, Nix sandbox, pure, bounded resources |
| Frozen SBOM hash | `5f2a9b5b3ec0d7dbfea7c890af8fbb0f1ef267b3f26bf6b0c0a195fb3521539e` |
| CycloneDX document hash | `0a587c2adf5e1446c8faa46240691ae82c750eb2d66cc604ded92aa35cc18945` |
| SBOM matches frozen spec | `true` |

The Node check log contains a real Playwright run: `1 passed` for
`tests/smoke.spec.mjs` with Chromium. The Rust check compiled and ran the
`wp0_rust_spike` unit test.

The exact included fetch artifact is `fetch-result.json` for spec
`cb365859…` with `fetch-request.json` (fetch duration 150 198 ms; environment digest `5bcadda9…`, closure manifest hash `d576d49f…`); it used the approved internal broker
network (`factory-egress-internal`) and a deny-by-default HTTP CONNECT proxy
that allowed only the frozen plan destinations (`cache.nixos.org`,
`devenv.cachix.org`, GitHub, npm, crates.io/rust static hosts). The build and
check phases ran with `--network none`.

The persistent store cache (`/nix` cache mount) was seeded from the verified
image once. This is why the warm build reuses the exact derivations and
`devenv-tasks`/source inputs without re-downloading. Nix signature
verification stayed enabled with the pinned `cache.nixos.org` and
`devenv.cachix.org` keys; there is no global `require-sigs=false` and no
unsigned local closure import.

Writable host paths are confined to an explicit cache base: the cache, run
directory and Nix store cache must resolve under it; `$HOME/.ssh`,
`$HOME/.gnupg`, `$HOME/.config`, container engine paths, `.git` segments,
`..`/symlink escapes and repository-root descendants are rejected. The
read-only workspace rejects the same protected descendants.
The builder runs with a read-only root filesystem and bounded tmpfs for
`/tmp`, `/run`, `/home/factory/.cache` and `/home/factory/.local`; `/cache`
and `/nix` are the only writable mounts. Disk is a post-hoc `du` bound against
the frozen `diskMiB`; `disk.exact=false` and the enforcement is recorded in
`disk-usage.json`. On overflow all success artifacts and PASSED check results
are removed from both run and output directories and only `disk-failure.json`
plus `disk-usage.json` remain. It is not claimed as an exact quota.

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
`run-request.json`, `fetch-request.json`/`fetch-result.json`, the CycloneDX
`sbom.cdx.json`, closure manifest, license inventory, final and warm-repeat receipts/evidence, `disk-usage.json`, raw
Node/Rust `check.log` files and the in-tree `egress-proxy.py` used for the run
(`sha256 66d31ec13fdf23ae35ba3eb75b31f1986104d47c037a1c1405f68329027d353d`).
`descriptor.json`, `self-test.json`, `image-inspect.json` and
`signature-config.txt` record the verified image identity and Nix signature
configuration. `build.log`, `node-check.log`, `rust-check.log` and
`egress-proxy.log` are bounded. These files allow the receipt hashes and
durations to be recomputed from the candidate. `MANIFEST.sha256` hashes every
evidence file except itself.
