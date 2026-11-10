# WP6 workspace recovery evidence

**Status:** implementer self-test evidence, **not** independent acceptance or
certification. This document records startup orphan-volume reconciliation, the
deterministic chaos tests and the review repairs for WP6. It does not claim
`ACCEPTED`, whole-work-package `PASS`, `TRUSTED_LOCAL` isolation or Compose
end-to-end coverage.

- Baseline: `bcb8b5b`
- Branch: `factory/quality-wp6-workspace-recovery`
- Original implementation commit: `e3b2454`
- Review-repair implementation commit: `1dc9176`
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

Independent review at `90289ed` found three valid issues; all were repaired:

1. **Owner-scoped deletion.** Volume/container deletion now requires an
   `owner` query value. The broker inspects the stored labels and refuses with
   `409 CONFLICT` when `managed=true` or the expected owner label is missing,
   unknown or different. Broker, verifier, Supervisor and Worker callers pass
   their own owner. Direct backend deletion (`RootlessPodmanVolumes`) performs
   the same label check.
2. **Safe volume creation.** `BrokerVolumes` no longer deletes a volume before
   creation. The broker inspects an existing volume and resets it only when it
   is managed, owned by the requesting service and labelled with the same
   workspace identity. A foreign or unlabelled pre-existing volume is preserved
   and the request fails closed. `RootlessPodmanVolumes` and the debug local
   backends implement the same rule.
3. **Bounded inventory reads.** The broker inventory subprocess is read through
   a capped stream and killed as soon as the next chunk would exceed 4 MiB; it
   is no longer buffered with `.output()` first. The runtime-broker client also
   accumulates the HTTP response chunk-by-chunk against the same bound instead
   of `response.bytes()`.
4. **Status truth.** `docs/implementation/quality-first-planning-status.md`
   now records WP6 as implemented for this slice with the remaining independent
   acceptance work, matching this evidence.

## Changed files

| File | Change |
|---|---|
| `native/src/runtime_broker.rs` | `ResourceOwner`, `ManagedResource(s)`, strict inventory/inspect parsers, owner/workspace identity checks, bounded stream accumulator, `list_managed_resources`, owner-scoped delete client calls |
| `native/src/bin/runtime_broker.rs` | Read-only authenticated `GET /v1/resources`; owner label on volume/container/provider creation; owner-verified delete endpoints; creation preserves foreign/mismatched volumes; bounded subprocess inventory reader; removal of broker startup container deletion |
| `native/src/execution.rs` | `VolumeBackend::owner`/`list_managed`, owner-labelled resources, ownership-checked create/remove, bounded `ReconcileReport`, durable-record repair and owner-scoped orphan sweep, chaos tests and real-Podman probes |
| `native/src/bin/supervisor.rs` | Startup reconciliation report logging, `/v1/reconcile` report, Supervisor-owned backend wiring, local inventory, ownership-checked debug backend |
| `native/src/bin/verifier.rs` | Verifier-owned backend label so Supervisor reconciliation cannot delete verifier resources |
| `native/src/bin/worker.rs` | Provider-owned container deletion during recovery |
| `native/src/provider_sandbox.rs` | Provider-owned container deletion during cleanup |
| `native/src/workspace_archive.rs` | Failed extraction removes its partial staging directory; truncated-archive rejection tests |
| `docs/security/container-runtime.md` | Reconciliation ownership, owner-scoped deletion and broker-restart behavior |
| `docs/engineering/container-native-engineering.md` | Broker-restart operational guidance |
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
6. `DELETE /v1/volumes/{volume}` and `DELETE /v1/containers/{container}` require
   `?owner=supervisor|verifier|provider`; the stored labels are verified before
   removal. Missing resources remain idempotent `204`; owner mismatch is `409`.
7. Volume creation is idempotent only for a matching managed owner/workspace
   identity. Preserving a foreign resource is the safe outcome, even if it
   delays a retry until an operator resolves the collision.

Limits: at most 4096 durable state records, 4096 inventory entries and 4 MiB of
inventory JSON/subprocess output; Podman commands retain the existing 120 s
command timeout. Inventory failure is reported (`sweepSupported: true`,
`sweepError`) and does not delete anything; an unsupported backend reports
`sweepSupported: false`.

## Commands and results

From `native/` unless noted:

```text
cargo fmt --all -- --check
clean

cargo test -p factory-native --lib
747 passed; 0 failed; 4 ignored

cargo test -p factory-native --bin factory-runtime-broker
13 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

Targeted new/affected tests:

```text
cargo test -p factory-native --lib reconcile
7 passed; 1 ignored

cargo test -p factory-native --lib workspace_archive::tests
6 passed

cargo test -p factory-native --lib runtime_broker::tests
8 passed
```

Review-repair tests include:

- `remove_volume_requires_the_expected_owner_label`
- `remove_volume_rejects_missing_owner_and_missing_resource`
- `remove_container_requires_the_expected_owner_label`
- `create_volume_refuses_a_preexisting_foreign_volume`
- `create_volume_reuses_a_matching_owned_workspace`
- `read_capped_stops_at_the_limit`
- `resource_ownership_requires_managed_and_exact_owner_labels`
- `volume_identity_requires_owner_and_workspace_match`
- `inspect_labels_and_bounded_streams_fail_closed`
- `create_and_remove_refuse_foreign_or_unlabelled_resources`

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
| Wrong/missing owner on delete | broker handler tests with a fake Podman | `409`; no `rm` invocation |
| Pre-existing foreign volume on create | broker handler test, execution backend test, real Podman probe | Preserved; request fails closed; no `rm`/`create` |
| Inventory output overflow | `read_capped_stops_at_the_limit`; client `extend_bounded` test | Stops at bound; process/response rejected |
| Inventory unavailable or unsupported | `reconcile_handles_unsupported_and_failed_inventory` | No deletion; status/error reported |
| Real Podman orphan sweep | `real_podman_reconcile_removes_only_its_orphan_volume` | Orphan removed; referenced volume preserved; cleanup confirmed |

## Security and authority

- Core remains the authority for active Attempt identity, candidates, budgets and
  acceptance. The sweep consumes only Supervisor durable records; it never
  infers authority from Podman state.
- No host Podman socket, host networking, privileged mode, host mount, arbitrary
  Podman flag or new image allowlist was added. The existing nested rootful
  engine inside the rootless outer container is unchanged.
- The broker inventory is read-only, authenticated, argument-free and filtered by
  the managed label; names are validated before any deletion.
- Deletion is owner-scoped and verifies the stored label. Creation resets only a
  managed, same-owner, same-workspace volume; foreign/unlabelled resources are
  preserved.
- Legacy managed resources without an owner label are never deleted. They require
  explicit manual cleanup; this is intentional (`never remove unlabelled`).
- Provider containers are owned by Worker recovery, not by Supervisor
  reconciliation, so a Supervisor sweep cannot kill a provider session.

## Limitations and NOT_TESTED / BLOCKED

- **NOT_TESTED:** full `podman compose up` end-to-end run against the nested
  engine and the HTTP `GET /v1/resources` endpoint. The endpoint handlers and
  parsers are unit-tested, including fake-Podman owner rejection; the real-Podman
  probes exercise the same inventory/sweep/ownership logic through
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
- **KNOWN LIMITATION:** the shared runtime-broker credential still lets a
  compromised trusted service attempt deletion of a resource name it knows; the
  owner-label check prevents accidental cross-owner deletion but is not a
  per-role credential boundary.
- **BLOCKED for acceptance:** independent verifier/reviewer run, candidate-bound
  Compose/browser acceptance, and any certification claim. This document is
  implementer self-test evidence only.
