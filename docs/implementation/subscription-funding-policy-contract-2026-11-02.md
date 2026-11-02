# Subscription-only funding policy — architecture contract (2026-11-02)

Source: GPT-6.1 Sol critical-architecture review against Wave 3 candidate
`c7016b5`/`f7479c0`. This is a proposed implementation contract; runtime
provider entitlement and quota behavior remain **NOT_VERIFIED**.

## Typed boundary

`native/src/execution_policy.rs`:

```rust
enum FundingPolicy {
    SubscriptionIncluded { binding_id: String, binding_sha256: String },
    Billable { envelope_id: String },
}
enum SubscriptionRoute { OpenCodeGo, CodexChatGptPlan }
enum Observation<T> { Verified { value: T, evidence_id: String }, Unknown { reason: String } }
enum FallbackState { Disabled, Enabled, NotApplicable }
struct SubscriptionBinding {
    provider: String, model: String, provider_revision: u32,
    route: SubscriptionRoute, account_fingerprint: String, plan: String,
    entitlement: Observation<bool>,
    extra_usage: Observation<FallbackState>,
    balance_fallback: Observation<FallbackState>,
    no_charge_route: Observation<bool>,
    observed_at_ms: i64, valid_until_ms: i64,
}
enum QuotaState { Available, Exhausted { resets_at_ms: Option<i64> }, Unknown { reason: String } }
```

Freeze funding in `ExecutionPolicyV3`; keep V1/V2 only for historical decode.
The V3 policy replaces the monetary-envelope field with `funding` and retains
scope, authorization, progress and operation limits. Persist an immutable
request funding grant before assessment; the Campaign authorization references
the same grant/binding. Bind every role/model operation to its exact
provider/model subscription binding.

## Admission

- Subscription mode must not require a positive USD authorization, create a
  zero-dollar envelope, use the fixture sentinel, select ledger V1 or invent a
  quote.
- Admission requires durable, redacted evidence of provider/account, plan,
  exact model entitlement, subscription route, fallback settings and freshness.
- Record quota observation independently, including explicit unknown values,
  source and reset information.
- A bare environment declaration does not prove Extra Usage is off or that the
  route cannot debit balance/paid API.
- Revalidate before every dispatch/retry; expired evidence, changed provider
  revision/account or model mismatch blocks admission.
- `Billable` returns `PAID_MODE_DISABLED` under the current policy even with a
  valid envelope. Future paid enablement requires explicit policy authorization
  and the existing #73 quote/reservation/settlement path.

## Usage and quota

Persist separate fields for entitlement, quota observation, measured token
classes, list-rate equivalent (rate revision/provenance), provider-reported
actual charge (`Observation<UsdMicros>`), and unknown. Actual zero is valid only
with explicit charge evidence. Missing quota is never unlimited; missing charge
is never zero.

Quota exhaustion records durable `PAUSED` with
`SUBSCRIPTION_QUOTA_EXHAUSTED`, preserves workspace/candidate/work/session
references and freezes authority; pause new dispatch, reconcile in-flight
outcomes, resume through an authenticated Core operation after refreshed
quota/auth evidence. Never replay ambiguous operations or switch provider,
model, account, credential or funding mode.

A reported positive charge on a subscription operation is an incident and blocks
further dispatch while preserving evidence.

## Fail-closed rules

- Extra Usage/balance enabled, unknown, stale or unverifiable: refuse.
- Codex API-key/pay-as-you-go route: refuse; never fallback.
- Entitlement unknown/false/expired/mismatched: refuse.
- Provider/account/model/revision differs from frozen binding: refuse.
- ChatGPT-plan auth unavailable: pause/block and request reauthentication.
- No-charge route unverifiable: refuse.
- Quota exhausted: durable recoverable pause.
- Transport/terminal ambiguity: preserve reconciliation state; no redispatch.

## Persistence and implementation

- `execution_policy.rs`: funding types, V3 policy, strict validation and typed
  admission failures.
- `inference_ledger.rs` / `db/inference_ledger.rs`: explicit
  subscription/billable classification; preserve #73 monetary arithmetic.
- `db/provider_settings.rs`: bind observations to settings revision/account;
  invalidate readiness when either changes.
- `db/authorization.rs`: freeze V3 funding with unchanged scope/risk/tools/
  route/delegation checks.
- `db/model_calls.rs`, `db/provider_sessions.rs`, `model_gateway.rs`:
  funding-aware admission, operation binding, usage provenance, pause/recovery.
- `bin/core.rs`: funding-aware request/assessment/approval APIs, recovery and
  truthful projections.
- Migration **0041_subscription_funding_policy.sql** (if unallocated): extend
  policy/authorization constraints for V3, allow absent monetary envelope only
  for typed subscription funding, add immutable funding/binding observations and
  operation accounting with explicit unknowns. Preserve 0034–0040, historical
  rows/hashes and FK/immutability.
- Harness: `.sh`, `.mjs`, `.test.mjs` reject paid modes, remove spend-cap
  requirement and consume Core verification evidence.
- Office: subscription plan/provider/model, verification freshness, quota/reset,
  contextual usage; no mandatory spend form for subscription mode; distinguish
  list-rate from actual/unknown charge; keep billable controls disabled.

## Tests

No-USD-cap admission across assessment/approval/worker/reviewer/evaluation;
enabled/unverified fallback, paid route, missing auth, entitlement mismatch,
stale evidence and provider substitution -> zero dispatches; exhaustion ->
pause -> restart -> verified recovery with no duplicate effects; retries/config
changes cannot select another funding route; measured/list-rate/actual/unknown
remain distinct; existing #73 tests unchanged in meaning; V1/V2 preservation and
0041 FK integrity. Preserve ledger atomicity, holds, ambiguity, reconciliation,
revocation, idempotency, scope binding, quarantine, leases, fencing, provider
context/rate limits, operation bounds, credential confinement and gate authority.
