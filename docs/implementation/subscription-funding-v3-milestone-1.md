# Subscription funding V3 — Milestone 1

Milestone 1 adds typed subscription and billable funding classifications,
provider entitlement and quota observations, and durable subscription policy
evidence. Migration `0041_subscription_funding_policy.sql` stores the frozen
policy digest, authorization identity, and quota recovery state. Subscription
records are proposals in `DISABLED` state; quota exhaustion can persist a
`PAUSED_QUOTA` state tied to that authorization and can be read after database
restart.

The shared monetary ledger is unchanged. Subscription policy recording does
not create an envelope, debit, reservation, or execution authorization. Existing
V1/V2 authorization rows and ledger records remain in their original tables and
retain their existing constraints.

## Not implemented

Provider entitlement discovery, OpenCode Go and Codex route verification,
provider egress admission, model dispatch under subscription funding, resuming
after quota recovery, and PR #79 reconciliation are `NOT_IMPLEMENTED`. The
database record returns `enabled: false`; callers must continue using the
existing monetary authorization and execution admission paths until a later
milestone implements and independently verifies these controls.
