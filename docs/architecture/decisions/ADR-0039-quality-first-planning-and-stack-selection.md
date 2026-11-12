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

## Owner decisions (2026-11-09)

The Product Owner resolved the open planning choices:

1. Every reviewer finding blocks acceptance and must be resolved
   automatically; there is no silent warning debt.
2. A stack change is scored with the same autonomy points/risk policy as other
   work; Core may authorize it within the frozen budget, and the owner is asked
   only when it is too large, irreversible, reserved or above budget.
3. Dependency acquisition supports both a digest-pinned vendor/cache and a
   brokered fetch; the project plan chooses one or both.
4. A plan review is mandatory for every lane, including `DIRECT`; the plan may
   be auto-approved by Core under the frozen policy, but it is never skipped.
5. Documentation is quality-first: the minimum set that actually protects the
   built product, not a fixed file count.
6. The Lead does not plan. It is a thin assessor/classifier; the Planner runs in
   every flow and the Spec Manager always runs for `FORMAL_SPEC`. OpenSpec is
   mandatory for `FORMAL_SPEC` and may be compact/optional for `DIRECT`.
7. OpenSpec is the repository-facing specification projection; Core remains the
   authority.
8. Reviewer roles must reflect architecture, code quality, security and testing,
   not one generic review.
9. Every technology used must have a current, source-cited skill pack. If it is
   missing or stale, the Librarian refreshes it before `FORMAL_SPEC` proceeds.

## Decision

1. **Right-size planning by route, not by one prompt.**
   Every change request is assessed into one of four planning lanes:
   `DIRECT` (bounded, existing stack, small), `DELEGATED` (multi-unit but no
   formal contract), `FORMAL_SPEC` (greenfield, large, ambiguous, public
   contract, data/schema, security boundary, missing architecture docs, or
   stack choice), and `REPAIR` (exact candidate). Every lane produces a plan
   and passes plan review; the lane changes depth and approval, not whether a
   plan exists. `FORMAL_SPEC` cannot be silently downgraded.

2. **Spec first; technology second.**
   A formal plan starts with observable behavior, user journeys, interfaces,
   non-goals, constraints, acceptance cases and negative cases. Technology
   choices belong in the architecture/stack decision, not in the functional
   specification. Every requirement must trace to a WorkUnit, a check and
   evidence.

3. **Stack selection is an explicit, evidence-based decision.**
   Precedence is: explicit owner constraints, then the existing project stack
   and conventions, then a model proposal validated by Core. A stack change is
   scored with the same autonomy points/risk policy as the rest of the work;
   Core may authorize it inside the frozen budget, and the owner is asked only
   when it is too large, irreversible, reserved or above budget. The proposal
   compares 2–3 realistic options against quality attributes (fit, security,
   maintainability, operability, ecosystem, cost, time to value, licensing,
   team/agent familiarity, exit cost). It records the selected option, rejected
   options, rationale, confidence, constraints and an ADR. The model may not
   invent an unavailable technology or silently replace a user-selected or
   existing stack.

4. **Capabilities, dependencies and knowledge are part of the decision.**
   A selected stack is valid only if its toolchain, libraries and checks can run
   in the frozen container topology. The Planner and Librarian inventory the
   available capabilities, research candidate dependencies and skills, and
   record license, maintenance, security and provenance evidence. The plan
   supports both a digest-pinned vendor/cache and a brokered dependency fetch,
   chosen per project. Missing dependency acquisition is an explicit constraint
   or an owner decision, not an assumed capability.

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

9. **Every reviewer finding blocks acceptance and is resolved automatically.**
   `critical`, `blocking`, `warning` and `info` findings are all actionable:
   acceptance requires zero open findings. The Gate routes each finding to a
   deterministic Finding Router: code defects go to a Repairer; spec gaps,
   ambiguity, architecture or stack issues go back to the Planner/Spec Manager
   as a new revision; security findings add the Security Reviewer and, where
   required, an owner decision. The campaign loops through bounded repair/replan
   until the finding is resolved or the policy exhausts its budget; exhaustion
   is an owner decision, never a silent acceptance. A model or reviewer cannot
   waive its own finding.

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

12. **Office owns the plan-review checkpoint for every lane.**
    The owner sees the plan before implementation for every request, including
    `DIRECT`: goals, non-goals, OpenSpec change (or compact plan), stack
    options/decision, architecture (when applicable), WorkGraph, docs plan,
    skills, checks, DoD and risks. The plan may be auto-approved by Core under
    the frozen charter/policy; the owner is asked only for a reserved matter or
    when the plan exceeds the autonomy budget. Core freezes the exact revision;
    no model output becomes authority.

