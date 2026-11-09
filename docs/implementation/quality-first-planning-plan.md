# Quality-first planning and stack-selection implementation plan

**Status:** Proposed — 2026-11-09. Design authority:
[ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md).
No behavior is claimed until the phases below have candidate-bound evidence.

## 1. Why this plan exists

The 2026-11-09 gym-manager run proved the Factory can produce an accepted
candidate from a high-level objective, but the plan chose a narrow CLI slice
without a formal stack/architecture/documentation decision. The accepted
candidate is recorded in
[the gym evidence](../evidence/gym-manager-2026-11-09/README.md).

The owner's requirement is broader: when the user has not chosen the
technology, the Factory must plan the product properly, choose the stack from
the actual need and available tools, prefer maintained libraries, define
architecture and conventions, create missing `AGENTS.md`/architecture docs,
select necessary skills, define a real Definition of Done, and continue
through bounded replanning rather than stopping or taking the easiest output.

This is not a prompt-only change. The current routing, contracts, data model,
execution sandbox and Office all participate.

## 2. Current behavior and evidence

| Area | Current behavior | Effect |
|---|---|---|
| Home planning | Uses the Lead proposal path; `native/src/lead_model.rs` forces `workUnits` to contain exactly one implementation unit and asks for the smallest subset of paths/capabilities. | A large objective becomes one shallow unit. |
| Rich planning | Planner/Spec Manager exist in `native/src/bin/core.rs` and `native/src/agentic.rs`, but they are Director actions, not the default request lifecycle. | The rich path is optional and usually unused. |
| Route floors | `agentic_triage_context` hardcodes `architecture_uncertainty`, `public_contract`, `data_or_schema` and `security_boundary` to `false`. A greenfield request with a one-unit proposal routes `DIRECT`. | Greenfield/large work does not become `FORMAL_SPEC`. |
| Stack | `StackProfile` is detected from manifests only (`native/src/agentic.rs`). Planner prompt says "Never invent a file, dependency or technology." | The model cannot choose or justify a stack; std-only is the path of least resistance. |
| Dependencies | Execution runs offline with `--network=none`; only the image's toolchain is available. No dependency acquisition/vendor step. | A planner cannot honestly select a crate/framework that is not already vendored. |
| Quality | `QualityProfile` is a fixed enum (`NodeBasic`, `NodeBrowser`, `Generic`). `Generic` requires only build/test/functional/negative. | No mandatory format, lint, typecheck, docs, ADR, security or traceability checks. |
| Review | Reviewer gets objective, checks, diff and source; not the frozen spec, stack, architecture, docs plan or DoD. Warnings can be accepted silently. | The reviewer cannot validate fidelity to the plan; quality debt is invisible. |
| Continuation | No-progress breaker halts the implementer; no automatic replan; plan is prompt context, not a durable artifact. | Work stops halfway instead of diagnosing/replanning. |
| Documentation | Target repositories are not required to have `AGENTS.md`, architecture docs or ADRs. | The next agent starts without conventions or decision history. |

## 3. Target planning lanes

| Lane | When | Required artifacts | Implementation shape |
|---|---|---|---|
| `DIRECT` | Small bounded change; existing stack and docs; ≤5 files; no architecture uncertainty | Objective, scope, checks, DoD | 1 unit; fast path; Lead plan remains valid |
| `DELEGATED` | 2–8 units; existing stack/docs; cross-module but no public contract | Spec-lite, WorkGraph, stack confirmation, docs update, DoD | 2–8 vertical units |
| `FORMAL_SPEC` | Greenfield, large, ambiguous, missing architecture docs, no stack choice, public contract, data/schema, security boundary, architecture uncertainty, >5 files or >1 expected unit | Constitution, formal spec, stack decision, architecture plan, ADRs, WorkGraph, docs plan, skills plan, DoD, traceability | 3–24 vertical units + docs/quality units |
| `REPAIR` | Exact rejected candidate | RepairPacket + bound findings | C+1 from exact candidate |

