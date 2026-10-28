# Supported Compose acceptance — 2026-10-28

**Status:** PASS for the deterministic legacy Action/Campaign lifecycle on the
supported GitHub-hosted rootless Podman runner. This is **not** the unified
intake journey yet, not live inference and not independent acceptance.

## Run identity

| Item | Value |
|---|---|
| Workflow | `Compose end-to-end (manual)` |
| Run | https://github.com/JamesMorales04/software-factory/actions/runs/40738776286 |
| Implementation candidate | `770a200c6bf164c7a9bb0202ac2948511a246bfb` on `campaign/office-ux-compose-fix-20261028` |
| Base code | `campaign/office-ux-baseline-20261028` at `de7e2b7` plus the Compose isolation fix |
| Candidate ID | `0df6eed6-1377-46d5-b4ff-7c11c437e3ef` |
| Attempt ID | `3f7a92a2-b296-4ed2-a87f-7830764cd005` |
| Quality Gate | `ACCEPTED` |
| Restart persistence | `true` |
| Paid inference | `false` (fixture provider) |

## What ran

The manual workflow installed rootless Podman, checked out the exact candidate,
and executed `scripts/compose-e2e.sh` with `--project-name` isolation. The real
Compose stack started Core/Office, Worker, Supervisor, the nested runtime engine
and broker, independent verifier and reviewer, the Git fixture, and a durable
SQLite volume. It exercised:

- project import from the disposable Git fixture;
- frozen Action checks;
- scoped delegation and Campaign authorization;
- Worker lease/Attempt and candidate creation;
- independent verifier and reviewer receipts;
- deterministic Quality Gate `ACCEPTED`;
- `compose down` / `up -d` restart and persistence of Project, Worker and Gate.

The script printed the PASS summary quoted above. No paid provider session was
started.

## Isolation fix

The first CI run failed because `compose.yaml` declares `name:
software-factory`, which overrode the E2E project guard. The script could have
targeted the owner's project name and failed its container lookup. The fix
forces `podman compose -p "$PROJECT"` on every invocation and prints container
diagnostics on failure. CI then passed. This was a real safety defect, not a
test-only cosmetic failure.

## Limitations

- Legacy Action/Campaign path only. Unified intake through Compose is a separate
  in-progress lane.
- Deterministic fixture provider/model; **no live inference**.
- Author-run CI, not an independent verifier or Product Owner UAT.
- Local Compose remains blocked on this host by the buildah/btrfs context-overlay
  limitation; CI is the supported environment for this evidence.
