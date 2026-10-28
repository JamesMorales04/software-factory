## 1. Enforce-point audit table

Contract scope: read-only architecture assessment; no files changed, execution authorized, or paid inference performed.
Citations bind to observed HEAD `132d43d1e4f2b2474b29c34593780ba969786204`, not supplied code candidate `7063321`; Director must reconcile this difference before implementation.
Paths below are repository-relative; proposed symbols, tables, and endpoints are requirements, not claims of existing implementation.
Runtime behavior, migrations, browser journeys, and provider billing guarantees: **NOT_VERIFIED**.

| File:line | Field/condition; actual meaning | Classification | Disposition |
|---|---|---|---|
| `native/src/execution_profile.rs:31` | `MAX_CAMPAIGN_ATTEMPTS=10`; profile validation ceiling, tighter downstream | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:33` | `MAX_CAMPAIGN_SESSIONS=16` | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:35` | `MAX_CAMPAIGN_TURNS=16`; does not override Worker limit four | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:28` | `MAX_ROLE_INPUT_TOKENS`, `MAX_ROLE_OUTPUT_TOKENS`; role-budget maxima | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:30` | `MAX_ROLE_COST_MICROS`; artificial role monetary maximum | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_MONETARY |
| `native/src/execution_profile.rs:36` | Campaign cumulative input/output maxima | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:38` | `MAX_CAMPAIGN_COST_MICROS`; universal configuration ceiling | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_MONETARY |
| `native/src/execution_profile.rs:39` | Campaign deadline range 30–86,400 seconds | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:148` | `ExecutionRoleRoute` token/cost fields; later consumed cumulatively | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/execution_profile.rs:178` | `CampaignBudgets`; mixes productivity, tokens, money, time | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/intake.rs:20` | `CORE_MAX_ATTEMPTS=5`, sessions two, turns four, deadline 600 | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/intake.rs:32` | `EXECUTION_POLICY_MAX_REPAIR_CYCLES=0` | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_PROGRESS |
| `native/src/intake.rs:33` | `EXECUTION_POLICY_MAX_PARALLEL=1`; concurrency capacity | RUNTIME_RESOURCE | KEEP |
| `native/src/intake.rs:63` | `execution_policy_terms`; derives per-request and cumulative ceilings | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/intake.rs:259` | `intersect_campaign_budget`; intersects profile/plan/grant/Core ceilings | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/lead.rs:27` | `MAX_CAMPAIGN_IMPLEMENTER_RUNS=64`; total execution count | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/lead.rs:29` | `MAX_CUMULATIVE_SESSIONS=256`, attempts 512 | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/lead.rs:32` | `MAX_CHECK_TIMEOUT_SECS=600`; individual verification operation | RUNTIME_RESOURCE | KEEP |
| `native/src/lead.rs:315` | Grant content hash, revocation, expiry validation | SECURITY_BOUNDARY | KEEP |
| `native/src/lead.rs:365` | Grant attempts, repairs, sessions, universal wall-clock bounds | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/lead.rs:679` | Plan repairs, wall clock, sessions constrained by grant | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_PROGRESS |
| `native/src/lead.rs:693` | Bounded WorkGraph representation; not permission to cap lifetime work | RUNTIME_RESOURCE | KEEP |
| `native/src/lead.rs:701` | Work-unit × attempts calculation and session sufficiency rejection | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/authorization.rs:159` | Authorization requires attempts within 1..=5 | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/authorization.rs:243` | Unique subset of authorized provider tools | SECURITY_BOUNDARY | KEEP |
| `native/src/db/authorization.rs:260` | Sessions ≤2, turns ≤4, deadline 30..=600 | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/authorization.rs:265` | Campaign cost field; currently separate from Lead/reviewer scopes | MONETARY_AUTHORIZATION | CONVERT_TO_MONETARY |
| `native/src/db/delegations.rs:360` | Cumulative request/session/attempt admission counters | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/delegations.rs:15` | `DelegationUsage`; historical activity counts remain useful | HISTORICAL_COUNTER | DIAGNOSTIC_ONLY |
| `native/src/db/workers.rs:15` | Authorization creation time + deadline; absolute dispatch expiry | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/workers.rs:244` | `validate_lead_attempt_lineage`; assignment provenance | SECURITY_BOUNDARY | KEEP |
| `native/src/db/model_calls.rs:18` | `ModelCallRole::{Lead,SemanticReviewer,Evaluation}`; attribution | TELEMETRY | KEEP |
| `native/src/db/model_calls.rs:181` | Output ≤8,192 tokens and reservation lifetime 120 seconds | RUNTIME_RESOURCE | KEEP |
| `native/src/db/model_calls.rs:194` | `BEGIN IMMEDIATE`; serialized reservation write | MONETARY_AUTHORIZATION | KEEP |
| `native/src/db/model_calls.rs:218` | Scope cumulative tokens/cost; separate accounting domain | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_MONETARY |
| `native/src/db/model_calls.rs:236` | `global_policy`; cumulative role-wide token/cost pool | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/model_calls.rs:272` | Reserves entire remaining input/cost envelope | MONETARY_AUTHORIZATION | CONVERT_TO_MONETARY |
| `native/src/db/provider_sessions.rs:180` | `max_requests` 1..=8; finite egress binding batch | RUNTIME_RESOURCE | KEEP |
| `native/src/db/provider_sessions.rs:241` | Current Worker, generation, lease, attempt/campaign state | SECURITY_BOUNDARY | KEEP |
| `native/src/db/provider_sessions.rs:265` | Session totals plus turn reservations; excludes other model-call scopes | MONETARY_AUTHORIZATION | CONVERT_TO_MONETARY |
| `native/src/db/provider_sessions.rs:311` | Reservation expiry derived from campaign deadline | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/provider_sessions.rs:953` | Session count must stay below authorization `max_sessions` | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/provider_sessions.rs:2372` | Tool requests limited to `max_turns * 8` | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/model_gateway.rs:19` | Prompt/response/upstream byte bounds | RUNTIME_RESOURCE | KEEP |
| `native/src/model_gateway.rs:398` | Individual request output ≤8,192 tokens | RUNTIME_RESOURCE | KEEP |
| `native/src/db/provider_settings.rs:20` | Provider settings store turns and cumulative tokens/cost | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/provider_settings.rs:24` | Input/output price rates; pricing inputs, not authorization | MONETARY_AUTHORIZATION | KEEP |
| `native/src/bin/worker.rs:657` | Assignment rejects `max_turns` outside 1..=4 | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/bin/worker.rs:1758` | OpenCode loop bounded by assignment turns | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_PROGRESS |
| `native/src/bin/worker.rs:2665` | Codex session deadline and turn-count loop at :2683 | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_PROGRESS |
| `native/src/bin/worker.rs:1131` | Credential lifetime derived from campaign deadline | SECURITY_BOUNDARY | KEEP |
| `native/src/bin/core.rs:1201` | Settings cost intersected into model policy | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_MONETARY |
| `native/src/bin/core.rs:1403` | Role-specific environment token/cost ceilings | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/bin/core.rs:1743` | Hidden default turns four; default cost at :1752 | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/bin/core.rs:5377` | Approval projection exposes old budget limits | TELEMETRY | DIAGNOSTIC_ONLY |
| `native/src/bin/core.rs:6513` | Separate measured/unmeasured session/model spending projection | TELEMETRY | CONVERT_TO_MONETARY |
| `native/src/bin/core.rs:8550` | Intake authorization copies old intersected limits | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/execution_profiles.rs:507` | `CampaignFrozenProfile`; immutable revision identity | SECURITY_BOUNDARY | KEEP |
| `native/src/db/intake.rs:510` | Profile deadline/session minimum checks | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/intake.rs:1088` | Cumulative delegation admission checks | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/intake.rs:1121` | Exact parallel-one/repair-zero policy matching | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/lead.rs:1625` | Additional attempts-five enforcement outside requested file list | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/issue_intake.rs:828` | Issue policy converted into capped delegation | ARTIFICIAL_PRODUCTIVITY | REMOVE |
| `native/src/db/issue_intake.rs:673` | Explicit batch total/per-issue financial allocation | MONETARY_AUTHORIZATION | CONVERT_TO_MONETARY |
| `native/src/db/issue_intake.rs:1675` | Queue retry counter gates transient retry | ARTIFICIAL_PRODUCTIVITY | CONVERT_TO_PROGRESS |
| `migrations/0004_campaign_authorization.sql:15` | SQL `max_attempts BETWEEN 1 AND 5` | ARTIFICIAL_PRODUCTIVITY | REMOVE |

