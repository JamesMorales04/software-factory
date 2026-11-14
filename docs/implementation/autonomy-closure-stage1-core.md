# Stage 1 — Core autonomous intake preparation

**Status:** implementer self-test in progress. This document describes the
Core-owned transition added by the stage 1 change. It is not independent
verification or acceptance.

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

## Authority boundaries

- The runner never grants authority, approves an owner-required plan, widens
  scope, changes the route, publishes external issues or starts delivery.
- A plan that requires an owner decision remains `PENDING`. The runner records a
  truthful waiting state and continues after the authenticated owner approval,
  which re-enqueues the same request.
- A rejected or changes-requested plan is never auto-approved or retried.
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
- `save_plan_review` reuses an identical review tuple inside one immediate
  transaction, so retries and concurrent identical owner approvals create one
  row. If the owner decision is recorded and the following environment phase
  fails, the response carries `reviewRecorded: true` and the real blocker.
- The assessment transition shares the per-request lock. An assessment already
  in progress is a recoverable wait, not a Lead failure.
- A recoverable prerequisite failure (provider/builder/model unavailable,
  transient assessment/funding) records a blocked operation without blocking
  the Lead; a restart, repaired prerequisite or valid owner approval resumes the
  same request. A terminal failure still blocks the Lead. Ambiguous dispatch is
  never retried blindly.
- A blocked environment caused by a builder outage is recompiled in a new
  revision when the runner resumes; a non-recoverable environment blocker is
  never replaced silently.

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
- withdrawal before preparation makes no model call and creates no Campaign.

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
- An owner plan approval that triggers a failing environment review can leave
  the approval row written while the endpoint returns an error. Retrying the
  same authenticated decision is safe in the current deterministic review
  path, but a deduplicated transaction is not part of stage 1.
- Withdrawal fences cover the WorkGraph, participant, specification draft,
  specification freeze, plan review recovery, environment compilation and
  environment freeze writes touched by the runner. Early malformed-submission
  blocked-environment writes are not separately fenced because they perform no
  model call.
