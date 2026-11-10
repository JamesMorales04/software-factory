# ADR-0037 — Agent topology, context handling and dynamic composition

**Status:** Proposed — 2026-11-08 (design authority in the owner-approved agentic-flow program)
**Supersedes:** none.
**Extends:** [ADR-0016](ADR-0016-dual-mode-autonomous-engineering.md), [ADR-0025](ADR-0025-container-native-and-office-parity.md), [ADR-0033](ADR-0033-provider-agnostic-review-and-bounded-autonomy.md), [ADR-0034](ADR-0034-model-capability-telemetry-and-agent-scope.md) and [ADR-0036](ADR-0036-owner-granted-autonomous-r0-intake.md).

## Context

The current Rust product has a single Lead assessment/proposal call, one
Implementer per WorkUnit, one semantic Reviewer, a deterministic Verifier and a
deterministic Quality Gate. The target product needs a durable owner-level
Director, an Operation Manager per operation, typed triage, a Planner and Spec
Manager, Explorer and Librarian discovery, temporary skills, dynamic
implementer/reviewer composition and bounded repair.

The Product Owner approved the following direction:

- triage adapted to Factory: informational, audit, change and repair;
- separate `AgentTopology` from the frozen `ExecutionProfile`;
- R0 autonomous and R1–R3 delegated through an owner charter and reserved
  matters;
- reviewer independence through a separate session and role, not a mandatory
  different provider/model family for now;
- Context7 MCP plus other ranked sources for the Librarian;
- temporary skills promoted after two successful uses and reevaluated weekly;
- free navigation only for the read-only Librarian;
- a context handler with 70% effective-context compaction and a handoff after at
  most two consecutive compactions;
- the owner-level role named Director, the per-operation semantic role named
  Operation Manager, and the existing integration process named Integrator;
- no fixed product concurrency cap, but resource-governed backpressure and a
  worktree per operation.

## Decision

1. **Separate topology from authority.** `AgentTopology` declares available
   canonical roles, specializations, provider/model routes, tool packs,
   concurrency and cost classes. `ExecutionProfile` remains the frozen authority
   envelope. A request freezes the exact topology revision and the exact profile
   revision it uses.
2. **Canonical roles are Lead/Director, Operation Manager, Explorer, Librarian,
   Planner, Spec Manager, Implementer, Reviewer, Repairer and Semantic
   Assessor.** The existing `supervisor.rs` is renamed conceptually to
   Integrator; it is not an LLM and holds no semantic authority. Small typed
   judgments use a bounded Semantic Assessor rather than a permanent agent.
3. **Triage is HYBRID.** A model proposes intent, scope meaning, decomposition
   and formalization signals. Core applies deterministic floors for request
   kind, route, assurance, risk, required validation and minimum review. Model
   output cannot lower a floor or create authority.
4. **`ParticipantPlan` is compiled deterministically.** WorkGraph, topology,
   route, assurance, CandidateImpact, skills, tool catalog, budgets and path
   conflicts determine roles, counts, specializations, tools and output
   contracts. A model cannot invent a participant identity or widen a tool.
5. **Context handling is a runtime service, not an authority.** Owner, operation
   and session context are separated. Compaction triggers at 70% of effective
   context; tool-result clearing and structured notes are preferred first. After
   at most two consecutive compactions, the context handler audits the summary
   and performs a handoff at a stable milestone when needed. The handoff
   references durable artifacts; it never replaces evidence.
6. **Repair is a first-class bounded operation.** A Gate `REPAIR_REQUIRED`
   produces a RepairPacket bound to the exact candidate. The Repairer works from
   that candidate, returns a ChangeSet, and Core creates candidate C+1,
   invalidates C evidence and revalidates. R0 may be autonomous; R1 is bounded;
   R2/R3 follow the owner charter and reserved matters.
7. **Specs can be projected to GitHub issues and a kanban.** Core remains the
   source of truth. A deterministic Spec Publisher creates/updates authorized,
   idempotent issues; comments and edits are operator context and never silently
   mutate a frozen spec. A Core-native board is the fallback.
8. **The owner charter governs delegation.** The owner sets reserved matters,
   domains, risk appetite, budgets and expiry. The Director and Operation
   Manager act within the charter; they cannot change their own authority,
   trust gates, credentials or delivery policy.

### Amendment 2026-11-09 — reviewer independence

Reviewer independence is defined by **identity, process, token and authenticated
principal**, not by provider or model diversity. Each required reviewer role is
staffed by a distinct configured identity; Core derives the principal and role
from trusted configuration, and a receipt whose `principal_id` or reviewer role
does not match the authenticated identity is rejected. The ParticipantPlan and
Gate preserve the required reviewer count and role assignments. Two reviewers
using the same provider/model are independent when their principals and
processes differ. Provider/model diversity may be recorded as a non-blocking
observation only; it never gates a plan or acceptance. If the deployment cannot
staff every required role, Core fails closed with
`REVIEWER_INDEPENDENCE_REQUIRED` rather than silently reducing the plan.

## Consequences

- The current single-Lead request remains a valid DIRECT path; it becomes one
  route in a broader topology rather than the only flow.
- More model calls and more durable records are introduced. Cost and latency are
  telemetry, not authority; no-progress and resource governance remain.
- The product can add specialist roles without putting all of them in the
  ExecutionProfile or starting them for every request.
- A second orchestrator (an external agent harness or controller) is not introduced.
  Core remains the Rust authority; provider adapters remain direct.
- The full program is delivered in slices. No status is claimed beyond the
  evidence of the exact candidate.

## Alternatives considered

- **Keep the single Lead and add prompts only:** rejected; it cannot provide
  independent planning, specialization, discovery, decomposition, repair or
  context rotation with auditable identities.
- **Let a model choose agents and tools:** rejected; it would grant authority to
  model output.
- **Adopt an external agent-harness controller as a second orchestrator:** rejected;
  it conflicts with Core ownership and the container-native service placement.
- **Force a new agent after every compaction:** rejected by the context research;
  rotation follows a threshold, a milestone and a summary audit.

## Verification

- Phase 0 freezes the contracts and capability proposals.
- Later phases require deterministic tests for triage floors, participant
  composition, context handoff, repair, spec issue projection, owner charter and
  concurrency, plus real browser acceptance with real models.
- No phase is accepted from model output alone; the deterministic Quality Gate
  and candidate-bound evidence remain authoritative.
