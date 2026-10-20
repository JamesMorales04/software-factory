# Contract: Agent observability projection (F1-S3-C)

**Status:** Agreed for F1-S3-C. **Version:** `1`. Experimental pre-release
contract; breaking changes update all in-repo callers, tests, docs and
fixtures atomically ([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

S3-C covers exactly the read-only observability slice below. It adds **no**
agent runtime, event ledger, lifecycle authority or external effect. Factory
Core remains authoritative for Runs, Attempts, lease generations, session
bindings, policy, results and events. Paseo remains the owner of real agent
sessions and native timelines. Factory Office only projects Core-owned state.

## 1. Sources of truth

| Projection | Authoritative source |
| --- | --- |
| Session identity, routing, binding status | `paseo_bindings` (S3-A) |
| Run status and requested route | `runs` |
| Attempt history | `attempts` |
| Lifecycle timeline | durable `events` ledger (S1 transport) |
| Bounded final evidence | Worker result persisted by the S2/S3-B report path |
| External session state | Paseo only; Core records observations, never remote truth |

`orphaned` means a Paseo session may still exist externally while its
generation is superseded or its outcome is unconfirmed. It is never proof
that the session is running.

## 2. Cross-Run session projection

`GET /api/v1/sessions?status=&provider=&limit=` (operator Bearer token).

```json
{
  "sessions": [
    {
      "attemptId": "ab…",
      "runId": "9f…",
      "seq": 1,
      "workerId": "local-1",
      "leaseGeneration": 1,
      "provider": "codex",
      "model": "gpt-6-luna",
      "mode": "full-access",
      "execMode": "trusted-local",
      "workspaceId": "ws_…",
      "sessionId": "…",
      "status": "intended | active | succeeded | failed | cancelled | orphaned",
      "error": "bounded redacted reason or empty",
      "createdAt": "RFC3339Nano UTC",
      "updatedAt": "RFC3339Nano UTC",
      "runStatus": "queued | leased | running | succeeded | failed | cancelled"
    }
  ],
  "eventCursor": 42
}
```

- `status` is an exact binding-status filter; an unknown value is `400`.
- `provider` is an exact, length-bounded filter; unknown providers return an
  empty page, not an error.
- `limit` is `1..200`, default `100`. The projection is bounded; a client
  narrows with filters or opens a Run for older history.
- Ordering is live bindings (`intended`, `active`) first, then most recently
  updated history.
- `eventCursor` is the committed ledger head at snapshot time. Clients start
  the existing authenticated SSE stream from it, so live updates do not
  require replaying the entire ledger.
- An empty result is `[]`, never `null`. Worker tokens are rejected (`401`).
- No prompt, transcript, tool output, provider credential or source body is
  part of this projection.

## 3. Office projections

### Agents

The Agents view renders real recorded bindings, grouped into live and
historical sections. Each card shows provider/model/mode, the exact
`TRUSTED_LOCAL` label, binding status, Run link and status, Attempt sequence
and ID, lease generation, Worker, session/workspace identity and timestamps.
Status/provider filters are server-backed. Loading, empty, error and
disconnected states are explicit. A disconnected view shows the last loaded
snapshot and never implies that an agent stopped.

### Run detail

For `paseo-task` Runs, Run detail renders every Attempt's session binding
separately; generations are never collapsed. It shows requested versus
observed route when final evidence recorded one, session/workspace IDs,
Worker, execution mode, timestamps, binding reason, bounded evidence and
orphan warnings. A Core-cancelled Run with an unconfirmed external session
is described as orphaned, never as externally stopped. Runs with no binding
say so accurately; synthetic Runs state that no Paseo session applies.

## 4. Evidence levels

**Level 1 — authoritative lifecycle:** `RunQueued`, `RunLeased`,
`RunStarted`, `PaseoSessionIntended`, `PaseoSessionActive`,
`PaseoSessionSucceeded/Failed/Cancelled/Orphaned`, and terminal Run events,
read from the durable ledger in `globalSeq` order. Reload and reconnect use
the existing cursor/replay transport.

**Level 2 — bounded final session summary:** counts already persisted by
S3-B (timeline lines/steps, tool-step count), source diff stat/bytes,
format/test exit codes, nudge count and cleanup confirmation. Office labels
this as a final summary observed at session close, not individual live tool
events.

**Level 3 — live individual tool activity: NOT IMPLEMENTED in S3-C.**
Paseo 0.9.1 exposes curated `logs --follow`/`attach` text and a typed
`@getpaseo/client` timeline subscription. The current supported Worker
integration is the Go CLI adapter, whose supported observation path is the
curated text/flags surface; live ingestion from it would require a new
typed client/runtime dependency plus a reconnection, replacement and
idempotent-ingestion contract. S3-C does not claim live per-tool streaming,
does not infer individual edits from a final diff, and does not fabricate
timestamps. Any expansion requires explicit authorization and a new contract
revision.

## 5. Security and redaction

- `TRUSTED_LOCAL` is owner-authorized local execution with broad host
  access. Office never labels it isolated, sandboxed, certified or SEC-01
  PASS.
- S3-C stops persisting the S3-B `diffExcerpt` source body. Event read/SSE
  projections recursively drop any legacy `diffExcerpt` key without
  rewriting the durable ledger. Office renders a whitelist of lifecycle
  fields and never dumps event payload JSON.
- Session reason/error text keeps the S3-A bounded, shape-redacted
  persistence rule.
- Preexisting S3-B run/attempt result snapshots may retain a legacy
  `diffExcerpt`; Office does not render result bodies or source excerpts.
  Removing that historical field requires an authorized data-retention
  decision, not an S3-C rewrite.

## 6. Compatibility and verification

Per ADR-0006 this is an additive read-only projection with no compatibility
shims or dual state. Acceptance covers: the real Core/Worker lifecycle
through a deterministic Paseo CLI stub; live Agents appearance; provider/
model/mode/Worker/Attempt/generation identity; Run-detail evidence; reload
and malformed-SSE reconnect without duplicates; cancellation/orphan
wording; Core-restart durability; scoped authorization; invalid filters; and
the existing S1/S2/S3-A/S3-B suites.
