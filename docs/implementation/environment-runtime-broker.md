# Environment runtime broker integration — WP1-C

**Status:** Implemented WP1-C broker/build slice — 2026-11-11. This is not
acceptance evidence and does not claim a real Nix/devenv build in the installed
Compose stack. An isolated Compose observation (`wp1c-e2e`) verified the bundle
digest, nested import, broker `READY` and Core `READY`; see the
[evidence record](../evidence/environment-runtime-broker-2026-11-11/README.md).
`TRUSTED_LOCAL`; a container is not isolation certification.

**Contract:** [environment plan](../contracts/environment-plan.md),
[ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md).
**Builder:** [containerized Nix/devenv builder](../engineering/environment-builder.md),
[WP1-B evidence](../evidence/environment-builder-2026-11-10/README.md).
**Security:** [container runtime](../security/container-runtime.md).

## 1. Problem and design

WP1-B produced a builder image and a host/CI wrapper. The installed product had no
path from Core to that image: `EnvironmentBuilder` was `UNAVAILABLE`, and the
host wrapper is not a product runtime. WP1-C adds one narrow typed path:

```text
Core (environment owner token)
  -> POST /v1/environments/resolve | /v1/environments/build
  -> runtime broker (dedicated nested Podman engine)
  -> environment-builder image@verified-identity
```

Core never supplies an argv, Podman flag, host path, socket, image outside the
dedicated allowlist, network name, proxy URL or environment variable.

### Broker operations

| Route | Input | Result |
| --- | --- | --- |
| `/v1/environments/resolve` | generated `EnvironmentPlan`, exact image/digest, bounded project-file set | `EnvironmentResolution` with `flake.lock`/`devenv.lock`, Nix/devenv versions and `planSha256` |
| `/v1/environments/build` | frozen `EnvironmentPlan` + `EnvironmentSpec`, exact image/digest, bounded project-file set, optional `FETCH` phase | typed `EnvironmentBuildReceipt` / fetch evidence |

The build operation runs a fetch container on the broker-owned internal
environment egress network when the plan allowlist is non-empty, then a
`--network=none` build container. A fetch failure is a typed error; there is no
fallback to the host wrapper, host Podman or a language image.

## 2. Image identity and allowlist

- `FACTORY_RUNTIME_ENVIRONMENT_BUILDER_IMAGES` is the dedicated builder image
  allowlist. A provider/supervisor/global Rust image is never accepted.
- The broker reads the exact content digest from
  `FACTORY_RUNTIME_ENVIRONMENT_BUILDER_DIGEST_FILE` (Compose:
  `/bundles/environment-builder.digest`) or the explicit `_DIGEST` variable.
- The bundle service computes `sha256:<rootfs-tar>` and writes it next to
  `environment-builder-rootfs.tar`; the broker recomputes the bundle SHA-256
  before `podman import` and labels the imported image with that exact digest.
- Every request must match the configured image and digest. The broker inspects
  the imported image ID and refuses a changed identity. Generic execution by
  `builder-image@digest` is mapped to the verified imported image ID; an
  unrelated `rust@sha256:...` reference is rejected.
- Core reads the same digest file and installs the builder only when the broker
  health projection reports `READY` for that exact image/digest. Missing config
  leaves the builder `UNAVAILABLE`; partial broker/token configuration is a
  startup warning and never a fake `READY`.

## 3. Sandbox, resources and network

The builder container is started by the broker with:

- `--pull=never`, exact imported image ID;
- no host network, socket, bind mount, HOME, SSH agent or credential;
- read-only root, bounded `/tmp` and `/run`, `no-new-privileges`, `cap-drop=all`
  plus only `SYS_ADMIN`/`SYS_CHROOT` and `unmask=ALL` required by Nix sandboxing;
- non-root `10001:10001`;
- exact frozen plan `cpu`, `memoryMiB`, `pids` and wall-clock timeout (no
  hardcoded superset);
- one ephemeral staging volume for the bounded project file set and request;
- one owner-scoped cache volume keyed by the immutable spec hash for the fetched
  store, so the later offline build/check does not refetch;
- `--network=none` for build/evaluation. Fetch/resolve uses only a
  broker-created internal network and a per-operation credential-free CONNECT
  proxy with the exact plan allowlist. A missing proxy/network/allowlist mapping
  fails closed.

Ephemeral containers and staging volumes are removed on success, error, timeout
and client cancellation; cleanup confirmation is required before the response
reports success. The per-spec cache is the only persistent broker resource and
is owner-labelled, never a host path.

