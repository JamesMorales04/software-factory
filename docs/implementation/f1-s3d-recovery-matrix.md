# F1-S3-D failure and recovery matrix

> **Status:** design evidence for F1-S3-D. It describes behavior that the
> implementation must prove; it is not an acceptance verdict. Base:
> `baf7aa6` (S3-C merged, all five workflows green).

## 1. Inspected baseline

The matrix was derived from the actual S2/S3-A/S3-B/S3-C code at `baf7aa6`
and from the installed Paseo 0.9.1 CLI, not from the historical summary.

| Surface | Observed fact | Evidence |
| --- | --- | --- |
| Lease fencing | Core grants one Attempt per lease, increments `leaseGeneration`, rejects stale/expired/cancelled Worker messages | `core/internal/store/runs.go`, S2 contract |
| Restart reconciliation | `ReconcileOnBoot` expires every leased/running Attempt (`core-restart`) and requeues or fails the Run; it never resumes a generation | `core/internal/store/runs.go` `ReconcileOnBoot` |
| Binding | One immutable terminal binding per Attempt; terminal means `succeeded|failed|cancelled|orphaned` | `core/internal/store/paseo.go` |
| Orphan recording | Cancellation, expiry, timeout, Core restart and failed paseo reports orphan a live binding; Core never claims the external stop | `store/runs.go`, `store/paseo.go` |
| External identity | S3-B workspace/session titles are `factory-<8-char-run-id>`; no labels were set | `factory-worker/paseo_task.go`, `paseo/cli.go` |
| Workspace discovery | `paseo workspace ls --json` lists active workspaces with `workspaceId`, `name`, `cwd`, `project`, `isolation`; archived workspaces are absent | installed CLI, read-only probe |
| Session discovery | `paseo ls -g -a --json --label k=v` filters agents, including archived; output carries `id`, `name`, `status`, `cwd` | installed CLI help and package source (`commands/agent/ls.js`) |
| Session labels | `paseo run --label k=v` and `paseo agent update --label k=v` are supported; exact-match filtering is server/client checked | installed CLI and `@getpaseo/cli` source |
| Archived inspection | `paseo inspect <id> --json` still returns an archived agent and reports `Archived`/`ArchivedAt` | read-only probe of an archived agent |
| Live tool streaming | Not implemented (S3-C Level 3 NOT IMPLEMENTED) | S3-C contract |

Consequences:

1. The old short-run title is **not** a generation identity and cannot be
   used to assign an unbound workspace/session to an Attempt.
2. Session labels plus a full-run-and-generation title are the only
   supported, exact external correlation available in Paseo 0.9.1.
3. Workspaces have titles but no labels; the deterministic title is their
   only discoverable ownership marker. Bound workspace IDs remain the
   strongest ownership proof.
4. `paseo archive` is a soft delete, so `ls -a` still finds an archived
   session; absence from `-a` is the authoritative "not present" check.
5. There is no external atomic transaction with SQLite. Crash windows are
   handled by durable intent, exact identity and idempotent reconciliation.

## 2. Matrix

Legend for **Action**:

- **Observe** — inspect/list only.
- **Confirm** — stop and archive owned resources, verify externally, record
  a receipt.
- **Absent** — prove absence with two authoritative observations separated
  by the quiet period, record it.
- **Wait** — no authoritative change; retry within the bounded recovery
  budget.
- **New Attempt** — allowed only after the prior generation is `confirmed`
  or `absent`.
- **Blocked** — no safe automatic action; leave the case inspectable.

