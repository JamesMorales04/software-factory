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
| `SUBSCRIPTION_FUNDING_VERIFICATION_REQUIRED` | 503 | A persisted proposal or recoverable row exists but execution remains disabled. |
| `SUBSCRIPTION_FUNDING_QUOTA_PAUSED` | 503 | A durable quota pause exists; execution remains held and re-admission is unavailable. |
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

## Phase 1 repair: ledger test admission, quota schema and RECOVERABLE (2026-11-02)

PR #83's fail-closed gate left the ledger/dispatch regression suite red: those
tests exercise reservation, settlement, replay, ambiguity, dispatch-intent and
egress-fence semantics rather than funding. They now opt in through an explicit
test-only fixture:

- `test_subscription_dispatch_admissions` is created only in `#[cfg(test)]`
  builds and is keyed by one exact objective/scope or Campaign id.
- The gate consults it **only when no `subscription_funding_policies` row
  exists**, so a persisted proposal in any state (including `DISABLED`,
  `PAUSED_QUOTA`, `RECOVERABLE` or invalid evidence) always outranks it.
- Admission is per database and requires an explicit
  `Database::admit_test_subscription_dispatch` call; there is no environment
  switch, default-on admission or production build surface.
- Negative funding tests remain negative, with added coverage that an
  unrelated scope and a fresh non-opted-in database still refuse with
  `SUBSCRIPTION_FUNDING_BINDING_REQUIRED`.

`0042_subscription_funding_recovery_state.sql` repairs the 0041 recovery-state
constraint without editing the applied migration. 0041 required `reason`
exactly for `PAUSED_QUOTA`, which made the decoder's documented
`RECOVERABLE { reason }` state unrepresentable. The repaired schema requires a
reason for every non-`DISABLED` state, forbids one for `DISABLED`, permits
`resets_at_ms` only for `PAUSED_QUOTA`, and keeps the JSON discriminator,
foreign key, uniqueness and trigger set. The read model and the dispatch gate
now share one decoder, so a state cannot be valid for one and contradictory for
the other. The identity trigger refuses every transition out of
`PAUSED_QUOTA`, making a quota hold durable; quota recovery remains
**NOT_IMPLEMENTED / NOT_TESTED**. A durable pause now surfaces as the distinct
`SUBSCRIPTION_FUNDING_QUOTA_PAUSED` refusal (HTTP 503); it carries no dispatch
authority.

Evidence for the final merged candidate (host toolchain 1.90.0, no provider
calls):

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test --locked -p factory-native --lib --features test-support`:
  512 passed, 0 failed, 2 ignored (ignored tests require a cached Podman
  alpine image).
- `cargo test --locked -p factory-native --bin factory-core --features test-support`:
  33 passed, 0 failed. Without the feature the same target has 30 passed and
  compiles out the test-only route and positive provider fixtures.
- `cargo test --locked -p factory-native --bin factory-worker --features test-support`:
  13 passed, 0 failed.
- `cargo test --locked -p factory-native --test e2e_cap014 --features test-support`:
  2 passed, 0 failed.
- `cargo test --locked -p factory-native --test e2e_fixture_dispatch --features test-support`:
  12 passed, 0 failed.
- `cargo build -p factory-native --bins --locked` without `test-support`:
  passed; production remains fail-closed.
- Strict repository hygiene (`factory-repo-hygiene --mode strict`): passed,
  0 findings.
- `cargo xtask e2e intake`: passed with the deterministic fake model.
- `cargo xtask e2e intake-full`: passed through Worker, candidate, verifier,
  reviewer and Gate `ACCEPTED` with the deterministic fixture provider.
- Independent verification, live provider entitlement, real quota recovery
  and live inference remain **NOT_TESTED**.

## Test-only admission seam (2026-11-02)

Production `factory-core` and `factory-native` remain fail-closed. The
`test-support` Cargo feature is disabled by default and is rejected in
non-debug builds. It enables an opt-in, per-scope SQLite admission table used
only by deterministic tests: binary API tests and the process-level CAP-014
evaluator opt their exact evaluation/request scope in after Core is healthy.
It is not an environment variable, a runtime policy switch, or a provider
entitlement. A persisted subscription-policy row still outranks the fixture for
the same Campaign, and a test admission for one scope does not admit another.
The test commands in `xtask` and the Rust CI workflow explicitly enable the
feature; production build commands do not.

The CAP-014 tests are compiled only when this feature is enabled. A default
`cargo test` without the feature skips those positive provider-dispatch tests;
it does not enable a production route.

### Process-level browser fixture admission (2026-11-02)

`cargo xtask e2e intake` and `cargo xtask e2e intake-full` build the debug
binaries with the `test-support` feature. After creating the intake request,
the journey opts the exact intake request, Lead request and resulting Campaign
scopes into the per-database fixture before assessment and provider dispatch.
The opt-in route is registered only when the feature is compiled in, requires
the operator token, and stores an exact scope string; it is absent from
production builds and from `cargo xtask check-container`. The deterministic
fake model and fixture provider remain the only inference sources in this lane,
and no live provider call is made.
