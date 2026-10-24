# Software Factory Roadmap

This page is generated from `docs/product/capabilities.yaml`. Edit the registry and run `cargo run --manifest-path native/Cargo.toml --bin factory-architecture-check -- --root . --write-roadmap`.

## F1-S4-C — Completed governed multi-project baseline

Decision: `approved`

### Dependencies

- None specified.

### Capabilities

- CAP-001 — Registered projects (approved, in_progress)
- CAP-002 — Action and frozen specification (approved, in_progress)
- CAP-003 — Campaign orchestration (approved, in_progress)
- CAP-004 — Worker and provider integration (approved, in_progress)
- CAP-005 — Candidate revision lineage (approved, in_progress)
- CAP-006 — Independent verification and review (approved, in_progress)
- CAP-007 — Deterministic Quality Gate (approved, in_progress)
- CAP-008 — Scoped capability grants (approved, in_progress)
- CAP-009 — Factory-managed browser capability (approved, in_progress)
- CAP-010 — Bounded deterministic recovery (approved, in_progress)
- CAP-011 — Factory Office (approved, in_progress)
- CAP-012 — Durable events and telemetry (approved, in_progress)

### Deliverables

- Project registry and scoped capabilities
- Core Campaign pipeline
- Worker-managed browser server
- bounded C-07 Vlog live path

### Exclusions

- Deployment readiness
- SEC-01 isolation
- live corrective repair
- autonomous GitHub delivery

### Risks

- TRUSTED_LOCAL has broad host access
- SQLite contention is unresolved

### Deterministic acceptance

- S1 through S4-C candidate-bound regression and negative oracles

### Real acceptance

- C-07 Vlog at tested Factory head 9567bc5 and runtime 14133cb7 only

### Product Owner decisions

- Historical Product Owner UAT attestation recorded; no further paid session authorized


## F1-S4-D — Leadership and planning

Decision: `proposed`

### Dependencies

- F1-S4-C

### Capabilities

- CAP-013 — Intelligent Lead (proposed, in_progress)
- CAP-014 — Intent and semantic assessment (proposed, in_progress)
- CAP-015 — Frozen executable planning (proposed, in_progress)
- CAP-016 — Dependency-aware WorkGraph (proposed, in_progress)
- CAP-017 — Standing delegation contract (proposed, in_progress)
- CAP-022 — Parallel WorkUnits (proposed, not_implemented)
- CAP-043 — Autonomous product development (approved, in_progress)

### Deliverables

- Intent interpretation
- bounded semantic assessment
- decision-mechanism selection contract
- frozen plan
- WorkGraph validation
- standing authorization contract

### Exclusions

- Autonomous merge
- permanent specialist team
- self-development

### Risks

- Model plans may overrun scope or hide ambiguity

### Deterministic acceptance

- Reject self-authorizing plans
- invalid dependencies
- budget overflow
- stale authorization and unsupported model routes
- Compare CAP-014 judgments with a reasonable deterministic baseline on representative tasks, including ambiguous engineering-effort estimates

### Real acceptance

- Natural-language request becomes an authorized executable plan through a real direct-provider run without manual per-role orchestration
- Independently demonstrate better practical semantic decision quality than the deterministic baseline without granting model output Core authority

### Product Owner decisions

- Approve the frozen Action and standing-delegation limits before implementation


## F1-S4-E — Autonomous team composition

Decision: `proposed`

### Dependencies

- F1-S4-D

### Capabilities

- CAP-018 — Agent Composer (proposed, not_implemented)
- CAP-019 — Operation Supervisor (proposed, not_implemented)
- CAP-020 — Specialist agent roles (proposed, not_implemented)
- CAP-021 — Independent semantic Reviewer (proposed, not_implemented)
- CAP-023 — Context and knowledge resolver (proposed, not_implemented)

### Deliverables

- Minimal specialist selection
- bounded supervision
- parallel WorkUnits
- semantic review
- context assembly and replanning

### Exclusions

- Always-on fixed agent roster
- external skill promotion

### Risks

- Coordination overhead
- conflicting edits
- reviewer independence

### Deterministic acceptance

- Validate role/tool separation
- dependency ordering
- work-unit fencing and bounded repair

### Real acceptance

- Genuine multi-unit engineering task integrates cleanly with independent verification and a corrective cycle

### Product Owner decisions

- Approve role permissions and resource ceiling


## F1-S5 — Governed delivery

Decision: `proposed`

### Dependencies

- F1-S4-E

### Capabilities

