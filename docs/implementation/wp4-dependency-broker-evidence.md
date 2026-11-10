# WP4 — dependency broker HTTP/Compose wiring evidence

**Status:** Implemented at the broker/Supervisor/Worker boundary; independently
unverified. This document records implementer evidence only. It is not
acceptance, certification, or a merged/delivered claim.

**Implementation commits:**
`e9dc41bf12888cff69cd7316d5e0a8a3ec29c46e` (initial broker wiring),
`8882b8ea3af3476d72df7df9c7ca5702f26ef9df` (lockfile source validation),
`b43dab41b1109a4019ccd6881a262d54a4182405` (worker dispatch, egress proxy,
workspace synchronization, image pinning), and
`21b84f57b9ae4b48caf4aab9aaaf239e2c8400bd` (internal-network enforcement and
pre-authorization dependency-plan attachment). The final evidence-doc commit is
the branch HEAD after this file.

Branch: `factory/quality-wp4-dependency-broker`, baseline `bcb8b5b`.

## Scope

Wire dependency acquisition into a real bounded HTTP/Compose execution path
without weakening the offline check container:

- `POST /v1/dependencies/acquire` on the trusted runtime broker.
- A dedicated credential-free dependency image and Compose bundle/import path.
- A deny-by-default internal network and CONNECT proxy for acquisition egress.
- An ephemeral, allowlisted acquisition container that runs
  `cargo fetch --locked` / `cargo vendor --locked` and writes SBOM, license and
  advisory evidence.
- Atomic promotion/rollback of the vendor tree and its evidence.
- A typed Rust client, `VolumeBackend` operation, Supervisor route,
  Core-authorized operation grant, Worker dispatch and explicit blockers.
- The check container remains `--network=none` and uses `--offline` commands.

## Changed files

```text
 compose.yaml
 deploy/Containerfile
 deploy/runtime-entrypoint.sh
 docs/operations/compose-deployment.md
 migrations/0065_pre_authorization_dependency_plan.sql (new)
 native/Cargo.toml
 native/src/bin/core.rs
 native/src/bin/dependency_acquire.rs          (new)
 native/src/bin/dependency_egress_proxy.rs     (new)
 native/src/bin/runtime_broker.rs
 native/src/bin/supervisor.rs
 native/src/bin/verifier.rs
 native/src/bin/worker.rs
 native/src/db/authorization.rs
 native/src/db/intake.rs
 native/src/db/lead.rs
 native/src/db/mod.rs
 native/src/db/projects.rs
 native/src/db/tests.rs
 native/src/db/workers.rs
 native/src/dependencies.rs
 native/src/dependency_broker.rs
 native/src/dependency_egress.rs               (new)
 native/src/dependency_service.rs              (new)
 native/src/execution.rs
 native/src/lead.rs
 native/src/lib.rs
 native/src/runtime_broker.rs
 native/tests/dependency_acquire_live.rs       (new, opt-in)
 native/tests/dependency_broker_http.rs        (new)
 native/tests/dependency_broker_podman.rs      (new, opt-in)
```

Shared-file registrations changed in this branch (call out for the integration
owner): `native/Cargo.toml` (new bins), `compose.yaml` (new `dependency-bundle`
service and runtime egress env), `native/src/bin/core.rs` and
`native/src/db/workers.rs` (allow the `dependencies` workspace operation, treat
it as a long-lived fenced grant, persist the blocker reason),
`native/src/db/mod.rs` (`ActionSpec.dependency_plan`, `WorkerReport.reason`),
and `native/src/lib.rs` (new modules).

## Runtime-broker operation

`POST /v1/dependencies/acquire` accepts a strict `deny_unknown_fields` JSON body:

```json
{
  "volume": "factory-ws-<uuid>",
  "container": "factory-deps-<uuid>",
  "workdir": ".",
  "lockfilePath": "Cargo.lock",
  "lockfileSha256": "<64 hex>",
  "vendorPath": "vendor",
  "registry": "sparse+https://index.crates.io/",
  "timeoutSecs": 600
}
```

Enforcement:

- Constant-time bearer authentication using the existing runtime-broker token.
- 64 KiB JSON body limit; 1 MiB bounded container stdout/stderr; 256 KiB
  response bound in the client.
- Strict volume/container identities; safe workspace-relative workdir,
  lockfile and vendor paths; no absolute paths, `..`, quotes, control bytes or
  drive prefixes.
- Exactly one dependency image configured by the broker; no caller-selected
  image, Podman flag, host path or environment.