`FORMAL_SPEC` is a deterministic floor. A model may raise a lane but never
lower a floor. A `DIRECT` override of a `FORMAL_SPEC` floor requires an explicit
owner decision with rationale.

## 4. Target artifacts

### 4.1 Project constitution (`AGENTS.md`)

A concise project-level contract, not a duplicate README:

- build, format, lint, typecheck, test, functional, negative, browser/security
  commands that actually work;
- repository structure and module boundaries;
- language/framework conventions and error/validation rules;
- security, secrets, data and migration rules;
- links to architecture, ADRs, product docs and plans;
- what the project explicitly does not do.

Target: under ~150 lines. It is generated only when absent or materially
incomplete; existing docs are cited and preserved. Discovery cites the files it
read.

### 4.2 Stack decision

```json
{
  "schemaVersion": 1,
  "source": "USER | EXISTING | MODEL",
  "constraints": ["must run in the container toolchain", "no host-only service"],
  "options": [
    {
      "id": "rust-std-cli",
      "summary": "std-only Rust CLI with JSON persistence",
      "fit": 3,
      "security": 5,
      "maintainability": 5,
      "operability": 5,
      "timeToValue": 5,
      "exitCost": 2,
      "pros": ["no dependency supply chain", "fits offline checks"],
      "cons": ["limited web/API scope"],
      "evidenceRefs": ["Cargo.toml", "AGENTS.md"]
    }
  ],
  "selected": "rust-std-cli",
  "reason": "The objective is a bounded local CLI; std-only is the smallest stack that meets the quality attributes and available capabilities.",
  "confidence": "high",
  "requiredCapabilities": ["generic-toolchain"],
  "dependencyPlan": {"status": "not-required", "items": []},
  "adrPath": "docs/architecture/decisions/ADR-0001-stack-rust-std-cli.md"
}
```

Precedence: **user constraints > existing project stack/conventions > model
proposal**. A model may not silently replace a user-selected or existing stack.
Every selection records rejected options and why. For `FORMAL_SPEC`, at least
two realistic options are normally required; a single option is allowed only
with a recorded reason (user constraint, existing stack, or no viable
alternative).

### 4.3 Architecture plan

Records context, components, responsibilities, boundaries, interfaces, data
ownership, deployment topology, quality attributes, risks, sensitivity points,
tradeoffs and ADR references. It must comply with
[service placement](../architecture/service-placement-policy.md) and the
[container-native policy](../engineering/container-native-engineering.md).
A container/Pod is never described as certified isolation.

### 4.4 Formal spec

Observable behavior, user journeys, interfaces, constraints, non-goals,
assumptions, acceptance cases and negative cases. Requirements are numbered,
traceable and testable; implementation instructions are rejected. The current
Spec Manager schema is the starting point; it needs non-goals, quality
attributes, interfaces and docs requirements.

### 4.5 WorkGraph

Vertical slices, 1–24 units. Each unit has:

- one responsibility and a one-sentence objective;
- dependencies and non-overlapping write paths when parallel;
- 2–6 observable acceptance checks;
- the exact commands/evidence expected;
- docs impact (which docs are created/updated);
- a handoff/milestone and remaining work.

For `FORMAL_SPEC`: at least 3 units, at least one foundation/docs/architecture
unit and one independent verification/quality unit. A single-unit plan for a
large/greenfield objective is rejected deterministically.

### 4.6 Definition of Done

Project DoD:

- build, format, lint, typecheck (where the stack provides them), test,
  functional and negative checks;
- security/dependency checks where applicable;
- required `AGENTS.md`, architecture overview, ADR and spec updates;
- requirement-to-unit-to-check-to-evidence traceability;
- no critical/blocking findings; warnings handled by policy;
- no unowned build warnings or temporary scaffolding.

Unit DoD: its checks pass, its docs impact is applied, its handoff is recorded.
The Worker cannot finalize a unit without its DoD; the Gate cannot accept a
candidate without the project DoD evidence.

### 4.7 Documentation and skill plans

