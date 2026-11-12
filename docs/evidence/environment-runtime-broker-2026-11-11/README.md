# WP1-C environment runtime broker — Compose observation

**Candidate source commit:** `4bc082f98e6898643e0379e4086107c962a55db1`
**Integration merge base:** `61ff424` (WP1-A `19e2723`, WP1-B `3325831`)
**Date:** 2026-11-11. **Status:** implementer self-test observation, not
independent acceptance. **Security:** `TRUSTED_LOCAL`; not isolation certified.

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

## NOT_TESTED

- A real `resolve` or `build` through the broker with a real project
  `EnvironmentPlan`/`EnvironmentSpec` producing an `EnvironmentBuildReceipt`
  with SBOM hash and warm repeat. The host/CI WP1-B evidence is not a substitute.
- Worker/Verifier frozen check execution through `builder check`; that is the
  WP1-D gap and is not wired or claimed.
- Independent acceptance, Office browser acceptance and isolation certification.
