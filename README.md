# Software Factory

**Permanent repository:** [`JamesMorales04/software-factory`](https://github.com/JamesMorales04/software-factory)  
**Project phase:** native Rust migration closure is in progress on PR #17. The deployed Core/Office, managed Supervisor/Worker, rootless-Podman named-volume execution, sandboxed verifier and deterministic fixture Campaign path are implemented and author-verified. S4-D Lead/WorkGraph parity, provider coding, legacy deletion, independent isolation and final review remain open. See the [migration closure ledger](docs/implementation/native-rust-closure.md), [capability decisions](docs/product/capability-decisions.md) and [capability registry](docs/product/capabilities.yaml).
**Implementation:** Rust for Core, Worker, Supervisor, verifier and governance; React/TypeScript for the single Office ([ADR-0018](docs/architecture/decisions/ADR-0018-native-rust-replacement.md)). SEC-01 isolation remains paused and is not claimed.

Software Factory is a local-first, multi-project engineering platform under development. The current Core runs governed fixture Actions and Campaigns through a managed Worker/Supervisor, rootless Podman, independent sandboxed verification and a deterministic Quality Gate. Provider-backed coding, autonomous start, maintenance, GitHub delivery and stable-controller self-development remain fail-closed or future roadmap capabilities.

## Start here

- [AGENTS.md](AGENTS.md) — concise operational rules for coding agents.
- [Architecture specification](docs/architecture/MASTER.md) — architecture, product decisions, guarantees, open risks.
- [Documentation governance](docs/README.md) — authority hierarchy and architecture change process.
- [Capability registry](docs/product/capabilities.yaml) and [roadmap](docs/roadmap/ROADMAP.md) — canonical status and future slices.
- [Browser automation](docs/engineering/browser-automation.md) — pinned Playwright CLI, Implementer workflow and verifier boundary.
- [Security isolation](docs/security/isolation.md) — trust boundaries and the blocked ISOLATED gate.
- [Historical evidence](docs/evidence/history/README.md) — archived checkpoints, contracts and security findings.
- [Quality baseline](docs/quality/baseline.md) — non-negotiable acceptance requirements.
- [Engineering standards](docs/engineering/code-quality.md) — clean architecture, design, simplicity, and DRY.
- [Lint and static analysis policy](docs/engineering/linting.md) — stack-conditional tools and CI gates.
- [Telemetry](docs/observability/telemetry.md) — durable events, traces, metrics and logs from the first slice.

Documentation is the **versioned design source of truth**, not proof that features work. Every status must be supported by executable evidence. Architectural approvals and security authority remain governed by their respective policies.
