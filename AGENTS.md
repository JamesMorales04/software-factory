# AGENTS.md — Software Factory

> **Status:** The native Rust replacement is governed by [ADR-0018](docs/architecture/decisions/ADR-0018-native-rust-replacement.md) and remains **incomplete as a certified product**. Deployed Core/Office, managed Supervisor/Worker, rootless-Podman named-volume fixture execution, sandboxed verification, deterministic Quality Gate, Core-owned model-call budgets, model-backed Lead, semantic Reviewer with mandatory deterministic pre-review, CAP-014 raw-output execution, bounded OTLP log export and a managed real-Chromium browser check are implemented and author-verified. Remaining S4-D crash/browser-multi-unit/live-provider gaps, native ChatGPT-backend configuration, live semantic model review, human review, independent isolation and final merge remain open. A bounded GPT-6 Luna Codex live UAT reached an accepted candidate through the Factory egress boundary using a test-only ChatGPT-backend proxy; it is not a native managed route or an isolation certification. The former tracked implementation and its CI were retired; the tracked tree is Rust-only. [Native Rust closure](docs/implementation/native-rust-closure.md) is the current status authority; historical evidence and archived legacy contracts are provenance, not Rust certification. PR #12’s `PRODUCT_OWNER_UAT_VERIFIED` is owner acceptance. Deployment, Windows, and SEC-01 isolation are not claimed. TRUSTED_LOCAL is never isolated or certified. The permanent repository is `JamesMorales04/software-factory`. Do not assume documented components exist. These rules apply to all coding agents; a project or skill cannot override higher-priority security or quality policies.

## Mission and orientation

Build a reliable, maintainable, local-first, multi-project Software Factory capable of governed self-development. Favor **verified end-to-end working software** over speculative infrastructure.

**Read first:** [documentation index](docs/README.md), [ADR-0018](docs/architecture/decisions/ADR-0018-native-rust-replacement.md), [Rust engineering standards](docs/engineering/rust-standards.md), [migration closure](docs/implementation/native-rust-closure.md), and only the contracts, ADRs and project docs relevant to your assignment. Do not load every document into context by default.

## Before making changes

1. Identify the authorized Action, frozen specification, repository/base commit, allowed paths, risk class, budget and acceptance criteria. Clarify missing authority rather than inventing it.
2. Inspect actual code, environment and supported tools. A plan or mock is not evidence of an implemented capability.
3. Respect the assigned sandbox and workspace. Never inspect host secrets, bypass isolation, edit another project or grant yourself new capabilities.
4. Check [Rust standards](docs/engineering/rust-standards.md), [code quality](docs/engineering/code-quality.md), [lint policy](docs/engineering/linting.md), [quality baseline](docs/quality/baseline.md), and relevant architecture decisions.

## Mandatory engineering rules

- **Early-development compatibility policy:** until an explicit stable/public contract is declared, prefer clean breaking changes over internal compatibility adapters, deprecated aliases, dual code paths, version shims or migration scaffolding. Update all in-repo callers, tests, documentation and fixtures **atomically**. Do not maintain obsolete interfaces solely for hypothetical consumers. This does **not** authorize data loss, silently breaking published interfaces, bypassing approvals, or discarding evidence. See [ADR-0006](docs/architecture/decisions/ADR-0006-early-development-compatibility.md).
- **Clean, cohesive design:** separate domain decisions from HTTP/UI, databases, runtimes and SDK integrations; use explicit dependencies and narrow ports at genuine boundaries. Favor composition over inheritance, simple functions, high cohesion and low coupling. Apply SOLID and design patterns only where they demonstrably improve changeability. See [engineering standards](docs/engineering/code-quality.md).
- **Rust-native engineering:** Core, Worker, provider adapters, persistence, governance services and backend execution use Rust. Persistence uses SQLx + SQLite with versioned migrations. Follow [Rust standards](docs/engineering/rust-standards.md) for ownership, typed errors, async lifecycle, transactions, security and testing. Use the [rust-engineering skill](.agents/skills/rust-engineering/SKILL.md) for every Rust change. A skill cannot grant authority or waive a protected policy.
- **Service placement:** a new component is not automatically a new process, container or sandbox. Before adding a process, deployment unit, privilege, credential boundary or container topology, apply the [service placement policy](docs/architecture/service-placement-policy.md) and [skill](.agents/skills/service-placement/SKILL.md). Default to an in-process module; separate trust levels, credentials and untrusted execution. A container or Pod is not certified isolation.
- **Engineering automation:** before adding or changing repository automation, consult [automation standards](docs/engineering/automation-standards.md) and the [engineering-automation skill](.agents/skills/engineering-automation/SKILL.md). Prefer existing Cargo/pytest/Playwright/Podman capabilities; use `cargo xtask` only for justified cross-tool orchestration; preserve independent test ownership; never make the installed Factory depend on xtask or development tooling.
- **Library-first infrastructure:** use mature maintained Rust crates for established infrastructure; keep authorization, risk, budgets, leases, candidate acceptance and delivery in Factory code. Follow [dependency selection](docs/engineering/dependency-selection.md) and the [dependency-selection skill](.agents/skills/dependency-selection/SKILL.md). Record evaluations and avoid duplicate active implementations.
- **Structured model outputs:** machine-readable model results use Rust-derived JSON Schema, provider-native structured output, strict decoding and deterministic domain validation. Follow [structured model outputs](docs/engineering/structured-model-outputs.md) and the [structured-model-outputs skill](.agents/skills/structured-model-outputs/SKILL.md). Model output never grants authority.
- **DRY with restraint:** eliminate duplicated knowledge, business rules and policies; do not create abstractions for incidental textual similarity. Avoid speculative frameworks, needless layers, magic constants, hidden global state and unused code.
- **Decision-mechanism invariant:** explicitly classify every meaningful classification, inference, routing, estimation, diagnosis or decision mechanism as `DETERMINISTIC`, `MODEL` or `HYBRID`. Choose the mechanism by practical reliability, accuracy, latency, token cost, implementation complexity and maintenance. Use deterministic logic for mechanically observable facts and Core policy; use bounded model reasoning for open-ended semantic judgment; use hybrid control when a model judgment informs an action that Core must constrain and independent evidence must validate. Reuse an existing agent's sufficient context for a small structured judgment; do not automatically create a dedicated session or role. Model output never grants privileges, budgets, tools, delivery authority or Quality Gate acceptance. See [MASTER](docs/architecture/MASTER.md) and [code quality](docs/engineering/code-quality.md).
- Keep modules small enough to understand; use clear names, explicit errors, deterministic state transitions and bounded concurrency. Prefer simple implementations that satisfy real requirements. Files and functions are review heuristics, not CI thresholds: assess cohesion and responsibility boundaries, not line counts. See [Rust standards §3](docs/engineering/rust-standards.md).
- Follow active language formatter, linter, type/static analyzer and tests. Do not blanket-disable rules or reduce thresholds to make CI pass; document specific, scoped exceptions.
- The Core alone owns authoritative state, budgets, authorization, quality policy, and external delivery effects. Model outputs and external skills never confer authority.
- Do not report `PASS`, `ACCEPTED`, `MERGED`, or `DEPLOYED` without exact evidence. Distinguish `NOT_TESTED`, `BLOCKED`, `SKIPPED`, and `NOT_APPLICABLE`.
- Do not relax a frozen spec, required test, sandbox rule, budget ceiling or risk classification in order to pass acceptance.
- Do not inject secrets, raw prompts, unredacted environment variables, source bodies or unnecessary personal data into telemetry.