## 2. Execution Policy V2 typed contract

All definitions below are new; derive `Serialize`, `Deserialize`, `JsonSchema`; structs deny unknown fields.
Validated ID/hash newtypes wrap strings; `UsdMicros(u64)` validates ≤`i64::MAX`; arithmetic is checked.
```rust
enum FrozenExecutionPolicy {
    LegacyV1 { evidence_id: String, content_sha256: String },
    V2(ExecutionPolicyV2),
}
struct ExecutionPolicyV2 {
    policy_id: String,
    authorization_id: String,
    monetary_envelope_id: String,
    scope_sha256: String,
    progress: ProgressPolicy,
    operations: OperationLimits,
}
struct OperationLimits {
    model_request_timeout_secs: NonZeroU32,
    tool_call_timeout_secs: NonZeroU32,
    max_request_bytes: NonZeroU32,
    max_response_bytes: NonZeroU32,
    max_generation_tokens: NonZeroU32,
    max_in_flight_operations: NonZeroU32,
}
struct ProgressPolicy {
    diagnose_after_stagnant_checkpoints: NonZeroU32,
    escalate_after_unsuccessful_replans: NonZeroU32,
    observation_timeout_secs: NonZeroU32,
}
enum ProgressKind {
    CandidateChanged, TestsImproved, FindingsResolved, WorkGraphAdvanced,
    UnitAccepted, RepairValidated, BlockerIdentified,
}
struct ProgressEvidence {
    evidence_id: String, campaign_id: String, work_item_id: String,
    kind: ProgressKind, artifact_ref: String, artifact_sha256: String,
    baseline_ref: String, observed_sequence: u64,
}
struct ProgressCheckpoint {
    checkpoint_id: String, work_item_id: String, operation_id: String,
    evidence_ids: Vec<String>, outcome_sha256: String,
}
enum ContinuationDecision {
    Continue, Diagnose, Replan, Escalate, AwaitAuthorization,
    AwaitReconciliation, Cancel, Complete,
}
```
No V2 turns, attempts, sessions, repairs, cumulative tokens, per-role money, or campaign deadline fields; reject obsolete fields on new writes.
Profile routes retain provider/model/reasoning; request limits describe technical bounds, never replenishing cumulative role allowances.
Continuation is **DETERMINISTIC**: valid scope/risk/tools/isolation, current fencing, active authorization, financial availability, and progress state.
Completion requires existing deterministic Quality Gate evidence for the exact candidate; spending exhaustion pauses, never grants acceptance.
Evaluate after each terminal model/tool operation and each attempt; observation timeout probes/reconciles an operation rather than ending a campaign.
Defaults: diagnose after two consecutive stagnant checkpoints per work item; escalate after one unsuccessful diagnosis/replan episode.
Diagnosis may use existing Lead context (**MODEL**); Core validates the proposed replan and grants only the existing bounded authority (**HYBRID**).
After the replan, two further stagnant checkpoints escalate; no background retries until an authenticated operator decision or verified external change.
Only verified evidence resets stagnation; elapsed time, heartbeat, prose, session creation, and token consumption do not.
Deduplicate artifacts and outcomes across restarts: reverting/reapplying a known diff, empty commits, or replayed receipts earns no progress.
Test improvement compares identical suites/environments; resolved findings require stable finding IDs; acceptance requires Core gate records.
A new blocker requires a distinct reproducible dependency/failure reference and moves work to blocked/diagnosis; blocker discovery cannot renew a loop indefinitely.
Changed commits/diffs count once as provisional progress; require validated test/finding/graph/repair advancement within the next two checkpoints.
Bound WorkGraph payload/page size, not campaign lifetime units; explicit validated graph revisions allow further authorized work.
Legacy V1 remains an immutable evidence representation, with original JSON bytes/hashes and provenance; it is never decoded using V2 defaults.
At cutover, quiesce active V1 execution at safe operation boundaries and require a new linked V2 request for continuation; retain unresolved charges.

