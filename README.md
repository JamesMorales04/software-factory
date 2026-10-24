# Software Factory

**Permanent repository:** [`JamesMorales04/software-factory`](https://github.com/JamesMorales04/software-factory)  
**Project phase:** native Rust migration closure is in progress on PR #17. The deployed Core/Office, managed Supervisor/Worker, rootless-Podman named-volume execution, sandboxed verifier and deterministic fixture Campaign path are implemented and author-verified. S4-D Lead/WorkGraph parity, provider coding, legacy deletion, independent isolation and final review remain open. See the [migration closure ledger](docs/implementation/native-rust-closure.md), [capability decisions](docs/product/capability-decisions.md) and [capability registry](docs/product/capabilities.yaml).
**Implementation:** Rust for Core, Worker, Supervisor, verifier and governance; React/TypeScript for the single Office ([ADR-0018](docs/architecture/decisions/ADR-0018-native-rust-replacement.md)). SEC-01 isolation remains paused and is not claimed.

Software Factory is a local-first, multi-project engineering platform under development. The current Core runs governed fixture Actions and Campaigns through a managed Worker/Supervisor, rootless Podman, independent sandboxed verification and a deterministic Quality Gate. Provider-backed coding, autonomous start, maintenance, GitHub delivery and stable-controller self-development remain fail-closed or future roadmap capabilities.

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
