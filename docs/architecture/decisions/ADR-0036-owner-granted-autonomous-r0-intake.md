# ADR-0036 — Owner-granted autonomous R0 intake

**Status:** Accepted — 2026-11-08 (Product Owner authorization in the autonomy/UX session)
**Supersedes:** none. It extends [ADR-0011](ADR-0011-native-campaign-orchestration.md), [ADR-0016](ADR-0016-dual-mode-autonomous-engineering.md), [ADR-0033](ADR-0033-provider-agnostic-review-and-bounded-autonomy.md) and [ADR-0034](ADR-0034-model-capability-telemetry-and-agent-scope.md).

## Context

The unified intake flow made the owner perform every step of a request:

1. confirm the subscription assumption;
2. run the Lead assessment;
3. freeze and review the plan;
4. approve and start the Campaign.

The Product Owner directed that selecting an autonomous execution profile and
sending an objective should be enough for bounded R0 work: the Lead should
assess, freeze and start without per-request UI approval, while Core remains the
authority. The owner also asked for a repeatable UI/UX acceptance process for
every future UI change.

A model cannot grant authority. The owner must therefore grant a durable,
bounded policy before the Lead can act. The policy is the authority; the Lead's
assessment and plan remain advisory evidence.

## Decision

1. **Owner-granted autonomous policy.** An execution-policy grant can be created
   with `autonomous=true` only when the exact frozen profile enables
   `auto_start` for `R0`. The grant records `subscription_confirmed=true` only
   when the owner confirms the subscription billing assumption once.
2. **No per-request subscription confirmation.** On the subscription route Core
   creates the request-scoped admission from the durable owner confirmation and
   the exact frozen profile/provider/settings revision. The Request workspace
   does not ask again.
3. **Core-owned autopilot.** Creating an `AUTONOMOUS` request makes Core run the
   assessment, freeze the plan and start the Campaign through
   `StartAuthority::Autonomous`, under the already-frozen delegation, profile,
   allowed paths, operations, progress policy and expiry. The background runner
   is idempotent for one request and is resumed after Core restart for requests
   that have no Campaign.
4. **Deterministic safety boundary.** Autonomous start is limited to:
   - R0 plans only;
   - an exact frozen profile revision whose `auto_start` includes R0;
   - an active, unexpired, non-revoked autonomous delegation;
   - the exact request/plan/spec/config/delegation/profile binding;
   - the fixed allowed paths and capabilities;
   - the deployment policy `FACTORY_ENABLE_AUTONOMOUS_START=1`.
5. **Human authority remains for protected decisions.** R1/R2/R3 work,
   policy/profile widening, cancellation, merge/delivery, budget or isolation
   changes, credential changes and non-R0 repairs remain explicit operator
   actions. The Lead model never grants authority, widens scope, changes the
   route or approves its own output.
6. **Truthful failure.** If assessment, subscription admission or autonomous
   start fails, Core records the lead failure and blocks the request with the
   reason instead of retrying silently or presenting a false success.
7. **UI/UX acceptance.** Every future Office UI change follows the
   [UI change acceptance skill](../../../.agents/skills/ui-change-acceptance/SKILL.md):
   enumerate every new/changed option, exercise each in a real browser, check
   responsive/accessibility/visual quality, and record candidate-bound evidence.
   A route smoke test is not acceptance of every option.

## Alternatives considered

- **Browser auto-clicking assessment and approval:** rejected as the only
  mechanism. It is not durable, stops when the browser closes, and would make
  the browser the authority for starting work.
- **Lead model approval:** rejected. Model output cannot grant authority or
  widen a policy.
- **Blanket autonomous start for every risk class:** rejected. R1+ remains
  human-authorized.
- **Per-request confirmation retained:** rejected by the Product Owner; it
  duplicates the durable policy and prevents a natural autonomous flow.

## Consequences

- An owner who enables `auto_start` R0 in an execution profile gets a natural
  “select profile, send objective, watch the Lead work” flow.
- The first use of a Project/profile still requires the one-time policy grant
  and subscription confirmation. Subsequent requests need no per-request
  click-through.
- Autopilot can still end `BLOCKED` or `FAILED` on model, provider, plan,
  toolchain or quality failures. Those states are visible and require an
  operator action; autonomous repair remains disabled. A new request starts
  from the Project base commit, so repairing a rejected candidate requires a
  Project based on that candidate or an explicit delivery step.
- The profile's `auto_start` declaration and the grant's `autonomous` bit are
  separate. Neither alone starts work.
- `TRUSTED_LOCAL` remains broad local access and is not isolation or
  certification.

## Verification

- DB tests cover autonomous grant persistence, delegation autonomy, R0 risk
  classes and autonomous request creation.
- The live Compose browser run created a new managed Project, enabled the R0
  autopilot profile, sent “crea un gestor de calendario”, and reached Gate
  `ACCEPTED` without per-request assessment or approval clicks.
- Later live hardening runs found and fixed generated-bytecode scope creep,
  reviewer timeout/provenance handling and a misleading `Executing` projection
  for terminal repair/not-tested gates. Those runs ended `REPAIR_REQUIRED` or
  `NOT_TESTED` on real findings; Core reported them as `Blocked`, not success.
  See [the hardening manifest](../../evidence/autopilot-r0-hardening-2026-11-08/manifest.json).
- Existing manual-review flows remain available for non-autonomous profiles.