`DocumentationPlan` lists existing docs, required new docs, owners and
acceptance checks. `SkillPlan` lists skill/expertise needs, existing skills,
proposed temporary skills, license status, scope and adoption decision. A skill
never grants tools, authority or scope.

## 5. Route and plan-depth rules

1. Greenfield (no manifest, no docs, no usable stack) → `FORMAL_SPEC`.
2. Large/ambiguous objective, architecture uncertainty, public contract,
   data/schema change, security boundary, cross-module work, >5 files or >1
   expected unit → `FORMAL_SPEC`.
3. 2–8 units with an existing stack/docs and no public contract → `DELEGATED`.
4. Small bounded change with existing stack/docs → `DIRECT`.
5. `FORMAL_SPEC` with fewer than 3 units or no docs/quality unit → reject and
   replan.
6. One-unit plans may be `DIRECT` only; they may not be used to bypass depth
   for a large objective.
7. Every plan records non-goals and out-of-scope items.

## 6. Stack and dependency selection method

Use a lightweight ATAM-style utility tree rather than an arbitrary score:

1. Elicit 5–10 architecturally significant requirements (latency, scale,
   security, availability, maintainability, operability, cost, time to value,
   compliance, deployment constraints).
2. Inventory existing stack/docs and available Factory capabilities.
3. Propose 2–3 candidate stacks; each must state fit to each requirement and
   the main tradeoff.
4. Reject candidates that need unavailable capabilities, unsupported
   dependencies, host-only runtime, or non-compliant licenses.
5. Select the smallest coherent stack that satisfies the high-priority
   attributes; record the ADR and confidence.
6. Define the dependency plan and the acquisition path.

### Dependency acquisition

The current check sandbox is offline. The plan adds one of:

- **Preferred:** a digest-pinned vendor/cache image per stack, populated by the
  owner or a trusted build step; checks run offline against the cache, with an
  SBOM and license/security evidence.
- **When needed:** a brokered dependency-acquisition step in a separate,
  network-enabled, credential-free process/container with an allowlist,
  lockfile, checksums, license and vulnerability checks. The result is vendored
  into the workspace; subsequent checks remain offline.

If neither is available, the Planner must declare the dependency constraint and
choose a stack that fits, or raise an owner decision. It must not pretend a
crate/framework is available.

## 7. Quality Gate and warning policy

- Required checks are derived from the frozen stack + assurance + project
  profile, not a single fixed enum. Keep existing profiles as presets.
- Add `Typecheck`, `Security` and `Documentation` check kinds (format/lint
  already exist). A documentation check can run deterministic commands such as
  `test -f AGENTS.md`, `test -f docs/architecture/overview.md`, and `grep` for
  required sections.
- Reviewer bundle includes the frozen spec, stack decision, architecture,
  documentation plan, DoD and traceability. The reviewer must flag spec drift,
  architecture deviation, missing docs and unowned warnings.
- `critical` and `blocking` always stop acceptance. For `FORMAL_SPEC` and
  `ELEVATED`/`CRITICAL`, warnings block unless an authenticated owner accepts
  the exact warning with rationale, follow-up issue and expiry.
- Accepted warnings are recorded as debt and projected to the issue/kanban.
  Silent warning acceptance is forbidden.
- Gate evidence is candidate-bound and immutable.

## 8. Continuation and context

- Store the frozen plan/spec/stack/architecture/DoD in Core and expose a
  bounded, read-only plan artifact to every implementer unit.
- Handoff summaries reference durable artifact identities, completed units,
  checks, remaining work and risks.
- On stagnation, run a bounded diagnosis and create a new WorkGraph revision
  (same spec/stack) within the frozen budget. Do not stop the campaign on the
  first stagnation. Escalate to owner only when replans are exhausted or a
  reserved matter is touched.
- Commit a milestone per completed unit (or per coherent slice) so progress
  survives context loss and a new session can resume from the plan.

## 9. Office experience

