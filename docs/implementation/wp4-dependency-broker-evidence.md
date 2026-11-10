# WP4 — dependency broker HTTP/Compose wiring evidence

**Status:** Implemented at the broker/Supervisor boundary; independently
unverified. This document records implementer evidence only. It is not
acceptance, certification, or a merged/delivered claim.

**Implementation candidate:** `e9dc41bf12888cff69cd7316d5e0a8a3ec29c46e`
(branch `factory/quality-wp4-dependency-broker`, baseline `bcb8b5b`).

## Scope

Wire the existing library-level dependency acquisition broker into a real
bounded HTTP/Compose execution path without weakening the offline check
container:

- `POST /v1/dependencies/acquire` on the trusted runtime broker.
- A dedicated credential-free dependency image and Compose bundle/import path.
- An ephemeral, network-enabled, allowlisted acquisition container that runs
  `cargo fetch --locked` / `cargo vendor --locked` and writes SBOM, license and
  advisory evidence.
- Atomic promotion/rollback of the vendor tree and its evidence.
- A typed Rust client, `VolumeBackend` operation, Supervisor route and
  Core-authorized operation grant.
- The check container remains `--network=none` and uses `--offline` commands.

## Changed files

```text
 compose.yaml
 deploy/Containerfile
 deploy/runtime-entrypoint.sh
 docs/operations/compose-deployment.md
 native/Cargo.toml
 native/src/bin/core.rs
 native/src/bin/dependency_acquire.rs        (new)
 native/src/bin/runtime_broker.rs
 native/src/bin/supervisor.rs
 native/src/bin/verifier.rs
 native/src/db/workers.rs
 native/src/dependencies.rs
 native/src/dependency_broker.rs
 native/src/dependency_service.rs            (new)
 native/src/execution.rs
 native/src/lib.rs
 native/src/runtime_broker.rs
 native/tests/dependency_acquire_live.rs     (new, opt-in)
 native/tests/dependency_broker_http.rs      (new)
 native/tests/dependency_broker_podman.rs    (new, opt-in)
```

Shared-file registrations changed in this branch (call out for the integration
owner): `native/Cargo.toml` (new bin), `compose.yaml` (new `dependency-bundle`
service and runtime env), `native/src/bin/core.rs` and
`native/src/db/workers.rs` (allow the `dependencies` workspace operation and
treat it as a long-lived fenced grant), and `native/src/lib.rs` (new module).

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
- 64 KiB JSON body limit; 1 MiB bounded container stdout/stderr.
- Strict volume/container identities; safe workspace-relative workdir,
  lockfile and vendor paths; no absolute paths, `..`, quotes, control bytes or
  drive prefixes.
- Exactly one dependency image configured by the broker; no caller-selected
  image, Podman flag, host path or environment.
- Registry must match the configured logical allowlist.
- Timeout must be `1..=900` seconds.
- The receipt must bind the requested lockfile digest, the configured image,
  the exact evidence paths under `<vendor>/.factory-supply-chain/`, and SHA-256
  digests for the vendor tree, SBOM, licenses and advisory inventory.
- The broker logs only the container and volume identities, never the bearer
  token, request body, container environment or container output.

The production acquirer runs:

```text
podman run --rm --name=factory-deps-<uuid>
  --label=io.software-factory.managed=true
  --label=io.software-factory.workspace=factory-ws-<uuid>
  --label=io.software-factory.dependency=acquire
  --pull=never --network=bridge --read-only
  --tmpfs=/tmp:rw,noexec,nosuid,size=1g
  --tmpfs=/cargo-home:rw,nodev,nosuid,size=4g
  --cap-drop=all --security-opt=no-new-privileges
  --ulimit=fsize=67108864 --pids-limit=256 --memory=4g --cpus=2
  --workdir=/workspace
  --env=HOME=... --env=TMPDIR=...
  --env=CARGO_HOME=/cargo-home --env=CARGO_NET_OFFLINE=false
  --env=CARGO_TERM_COLOR=never --env=RUSTUP_HOME=/usr/local/rustup
  --env=FACTORY_DEPENDENCY_IMAGE=<configured image>
  --volume=factory-ws-<uuid>:/workspace:rw
  <image> --workdir . --lockfile Cargo.lock --lockfile-sha256 <sha>
          --vendor vendor --registry <allowlisted>
```

The only forwarded host process values are non-secret `HOME`/`TMPDIR`; the
child environment is otherwise a fixed allowlist. No provider, registry, Core,
GitHub or operator credential is passed. No Podman socket, host network,
privileged mode or arbitrary host mount is used.

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

The check container is unchanged: `--network=none`, `cargo build/test
--locked --offline`. Acquisition is a separate network-enabled step and does
not execute project build scripts with credentials.

## Verification run

All commands were run from `native/` unless noted, on the implementation
candidate above.

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib
742 passed; 0 failed; 2 ignored

cargo test -p factory-native --lib dependency_
14 passed; 0 failed; 0 ignored

cargo test -p factory-native --test dependency_broker_http
6 passed; 0 failed

cargo test -p factory-native --test dependency_acquire_live
1 passed (explicit SKIPPED without FACTORY_LIVE_DEPENDENCY_FETCH=1)

FACTORY_LIVE_DEPENDENCY_FETCH=1 \
  cargo test -p factory-native --test dependency_acquire_live