## 3. Shared campaign ledger contract

One `envelope_id` is created before the first billable assessment for an operator request; campaign attachment never creates another allowance.
A direct campaign request gets the same request identity abstraction; subagents, auxiliary tasks, repairs, and reviewers inherit that envelope.
Standalone evaluation needs its own explicitly authorized request; campaign-associated evaluation charges the campaign envelope.
Amounts use integer USD micros; unsupported currencies require an explicitly frozen conversion quote, otherwise dispatch fails closed.
Subscription authentication does not imply zero-cost or unlimited inference; unknown billing basis blocks authorization pending explicit bounded treatment.
New SQL schema; identifiers are TEXT, amounts/counters INTEGER, timestamps INTEGER UTC milliseconds unless stated:
```text
inference_envelopes(
 id PK, request_id UNIQUE NOT NULL, project_id FK projects,
 currency CHECK='USD', state CHECK IN('ACTIVE','REVOKED','CLOSED','OVERDRAWN'),
 authorized_micros NOT NULL CHECK>=0, authorization_revision NOT NULL,
 created_at_ms NOT NULL, revoked_at_ms NULL)
inference_campaign_bindings(
 campaign_id PK FK campaigns, envelope_id NOT NULL FK inference_envelopes,
 bound_at_ms NOT NULL)
inference_authorizations(
 id PK, envelope_id FK, revision NOT NULL, limit_micros CHECK>=0,
 actor NOT NULL, scope_sha256 NOT NULL, evidence_sha256 NOT NULL,
 idempotency_key UNIQUE NOT NULL, created_at_ms NOT NULL,
 UNIQUE(envelope_id,revision))
inference_reservations(
 id PK, envelope_id FK, operation_id NOT NULL, idempotency_key NOT NULL,
 request_sha256 NOT NULL, role NOT NULL, principal_id NOT NULL,
 parent_operation_id NULL, provider NOT NULL, model NOT NULL,
 pricing_revision NOT NULL, pricing_snapshot_json NOT NULL,
 upper_bound_micros NOT NULL CHECK>=0, settled_micros NULL CHECK>=0,
 state CHECK IN('RESERVED','SETTLED','RELEASED','AMBIGUOUS'),
 worker_id NULL, generation NULL, dispatched_at_ms NULL,
 provider_request_id NULL, expires_at_ms NOT NULL,
 settlement_sha256 NULL, created_at_ms NOT NULL, updated_at_ms NOT NULL,
 UNIQUE(envelope_id,idempotency_key), UNIQUE(envelope_id,operation_id))
inference_ledger_events(
 id PK, envelope_id FK, reservation_id NULL FK inference_reservations,
 kind NOT NULL, actor NOT NULL, evidence_ref NOT NULL,
 payload_sha256 NOT NULL, created_at_ms NOT NULL)
```
Indexes: reservations `(envelope_id,state)`, `(state,expires_at_ms)`; bindings `(envelope_id)`; events `(envelope_id,created_at_ms,id)`.
Authorization and event rows are append-only; triggers prevent deletion or mutation of reservation identity, request digest, quote, and original bound.
Reservation state: `RESERVED→SETTLED|RELEASED|AMBIGUOUS`; `AMBIGUOUS→SETTLED|RELEASED` requires reconciliation evidence.
`SETTLED` and `RELEASED` are terminal; conflicting duplicate settlement returns conflict; identical settlement replays its receipt.
Reserve using SQLx `begin_with("BEGIN IMMEDIATE")`; process-local mutexes are insufficient for cross-connection safety.
Inside the transaction: validate request lineage, authorization revision/revocation, fencing, provider/model, and request-bound idempotency.
Existing identical operation returns its durable status; a different digest under the same key conflicts; never redispatch an ambiguous operation.
Calculate `committed = SUM(SETTLED.settled_micros) + SUM(RESERVED|AMBIGUOUS.upper_bound_micros)` using checked arithmetic.
Admit only if `committed + requested_upper_bound <= authorized_micros`; insert reservation and audit event, then commit before upstream effects.
Reserve a conservative per-operation bound from frozen pricing, bounded input/output, cached/reasoning/tool billing, and adapter-specific charges.
An estimate is insufficient: unavailable enforceable upper bounds make billable dispatch unavailable; do not reserve an invented price.
Each billable egress request, including SDK retries and parallel children, requires its own reservation; one turn may group receipts without double charging.
Record dispatch intent before sending; mark uncertain send/timeout/disconnect `AMBIGUOUS`, holding the full upper bound.
Lease or reservation expiry never proves no charge; crash before proven dispatch may release, uncertain dispatch requires reconciliation.
Settlement atomically replaces the hold with actual charge, stores usage/pricing evidence, and appends an event; financial settlement is separate from task success.
Unknown tokens with authoritative final cost may settle; unknown cost remains ambiguous even after process cleanup.
Actual cost above bound is recorded truthfully, marks envelope `OVERDRAWN`, blocks further spending, and raises a billing-bound failure.
Revocation uses the same immediate transaction serialization; blocks new reservations, requests cancellation, and preserves in-flight liabilities.
Operator reconciliation cannot erase liability without evidence; unsupported providers retain ambiguity or conservatively settle the held bound with explicit provenance.
Campaign totals derive exclusively from this ledger; session/model-call totals remain evidence projections and must not be added again.
Internal API: `reserve_inference`, `mark_inference_dispatched`, `settle_inference`, `release_inference`, `reconcile_inference`, `revoke_envelope`, `spending_status`.
New authenticated API: `GET /api/requests/{id}/spending`, `POST /api/requests/{id}/spending/authorize`, `POST /api/requests/{id}/spending/revoke`.
New operator reconciliation API: `POST /api/inference-reservations/{id}/reconcile`; settlement routes retain role authentication and fencing.
Office fields: `envelopeId`, `currency`, `authorizedMicros`, `settledMicros`, `reservedMicros`, `ambiguousMicros`, `availableMicros`, `overrunMicros`.
Also expose `authorizationRevision`, `billingBasis`, `pricingProvenance`, `unresolvedCount`, `state`, `asOf`, and role/provider breakdowns.
Show held/unknown spending separately from measured spending; authorization, reconciliation, revocation, offline, and recovery journeys require real Core operations.

