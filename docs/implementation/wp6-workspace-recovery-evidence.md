# WP6 workspace recovery evidence

**Status:** implementer self-test evidence, **not** independent acceptance or
certification. This document records startup orphan-volume reconciliation, the
deterministic chaos tests and two review-repair rounds for WP6. It does not
claim `ACCEPTED`, whole-work-package `PASS`, `TRUSTED_LOCAL` isolation or Compose
end-to-end coverage.

- Baseline: `bcb8b5b`
- Branch: `factory/quality-wp6-workspace-recovery`
- Original implementation commit: `e3b2454`
- First review-repair commit: `1dc9176`
- Second review-repair implementation: `394719a`
- Third review-repair implementation: `22fd634`
- Fourth review-repair implementation: `d6c53cb`
- Fifth review-repair implementation: `cfaf20f`
- Worktree: `/home/james/Escritorio/Development/sf-quality-wave12/wp6`

## Scope

WP6 previously had content-addressed snapshots, generated-target exclusion,
unsafe-archive rejection, cleanup and durable workspace state. The remaining gap
was startup reconciliation of managed Podman resources that are not referenced
by durable state, plus a dedicated crash/restart chaos run.

Core remains the authority for active Attempt identity. The Supervisor owns its
durable `workspace-*.json` records and a backend owner label; it may remove only
resources labelled `io.software-factory.owner=supervisor` that no
`ALLOCATED`/`RUNNING` record references.

## Review repairs

Independent review at `90289ed` (P1/P2) and re-review at `fb3818b` (P1/P2) found
valid issues. All were repaired:

1. **Owner-scoped deletion.** The first repair added an `owner` query parameter.
   The re-review correctly rejected caller-chosen ownership: every caller shared
   one bearer token. The final model is three distinct owner tokens.
2. **Safe volume creation.** `BrokerVolumes` no longer deletes a volume before
   creation. The broker inspects an existing volume and resets it only when it
   is managed, owned by the authenticated service and labelled with the same
   workspace identity. A foreign or unlabelled pre-existing volume is preserved
   and the request fails closed. `RootlessPodmanVolumes` and the debug local
   backends implement the same rule.
3. **Bounded inventory/inspect reads.** Inventory and label-inspect subprocesses
   are read through a capped stream and killed as soon as the next chunk would
   exceed 4 MiB; they are no longer buffered with `.output()` first. The
   runtime-broker client accumulates HTTP chunks against the same bound.
4. **Per-owner credentials.** The runtime broker reads three token files and
   derives `ResourceOwner` from the authenticated bearer token. No request body
   or query parameter can choose an owner. Create, import, export, execute,
   delete, reset, status and inventory operations enforce the derived owner.
   Cleanup/reaper paths use the same checked helpers.
5. **Serialize per-resource mutations.** Create/reset/delete/import/export/run
   acquire an exact per-resource-name async lock and recheck ownership after
   acquiring it. Locks are bounded with safe reclamation; lock order is
   volume -> container. Concurrent create for different owners and concurrent
   delete/reset are deterministic tests.
6. **Status truth.** `docs/implementation/quality-first-planning-status.md`
   records WP6 as implemented for this slice with the remaining independent
   acceptance work.
7. **Exact resource locks.** The re-review found that sharded locks could block
   unrelated names on a long execution's shard. Locks are now exact per resource
   name, permanently bounded at 4096 entries; entries are never reclaimed so a
   name keeps one lock identity. Additional names permanently share one overflow
   lock and intentionally serialize. Lock order is volume -> container.
8. **Execution cleanup lock.** `run_child` cleanup now calls the locking
   container wrapper, not the unchecked helper, so concurrent delete/recreate
   cannot replace the inspected container before removal. A deterministic race
   test asserts strict inspect/remove serialization.
9. **Overflow lock identity.** The fourth review found that reclaiming an exact
   entry could give an overflowed name a second lock identity. Reclamation is
   removed: the 4096-entry exact table is permanent, and additional names
   permanently share one overflow lock. A saturation test holds the overflow
   lock while an exact slot becomes idle and re-acquires the same name, proving
   exclusion is preserved.
