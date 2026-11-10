# Wave 12 integration evidence

**Status:** implementation integrated on
`factory/quality-wave12-integration`; **not accepted, certified or deployed**.
This document records the integration owner's merge, conflict resolution and
validation evidence. It is implementer evidence only. WP10/WP12 acceptance and
all independent acceptance remain pending. No paid model inference was used.

- Baseline: `bcb8b5b` (`factory/agentic-flow-20261108`)
- Integration branch: `factory/quality-wave12-integration`
- Worktree: `/home/james/Escritorio/Development/sf-quality-wave12/integration`
- Date: 2026-11-09 local / 2026-11-10 UTC

## 1. Merged work packages

All branches were merged with `git merge --no-ff` in this order.

| WP | Branch | Branch head | Merge commit | Scope |
|---|---|---|---|---|
| WP5 | `factory/quality-wp5-provider-chaos` | `bd81c27` | `ac245f6` | Provider reconnect, duplicate suppression, lifecycle and structured-output reliability |
| WP7 | `factory/quality-wp7-openspec-compose` | `0b7fc3d` | `8348269` | Profile-gated OpenSpec validation job, token volume, archive/drift endpoint |
| WP9 | `factory/quality-wp9-reviewer-fleet` | `55c43e8` | `00ba373` | Six reviewer services, per-identity tokens/principals/roles, Gate role coverage |
| WP11 | `factory/quality-wp11-trace-view` | `fbc105c` | `054054d` | Bounded workflow trace endpoint and Office trace view |
| WP3 | `factory/quality-wp3-skill-refresh` | `35fcae7` | `487b0b8` | Skill-pack refresh scheduler, migrations `0067`/`0068`, Office Skills panel |
| WP6 | `factory/quality-wp6-workspace-recovery` | `b5dd282` | `40bce21` | Per-owner runtime-broker credentials, exact resource locks, active registry, reconciliation |
| WP4 | `factory/quality-wp4-dependency-broker` | `6cde43c` | `cce868e` | Dependency acquisition endpoint, internal egress proxy, image pinning, migration `0069` |

WP3, WP5, WP7, WP9 and WP11 were already merged on the integration branch
before this run. WP6 and WP4 were merged last because both change the runtime
broker and execution path.

## 2. Conflict resolutions

No conflict was resolved by accepting one side wholesale.

### WP6 merge (`40bce21`)

- `docs/implementation/quality-first-planning-status.md`: kept the newer WP7
  status text from the integration branch, applied the WP6 work-package row from
  `factory/quality-wp6-workspace-recovery`, and later updated all rows for the
  combined branch.
- `native/src/bin/compose_init.rs`: combined the six WP9 reviewer secret
  placements, the WP7 `OPENSPEC_SECRETS = ["operator.token"]` placement, and the
  WP6 per-owner runtime tokens. `VERIFIER_SECRETS` keeps both `verifier.token`
  and `runtime-broker-verifier.token`; `RUNTIME_SECRETS` holds only
  `runtime-broker-{supervisor,verifier,provider}.token`. The retired shared
  `runtime-broker.token` is deleted from every config volume on init.

### WP4 merge (`cce868e`)

- `native/src/runtime_broker.rs` (library): kept the WP6 owner/inventory/label
  and bounded-read implementation and added the WP4 dependency receipt bound.
- `native/src/bin/runtime_broker.rs`: combined the WP6 imports and validators
  with the WP4 egress socket and dependency image/bundle state. The WP4
  duplicate name validators were removed; dependency container names are
  validated by the shared library helper instead of a second copy.
- `native/src/bin/supervisor.rs`: kept both the WP6 reconciliation test and the
  WP4 dependency-capability/operation-grant test.
- `native/src/bin/worker.rs`: kept the WP4 recovery path and dependency state
  while preserving the WP5 no-candidate-on-provider-failure behavior. The
  `last_message_id` initializer and dependency state are initialized before the
  turn loop.