## 4. Migration plan

Actual SQLx migration source is repository-root `migrations/`, wired at `native/src/db/mod.rs:502`; `native/migrations/` does not exist.
Observed head: `0033_issue_intake_policy.sql`; `0032` is absent. Do not renumber history or fill that gap.
Reserve `0034_execution_policy_v2.sql` for Lane C and `0035_shared_inference_ledger.sql` for Lane L; Director owns numbering conflicts.
0034 adds immutable V2 policy/authorization evidence and progress checkpoint/evidence tables, plus explicit version discriminators.
Rebuild `campaign_authorizations` to retain every legacy column/value and immutable trigger while allowing NULL legacy budget fields only for explicit V2 rows.
Use a versioned CHECK: V1 retains original requirements; V2 requires its policy reference and NULL obsolete cap fields; never use zero or huge-number sentinels.
Likewise version/rebuild cap-bearing `standing_delegations` and `issue_intake_policy_revisions`; add V2 authority JSON/hash references, preserving old bytes.
Add `schema_version DEFAULT 1` to profile revisions and frozen execution contexts; future revisions write version two, with no automatic pointer/default conversion.
Existing intake/profile/delegation/start evidence, issue batches, request hashes, authorization IDs, and receipts remain unchanged and auditable.
0035 creates ledger tables; adds nullable unique `ledger_reservation_id` links to model-call and provider-turn reservation tables.
Keep old accounting rows intact; only V2 uses ledger balances. Historical unresolved liabilities stay visible and require reconciliation before replacement execution.
Legacy evidence is not financially backfilled as new spend or available credit; linked replacement requests receive fresh explicit authority.
Execute rebuilds through one pinned SQLite connection using the documented foreign-key-safe rebuild procedure; restore constraints and run `foreign_key_check`.
Migration must fail atomically on row/hash/count mismatch; preserve trigger/index definitions and test restart after interrupted migration.
No V1 request silently resumes as V2; policy upgrade does not widen authority, renew credentials, waive risk, or alter frozen acceptance criteria.
Director records the product decision in a new ADR and updates MASTER, budget/provider contracts, Office parity, capability status, and roadmap.

