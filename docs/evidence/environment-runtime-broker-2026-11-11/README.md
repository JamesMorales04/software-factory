# WP1-C environment runtime broker — Compose observation

**Observation source commit:** `4bc082f98e6898643e0379e4086107c962a55db1`
**Pre-rebase implementation candidate:** `5f98daec2b92e3adaee0b4f12720ed56a8d6ef58`
**Rebased code candidate:** `8611e3b0c1b49e4adc5456cb41f733ef0a30ad8c`
**Integration merge base:** `61ff424` (WP1-A `19e2723`, WP1-B `3325831`)
**Date:** 2026-11-11 (rebased 2026-11-12). **Status:** implementer self-test
observation, not independent acceptance. **Security:** `TRUSTED_LOCAL`; not
isolation certified.

The real `resolve`/`build` observation ran at `4bc082f`/`5f98daec`. The rebase
onto `a4591dac` retained the runtime-broker allowlist and Compose configuration
but dropped the earlier generic Supervisor image-allowlist change as superseded
by WP1-D's dedicated environment-check route. The recorded build path and hashes
were not rerun at `8611e3b0`; no rerun claim is made.

The live `software-factory` Compose project was **not** started, stopped,
recreated, migrated or modified. The observation used the isolated project name
`wp1c-e2e2` and host port `18894` for Core.

## What ran

```text
podman compose -p wp1c-e2e2 -f compose.yaml build environment-builder-bundle
podman compose -p wp1c-e2e2 -f compose.yaml up --no-deps \
  --abort-on-container-exit --exit-code-from environment-builder-bundle \
  environment-builder-bundle
podman compose -p wp1c-e2e2 -f compose.yaml up -d init \
  environment-builder-bundle provider-bundle dependency-bundle
podman compose -p wp1c-e2e2 -f compose.yaml build runtime
podman compose -p wp1c-e2e2 -f compose.yaml up -d runtime
FACTORY_PORT=18894 podman compose -p wp1c-e2e2 -f compose.yaml up -d core
```

The selected Compose services used isolated volumes and project-scoped container
names. No live volume or database was touched.

## Observed identities

| Item | Value |
| --- | --- |
| Builder image (outer, final branch build) | `localhost/software-factory-environment-builder:dev` |
| Builder image ID | `sha256:2a787e84898d709aed27b264c6f340177148ab88e7f7718445b8d91a42e53759` |
| Builder bundle rootfs tar SHA-256 | `efc5bcd81e797f319a6473417257987050a1908140e605982131488e0ceae4bd` |
| Runtime imported builder image ID | `sha256:15219178beb79f7f9cc99fbc5af6796b2b789775b1105cd0581f4d860d4e5b31` |
| Runtime broker health builder status | `READY` |
| Core health builder status | `READY` with the same image/digest/platform/policy revision and `defaultNixpkgs=github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d` |

The broker recomputed the bundle tar SHA-256 before import, compared it to the
sidecar after prefix normalization, and failed closed until the tar and sidecar
were consistent. It did not pull the builder tag. Core read the same digest file
and installed the broker builder only after the broker reported `READY`.

## Files

- `broker-health.json` — isolated runtime broker `/health` (`environmentBuilder.status=READY`) with the candidate source commit.
- `core-health.json` — isolated Core `/health` with the same builder descriptor and default nixpkgs pin.
- `environment-builder.digest` — exact bundle digest.
- `MANIFEST.sha256` — hashes of these observations.

## Verification matrix at this source commit

- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p factory-native --all-targets --all-features -- -D warnings` — clean.
- `cargo test -p factory-native --lib` — 942 passed, 4 ignored.
- `cargo test --bin factory-core` — 65 passed.
- `cargo test --bin factory-worker` — 29 passed.
- `cargo test --bin factory-verifier` — 9 passed.
- `cargo test --bin factory-runtime-broker` — 53 passed.
- Builder Python tests — 34 passed.
- Office `npm ci`, `npm run typecheck`, `npm run lint`, `npm test -- --run` — 50 files / 353 tests passed.
- `podman compose -f compose.yaml config --quiet` — exit 0.
- `factory-architecture-check --root .` — PASS; parity JSON parses.

## Real resolve/build through the Compose runtime broker

The follow-up run used the same isolated-Compose approach but executed the real
typed broker operations with a real Nix/devenv plan. The exact request and
responses are recorded here; no host wrapper was used. The run used the
working tree that became `e72d362`; the subsequent allowlist/Supervisor-check
hardening is covered by unit tests and does not change the builder image,
receipt, hashes or build path recorded here.

| Item | Value |
| --- | --- |
| Compose project | `sfwp1c` (isolated; live `software-factory` untouched) |
| Builder bundle digest | `sha256:37957bf26a2231cda135c61172427c84d39445c7af7c28caf5b5286f2951fed0` |
| Imported nested-engine image ID | `sha256:82145b824fcd7cc81433394bf2bdc146960bb8b6f74456cdf23005a045ebb9c5` |
| Nix / devenv | `2.28.3` / `1.11.2` |
| Plan SHA-256 | `38f126a75ef61de30e4685c321d040c7ed56441b457c41d65cc6b57081879304` |
| Spec SHA-256 | `78f95db05b774e2a01a0b08779c0613a4123760806c7d313b13ea8d0372cd9f0` |
| SBOM SHA-256 | `537f08653402fe3c1fb43cbf0c69036933ea4dedfcba2708dba733de65ec0d43` |
| Output SHA-256 | `80c3d40cade99a48cd9c130a8e08fc098bf208cff66ff80762007006c022fa9b` |
| Store path | `/nix/store/dmn963h1k16293ip4d7v0wh80wp7izrp-nix-shell-env` |

The plan/spec hashes were recomputed with
`EnvironmentPlan::compute_sha256` and `EnvironmentSpec::compute_sha256` from the
exact request JSON before the build. `real-build-request.json` is that request.

Results:

- `resolve` returned `phase=RESOLVE`, `cleanupConfirmed=true`, the exact
  `flake.lock`, Nix `2.28.3`/devenv `1.11.2` and `planSha256` equal to the plan
  hash.
- `build` returned `phase=BUILD`, `cleanupConfirmed=true` and a `SUCCEEDED`
  `EnvironmentBuildReceipt` bound to the exact spec hash, builder image, builder
  digest, frozen SBOM hash, content-addressed output hash and a complete sandbox
  attestation (`nonRoot`, `noSecrets`, `noHostNetwork`, `noHostSocket`,
  `noHostMount`, `sandbox`, `boundedResources`, `impure=false`).
- The warm repeat produced the same output and SBOM hashes, proving store/cache
  reuse; its request and response are recorded separately.
- The fetch phase ran on the broker-created internal network with the exact
  plan allowlist; the build phase ran `--network=none`.
- Core started in the same isolated project and `/health` reported the builder
  `READY` with the same image/digest.

## Rebase verification at `8611e3b0`

The branch was rebased onto `a4591dac` and the earlier generic Supervisor
image-allowlist change was dropped as superseded by WP1-D's dedicated
environment-check route. The following checks ran at the rebased code candidate:

- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p factory-native --all-targets --all-features --locked -- -D warnings` — clean.
- `cargo test -p factory-native --lib --locked` — 1000 passed, 4 ignored.
- `cargo test -p factory-native --bin factory-core --locked` — 70 passed.
- `cargo test -p factory-native --bin factory-worker --locked` — 36 passed.
- `cargo test -p factory-native --bin factory-verifier --locked` — 11 passed.
- `cargo test -p factory-native --bin factory-runtime-broker --locked` — 77 passed.
- `cargo test -p factory-native --bin factory-supervisor --locked` — 14 passed.
- `cargo test --workspace --locked --features test-support` with the CI
  bwrap/provider skips — 1398 passed, 0 failed, 4 ignored.
- `cargo audit --file Cargo.lock` — exit 0 after selecting patched
  `libsqlite3-sys` 0.39.1; `cargo deny check licenses bans sources` — PASS.
- `python3 -W error::ResourceWarning -m unittest discover -s deploy/environment-builder/tests`
  — 87 passed.
- `FACTORY_REAL_PODMAN_TESTS=1 cargo test -p factory-native --test
  real_podman_environment_check_staging` — 1 passed.
- `podman compose -f compose.yaml config --quiet` and with `compose.dev.yaml`
  — exit 0.
- `factory-architecture-check --root .` — PASS; parity JSON parses.

The real Compose `resolve`/`build` observation was **not** rerun at this
candidate. Its request/response hashes remain bound to `4bc082f`/`5f98daec`;
this record does not claim a new real build at `8611e3b0`.

## NOT_TESTED

- Worker/Verifier frozen check execution through `builder check` is implemented
  by WP1-D but was not exercised in this WP1-C observation; the real Compose
  environment-check remains NOT_TESTED and is not re-claimed here.
- Full Core intake -> Lead review -> freeze -> builder as a live model-driven
  Campaign. Core startup installation and health were exercised; the typed
  client plus error mapping and plan/spec binding are unit-tested.
- Independent acceptance, Office browser acceptance and isolation certification.
