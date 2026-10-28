# ADR-0029 — Request-scoped operator approval and deferred executable specs

**Status:** Proposed (2026-10-28). This ADR authorizes implementation on the
Office UX campaign integration baseline only. Merging the protected
authorization change to `main` still requires independent review and Product
Owner approval.

**Supersedes:** none. **Amends:** the Lead start and ActionSpec assumptions in
ADR-0011/ADR-0022; it does not replace their Quality Gate or evidence rules.

## Context

The ordinary Office journey needs a user to submit an objective, review the
Lead plan and explicitly authorize one Campaign. The current implementation
cannot support that journey safely:

- `start_lead_campaign` requires a delegation with `autonomous_start` and the
  deployment-wide `FACTORY_ENABLE_AUTONOMOUS_START` flag. A non-autonomous
  delegation cannot be started after explicit operator approval.
- `action_specs` is immutable and an Action must have an executable spec before
  start, but a unified request cannot know the concrete checks until the Lead
  has assessed the objective. Creating an empty spec and mutating it later is
  forbidden, and start requires exact plan/spec equality.
- An intake-only cancellation flag does not prevent the existing start endpoint
  from starting a READY request.
- The frozen plan does not bind the exact execution-profile revision, and one
  gateway currently serves Lead and semantic review.

An independent `gpt-6.1-sol` review and a follow-up domain design (2026-10-28)
identified these as blockers. They are recorded in
[the Office UX contract](../implementation/office-ux-contracts-2026-10-28.md)
§3.5.

## Decision

### 1. One shared start operation with two authority modes

Add a single transactional start operation with Core-constructed authority:

```rust
enum StartAuthority {
    Autonomous(ValidatedAutonomyContext),
    Operator(AuthenticatedOperatorApproval),
}
```

- **Autonomous** retains the deployment flag, an active delegation with
  `autonomous_start`, autonomous R0 scope and the existing explicit R0
  classification. A profile cannot activate autonomy.
- **Operator** is a request-scoped approval by an authenticated operator. It
  permits a non-autonomous delegation but retains every common scope, risk,
  expiry, route, budget, execution-mode and frozen-identity check.
- Client-supplied `actor` is never identity. Core stores the authenticated
  principal and an optional display label.
- Approval evidence, Campaign authorization, reservations and Campaign/Run/
  Attempt creation commit in one transaction.

New immutable evidence:

```sql
lead_start_evidence(
  request_id PRIMARY KEY REFERENCES lead_requests(id),
  campaign_id UNIQUE REFERENCES campaigns(id),
  mode, principal_id, rationale, idempotency_key UNIQUE,
  request_hash, plan_hash, profile_revision_id, profile_hash,
  spec_sha256, config_hash, delegation_hash,
  policy_hash, authorization_digest, created_at
)
```

Exact replay returns the original Campaign without spending again. A different
principal, rationale or binding conflicts. A second idempotency key cannot create
a second Campaign for the same request.

This is a protected authorization change. It grants neither standing autonomy
nor delivery authority.

### 2. Draft Action, first executable spec at freeze

Intake may create an Action with an immutable assessment draft but no
`action_specs` row:

```sql
action_drafts(
  action_id PRIMARY KEY REFERENCES actions(id),
  assessment_input_json, input_sha256, created_at
)
```

At freeze, Core deterministically constructs the first executable **ActionSpec**
from the registered repository/base, original objective, implementer route,
validated work-unit scope and complete plan checks, and inserts it once with the
frozen plan. Existing `action_specs` rows remain byte-for-byte valid and
immutable. Draft Actions cannot execute; draft and spec cannot coexist at initial
creation. The draft remains as provenance.

Bind the Campaign to the frozen root spec through immutable
`campaign_execution_bindings(campaign_id PK, plan_id FK, spec_sha256,
envelope_hash)`. Attempt specs remain the exact execution payload; Worker and
Quality Gate read and hash-validate the attempt spec and Campaign lineage, never
a draft.

### 3. Freeze the exact profile and plan envelope

