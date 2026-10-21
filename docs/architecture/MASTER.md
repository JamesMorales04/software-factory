# Software Factory — Master Architecture Specification

> **Status:** F1-S1 through F1-S4-B merged. F1-S4-C multi-project registry, scoped capabilities and Factory-managed browser capability are implemented and deterministically verified (2026-10-21); real-Paseo live certification is not yet achieved and no component is certified beyond its recorded candidate-bound evidence. Architecture baseline revision 0.5. **Permanent repository:** `JamesMorales04/software-factory`. **Software status:** working components are exercised by real acceptance suites, but no component is certified beyond its recorded candidate-bound evidence. The document distinguishes agreed product policy from proposed technical solutions and unverified feasibility.

## 1. Product mission and priorities

Build an autonomous, local-first, multi-project engineering system that converts requests and observed incidents into Actions, assembles specialized agents and trusted knowledge as needed, performs isolated real coding work, obtains independent quality evidence, and delivers through governed GitHub workflows. The Factory must eventually improve its own code using an **independent stable controller**.

Order of priorities: (1) correct authority, isolation and integrity; (2) objective acceptance and delivery quality; (3) recovery, durability and observable continuity; (4) low total time, tokens and resources per **accepted** change; (5) an operational Office UI, later enhanced with visual agents. Number of agent turns, generated commits or PRs is not success.

Not in F1: a custom agent-coding engine replacing Paseo; distributed HA Core; production remote workers; animated pixel office; autonomous Core binary promotion; speculative plugins/services; required third-party telemetry SaaS.

## 2. Normative status and design authority

- **Agreed:** authorized by the product owner; not altered without a new decision and corresponding ADR where applicable.
- **Proposed:** plausible technical direction requiring explicit ratification or a feasibility test.
- **Unverified:** assertion or behavior requiring end-to-end evidence.
- **Implemented:** code exists, but integration may still be untested.
- **Verified:** demonstrably passes relevant real acceptance tests, with identity-bound artifacts.

Normative documents describe policy; **enforced Core policy** controls actions. A model, skill, UI component or project file cannot grant higher authority. Contradictions between docs, code and policy create a blocking Action to reconcile, not a silent lowest-common-denominator fallback.

## 3. Agreed design decisions

| Area | Decision | Status |
|---|---|---|
| Identity | Permanent public repo `JamesMorales04/software-factory` | Agreed |
| Product | Central, independent, local-first and multi-project | Agreed |
| Deployment | Local Worker first, expandable to owned/cloud-managed remote Workers | Agreed |
| Coding runtime | Paseo first; Factory owns its API/UI and policies | Agreed |
| UI | Real minimum Factory Office web interface required in F1 | Agreed |
| Execution | Per-Run sandbox default; authorized, auditable host exceptions only | Agreed |
| Autonomy | Standing autonomous monitoring and maintenance within bounds | Agreed |
| Delivery | Conditional autonomous merge for verified low-risk R0 changes | Agreed |
| Skills | Autonomous external discovery, evaluation and script adoption with technical controls | Agreed |
| Registry | Autonomous global skill promotion after independent safety/compatibility/utility evaluation | Agreed |
| Agents | Dynamic specialization and minimum sufficient composition | Agreed |
| Models | Adaptive routing; low-risk promotion only after reproducible evaluation | Agreed |
| Resources | Development/Maintenance/Evaluation reserves and reclaimable loans | Agreed |
| Quality | Mandatory global Quality Baseline with additive project profiles | Agreed |
| Projects | Registered local Projects bind canonical repository identity, base policy, profile, scope and capabilities | Agreed; ADR-0012 |
| Capabilities | Core-governed per-Project/Project-spec capability grants matched to Worker runtime | Agreed; ADR-0012 |
| Verification | Independent real-user verifier separate from the author | Agreed |
| Compatibility | No speculative backward compatibility for experimental internal contracts | Agreed; ADR-0006 |
| Engineering | Clean architecture principles, pragmatic patterns, DRY and low complexity | Agreed; ADR-0007 |
| Lint | Format/lint/static-analysis gates for every active implementation language | Agreed; ADR-0008 |
| Self-development | Stable controller A supervises isolated candidate B | Agreed safety boundary |

### Proposed technology choices (not yet ratified)