- Registry must match the configured logical allowlist; every registry
  `source` recorded in `Cargo.lock` must also be allowlisted, and git sources
  are rejected because the acquisition path has no owner-approval channel.
- Timeout must be `1..=900` seconds.
- The receipt must bind the requested lockfile digest, the configured image,
  the exact evidence paths under `<vendor>/.factory-supply-chain/`, and SHA-256
  digests for the vendor tree, SBOM, licenses and advisory inventory.
- The broker logs only the container, volume, image and host/port decisions;
  never the bearer token, request body, container environment, payloads or
  full container output.

## Network egress boundary

The acquisition container is attached **only** to the dedicated internal
Podman network `factory-deps-egress` (default `10.90.0.0/24`, gateway
`10.90.0.1`). Internal networks have no default route or external NAT, so
direct egress is unavailable.

When the network already exists, the broker inspects the full JSON definition
and requires all of the following before accepting it:

- `internal: true` — a matching gateway/subnet on a non-internal network is
  rejected, so an existing network cannot silently enable direct egress;
- the `io.software-factory.managed=true` label — an unmanaged network is
  rejected;
- the exact configured gateway and subnet.

Failure of any check aborts broker startup. A regression test covers an
existing non-internal network with the expected gateway/subnet, a missing
managed label, and a mismatched gateway. Operators upgrading from an earlier
local network without the label must remove and recreate it; the broker refuses
to adopt an unmanaged network.

The runtime broker runs the deny-by-default HTTPS CONNECT proxy in-process on
the network gateway (`10.90.0.1:18080` by default). The proxy:

- accepts CONNECT only for the fixed allowlist
  (`index.crates.io:443`, `static.crates.io:443`, `github.com:443`,
  `codeload.github.com:443` by default);
- refuses everything else with `403`;
- supports optional pinned host-to-IP resolution so a resolver answer cannot
  change the destination;
- bounds the CONNECT request at 8 KiB, connections at 16, connect at 10 s and
  each tunnel at 300 s;
- exposes only a public `/health` probe, logs host/port decisions, and never
  inspects or logs payload bytes;
- carries no credentials.