| # | Failure | Durable state before failure | Possible external state | Generation owner | Safe action | Evidence required | New Attempt? |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Worker graceful shutdown | Attempt running/leased; binding intended/active or none | Any; lifecycle's cancellation path attempts stop/archive | Old generation if lease still live, then Core expiry | Orphan via the normal path; recovery observes and confirms or proves absent | `PaseoSessionOrphaned`, recovery observation, terminal outcome | Only after cleanup/absence |
| 2 | Worker SIGKILL | Attempt running; binding intended/active; lease eventually expires | Session may be running, created but unbound, or absent | Core expiry orphans and increments on retry | Observe, validate exact identity, stop/archive; absence needs two observations | Lease expiry event, recovery receipt | Only after cleanup/absence |
| 3 | Core graceful restart | Runs/Attempts/bindings/events durable | Session may still run | `ReconcileOnBoot` expires the generation | Same as Core restart | Restart event, orphan event, recovery receipt | Only after cleanup/absence |
| 4 | Core crash/restart | SQLite WAL durable state | Session may still run | Superseded by restart reconciliation | Never reattach to a new generation; clean up old | Durable history equality before/after, fencing rejection of old generation | Only after cleanup/absence |
| 5 | Paseo daemon disconnect | Attempt running; binding intended/active | Unobservable at the moment | Lease may expire | Record `unconfirmed`, retry with backoff | Bounded `unconfirmed` receipt; no success/stoped claim | Only when prior generation resolved |
| 6 | Paseo daemon restart | Same as disconnect | Sessions may survive, be archived, or be lost | Same | Re-list/inspect after reconnect; confirm or prove absent | Post-reconnect observation; real daemon restart is BLOCKED in this environment | Same |
| 7 | CLI process death, daemon session survives | `paseo run` subprocess died; Core session may or may not be bound | Session may exist and run | Lease expiry supersedes | Discover by exact labels/title; stop/archive if identity matches | Exact label match + inspect identity + receipt | Only after cleanup |
| 8 | Crash before intent | Run leased/running, no binding | No Factory resource (no call made) | Core expiry | No external action; expiry requeues | No resource found; normal retry | Yes (no external resource) |
| 9 | Crash after intent, before workspace create | Binding `intended`, correlation key durable | Nothing created, or an in-flight CLI may still create | Superseded generation | Two absence observations before `absent`; if a resource appears, validate and clean | Quiet-period absence receipt | Only after absence |
| 10 | Crash after workspace create, before binding | Binding `intended`; workspace ID not persisted | Workspace may exist; session not started | Superseded | Find workspace by exact full title; verify unique; archive; prove absence | Workspace listing + exact title + post-archive absence | Only after cleanup/absence |
| 11 | Crash after session start, before binding | Binding `intended`; workspace ID maybe not persisted; no session ID | Workspace and session may exist | Superseded | Discover session by exact label set, workspace by title; validate route; archive; verify | Unique label match, inspect route, exact title, post-cleanup checks | Only after cleanup |
| 12 | Crash after binding while running | Binding `active`; IDs durable | Session running/idle | Superseded after expiry/restart | Inspect by ID, verify route/title, stop/archive, verify | Inspect identity + archived state/absence | Only after cleanup |
| 13 | Crash after session close, before report | Binding `succeeded`; Attempt still running | Session closed/possibly archived; workspace may remain | Lease expires/restart supersedes | The succeeded binding stays immutable; a cleanup receipt is scheduled on expiry, or on a completed report whose `cleanup` is unconfirmed | Recovery receipt; the succeeded result is preserved | Retry is allowed because the session is already closed; workspace cleanup proceeds independently |
| 14 | Lease expiry during active execution | Attempt running; binding active | Session may still run | Core expiry; new generation on retry | Orphan + recovery cleanup; retry gate blocks replacement until resolved | `RunExpired`, `PaseoSessionOrphaned`, recovery receipt | Only after cleanup/absence |
| 15 | Operator cancellation during startup | Run cancelled; binding intended/active | Session may be starting | Generation cancelled | Orphan + recovery observes; no claim of stop by Core | `RunCancelled`, `PaseoSessionOrphaned`, recovery receipt | No (Run terminal) |
| 16 | Operator cancellation during execution | Same as 15 with bound session | Session may still run | Cancelled | Same | Same | No |
| 17 | Stop timeout/failure | Orphaned binding; recovery claimed | Session may still run | No execution authority | Record `unconfirmed`, retry bounded; never `confirmed` | Stop/archive failure reason, retry count | Replacement blocked |
| 18 | Archive timeout/failure | Session stopped/archived; workspace present | Workspace may remain | No execution authority | Retry archive; verify absence before `confirmed` | Workspace listing after retry | Replacement blocked until resolved |
| 19 | Concurrent cleanup attempts | Orphaned binding, recovery row pending | Same external resource | One claim token only | Transactional claim; stale completion rejected; operations idempotent | One receipt, loser `stale_recovery_claim` | n/a |
| 20 | Duplicate recovery after restart | Recovery row terminal or claimed | Already cleaned or claim expired | Current claim token | Replay same completion is idempotent; expired claim is reclaimable; terminal rows are not re-run | One terminal receipt, bounded attempts | n/a |
| 21 | External session no longer discoverable | Orphaned binding | Absent, archived, or hidden by daemon failure | Superseded | Distinguish authoritative absence (successful listings) from infrastructure failure (`unconfirmed`) | Two successful absence checks vs CLI failure | Only after absence |
| 22 | Identity mismatch injected | Binding frozen route/IDs | Different session/workspace exists | Superseded | Do not stop/archive; record `blocked` with bounded reason | Mismatch observation, no destructive call | Replacement failed closed |
| 23 | Same external session offered to two Attempts | Existing binding owns the session ID | One live session | Original binding only | Reject bind/claim adoption; `stale_generation`/identity conflict | No second binding, no archive by the wrong Attempt | No |
| 24 | New Attempt while prior session uncertain | Run queued (attempt N+1) and prior orphan unresolved | Prior session may still run | Prior generation only | Hold the Run; after retries exhausted, fail the Run closed with an actionable reason | Queue hold, `recovery-unresolved` failure event | No |