- **Core + local Worker:** **Go, ratified** for the modular Core and separate Worker process ([ADR-0005](decisions/ADR-0005-go-core-worker-language.md)). TypeScript remains the Paseo SDK adapter language. This selection does not certify provider credential isolation (SEC-01).
- **Persistence:** Core-owned SQLite WAL with one logical writer initially; Postgres only when a real scale constraint warrants it.
- **Paseo:** a TypeScript adapter around the actual supported `@getpaseo/client`/CLI behavior; keep SDK types out of Core domain contracts.
- **Web UI:** React and TypeScript over authenticated HTTP + event stream (SSE/WebSocket to be empirically selected).
- **Sandbox:** rootless OCI/Podman, with no unauthorized host execution fallback; alternative confinement if proven appropriate.
- **Observability:** durable event ledger, structured redacted logs, OpenTelemetry for traces/metrics, local-first export and optional OTLP.

None of these proposed technologies overrides F0 security or acceptance blockers.

## 4. System responsibilities and boundaries

```text
      Factory Office web / CLI
                 |
        Factory Core (modular)
        - Project, Action, Campaign
        - state, policy, identity, audit
        - Scheduler, Resource Governor
        - Agent Composer, Model Router
        - Knowledge Resolver, Skill Registry
        - Quality, Evidence, Delivery
        - Incidents, Maintenance, Events
                 |
      authenticated Core–Worker contract
                 |
      local Worker (remote later)
                 |
      per-Run sandbox / repository snapshot
                 |
        Paseo coding agent sessions
```

The Core owns canonical state, policies, authorization, budgets and external effects. Workers execute **identified, leased and fenced** assignments and return observations; they do not self-authorize merges or change risk policy. Paseo owns runtime sessions, not Action state. The UI reads Core projections and issues authorized intents; it never writes SQLite directly or controls Workers out of band. Scheduler and Resource Governor are deterministic services, not permanently running LLM agents.

These names are cohesive logical modules **inside one Core**, not an instruction to create many processes, interfaces or microservices. Respect [engineering standards](../engineering/code-quality.md): high cohesion, narrow boundaries, no duplicate policy engines, no ceremonial Clean Architecture scaffolding.

## 5. Persistent entities and workflow

- **Project:** repository identity, authorized roots and Quality Profile.
- **Incident:** deduplicated issue/event with source evidence.
- **Action:** engineering goal, risk, owner, state and acceptance criteria.
- **ActionSpec:** versioned normative acceptance; frozen revision bound to work.
- **Campaign:** governed set of Actions/dependencies within budget.
- **Task:** assignable engineering work unit.
- **Run / Attempt:** exact configuration, source revision, sandbox, lease, activity and result.
- **AgentSession:** role, provider, model, capabilities, Paseo ID and Worker association.
- **CandidateRevision:** immutable exact content digest/commit to validate.
- **EvidenceBundle:** candidate-bound checks, logs, independent verdicts and provenance.
- **Delivery:** PR/merge intents, authoritative confirmations, idempotency keys and receipts.
- **PolicyRevision / SkillVersion / RouteRevision:** immutable references enabling audit and replay.

Main path: **intake → deduplicate/triage → risk-appropriate spec → minimal agent plan → authorization/budget → isolated Run → candidate → tests and independent review → deterministic Acceptance Gate → governed delivery → completion**.

A completed Run is *not* an accepted Action; an accepted Action is *not* a merged PR; a merged PR is *not* an updated live Core.

## 6. Agent composition, semantic assessments and model routing

Available responsibilities: Director, Planner/Architect, Explorer, Librarian, Spec Manager, Implementer, Reviewer, Repairer and Incident Analyst. Roles are activated as required; they are not nine always-running agents. Agent Composer selects the minimum viable set of roles, concrete skills, tools, provider routes, independent review constraints and per-participant budgets.

Semantic Assessment handles intent, complexity, stack, knowledge gap, failure diagnosis, change impact, validation need and spec ambiguity only when deterministic logic is insufficient. Every model judgment is scoped, evidence-bound, typed, budgeted, validated, uncertainty-aware and **advisory**. A policy check or test result must never depend on an LLM's unsupported claim.

Model Router uses `ECONOMY`, `BALANCED`, `REASONING` and `CRITICAL_REVIEW` logical profiles. Eligibility (runtime, schemas, capabilities, safety, independence and quota) precedes quality/latency/resource optimization. Certify exact provider–model–runtime combinations through real probes. All fallback decisions are explicit and traceable; unknown availability cannot authorize degradation. Promotion compares reproducible Champion/Challenger evaluations and affects only future eligible assignments; freeze selected routes per Run.

## 7. Knowledge and skills

