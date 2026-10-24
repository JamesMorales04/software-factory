# Software Factory capability decisions

**Status:** authoritative for the selected product capability portfolio.
**Last reviewed:** 2026-10-24 against migration baseline
`102d7ba29295554bd61ae71956347a025d219d7b` and PR #17 implementation
`8c627bebf7f8245cc5ee4f904d70825017bd33f4`.

This record is self-contained. It preserves the decisions, boundaries and
acceptance expectations of every selected capability without retaining a
dependency on any predecessor product, repository or runtime. It does not claim
that every capability is implemented. Implementation and verification status
must come from the closure ledger and candidate-bound evidence.

## Decision vocabulary

| Decision | Meaning |
|---|---|
| `ADOPT` | Preserve the responsibility and boundary in Factory. |
| `ADAPT` | Preserve the useful behavior with a Factory-native design. |
| `EVALUATE` | Candidate extension with a stated comparison and admission gate. |
| `DEFER` | Preserve for a later milestone; a named trigger and new approval are required. |
| `REJECT` | Explicitly rejected mechanism; do not reintroduce it under another name. |

`APPROVED` means product scope is approved, not that protected authority,
isolation, spend or delivery rights are granted.

## Scope rule

Migration includes the behavior required by current main, the complete
functional intent of PR #16/F1-S4-D, approved replacement architecture and
security invariants. A capability outside PR #17 implementation scope remains in
this record and the roadmap. **Out of migration scope does not mean out of the
product roadmap, and it does not mean rejected.**

## Capability portfolio

