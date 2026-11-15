# Stage 1 — Core autonomous intake preparation

**Status:** implementer self-test in progress. This document describes the
Core-owned transition added by the stage 1 change. It is not independent
verification or acceptance.

**R3 repair (2026-11-15):** latest-only plan-review idempotency (approve →
reject → approve is a new effective decision), truthful ambiguous-assessment
wait (no false restart resume, no blind paid retry), re-enqueue after a
recorded approval whose environment phase failed recoverably, effective-lane
guard against reusing a weaker frozen PLAN, one blocked environment revision per
builder descriptor change, transient SQLite/quote-error classification and the
provider-bound projection. See the implementation ficha for the exact tests and
the residual lock/deadline proposal.

## Scope

The authorized autonomous intake runner previously called assessment, then
`ensure_planning_ready_or_error`, and only then attempted the start. When no
operator had compiled planning artifacts, the runner failed with
`PLAN_ARTIFACTS_REQUIRED` and never reached a Campaign.

This change makes the same runner prepare the existing planning phases:

1. compile or reuse the Planner WorkGraph;
2. persist the deterministic STACK, ARCHITECTURE, DOCUMENTATION, SKILL, DOD
   and PLAN artifacts and the deterministic plan review;
3. compile or reuse the participant plan for the validated graph;
4. compile and freeze the environment through the existing Lead environment
   review and Core freeze;
5. for `FORMAL_SPEC`, author the bounded specification draft, rebuild the local
   OpenSpec projection if needed, and freeze it deterministically;
6. re-run the existing readiness guard and the exact start transaction.

Only `AUTONOMOUS` intake requests enter this path. Manual `REVIEW_PLAN`
profiles keep their existing endpoints and owner decisions.

## Provider output bound

The structured Lead protocol asks for 16,384 output tokens for the first
attempt and at most 32,768 for the one bounded truncation retry. A new provider
configuration therefore defaults its durable output envelope to 32,768, derived
from the existing protocol ceiling rather than an arbitrary maximum. An operator
may set explicit `maxInputTokens`/`maxOutputTokens` bounds; existing settings
keep their current values.

Before reserving or dispatching a structured Lead call, Core checks the durable
provider setting and the latest frozen pricing quote. If either is below the
protocol requirement it returns
`AGENTIC_LEAD_OUTPUT_BOUND_INSUFFICIENT` with the configured and required
bounds. No model call, quote change or envelope increase happens implicitly.
An already-frozen quote that is too small remains untouched; the operator must
create a new configuration/revision and a fresh funding admission.

The provider projection (`GET/PUT /settings/providers`) exposes the stored
`maxInputTokens`/`maxOutputTokens` plus the protocol's own
`leadOutputTokens` (16,384) and `leadRetryOutputTokens` (32,768), so Office does
not duplicate Core policy. Editing a bound never mutates frozen quotes or
envelopes.

## Authority boundaries

- The runner never grants authority, approves an owner-required plan, widens
  scope, changes the route, publishes external issues or starts delivery.
- A plan that requires an owner decision remains `PENDING`. The runner records a
  truthful waiting state and continues after the authenticated owner approval,
  which re-enqueues the same request.
- A rejected or changes-requested plan is never auto-approved or retried. It is
  an owner wait, not a terminal Lead failure: the Lead stays `READY`, the
  operation is `BLOCKED`, and the runner returns the typed rejection without
  calling a model. A later authenticated approval of the still-current PLAN
  revision is an audited owner reversal: the workflow moves
  `REPLANNING → PLAN_REVIEW → FROZEN`, and the exact start still revalidates
  every policy, funding, readiness and scope guard.
- The exact start transaction still revalidates the frozen policy, delegation,
  charter, funding admission, profile revision, plan/spec/config binding,
  allowed paths and R0 risk.
- A database error while reading the owner charter is propagated; it is not
  treated as “no charter”.
- Withdrawal is rechecked after model I/O and before persisting a WorkGraph,
  participant plan, specification draft, specification freeze or environment
  review.

## Decision mechanisms

- **DETERMINISTIC:** transition order, artifact/revision checks, review
  recovery, participant compilation, environment/spec freeze, start, blockers.
- **MODEL:** Planner WorkGraph, stack/skill fallback, Lead environment review,
  Spec Manager draft.
- **HYBRID:** Core orchestrates and validates; model output never becomes
  authority.

## Idempotency and recovery

