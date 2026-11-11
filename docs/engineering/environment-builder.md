# Containerized Nix/devenv environment builder

**Status:** WP1-B builder slice — 2026-11-10. Implements the builder side of
[ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md)
and the [environment plan contract](../contracts/environment-plan.md). This is
not acceptance evidence and does not claim a completed cold Node/Playwright or
Rust build; see the [WP1-B evidence record](../evidence/environment-builder-2026-11-10/README.md)
for what was and was not exercised.

**Core interface:** `native/src/planning/environment.rs` at WP1-A commit
`e9be02c` (`EnvironmentBuilder`, `EnvironmentBuildReceipt`,
`EnvironmentBuilderDescriptor`, `CheckExecutionTarget`).
**Security:** `TRUSTED_LOCAL`. The builder is not isolation certification.

## 1. One generic builder

`deploy/environment-builder/Containerfile` builds one image for every project.
It contains:

- pinned Nix `2.28.3` from `docker.io/nixos/nix:2.28.3@sha256:d078d715…`;
- pinned `nixpkgs` `ac62194c…` (nixos-25.05) for the image toolchain;
- devenv `1.11.2`, Node 22, npm, Rust, Python 3, Git, GCC, Make, pkg-config,
  coreutils, sbomnix and common system tools;
- Chromium from the same pinned `playwright-driver.browsers` as the browser
  runner;
- the fixed Factory `playwright-core` 1.64.1 runner.

There is no per-language catalog and no language-image selection. A project
environment is whatever its frozen `devenv.nix`/`flake.nix` and lock files
declare. The image tools are a warm cache and the path for the builder entrypoint,
not a default toolchain.

`deploy/factory-browser/Containerfile` builds a separate Chromium/Playwright
runner. It contains the `factory-browser` binary, Node, `playwright-core` and
Chromium, but not a project toolchain. Project Node/Playwright dependencies come
from the frozen `EnvironmentPlan`; the browser image is for the Factory-owned
browser check.

## 2. Interface

The image entrypoint is `/opt/factory-environment-builder/entrypoint.py`:

```text
factory-environment-builder descriptor
factory-environment-builder self-test
factory-environment-builder build --request - --phase fetch|build \
  --workspace /inputs/workspace --output /cache
factory-environment-builder check --request - \
  --workspace /inputs/workspace --output /cache
```

The request is:

```json
{
  "schemaVersion": 1,
  "spec": { "EnvironmentSpec": "…" },
  "plan": { "EnvironmentPlan": "…" },
  "check": { "checkId": "…", "argv": ["…"], "cwd": ".", "timeoutSecs": 300, "expectExit": 0 },
  "fetchProxy": "http://proxy.internal:18080"
}
```

`spec` and `plan` use the Core camelCase shape. The builder binds the
authoritative `spec.specSha256` verbatim, revalidates the plan/spec identity,
files, pins, licenses, sandbox, network, resources and secrets, and writes:

| File | Content |
|---|---|
| `result.json` | `EnvironmentBuildReceipt` (`SUCCEEDED`/`FAILED`) |
| `builder-evidence.json` | builder-local input digest, closure count, SBOM hash, attestation |
| `build.log` | bounded, redacted fetch/build log |
| `sbom.cdx.json` | deterministic CycloneDX 1.5 SBOM |
| `license-inventory.json` | Core-shaped `LicenseInventory` |
| `closure.json` | exact Nix closure paths, NAR hashes and sizes |
| `fetch-result.json` | fetch-phase store path and closure digest |
| `check-result.json` | one check result |

`descriptor` reads `FACTORY_ENVIRONMENT_BUILDER_IMAGE`,
`FACTORY_ENVIRONMENT_BUILDER_DIGEST`, `FACTORY_ENVIRONMENT_BUILDER_PLATFORM`,
`FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION` and the optional
`FACTORY_ENVIRONMENT_NIX_VERSION`/`FACTORY_ENVIRONMENT_DEVENV_VERSION`. An
absent image or digest is `UNAVAILABLE`, matching Core; it is never replaced by
a language image.

