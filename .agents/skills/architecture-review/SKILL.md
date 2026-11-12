---
name: architecture-review
description: Review a Factory change for module boundaries, dependency direction, data ownership, ADR fidelity, service placement, coupling, interface shape and evolution cost. Use for new modules, traits, crates, processes, containers, APIs, persistence entities, agent roles or any change that crosses an architectural boundary.
---

# Architecture review

Use this skill read-only. It cannot approve a candidate, waive an ADR or grant a
new process, credential boundary or authority path.

## 1. Establish the change boundary

- Which capability changes?
- Which module/process owns the decision and the data?
- Which boundaries are crossed: HTTP, DB, provider, Worker, browser, GitHub,
  container, filesystem?
- Which ADRs, contracts and standards apply?
- Is this a local change or a change to the architecture?

A file move, trait addition or new container is not an architecture change by
itself. A change in authority, ownership, consistency or failure semantics is.

## 2. Dependency direction

- Domain policy must not import transport, persistence row shapes, provider SDKs
  or UI types.
- A trait is justified by a real boundary, a second implementation or a test
  seam; not by symmetry.
- No cyclic modules. Shared policy moves to a third module or a narrow value.
- Adapters translate at the boundary; they do not own policy.
- Core owns authority, budgets, acceptance and external delivery effects.
- A second scheduler, policy engine, tool path, credential path or merge path
  is a review failure unless superseded by an accepted ADR.

## 3. Data ownership and consistency

- One writer owns each durable fact; projections are derived, not authoritative.
- Transactions have explicit boundaries; no I/O/lock across `.await`.
- Idempotency and fencing bind the exact candidate/lease/generation.
- Migrations are forward-safe, tested from a clean database and do not silently
  discard data.
- Events are durable and ordered; outbox/retry is bounded and idempotent.
- Cache/snapshot data has an invalidation and provenance story.

## 4. Interface shape

- Does the interface expose a domain result or a transport/DB leak?
- Is the failure model explicit and typed?
- Is the input parsed into a validated value once?
- Is the output bounded and redacted?
- Can a caller misuse ordering, identity or optional fields?
- Is the interface minimal for the real callers?
- Is versioning/compatibility appropriate for the early-development policy?

## 5. Service placement

Before accepting a new process/container/sandbox, require the service placement
assessment:

- why an in-process module is insufficient;
- trust/credential boundary;
- state and lifecycle owner;
- resource limits and failure mode;
- remote Worker impact;
- test evidence;
- why a container is not being described as certified isolation.

Default to an in-process module. Separate only for a real trust, credential,
crash or scaling boundary.

## 6. Operability and evolution

- Startup/shutdown/recovery are part of the design, not an afterthought.
- The design has an exit cost and a migration path.
- A new dependency is justified and maintained.
- The design can be tested at L0–L5 without a paid model.
- The failure modes are observable and diagnosable.
- The change does not make the next safe change harder for a cosmetic gain.

## 7. Review checklist

- Does the change add a second authority or policy path?
- Does a model output decide a tool, scope, budget or acceptance?
- Is the data owner unambiguous?
- Is the transaction/lease/candidate identity exact?
- Is the boundary testable without the whole system?
- Is the ADR still true, or does it need supersession?
- Is the container/process claim honest about isolation?
- Is the complexity proportional to the requirement?

## 8. Findings

```text
Candidate: <SHA>
Boundary:  <capability and modules>
Findings:
  - id: ARCH-001
    severity: BLOCKER / REQUIRED / OPTIONAL
    file: path:line
    evidence:
    impact:
    correction:
    verification:
ADR impact: <none / update / supersede>
Residual:  <NOT_TESTED / BLOCKED>
```

## 9. Stop conditions

Stop and report if:

- the change needs a new authority, credential boundary or isolation claim;
- an accepted ADR would be contradicted;
- the only design requires a second scheduler/policy/tool path;
- data ownership or migration safety is unclear;
- the change would make the installed product depend on development tooling.

## 10. References

- [MASTER](../../../docs/architecture/MASTER.md)
- [Service placement](../service-placement/SKILL.md)
- [Container-native engineering](../container-native-engineering/SKILL.md)
- [Code quality](../../../docs/engineering/code-quality.md)
- [ADRs](../../../docs/architecture/decisions/README.md)
- [Agentic flow contract](../../../docs/contracts/agentic-flow.md)