13. **OpenSpec is the repository-facing specification projection.**
    A `FORMAL_SPEC` change always gets an `openspec/changes/<change-id>/` folder
    with `proposal.md`, `specs/`, `design.md` (when technical decisions exist)
    and `tasks.md`. `DELEGATED` uses it when the change needs a durable repo
    spec; `DIRECT` may use a compact Core plan instead. Core renders the
    projection deterministically from the frozen typed
    plan/spec/stack/architecture/WorkGraph/DoD; it is idempotent,
    revision-bound and never a second authority. `openspec/specs/` is updated on
    convergence and completed changes are archived. `openspec validate` is a
    deterministic check when the OpenSpec toolchain is available. The
    implementer updates task checkboxes; Core validates the artifact and
    evidence, not the checkbox alone.

14. **Reviewer roles reflect the plan.**
    The ParticipantPlan defines the required reviewer roles for the lane and
    assurance: Functional, Architecture, Code Quality, Security, Testing and
    Traceability, and Operability/Performance where applicable. Each role is a
    separate read-only session with a role-specific prompt and evidence
    requirements. The Gate requires every required role to return an executed
    PASS with no open findings. The current single generic reviewer is a
    temporary limitation, not the target topology.

15. **The Lead assesses; the Planner plans.**
    The Lead is a thin assessor and request classifier; it never authors the
    plan. The Explorer inventories the repository and docs; the Librarian
    researches knowledge, dependencies and skills when there is a gap; the
    Planner runs in every flow; the Spec Manager always runs for
    `FORMAL_SPEC`. The Director/Operation Manager are orchestration projections,
    not additional model authorities. The Integrator remains deterministic.

16. **Planning and execution are durable, idempotent and replayable.**
    The planning lifecycle is a durable state machine with explicit
    checkpoints, idempotency keys, bounded retries and an outbox for external
    projections (GitHub, OpenSpec, issues). A crash, provider failure or
    compaction resumes from the last durable checkpoint. External effects are
    reconciled before retry; duplicate dispatch, duplicate issues and duplicate
    repairs are rejected by Core identity.

17. **Technology skill packs and documentation freshness.**
    Assessment detects the stack and checks for current, versioned, source-cited
    skill packs and documentation. Missing or stale knowledge goes to the
    Librarian, which fetches official documentation and composes a reviewable
    skill pack; the Planner uses it for stack/architecture/dependency decisions,
    and implementers/reviewers receive it as read-only context. Skill packs
    record official source URLs, supported versions, best practices,
    anti-patterns, commands, security/deprecation notes, `last_verified_at` and
    `review_after`. Stale packs trigger refresh and block `FORMAL_SPEC` until
    refreshed or explicitly waived by the owner. Skills remain context, never
    tools or authority. The documentation baseline is tiered and quality-first:
    README, concise `AGENTS.md`, C4 context/container or module map, ADRs,
    DoD/test strategy and OpenSpec change for every project; API/data/runbook/
    threat-model for services; design-system/accessibility/browser matrix for
    UI; public API/semver for libraries. Docs are reviewed in the same change
    and validated by deterministic checks.

## Consequences

- Small `DIRECT` changes keep a fast path; complex and greenfield work gets
  deeper planning and more evidence.
- More model calls, artifacts and DB records are introduced. They are bounded
  and right-sized; token/time/cost remain telemetry.
- The existing Lead plan is retired; `DIRECT` uses a compact Planner plan and
  plan review, while `DELEGATED`/`FORMAL_SPEC` use the WorkGraph and (for
  `FORMAL_SPEC`) the Spec Manager.
- The Factory must add dependency acquisition/vendor support, a capability
  inventory and a technology-skill refresh loop; otherwise stack choice is
  limited to what the sandbox can run and quality depends on stale knowledge.
- The Office needs a plan-review surface and a truthful "planning" stage; a
  request cannot jump from assessment to execution without showing the frozen
  artifacts. The review is mandatory for every lane.
- OpenSpec becomes the durable repository-facing plan and spec projection; Core
  still owns the frozen typed records and hashes.
- Reviewer specialization adds independent role sessions and provisioning of
  multiple reviewer identities; one generic reviewer is no longer sufficient.
- Every finding blocks acceptance and is routed to repair or replanning; a
  warning is never silently accepted.
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
  depth, docs requirements, DoD, finding routing, automatic repair/replan,
  OpenSpec projection and dependency blockers.
- Golden fixtures: greenfield Rust CLI, greenfield service with dependencies,
  existing Rust service, Node/TypeScript UI, Python service, missing docs,
  user-specified stack, large objective, warning-only candidate and
  architecture/spec finding.
- A real-model E2E campaign must show: frozen spec, 2–3 stack options or a
  justified single option, ADR, multiple WorkUnits, docs update, OpenSpec
  change, DoD checks, all required reviewer roles, automatic finding resolution
  and `ACCEPTED` only with zero open findings.
- Office per-option browser acceptance for the plan review, stack options,
  OpenSpec artifacts, docs, reviewer roles, repair/replan and responsive/
  accessibility states.
- No behavior claim is made until the candidate-bound evidence exists.