10. **No lock held across execution.** The fifth review found that when both the
    volume and container names used the shared overflow mutex, cleanup waited on
    a mutex held by its own execution task. Execution now holds the volume lock
    only for validation and registry registration, releases it for the run, and
    blocks volume delete/reset/import/export through an active-execution
    registry. Cleanup takes only the container lock. A test with a zero-entry
    exact table (both names overflow) asserts no deadlock, and a mutation test
    asserts volume delete/reset are refused while the execution is active.

## Per-owner credential model

`factory-compose-init` creates three distinct tokens:

| Caller | Token file in its config volume | Runtime broker file |
|---|---|---|
| Supervisor | `runtime-broker-supervisor.token` | `/config/runtime-broker-supervisor.token` |
| Verifier | `runtime-broker-verifier.token` | `/config/runtime-broker-verifier.token` |
| Worker/provider | `runtime-broker-provider.token` | `/config/runtime-broker-provider.token` |

The runtime service mounts `/config` read-only and receives
`FACTORY_RUNTIME_BROKER_TOKEN_DIR=/config`. Each caller receives only its own
token through its existing `FACTORY_RUNTIME_BROKER_TOKEN_FILE`. The retired
shared `runtime-broker.token` is removed by compose-init. Tokens are file-loaded,
never placed in argv, logs or the Office UI; the broker registers them for log
redaction.

Authorization rules:

- Supervisor/verifier may create/reuse their own workspace volumes, import,
  export, execute and delete their own resources.
- Provider may start/status/delete its own provider containers; provider cannot
  create workspace volumes or execute workspace children.
- Inventory is filtered to the authenticated owner.
- Unknown/missing token → `401`; wrong owner for a resource → `409`; provider
  attempting an execution operation the provider does not own → `403`.

## Changed files

| File | Change |
|---|---|
| `native/src/runtime_broker.rs` | `ResourceOwner`, managed-resource/inspect parsers, owner/workspace identity checks, bounded stream accumulator, owner-bound client, `list_managed_resources`; client constructors now take the owner |
| `native/src/bin/runtime_broker.rs` | Three-token `OwnerTokens` registry, derived-owner authorization on every operation, filtered inventory, owner-verified delete/reset, capped inventory/inspect readers, per-resource async locks, checked reaper/cleanup paths |
| `native/src/execution.rs` | `VolumeBackend::owner`/`list_managed`, owner-labelled resources, ownership-checked create/remove, bounded `ReconcileReport`, durable-record repair and owner-scoped orphan sweep, chaos tests and real-Podman probes |
| `native/src/bin/supervisor.rs` | Startup reconciliation report, `/v1/reconcile`, Supervisor-owned backend wiring, local inventory, ownership-checked debug backend |
| `native/src/bin/verifier.rs` | Verifier owner token/backend label |
| `native/src/bin/worker.rs` | Provider-bound broker client for recovery |
| `native/src/provider_sandbox.rs` | Provider-bound broker client for session cleanup |
| `native/src/bin/compose_init.rs` | Three owner token files, legacy shared-token removal |
| `compose.yaml` | Per-service broker token files; broker token directory |
| `native/src/workspace_archive.rs` | Failed extraction removes partial staging; truncated-archive rejection tests |
| `docs/security/container-runtime.md` | Per-owner credential model and reconciliation ownership |
| `docs/engineering/container-native-engineering.md` | Per-owner broker credential rule |
| `docs/operations/compose-deployment.md` | Owner-token deployment table and rules |
| `docs/implementation/quality-first-planning-status.md` | WP6 status aligned with this evidence |

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
3. The Supervisor reads its owner-scoped managed inventory. It removes only
   names that are valid `factory-ws-<uuid>` volumes or `factory-exec-<uuid>`
   containers, carry `io.software-factory.owner=supervisor`, and are absent from
   every durable record (including the in-process active map). Foreign-owner,
   unlabelled, provider and invalid-name resources are preserved.
4. The broker no longer deletes managed containers on startup. A broker-only
   restart therefore cannot kill an active provider session or execution child
   merely by restarting; the owning service reconciles failures it observes.
5. Broker mutations acquire an exact per-resource-name async lock before
   inspecting and recheck ownership after the lock. The exact table is
   permanently bounded at 4096 entries and never reclaimed, so a name always has
   one lock identity. Names beyond capacity permanently share one overflow lock
   and intentionally serialize. Execution takes the volume lock only to verify
   ownership and register an active-execution entry, then releases it for the
   run; volume delete/reset/import/export returns `409` while that registry entry
   exists. Cleanup takes the container lock after the volume lock is released, so
   volume -> container ordering is preserved and an overflowed volume/container
   pair cannot self-deadlock. Concurrent create for different owners cannot
   return success for a volume owned by the other caller; concurrent
   delete/reset cannot remove a replacement created under the lock, and an
   unrelated resource within exact-table capacity is not blocked by a long
   execution.