| CAP | Capability | Decision | Milestone | Migration scope | Current Rust state | Rust destination |
|---|---|---|---|---|---|---|
| CAP-001 | Registered projects | ADAPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Fixture-verified registry; project configuration revision not complete | Core project registry, repository adapter, SQLite |
| CAP-002 | Action and frozen specification | ADAPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Fixture-verified immutable ActionSpec | Core intake, spec validation, SQLite |
| CAP-003 | Campaign orchestration | ADAPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Authorized fixture Campaign/Run/Attempt path | Core lifecycle and scheduler |
| CAP-004 | Implementer execution | ADOPT responsibility | F1-S4-C | REQUIRED_FOR_MIGRATION | Rootless-Podman fixture Implementer; provider coding blocked | Worker, Supervisor, direct provider adapters |
| CAP-005 | Candidate revision lineage | ADOPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Fixture-verified single-parent candidate identity | Core candidate verifier, Git adapter |
| CAP-006 | Independent verification and review | ADOPT boundary | F1-S4-C | REQUIRED_FOR_MIGRATION | Sandboxed verifier; reviewer is receipt-only | Verifier process, reviewer process, evidence contracts |
| CAP-007 | Deterministic Quality Gate | ADOPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Fixture-verified immutable gate | Core quality policy |
| CAP-008 | Scoped capability grants | ADAPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Partial project/capability scoping | Core authorization |
| CAP-009 | Factory-managed browser capability | ADAPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Not implemented in Rust | Worker browser wrapper and managed test server |
| CAP-010 | Bounded deterministic recovery | ADAPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Lease/retry primitives; no real corrective cycle | Core recovery policy |
| CAP-011 | Factory Office | ADOPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Rust-backed six-route Office; browser journey passes | React Office projection |
| CAP-012 | Durable events and telemetry | ADOPT | F1-S4-C | REQUIRED_FOR_MIGRATION | Durable event ledger; no cursor/SSE contract | Core events and telemetry |
| CAP-013 | Intelligent Lead | ADAPT | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Typed Lead scaffold; no model assessor | Lead application service and provider assessor |
| CAP-014 | Intent and semantic assessment | ADAPT | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Strict assessment schema; no assessor or CAP-014 evaluator | Assessment domain, evaluation tool |
| CAP-015 | Frozen executable planning | ADAPT | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Proposal/freeze scaffold; no spec/Campaign creation | Lead planning service |
| CAP-016 | Dependency-aware WorkGraph | ADAPT | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Bounded graph validation only | WorkGraph domain and scheduler |
| CAP-017 | Standing delegation contract | ADAPT | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Sealed grants and atomic reservation; no autonomous policy | Core authorization |
| CAP-018 | Agent Composer | ADAPT | F1-S4-E | FUTURE_FEATURE | Not implemented | Deterministic composition service |
| CAP-019 | Operation Supervisor | ADAPT | F1-S4-E | FUTURE_FEATURE | Core-owned deterministic coordination only | Core supervision policy; no second controller |
| CAP-020 | Specialist agent roles | ADAPT | F1-S4-E | FUTURE_FEATURE | Not implemented | Scoped role prompts through provider ports |
| CAP-021 | Independent semantic Reviewer | ADOPT boundary | F1-S4-E | REQUIRED_FOR_VERIFICATION | Receipt-only reviewer | Reviewer process and quality contract |
| CAP-022 | Parallel WorkUnits | ADAPT | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Not executed | WorkGraph scheduler |
| CAP-023 | Context and knowledge resolver | ADAPT | F1-S4-E | FUTURE_FEATURE | Not implemented | Bounded retrieval port; simple source reads first |
| CAP-024 | GitHub PR delivery | ADAPT | F1-S5 | FUTURE_FEATURE | Not implemented | Core delivery adapter |
| CAP-025 | CI and review reconciliation | ADAPT | F1-S5 | FUTURE_FEATURE | Not implemented | Delivery reconciliation |
| CAP-026 | Conditional R0 merge | ADAPT | F1-S5 | FUTURE_FEATURE | Not implemented | Protected merge policy |
| CAP-027 | Persistent maintenance objectives | ADAPT | F1-S5-M | FUTURE_FEATURE | Not implemented | Maintenance campaign service |
| CAP-028 | Project health monitoring | EVALUATE | F1-S5-M | FUTURE_FEATURE | Not implemented | Health signal port |
| CAP-029 | Incident diagnosis and Action creation | ADAPT | F1-S5-M | FUTURE_FEATURE | Not implemented | Incident application service |
| CAP-030 | Maintenance outcome measurement | EVALUATE | F1-S5-M | FUTURE_FEATURE | Not implemented | Evidence analytics |
| CAP-031 | Stable-controller self-development | ADAPT | F1-S6 | FUTURE_FEATURE | Not implemented | Protected update supervisor |
| CAP-032 | Promotion and rollback | ADAPT | F1-S6 | FUTURE_FEATURE | Not implemented | Versioned promotion policy |
| CAP-033 | Portfolio Manager | DEFER | F1-S7 | FUTURE_FEATURE | Not implemented | Portfolio service after resource governance |
| CAP-034 | Resource Governor | ADAPT | F1-S7 | FUTURE_FEATURE | Campaign attempt ceilings only | Core resource policy |
| CAP-035 | Durable maintenance scheduling | DEFER | F1-S7 | FUTURE_FEATURE | Not implemented | Scheduler extension |
| CAP-036 | Adaptive Model Router | ADAPT | Later | FUTURE_FEATURE | Explicit route only | Provider route policy |
| CAP-037 | Model evaluation and route promotion | EVALUATE | Later | FUTURE_FEATURE | Read-only provider smokes only | Evaluation and routing promotion |
| CAP-038 | Skill Registry and admission | EVALUATE | Later | FUTURE_FEATURE | Repository skills only; no runtime registry | Skill admission port |
| CAP-039 | Remote Workers | DEFER | Later | FUTURE_FEATURE | Local Worker only | Authenticated remote transport |
| CAP-040 | Extended Factory Office | DEFER | Later | FUTURE_FEATURE | Not implemented | Office projections after Core APIs |
| CAP-041 | Reliability analytics | ADAPT | Later | FUTURE_FEATURE | Event ledger only | Analytics over immutable evidence |
| CAP-042 | Optional context integrations | EVALUATE | Later | FUTURE_FEATURE | Not implemented | Narrow optional ports |
| CAP-043 | Autonomous product development | APPROVED objective | F1-S4-D | REQUIRED_FOR_PR16_PARITY | Fail-closed; no autonomous start | Core policy and Lead/Campaign start |
| CAP-044 | Autonomous product maintenance and evolution | APPROVED objective | F1-S5+ | FUTURE_FEATURE | Not implemented | Maintenance and self-development policy |