1 passed; real crates.io fetch of `itoa = "=1.0.17"`; SBOM/license/advisory
files produced

cargo test -p factory-native --test dependency_broker_podman
1 passed (explicit SKIPPED without FACTORY_PODMAN_DEPENDENCY_TEST=1)

FACTORY_PODMAN_DEPENDENCY_TEST=1 \
FACTORY_DEPENDENCY_TEST_IMAGE=localhost/software-factory-dependency:dev \
  cargo test -p factory-native --test dependency_broker_podman
1 passed; real rootless Podman container, named-volume mount, receipt
validation, promoted evidence, container and volume cleanup verified

cargo test -p factory-native --bin factory-supervisor \
  --bin factory-verifier --bin factory-runtime-broker \
  --bin factory-dependency-acquire
3 + 7 + 8 + 7 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean

podman compose config
exit 0

podman build --target dependency \
  -t localhost/software-factory-dependency:dev -f deploy/Containerfile .
exit 0; image e3ca470a5a083f703e37783eb7543a238ac4acd94f350090218ab57149164b64
```

Real container smoke with a live registry fetch (implementer smoke, not the
nested engine):

```text
container: docker.io/library/rust:1.90.0-trixie
network:   --network=bridge (host rootless Podman)
fixture:   itoa = "=1.0.17"
exit:      0
receipt:   lockfileSha256 4e08e1b22e1dd04f9da7018ddb748adc33510d4ae970f397e71ba94b694e07db
           vendorSha256   15c9797e5d69a1c88fa85efcc2b39264532e07a2ed29618ff7c9e4446e12162f
           vendorEntries  17
           sbomSha256     16fdc85f1a220896187189e75a37f0a1d9e440a1b0fda2504fcf230f2f95c74c
           licenseSha256  6f2032e19e45e0ca47015a0fa371d31aeb35652b0a4256ec8acf0ea41b5a375c
           advisorySha256 d5b622217a917f93f6c106cef7905b3e2c4937b3a532b287ed3d163c337acaac
           stagingCleaned true
```

## Authority and integration path

- Core remains the authority. The Supervisor route requires the workspace
  capability and a Core-signed `dependencies` operation grant whose request
  digest binds every acquisition field. Core accepts the operation only for a
  leased `LEASED` attempt on an `AUTHORIZED` Campaign with an active provider
  session, and issues a long-lived fenced grant.
- `RuntimeBrokerClient::acquire_dependencies` is the typed client.
- `WorkspaceManager::acquire_dependencies` imports the worktree snapshot into
  the workspace volume, runs the broker operation, exports the volume and
  replaces the host worktree so vendor/evidence survive later check snapshots.
- The runtime broker `/health` reports whether dependency acquisition is
  configured.

The Worker does not yet call the Supervisor route automatically. Remaining
integration work for the integration owner:

1. Carry the frozen `DependencyPlan`/acquisition choice into the Worker
   assignment and call
   `POST /v1/workspaces/{workspace_id}/dependencies` after provider writes and
   before offline checks.
2. Decide and enforce candidate scope for `vendor/` and
   `vendor/.factory-supply-chain/` evidence (allow them explicitly or treat them
   as generated artifacts). The current `discard_outside_scope` pass is not
   applied to acquisition output.
3. Add Core/Office plan-review and diagnostics projection for dependency
   acquisition and evidence. Office parity impact for this slice is `none`
   because the change is an internal execution boundary, but a future plan
   review surface must show the selected acquisition strategy, evidence and
   blockers.

## Network and credential boundary

- Acquisition container: network-enabled (`--network=bridge` in the dedicated
  engine), credential-free, fixed image/argv, named workspace volume only.
- Check container: `--network=none`, offline Cargo commands.
- Broker: no Podman socket exposure, no host networking, no privileged mode, no
  arbitrary Podman flags, no caller-supplied image or environment.
- Registry allowlisting is **logical** (request field, plan contract and image
  allowlist). Network-level egress allowlisting to crates.io/GitHub is not
  implemented in this slice.
- The acquisition container runs `cargo fetch`/`cargo vendor`, which parse
  manifests and fetch/vendor sources. It does not invoke project build scripts
  or run untrusted project code with credentials.
- The runtime-broker token remains shared execution authority for the Worker,
  Supervisor, verifier and runtime broker; the reviewer does not receive it.

## Not tested / blocked / limitations

- `NOT_TESTED`: real acquisition inside the nested Podman engine of the
  Compose runtime container, including bundle import, `--network=bridge`
  reachability and engine cleanup. The actual `dependency` image target built
  successfully and the acquirer passed against host rootless Podman.
- `NOT_TESTED`: live registry fetch through the built `dependency` image; the
  live fetch smoke used a host-built debug binary in a Rust test image.
- `NOT_TESTED`: Worker/Core end-to-end dispatch of a frozen dependency plan.
- `NOT_IMPLEMENTED`: pinned advisory database, `cargo audit`/`cargo deny`
  verdicts and license-policy enforcement.
- `NOT_IMPLEMENTED`: network-level registry egress allowlisting.
- `NOT_TESTED`: Windows/Podman Machine, SELinux/AppArmor hosts, digest pinning.
- `BLOCKED` for any acceptance/certification claim: independent verification,
  independent security review and Product Owner acceptance have not run.
