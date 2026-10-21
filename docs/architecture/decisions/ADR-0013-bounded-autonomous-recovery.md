# ADR-0013 — Bounded autonomous Implementer recovery and oracle discipline

**Status:** Accepted (F1-S4-C-03 implementation; final candidate evidence is recorded in the PR). **Date:** 2026-10-21.

## Context

S4-B/S4-C terminated a Campaign when an Implementer Run failed before a
candidate existed. Ordinary recoverable failures — no diff, failed tests,
temporary runtime artifacts interfering with candidate construction, or a
transient Paseo startup failure — therefore escalated to the Product Owner.
That contradicts the Factory's purpose: exercise bounded corrective work
autonomously and escalate only protected authority decisions.

At the same time, a failed acceptance oracle must never be "fixed" by letting
the Implementer rewrite protected checks. Security-shaped failures and
unresolved external sessions must remain fail-closed.

## Decision

1. **Campaign remains the sole orchestration authority.** A pre-candidate
   Implementer failure is classified deterministically and may authorize a
   new fenced Run under the same Campaign/Task, using the same immutable
   ActionSpec, Project scope and capabilities. No second Run/Attempt
   lifecycle and no external orchestrator are introduced.
2. **The failed identity is immutable.** The failed Run, Attempt, Paseo
   binding/session, error and evidence remain unchanged. A
   `campaign_recoveries` audit row records the failure class, bounded reason,
   previous Run/Attempt and the new Run. The task returns to `ready`; the next
   dispatch creates a new Run with a new lease generation.
3. **Failure classes are closed and deterministic.**
   - `RECOVERABLE_IMPLEMENTATION_FAILURE`: no diff, failed tests/formatting,
     ordinary incomplete changes.
   - `RECOVERABLE_EXECUTION_FAILURE`: transient startup/timeout with confirmed
     cleanup and no authority defect.
   - `QUALITY_REPAIR_REQUIRED`: the existing gate-authorized repair cycle.
   - `ORACLE_DEFECT_SUSPECTED`: ambiguous evidence; escalate
     `HUMAN_REQUIRED`. Correction requires a new frozen specification; prior
     evidence is never mutated.
   - `POLICY_OR_SECURITY_BLOCKED`: unauthorized paths, protected-oracle
     modification, forged/cross-project evidence; no retry.
   - `UNRESOLVED_EXTERNAL_RECOVERY`: active/orphaned Paseo state without a
     safe replacement receipt; no retry.
   - `INFRASTRUCTURE_BLOCKED`: missing tooling, capability or participant
     executable; diagnose infrastructure before spending another model session.
4. **Retry requires budget, cleanup and fencing.** The total Implementer Run
   budget is explicit (`maxImplementerAttempts`, bounded `1..24`), counts
   every Run for the frozen spec including failed recoveries, and is checked
   before authorization and dispatch. Unresolved S3-D recovery or unconfirmed
   cleanup blocks. No stale dispatch token or Run identity is reused.
5. **Repair instructions are evidence-driven.** The new prompt includes the
   original objective, frozen scope, observed failure class/reason, attempt
   number, remaining attempt budget and a requirement to retest. It is not a
   verbatim retry.
6. **The oracle is protected.** A suspected oracle defect cannot be repaired
   by the Implementer. The operator must freeze a new ActionSpec with a new
   content/binding hash; the old candidate, verification and decision remain.
   New specs for the same Action are distinct immutable revisions.
7. **Resource truthfulness.** Unknown provider cost/quota remains `unknown`.
   The certification budget is an operator/live-test parameter, not a hidden
   permanent Factory-wide ceiling. Successful acceptance stops further work.

## Consequences

- New `campaign_recoveries` table and `CampaignImplementerRecoveryAuthorized`
  event; campaign projections expose recovery history and recovery count.
- `maxImplementerAttempts` is an explicit campaign creation parameter with a
  conservative default derived from spec attempts and repair cycles.
- Office shows recovery history, failure class, previous/new Run IDs and the
  attempt budget.
- Deterministic acceptance adds R1–R10: recovery, fencing, budget exhaustion,
  security/oracle blocking and final ACCEPTED lineage.
- Live certification is bounded per Project and remains TRUSTED_LOCAL; it is
  not isolation and does not authorize delivery/merge.

## Alternatives rejected

- **Retry the same Run in place:** loses the distinction between transport
  retries and a new implementation attempt, and cannot carry a new repair
  instruction cleanly.
- **Create a second Campaign for recovery:** breaks budgets, lineage and
  single-authority orchestration.
- **Let the Implementer edit the failing oracle:** converts a quality signal
  into self-authorization.
- **Retry security failures:** would launder unauthorized scope changes.
- **Escalate every failure to the Product Owner:** defeats autonomous bounded
  engineering.