- Home: optional planning constraints (language/framework/deploy target,
  dependencies, non-goals) and an Auto/Formal planning choice. Auto is default;
  the deterministic floor wins.
- A Plan Review surface for `FORMAL_SPEC` (and policy-required cases): tabs for
  Spec, Stack, Architecture, WorkGraph, Docs, Skills, Checks/DoD and Risks.
  Approve/amend/reject; Core freezes the exact revision.
- Director: show stack decision, ADRs, architecture, docs coverage, skill plan
  and traceability.
- Request workspace: a truthful `Planning` / `Awaiting plan review` stage, not
  just `Executing`; show accepted warning debt.
- Issues/kanban: spec publication remains a projection; accepted debt creates a
  visible follow-up. No decorative actions.

## 10. Data, API and migration

New immutable, revisioned artifacts bound to the request/plan/spec:

- `project_constitutions`
- `stack_decisions`
- `architecture_plans`
- `documentation_plans`
- `skill_plans`
- `definition_of_done`
- `plan_reviews` / `accepted_warnings`
- `dependency_plans` (and acquisition evidence)

Add Core endpoints for discovery, stack, architecture, docs, skills, plan
review and convergence. All are additive; old `LeadPlanProposal`/`FrozenSpec`
records remain readable. `FORMAL_SPEC` requires the new artifacts; `DIRECT`
continues to work with the old shape.

## 11. Phases

| Phase | Scope | Main changes | Acceptance |
|---|---|---|---|
| 0 | Decision and contracts | ADR-0039, update agentic-flow contract, roadmap/capability registry | Documents reviewed; no behavior claim |
| 1 | Typed artifacts and validators | Structs/schemas for constitution, stack, architecture, docs, skills, DoD, dependency plan; deterministic validation; DB migration | Schema/validator tests; invalid artifacts rejected; hashes/revisions immutable |
| 2 | Route and prompt integration | Real triage signals; `FORMAL_SPEC` floor; Home uses Planner/Spec Manager for `DELEGATED`/`FORMAL_SPEC`; depth policy; prompt rewrite | Greenfield/large fixture never collapses to one unit; route tests; prompt/model fixtures |
| 3 | Stack, docs and dependencies | Stack decision/ADR, docs plan, skill plan, dependency cache/acquisition capability | Stack precedence tests; capability-fit tests; docs generated only when missing; dependency blocker is explicit |
| 4 | Execution and convergence | Plan artifact in workspace; per-unit DoD; docs updates; bounded replan; milestone commits; convergence | Multi-unit campaign reaches accepted; no partial final; docs/spec match code; replan fixture |
| 5 | Quality and Office | Reviewer bundle/traceability; warning policy; Gate debt; Plan Review UI; Director/docs views | Warning-only candidate is blocked or explicitly accepted; per-option browser acceptance; responsive/a11y |
| 6 | Evals and live pilot | Golden fixtures, real-model E2E, metrics, owner review | 3 live scenarios; first-pass acceptance and doc/traceability evidence recorded; remaining gaps explicit |

Suggested sequencing: 0 → 1 → 2 → 3 → 4 → 5 → 6. Phase 3's dependency
acquisition can run in parallel with Phase 2 after the stack contract freezes.

## 12. Test matrix

| Scenario | Expected route | Required evidence | Negative assertion |
|---|---|---|---|
| Greenfield Rust CLI, no docs | `FORMAL_SPEC` | Constitution, 2–3 stack options, ADR, 3+ units, docs/quality units, DoD | Cannot accept a one-unit plan or missing `AGENTS.md` |
| Greenfield HTTP service needing a framework | `FORMAL_SPEC` | Stack decision, dependency plan, architecture, interfaces, security/health checks | Cannot silently choose an unavailable dependency |
| Existing Rust service, small change | `DIRECT` | Existing stack/conventions, checks, docs impact | Cannot rewrite the stack without ADR + owner |
| Existing Node/TypeScript UI | `DELEGATED`/`FORMAL_SPEC` | Typecheck/lint/test/browser, docs | Cannot skip browser acceptance for UI change |
| User specifies stack | selected `source=USER` | Exact constraints, checks, docs | Model cannot replace it |
| Missing docs in an existing repo | `DELEGATED`/`FORMAL_SPEC` | Docs unit, constitution, architecture overview | Cannot accept with missing required docs |
| Large objective with one-unit proposal | `FORMAL_SPEC` | Depth rejection, replan, 3+ units | Cannot lower the floor |
| Reviewer warning only | per assurance | Explicit owner acceptance + debt issue or block | Cannot silently accept |
| Stagnant unit | bounded replan | New graph revision, same spec/stack | Cannot stop on first stagnation |
| Dependency unavailable | stack alternative or owner decision | Blocker/decision record | Cannot pretend it is installed |

