# Test-data reset and restore (development only)

**Status:** development/operator recovery tool. **Not** an install step, a
runtime feature, an Office action, a Core API or a scheduled cleanup job.
Raw SQL deletion is not a Software Factory product feature.

The live test database can accumulate old Campaigns, requests, provider
sessions and `AMBIGUOUS`/`UNKNOWN` model reservations while the Project,
profile, provider, GitHub and charter configuration is still valid. Resetting
the transactional rows is sometimes necessary before a clean acceptance run.
`scripts/reset-test-data.sh` performs that reset only for an explicitly marked
test/dev Compose project, after an offline backup.

## What is preserved and what is deleted

Preserved operator configuration:

- Projects, project configuration and source discovery;
- execution profiles, revisions, defaults, profile audit and policy grants;
- standing delegations and issue-intake policy revisions/heads;
- provider settings and their audit, inference-ledger mode;
- the GitHub connection, project bindings and connection audit;
- owner charters, Factory settings, skill packs and skill-refresh state;
- device sessions, pairing state, the owner bootstrap claim, access epoch.

Deleted transactional/diagnostic state includes Actions, Campaigns, Runs,
attempts, candidates, quality receipts/gates and revisions, Lead/intake/agentic
requests and plans, provider sessions, tool events, turn reservations, model
calls, inference envelopes/reservations/ledger events, events, leases,
idempotency keys, outbox/external-effect records, subscription admissions and
GitHub issue/sync rows.

Project repositories, Supervisor workspace volumes, provider-session files,
logs and evidence outside the SQLite database are not touched. This script
resets **Core database state only**.

The helper fails closed if a migration introduced a table that is not
explicitly classified as preserved or transactional. Update
`scripts/reset-test-data.py` and its test before resetting such a schema.

## Guards

The script refuses unless all of these are true:

1. `FACTORY_TEST_DATA_RESET=1` is set explicitly.
2. The Compose project name contains a test/dev/e2e/uat/wp marker and is not
   `software-factory`, `production`, `prod` or `default`.
3. `--evidence-dir` names an empty directory outside `/data`.
4. `--database` (test-copy mode) does not resolve into `/data`.
5. The operator confirms interactively by typing the exact project name, or
   passes both `--yes` and `--confirm-project <exact project>`.
6. For Compose mode, every project service is stopped before the backup and
   SQL mutation. The script stops them after confirmation.

The Python helper enforces the same environment, project-marker,
`--confirmed-project`, empty-evidence and `/data` rules when invoked directly.
It mounts the Compose volume at `/factory-data`, never `/data`, so its `/data`
refusal also applies to the helper container path. Direct helper use without
the exact confirmation argument fails. The database and its WAL/SHM sidecars
must be regular files; symlinked inputs are refused before opening or copying,
and backups are created as regular files with no symlink target.

The script never runs automatically. It is not referenced by Compose, the
Containerfiles, Core startup, the Worker, install scripts or normal runtime.

## Backup layout

The caller chooses the evidence directory. It must be empty, and the script
writes:

| File | Meaning |
|---|---|
| `factory.sqlite` | Consistent main database copy |
| `factory.sqlite-wal` | WAL copy when present at backup time |
| `SHA256SUMS` | `sha256sum -c` compatible checksums |
| `manifest.json` | Project, source path, file hashes and before counts |
| `reset-report.json` | Dry-run/reset result, before/after counts and table classes |
| `recovery-before-restore/` | Backup of the failed reset database before automatic recovery |
| `recovery-report.json` | `RESTORED_PRE_RESET` or `RECOVERY_FAILED` after a post-commit failure |
| `reset-report.committed.json` | Preserved committed report when a later Core health failure rolls the reset back |

The backup happens before any delete. A missing backup or checksum failure
aborts the reset.

## Reset a Compose test project

```sh
FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
  --project software-factory-test \
  --evidence-dir "$HOME/factory-evidence/reset-2026-11-10"
```

Non-interactive (for a scripted operator workflow):

```sh
FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
  --project software-factory-test \
  --evidence-dir "$HOME/factory-evidence/reset-2026-11-10" \
  --yes --confirm-project software-factory-test
```