Limits: at most 4096 durable state records, 4096 inventory entries, 4096 exact
resource-lock entries (entries are permanent; additional names permanently share
one overflow lock and serialize), 4096 concurrently registered active executions,
and 4 MiB of inventory/inspect subprocess output; Podman commands retain the
existing 120 s command timeout. Inventory failure is reported
(`sweepSupported: true`, `sweepError`) and does not delete anything; an
unsupported backend reports `sweepSupported: false`.

## Commands and results

From `native/` unless noted:

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib
747 passed; 0 failed; 4 ignored

cargo test -p factory-native --bin factory-runtime-broker
23 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

Targeted results:

```text
cargo test -p factory-native --lib reconcile
7 passed; 1 ignored

cargo test -p factory-native --lib workspace_archive::tests
6 passed

cargo test -p factory-native --lib runtime_broker::tests
8 passed
```

Owner-token spoofing and authorization tests (deterministic fake Podman):

- `remove_volume_requires_the_authenticated_owner` — no token `401`;
  verifier token against a supervisor-owned volume `409` with no `rm`; correct
  owner `204`.
- `import_export_execute_and_provider_start_require_the_authenticated_owner` —
  supervisor token against a verifier-owned volume is rejected for import,
  export and execute; supervisor cannot start a provider session.
- `remove_container_requires_the_expected_owner_label` — wrong owner `409`,
  correct provider owner `204`.
- `managed_inventory_is_scoped_to_the_authenticated_owner`.
- `owner_tokens_are_distinct_and_authenticate_exactly` — duplicate or short
  tokens fail startup.

Concurrency tests (stateful fake Podman with a shared log):

- `resource_locks_keep_one_identity_per_name_at_saturation` — same name shares
  one lock, different names do not, the exact table is never reclaimed, an
  overflowed name keeps the overflow identity after an exact slot becomes idle,
  and a held overflow lock still excludes that name.
- `concurrent_create_for_different_owners_serializes_per_name` — one `201` and
  one `409`; the stored owner matches the successful caller.
- `concurrent_delete_and_reset_never_removes_the_replacement` — no `rm`
  interleaves between create-reset start and completion.
- `execution_cleanup_serializes_with_container_delete` — execution cleanup and a
  concurrent container delete produce two strict
  inspect-start/inspect-end/rm sequences; no replace-before-remove window.
- `execution_cleanup_works_when_volume_and_container_share_overflow` — a
  zero-entry exact table forces both names onto the overflow mutex; cleanup
  completes instead of waiting on a mutex held by its own execution task.
- `active_execution_blocks_volume_mutation_without_holding_the_lock` — an active
  execution refuses volume deletion and reset without holding a lock across the
  run.
- `long_execution_does_not_block_an_unrelated_resource` — a create for another
  volume within exact-table capacity completes while an 800 ms execution is
  active.

Bounded-stream tests:

- `read_capped_stops_at_the_limit`
- `inspect_output_is_bounded_before_parsing` (fake inspect emits 5 MB; the
  capped reader rejects it)
- runtime-broker client `extend_bounded` overflow test

Real rootless Podman probes (host Podman `6.1.4`, rootless confirmed,
`alpine:3.20` cached):

```text
cargo test -p factory-native --lib real_podman -- --ignored
3 passed; 0 failed

podman volume ls --filter label=io.software-factory.managed=true
(empty after cleanup)

podman ps -a --filter label=io.software-factory.managed=true
(empty after cleanup)
```

