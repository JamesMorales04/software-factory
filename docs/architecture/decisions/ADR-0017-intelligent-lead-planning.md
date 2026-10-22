# ADR-0017 — Standing autonomous Lead planning and bounded WorkGraph execution

**Status:** Proposed revision 2, 2026-10-22. The Product Owner directed the
design toward goal-level autonomy under explicit standing authority. This ADR
records that direction; it does not ratify issuer/risk/revocation policy, grant
paid-provider spend, authorize delivery/merge/deployment, or certify isolation.
Existing accepted authority boundaries remain those of ADR-0016. **Date:**
2026-10-22 UTC.

## Context

ADR-0016 accepts autonomous development and maintenance as product objectives.
The first S4-D implementation connected a natural-language objective to one
advisory model assessment, one implementation WorkUnit, a frozen ActionSpec and
an existing Campaign. It required an explicit human start and one distinct
request per delegation. Deterministic evidence and CI for that revision are
recorded at `8b932f24b2504223a4e255261b7d2c8818544a52`.

The Product Owner now requires goal-level authorization. A standing delegation
must cover an objective and its derived engineering work without a human
approval at every routine transition. Human approval is an exception driven by
policy: protected risk, exhausted authority or budget, security concerns, or a
genuinely unresolved product decision. The Product Owner authorized this design
direction, not an unrestricted privilege grant.

## Decision drivers

- The Product Owner owns objectives, risk appetite, budgets and delegation
  limits; Core enforces them.
- Reusable scoped authority must cover multiple Actions and Campaigns within
  cumulative ceilings without allowing budget evasion through child objects.
- Semantic planning remains advisory. Core alone owns identities, scope,
  authorization, reservations, scheduling, quality and external effects.
- Dependency-aware implementation needs genuine execution, not schema-only
  graph validation.
- Final integrated acceptance requires independent evidence; a unit's success
  never proves the integrated product correct.
- Existing S4-A/S4-B/S4-C Campaign, Run, lease, candidate, Verifier, Reviewer
  and Quality Gate machinery remains authoritative.

## Decision

### 1. Reusable standing delegation

A delegation is a durable, owner-issued grant with:

- authenticated issuer/principal provenance and a durable audit identity;
- one registered Project;
- allowed objective classes, repository paths, capabilities and risk classes;
- explicitly permitted autonomous risk classes;
- exact provider/model/Paseo-mode route;
- per-Campaign limits (attempts, timeout, repair cycles, wall clock,
  parallelism, sessions) and cumulative limits (requests, sessions, attempts);
- start and expiry times, revocation semantics and escalation conditions;
- an immutable content hash; revocation is one-way and outside the hash.

Core maintains usage atomically in the same transaction as reservation. An
agent cannot evade a ceiling by creating child Actions, Campaigns, delegations
or replacement sessions. Existing persisted grants whose schema lacks the new
explicit fields fail closed and must be reissued. No default broadens a grant.

### 2. Autonomous start under standing authority

A frozen plan may start automatically only when:

- the delegation is active, unrevoked and unexpired;
- the Project, Action, base, configuration, frozen plan and spec bindings match;
- the proposal's risk class is in the delegation's autonomous risk classes;
- cumulative request/session/attempt reservations succeed; and
- the Campaign is pending and fully covered by the frozen plan.

Core records an auditable automatic-start event naming the delegation, risk
class and decision basis. The Office displays why execution started and under
which delegation. A ready plan outside these conditions stops at
`HUMAN_REQUIRED`/`blocked` with the exact missing authority. The existing
explicit authenticated start remains available. Model output never authorizes
its own start.

### 3. Risk policy

- `R0`: autonomous execution within active delegated scope and budget.
- `R1`: autonomous only when the delegation explicitly names `R1` as an
  autonomous risk class.
- `R2`/`R3`: explicit approval for the specific Action/Campaign; never
  autonomous under this revision.
- Merge, deployment and controller promotion remain separate future
  capabilities. Security credentials, isolation and SEC-01 restrictions are
  unchanged.

