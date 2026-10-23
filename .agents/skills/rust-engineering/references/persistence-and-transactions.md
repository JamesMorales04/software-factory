# Persistence and transaction reference

## Transaction boundary

One authoritative operation commits together:

- the state transition;
- the audit event;
- the idempotency record (operation + canonical payload digest);
- the budget/lease/counter debit;
- the immutable evidence identity.

If any part fails, roll back. Do not emit an event or debit a budget outside
the transaction that justifies it.

## SQLite/WAL rules

- Enable WAL and a bounded `busy_timeout` at open.
- Use `BEGIN IMMEDIATE` for claims, reservations and any read-modify-write that
  must serialize writers.
- Keep transactions short. Never hold one across a provider call, network
  request, process wait or `.await`.
- Use `PRAGMA foreign_keys = ON` and explicit constraints for referential
  invariants.
- Store timestamps consistently and inject the clock where tests need control.
- Migrations are versioned and append-only. Do not rewrite existing rows or
  reinterpret old evidence silently.

## Idempotency

- Key = caller operation + stable idempotency token.
- Store the canonical request digest with the key.
- Same key + same digest: return the recorded result.
- Same key + different digest: typed conflict; never overwrite or replay the
  wrong effect.
- Receipts have their own identity so duplicate delivery is a no-op, not a new
  transition.

## Concurrency and overflow

- Checked arithmetic for generations, attempts, time budgets and counters.
- Reject overflow with a typed error; do not wrap.
- Compare lease owner, token digest and generation in the same statement that
  mutates state.
- A stale writer must affect zero rows and receive a typed refusal.

## Recovery

On startup, reconstruct:

- ready and in-flight work;
- leases that expired while offline;
- receipts that were durably written but not acknowledged;
- candidates referenced by an Attempt;
- external sessions and workspaces whose terminal state is unknown.

Recovery is deterministic and idempotent. It must not create a second live
execution for an unresolved external session. Prefer blocking retry with an
explicit unresolved state over duplicate paid work.

## Tests

Use the real SQLite engine (temporary file or in-memory with the same pragmas),
not a mock. Test rollback, concurrent writers, idempotent replay, conflicting
replay, stale generation, crash/restart and migration from every supported
schema version.