## Preserved engineering opportunities

These are capability inputs, not authority. They remain selected for evaluation
or adaptation even when implementation is deferred.

| Opportunity | Decision | Admission or comparison | Boundary |
|---|---|---|---|
| Engineering workflow and Lead skills | ADAPT | Owner request completes with fewer interventions | Guidance only; no authority |
| Implementation, validation and browser skills | ADAPT | Real candidate plus independent browser/commands oracle | Never expands sandbox or credentials |
| Independent audit and security review skills | ADOPT boundary | Findings are evidence; Core decides | No self-acceptance |
| Structured-output and specification-authoring skills | ADAPT | Closed schema plus semantic validation | Model output remains advisory |
| GitHub, worktree and recovery skills | ADAPT | Exact candidate/head and durable recovery evidence | Protected delivery remains separate |
| Knowledge retrieval and repository navigation | EVALUATE | Measured quality and token cost versus simple reads | Source-bound and bounded |
| Serena | EVALUATE | Compare with simple source search on real tasks | Optional port; no default dependency |
| Graphify | EVALUATE | Impact-analysis quality with source confirmation | Optional port |
| Engram | EVALUATE | Repeated-task utility and data governance | Optional port |
| Headroom | EVALUATE | Quality stable while measured context cost falls | Optional port |
| Controlled MCP and external tools | ADAPT narrow / REJECT platform | Explicit scoped capability and permission mediation | No generic plugin runtime |
| Skill admission and scoped promotion | EVALUATE | Digest, provenance, license, capabilities and risk review | Global promotion is protected |
| Model routing, benchmarking and adaptive promotion | EVALUATE | Representative corpus, cost and quality; revocable prospective promotion | No silent substitution |
| Reliability and quality evaluation | ADAPT | Metrics reconcile to immutable Campaign evidence | No fabricated telemetry |

## Rejected mechanisms

- A second generic controller or orchestrator competing with Core authority.
- A permanently running roster of unnecessary specialist agents.
- Model output granting scope, budget, risk, tools, delivery or acceptance.
- A generic plugin/tool platform admitted without a concrete capability.
- Compatibility bridges or aliases that preserve an obsolete runtime as an
  active dependency.
- Treating deterministic integration, a fixture, a probe or documentation as
  live-agent certification.
- A side budget ledger that competes with Core reservations.
- Mounting a container-runtime socket, host HOME or provider credentials into
  untrusted execution.

## Preservation gate

The replacement record must keep every selected CAP identifier and sub-scope,
including meanings that previously collided:

- CAP-002 covers Action/frozen specification; wiki/Kanban remains a separate
  `EVALUATE` opportunity.
- CAP-004 covers Implementer execution; the Implementer role cannot accept its
  own work.
- CAP-024 covers GitHub PR delivery; local delivery is a separate `DEFER`
  opportunity.
- CAP-027 covers maintenance objectives; portfolio work belongs to CAP-033/035
  until explicitly merged.
- CAP-039 covers Remote Workers; MCP/external tools are handled by the separate
  `ADAPT narrow` opportunity.

No capability is removed because it is outside PR #17 implementation scope.
No `EVALUATE` or `DEFER` decision is converted to `REJECT` without an explicit
Product Owner decision.

## Relationship to other documents

- [Capability registry](capabilities.yaml) is the machine registry and
  implementation/evidence projection.
- [Roadmap](../roadmap/ROADMAP.md) is generated from the registry.
- [Migration closure](../implementation/native-rust-closure.md) is the current
  migration status, gap and evidence ledger.
- Historical Git commits and evidence remain provenance and are not rewritten.
