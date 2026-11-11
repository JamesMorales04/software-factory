# WP1-C environment runtime broker — Compose observation

**Date:** 2026-11-11. **Status:** implementer self-test observation, not
independent acceptance. **Security:** `TRUSTED_LOCAL`; not isolation certified.

The live `software-factory` Compose project was **not** started, stopped,
recreated, migrated or modified. The observation used the isolated project name
`wp1c-e2e` and host port `18893` for Core.

## What ran

```text
podman compose -p wp1c-e2e -f compose.yaml build environment-builder-bundle
podman compose -p wp1c-e2e -f compose.yaml up --no-deps \
  --abort-on-container-exit --exit-code-from environment-builder-bundle \
  environment-builder-bundle
podman compose -p wp1c-e2e -f compose.yaml up -d init \
  environment-builder-bundle provider-bundle dependency-bundle
podman compose -p wp1c-e2e -f compose.yaml up -d runtime
FACTORY_PORT=18893 podman compose -p wp1c-e2e -f compose.yaml up -d core
```

The selected Compose services used isolated volumes and project-scoped container
names. No live volume or database was touched.

## Observed identities

| Item | Value |
| --- | --- |
| Builder image (outer, final branch build) | `localhost/software-factory-environment-builder:dev` |
| Builder image ID | `sha256:3cc0cbb902908883bf9d610aacb8888195d18676141fabadb3ef028c69847667` |
| Builder bundle rootfs tar SHA-256 | `58befb8c9c5477e44527d6e42a589bf93f8819ebfe512887982b236422f4fc29` |
| Runtime imported builder image ID | `sha256:9f8e05fff5d6a4591e354806667d6d8ec89b00aa7e3827b0f112fc03da8af1d3` |
| Runtime broker health builder status | `READY` |
| Core health builder status | `READY` with the same image/digest/platform/policy revision |

The broker did not pull the builder tag. The bundle service wrote the digest
next to the rootfs tar, the broker imported the tar, labelled the imported image
with that digest, and health reported the exact configured digest. Core read the
same digest file and installed the broker builder only after the broker reported
`READY`.

## Files

- `broker-health.json` — isolated runtime broker `/health` (`environmentBuilder.status=READY`).
- `core-health.json` — isolated Core `/health` with the same builder descriptor.
- `environment-builder.digest` — exact bundle digest.
- `MANIFEST.sha256` — hashes of these observations.

## NOT_TESTED

- A real `resolve` or `build` through the broker with a real project
  `EnvironmentPlan`/`EnvironmentSpec` producing an `EnvironmentBuildReceipt`
  with SBOM hash and warm repeat. The host/CI WP1-B evidence is not a substitute.
- Worker/Verifier frozen check execution through `builder check`; that is the
  WP1-D gap and is not wired or claimed.
- Independent acceptance, Office browser acceptance and isolation certification.