Knowledge Resolver distinguishes sufficient, retrievable, missing, stale, conflicting and unverified knowledge. A missing skill is not automatically missing competence. Invoke Librarian only when an actual knowledge gap exists.

Skill Registry stores identity, content digest, provenance, version, license, dependencies, stack compatibility, evaluation evidence, requested capabilities, risk and `GLOBAL`/`PROJECT`/`RUN` scope. External SKILL.md instructions and scripts are untrusted; admission evaluates them in isolation with no Core secrets and no privilege expansion. An admitted skill remains confined to authorized Run capabilities. Autonomous global promotion requires an independent safety, compatibility and utility decision with revocation capability; agents cannot approve changes to their own trust gates.

## 8. Execution security and isolation

A per-Run isolated checkout bound to a commit is the recommended unit of coding execution; Git worktrees alone are not OS isolation. Do not expose host HOME, secrets, provider tokens, Core DB, privileged sockets, merge credentials or other projects to arbitrary repository commands. Reviewers inspect read-only exact candidates and independent oracles.

Core–Worker authentication, leases, fencing, heartbeat and reconciliation must prevent stale or repeated external effects. Remote Workers later use authenticated API boundaries rather than direct SQLite access.

**Multi-project scoping (F1-S4-C):** a registered Project binds one canonical
local Git repository identity, authorized base-ref tips, workspace scope,
Quality Profile and a closed capability grant. ActionSpecs snapshot the exact
Project configuration revision and binding hash; Runs carry the capability
snapshot; Workers advertise observed capabilities; dispatch rejects paused,
mismatched or unsupported work. Browser automation is a Factory-managed
capability tied to the Run, not a prompt claim. This is **control-plane
authorization, not OS isolation**: TRUSTED_LOCAL retains broad host access and
cross-project confidentiality is never claimed. See [ADR-0012](decisions/ADR-0012-project-registry-and-capabilities.md)
and the [S4-C contract](../contracts/multi-project-s4c.md).

**SEC-01 disposition is mode-specific (see [ADR-0009](decisions/ADR-0009-trusted-local-execution.md)):** a host Paseo agent actually read a synthetic canary outside its workspace. Rootless Podman protected tested host paths and a brokered SDK → Codex → MCP → Worker execution worked, but mandatory isolation of all tools from provider credentials is **not certified**. Historical FAIL/NOT TESTED findings remain valid observations and are never rewritten to PASS. SEC-01-STRICT is **not required for TRUSTED_LOCAL** (owner risk acceptance); SEC-01-ISOLATED is available-only-where-verified. Product owner paused further spike experiments and authorized S1, credential-free S2 prototypes, and S3-TRUSTED_LOCAL under its entry conditions; S3-ISOLATED, credential-bearing untrusted execution under an isolation claim, and privileged autonomous effects remain blocked. Never fall back to a privileged host process. Never call TRUSTED_LOCAL isolated or certified. See [security](../security/isolation.md), [trusted-local policy](../security/trusted-local.md), [spike plan](../security/paseo-sandbox-spike.md) and [observed results](../security/SEC-01-results-2026-10-18.md).

## 9. Global quality, design quality and delivery

The non-negotiable Quality Baseline requires exact spec/candidate/policy identity, authorized workspace and scope, functional acceptance, relevant regression, security, independent verification, provenance and exact correspondence between checked and delivered SHA. Project Quality Profiles **add**, never weaken, checks. `SKIPPED`, `NOT_TESTED` and `NOT_APPLICABLE` are not `PASS`.

Code must follow [maintainability standards](../engineering/code-quality.md): clear boundaries, simple modules, pragmatic design patterns, explicit failure modes, minimal coupling and single sources of truth for duplicated knowledge/rules. Avoid speculative abstraction and backward-compatibility shims while internal contracts remain experimental. [ADR-0006](decisions/ADR-0006-early-development-compatibility.md) defines the exceptions for stable external contracts, persistent evidence and real consumer data. Apply [lint/static checks](../engineering/linting.md) for the chosen stack, and run them as mandatory candidate-bound CI gates.

Risk classes: R0 low, R1 moderate, R2 high (security/schema/public contracts), R3 critical (Core authorization, credential/sandbox/policy/upgrade). Reassess risk from the **actual diff**. Autonomous merge is restricted to eligible R0 changes with required CI, independent review and GitHub protection verified against exact candidate SHA immediately before the merge effect. Agents cannot acquire merge credentials. Live Core promotion is a separate protected operation.

