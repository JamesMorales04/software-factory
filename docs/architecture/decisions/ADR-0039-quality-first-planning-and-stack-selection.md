# ADR-0039 — Quality-first planning, stack selection and living documentation

**Status:** Proposed — 2026-11-09
**Supersedes:** none.
**Extends:** [ADR-0025](ADR-0025-container-native-and-office-parity.md), [ADR-0033](ADR-0033-provider-agnostic-review-and-bounded-autonomy.md), [ADR-0037](ADR-0037-agent-topology-context-and-composition.md), [ADR-0038](ADR-0038-owner-charter-and-delegated-autonomy.md), [ADR-0006](ADR-0006-early-development-compatibility.md).

## Context

The 2026-11-09 gym-manager live run reached `ACCEPTED` on the first clean
attempt, but only after the planning path had already chosen a very narrow
product slice. The evidence is in
[docs/evidence/gym-manager-2026-11-09/](../evidence/gym-manager-2026-11-09/README.md).

The run exposed a structural gap, not just a prompt-quality gap:

- The Home request used the Lead proposal path, whose prompt forces
  `workUnits` to contain **exactly one implementation unit** and tells the model
  to choose the "smallest needed subset" of paths and capabilities
  (`native/src/lead_model.rs`).
- The richer `Planner`/`Spec Manager` path exists, but it is a manual Director
  action. The default request lifecycle does not automatically select it.
- `agentic_triage_context` hardcodes `architecture_uncertainty`,
  `public_contract`, `data_or_schema` and `security_boundary` to `false`
  (`native/src/bin/core.rs`). A greenfield objective with a one-unit Lead
  proposal therefore routes to `DIRECT`, not `FORMAL_SPEC`.
- The Planner prompt says "Never invent a file, dependency or technology"
  (`native/src/bin/core.rs`). That is safe for authority, but it also prevents
  deliberate stack and library selection.
- `StackProfile` is only detected from existing manifests
  (`native/src/agentic.rs`). It records what exists; it never chooses what
  should exist when the user has not chosen.
- The frozen `Generic` quality profile requires only build, test, functional and
  negative checks. It does not require format, lint, typecheck, docs, security,
  ADR, stack-decision or traceability evidence.
- The reviewer receives the objective, checks, diff and source files, but not a
  frozen spec, stack decision, architecture constraints, documentation plan or
  Definition of Done. Warnings can be accepted without an explicit owner
  decision.
- The execution sandbox is offline. A std-only crate works; a new crate,
  framework or package cannot be fetched inside the check container unless the
  project already vendors it. The planner does not model this constraint.

The owner wants a planning system that, when the user has not chosen the
technology, selects an appropriate stack and architecture from the real need,
uses available tools and maintained libraries, adopts necessary skills, writes
or updates project documentation (including `AGENTS.md`), defines a real
Definition of Done, and keeps working through bounded replanning instead of
stopping halfway or choosing the easiest output.

The contract already allows a model to propose a stack
([agentic flow contract](../contracts/agentic-flow.md)). The implementation
does not yet make that a first-class, validated planning artifact.

## Decision

1. **Right-size planning by route, not by one prompt.**
   Every change request is assessed into one of four planning lanes:
   `DIRECT` (bounded, existing stack, small), `DELEGATED` (multi-unit but no
   formal contract), `FORMAL_SPEC` (greenfield, large, ambiguous, public
   contract, data/schema, security boundary, missing architecture docs, or
   stack choice), and `REPAIR` (exact candidate). The lane selects the planning
   artifacts and approval depth. `FORMAL_SPEC` cannot be silently downgraded.

2. **Spec first; technology second.**
   A formal plan starts with observable behavior, user journeys, interfaces,
   non-goals, constraints, acceptance cases and negative cases. Technology
   choices belong in the architecture/stack decision, not in the functional
   specification. Every requirement must trace to a WorkUnit, a check and
   evidence.

3. **Stack selection is an explicit, evidence-based decision.**
   Precedence is: explicit owner constraints, then the existing project stack
   and conventions, then a model proposal validated by Core. The proposal
   compares 2–3 realistic options against quality attributes (fit, security,
   maintainability, operability, ecosystem, cost, time to value, licensing,
   team/agent familiarity, exit cost). It records the selected option, rejected
   options, rationale, confidence, constraints and an ADR. The model may not
   invent an unavailable technology or silently replace a user-selected or
   existing stack.

4. **Capabilities and dependencies are part of the decision.**
   A selected stack is valid only if its toolchain, libraries and checks can run
   in the frozen container topology. Missing dependency acquisition is an
   explicit constraint or an owner decision, not an assumed capability. The
   planner must choose between an approved offline cache/vendor path, a
   brokered dependency-acquisition step, or a simpler stack that fits the
   available capabilities.

5. **Architecture is a first-class frozen artifact.**
   The architecture plan records context, components, boundaries, data,
   interfaces, deployment, quality attributes, risks, tradeoffs and ADR
   references. It must fit the service-placement and container-native policies;
   a container is not isolation and no plan may imply otherwise.