## 4. Core client

`RuntimeBrokerEnvironmentBuilder` in `native/src/planning/environment.rs`
implements the existing `EnvironmentBuilder` port:

- `resolve(plan, workspace)` and `build(spec, plan, workspace)` call the broker
  with a bounded file set derived from the exact plan files; the host workspace
  path is never sent.
- The client maps broker error codes back to `EnvironmentBlockerCode` and
  preserves `PIN`, `LICENSE`, `SANDBOX`, `RESOURCE`, `HASH` and `NETWORK` codes.
- `resolve` is used before freeze; `build` before persisting the frozen spec.
  The returned resolution/receipt is validated against the exact plan/spec hash,
  image, digest, platform, SBOM and sandbox attestation by WP1-A validation.
- Core startup reads `FACTORY_RUNTIME_BROKER_URL`,
  `FACTORY_RUNTIME_BROKER_TOKEN_FILE`, `FACTORY_ENVIRONMENT_BUILDER_IMAGE` and
  `FACTORY_ENVIRONMENT_BUILDER_DIGEST`/`_DIGEST_FILE`. If they are absent the
  truthful `UNAVAILABLE` builder remains.

## 5. Compose topology

`compose.yaml` adds:

- `environment-builder-bundle`: builds the WP1-B image and writes
  `environment-builder-rootfs.tar`, `environment-builder.digest` and `.ready`
  into the shared `factory-image-bundle` volume, with no network;
- runtime broker config for the builder allowlist, bundle, digest file, platform
  and policy revision;
- Core config for the broker URL, the distinct
  `runtime-broker-environment.token`, builder image and digest file.

`compose_init` generates a fourth distinct runtime-broker owner token for the
environment. The builder bundle is not attached to any credential network; the
broker's environment egress network is created inside the nested engine only for
fetch/resolve.

## 6. Health and diagnostics

- Broker `/health` includes `environmentBuilder` with `status`
  (`READY`/`UNAVAILABLE`/`IMPORTING`), image, digest, platform, policy revision
  and a bounded reason. No token or credential is exposed.
- Core's builder descriptor exposes image/digest/platform/policy/status in the
  environment projection and `/health`. Broker reachability or a mismatched
  image/digest keeps the descriptor `UNAVAILABLE`.

## 7. Tests performed

- `cargo fmt --all -- --check`
- `cargo clippy -p factory-native --all-targets --all-features -- -D warnings`
- `cargo test -p factory-native --lib` — 942 passed, 4 ignored.
- `cargo test -p factory-native --bin factory-core` — 65 passed.
- `cargo test -p factory-native --bin factory-worker` — 29 passed.
- `cargo test -p factory-native --bin factory-verifier` — 9 passed.
- `cargo test -p factory-native --bin factory-runtime-broker` — 53 passed,
  including owner separation, exact bundle digest/allowlist, tampered bundle,
  symlink/oversize workspace rejection, cache eviction, resource/network policy,
  narrow container args, fake builder success, resolve, timeout/cleanup,
  dropped-request cleanup and concurrent owner isolation.
- `python3 -m unittest discover -s deploy/environment-builder/tests` — 34 passed.
- Office `npm ci`, typecheck, lint and tests — 50 files / 353 tests passed.
- `podman compose -f compose.yaml config --quiet` — exit 0.
- `factory-architecture-check --root .` — PASS.
- Isolated Compose observation (`podman compose -p wp1c-e2e2`, source commit
  `4bc082f`): bundle tar digest `sha256:efc5bc…`, runtime broker health
  `environmentBuilder.status=READY`, imported builder image ID
  `sha256:152191…`, Core health `READY` with the same image/digest and
  `defaultNixpkgs=github:NixOS/nixpkgs/ac62194c…`. The live
  `software-factory` project was not modified. See the
  [evidence record](../evidence/environment-runtime-broker-2026-11-11/README.md).

**NOT_TESTED / remaining gap:** a real end-to-end `resolve`/`build` through the
broker (not the host wrapper) with a real project plan/spec producing an
`EnvironmentBuildReceipt` with SBOM hash and warm repeat. Frozen Worker/Verifier
check execution is **not wired** to a typed builder-check route: the existing
generic `supervisor_run` path remains, so a frozen check still does not realize
the Nix/devenv environment through `builder check`. That is the WP1-D gap; no
check-execution claim is made here.