- CAP-024 — GitHub PR delivery (proposed, not_implemented)
- CAP-025 — CI and review reconciliation (proposed, not_implemented)
- CAP-026 — Conditional R0 merge (proposed, not_implemented)

### Deliverables

- PR creation
- CI and review observation
- exact-SHA delivery receipts
- conditional R0 eligibility

### Exclusions

- Unconditional merge
- controller binary promotion

### Risks

- Credential misuse
- branch protection drift
- duplicate external effects

### Deterministic acceptance

- Reject stale SHA
- unapproved risk
- missing CI/review
- revoked grant and duplicate webhook effects

### Real acceptance

- Create a real PR and reconcile CI and review at the exact head; exercise eligible R0 merge only after separate authorization

### Product Owner decisions

- Protected delivery credential and merge-policy approval


## F1-S5-M — Autonomous maintenance pilot

Decision: `proposed`

### Dependencies

- F1-S5

### Capabilities

- CAP-027 — Persistent maintenance objectives (proposed, not_implemented)
- CAP-028 — Project health monitoring (proposed, not_implemented)
- CAP-029 — Incident diagnosis and Action creation (proposed, not_implemented)
- CAP-030 — Maintenance outcome measurement (proposed, not_implemented)
- CAP-044 — Autonomous product maintenance and evolution (approved, not_implemented)

### Deliverables

- Persistent objective
- repository health observation
- incident triage
- corrective Action
- outcome measurement

### Exclusions

- Factory live controller as first target
- continuous LLM polling

### Risks

- False incident classification
- alert loops
- excess spend

### Deterministic acceptance

- Deduplicate events and enforce authorization
- expiry
- budget and Action identity

### Real acceptance

- External fixture incident is detected
- repaired
- independently accepted and delivered through governed PR

### Product Owner decisions

- Approve monitored project
- signals
- standing limits and intervention classes


## F1-S6 — Guarded self-development

Decision: `proposed`

### Dependencies

- F1-S5-M

### Capabilities

- CAP-031 — Stable-controller self-development (proposed, not_implemented)
- CAP-032 — Promotion and rollback (proposed, not_implemented)

### Deliverables

- Stable controller A
- isolated candidate B
- independent promotion and rollback

### Exclusions

- GitHub merge as implicit deployment
- candidate controlling its own promotion

### Risks

- Control-plane compromise
- failed rollback
- shared-state corruption

### Deterministic acceptance

- Candidate B cannot alter controller A or promotion authority; rollback remains possible

### Real acceptance

- A governs B through a real change
- independent verification
- controlled promotion and rollback drill

### Product Owner decisions

- Protected promotion and isolation approval


## F1-S7 — Persistent maintenance and portfolio

Decision: `proposed`

### Dependencies

- F1-S5-M

### Capabilities

- CAP-033 — Portfolio Manager (proposed, not_implemented)
- CAP-034 — Resource Governor (proposed, not_implemented)
- CAP-035 — Durable maintenance scheduling (proposed, not_implemented)

### Deliverables

- Recurring objectives
- portfolio fairness
- resource reserves
- cross-project prioritization

### Exclusions

- Unlimited autonomous objectives
- single project monopolizing capacity

### Risks

- Starvation
- unnoticed drift
- accumulated maintenance debt

### Deterministic acceptance

- Enforce project fairness
- leases
- reservations
- expiry and restart recovery

### Real acceptance

- Multiple projects receive scheduled improvements across restarts with observable outcomes

### Product Owner decisions

- Approve portfolio priorities and resource allocation boundaries


## LATER — Evaluated extensions

Decision: `proposed`

### Dependencies

- F1-S7

### Capabilities

- CAP-036 — Adaptive Model Router (proposed, not_implemented)
- CAP-037 — Model evaluation and route promotion (proposed, not_implemented)
- CAP-038 — Skill Registry and admission (proposed, not_implemented)
- CAP-039 — Remote Workers (proposed, not_implemented)
- CAP-040 — Extended Factory Office (proposed, not_implemented)
- CAP-041 — Reliability analytics (proposed, not_implemented)
- CAP-042 — Optional context integrations (proposed, not_implemented)

### Deliverables

- Adaptive routing
- skill evaluation
- remote Workers
- extended Office
- reliability analytics

### Exclusions

- Unproven integrations as default dependencies
- autonomous privilege expansion

### Risks

- Provider variability
- skill supply chain
- remote security and operating cost

### Deterministic acceptance

- Validate compatibility
- authorization
- revocation and provenance for each extension

### Real acceptance

- Each extension demonstrates measured value and candidate-bound external behavior before promotion

### Product Owner decisions

- Ratify each extension's protected authority and cost ceiling separately
