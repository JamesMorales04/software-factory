# Environment runtime broker integration — WP1-C

**Status:** Implemented WP1-C broker/build slice and WP1-D frozen check execution
— 2026-11-11. This is not acceptance evidence and does not claim a real
Nix/devenv build or check in the installed Compose stack. An isolated Compose
observation (`wp1c-e2e`) verified the bundle digest, nested import, broker
`READY` and Core `READY`; see the
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
| `/v1/environments/check` | frozen plan/spec/receipt binding, exact image/digest, one `EnvironmentCheckSpec`, caller-owned workspace volume | typed `EnvironmentCheckResult` |

Core merges the resolved lock files into the plan and rebinds
`resolution.planSha256` to the final plan hash before Lead review. The binding
field is excluded from the plan content hash to avoid a circular hash; the
resolved file hashes, versions and builder identity remain covered. A plan
whose resolution binding does not match its final hash is rejected by
`EnvironmentPlan::validate`.

The build operation runs a fetch container on the broker-owned internal
environment egress network when the plan allowlist is non-empty, then a
`--network=none` build container. A fetch failure is a typed error; there is no
fallback to the host wrapper, host Podman or a language image. After a
successful build, the broker keeps the build container and asks Podman for its
authoritative mount record: exactly the three expected named volumes with their
names, targets and read/write modes, and the runtime host files of that exact
container id. Only then does it replace the builder's in-container
`noHostMount` observation with the broker attestation; the container is
removed after inspection. An abrupt broker stop is reconciled at startup by
removing leftover containers labelled as environment-owned.

### Frozen check execution (WP1-D)

`/v1/environments/check` is the only path that realizes a frozen environment for
a Worker or Verifier check. It accepts an authenticated Environment, Supervisor
or Verifier owner token (never Provider) and exactly:

- the configured builder image and digest;
- the frozen `EnvironmentPlan`, `EnvironmentSpec` and successful
  `EnvironmentBuildReceipt`;
- one bounded `EnvironmentCheckSpec` (`checkId`, `argv`, `cwd`, `timeoutSecs`,
  `expectExit`);
- one broker-managed `factory-ws-*` volume owned by the authenticated owner.

The broker validates the plan/spec/receipt identity, builder identity, check
bounds and volume ownership before creating a container. It mounts the
candidate workspace read-only at `/inputs/workspace`, the persistent per-spec
cache read-write at `/cache`, the verified Nix store at `/nix`, and runs the
builder image as:

```text
check --request /cache/request.json --workspace /inputs/workspace --output /cache
```

The container uses `--network=none`, a read-only root, bounded tmpfs, only the
Nix-required `SYS_ADMIN`/`SYS_CHROOT` capabilities after `cap-drop=all`, the
exact frozen plan CPU/memory/PIDs and the frozen check timeout. No host path,
socket, secret, arbitrary image or client argv reaches Podman. The broker reads
the typed `check-result.json`, returns the exit code, expected exit, pass/fail,
bounded stdout/stderr and truncation, and confirms container cleanup while
retaining the bounded per-spec cache. The builder prepares a fresh
`check-workspace` from the candidate volume on every check; the build-phase
`workspace` directory in the shared cache is never reused as candidate source.
A timeout, cleanup failure, malformed result, hash mismatch or builder refusal
is a typed `ENVIRONMENT_*` error; the broker never fabricates `PASSED`.

Worker and Verifier integration:

- the Worker sends the typed plan/spec/receipt/check through
  `POST /v1/workspaces/{workspace_id}/environment-check` on the Supervisor. The
  Supervisor authenticates the workspace capability and a `run` operation grant
  whose frozen argv/timeout must match, **materializes the exact current host
  worktree into the broker workspace volume**, resolves the volume from its
  durable `WorkspaceManager` record, verifies the configured builder
  image/digest and calls the broker with its own token. Allocation creates an
  empty volume and only generic run/dependency operations refresh it, so a
  frozen check that is the first operation (or that follows `write_files`)
  must stage the worktree itself; otherwise it runs against an empty or stale
  workspace. A missing or stale Campaign environment binding fails before
  execution; there is no generic image fallback.
- every `EnvironmentCheckSpec` carries the human `SpecCheck` description. The
  builder echoes it into `check-result.json` together with one redacted,
  bounded stderr line; the broker returns a bounded
  `description`/`diagnostic` in `EnvironmentCheckResult`, and the Worker/Verifier
  show it even when a failed check writes no output.
- the verifier stages its clean candidate worktree into its own broker volume and
  calls the broker check route with the verifier token. Its direct Podman/local
  development backend has no builder-check route and refuses rather than running
  the generic verifier image.
- check receipts keep the exact check id, exit code, expected exit, passed
  state, stdout, stderr and truncation from the builder result.

### Workspace staging and failed-check diagnostics

A live campaign exposed two coupled defects. First, the Supervisor's
environment-check route used the recorded workspace volume without staging the
current worktree. Allocation creates an empty volume and only generic
`run`/dependency calls refresh it, so a frozen `npm test` that was the first
operation ran in a check workspace containing only generated environment files.
Real npm reported `ENOENT ... package.json` and exited 254; the same check
against the plain project or a properly staged Git worktree passed. The route
now calls `WorkspaceManager::stage_workspace_for_check`, which snapshots the
exact worktree (excluding `.git`, matching the archive contract) into the
volume under the manager lock before the broker call.

