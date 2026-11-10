# WP6 workspace recovery evidence

**Status:** implementer self-test evidence, **not** independent acceptance or
certification. This document records the implementation of startup orphan-volume
reconciliation and the deterministic chaos tests for WP6. It does not claim
`ACCEPTED`, `PASS` for the whole work package, `TRUSTED_LOCAL` isolation or
Compose end-to-end coverage.

- Baseline: `bcb8b5b`
- Branch: `factory/quality-wp6-workspace-recovery`
- Implementation commit: `e3b2454`
- Worktree: `/home/james/Escritorio/Development/sf-quality-wave12/wp6`

## Scope

WP6 previously had content-addressed snapshots, generated-target exclusion,
unsafe-archive rejection, cleanup and durable workspace state. The remaining gap
was startup reconciliation of managed Podman resources that are not referenced
by durable state, plus a dedicated crash/restart chaos run.

The implementation keeps Core as the authority for active Attempt identity. The
Supervisor owns its durable `workspace-*.json` records and a backend owner label;
it may remove only resources labelled `io.software-factory.owner=supervisor`
that no `ALLOCATED`/`RUNNING` record references.

## Changed files

| File | Change |
|---|---|
| `native/src/runtime_broker.rs` | `ResourceOwner`, `ManagedResource(s)`, strict `podman volume ls`/`ps` JSON parsers, shared resource-name validators, `RuntimeBrokerClient::list_managed_resources` |
| `native/src/bin/runtime_broker.rs` | Read-only authenticated `GET /v1/resources`; owner label on volume/container/provider creation; **removed** unconditional managed-container deletion at broker startup |
| `native/src/execution.rs` | `VolumeBackend::owner`/`list_managed`, owner-labelled volumes/containers, bounded `ReconcileReport`, durable-record repair and owner-scoped orphan sweep, new chaos tests and a real-Podman ignored test |
| `native/src/bin/supervisor.rs` | Startup reconciliation report logging, `POST /v1/reconcile` report response, Supervisor-owned backend wiring, local test-backend inventory |
| `native/src/bin/verifier.rs` | Verifier-owned backend label so Supervisor reconciliation cannot delete verifier volumes/containers |
| `native/src/workspace_archive.rs` | Failed extraction removes its partial staging directory; truncated-archive rejection tests |
| `docs/security/container-runtime.md` | Reconciliation ownership and broker-restart behavior corrected |
| `docs/engineering/container-native-engineering.md` | Broker-restart operational guidance corrected |

## Reconciliation behavior

1. The Supervisor takes its single workspace-manager write lock. Allocation,
   execution, cleanup and reconciliation are serialized, so an in-process active
   execution cannot lose its volume or container to a concurrent reconciliation.
2. Durable records are repaired in one bounded pass:
   - `RUNNING` → remove its container/volume, persist `FAILED` with
     `Supervisor restarted during execution`; the record is retained so a
     re-allocation can recover the worktree.
   - `FAILED` → remove volume/worktree and delete the record.
   - `COMPLETED` → remove the disposable transfer volume; keep the record and
     Git candidate for idempotent reopen.
   - `ALLOCATED` → re-create/ensure the transfer volume (repairs a stale record
     whose volume disappeared).
3. The Supervisor reads the broker's bounded managed inventory. It removes only
   names that are valid `factory-ws-<uuid>` volumes or `factory-exec-<uuid>`
   containers, carry `io.software-factory.owner=supervisor`, and are absent from
   every durable record (including the in-process active map). Foreign-owner,
   unlabelled, provider and invalid-name resources are counted and preserved.
4. `GET /v1/resources` requires the runtime-broker bearer token, accepts no
   arguments, does not mutate the engine and returns only resources filtered by
   `io.software-factory.managed=true`.
5. The broker no longer deletes managed containers on startup. A broker-only
   restart therefore cannot kill an active provider session or execution child
   merely by restarting; the owning service reconciles failures it observes.

Limits: at most 4096 durable state records, 4096 inventory entries and 4 MiB of
inventory JSON; Podman commands retain the existing 120 s command timeout.
Inventory failure is reported (`sweepSupported: true`, `sweepError`) and does not
delete anything; an unsupported backend reports `sweepSupported: false`.

## Commands and results

From `native/` unless noted:

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib
743 passed; 0 failed; 3 ignored

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

Targeted new tests:

```text
cargo test -p factory-native --lib reconcile
7 passed (3 new reconciliation tests plus 4 existing matching tests)

cargo test -p factory-native --lib duplicate_concurrent_reconciliation
1 passed

cargo test -p factory-native --lib active_execution_outlives
1 passed (repeated 5 times consecutively)

cargo test -p factory-native --lib corrupt_exported_archive
1 passed

cargo test -p factory-native --lib workspace_archive::tests
6 passed

cargo test -p factory-native --lib runtime_broker::tests
5 passed
```

