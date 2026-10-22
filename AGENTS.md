# AGENTS.md — Software Factory

> **Status:** F1-S1 through F1-S4-B merged. F1-S4-C multi-project registry, scoped capabilities, Factory-managed browser capability, bounded autonomous Implementer recovery, same-session browser capability completion and Worker-managed browser test servers are implemented and deterministically verified. F1-S4-C-07 achieved the bounded real-Paseo Vlog live path on tested head `9567bc5` / runtime `14133cb7`: one paid session, initial-turn model-mediated browser interaction, independent verifier PASS, reviewer APPROVE and gate ACCEPTED. PR #12 is merged at `9ec7643`; `PRODUCT_OWNER_UAT_VERIFIED` is recorded from the Product Owner's explicit attestation (owner acceptance, not independent technical evidence). Deployment and SEC-01 isolation are not claimed. No other Factory product component is certified beyond its recorded candidate-bound evidence. SEC-01 security work is paused, **not approved**; TRUSTED_LOCAL is never isolated or certified. The permanent repository is `JamesMorales04/software-factory`. Do not assume documented components exist. These rules apply to all coding agents; a project or skill cannot override higher-priority security or quality policies.

## Mission and orientation

Build a reliable, maintainable, local-first, multi-project Software Factory capable of governed self-development. Favor **verified end-to-end working software** over speculative infrastructure.

**Read first:** [documentation index](docs/README.md), [architecture baseline](docs/architecture/MASTER.md), and only the contracts, ADRs, engineering rules and project docs relevant to your assignment. Do not load every document into context by default.

## Before making changes

1. Identify the authorized Action, frozen specification, repository/base commit, allowed paths, risk class, budget and acceptance criteria. Clarify missing authority rather than inventing it.
2. Inspect actual code, environment and supported tools. A plan or mock is not evidence of an implemented capability.
3. Respect the assigned sandbox and workspace. Never inspect host secrets, bypass isolation, edit another project or grant yourself new capabilities.
4. Check [code quality](docs/engineering/code-quality.md), [lint policy](docs/engineering/linting.md), [quality baseline](docs/quality/baseline.md), and relevant architecture decisions.

## Mandatory engineering rules

- **Early-development compatibility policy:** until an explicit stable/public contract is declared, prefer clean breaking changes over internal compatibility adapters, deprecated aliases, dual code paths, version shims or migration scaffolding. Update all in-repo callers, tests, documentation and fixtures **atomically**. Do not maintain obsolete interfaces solely for hypothetical consumers. This does **not** authorize data loss, silently breaking published interfaces, bypassing approvals, or discarding evidence. See [ADR-0006](docs/architecture/decisions/ADR-0006-early-development-compatibility.md).
- **Clean, cohesive design:** separate domain decisions from HTTP/UI, databases, runtimes and SDK integrations; use explicit dependencies and narrow ports at genuine boundaries. Favor composition over inheritance, simple functions, high cohesion and low coupling. Apply SOLID and design patterns only where they demonstrably improve changeability. See [engineering standards](docs/engineering/code-quality.md).
- **DRY with restraint:** eliminate duplicated knowledge, business rules and policies; do not create abstractions for incidental textual similarity. Avoid speculative frameworks, needless layers, magic constants, hidden global state and unused code.
- Keep modules small enough to understand; use clear names, explicit errors, deterministic state transitions and bounded concurrency. Prefer simple implementations that satisfy real requirements.
- Follow active language formatter, linter, type/static analyzer and tests. Do not blanket-disable rules or reduce thresholds to make CI pass; document specific, scoped exceptions.
- The Core alone owns authoritative state, budgets, authorization, quality policy, and external delivery effects. Model outputs and external skills never confer authority.
- Do not report `PASS`, `ACCEPTED`, `MERGED`, or `DEPLOYED` without exact evidence. Distinguish `NOT_TESTED`, `BLOCKED`, `SKIPPED`, and `NOT_APPLICABLE`.
- Do not relax a frozen spec, required test, sandbox rule, budget ceiling or risk classification in order to pass acceptance.
- Do not inject secrets, raw prompts, unredacted environment variables, source bodies or unnecessary personal data into telemetry.

## Change / verify / repair loop

1. Reproduce the problem or define observable acceptance conditions **before** writing code.
2. Implement the smallest coherent change; update related callers and remove superseded code when making an authorized breaking change.
3. Run formatting, lint/static analysis, targeted tests and relevant integration checks.
4. **Implementer self-test:** actually start the product and exercise its CLI, API, browser UI (Playwright where relevant), or real Paseo workflow. Diagnose and repair failures within budget.
5. Freeze the exact candidate commit. An **independent verifier**, separate from the author, exercises user journeys against a clean, read-only candidate and independent acceptance oracles; a Reviewer checks code, risks and evidence. The deterministic Quality Gate decides acceptance.
6. After repair, produce a new candidate and rerun invalidated checks. Do not treat mocks as final end-to-end evidence.
7. Update architecture docs, tests, ADRs, lint configuration and telemetry contracts in the same change when affected. Report what ran, what failed and what remains unverified.

## Architecture and documentation governance

- [MASTER](docs/architecture/MASTER.md) — product scope, boundaries, agreed vs proposed decisions.
- [ADRs](docs/architecture/decisions/README.md) — context and rationale; supersede accepted ADRs instead of rewriting history.
- [Contracts](docs/contracts/README.md) — protocol and state invariants; completion is part of F0.
- [Security](docs/security/isolation.md), [SEC-01](docs/security/paseo-sandbox-spike.md) and [2026-10-18 results](docs/security/SEC-01-results-2026-10-18.md) — TRUSTED_LOCAL real-agent work is allowed only under its recorded owner risk acceptance; ISOLATED execution remains blocked without SEC-01 certification.
- [Quality](docs/quality/baseline.md) / [live certification](docs/quality/live-certification.md) / [telemetry](docs/observability/telemetry.md) — candidate-bound evidence, evidence levels, measured operation.
- [Capability registry](docs/product/capabilities.yaml) / [generated roadmap](docs/roadmap/ROADMAP.md) / [reliability debt](docs/quality/reliability-debt.md) — current status, future slices and unresolved findings.
- [F0/F1 plan](docs/implementation/f0-f1-plan.md) — slice boundaries and independent real-use acceptance.

A material architectural change requires an Action, relevant ADR, updated normative documents and acceptance tests. Authorization/merge/sandbox/quality-policy changes are protected, not ordinary R0 changes.

## Commands and evidence

**Real local commands exist and are documented;** they are evidence only for the exact environment and candidate that ran them. Follow [core/README.md](core/README.md), [quality acceptance](docs/implementation/f1-s4a-acceptance.md) and [browser automation](docs/engineering/browser-automation.md). Never invent commands, silently install host dependencies, or treat a green simulator as independent acceptance.

Every handoff reports: Action and candidate SHA; changed files; format/lint/typecheck/test and real-use results; independent verification evidence; security/authorization impact; documentation updates; PR/delivery state; unresolved risks.