## 3. Cases that must remain uncertain or blocked

- **Legacy unbound resources.** S3-B rows have no correlation key and no
  labels. If a legacy binding has no session/workspace ID, ownership cannot
  be positively established; recovery records `blocked` (or `unconfirmed`
  while infrastructure is unavailable) and takes no destructive action.
- **Ambiguous label matches.** More than one session with the exact full
  label set, or more than one workspace with the exact generation title,
  is `blocked`, never "pick the newest".
- **Identity drift.** Provider/model/mode or title mismatch is `blocked`;
  the recovery worker does not substitute or archive.
- **Daemon unavailable.** No stop/archive conclusion is drawn. The case is
  `unconfirmed` and retried within the bounded budget.
- **Absence during an in-flight creation window.** One successful empty
  listing is not enough. `absent` requires two successful observations
  separated by the quiet period, so a delayed external creation cannot be
  mistaken for permanent absence.
- **Exactly-once external effects.** Core fencing guarantees one
  authoritative Run result, not exactly-once external Paseo side effects.
  A stale CLI call may still have created a session; that session is
  discovered and cleaned, never promoted to authority.

## 4. Same-generation reattachment

The frozen S2 contract makes a generation non-resumable after lease expiry
or Core restart (`ReconcileOnBoot` expires it and every later retry gets a
new generation). S3-D therefore does **not** reattach a superseded session
to a replacement Attempt; doing so would weaken fencing. "Recovered" in
S3-D means the surviving generation was identified and reconciled safely
(cleanup confirmed or absence proven), not that it resumed authoritative
execution. A live Worker whose lease is still valid keeps its existing
S3-B in-process lifecycle; a transient observation failure there remains a
failed/orphaned Attempt, not a cross-generation resume.

## 5. Non-goals

- No autonomous LLM supervisor.
- No global session console or `paseo stop --all`/`--cwd` cleanup.
- No hard delete (`paseo delete`).
- No real daemon restart in automated tests; that case is BLOCKED unless an
  isolated daemon can be established safely.

## 6. Implementation evidence (local, pre-independent)

Local implementation evidence is recorded on branch `f1-s3d-paseo-recovery`:
`gofmt`/`go vet`/pinned `golangci-lint`/`go test -race`, Office
typecheck/lint/build, S1, S3-A, S3-B stub, S3-C browser and the S3-D
deterministic suite. Local S2 Podman acceptance is BLOCKED by the host
storage backend (overlay on btrfs); CI must supply the S2 evidence. This
section is implementer evidence, not independent verification.