**Integration note:** Core's `DEFAULT_ENVIRONMENT_DEVENV_VERSION` at `e9be02c`
is `1.10.0`, while this image pins devenv `1.11.2`. Core must set
`FACTORY_ENVIRONMENT_NIX_VERSION=2.28.3` and
`FACTORY_ENVIRONMENT_DEVENV_VERSION=1.11.2` when freezing, or the image must be
rebuilt against the exact frozen versions. The builder fails
`ENVIRONMENT_HASH_MISMATCH` rather than lying about its version. Nix `2.28.3`
matches Core's default.

## 3. Cache and offline strategy

`scripts/environment-builder.sh` runs two phases:

1. **fetch** — network only through `FACTORY_ENVIRONMENT_BUILDER_FETCH_PROXY`
   (credential-free HTTP(S) CONNECT proxy) on an explicitly selected network.
   It realises the environment and exports the exact closure to
   `/cache/closures/<specSha256>.nar`.
2. **build** — `--network none`, `subprocess` timeout, Nix
   `sandbox = true`, `sandbox-fallback = false`, `substituters =` empty and
   `--offline`. A missing prefetched path is `ENVIRONMENT_NETWORK_DENIED`, not a
   reason to enable network.

The wrapper mounts only the project at `/inputs/workspace:ro` and a
writable cache at `/cache`. The build workspace, npm/devenv cache and closure
live in the cache. Non-root execution uses
`--userns=keep-id:uid=10001,gid=10001`, `--user 10001:10001`,
`--cap-drop ALL --cap-add SYS_ADMIN --cap-add SYS_CHROOT` (Nix sandbox and
chroot), `--security-opt unmask=ALL` (Nix's `/proc` remount test),
`no-new-privileges`, bounded `--cpus/--memory/--pids-limit` and a wall-clock
timeout. There is no host socket, HOME, Podman socket, provider secret or
GitHub credential.

## 4. Typed failures

Errors use the stable `EnvironmentBlockerCode` values. The builder emits a
failed receipt/check result rather than a partial success:

| Condition | Code |
|---|---|
| missing/invalid spec or plan fields | `ENVIRONMENT_SPEC_INVALID` |
| spec not `FROZEN` | `ENVIRONMENT_NOT_FROZEN` |
| image/digest/version/file hash mismatch | `ENVIRONMENT_HASH_MISMATCH` |
| absent image/digest/closure | `ENVIRONMENT_BUILDER_UNAVAILABLE` |
| no exact Nix input/lock pin | `ENVIRONMENT_PIN_MISSING` |
| `builtins.exec`, impure eval, host path/socket | `ENVIRONMENT_SANDBOX_DENIED` |
| secret-shaped file/env value | `ENVIRONMENT_SECRET_DETECTED` |
| build network not `NONE` or offline miss | `ENVIRONMENT_NETWORK_DENIED` |
| CPU/memory/PID/time outside policy | `ENVIRONMENT_RESOURCE_LIMIT` |
| Nix/devenv build failure | `ENVIRONMENT_BUILD_FAILED` |
| wall-clock exceeded | `ENVIRONMENT_BUILD_TIMEOUT` |
| denied/unknown/unfree license | `ENVIRONMENT_LICENSE_DENIED` / `ENVIRONMENT_LICENSE_UNKNOWN` / `ENVIRONMENT_UNFREE_REQUIRES_OWNER` |
| repaired environment differs from receipt | `ENVIRONMENT_HASH_MISMATCH` |

## 5. Check execution

`check` requires a successful receipt bound to the exact spec hash. It restores
the cached closure, re-realises the environment offline, computes the closure
digest and refuses execution if it differs from `receipt.outputSha256`. A check
is refused without a plan/spec/receipt binding; there is no implicit Rust or
generic verifier image.

The wrapper host side is a development/CI harness. Integration must place the
same image behind the runtime broker or an equivalent narrow execution service,
with the broker's image allowlist/digest and the same no-secret/no-network
limits. Core authorization, risk, Gate and candidate acceptance are unchanged by
this slice.

## 6. Evidence boundary

`deploy/environment-builder/tests/test_builder.py` covers deterministic policy,
receipt and SBOM behavior. The containerized descriptor, self-test and typed
failure paths were exercised against a locally derived builder image; the full
cold Node+Playwright/Rust environment build and browser launch are **NOT_TESTED**
in this slice because the closure fetch was interrupted by the time box. Do not
present the partial spike measurement as a builder receipt. See the evidence
record.