- `native/src/execution.rs`: merged the WP6 `LocalBackend` inventory/owner
  fields with the WP4 dependency barrier and acquisition fixture. Both
  `list_managed` and `acquire_dependencies` are implemented. Container-name
  validation accepts exact `factory-exec-<uuid>` and `factory-deps-<uuid>`
  identities.
- `docs/operations/compose-deployment.md`: kept both sections, with dependency
  acquisition before the OpenSpec validation job.

### WP4 adaptation to per-owner WP6 credentials

The WP4 branch forked before WP6 changed the broker from one shared token to
three owner tokens. The merged dependency route does **not** use a shared
destructive token:

- `DependencyService` is authenticated with the Supervisor owner token only.
- Before acquisition, a `DependencyGuard` acquires the same exact per-volume
  lock, verifies that the volume carries the managed Supervisor owner label and
  a workspace binding, and registers the volume/container in the active
  execution registry.
- The guard is released only after the acquisition finishes; volume
  delete/import/export and create paths see the active registration and fail
  closed.
- The dependency container is validated as `factory-deps-<uuid>` and the
  workspace volume as `factory-ws-<uuid>`.
- No owner is accepted from a query string, body field or caller-chosen
  parameter.

## 3. Combined topology

`compose.yaml` now includes:

- `core`, `supervisor`, `worker`, `verifier`, `runtime`, `init`,
  `provider-bundle`, `dependency-bundle`;
- six reviewer services: `reviewer-functional`, `reviewer-architecture`,
  `reviewer-code-quality`, `reviewer-security`, `reviewer-testing`,
  `reviewer-operability`;
- `openspec-validate` under the `openspec-validation` profile;
- dedicated config volumes for all six reviewers, the runtime broker owner
  tokens and the OpenSpec operator token;
- six reviewer state volumes, `factory-codex-home`, the dependency image bundle
  in `factory-image-bundle`, and the credential-free dependency image target.

`factory-compose-init` writes the reviewer identity registry and per-identity
tokens; Core reads `reviewer-identities.json` and derives the principal/role
from the authenticated token. Secrets are file-backed and are not passed in
argv, environment values or logs.

The runtime reaches the dependency image only from the shared read-only bundle.
The broker imports it into the nested engine, pins its image ID and re-verifies
that exact ID before each acquisition. Acquisition runs on a dedicated internal
network with a deny-by-default CONNECT proxy; the check container remains
`--network=none` and `--offline`.

## 4. Migrations

Order is unique and continuous for this wave:

| Version | Work package | Purpose |
|---|---|---|
| `0065_reviewer_role_coverage.sql` | WP9 | reviewer role on quality receipts |
| `0066_reviewer_role_assignments.sql` | WP9 | frozen role-to-principal assignment set |
| `0067_skill_refresh_cursor.sql` | WP3 | durable rotating project cursor |
| `0068_skill_pack_content_review.sql` | WP3 | separate source availability from content review |
| `0069_pre_authorization_dependency_plan.sql` | WP4 | dependency plan attached before Campaign authorization and frozen thereafter |

No duplicate version exists. `native/src/db/tests.rs` now has two smoke tests:

- `wave12_fresh_database_applies_latest_migrations` opens a new database,
  requires versions 65-69 exactly once, and queries the new columns/table/
  trigger.
- `wave12_upgraded_database_applies_only_pending_migrations` applies the
  repository migrations only through 0064, verifies 0065-0069 are absent,
  inserts a Project, reopens through `Database::open`, and verifies the pending
  migrations apply exactly once without losing the row.

## 5. Validation commands and results

All commands below ran in this worktree on the combined branch, with no paid
model inference.

### Rust

```text
cd native

cargo fmt --all -- --check
clean

cargo check -p factory-native --all-targets --all-features
clean

cargo test -p factory-native --lib
829 passed; 0 failed; 4 ignored

cargo test -p factory-native --bins
all binary suites passed:
  factory-core       38 passed
  factory-runtime-broker 29 passed
  factory-supervisor  9 passed
  factory-verifier    7 passed
  factory-worker     17 passed
  other binaries      0 tests

cargo test -p factory-native --test planning_pipeline --test provider_chaos \
  --test dependency_broker_http --test openspec_compose_validation \
  --test migration_skill_refresh
29 passed; 0 failed

cargo test -p factory-native --test dependency_broker_podman \
  --test dependency_acquire_live
2 passed; the real-Podman and live-registry bodies are opt-in and reported
SKIPPED without `FACTORY_PODMAN_DEPENDENCY_TEST=1` /
`FACTORY_LIVE_DEPENDENCY_FETCH=1`.

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

The 4 ignored library tests are the opt-in real-rootless-Podman execution
tests. They are reported as NOT_TESTED, not as passes.

### Office

```text
cd office

