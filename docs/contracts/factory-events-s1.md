# Contract: Factory Events (S1 frozen portion)

**Status:** Agreed for F1-S1. **Version:** `1`. The durable ledger is the
replayable source of truth for state the UI shows; UI state that cannot be
replayed from the ledger must not be rendered as fact.

## 1. Event schema (JSON)

```json
{
  "eventId": "hex UUID",
  "eventType": "CoreStarted | ActionCreated | ActionTransitioned",
  "schemaVersion": "1",
  "aggregateType": "core | action",
  "aggregateId": "core | <actionId>",
  "sequence": "per-aggregate monotonic, starts at 1",
  "globalSeq": "ledger commit order, starts at 1, gapless per committed prefix",
  "occurredAt": "RFC3339Nano UTC",
  "recordedAt": "RFC3339Nano UTC (Core authority)",
  "correlationId": "client-supplied X-Correlation-ID or Core-generated",
  "causationId": "reserved, empty in S1",
  "actor": "local-operator",
  "payload": "typed per eventType, redacted at the log boundary"
}
```

Payloads: `ActionCreated {projectId, title, description, status}`,
`ActionTransitioned {from, to}`, `CoreStarted {version}`.

## 2. Durability rules

- Business state and its event commit in **one SQLite transaction** (WAL
  mode asserted at startup; opening a non-WAL database is a fatal error).
- Events are never sampled, never deleted, never rewritten in S1.
- Every Core boot appends `CoreStarted` (`aggregate core/core`), so
  restarts are visible in the timeline and testable.

## 3. Replay transport

- `GET /api/v1/events?after=<globalSeq>&limit=<n>` returns ledger order
  with `nextAfter` cursor. Default limit 100, max 500.
- `GET /api/v1/events/:id/events` (`/api/v1/actions/:id/events`) returns
  one aggregate in `sequence` order.
- `GET /api/v1/events/stream?after=<globalSeq>` (SSE, `event: factory`):
  replays the missed suffix first, then pushes live events; `: ping`
  comments keep the connection alive. Reconnecting with the last seen
  `globalSeq` yields exactly the missed events — no gaps, no duplicates.
- The stream accepts the API token as `access_token` query only because
  `EventSource` cannot set headers; the fetch-based Office client sends it
  as a header instead. The token value is never logged.

## 4. Correlation and redaction

- Clients send `X-Correlation-ID`; the Core echoes it and stores it on
  caused events. Absent IDs are generated.
- Logs are JSON (`LOG_LEVEL` controlled) with secret shapes
  (`token=…`, `Bearer …`, registered secrets incl. the API token) masked
  before emission. Synthetic canaries must never appear in logs.
- Traces/metrics are best-effort OpenTelemetry: with no
  `OTEL_EXPORTER_OTLP_ENDPOINT` the provider is a no-op; with an
  unreachable endpoint, export fails in the background and business
  writes still succeed. Exporter health never gates persistence.

## 5. S1 acceptance (binding)

- Restart: identical Actions and complete per-aggregate sequences survive.
- Replay from any cursor returns a contiguous, ordered suffix.
- OTel exporter outage (bogus endpoint) does not lose business writes.
- Secret canary absent from logs after an Action carrying it is stored.
- `scripts/e2e-s1.sh` exercises every rule above against a real Core.