This ADR does not activate these privileges until the issuer, risk and
revocation semantics are ratified and encoded in the active delegation schema.

### 4. Revocation

Revocation immediately prevents new reservations and dispatch. Core blocks
open Lead requests and pending Campaigns, cancels or fences pending tasks, and
prevents new model sessions, repairs or protected effects. A running Campaign
is cancelled on the next orchestrator reconciliation; active Runs are cancelled
through the existing lease/fencing path. If an external session cannot be
confirmed terminated, it remains unresolved and further dispatch is contained;
Core does not fabricate termination. Immutable evidence is preserved.

### 5. Bounded dependency-aware WorkGraph

S4-D replaces the single-WorkUnit restriction with a bounded DAG:

- stable WorkUnit IDs, descriptions, dependencies, paths and capabilities;
- Core validation of unique IDs, unknown references, self-dependencies, cycles,
  unauthorized scope/capabilities, parallel path conflicts and budgets;
- one Core-generated integration node when more than one unit executes; it
  consumes dependency candidates and produces the final integrated candidate;
- per-unit frozen ActionSpecs derived from registered Project configuration;
- sequential units start from their prerequisite candidate; parallel units
  start from the campaign base; integration consumes all terminal dependency
  candidates;
- the existing Campaign scheduler releases nodes only after every dependency
  succeeds, persists state across restart, and fences stale dispatch;
- final independent Verifier, Reviewer and deterministic Quality Gate run on
  the integrated candidate. Per-unit verification is not required for this
  bounded slice because no unit is independently accepted or delivered.

This is not a general workflow DSL. Portfolio scheduling and dynamic specialist
composition remain S4-E/S7 responsibilities.

## Rejected alternatives

1. **Human approval for every transition.** Safe but contradicts goal-level
   autonomy and makes routine engineering impractical.
2. **Unbounded reusable grants.** No cumulative accounting or revocation would
   permit budget evasion and privilege creep.
3. **Schema-only WorkGraph.** Validating dependencies without executing them
   would misrepresent CAP-016.
4. **Multiple independent Campaigns per WorkUnit.** Easier scheduling but
   creates competing authority, budget and evidence paths and risks child-object
   evasion.

## Consequences and limits

- The current revision implements standing authority and a bounded DAG; it does
  not implement GitHub PR/merge/deployment, autonomous controller promotion,
  portfolio allocation or SEC-01 isolation.
- `maxSessions` still does not prove provider billing. Token/monetary ceilings
  remain observable only where a future meter records them; unknown cost stays
  unknown.
- TRUSTED_LOCAL retains broad host access. No temporary workspace establishes
  OS isolation or credential confinement.
- CAP-014 semantic quality requires an independently scored model-versus-
  baseline evaluation. This ADR does not substitute for it.
- Paid provider sessions require a separate numeric authorization envelope.
- Revocation of already-running external work is best-effort cancellation with
  truthful unresolved-session representation, not guaranteed termination.

## Verification required before acceptance

- Reusable delegation accounting is atomic, durable, idempotent and
  concurrency-safe, including denial beyond cumulative limits.
- Autonomous start occurs only under valid delegated risk/authority and records
  an audit event; all other cases block with an exact reason.
- Revocation prevents new reservation/dispatch and contains in-flight work.
- Sequential, parallel, fan-in, dependency-failure, cycle, scope, budget,
  restart and integration-conflict tests pass on the exact candidate.
- The final integrated candidate receives independent verification, review and
  Gate acceptance; intermediate unit success is insufficient.
- CAP-014 is scored against frozen `S4D-RULES-1` with the frozen rubric and
  thresholds.
- Any live certification is exact-SHA, budget-authorized and leaves durable
  session/cleanup evidence.
- All applicable lint, static analysis, CI and S1–S4-C regressions pass.

## Supersession

This revision supersedes the one-WorkUnit/explicit-start decision in revision 1
of this ADR while preserving ADR-0016's Core ownership, delivery separation and
security limits. It does not supersede ADR-0006 or authorize breaking published
contracts without the atomic changes required by repository policy.