- The review policy is a single deterministic function over the frozen PLAN
  lane, signals and assurance plus the frozen STACK decision. Crash recovery
  reads those from the PLAN bundle; it never re-derives owner-required behavior
  from a drifted live triage context.
- A review that does not bind the current PLAN revision and exact hash is
  stale. It remains historical evidence but cannot reject or approve the current
  plan; the endpoint refuses a decision for an older revision.
- `save_plan_review` reuses an identical review tuple only while it is the
  request's latest decision, inside one immediate transaction. A consecutive
  retry (or concurrent identical retries) therefore creates one row. An
  identical tuple that precedes an intervening decision is a new intent: it is
  appended, becomes the effective decision and cannot be masked by the older
  row. This covers approve → reject → approve with the same rationale.
- The effective decision is the latest row in insertion order, not the largest
  wall-clock string. This matters when two reviews share a timestamp or when a
  random UUID would otherwise decide the tie.
- A plan review is an operator decision on a live request. The same immediate
  transaction that writes it checks the durable withdrawal marker, so a
  withdrawal committed before the write can never be followed by a recorded
  approval/rejection. A withdrawal that arrives later orders after the decision;
  the runner still fences the start.
- `rowid` is the decision sequence because the table is append-only. A real
  SQLite `VACUUM` repro on the `TEXT PRIMARY KEY` schema preserved relative
  `rowid` order; no speculative migration is added. A future delete/reinsert
  path would require an explicit sequence instead.
- If the owner decision is recorded and the following environment phase fails,
  the response carries `reviewRecorded: true` and the real blocker. A
  recoverable environment failure (builder/model/funding unavailable) also
  re-enqueues the same request, so repairing the prerequisite resumes through
  readiness without another owner decision. A terminal environment decision is
  not requeued.
- A successful `POST /agentic/requests/{id}/environment/recompile` is an
  operator-repaired prerequisite and re-enqueues an autonomous request with no
  Campaign; the runner still revalidates every authority and readiness guard.
- The assessment transition shares the per-request lock. An assessment already
  in progress is a recoverable wait, not a Lead failure. A durable `ASSESSING`
  state without a recorded assessment is the ambiguous-dispatch case: Core does
  not retry a possibly billed call and does not claim that a restart resumes it.
  The real operator actions are withdrawal (`POST /lead/requests/{id}/withdraw`)
  or a new request; the runner records one durable
  `autopilot.assessment_outcome_unknown` event for observability.
- A recoverable prerequisite failure (provider/builder/model unavailable,
  transient assessment/funding) records a blocked operation without blocking
  the Lead; a restart, repaired prerequisite or valid owner approval resumes the
  same request. A terminal failure still blocks the Lead. Ambiguous dispatch is
  never retried blindly.
- A blocked environment caused by a builder outage is recompiled in a new
  revision only when the builder descriptor actually changed (for example, the
  builder returned); repeating the same unavailable builder on every resume
  does not append a blocked revision. A non-recoverable environment blocker is
  never replaced silently.
- A live deterministic floor deeper than the frozen PLAN lane is not ignored:
  the runner stops with a typed `PLAN_LANE_ELEVATED` recoverable wait and asks
  for the existing deterministic plan recompile plus a new owner review. It does
  not silently reuse the weaker plan or author a replacement that discards the
  reviewed revision. Preparation and readiness share the same effective lane
  (the deeper of the frozen PLAN lane and the current floor).
- A transient SQLite failure (busy, locked, I/O, full) is a recoverable wait;
  a durable constraint or programming error remains terminal. A pricing-quote
  read failure is propagated instead of being treated as “no quote”.

- A completed Campaign short-circuits before any assessment, funding or model
  call.
- A persisted complete plan is reused without another Planner call.
- A persisted WorkGraph without a PLAN is reused; only the deterministic
  artifact/participant phase runs.
- A persisted participant plan that already matches the frozen reviewer plan is
  reused.
- A frozen environment/spec is reused; an approved environment review without a
  spec is frozen without another model call.
- A frozen specification is never re-authored; a missing local OpenSpec
  projection is rebuilt deterministically.
- Per-request locking serializes the autopilot and the authenticated planning
  endpoints, so one request cannot create two plans or WorkGraphs concurrently.

## Projection contract

Existing projections remain the authority. This change adds three Core-owned
boolean/identity fields without a new endpoint:

- `planning.reviewCurrent`: true only when the latest review binds the current
  PLAN revision and exact artifact hash.