## 5. Lane boundary

Lane C owns `execution_profile.rs`, `intake.rs`, `lead.rs`, new `execution_policy.rs`/`progress.rs`, `db/execution_profiles.rs`, `db/delegations.rs`, and migration 0034.
Lane C removes productivity predicates and introduces versioned evidence, progress supervision, and migration-only V1 decoding.
Lane L owns new `inference_ledger.rs`/`db/inference_ledger.rs`, `db/model_calls.rs`, `db/provider_sessions.rs`, `model_gateway.rs`, and migration 0035.
Lane L owns reservation arithmetic, request-bound pricing, receipt reconciliation, aggregation, and financial status projections.
Director integrates shared `bin/core.rs`, `bin/worker.rs`, `db/authorization.rs`, `db/workers.rs`, `db/intake.rs`, `db/lead.rs`, `db/issue_intake.rs`, `db/provider_settings.rs`.
Director also owns module exports, shared DTOs, provider egress integration, Office changes, fixtures, cross-lane tests, and normative documents.
Before parallel implementation, commit one compiling shared-interface change: `FrozenExecutionPolicy`, `ExecutionPolicyV2`, `ContinuationDecision`, `UsdMicros`, `InferenceRole`.
`InferenceRole = Lead | Implementer | SemanticReviewer | Evaluation | Auxiliary`; parallel agents use principal/parent operation identity, not separate allowances.
Also freeze `ReserveInferenceInput`, `InferenceReservation`, `InferenceSettlement`, `SpendingStatus`, typed ledger errors, ownership map, and migration filenames.
The interface commit contains types/signatures and compile checks; no permissive placeholder authorization or duplicated active accounting path.