npm test -- --run
317 passed across 46 files

npm run typecheck
clean

npm run lint
clean
```

### Compose and policy

```text
podman compose -f compose.yaml config
OK (exit 0)

podman compose --profile openspec-validation -f compose.yaml config
OK (exit 0)

python3 .github/scripts/check-compose-policy.py /tmp/wave12-base.json
compose-policy: PASS

python3 .github/scripts/check-compose-policy.py /tmp/wave12-openspec.json
compose-policy: PASS

COMPOSE_CONFIG_JSON=/tmp/wave12-base.json \
  python3 .github/scripts/test_check_compose_policy.py
36 tests passed (8 skipped)

COMPOSE_CONFIG_JSON=/tmp/wave12-openspec.json \
  python3 .github/scripts/test_check_compose_policy.py
36 tests passed (5 skipped)
```

The local Podman Compose provider does not implement
`config --format json`, so the YAML output was normalized to the Docker Compose
JSON shape for the policy checker. CI continues to use the real Docker Compose
provider and the same checker/tests. The policy now covers the six reviewer
services, `dependency-bundle`, the runtime dependency image/bundle/network
configuration, per-reviewer identity files/principals/roles/state volumes, and
the `openspec-validate` profile. Negative tests reject cross-role reviewer
config/state mounts, reviewer identity swaps, dependency-bundle networking or
extra mounts, and runtime dependency policy drift.

### Architecture governance

```text
cargo run --manifest-path native/Cargo.toml \
  --bin factory-architecture-check -- --root . --write-roadmap
architecture governance: PASS
```

## 6. Documentation integration

- `docs/implementation/quality-first-planning-status.md` records every merged
  WP, the combined verification results and the remaining NOT_TESTED items. It
  states implementation integration only and keeps WP10/WP12 acceptance
  pending.
- `docs/product/office-parity.json` updates the planning capability notes for
  the runtime-broker owner tokens, reviewer fleet, skill refresh, OpenSpec job
  and dependency acquisition; those internal steps are explicitly not presented
  as new Office actions, and browser acceptance remains NOT_TESTED.
- `docs/product/capabilities.yaml` links the WP3/WP4/WP5/WP6/WP7/WP9/WP11
  implementation evidence and limitations without changing acceptance status;
  `docs/roadmap/ROADMAP.md` was regenerated from the registry.
- `docs/README.md` indexes this evidence document.

## 7. Remaining integration work and NOT_TESTED items

- WP10/WP12 acceptance: real Core/Compose browser acceptance, per-option
  matrix, visual review and Product Owner approval are pending.
- WP4: real Compose dependency acquisition through the internal network and
  allowlisted proxy; the real-Podman/live-registry tests are opt-in and were not
  enabled.
- WP6: Compose end-to-end restart and HTTP inventory acceptance; independent
  acceptance.
- WP5: live provider long-turn/SSE-cut UAT; independent acceptance.
- WP3: real Compose scheduler/restart evidence, content review of remaining
  catalog packs and independent browser acceptance of the Skills refresh panel.
- WP7: OpenSpec Compose validation against a projection produced by the real
  Core planning pipeline; current coverage uses a Core-rendered fixture.
- WP9: a real Campaign with two or more reviewer role receipts aggregated by
  the Gate; only deterministic DB/plan/Gate coverage ran.
- WP11: real Core/Compose browser acceptance of the trace view.
- No isolation, deployment or certification claim is made. `SEC-01-ISOLATED`
  remains out of scope under ADR-0035 and isolated dispatch remains fail-closed.