- `planning.reviewRevision`: the review's plan revision, when present.
- `startReady`: `planning.ready` and the environment is frozen or not required
  and no Campaign is bound.

`reviewApproved` now requires the exact current revision and hash. A stale
review remains visible but cannot be presented as the current owner decision.
`authorization.available` is not a readiness substitute.

## Tests

New Core tests:

- unseeded autonomous intake compiles planning and starts a Campaign;
- a pending owner review performs the mechanical environment/spec preparation,
  then blocks without a Campaign or extra model call;
- replay after start does not repeat inference and does not create a second
  Campaign;
- restart from a persisted WorkGraph reuses the graph without a Planner call;
- a stale review is not reported as current/approved;
- withdrawal before preparation makes no model call and creates no Campaign;
- approve → reject → approve with the same rationale appends a new effective
  approval, the latest review approves current readiness and the runner does not
  classify it as rejected; a consecutive replay remains one row;
- concurrent identical retries share one review row, including retries after an
  intervening rejection;
- a withdrawal committed before a plan review leaves no new row; the endpoint
  returns `INTAKE_WITHDRAWN` and creates no Campaign;
- REJECT then APPROVE of the current revision restores `REPLANNING → PLAN_REVIEW
  → FROZEN`, keeps the Lead in the owner wait without blocking it, denies the
  start while rejected, and starts only after the authenticated reversal;
- a deeper live triage floor stops reuse of a weaker frozen PLAN with
  `PLAN_LANE_ELEVATED` (recoverable, no model call, no Campaign);
- an unchanged unavailable environment builder does not append a blocked
  revision per resume, while a changed builder descriptor recompiles once;
- transient SQLite pressure is a recoverable wait and a durable constraint is
  terminal;
- the provider projection carries the stored bounds and the protocol bounds,
  including a GET after a partial PUT and an exact `PUT(32768) → GET(32768)`
  round trip (not a universal policy value).

The deterministic test provider is a local fixture. It is development evidence,
not live provider or independent acceptance evidence.

## Limitations and residual gaps

- A real Compose/Office autonomous journey, restart-at-checkpoint injection and
  live provider acceptance remain separate evidence.
- The WorkGraph-to-Campaign dispatch bridge (A-02) is outside this stage.
- Autonomous repair and external specification publication remain operator
  actions.
- `TRUSTED_LOCAL` is not isolation or certification.
- A provider/build ambiguity, unavailable environment builder or reviewer
  independence failure still blocks the Lead request through the existing
  failure path. The operator must refresh, repair the configuration or restart
  the request; stage 1 does not add a WaitingForOperator scheduler.
- The manual `POST /agentic/requests/{id}/plan` path still returns an existing
  plan without repairing a missing review row. The autonomous runner recovers
  that state; the manual endpoint recovery is a separate small change.
- An owner plan approval that triggers a failing environment review still
  returns the recorded review plus the real blocker; a recoverable failure now
  re-enqueues the request, and a successful environment recompile re-enqueues it
  as well. Terminal environment decisions are not retried.
- `ASSESSING` without a recorded assessment has no reconciliation endpoint in
  this change. The message and this contract are now truthful (withdraw or new
  request; no automatic paid retry). An audited operator reconciliation that
  releases or re-issues the reservation would require a separate authorized
  transition; it is not invented here.
- The `/assess` handler still holds the per-request lock during the bounded
  model call, and the shared gateway has no total request deadline. A hung
  upstream can therefore block approve/recompile of the same request and hold
  one scheduler permit. A client-side timeout is deliberately not added: it
  could abort a call the upstream still bills and invite duplicate inference.
  The correct repair is a durable assessment lease plus explicit cancellation /
  outcome reconciliation (`assess` dispatched → known outcome), which is a
  separate architectural change. Reproduction for that change: a fixture
  upstream that accepts the connection and never responds; start `POST
  /agentic/requests/{id}/assess`, wait, then `POST /plan-review` and
  `/environment/recompile` and observe both waiting on the same lock while the
  scheduler permit stays held.
- A deeper live triage floor requires the existing deterministic plan recompile
  and a new owner review; stage 1 does not auto-recompile or auto-approve a
  risk-elevated plan.

- Withdrawal fences cover the WorkGraph, participant, specification draft,
  specification freeze, plan-review recovery, environment compilation and
  environment freeze writes touched by the runner; the endpoint plan-review
  write itself checks the withdrawal marker in its own immediate transaction.
  Early malformed-submission blocked-environment writes are not separately
  fenced because they perform no model call.