Store the exact profile revision/hash and role context with the request and a
versioned frozen-plan envelope. Submission accepts an expected revision/hash;
omitted defaults resolve once transactionally. Later profile edits do not change
an existing snapshot. A disabled provider connection blocks execution; a newer
profile revision alone does not.

A profile is `executable: true` only after Core can actually resolve every
declared role route, including Lead/implementer separation and candidate →
Campaign → frozen-profile reviewer lookup. A configured credential alone is not
proof. The current single default gateway cannot substantiate cross-provider
executability.

### 4. Budget intersection

Per-call limits (tokens, cost, timeout, reasoning) and a whole-request/Campaign
envelope (assessment, implementation, repair and required review) are distinct.
Every effective ceiling is the minimum of the profile, frozen plan, active
delegation, provider setting and Core policy. Paths/tools/capabilities are
intersected; routes must match exactly. Profiles never raise policy ceilings or
replenish a delegation.

Total implementation attempts are bounded by
`(work_units + max_repair_cycles) × max_attempts`; assessment and required review
sessions count against the session ceiling. Unknown/unsettled usage remains
charged and blocks a silent retry.

### 5. Withdrawal fencing

New immutable `lead_withdrawals(request_id PK, principal_id, reason,
created_at)` is an authoritative withdrawal fact, not a second lifecycle.
Withdrawal and start acquire the same write transaction. If withdrawal commits
first, assessment dispatch, freeze, start and alternate Campaign creation paths
reject the request. If start commits first, intake cancellation returns `409`
with the Campaign identity and directs the operator to Campaign cancellation.
Projection precedence is Campaign/gate outcome, then withdrawal, then Lead
failure/block/progress.

### 6. Transaction-scoped operations

Extract internal `*_in_tx` functions; public methods own locks and transactions.
Intake creation, freeze/READY and start are atomic. Replay lookup happens before
lifecycle checks against the original submitted snapshot, not current defaults.
Unknown upstream outcomes are charged and cannot be retried silently.

## Alternatives rejected

- **Enable `FACTORY_ENABLE_AUTONOMOUS_START` by default.** It would make every
  start autonomous authority and conflate approval with standing autonomy.
- **Create an empty ActionSpec and mutate it.** Violates immutable evidence.
- **Create a second Action/Lead request after assessment.** Duplicates state and
  breaks the request identity.
- **Add a full spec-revision selector everywhere.** Larger change surface than a
  single draft-to-first-spec transition; the latter preserves existing hashes.
- **Record a "proposed" autonomous delegation at intake.** Existing consumers
  treat active delegations as usable bounds; this would mint authority from
  profile configuration.
- **Use an intake-only cancellation flag.** It can be bypassed by the existing
  start endpoint.

## Consequences

- New migrations and tests are required before unified intake can merge.
- The default Compose stack keeps autonomous start off; explicit operator start
  is a separate, authenticated path.
- Existing FROZEN/READY/RUNNING history must remain readable. Upgrade tests must
  verify original bytes/hashes, FK integrity and trigger enforcement.
- Office must display approval, withdrawal, authorization and Campaign status as
  distinct facts.
- Independent verification and a real Compose/browser journey are required; a
  unit test does not satisfy the acceptance contract.

## Required tests

Unit hash/intersection/state tests; DB atomicity and reservation rollback;
simultaneous approve/approve, approve/withdraw, start/withdraw and
alternate-endpoint races; crash injection around commits and provider calls;
stale plan/profile/config; revoked delegation; forged actor/model/Worker
approval; unknown-usage fencing; existing autonomous fixture/provider
regressions; migration from a populated pre-change database; Compose/browser
approval, reload/replay, cancellation, stale selection, unsupported route and
permission/offline recovery.

## References

- [Office UX contract](../implementation/office-ux-contracts-2026-10-28.md)
- [Campaign ledger](../implementation/office-ux-campaign-2026-10-28.md)
- [ADR-0010](ADR-0010-frozen-candidate-quality-pipeline.md),
  [ADR-0011](ADR-0011-native-campaign-orchestration.md),
  [ADR-0013](ADR-0013-bounded-autonomous-recovery.md)
- Independent review and design at `9f341db` (2026-10-28); provenance only, not
  runtime acceptance.