The real probes create a verifier-owned volume and prove a supervisor cannot
reuse or delete it; create one Supervisor-owned orphan and one referenced
workspace volume, assert the pre-sweep inventory contains no unrelated managed
resources, run `reconcile`, and verify only the orphan was removed. The
destructive sweep refuses to run while unrelated managed resources exist.

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
| Wrong/missing owner token or resource owner | broker handler tests with fake Podman | `401`/`403`/`409`; no mutation invoked |
| Pre-existing foreign volume on create | broker handler test, execution backend test, real Podman probe | Preserved; request fails closed; no `rm`/`create` |
| Concurrent create for different owners | `concurrent_create_for_different_owners_serializes_per_name` | One success, one conflict; ownership consistent |
| Concurrent delete/reset | `concurrent_delete_and_reset_never_removes_the_replacement` | Serialized; no interleaved delete |
| Execution cleanup vs container delete | `execution_cleanup_serializes_with_container_delete` | Strict inspect/remove sequences; no replacement race |
| Overflowed volume + container cleanup | `execution_cleanup_works_when_volume_and_container_share_overflow` | No self-deadlock on the shared overflow mutex |
| Volume mutation during active execution | `active_execution_blocks_volume_mutation_without_holding_the_lock` | Delete/reset `409`; run continues |
| Long execution vs unrelated resource | `long_execution_does_not_block_an_unrelated_resource` | Unrelated create within exact-table capacity completes while the execution is active; overflow names intentionally serialize |
| Inventory/inspect output overflow | `read_capped_stops_at_the_limit`, `inspect_output_is_bounded_before_parsing` | Stops at bound; process/response rejected |
| Inventory unavailable or unsupported | `reconcile_handles_unsupported_and_failed_inventory` | No deletion; status/error reported |
| Real Podman orphan sweep | `real_podman_reconcile_removes_only_its_orphan_volume` | Orphan removed; referenced volume preserved; cleanup confirmed |

## Security and authority

- Core remains the authority for active Attempt identity, candidates, budgets and
  acceptance. The sweep consumes only Supervisor durable records; it never
  infers authority from Podman state.
- No host Podman socket, host networking, privileged mode, host mount, arbitrary
  Podman flag or new image allowlist was added. The existing nested rootful
  engine inside the rootless outer container is unchanged.
- Runtime-broker authorization is derived from three distinct file-loaded owner
  tokens. The broker never accepts an owner from the request. The reviewer has
  no broker token.
- The broker inventory is read-only, authenticated, argument-free and scoped to
  the authenticated owner.
- Deletion is owner-scoped and verifies the stored label after acquiring the
  per-name lock. Creation resets only a managed, same-owner, same-workspace
  volume; foreign/unlabelled resources are preserved.
- Mutations use exact per-resource locks with a permanent 4096-entry table.
  Entries are never reclaimed so a name cannot change lock identity; names
  beyond capacity permanently share one overflow lock and serialize. An
  execution holds the volume lock only for validation/registration and then
  blocks volume delete/reset/import/export through the active-execution registry;
  cleanup takes only the container lock. No lock is held across the run, so an
  overflowed volume/container pair cannot self-deadlock.
- Legacy managed resources without an owner label are never deleted. They require
  explicit manual cleanup; this is intentional (`never remove unlabelled`).
- Provider containers are owned by Worker recovery, not by Supervisor
  reconciliation, so a Supervisor sweep cannot kill a provider session.

## Limitations and NOT_TESTED / BLOCKED

- **NOT_TESTED:** full `podman compose up` end-to-end run against the nested
  engine and the HTTP `GET /v1/resources` endpoint. The endpoint handlers,
  tokens, parsers and concurrency are unit-tested with fake Podman; the
  real-Podman probes exercise the execution/sweep/ownership logic through
  `RootlessPodmanVolumes`, not through the broker HTTP boundary.
- **NOT_TESTED:** actual process kill/restart of the Compose Supervisor at every
  crash point. The durable-record scenarios are deterministic simulations.
- **NOT_TESTED:** broker-only restart with a live provider session; provider
  ownership is covered at the Worker recovery boundary but not a live restart.
- **NOT_TESTED:** duplicate reconciliation across two Supervisor processes. The
  current Compose topology runs one Supervisor; the manager mutex covers
  in-process duplicates only. A cross-process file lock is not implemented.
- **KNOWN LIMITATION:** an inventory failure during startup is logged and the
  Supervisor continues without sweeping; unresolved orphans remain until the
  next successful reconciliation.
- **KNOWN LIMITATION:** a compromised trusted caller can perform any operation
  within its own owner scope, and the runtime container necessarily holds all
  three tokens. This is engine-scoped authority, not per-role isolation.
- **BLOCKED for acceptance:** independent verifier/reviewer run, candidate-bound
  Compose/browser acceptance, and any certification claim. This document is
  implementer self-test evidence only.