Real rootless Podman chaos probe (host Podman `6.1.4`, rootless confirmed,
`alpine:3.20` cached):

```text
cargo test -p factory-native --lib \
  execution::tests::real_podman_reconcile_removes_only_its_orphan_volume \
  -- --ignored --exact
1 passed

podman volume ls --filter label=io.software-factory.managed=true
(empty after cleanup)

podman ps -a --filter label=io.software-factory.managed=true
(empty after cleanup)
```

The real test creates one Supervisor-owned orphan volume and one referenced
workspace volume, asserts the pre-sweep inventory contains no unrelated managed
resources, runs `reconcile`, verifies only the orphan was removed, then cleans
the referenced workspace. It refuses to run a destructive sweep if unrelated
managed volumes or any managed containers are present.

Compose static validation from the repository root:

```text
podman compose config
exit 0; 411 lines rendered
```

## Chaos matrix

| Scenario | Test | Result |
|---|---|---|
| Volume created without a durable record (crash/lost state) | `reconcile_removes_only_owned_unreferenced_managed_resources` | Orphan removed; referenced volume preserved |
| Durable `ALLOCATED` record with missing volume | `reconcile_repairs_missing_volume_and_recovers_running_record` | Volume re-created |
| Durable `RUNNING` record after restart | same test | Container/volume removed; record `FAILED`; next pass deletes record |
| Duplicate concurrent reconciliation | `duplicate_concurrent_reconciliation_is_idempotent` | Serialized; each orphan removed once; third pass no-op |
| Active execution during reconciliation | `active_execution_outlives_concurrent_reconciliation` | Reconcile waits for the manager lock; candidate committed |
| Corrupt/incomplete exported archive | `corrupt_exported_archive_fails_closed_without_replacing_the_worktree` | `Archive` error; record `FAILED`; repository HEAD/worktree unchanged |
| Truncated tar header/content | `rejects_truncated_archives_and_removes_partial_staging`, `rejects_archive_truncated_mid_entry_and_leaves_existing_destination` | Rejected; partial staging removed; pre-existing destination untouched |
| Foreign owner / unlabelled / provider resources | `reconcile_removes_only_owned_unreferenced_managed_resources` | Preserved and counted |
| Inventory unavailable or unsupported | `reconcile_handles_unsupported_and_failed_inventory` | No deletion; status/error reported |
| Real Podman orphan sweep | `real_podman_reconcile_removes_only_its_orphan_volume` (ignored, run explicitly) | Orphan removed; referenced volume preserved; cleanup confirmed |

## Security and authority

- Core remains the authority for active Attempt identity, candidates, budgets and
  acceptance. The sweep consumes only Supervisor durable records; it never
  infers authority from Podman state.
- No host Podman socket, host networking, privileged mode, host mount, arbitrary
  Podman flag or new image allowlist was added. The existing nested rootful
  engine inside the rootless outer container is unchanged.
- The broker inventory is read-only, authenticated, argument-free and filtered by
  the managed label; names are validated before any deletion.
- Legacy managed resources without an owner label are never deleted. They require
  explicit manual cleanup; this is intentional (`never remove unlabelled`).
- Provider containers are owned by `worker` recovery, not by Supervisor
  reconciliation, so a Supervisor sweep cannot kill a provider session.

## Limitations and NOT_TESTED / BLOCKED

- **NOT_TESTED:** full `podman compose up` end-to-end run against the nested
  engine and the HTTP `GET /v1/resources` endpoint. The endpoint handler and
  parsers are unit-tested; the real-Podman test exercises the same inventory and
  sweep logic through `RootlessPodmanVolumes`, not through the broker HTTP
  boundary.
- **NOT_TESTED:** actual process kill/restart of the Compose Supervisor at every
  crash point. The durable-record scenarios are deterministic simulations.
- **NOT_TESTED:** broker-only restart with a live provider session; deriving a
  provider owner in the Supervisor is intentionally out of scope.
- **NOT_TESTED:** duplicate reconciliation across two Supervisor processes. The
  current Compose topology runs one Supervisor; the manager mutex covers
  in-process duplicates only. A cross-process file lock is not implemented.
- **KNOWN LIMITATION:** an inventory failure during startup is logged and the
  Supervisor continues without sweeping; unresolved orphans remain until the
  next successful reconciliation.
- **KNOWN LIMITATION:** the broker delete endpoints do not verify an ownership
  label; the shared runtime-broker credential remains a scoped shared authority
  (already recorded in `docs/security/container-runtime.md`).
- **BLOCKED for acceptance:** independent verifier/reviewer run, candidate-bound
  Compose/browser acceptance, and any certification claim. This document is
  implementer self-test evidence only.
