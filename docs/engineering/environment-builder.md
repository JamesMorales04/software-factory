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
not a default toolchain. Generated environments are flake-only (`flake.nix` plus
`flake.lock`); Core does not emit a `devenv.nix` without a matching pinned
devenv input. A repository that already carries a coherent
`devenv.nix`/`devenv.yaml`/`devenv.lock` remains a supported project-committed
environment. After the builder returns a resolution, Core merges the resolved
lock files and rebinds `resolution.planSha256` to the final plan hash; the
binding field is excluded from the plan content hash to avoid a circular hash,
while the resolved file hashes and builder identity remain covered.

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
| `fetch-result.json` | fetch-phase store path, environment digest and closure manifest hash |
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
   (credential-free HTTP(S) CONNECT proxy) on an explicitly selected broker
   network. Only `none`, `factory-egress-internal`, `factory-deps-egress` and
   their internal Compose-prefixed forms are accepted. The wrapper verifies
   with `podman network inspect` that the selected network is
   `internal=true`; `host`, `container`, `slirp4netns`, `pasta`, `bridge`,
   non-internal `factory-egress` and arbitrary networks are rejected. A
   non-`none` network requires the proxy; the exact plan allowlist is passed
   as `fetchPolicy` and the builder refuses a fetch whose broker policy is
   missing or differs from the plan. The fetch phase realises the environment
   and populates the verified persistent Nix store cache.
2. **build** — `--network none`, `subprocess` timeout, Nix
   `sandbox = true`, `sandbox-fallback = false`, `substituters =` empty and
   `--offline`. A missing prefetched path is `ENVIRONMENT_NETWORK_DENIED`, not a
   reason to enable network.

The wrapper mounts only the read-only project at `/inputs/workspace`, a
writable run cache at `/cache`, and a persistent Nix store cache at `/nix`.
Run directories and the store cache must canonicalize inside an explicit
cache base (`FACTORY_ENVIRONMENT_BUILDER_CACHE_BASE`, default
`$HOME/.cache/software-factory`). `$HOME/.ssh`, `$HOME/.gnupg`,
`$HOME/.config`, container engine paths, `.git` segments, `..`/symlink escapes,
repository-root descendants and protected host paths are rejected for cache,
store, run and workspace paths. The store cache is seeded

from the verified image once and lets the offline build/check reuse the exact
fetched derivations, sources and `devenv-tasks` inputs. Nix signature
verification stays enabled with the pinned `cache.nixos.org` and
`devenv.cachix.org` keys; the builder does not set global
`require-sigs=false` and does not import unsigned local closures.

Disk is enforced against the frozen `diskMiB` in two layers. The Python builder
reads the frozen `diskMiB`, samples `/cache`, `/nix` and the workspace with
`du -skc` during build/resolve/check subprocesses (default 10 s interval,
120 s per-scan timeout), and
kills the child with `ENVIRONMENT_RESOURCE_LIMIT` only after three consecutive
incomplete or over-budget samples; pre/post checks retry an incomplete scan up
to three times with a short backoff before failing closed. A `du` error, timeout, excessive stdout, missing
aggregate `total` or expected path row, malformed output or missing path is an
incomplete scan and also fails closed: the builder cannot prove the bound and
never treats a lower bound as success. `du` stderr is drained through a bounded
buffer while exit status remains authoritative. The host/CI wrapper
also checks before and after the run and removes success artifacts. An
over-budget run fails `ENVIRONMENT_RESOURCE_LIMIT`, writes a typed
`disk-failure.json` plus `disk-usage.json`, and the broker removes the per-spec
cache volume or quarantines it if removal cannot be confirmed. This is
`disk.exact=false`, not an exact filesystem quota; the evidence records the
observed usage and never claims exact disk attestation.

The builder root filesystem is read-only; only `/cache` and `/nix` are
writable mounts, with bounded tmpfs for `/tmp`, `/run` and the writable home
subdirectories. There is no writable container layer for build state.


