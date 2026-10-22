# Software Factory Roadmap

This page is generated from `docs/product/capabilities.yaml`. Edit the registry and run `scripts/check-architecture-consistency --write-roadmap`.

## F1-S4-C — Completed governed multi-project baseline

Decision: `approved`

### Dependencies

- None specified.

### Capabilities

- CAP-001
- CAP-002
- CAP-003
- CAP-004
- CAP-005
- CAP-006
- CAP-007
- CAP-008
- CAP-009
- CAP-010
- CAP-011
- CAP-012

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

- CAP-013
- CAP-014
- CAP-015
- CAP-016
- CAP-017
- CAP-043

### Deliverables

- Intent interpretation
- bounded semantic assessment
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

### Real acceptance

- Natural-language request becomes an authorized executable plan through a real Paseo run without manual per-role orchestration

### Product Owner decisions

- Approve the frozen Action and standing-delegation limits before implementation


## F1-S4-E — Autonomous team composition

Decision: `proposed`

### Dependencies

- F1-S4-D

### Capabilities

- CAP-018
- CAP-019
- CAP-020
- CAP-021
- CAP-022
- CAP-023

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

- CAP-024
- CAP-025
- CAP-026

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

- CAP-027
- CAP-028
- CAP-029
- CAP-030
- CAP-044

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

- CAP-031
- CAP-032

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

- CAP-033
- CAP-034
- CAP-035

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

- CAP-036
- CAP-037
- CAP-038
- CAP-039
- CAP-040
- CAP-041
- CAP-042

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
