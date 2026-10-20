# ADR-0011 — Core-owned native campaign orchestration

**Status:** Accepted (F1-S4-B implementation; final candidate evidence is recorded in the PR). **Date:** 2026-10-20.

## Context

F1-S4-A proved the frozen specification, immutable candidate, independent
verifier, read-only reviewer and deterministic Quality Gate as separate
components. A human still had to launch each participant. The Factory could
not truthfully claim to coordinate an engineering workflow.

The next minimum capability is a durable, Core-owned Campaign that binds one
authorized Action/ActionSpec to a bounded task graph and advances it through
existing Worker, Paseo, verifier, reviewer and gate infrastructure.

## Decision

1. **Campaign is a Core aggregate, not a second execution engine.** It stores
   an immutable plan revision, repair horizon, wall-clock/concurrency budgets,
   terminal outcome and durable task rows. Implementer work is an existing S2
   Run with its existing Attempt, lease, generation-fencing and S3-D recovery
   semantics. No parallel Run/Attempt lifecycle exists.
2. **The plan is data and immutable.** The first engineering graph is
   `implementer → (verifier || reviewer) → quality_gate` per cycle, with at most
   three repair cycles. Later cycles are dormant until the preceding gate
   returns `REPAIR_REQUIRED`; they cannot be authorized by an operator, a
   Worker or a model.
3. **Core schedules, participants execute.** Core atomically claims ready
   tasks, creates spec-bound Runs, and spawns the existing separate
   `factory-verifier` and `factory-reviewer` binaries with dedicated scoped
   credentials. Participant exit status is not success: a task completes only
   from durable candidate-bound evidence. Gate evaluation stays deterministic
   and Core-owned.
4. **Candidate dependency is explicit.** Verification/review tasks are promoted
   only after the Implementer Run has authoritatively succeeded and a frozen
   CandidateRevision exists. Both consume the same candidate identity. Only the
   gate's one decision can accept a candidate or authorize repair.
5. **Bounded repair.** `REPAIR_REQUIRED` increments a campaign repair counter
   and creates a new cycle against the same ActionSpec/scope. New attempts
   produce a new CandidateRevision and lineage; old evidence is never reused.
   `BLOCKED` and `HUMAN_REQUIRED` are terminal and never authorize repair.
6. **Fail closed.** Unresolved S3-D recovery, missing participant executables,
   exhausted retries, wall-clock exhaustion, evidence identity mismatch and
   cancellation terminate or block the campaign with a durable reason. Budget
   escalation is rejected at creation; unknown token/quota/cost data stays
   "unknown", never zero.
7. **Office reads Core projections.** Campaign, task, participant, candidate,
   evidence, repair and gate state are served from the Core API and the
   existing durable event ledger/SSE replay. Office has no scheduler or
   invented state.
8. **Scope boundaries.** No GitHub delivery, S5/S6, general workflow DSL,
   dynamic skill discovery, field-level model routing, remote Workers or
   isolation certification is added. TRUSTED_LOCAL remains a documented risk
   acceptance, never "isolated".

## Consequences

- New `campaigns` and `campaign_tasks` tables plus campaign events on the
  existing ledger; no migration of S1–S4-A state.
- Core gains a deterministic scheduler loop and local participant process
  supervision. `factory-core` must be distributed with `factory-verifier` and
  `factory-reviewer` (or be configured with explicit paths).
- Concurrent Core instances are safe only through transactional task claims,
  unique candidate decisions and existing SQLite serialization; this is not a
  distributed-HA design.
- Repair is deliberately non-recursive: a campaign cannot spawn child
  campaigns to evade budgets.
- Office gains Campaign list/detail projections and a browser start path.

## Alternatives rejected

- **External Campaign Director commands:** still requires a human/script to
  invoke every role and cannot be Core-authoritative.
- **Model-generated workflow graphs:** unbounded, unverifiable and outside the
  first workflow requirement.
- **Verifier/reviewer as in-process functions:** would erase the independent
  process/credential boundary proven in S4-A.
- **Run completion as campaign success:** execution evidence is not acceptance.
- **Unbounded automatic repair:** can loop and consume resources without a new
  authority decision; the gate plus a hard repair ceiling is the boundary.
