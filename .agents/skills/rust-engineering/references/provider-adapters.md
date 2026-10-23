# Provider adapter reference

Codex App Server (JSON-RPC over stdio) and OpenCode (HTTP/OpenAPI/SSE) are
directly integrated from Rust. No Node bridge, Paseo runtime or Go adapter is
part of the target architecture.

## Common contract

An adapter exposes typed operations for:

- preflight/health and capability discovery;
- session/thread creation with exact workspace and model identity;
- prompt/turn submission;
- progress and tool/permission events;
- terminal outcome;
- inspection/reconnect;
- abort/cancel;
- cleanup with an honest receipt.

Every adapter reports provider, model, session identity, workspace identity,
attempt identity and generation. Unknown usage is unknown, never zero.

## Codex App Server

- JSON-RPC request IDs must be correlated exactly; do not match on method name
  alone.
- A response to `thread/start`, `turn/start` or a stream open is not success.
- Success requires a terminal `turn/completed` event with `status=completed`,
  correlated to the started turn.
- Permission requests fail closed unless Core policy explicitly allows the
  action. Record the decision and scope.
- Cancellation sends an interrupt request; it is not proof the turn stopped.
  Reconcile thread/turn state before retry or cleanup.
- `error`, disconnect, EOF or timeout means unknown or failed, never completed.
- Thread cleanup must be confirmed by a supported path. Absence from a newly
  spawned server's state index does not prove the original server cleaned up.
  Keep the identified unresolved thread
  `01a169da-763d-7240-a953-6937793f8360` visible until conclusively resolved or
  classified.

## OpenCode HTTP/SSE

- Start the server on an ephemeral loopback port and read its actual bound
  address. Use an authenticated per-process credential.
- Validate health/OpenAPI version before use; a schema mismatch is a protocol
  incompatibility.
- Correlate SSE events to the intended session. Keep-alives are not progress.
- Creation acknowledgement is not task completion. Reconcile session status
  and message state after events, disconnects and reconnects.
- Validate that the session workspace matches the authorized canonical
  workspace.
- DELETE plus a subsequent GET absence is cleanup evidence for that session.
  Do not claim cleanup for an unqueried process.

## Tests and evidence

Fixture transcripts must cover request correlation, terminal success, failure,
permission denial/approval, malformed/partial frames, disconnect, reconnect,
duplicate events, cancellation, timeout and cleanup.

A no-paid probe or fixture is `LIVE_NOT_TESTED` for coding compatibility. A live
turn requires a bounded, explicit Product Owner authorization with provider,
model, route, session/attempt budget and exact candidate. Record session IDs,
token usage if reported, cleanup outcome and unresolved state. Never fabricate
a model result, cost, identity or cleanup claim.
