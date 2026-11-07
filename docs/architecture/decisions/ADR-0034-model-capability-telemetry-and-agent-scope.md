# ADR-0034 — Model capability, telemetry bounds and agent-proposed scope

**Status:** Accepted by the Product Owner on 2026-11-07. Supersedes the
productivity-cap dispositions in the 2026-10-28 Execution Policy V2 contract
where they conflict with this decision.
**Relates to:** ADR-0006, ADR-0010, ADR-0013, ADR-0029, ADR-0031, ADR-0033.

## Context

Execution Policy V2 removed cumulative campaign counters but left fixed
productivity bounds in the active model path:

- a 4,096-token implementer turn;
- an 8,192-token Lead/reviewer/model-call ceiling;
- fixed aggregate model-request timeouts;
- a provider-specific request count and an ineffective OpenCode
  `maxRetries: 0`;
- model-facing plan fields (`maxSessions`, `maxWallClockSecs`, repair counts);
- provider-settings and UI controls that were inert, misleading or enforced
  only on legacy paths.

These values differ by model and provider and caused healthy work to fail or
require a new delegation. They are not authority. The Product Owner directed
that token, time, retry and cost counts become telemetry, that the only
automatic brake be no-progress supervision, and that the planner/supervisor
propose the per-task scope inside a Core-validated outer boundary.

## Decision

1. **Model output is model/provider capability, not a fixed number.** The
   product does not impose a fixed per-request output limit on subscription
   routes. Providers may apply their own native limits; observed usage is
   recorded. A context overflow is handled by compaction or delegation, not by
   a token-counter failure.
2. **No total model-request deadline.** Latency is telemetry. Cancellation,
   process death and liveness checks remain operational signals; a slow but
   progressing provider call is not failed because of elapsed time.
3. **No fixed retry/request count and no retry-suppression config.** Every
   provider request is bound to the same Core reservation and recorded. A
   repeated identical failure or lack of new evidence triggers autonomous
   progress diagnosis. The no-progress circuit breaker is the automatic stop.
4. **Subscription cost is telemetry.** A provider-notional cost value does not
   reject a subscription call. Billable routes retain the operator's monetary
   envelope as the hard money authority.
5. **The planner/supervisor proposes per-task scope.** A plan contains paths,
   tool capabilities, risk, route and rationale. Core validates it against the
   project's outer boundary and freezes the accepted scope. The reviewer checks
   the actual candidate against the frozen scope. The operator does not choose
   every task's paths.
6. **Finding points, not fixed repair counts.** The reviewer proposes severity,
   points and evidence; Core validates canonical points and risk budgets.
   Critical/blocking findings cannot be waived by a model. The point budget and
   no-progress breaker govern repair; a human is required only for critical,
   irreversible or security-sensitive decisions.
7. **Legacy cap fields are historical evidence.** They are removed from active
   Office surfaces, new plan/model schemas and execution enforcement. Existing
   V1/V2 rows remain decodable read-only; new work uses the capability/telemetry
   policy. Legacy columns are archived before any schema removal.
8. **Security and quality boundaries remain.** Credentials, provider/model
   allowlists, exact egress request binding, one writer/lease fencing, exact
   candidate identity, immutable receipts, independent verifier/reviewer,
   measured usage provenance, protected project paths and no fallback are not
   productivity caps and remain enforceable.
9. **Reviewer and every model-backed role are selected in Office.** The frozen
   Execution Profile route is the authority. No global reviewer model constant
   or provider-name branch may override it. Native review transports receive the
   selected model; a missing route fails closed.
10. **Technical resource guards remain.** Broker body/connection limits, argv
    limits, memory/disk guards, command liveness and lease/grant TTL are safety
    boundaries, not productivity budgets. They must be labelled as such.

## Consequences

- A long, productive subscription campaign can run across provider-native
  token/time limits without an artificial Factory rejection.
- A stalled campaign is stopped by durable progress evidence, not by a clock or
  token counter. Progress evidence and diagnosis remain auditable.
- Legacy V1 campaigns retain their historical evidence but do not regain active
  authority. New work cannot create legacy caps from Office.
- The model-facing plan is narrower and no longer emits productivity counters.
- Real-model acceptance must prove high-usage/long-call behavior, multi-request
  turns, reviewer selection, no fallback and progress supervision.

## Verification

- Unit tests cover uncapped subscription turns, multiple bound egress requests,
  model calls above the former 8,192 limit, no `maxRetries` config, plan
  serialization without legacy counters, reviewer route selection and absent
  legacy UI controls.
- Compose/browser acceptance must show the Office no longer exposes the removed
  caps and that a deterministic multi-request Campaign reaches the Gate.
- Live-model acceptance remains a separate authorized run; this ADR does not
  itself certify a provider/model.
