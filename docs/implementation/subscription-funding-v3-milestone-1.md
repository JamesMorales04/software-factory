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
database record returns `enabled: false`. Current Owner policy permits only
verified subscription-included inference; historical monetary envelopes and
quotes never substitute for subscription verification or enable paid fallback.
Core and Worker model dispatch remain blocked until the trusted evidence and
request/Campaign admission boundaries are implemented and independently verified.

## Follow-up: deterministic refusal at both dispatch boundaries (2026-10-29)

Core model-call reservation/dispatch and Worker turn admission/egress binding
share typed subscription refusal reasons. Existing API error responses retain
`error` and add `code` for these refusals:

| Code | HTTP status | Meaning |
|---|---|---|
| `SUBSCRIPTION_FUNDING_BINDING_REQUIRED` | 503 | No verified request/Campaign subscription binding exists. |
| `SUBSCRIPTION_FUNDING_VERIFICATION_REQUIRED` | 503 | A persisted proposal exists but execution remains disabled. |
| `SUBSCRIPTION_FUNDING_EVIDENCE_INVALID` | 403 | Stored evidence shape or dispatch role is invalid. |

These are refusals, never authority or advice to retry a model call. Worker
checks subscription funding after active session/lease/generation validation
and before historical monetary-envelope/pricing prerequisites. Repeated refusal
creates no monetary reservation or egress binding. V2 retains a blocking
progress hold with the subscription refusal reason to prevent repeated
assignment; expired-reservation reclamation happens only after funding admission.
Existing unknown-outcome settlement/reconciliation remains available without
creating new dispatch authority. The existing Core dispatch-intent fence and
Worker egress fence both recheck funding.

Office impact: existing error surfaces receive the safe reason through the
same API operations; no new operator action, route, or successful subscription
state is introduced. Dedicated subscription UI and real browser acceptance
remain NOT_VERIFIED. Pre-Project request admission is still NOT_IMPLEMENTED;
provider entitlement observations alone cannot justify creating intake resources.
No live inference, provider proof, positive USD cap or paid fallback is enabled.

Author validation used the canonical Compose development service from this
worktree with development image
`sha256:43cebaa1770f687c7b25edf457db0e0a54207ff04112a28b1e8fb1325ce69bdc`:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked -p factory-native --lib --bin factory-core --bin factory-worker --all-features -- -D warnings`: passed.
- `cargo test --locked -p factory-native --lib db::provider_sessions::`: 33 passed,
  including no reservation/egress, one idempotent V2 blocking hold, repeated
  assignment refusal, infrastructure-error rollback, stale lease/generation,
  unknown usage and persisted ledger settlement checks.
- `cargo test --locked -p factory-native --lib db::model_calls::`: 10 passed.
- `cargo test --locked -p factory-native --lib subscription_funding_proposal_never`:
  1 passed.
- `cargo test --locked -p factory-native --bin factory-core subscription_funding_refusals`:
  1 passed, covering all three stable refusal codes and statuses.

All commands above ran via
`podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev`.
No provider call was authorized or performed. Full-workspace checks, runtime
stack/browser acceptance and independent verification were NOT_RUN for this
bounded slice; these containerized regressions do not certify product behavior.
The temporary development containers used `--rm` and exited after their jobs.
