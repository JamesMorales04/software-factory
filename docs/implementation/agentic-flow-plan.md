# Agentic flow implementation plan

**Status:** proposed program F1-S10. The detailed design is in
[ADR-0037](../architecture/decisions/ADR-0037-agent-topology-context-and-composition.md)
and [ADR-0038](../architecture/decisions/ADR-0038-owner-charter-and-delegated-autonomy.md).
The typed boundary is [the agentic flow contract](../contracts/agentic-flow.md).

## Current state

The Rust product has one Lead assessment/proposal call, one Implementer per
WorkUnit, one semantic Reviewer, a deterministic Verifier and the deterministic
Quality Gate. `supervisor.rs` is an Integrator, not a semantic Operation
Manager. Director, Operation Manager, Explorer, Librarian, Planner, Spec Manager,
Repairer, Agent Composer, dynamic fleets, context handler, owner charter and
spec issue projection are not implemented.

## Target flow

```text
Owner -> Director -> Triage/Semantic Assessor -> Operation Manager
      -> Explorer/Librarian -> Planner -> Spec Manager
      -> Agent Composer -> Implementers + cheap ops -> Integrator
      -> Verifier -> Reviewers -> Quality Gate
      -> Repairer (on REPAIR_REQUIRED) -> Gate
      -> Spec issue/kanban projection -> delivery
```

## Phases

| Phase | Scope | Main capabilities | Acceptance |
|---|---|---|---|
| 0 | ADRs, contracts, topology, capability proposals | — | Documents reviewed; no behavior claim |
| 1 | Context handler, triage/stack, request kinds | CAP-057, CAP-058 | Informational/audit/change/repair differ; 70% compaction and handoff probes |
| 2 | Director, Operation Manager, concurrency/resource governance | CAP-059, CAP-034 | Two simultaneous requests coordinated in separate worktrees |
| 3 | Bounded repair | CAP-064 | Real REPAIR_REQUIRED becomes C+1 or blocks truthfully |
| 4 | Planner/Spec Manager and spec issue/kanban | CAP-060, CAP-061 | Frozen spec projected to issues and board without drift |
| 5 | Explorer/Librarian/Context7/skills | CAP-062, CAP-038 | KnowledgePack and temporary skill used by one participant |
| 6 | Dynamic fleets and cheap interpreter | CAP-063, CAP-036 | Multi-stack task compiles multiple implementers/reviewers |
| 7 | Owner charter and R1-R3 autonomy | CAP-065 | Owner only sees reserved matters |
| 8 | Real-model browser E2E and UI acceptance | CAP-066 | Browser-only real campaign, per-option matrix, no gaps |

## Rules

- Core remains the authority; no second orchestrator.
- No model-selected tools or identities.
- No silent scope widening, spec mutation or candidate reuse.
- No fixed concurrency cap; resource governance and backpressure are mandatory.
- Reviewer independence is a separate session/role; same model family is allowed.
- A handoff summary is not evidence; Core records remain authoritative.
- Every phase ships Office discoverability, states, error/empty/offline
  handling, browser acceptance and parity/evidence updates.