## 10. Self-development and persistent maintenance

Treat the Factory itself as a managed project. **Stable Core A** supervises **isolated candidate B**; B cannot change the currently operating acceptance rules, credentials or budgets. PR/merge of B does not install B. Promotion and rollback belong to a separate guarded Update Supervisor to be delivered later.

Deterministic incident monitoring stays active without always-running expensive LLM sessions; events deduplicate into Incidents and authorized Actions. Scoped repair Runs obey existing risk, progress and budget ceilings. High-risk incidents affecting Core integrity or security trigger containment, not ungoverned self-repair using ordinary R0 authority.

## 11. Resource governance

Reserve capacity across **Development**, **Maintenance** and **Evaluation**. Idle capacity may be loaned and reclaimed at safe boundaries without wasteful interruption; critical incidents take appropriately higher priority. Apply project fairness and hierarchical Factory → Project → Campaign → Action → Run budgets, plus a maintenance reserve. Track actual and estimated tokens/turns, time, concurrency, CPU/RAM/storage, retries and monetary spend where observable. Unknown provider quotas and cost remain unknown, never zero.

An authorized reallocation within a ceiling is not new authority; exceeding the ceiling or spawning child campaigns to bypass it is forbidden.

## 12. Evals and adaptive learning

Use frozen code bases, task specs, fixtures and independent acceptance oracles to compare models, skills and team compositions. Record exact route, runtime/provider/model, skill versions, prompt/config identity, latency, resources, first-pass success, repair count, final acceptance and uncertainty. Enforce safety/quality thresholds **before** efficiency optimization. Champion/Challenger promotion is limited to approved low-risk routing classes, never from a single favorable sample or self-edited evaluation harness. Evaluation runs consume their own governed budgets.

## 13. Factory Office

F1 requires Dashboard, Actions, Agents and Action Detail. Use authenticated Core snapshots and durable event replay for real timeline, sessions, acceptance, PR and blocking reasons. Reconnected clients catch up without invented history or duplicate effects. Display unknown/disconnected status explicitly; the UI cannot reveal private reasoning. A future animated office uses the same records and never creates a parallel authority.

## 14. Telemetry from inception

Keep three systems distinct: (1) transactionally durable domain events that are never sampled, (2) operational OpenTelemetry traces and metrics, and (3) structured, redacted logs and evidence artifacts. Core must operate while an exporter is offline. Propagate correlatable IDs across Core/Worker/Paseo adapter/tests/delivery; no raw secret, source bodies or unredacted prompt content in telemetry. See [telemetry specification](../observability/telemetry.md).

## 15. F0 and F1 gates

**F0 (documentation + feasibility):** finish six contracts (Project–Action, Campaign–Run, Core–Worker, Agent Session, Quality–Delivery, Factory Events), apply the ratified Go toolchain and static-analysis requirements, record SEC-01 as **partially verified / security BLOCKED for ISOLATED** with mode-specific disposition per [ADR-0009](decisions/ADR-0009-trusted-local-execution.md) (SEC-01-STRICT not-required-for-TRUSTED_LOCAL; SEC-01-ISOLATED available-only-where-verified), and freeze explicit F1 acceptance criteria. S1 may begin with its minimum applicable contracts while remaining F0 contracts are completed incrementally; this is not a waiver of S3-ISOLATED. Repo documentation and `AGENTS.md` are versioned now.

**F1 (single vertical path):** S1 persistent Core/API/events/telemetry/minimal UI; S2 local Worker/leases/reconciliation; S3 real Paseo — S3-TRUSTED_LOCAL under ADR-0009 entry conditions (implemented local-trust policy, S2 green, provider operational, bounded readiness test, observable mode, no unauthorized delivery, owner authorization) or S3-ISOLATED inside a certified sandbox; S4 implementer + frozen ActionSpec/CandidateRevision + independent user-journey verifier + read-only Reviewer + deterministic Quality Gate (S4-A; browser checks included) and a Core-owned Campaign/Task orchestrator that natively advances the workflow, bounds repair and recovers across restart (S4-B); S4-C adds a Project Registry, Project-bound specs, scoped capability grants, Factory-managed Playwright and the complete Office provisioning journey; S4-C-03 adds deterministic failure classification, bounded pre-candidate Implementer recovery with immutable failed identities, explicit attempt budgets and protected-oracle discipline, with real-Paseo certification governed by the [live certification policy](../quality/live-certification.md); S5 governed GitHub PR/conditional R0 merge; S6 real self-development change managed by stable A against candidate B. One real local Worker and one primary project are enough; a second fixture repo validates project isolation.