The acquisition container receives only a fixed environment:
`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `CARGO_HTTP_PROXY` point at the
proxy; `NO_PROXY` is empty. It has no other route. Broker startup fails closed
when the dependency image is configured but the internal network cannot be
created/inspected or the proxy cannot bind. The acquirer refuses when the proxy
readiness flag is false, and the container cannot bypass the proxy because
there is no external route even if Cargo ignored the proxy variables.

The check container is unchanged: `--network=none`, `cargo build/test
--locked --offline`. Acquisition is a separate network-enabled step and does
not execute project build scripts with credentials.

## Image pinning

The broker imports the dependency bundle into the dedicated engine, inspects
the imported image config ID, normalizes it to `sha256:<64 hex>`, and passes it
to the acquirer. Before every acquisition the acquirer re-inspects the image
and refuses to run on a missing or different ID. The import path also records
the bundle SHA-256 label and reuses an existing image only when the label
matches. A mismatched pinned ID fails closed before any container starts.

## Atomic acquisition and evidence

`native/src/dependency_broker.rs` stages `cargo fetch`, `cargo vendor` and
evidence under `.factory-deps-<uuid>` inside the workspace. The producer writes
`sbom.cdx.json`, `licenses.json` and `advisories.json` into
`<staging-vendor>/.factory-supply-chain/` **before** promotion, then a single
rename publishes the whole vendor tree (sources plus evidence). The previous
vendor tree is renamed to a backup and restored if promotion fails; staging is
removed on every failure path.

Evidence content:

- SBOM: deterministic minimal CycloneDX 1.5 JSON derived from `Cargo.lock`.
- License report: package inventory with the license declared by each vendored
  `Cargo.toml` (`license` or `license-file`, `UNKNOWN` when absent).
- Advisory report: deterministic package inventory with
  `"status": "NOT_CHECKED"` and `"database": "not-configured"`. **No advisory
  database or `cargo audit`/`cargo deny` verdict is produced in this slice.**

## Worker dispatch and blockers

- `ActionSpec` carries an optional frozen
  `crate::dependencies::DependencyPlan` and a `dependencies_required` flag;
  `ActionSpecInput` and the operator `NewAction` API accept a plan, validate it
  and bind it into the stored spec hash.
- The planning route converts the frozen planning dependency decision into an
  execution plan and attaches it to the ActionSpec before plan review and
  Campaign authorization. Migration `0065` permits that one pre-authorization
  ActionSpec update only while no Campaign exists; the spec is immutable after
  Campaign start and the migration keeps that invariant in the database.
  - Existing Rust repository with a lockfile and no committed `vendor/`:
    `BrokeredFetch`, with the exact base-commit `Cargo.lock` SHA-256 obtained
    from `git show <base>:Cargo.lock`.
  - Existing Rust repository with a committed `vendor/`: `VendorCache`.
  - Rust repository with neither: `VendorCache` (a committed cache is
    required); offline checks remain the enforcement point.
  - Non-Rust/not-required: no plan.
  - If planning cannot attach the plan, the route refuses with
    `DEPENDENCY_PLAN_INVALID`/`DEPENDENCY_PLAN_REQUIRED`.
- Core refuses authorization when `dependencies_required` is true and the plan
  is absent (`DEPENDENCY_PLAN_REQUIRED` in `load_intake_approval_context`) and
  refuses assignment dispatch with the same invariant in `load_assignment`.
- The Worker independently refuses an assignment that requires dependencies but
  has no plan before any HTTP call, then derives a typed acquisition request
  from the plan's `BrokeredFetch` contract and calls the Supervisor route with a
  Core-signed `dependencies` operation grant. The request digest binds every
  field.
- Acquisition runs lazily after provider writes and before the first offline
  check and before finalization, in the OpenCode tool loop, the Codex tool
  loop, the patch flow and the error-after-writes finalization path.
- A failed acquisition, an invalid receipt or an evidence mismatch returns
  `dependency acquisition blocked the candidate: ...` and never reaches
  finalize. The Worker terminal report carries a bounded `reason`; Core
  persists it in the run status event, the Lead failure reason and the
  progress-hold reason. Receipt success is recorded as a durable provider tool
  event.
- The apply-only provider path (`allowed_tools` empty) refuses a brokered plan
  because it cannot acquire before its combined apply/finalize.
- Tests: a fixture assignment with `dependencies_required = true` and no plan
  is blocked before any HTTP call; a no-dependency assignment proceeds to
  finalize without calling `/dependencies`; the planner conversion emits
  VendorCache/BrokeredFetch plans and rejects brokered plans without a frozen
  lockfile digest; the DB setter attaches the plan before a Campaign and
  rejects it after.

## Workspace synchronization

`WorkspaceManager::acquire_dependencies` uses a per-workspace async lock. The
global state lock is held only for record transitions and the final local
worktree replacement; snapshot, volume import, broker acquisition, export and
extraction happen outside it. The record is revalidated after the broker
returns and again after export, and cancellation is checked before publishing.
Other workspaces can acquire concurrently; a concurrency test uses a barrier
backend to prove both acquisitions reach the backend without a global-lock
serialization.

## Verification run

All commands were run from `native/` unless noted, on
`21b84f57b9ae4b48caf4aab9aaaf239e2c8400bd`.

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib
753 passed; 0 failed; 2 ignored

cargo test -p factory-native --lib dependency_
24 passed; 0 failed (includes egress proxy, image pinning, per-workspace
concurrency, internal-network enforcement, plan conversion, pre-authorization
attachment and missing-plan blockers)

cargo test -p factory-native --test dependency_broker_http
6 passed; 0 failed

cargo test -p factory-native --test dependency_acquire_live
1 passed (explicit SKIPPED without FACTORY_LIVE_DEPENDENCY_FETCH=1)

FACTORY_LIVE_DEPENDENCY_FETCH=1 \
  cargo test -p factory-native --test dependency_acquire_live
1 passed; real crates.io fetch of `itoa = "=1.0.17"`; lockfile source
allowlist enforced; SBOM/license/advisory files produced

cargo test -p factory-native --test dependency_broker_podman
1 passed (explicit SKIPPED without FACTORY_PODMAN_DEPENDENCY_TEST=1)

FACTORY_PODMAN_DEPENDENCY_TEST=1 \
FACTORY_DEPENDENCY_TEST_IMAGE=localhost/software-factory-dependency:dev \
  cargo test -p factory-native --test dependency_broker_podman
1 passed; real rootless Podman, internal network, in-container proxy, direct
egress denied, non-allowlisted CONNECT denied, allowlisted crates.io CONNECT
allowed, pinned-image mismatch blocked before execution, acquisition receipt
and promoted evidence verified, container/volume/network cleanup verified

cargo test -p factory-native --bin factory-worker
17 passed; includes acquisition-before-finalize, failed-acquisition blocking,
missing-plan blocking and no-dependency-proceeds tests

cargo test -p factory-native --bin factory-supervisor \
  --bin factory-verifier --bin factory-runtime-broker \
  --bin factory-dependency-acquire
8 + 7 + 8 + 3 passed; 0 failed (runtime broker includes the existing
non-internal/unmanaged network regression test)

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean

podman compose config
exit 0

podman build --target dependency \
  -t localhost/software-factory-dependency:dev -f deploy/Containerfile .
exit 0; image 73bc369fbd600e77deaea9342af5914c0f0a5d431dee0495324ba2cbb9890a5b
```