Second, a failing check could return an exit code with no stderr, leaving the
Implementer unable to tell what failed. The check description is now part of
`EnvironmentCheckSpec`, the builder result and `EnvironmentCheckResult`; the
broker produces a bounded `diagnostic` containing the description, exit code,
expected exit and at most one redacted stderr line. The Worker tool result,
automatic-final summary and Verifier failure all surface it. Source bodies and
full logs are never copied into the diagnostic.

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
is owner-labelled, never a host path. If the builder reports
`ENVIRONMENT_RESOURCE_LIMIT`, the broker removes that per-spec cache volume
after container cleanup so an over-budget cache cannot be reused. If removal
cannot be confirmed, the broker records the cache in a durable quarantine
ledger and an immediate in-memory block; every later build/check for that cache
fails closed until it is cleared. If the ledger cannot be read or written, all
environment cache reuse is denied until the broker restarts. At broker startup a
cache-epoch sweep evicts every previous `factory-env-cache-*` volume and
prunes the ledger, so a failed ledger write cannot permit reuse after a restart;
the ledger has a hard entry bound. Disk enforcement uses complete `du -skc`
sampling plus `disk.exact=false`; it is not an exact filesystem quota, and a
`du` error, missing path or incomplete/malformed result fails closed rather
than claiming the bound is satisfied.

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
  `runtime-broker-environment.token`, builder image and digest file;
- Supervisor config for the exact builder image and read-only digest file. The
  Supervisor resolves the workspace volume from its own record; the Worker never
  supplies it.

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
- `cargo test -p factory-native --lib -- --test-threads=1` — 946 passed,
  4 ignored.
- `cargo test -p factory-native --bin factory-core` — 65 passed.
- `cargo test -p factory-native --bin factory-worker` — 34 passed, including
  the frozen builder-check path, description propagation, bounded
  description-first tool diagnostics, missing Campaign binding refusal, failure
  exit-code mapping, timeout refusal and exact request binding.
- `cargo test -p factory-native --bin factory-verifier` — 11 passed, including
  the broker builder-check path, description binding, refusal, cleanup and
  missing binding.
- `cargo test -p factory-native --bin factory-runtime-broker` — 72 passed,
  including environment-check owner separation, workspace ownership, exact
  builder identity, description binding, plan/spec/receipt mismatch,
  argv/timeout bounds, success/failure exit mapping, bounded diagnostics,
  fabricated-PASS refusal, resource-limit cache purge/quarantine, bundle
  digest/cache bounds, narrow container arguments and timeout cleanup.
- `cargo test -p factory-native --bin factory-supervisor` — 12 passed, including
  grant/image/workspace refusal, successful typed proxy, and a real Git
  worktree check that fails with npm exit 254 when the workspace volume is
  empty and passes after `stage_workspace_for_check`.
- `python3 -W error::ResourceWarning -m unittest discover -s deploy/environment-builder/tests`
  — 69 passed, including description bounds, failed-check
  exit-code/stdout/stderr preservation, bounded redacted stderr tail,
  requested-cwd propagation, typed timeout, offline npm cache, proof that a
  check uses the candidate workspace rather than the build cache, over-budget
  build/check termination, unreadable-scan fail-closed behavior, mount-source
  attestation for root/system targets, epoch-sweep restart safety, bounded
  noisy-output capture, pipe cleanup and disk failure evidence.
- `TMPDIR=… bash scripts/environment-check-reproduce.sh` — PASS in the real
  builder image: empty workspace volume → npm exit 254; Git worktree and plain
  project → exit 0. See the
  [staging evidence](../evidence/environment-check-staging-2026-11-11/README.md).
- `podman compose -f compose.yaml config --quiet` — exit 0.
- `factory-architecture-check --root .` — PASS.
- Isolated Compose observation (`podman compose -p wp1c-e2e2`, source commit
  `4bc082f`): bundle tar digest `sha256:efc5bc…`, runtime broker health
  `environmentBuilder.status=READY`, imported builder image ID
  `sha256:152191…`, Core health `READY` with the same image/digest and
  `defaultNixpkgs=github:NixOS/nixpkgs/ac62194c…`. The live
  `software-factory` project was not modified. See the
  [evidence record](../evidence/environment-runtime-broker-2026-11-11/README.md).

**NOT_TESTED / remaining gap:** a real end-to-end `check` through the broker
against a running Compose stack with a real frozen spec/receipt and a real
Node/Rust workspace has not been run for WP1-D. The check route, Supervisor
proxy, Worker and Verifier wiring are exercised with deterministic unit/fixture
tests only; those tests do not claim a real Nix/devenv environment-check result.
The live campaign's `npm test` exit 254 was reproduced against the real builder
image and real npm for an unstaged workspace, and the staging regression is
covered by a real Git worktree plus archive import. A full live Compose rerun
with the v16 environment remains open. The earlier WP1-C `resolve`/`build`
end-to-end gap also remains.
