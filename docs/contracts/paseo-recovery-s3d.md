# Contract: Paseo recovery, reconciliation and cleanup (F1-S3-D)

**Status:** Agreed for F1-S3-D. **Version:** `1`. Experimental pre-release
contract; breaking changes update all in-repo callers, tests, docs and
fixtures atomically
([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

S3-D covers exactly the reconciliation slice below. It does not change S2
lease semantics, S3-A binding authority or S3-C read-only projections. Core
remains authoritative for Run/Attempt/generation state and every recovery
decision. Workers execute bounded external observations and idempotent
stop/archive operations. Paseo remains the owner of agent sessions.

The failure matrix and its evidence are in
[`f1-s3d-recovery-matrix.md`](../implementation/f1-s3d-recovery-matrix.md).

## 1. External ownership identity

A Factory-created attempt uses one deterministic correlation key:

```text
factory-s3d:<runId>:g<generation>
```

- It is stored durably on the binding as `correlationKey` at intend time.
- It is the exact Paseo workspace title and the exact session title.
- The session also carries these exact labels at creation:

| Label | Value |
| --- | --- |
| `factory.owner` | `software-factory` |
| `factory.run` | Run ID |
| `factory.attempt` | Attempt ID |
| `factory.generation` | Decimal lease generation |
| `factory.workspace` | Paseo workspace ID |

Only exact matches count. Recovery never assigns a resource by prefix,
case-insensitive title, short Run ID or "newest" heuristics.

Legacy S3-B rows carry an empty `correlationKey` and the old
`factory-<8-char-run-id>` title. A bound legacy session/workspace may be
cleaned by its recorded ID, with the old title checked as an additional
ownership marker. An **unbound** legacy row has no positive external
identity and stays `blocked`/`unconfirmed`; it is never matched by a loose
title.

Paseo 0.9.1 evidence for these capabilities:

- `paseo run --label key=value` and `paseo agent update --label`;
- `paseo ls -g -a --json --label key=value` exact filtering, including
  archived agents;
- `paseo workspace ls --json` active workspace listings (`workspaceId`,
  `name`, `cwd`);
- `paseo inspect` reports archived state;
- `paseo workspace create --title` and `paseo run --title` set the
  discoverable title.

## 2. Recovery record

One recovery record exists per cleanup target. The primary target is an
orphaned binding. A succeeded binding also gets a record when its post-close
cleanup was reported unconfirmed or the Worker died before reporting; in
that case the authoritative succeeded result is preserved and only external
cleanup is verified. The record is an operational record, not a second
source of truth for the binding or Run.

```json
{
  "attemptId": "ab…",
  "runId": "9f…",
  "leaseGeneration": 2,
  "status": "pending | claimed | unconfirmed | confirmed | absent | blocked",
  "claimOwner": "local-1 (empty when not claimed)",
  "claimExpiresAt": "RFC3339Nano UTC or empty",
  "attempts": 1,
  "nextAttemptAt": "RFC3339Nano UTC or empty",
  "lastObservedAt": "RFC3339Nano UTC or empty",
  "sessionId": "observed or recorded session identity",
  "workspaceId": "observed or recorded workspace identity",
  "sessionObserved": "absent | archived | active | idle | unknown",
  "workspaceObserved": "absent | present | unknown",
  "reason": "bounded redacted reason or empty",
  "createdAt": "RFC3339Nano UTC",
  "updatedAt": "RFC3339Nano UTC"
}
```

`claimToken` is returned only to the claiming Worker; it is not part of the
operator projection.

Outcome meaning:

- **confirmed** — the owned session is archived or absent, the owned
  workspace is absent from the active listing, and the post-archive checks
  still hold. Destructive success is never inferred from a Core row alone.
- **absent** — two successful authoritative listings separated by the
  quiet period find neither session nor workspace. This is the only basis
  for "already absent".
- **unconfirmed** — identity/state could not be established because of a
  transient failure (daemon/CLI unavailable, stop/archive not verified).
  Retryable within the bounded budget.
- **blocked** — an identity mismatch/ambiguity, ownership conflict, unsafe
  legacy row or exhausted retry budget prevents automatic action. No
  destructive call is made for identity/ambiguity cases.

Terminal recovery outcomes are `confirmed`, `absent` and `blocked`.
`unconfirmed` is retryable until the retry budget is exhausted, at which
point it becomes `blocked` with an explicit `retry-exhausted` reason.

## 3. Claim and completion protocol

Worker routes use the scoped Worker Bearer token; operator routes use the
operator token. Worker tokens never grant the operator projection.

| Method | Route | Auth | Meaning |
| --- | --- | --- | --- |
| POST | `/api/v1/workers/recovery/claim` | worker | `{workerId}` → one orphaned binding claim (`200`) or no work (`204`) |
| POST | `/api/v1/workers/recovery/complete` | worker | `{workerId, attemptId, claimToken, outcome, sessionId?, workspaceId?, sessionObserved?, workspaceObserved?, reason?}` → recorded receipt or idempotent replay |

Claim:

- eligible: binding `status=orphaned`, or `status=succeeded` with a recovery
  record created for unconfirmed/missing post-close cleanup, and the recovery
  record is `pending`,
  retryable `unconfirmed` with `nextAttemptAt <= now`, or `claimed` with an
  expired claim;
- one transaction sets `status=claimed`, `claimOwner`, a fresh random
  `claimToken`, `claimExpiresAt = now + 60s` and `attempts = attempts + 1`;
- terminal `confirmed`/`absent`/`blocked` records are never claimed again;
- concurrent Workers cannot receive the same live claim.

Completion fencing (no state change, no new event):

- unknown Worker/binding/recovery → `404 not_found`;
- claim owner/token mismatch, or an expired claim → `409
  stale_recovery_claim`; the stale Worker must abandon the receipt and let
  the current claim re-observe;
- completion after a terminal receipt with the same token/outcome →
  `200 OK`, `Idempotent-Replayed: true`, no duplicate event;
- confirmed/absent evidence that names a session/workspace bound to another
  Attempt, or contradicts the binding's recorded identity → `409
  recovery_identity_conflict`, no state change;
- malformed or unknown observation values → `400 invalid`.

A claim is a cleanup lease, not a Run lease. It never changes the Run,
Attempt, binding status, generation or `leaseExpiresAt`.

## 4. Recovery algorithm (Worker)

1. Claim one orphaned binding.
2. Compute the exact correlation key, labels and expected legacy title.
3. If a session ID is recorded, inspect it. If no ID is recorded and a
   correlation key exists, find sessions by the exact label set. More than
   one match is `blocked`.
4. Validate observed provider/model/mode against the frozen binding route and
   validate the exact title/legacy marker. A mismatch is `blocked`; never
   stop or archive it.
5. If a workspace ID is recorded, verify that workspace's listing. If no ID
   is recorded and a correlation key exists, find the workspace by the exact
   title. More than one match is `blocked`.
6. For an owned active session: `stop`, `archive`, then inspect and require
   archived/absent. For an owned active workspace: `archive`, then require
   it is no longer listed. Re-check the exact session labels so a session
   created during the cleanup window is not missed.
7. Classify:
   - session archived/absent and workspace absent → `confirmed`;
   - both absent on two authoritative checks separated by the quiet period
     → `absent`;
   - transient failure → `unconfirmed` with bounded backoff;
   - identity/ambiguity/ownership conflict → `blocked`.
8. Complete the claim with the observation. The receipt is recorded in the
   recovery row and one `PaseoSessionRecoveryRecorded` event.

The Worker never uses `paseo stop --all`, `paseo stop --cwd`, `paseo
delete`, or a title with a short/derived Run ID for ownership.

## 5. Retry gate and fencing

- A queued `paseo-task` Run with a prior binding that is not safely settled
  (`orphaned`/unresolved) is **not leased** to a replacement Attempt. It
  waits while recovery is `pending`/`claimed`/retryable `unconfirmed`.
- If the prior recovery becomes `blocked` (including retry exhaustion), the
  Run fails closed with a bounded `recovery-unresolved` reason instead of
  starting concurrent coding work. Other Runs and projects continue.
- A binding whose recovery is `confirmed` or `absent` permits a new
  Attempt, which always receives a new generation and a new correlation
  key.
- Recovery never binds, closes or rewrites an `orphaned` or `succeeded`
  binding. The
  original orphan event and reason remain immutable; the recovery event is
  additional history.
- A delayed stale Worker message is rejected by the existing S2 fencing;
  its external cleanup must use idempotent stop/archive and cannot create a
  second authoritative result.

## 6. Events

One event type is added, on the existing `run/<runId>` aggregate and the
existing durable ledger:

`PaseoSessionRecoveryRecorded`

Bounded payload:

```json
{
  "attemptId": "ab…",
  "seq": 2,
  "generation": 2,
  "workerId": "local-1",
  "outcome": "confirmed | absent | unconfirmed | blocked",
  "sessionId": "optional observed identity",
  "workspaceId": "optional observed identity",
  "sessionObserved": "absent | archived | active | idle | unknown",
  "workspaceObserved": "absent | present | unknown",
  "retryCount": 1,
  "reason": "bounded redacted reason"
}
```

No prompts, transcripts, raw CLI output, tool bodies, credentials or source
contents are persisted. Reason text uses the existing shape redaction and
length bound.

## 7. Read projections and Office

`GET /api/v1/sessions` and `GET /api/v1/runs/{id}/sessions` may include an
optional `recovery` object for a binding. It contains the operator-safe
fields of §2 but never `claimToken`.

Office shows, for an orphaned binding:

- owning Run, Attempt sequence and generation;
- cleanup status (`pending`, `claimed`, `unconfirmed`, `confirmed`,
  `absent`, `blocked`);
- last trustworthy observation time;
- bounded reason and retry count.

Office never labels `orphaned` as running, never renders `confirmed` as
proof of exactly-once external side effects, and never invents a cleanup
state when no recovery record exists yet.

## 8. Bounds and compatibility

- Claim TTL 5 minutes; retry attempts at most 8; exponential backoff capped at
  5 minutes; absence quiet period 10s.
- Recovery limits are per orphan and never block unrelated Runs.
- S2 synthetic Runs, S1 events, S3-A binding lifecycle, S3-B Worker
  lifecycle and S3-C projections keep their existing behavior. The only
  new dispatch rule is the `paseo-task` retry gate above.
- Real daemon restart testing is BLOCKED unless an isolated test daemon is
  available; the deterministic simulator covers the state machine. A
  simulated success is not a claim of real-daemon recovery certification.

## 9. S3-D acceptance

- D1 Worker SIGKILL: expiry, fenced old generation, discovery or explicit
  unconfirmed/blocked outcome, safe cleanup attempt, no stale success.
- D2 Core restart: Runs/Attempts/bindings/events survive, S2 fencing holds,
  no superseded generation is adopted, cleanup continues from durable state.
- D3 daemon disconnect/restart simulator: reconnect, repeated inspection,
  bounded retries; real daemon restart reported BLOCKED.
- D4 unbound session: identification by durable correlation, never by
  guessed ownership.
- D5 concurrent recovery: one authoritative receipt, no cross-cleanup.
- D6 repeated recovery: idempotent after success, timeout and restart.
- D7 wrong identity: reject/block without destructive action.
- D8 S1/S2/S3 regressions green.
- Office browser journey shows pending/confirmed/blocked recovery truth.
- At most one bounded real provider session is used for final acceptance;
  no automatic retry of a paid session.