## 13. Metrics

Telemetry, not productivity caps:

- plan completeness rubric (goals/non-goals, stack options, architecture, docs,
  DoD, checks, risks);
- first-pass Gate acceptance rate;
- repair cycles per accepted candidate;
- documentation coverage and spec-to-code traceability;
- owner amendments and accepted-warning debt;
- dependency/security blockers;
- time to first accepted candidate (observability only).

## 14. Risks, non-goals and open decisions

**Risks:** over-planning small changes (mitigate with lanes); model output
unreliable (deterministic validation + owner review); dependency supply chain
(allowlist, SBOM, license/security review); docs drift (convergence + reviewer);
more model calls/cost (right-size; telemetry only); plan review becoming a
bottleneck (R0 auto-approval under charter, R1+ owner).

**Non-goals:** no second orchestrator; no model authority; no automatic public
contract/security/production changes; no stack rewrite without an ADR and
owner decision; no container-as-isolation claim.

**Open owner decisions:**

1. Should warnings block every `FORMAL_SPEC`, or only `ELEVATED`/`CRITICAL`?
2. Stack changes in an existing repo: owner approval for R0/R1, or only R1+?
3. Dependency acquisition: vendor/cache image first, brokered fetch, or both?
4. Is a plan-review checkpoint required for autonomous R0 `FORMAL_SPEC`, or can
   the charter auto-approve after deterministic validation?
5. Minimum documentation by project size (CLI vs service vs UI)?
6. Keep `LeadPlan` as the `DIRECT` lane or migrate all change requests to the
   WorkGraph and keep Lead as a thin assessor?
7. Should quality profiles be derived per stack or remain explicit presets that
   the stack decision selects?

## References

- GitHub Spec Kit — Specify → Plan → Tasks → Implement → Converge:
  https://github.github.com/spec-kit/
- Microsoft, Spec-Driven Development:
  https://developer.microsoft.com/blog/spec-driven-development-ai-native-engineering/
- GitHub Blog, spec-driven development:
  https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/
- Martin Fowler, Architecture Decision Record:
  https://www.martinfowler.com/bliki/ArchitectureDecisionRecord.html
- Microsoft Azure Well-Architected, ADR:
  https://learn.microsoft.com/en-us/azure/well-architected/architect-role/architecture-decision-record
- AWS, ADR best practices:
  https://aws.amazon.com/blogs/architecture/master-architecture-decision-records-adrs-best-practices-for-effective-decision-making/
- SEI, Architecture Tradeoff Analysis Method:
  https://www.sei.cmu.edu/library/architecture-tradeoff-analysis-method-collection/
- Martin Fowler, Context Engineering for Coding Agents:
  https://martinfowler.com/articles/exploring-gen-ai/context-engineering-coding-agents.html
- Red Hat, AGENTS.md and Agent Skills:
  https://developers.redhat.com/articles/2026/07/27/standardize-project-context-agentsmd-and-agent-skills
- Softwarefabrik, plan before you build:
  https://softwarefabrik.io/en/academy/kapitel-05-planen.html
- Deep Work Plan, the plan as source of truth:
  https://www.dailybot.com/blog/how-we-run-long-horizon-agent-work/
- Specifying done:
  https://dsplce.co/agentic-engineering/core/specifying-done
