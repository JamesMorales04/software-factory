# Software Factory

**Permanent repository:** [`JamesMorales04/software-factory`](https://github.com/JamesMorales04/software-factory)  
**Project phase:** native Rust developer-preview baseline, PRs #17–#26 merged. Core/Office, managed Supervisor/Worker, rootless-Podman named-volume execution, sandboxed verifier, deterministic Quality Gate, Core-owned model-call budgets, model-backed Intelligent Lead, semantic Reviewer with mandatory deterministic pre-review, CAP-014 raw-output execution, bounded OTLP log export, a managed real-Chromium browser check, OpenCode and Codex App Server coding adapters, run lifecycle projection and the pinned Rust editor/developer-container setup are implemented. A managed external-project UAT reached a genuine OpenCode/DeepSeek candidate, independent rootless-Podman verification, semantic review and `ACCEPTED`; a separate controlled self-development run reached `ACCEPTED` in a disposable clone. Codex native ChatGPT-authenticated live inference, paid cancellation/revocation injection, SEC-01 ISOLATED certification, Windows/deployment, human security review and final merge remain open. See the [managed live acceptance record](docs/evidence/managed-live-uat-2026-10-26.md), [migration closure ledger](docs/implementation/native-rust-closure.md), [capability decisions](docs/product/capability-decisions.md) and [capability registry](docs/product/capabilities.yaml).
**Implementation:** Rust for Core, Worker, Supervisor, verifier and governance; React/TypeScript for the single Office ([ADR-0018](docs/architecture/decisions/ADR-0018-native-rust-replacement.md)). SEC-01 isolation remains paused and is not claimed.

Software Factory is a local-first, multi-project engineering platform under development. The current Core runs governed Actions and Campaigns through a managed Worker/Supervisor, rootless Podman, independent sandboxed verification and a deterministic Quality Gate. `factory start` accepts strict private Core-model and Worker-provider env files. OpenCode coding is verified on a bounded TRUSTED_LOCAL route; Codex coding is implemented and deterministic-tested, but a supported native ChatGPT-backend live route is not available through the current credential boundary. Maintenance, GitHub delivery and stable-controller self-development remain future roadmap capabilities.

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