Non-root execution uses `--userns=keep-id:uid=10001,gid=10001`,
`--user 10001:10001`, `--cap-drop ALL --cap-add SYS_ADMIN --cap-add
SYS_CHROOT` (Nix sandbox and chroot), `--security-opt unmask=ALL` (Nix's
`/proc` remount test), `no-new-privileges`, bounded
`--cpus/--memory/--pids-limit` and a wall-clock timeout exactly equal to the
frozen plan. There is no host socket, HOME, Podman socket, provider secret or
GitHub credential. Mount attestation validates the source for every accepted
target, including `/` and system mounts: `/cache`, `/nix` and
`/inputs/workspace` must be Podman-managed named-volume sources, while `/`,
`/proc`, `/sys`, cgroup, devpts, mqueue, tmpfs and container-runtime host-file
mounts must use known-safe pseudo/rootfs sources. The runtime host-file source
must normalize to exactly
`overlay-containers/<container-id>/userdata/<hosts|hostname|resolv.conf>`;
`..`/`.` components, other filenames and bare host paths are rejected. A
forbidden host path at any allowed target cannot produce `noHostMount=true`.
Generated and project-committed environment files may pin the open-source
`github:cachix/devenv` and `github:cachix/git-hooks.nix` inputs and may name
the public `devenv.cachix.org` cache as an optional performance input.
`cachix login`, `cachix authtoken`, `CACHIX_AUTH_TOKEN` and Devenv Cloud remain
`ENVIRONMENT_SANDBOX_DENIED`; no Cachix account or paid cache is required.

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

`check` requires a successful receipt bound to the exact spec hash. It reuses
the persistent store cache, re-realises the environment offline, computes the
closure digest and refuses execution if it differs from
`receipt.outputSha256`. The output digest includes the frozen SBOM hash, so a
different license inventory cannot silently run. A check is refused without a
plan/spec/receipt binding; there is no implicit Rust or generic verifier image.
Every check prepares a fresh `check-workspace` from the broker-supplied
candidate workspace and never reuses the build-phase `workspace` directory in
the per-spec cache; only the Nix store and other cache state persist. Check
stdout/stderr are streamed concurrently into fixed-size buffers (256 KiB per
stream) while the rest is drained and discarded, so noisy output cannot grow
process memory without bound; truncation is recorded in the typed result.

The sandbox attestation compares the observed cgroup memory/PID limits and
`cpu.max` quota/period directly against the frozen plan: there is no multiplier
or tolerance. The wrapper also refuses a request whose plan resources do not
equal the spec resource policy.

The wrapper host side is a development/CI harness. WP1-C places the same image
behind the runtime broker (`/v1/environments/resolve` and
`/v1/environments/build`) with the broker's exact image/digest allowlist, bounded
staging, internal fetch egress and no-secret/no-network limits. WP1-D adds the
typed `/v1/environments/check` route and wires Supervisor, Worker and Verifier
checks to `builder check`; see the
[environment runtime broker integration](../implementation/environment-runtime-broker.md).
A failed check now returns its real exit code, stdout, stderr and truncation to
the broker instead of collapsing into a builder error; the CLI decides PASSED or
FAILED from the frozen expected exit. The direct Podman verifier backend has no
builder-check route and refuses environment-bound checks. Core authorization,
risk, Gate and candidate acceptance are unchanged by this slice. A real Compose
environment-check remains **NOT_TESTED** and is not claimed.

## 6. Evidence boundary

`deploy/environment-builder/tests/` covers deterministic policy, receipt, SBOM,
exact cgroup limits, fetch-policy enforcement, protected path descendants,
rootful-Podman rejection, wrapper nonzero-exit cleanup, runtime disk-budget
enforcement, root/system mount-source attestation, bounded noisy-output capture
and disk-overflow artifact purging (61 tests). The
final `deploy/environment-builder/Containerfile` image was built and exercised
under the unique WP1-B evidence tag
`localhost/software-factory-environment-builder:wp1b-340fa54-r3`
(`sha256:8f7693fc…`): descriptor/self-test, typed failures, a real
Node+Playwright build/check, a real Rust build/check, a warm repeat with the
same output digest and a real-closure SBOM. The receipt is bound to the exact
image digest. The shared `:dev` tag is owned by the WP1-C Compose bundle; the
final Compose rebuild of the same Containerfile must be validated under its own
digest. The separate
`deploy/factory-browser/Containerfile` Chromium launch, registry publication,
Compose/runtime-broker invocation and independent acceptance remain
**NOT_TESTED**. See the evidence record for exact hashes and durations.
