# ADR-0038 — Owner charter and delegated autonomy

**Status:** Proposed — 2026-11-08
**Supersedes:** none.
**Extends:** [ADR-0008](ADR-0008-authority-and-risk-classes.md), [ADR-0016](ADR-0016-dual-mode-autonomous-engineering.md), [ADR-0034](ADR-0034-model-capability-telemetry-and-agent-scope.md) and [ADR-0036](ADR-0036-owner-granted-autonomous-r0-intake.md).

## Context

The owner wants the Director to behave like a CEO: execute routine engineering
without per-request approval, and bring only genuinely critical decisions back.
Risk class alone is not sufficient. A change can be R1 in code but require a
credential, a production deploy or a public contract; another R2 change can be a
bounded internal refactor with independent review.

The owner also needs the ability to delegate without granting unlimited
authority or losing the ability to revoke it.

## Decision

1. **Owner Charter.** A durable, versioned owner charter defines domains,
   projects, objectives, risk appetite, budgets, notification rules, expiry and
   revocation. It is created by an authenticated owner and cannot be edited by a
   model or by the Director.
2. **Reserved matters.** The following always require an explicit owner decision
   or a specific owner-delegated exception: credentials and secrets, isolation
   and sandbox policy, production deployment, destructive data migrations,
   public API/contract commitments, security exceptions, policy/trust changes,
   budget outside the envelope, cross-project changes and irreversible
   operations.
3. **Separate execution, repair, delivery and policy autonomy.** A charter may
   permit autonomous execution and bounded repair for a domain while still
   requiring an owner decision for merge, deploy or credential effects.
4. **Risk is an input, not the whole decision.** R0 is autonomous under the
   existing policy. R1 may be autonomous with bounded repair and notification.
   R2 may be autonomous only inside an explicit domain charter with independent
   review. R3 requires an explicit charter and independent security review;
   reserved effects remain owner-controlled.
5. **Owner decisions are typed.** A decision presents the exact operation,
   candidate, effect, options, evidence, recommendation, deadline and default
   deny. A decision cannot be fabricated by a model; Core records the
   authenticated principal, revision and effect binding.
6. **Revocation and expiry.** Every delegation has a scope, expiry, revocation
   path and audit record. Revocation fences new starts and repairs; in-flight
   effects follow the existing cancellation/lease rules.

## Consequences

- The Director and Operation Manager can act more autonomously without turning
  every risk class into a click-through.
- reserved matters remain a hard boundary even when the code risk is low.
- The owner can delegate widely but retains audit, revocation and exception
  authority.
- A charter is not a model prompt; it is Core policy validated on each start,
  repair and delivery decision.

## Verification

- Deterministic tests cover expired, revoked, out-of-domain, over-budget and
  reserved-matter refusals.
- A real browser journey shows the owner granting a charter, the Director acting
  within it, and a reserved decision returning to the owner with exact evidence.