The opt-in network test uses the real dependency image and a host rootless
Podman. It creates an internal network with no default route, starts the
standalone egress proxy as a real container on that network plus the default
network, verifies direct egress fails, verifies a non-allowlisted CONNECT gets
`403`, verifies an allowlisted `index.crates.io` CONNECT succeeds, verifies a
wrong pinned image ID returns `502` before execution, then runs the production
acquirer against a workspace volume and checks the promoted SBOM/license/
advisory evidence and cleanup.

## Authority and remaining integration

- Core remains the authority. The Supervisor route requires the workspace
  capability and a Core-signed `dependencies` operation grant whose request
  digest binds every acquisition field. Core accepts the operation only for a
  leased `LEASED` attempt on an `AUTHORIZED` Campaign with an active provider
  session, and issues a long-lived fenced grant.
- `RuntimeBrokerClient::acquire_dependencies` is the typed client.
- `WorkspaceManager::acquire_dependencies` round-trips the workspace so the
  vendor tree and evidence survive later check snapshots.
- The runtime broker `/health` reports whether acquisition is configured and
  that the dependency network is proxy-only.

Remaining integration work for the integration owner:

1. Exercise the real intake planning route end-to-end (assessment -> planning ->
   plan review -> authorization) with a dependency-bearing repository and a
   real Worker; the unit/DB tests prove conversion and attachment, not the full
   browser/Compose journey.
2. Candidate scope must explicitly allow `vendor/` and
   `vendor/.factory-supply-chain/` or treat them as generated artifacts.
   `discard_outside_scope` is not applied to acquisition output.
3. Add Office plan-review and diagnostics projection for the dependency
   acquisition strategy, evidence, blocker and receipt. Office parity impact
   for this slice is `none` because the change is an internal execution
   boundary, but the plan-review surface is still required by WP10.
4. Migration `0065` is a shared migration-owner file. It permits the single
   pre-Campaign ActionSpec dependency-plan update; review it in the next
   migration wave.

## Network and credential boundary

- Acquisition container: internal Podman network only, proxy-only egress,
  credential-free, fixed image/argv, named workspace volume.
- Check container: `--network=none`, offline Cargo commands.
- Broker: no Podman socket exposure, no host networking, no privileged mode, no
  arbitrary Podman flags, no caller-supplied image or environment.
- Registry allowlisting is enforced logically (request, lockfile sources) and at
  the network boundary by the CONNECT proxy. Direct egress has no route.
- The acquisition container runs `cargo fetch`/`cargo vendor`, which parse
  manifests and fetch/vendor sources. It does not invoke project build scripts
  or run untrusted project code with credentials.
- The runtime-broker token remains shared execution authority for the Worker,
  Supervisor, verifier and runtime broker; the reviewer does not receive it.

## Not tested / blocked / limitations

- `NOT_TESTED`: the full nested engine startup path (bundle import, internal
  network creation and proxy bind inside the Compose runtime container) and a
  Worker-to-Core-to-nested-broker acquisition. The actual `dependency` image
  built successfully; the acquirer, proxy and network boundary were exercised
  with real host rootless Podman.
- `NOT_TESTED`: the full browser/Compose intake planning and plan-review
  journey. The planner conversion, pre-authorization DB attachment, Core
  blocker and Worker blocker are covered by focused tests.
- `NOT_IMPLEMENTED`: pinned advisory database, `cargo audit`/`cargo deny`
  verdicts and license-policy enforcement.
- `LIMITATION`: a greenfield Rust project with neither a committed `Cargo.lock`
  nor a `vendor/` tree is planned as `VendorCache`; its offline checks will fail
  until a cache is committed or the owner selects a brokered strategy. This is
  fail-closed, not a silent skip.
- `LIMITATION`: an existing `factory-deps-egress` network without the
  `io.software-factory.managed=true` label is refused; operators must remove
  and recreate it.
- `NOT_TESTED`: Windows/Podman Machine, SELinux/AppArmor hosts, registry digest
  pinning (the imported image config ID is pinned per broker process; Compose
  still uses a local `:dev` tag for the bundle).
- `BLOCKED` for any acceptance/certification claim: independent verification,
  independent security review and Product Owner acceptance have not run.