## 6. Test plan

Regression fixtures must exercise production Core admission and Worker loops, not merely construct uncapped DTOs.
Prove OpenCode and Codex reach turn five with verified progress; prove a campaign reaches attempt six, session three, and repair cycle two.
Repeat beyond former cumulative role token allowances and past former campaign deadlines while individual operation timeouts remain enforced.
For every case assert unchanged scope/tools/risk/fencing, positive shared financial availability, durable progress receipts, and candidate-bound gate requirements.
No-progress tests: two duplicate outcomes diagnose; one unsuccessful replan plus two stagnant checkpoints escalates; restart cannot reset counts.
Reject fake progress: empty commits, diff oscillation, replayed test receipts, changed test suites, repeated blockers, unsupported model assertions.
Concurrency: independent SQLite connections race mixed Lead/Implementer/reviewer/provider reservations; admitted holds plus settlements never exceed authorization.
Idempotency: concurrent duplicate keys dispatch once; mismatched request digests conflict; identical settlement replays; conflicting settlement fails.
Crash injection: before reservation commit, after commit/before dispatch, after dispatch intent, after upstream send, and before settlement commit.
Assert uncertain charges remain held, restart never resends them, reconciliation settles once, and session receipts never double count turn receipts.
Race revoke against reserve/dispatch; reject stale generations; accept authenticated late settlement after revocation without renewing authority.
Test provider price changes, missing prices, subscription billing ambiguity, mixed providers, bounded retries, overflow, zero balance, and over-bound actual charges.
Migration fixtures cover populated V1 databases, active campaigns, unknown usage, immutable hashes, SQL triggers, FK integrity, interruption, and fresh V2 writes.
Office browser tests cover one visible envelope, all-role spending, holds/ambiguity, exhaustion, new authorization, revocation, diagnosis, and escalation.
Run repository format/lint/tests and real Compose self-tests; independent verifier/reviewer bind evidence to the frozen candidate; Quality Gate remains deterministic.
All proposed tests, provider guarantees, migration results, and real-use acceptance are **NOT_VERIFIED** by this read-only contract.