## Change / verify / repair loop

1. Reproduce the problem or define observable acceptance conditions **before** writing code.
2. Implement the smallest coherent change; update related callers and remove superseded code when making an authorized breaking change.
3. Run formatting, lint/static analysis, targeted tests and relevant integration checks.
4. **Implementer self-test:** actually start the product and exercise its CLI, API, browser UI (Playwright where relevant), or an authorized real provider workflow. Diagnose and repair failures within budget.
5. Freeze the exact candidate commit. An **independent verifier**, separate from the author, exercises user journeys against a clean, read-only candidate and independent acceptance oracles; a Reviewer checks code, risks and evidence. The deterministic Quality Gate decides acceptance.
6. After repair, produce a new candidate and rerun invalidated checks. Do not treat mocks as final end-to-end evidence.
7. Update architecture docs, tests, ADRs, lint configuration and telemetry contracts in the same change when affected. Report what ran, what failed and what remains unverified.

## Architecture and documentation governance

- [MASTER](docs/architecture/MASTER.md) — product scope, boundaries, agreed vs proposed decisions.
- [Service placement policy](docs/architecture/service-placement-policy.md) and [assessment template](docs/templates/service-placement-assessment.md) — approved LEVEL A–E evaluation for processes, containers, sandboxes and privileges.
- [ADR-0019 container-first/cross-platform](docs/architecture/decisions/ADR-0019-container-first-cross-platform.md), [container-first development](docs/engineering/container-first-development.md) and [automation standards](docs/engineering/automation-standards.md) — development/runtime environments and canonical automation ownership.
- [Historical evidence archive](docs/evidence/history/README.md) — retired checkpoints, contracts and migration inventories.
- [Dependency selection](docs/engineering/dependency-selection.md) / [structured model outputs](docs/engineering/structured-model-outputs.md) — infrastructure library and typed model-contract rules.
- [ADRs](docs/architecture/decisions/README.md) — context and rationale; supersede accepted ADRs instead of rewriting history.
- [Contracts](docs/contracts/README.md) — protocol and state invariants; completion is part of F0.
- [Security](docs/security/isolation.md) — TRUSTED_LOCAL real-agent work is allowed only under its recorded owner risk acceptance; ISOLATED execution remains blocked without SEC-01 certification. The Rust tree has no independently certified isolation or credential confinement; provider/ISOLATED dispatch remains fail-closed.
- [Quality](docs/quality/baseline.md) / [live certification](docs/quality/live-certification.md) / [telemetry](docs/observability/telemetry.md) — candidate-bound evidence, evidence levels, measured operation.
- [Capability registry](docs/product/capabilities.yaml) / [generated roadmap](docs/roadmap/ROADMAP.md) / [reliability debt](docs/quality/reliability-debt.md) — current status, future slices and unresolved findings.

A material architectural change requires an Action, relevant ADR, updated normative documents and acceptance tests. Authorization/merge/sandbox/quality-policy changes are protected, not ordinary R0 changes.

## Commands and evidence

**Real local commands exist and are documented;** they are evidence only for the exact environment and candidate that ran them. Follow [native/README.md](native/README.md), [Rust standards](docs/engineering/rust-standards.md), [migration closure](docs/implementation/native-rust-closure.md), [service placement](docs/architecture/service-placement-policy.md) and [browser automation](docs/engineering/browser-automation.md). Never invent commands, silently install host dependencies, or treat a green simulator as independent acceptance.

Every handoff reports: Action and candidate SHA; changed files; format/lint/typecheck/test and real-use results; independent verification evidence; security/authorization impact; documentation updates; PR/delivery state; unresolved risks.