The script stops the project, copies the database and WAL into the
evidence directory, drops only the `BEFORE DELETE` immutable guards inside one
transaction, deletes children before parents in a schema-derived foreign-key
order, verifies `PRAGMA foreign_key_check`, restores the triggers, and runs
`PRAGMA integrity_check` before commit. It commits only after that check, runs
the integrity check again, then starts `init` and Core, starts the remaining
configured services, waits for Core health and prints before/after counts.

A pre-commit failure rolls back. A post-commit integrity failure, or a Core
health failure after a successful commit, automatically restores the verified
pre-reset backup after first backing up the failed reset database into
`recovery-before-restore/`. The script then restarts Core, rechecks health and
exits nonzero. It prints `The reset is NOT complete` and never prints the
success line after a recovery. If automatic recovery itself fails, it reports
the evidence location and exits nonzero; manual restore is required.

## Dry-run a copy

A Compose dry-run is deliberately unsupported because it would have to stop
the live project merely to inspect it. Stop the project, export the data volume
to a temporary directory, restart the project, then run against the copy so no
live transaction or WAL file is touched:

```sh
PROJECT=software-factory-test
podman compose -p "$PROJECT" stop
mkdir -p /tmp/factory-db-copy
podman volume export "$(podman volume ls \
  --filter "label=com.docker.compose.project=$PROJECT" \
  --filter "label=com.docker.compose.volume=factory-data" \
  --format '{{.Name}}')" | tar -x -C /tmp/factory-db-copy
podman compose -p "$PROJECT" up -d

FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
  --project wp6-test \
  --database /tmp/factory-db-copy/factory.sqlite \
  --evidence-dir /tmp/factory-reset-evidence \
  --dry-run
```

`--database` mode is for test copies only. It uses host `python3` and refuses
`/data`. The dry-run takes the same backup, executes the same deletes in a
transaction, prints projected counts and rolls back. The report records
`"database_modified": false`.

## Restore a backup

The script verifies `SHA256SUMS`, accepts only the exact names
`factory.sqlite`, `factory.sqlite-wal` and `factory.sqlite-shm`, rejects
separators, `..`, absolute paths, symlinks and unknown sidecars, backs up the
current database into a new empty evidence directory, removes stale WAL/SHM
files, restores the main database and any WAL from the backup, then runs
integrity and foreign-key checks:

```sh
FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
  --project software-factory-test \
  --evidence-dir "$HOME/factory-evidence/pre-restore-2026-11-10" \
  --restore-from "$HOME/factory-evidence/reset-2026-11-10" \
  --yes --confirm-project software-factory-test
```

For a test copy, add `--database /tmp/factory-copy.sqlite`. For Compose mode the
same stop/backup/restore/start/health lifecycle applies.

Manual verification after a restore:

```sh
(cd "$HOME/factory-evidence/reset-2026-11-10" && sha256sum -c SHA256SUMS)
sqlite3 /tmp/factory-copy.sqlite 'PRAGMA integrity_check; PRAGMA foreign_key_check;'
```

## Verification in this repository

```sh
scripts/reset-test-data.test.sh
```

The shell test builds a real migrated SQLite database, seeds preserved and
transactional rows (including immutable-delete triggers and an
`AMBIGUOUS` model reservation), and asserts wrapper and direct-helper guards,
symlinked database/sidecar refusal, dry-run rollback, backup checksums, reset
cutover, trigger restoration, restored integrity, corrupt-backup refusal,
duplicate-checksum and path-traversal refusal, symlinked backup refusal,
automatic restore after an injected post-commit integrity failure, and
automatic restore after a simulated Core health failure. The health-failure
case uses a fake `podman` shim, a temporary database/volume directory and
reduced health-probe attempts; it never starts Compose, reads `/data`, or calls
a paid provider.

The model-call release/reconcile action itself is covered by Core/Office
regression tests; this script is not an alternative to the Decision Inbox
release action and does not silently release budgets on its own. A reset
deletes the whole model-call reservation row only after the database is
quiesced and backed up.