F1 excludes production remote workers, mature autonomous global skills, full adaptive router, animated Office and automatic live Core upgrade.

## 16. Each increment must prove real behavior

For each slice: define independent oracle and negative cases before coding → implement + format/lint/build/tests → Implementer runs the product and repairs → freeze candidate SHA → **Independent Verifier** runs real CLI/API/Playwright/Paseo user journeys on a clean read-only candidate → independent Reviewer assesses quality/security/evidence → deterministic gate accepts or blocks. After repair, revalidate changed evidence and never silently relax a test.

F1 evidence must include an actual Paseo coding Run, tested sandbox/credential boundary, crash/restart and deduplication, exact candidate verification, separate Reviewer, actual PR and eligibility-specific merge decisions, a real UI timeline, and a functional Factory improvement delivered under the stable controller.

## 17. Documentation governance and compatibility

`AGENTS.md` is concise agent guidance; this MASTER expresses architecture; `docs/contracts/` describes implementation-independent protocols; ADRs record why; docs/engineering defines code and tool quality. Significant architecture changes require an Action and approved ADR, updated contracts/MASTER/tests and review. Supersede accepted ADRs rather than rewriting history.

**During pre-release development, do not implement compatibility with old internal contracts for hypothetical consumers.** Prefer one clean design and update all usages atomically. Document explicit stable external interfaces and avoid destructive migrations. Never treat this policy as permission to break the Quality Baseline or discard durable audit data.

## 18. Open blockers and decision owners

| ID | Open question | Required proof |
|---|---|---|
| SEC-01 | Mode-specific per [ADR-0009](decisions/ADR-0009-trusted-local-execution.md): STRICT not-required-for-TRUSTED_LOCAL (owner risk acceptance); ISOLATED paused / BLOCKED available-only-where-verified — host read canary FAIL; partial Podman/SDK/MCP feasibility PASS | ISOLATED/S3-ISOLATED: mandatory tool/credential OS boundary, real isolated Paseo session, bypass-negative and restart tests; [results](../security/SEC-01-results-2026-10-18.md). S3-TRUSTED_LOCAL entry conditions replace the absolute gate for that mode only. |
| EXEC-01 | Per-Run Paseo sandbox lifecycle and SDK capabilities | Real start/observe/cancel/cleanup/recovery |
| TECH-01 | Core/Worker language selection | **Closed: Go ratified by product owner**; Go toolchain/integration checks remain implementation tasks, not a comparative Rust prerequisite |
| DATA-01 | SQLite WAL, durability, single-writer semantics | Crash recovery, fencing, idempotency and replay tests |
| UI-01 | Browser event transport choice | Authenticated reconnect and missed/duplicate-event recovery |
| POLICY-01 | Initial quality/budget thresholds | Empirical baseline and policy tests |

## 19. References and normative supplements

- [Architecture decisions](decisions/README.md)
- [Documentation governance](../README.md)
- [Engineering/code quality](../engineering/code-quality.md)
- [Lint and static analysis](../engineering/linting.md)
- [Global quality baseline](../quality/baseline.md)
- [F1-S4-A quality pipeline contract](../contracts/quality-pipeline-s4a.md), [F1-S4-B campaign contract](../contracts/campaign-orchestration-s4b.md), [F1-S4-C multi-project contract](../contracts/multi-project-s4c.md), [ADR-0010](decisions/ADR-0010-frozen-candidate-quality-pipeline.md), [ADR-0011](decisions/ADR-0011-native-campaign-orchestration.md) and [ADR-0012](decisions/ADR-0012-project-registry-and-capabilities.md)
- [Live certification policy](../quality/live-certification.md), [campaign recovery contract](../contracts/campaign-recovery-s4c03.md), [ADR-0013](decisions/ADR-0013-bounded-autonomous-recovery.md) and [F1-S4-C acceptance](../implementation/f1-s4c-acceptance.md)
- [Isolation](../security/isolation.md)
- [F0/F1 implementation plan](../implementation/f0-f1-plan.md)
- Historical AEH for selective conceptual reuse: https://github.com/JamesMorales04/agentic-engineering-harness
- Agent guidelines: https://agents.md/
- Architecture decision template: https://github.com/adr/madr
- Observability: https://opentelemetry.io/docs/languages/go/

**This is design documentation, not a representation of working or certified software and not a grant of merge, deploy or privileged access.**
