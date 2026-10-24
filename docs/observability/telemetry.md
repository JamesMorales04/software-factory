# Observability from the first slice

**Status:** S1 onward implements transactional Core events, structured redacted logs and an OpenTelemetry tracing path (`native/src/store/sqlite.go`, `native/src/telemetry/log.go`, `tracing.go`). The broader metric/event vocabulary below remains a proposed target; its full coverage is not certified. Observability began with S1.

## Three distinct planes

| Plane | Source of truth | Guarantee |
|---|---|---|
| Domain state and events | Core SQLite; state mutation and event committed in one transaction | Durable, unsampled, idempotent and replayable |
| Traces and metrics | OpenTelemetry with local exporter initially, optional OTLP | Diagnostic and optimizable; sampling/failure must not affect business state |
| Logs and artifacts | Structured redacted local logs, bounded artifacts with digests | Diagnostics, retention and access control |

A trace is **not** a delivery receipt; a log is **not** authoritative Action state. An external collector must not be a production dependency for correctness.

## Event identity and schema

Correlate appropriate fields: `traceId`, `spanId`, `projectId`, `actionId`, `campaignId`, `taskId`, `runId`, `attempt`, `workerId`, `agentId`, `providerSessionId`, `candidateDigest`, `policyRevision`. Missing means unknown, not an invented placeholder.

Conceptual `FactoryEvent` fields: `eventId`, `eventType`, `schemaVersion`, `aggregateType`, `aggregateId`, monotonic per-aggregate `sequence`, `occurredAt` from source (may be untrusted), authoritative `recordedAt` from Core, `origin`, `actor`, `correlation`, `causationId`, redacted typed `payload`, `evidenceRefs` and optional `payloadDigest`.

Persist business state and related Core-originated events atomically. Reconcile and deduplicate remote Worker observations by exact generation/attempt/source IDs. Do not claim global ordering across Workers merely from wall-clock timestamps. Browser clients restore snapshot + missed events from a cursor; message replay must not repeat effects.

## Incremental event vocabulary

`CoreStarted`, `CoreStopped`, `WorkerRegistered`, `WorkerHeartbeat`, `WorkerDisconnected`, `ActionCreated`, `ActionTransitioned`, `RunQueued`, `RunLeased`, `RunStarted`, `RunProgressObserved`, `RunCompleted`, `RunFailed`, `ProviderSessionBound`, `AgentTurnStarted`, `AgentTurnFinished`, `ToolCallObserved`, `ValidationStarted`, `ValidationFinished`, `CandidateRecorded`, `ReviewRecorded`, `DeliveryIntentCreated`, `DeliveryReceiptReconciled`, `PolicyDenied`, `IncidentObserved`.

Only implement event kinds justified by a current F1 slice. New event schemas require documented versions, replay/serialization tests, idempotency checks and redaction rules.

## Traces and metrics

Instrument HTTP and Core/Worker operations with parent/child spans and context propagation. Suggested names: `factory.action.create`, `factory.run.schedule`, `factory.worker.execute`, `factory.provider.launch`, `factory.agent.turn`, `factory.validation.run`, `factory.delivery.check`, `factory.delivery.merge`.

Begin with actual counters/histograms for Action outcomes, active/queued Runs, stage duration/errors, Worker wait/recovery time, retry/repair/first-pass rate, observed token/turn/cost usage with `KNOWN/PARTIAL/UNKNOWN` coverage, and resource consumption when available. Do not fabricate telemetry for unimplemented subsystems.

Avoid high-cardinality metric labels: `actionId`, `runId` and `sessionId` belong in trace/log correlation, not generic aggregated metric tags. Correlate structured logs to trace/span IDs. Prefer standard structured logging for Go until its logging SDK path is tested.

## Security, retention and reliability

No tokens, OAuth credentials, authorization headers, complete prompts/outputs, source code bodies or unnecessary personal data in telemetry. Redact **before** export, use role/project-scoped artifact access, rotate and bound local logs, and configure retention after measurement. Large artifacts are stored by reference/hash rather than inside events.

Local operation cannot depend on telemetry SaaS or OTLP connectivity. A failing exporter is reported as telemetry degradation, not a reason to fake success or lose Core state. Synthetic canary tests must confirm that sensitive values are not exported.

## F1 acceptance

1. Create an Action through UI/API; restart Core; both state and history persist.
2. State and event sequence agree; duplicate observations do not create duplicate effects.
3. Browser reconnect obtains missing events from a snapshot/replay boundary.
4. Core, Worker and provider can be correlated using observed IDs.
5. Disabling telemetry export leaves business behavior intact.
6. Synthetic secret values never appear in logs/traces/artifacts.
7. Unreported provider usage remains `UNKNOWN` or `PARTIAL`, never an invented zero.
