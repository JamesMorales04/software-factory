# Software Factory

**Permanent repository:** [`JamesMorales04/software-factory`](https://github.com/JamesMorales04/software-factory)  
**Project phase:** F1-S1 through F1-S4-C merged through PR #12. The C-07 Vlog path has bounded real-Paseo acceptance at Factory head `9567bc5` / runtime `14133cb7` and a separate Product Owner UAT attestation. No deployment, SEC-01 isolation, autonomous delivery or live corrective repair is certified. See the [capability registry](docs/product/capabilities.yaml), [roadmap](docs/roadmap/ROADMAP.md) and [live certification policy](docs/quality/live-certification.md) for precise scope.
**Selected implementation language:** Go for Factory Core and Workers ([ADR-0005](docs/architecture/decisions/ADR-0005-go-core-worker-language.md)); SEC-01 isolation remains paused.

Software Factory is a local-first, multi-project engineering platform under development. Its current Core runs governed Actions and Campaigns through a local Worker and candidate-bound quality pipeline. Autonomous intent planning, maintenance, GitHub delivery and stable-controller self-development are future capabilities in the roadmap.

## Start here

- [AGENTS.md](AGENTS.md) — concise operational rules for coding agents.
- [Architecture specification](docs/architecture/MASTER.md) — architecture, product decisions, guarantees, open risks.
- [Documentation governance](docs/README.md) — authority hierarchy and architecture change process.
- [F0/F1 execution plan](docs/implementation/f0-f1-plan.md) — slices and independently verified acceptance.
- [F1-S4-A acceptance matrix](docs/implementation/f1-s4a-acceptance.md) — frozen spec/candidate, independent verifier/reviewer, deterministic gate and browser checks.
- [F1-S4-B acceptance matrix](docs/implementation/f1-s4b-acceptance.md) — native Core campaign orchestration, repair, concurrency, recovery and browser journey.
- [F1-S4-C acceptance matrix](docs/implementation/f1-s4c-acceptance.md) — multi-project capability scoping and bounded live evidence.
- [Capability registry](docs/product/capabilities.yaml) and [roadmap](docs/roadmap/ROADMAP.md) — canonical status and future slices.
- [Browser automation](docs/engineering/browser-automation.md) — pinned Playwright CLI, Implementer workflow and verifier boundary.
- [SEC-01 feasibility spike](docs/security/paseo-sandbox-spike.md) — Paseo, sandboxes, credentials and real-provider validation.
- [SEC-01 observed results (2026-10-18)](docs/security/SEC-01-results-2026-10-18.md) — partial functional passes, observed boundary failures and paused security gate.
- [Quality baseline](docs/quality/baseline.md) — non-negotiable acceptance requirements.
- [Engineering standards](docs/engineering/code-quality.md) — clean architecture, design, simplicity, and DRY.
- [Lint and static analysis policy](docs/engineering/linting.md) — stack-conditional tools and CI gates.
- [Telemetry](docs/observability/telemetry.md) — durable events, traces, metrics and logs from the first slice.
- [Bootstrap campaign brief](docs/implementation/campaign-prompt.md) — guidance for a future Campaign Director.

Documentation is the **versioned design source of truth**, not proof that features work. Every status must be supported by executable evidence. Architectural approvals and security authority remain governed by their respective policies.
