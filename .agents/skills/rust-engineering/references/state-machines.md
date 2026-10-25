# Core/Worker state machines reference

Core is authoritative. Worker reports are proposals that Core validates against
durable state, lease ownership and generation.

## Invariants

- One authoritative scheduler and one Campaign/Run/Attempt lifecycle.
- An Attempt belongs to exactly one Run and one Worker lease generation.
- A lease grants execution for a bounded time and generation.
- Only Core persists terminal outcomes.
- Duplicate delivery is idempotent; conflicting duplicate payloads are
  rejected.
- A stale generation can never mutate state, candidate identity or evidence.
- Cancellation and expiry are terminal for that generation; a late report is
  retained as evidence but not applied.
- A candidate is valid only for the exact successful Attempt and frozen base.

## Suggested transition shape

```text
Campaign: created -> ready -> running -> (completed | failed | canceled)
Run:      created -> dispatched -> (completed | failed | canceled | expired)
Attempt:  leased -> running -> (succeeded | failed | canceled | expired)
Lease:    granted -> (renewed -> granted | expired | released)
```

Use the actual contract's states; this sketch is not a second specification.

## Implementation rules

- Encode transitions as functions over typed state, returning a typed refusal
  for an invalid transition.
- Persist the state change, event, idempotency receipt and any budget debit in
  one transaction.
- Claim/reserve with `BEGIN IMMEDIATE` or an equivalent serialized write path.
- Compare `(attempt_id, worker_id, generation, lease_token_digest)` atomically.
- Use checked increments for generation and counters; reject overflow.
- Bind a candidate claim to the Attempt, generation, frozen base, tree digest
  and allowed paths.
- Reject a candidate from a failed, canceled, expired or superseded Attempt.
- Reconcile on restart: reconstruct ready work, expired leases, unknown
  external sessions and orphaned workspaces. Never assume a crash stopped an
  external process.
- Model permission decisions in Core; deny by default when no policy decision
  exists.

## Required negative tests

- Claim with another Worker's credential.
- Two concurrent claims; exactly one wins.
- Heartbeat/report after lease expiry or generation increment.
- Report with a mismatched attempt, worker, generation or receipt payload.
- Same idempotency key, different payload.
- Cancellation racing with a late success.
- Duplicate terminal receipt.
- Core restart between lease grant and first heartbeat.
- Worker restart with an unknown lease.
- Candidate with wrong parent, forged SHA, empty diff or out-of-scope path.
- Budget exhaustion during reservation.
