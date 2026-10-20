# Software Factory

**Permanent repository:** [`JamesMorales04/software-factory`](https://github.com/JamesMorales04/software-factory)  
**Project phase:** F1-S1 through F1-S3-D and F1-S4-A merged; F1-S4-B native campaign orchestration implemented and exercised by deterministic and one bounded real workflow. SEC-01 security certification remains paused / BLOCKED. No component is certified beyond its recorded candidate-bound evidence.
**Selected implementation language:** Go for Factory Core and Workers ([ADR-0005](docs/architecture/decisions/ADR-0005-go-core-worker-language.md)); SEC-01 isolation validation remains pending. No production Core, Worker, or Factory Office has been certified or delivered.

Software Factory is a local-first, multi-project autonomous software engineering platform. It turns requirements and observed incidents into traceable Actions, composes scoped coding agents, executes changes in isolated workspaces, validates candidates independently, and delivers changes through governed GitHub workflows. Its first major milestone is a real self-development campaign controlled by a stable Factory instance.

## Start here

- [AGENTS.md](AGENTS.md) — concise operational rules for coding agents.
- [Architecture specification](docs/architecture/MASTER.md) — architecture, product decisions, guarantees, open risks.
- [Documentation governance](docs/README.md) — authority hierarchy and architecture change process.
- [F0/F1 execution plan](docs/implementation/f0-f1-plan.md) — slices and independently verified acceptance.
- [F1-S4-A acceptance matrix](docs/implementation/f1-s4a-acceptance.md) — frozen spec/candidate, independent verifier/reviewer, deterministic gate and browser checks.
- [F1-S4-B acceptance matrix](docs/implementation/f1-s4b-acceptance.md) — native Core campaign orchestration, repair, concurrency, recovery and browser journey.
- [Browser automation](docs/engineering/browser-automation.md) — pinned Playwright CLI, Implementer workflow and verifier boundary.
- [SEC-01 feasibility spike](docs/security/paseo-sandbox-spike.md) — Paseo, sandboxes, credentials and real-provider validation.
- [SEC-01 observed results (2026-10-18)](docs/security/SEC-01-results-2026-10-18.md) — partial functional passes, observed boundary failures and paused security gate.
- [Quality baseline](docs/quality/baseline.md) — non-negotiable acceptance requirements.
- [Engineering standards](docs/engineering/code-quality.md) — clean architecture, design, simplicity, and DRY.
- [Lint and static analysis policy](docs/engineering/linting.md) — stack-conditional tools and CI gates.
- [Telemetry](docs/observability/telemetry.md) — durable events, traces, metrics and logs from the first slice.
- [Bootstrap campaign brief](docs/implementation/campaign-prompt.md) — guidance for a future Campaign Director.

Documentation is the **versioned design source of truth**, not proof that features work. Every status must be supported by executable evidence. Architectural approvals and security authority remain governed by their respective policies.