6. **Documentation is living code.**
   Discovery reads `AGENTS.md`, `README`, architecture docs, ADRs and
   conventions. If the project has no usable constitution, the plan creates a
   minimal one: build/test/lint commands, structure, conventions, constraints,
   docs pointers and security rules. The plan also creates or updates the
   architecture overview and ADRs when an architecturally significant decision
   is made. Documentation is part of the Definition of Done and is updated in
   the same change as the code; an accepted change cannot leave required docs
   absent or stale.

7. **Vertical slices and real depth.**
   A large or greenfield objective cannot be planned as one implementation
   unit. The Planner produces 1–24 units; a `FORMAL_SPEC` objective needs
   multiple reviewable vertical slices, plus explicit documentation and
   quality work. Each unit has one responsibility, 2–6 acceptance checks,
   dependencies, scope, evidence and a clear handoff. Horizontal "all database,
   then all services" decomposition is rejected for value delivery.

8. **Definition of Done is explicit and executable.**
   Project-level DoD includes build, tests, format/lint/typecheck where
   applicable, functional/negative checks, security/dependency checks,
   required docs/ADR updates and no unowned blocking findings. Unit-level
   acceptance is separate. The Worker cannot finalize a unit or candidate until
   its checks pass; the Gate cannot accept without the required evidence.

9. **Warnings are not silent acceptance.**
   `critical` and `blocking` findings always stop acceptance. For
   `FORMAL_SPEC` and `ELEVATED`/`CRITICAL` assurance, warnings also block unless
   an authenticated owner accepts the exact warning, rationale and follow-up
   debt. Accepted debt is recorded and projected to the issue/kanban. A model
   or reviewer cannot accept its own warning.

10. **Planning artifacts are durable and continuation is bounded.**
    The frozen plan, spec, stack/architecture decision and DoD are stored by
    Core and made available to every unit and handoff. Context compaction must
    reload them; a summary never replaces them. A stalled unit triggers a
    bounded diagnosis/replan within the policy before the campaign can stop.
    R0 may continue autonomously inside the charter; R1–R3 follow owner
    decisions. Telemetry (tokens, time, cost) is not a productivity cap.

11. **Skills are selected, licensed and scoped.**
    Planning records required skill/expertise needs, checks the existing skill
    registry, and only adopts a skill after license review and scope approval.
    Skills remain read-only context; they never gain tools, authority or the
    right to widen the plan. Missing knowledge is researched and cited, not
    invented.

12. **Office owns the human checkpoint.**
    The owner sees the plan before implementation for `FORMAL_SPEC` (or when a
    policy requires it): goals, non-goals, stack options/decision, architecture,
    WorkGraph, docs plan, skills, checks, DoD and risks. The owner can amend or
    reject; Core freezes the exact revision. The default remains autonomous
    where the owner has authorized it, but no model output becomes authority.

## Consequences

- Small `DIRECT` changes keep a fast path; complex and greenfield work gets
  deeper planning and more evidence.
- More model calls, artifacts and DB records are introduced. They are bounded
  and right-sized; token/time/cost remain telemetry.
- The existing Lead plan remains valid for `DIRECT`, but the Home lifecycle must
  route `DELEGATED`/`FORMAL_SPEC` through the Planner/Spec Manager and the
  WorkGraph execution path.
- The Factory must add dependency acquisition/vendor support and a capability
  inventory; otherwise stack choice is limited to what the sandbox can run.
- The Office needs a plan-review surface and a truthful "planning" stage; a
  request cannot jump from assessment to execution without showing the frozen
  artifacts.
- Accepted warnings become explicit debt, not invisible quality loss.
- Historical plans remain readable; new fields are additive and optional until
  the route requires them.

## Alternatives considered

- **Only improve the prompt.** Rejected: the one-unit constraint, route floors,
  missing artifacts and offline dependency constraint are structural.
- **Always use the full formal spec.** Rejected: it over-plans small repairs and
  increases cost/latency; use lanes.
- **Let the model choose the stack and tools freely.** Rejected: model output
  would grant capability and authority; Core must validate availability,
  policy and scope.
- **Keep the single Lead plan and add a stack field.** Rejected: it cannot
  provide multi-unit depth, docs, traceability or independent review against a
  frozen architecture.
- **Add an external planner/orchestrator.** Rejected: conflicts with Core
  authority and the container-native topology.

## Verification

- Deterministic tests for route floors, stack precedence, capability fit, plan
  depth, docs requirements, DoD, warning acceptance and dependency blockers.
- Golden fixtures: greenfield Rust CLI, greenfield service with dependencies,
  existing Rust service, Node/TypeScript UI, Python service, missing docs,
  user-specified stack, large objective and warning-only candidate.
- A real-model E2E campaign must show: frozen spec, 2–3 stack options or a
  justified single option, ADR, multiple WorkUnits, docs update, DoD checks,
  independent reviewer evidence and `ACCEPTED` only with complete evidence.
- Office per-option browser acceptance for the plan review, stack options,
  docs, warnings and responsive/accessibility states.
- No behavior claim is made until the candidate-bound evidence exists.
