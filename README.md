# Software Factory

**Permanent repository:** [`JamesMorales04/software-factory`](https://github.com/JamesMorales04/software-factory)  
**Project phase:** native Rust developer-preview baseline, PRs #17–#27 merged, plus the container-native Compose iteration on a campaign branch. Core/Office, containerized Supervisor/Worker, a dedicated nested-Podman execution engine and narrow runtime broker, sandboxed independent verifier, deterministic Quality Gate, Core-owned model-call budgets, Office-first first-run enrollment/provider/project/action/delegation configuration, semantic Reviewer with mandatory deterministic pre-review and automatic quality dispatch are implemented and author-run. A deterministic clean Compose deployment reached an `ACCEPTED` candidate, preserved state across restart and passed real-Chromium Office onboarding; parallel attempts and a managed OpenCode/DeepSeek live acceptance are recorded. Codex native ChatGPT authentication, containerized Codex live inference, paid UAT of the new stack, SEC-01 ISOLATED certification, Windows support, independent security review and final merge remain open. See the [container-native status](docs/implementation/container-native-status.md), [Compose deployment](docs/operations/compose-deployment.md), [migration closure ledger](docs/implementation/native-rust-closure.md), [capability decisions](docs/product/capability-decisions.md) and [capability registry](docs/product/capabilities.yaml).
**Implementation:** Rust for Core, Worker, Supervisor, verifier, reviewer, runtime broker and governance; React/TypeScript for the single Office; rootless Podman Compose for the permanent stack ([ADR-0024](docs/architecture/decisions/ADR-0024-container-native-compose-runtime.md)). SEC-01 isolation remains paused and is not claimed.

## Quick start (developer preview)

Prerequisites: rootless Podman 6.x and a supported Compose provider reachable through `podman compose` (tested with `podman-compose`). Rust, Node, npm, OpenCode and Codex are not host requirements.

```sh
git clone https://github.com/JamesMorales04/software-factory.git
cd software-factory
PODMAN_COMPOSE_PROVIDER=/usr/bin/podman-compose podman compose up -d --build
```

Open `http://127.0.0.1:8893`, read the one-time enrollment code with `podman compose exec core cat /data/enrollment-code`, and complete setup in Office. With a configured Docker Compose provider and the rootless Podman socket enabled, the plain `podman compose up -d --build` form is also supported. See [Compose deployment](docs/operations/compose-deployment.md) for prerequisites, lifecycle, diagnostics and unsupported claims.

Software Factory is a local-first, multi-project engineering platform under development. The current Core runs governed Actions and Campaigns through a managed Worker/Supervisor, rootless Podman, independent sandboxed verification and a deterministic Quality Gate. `factory start` accepts strict private Core-model and Worker-provider env files. OpenCode coding is verified on a bounded TRUSTED_LOCAL route; Codex coding is implemented and deterministic-tested, but a supported native ChatGPT-backend live route is not available through the current credential boundary. Maintenance, GitHub delivery and stable-controller self-development remain future roadmap capabilities.

## Start here

- [AGENTS.md](AGENTS.md) — concise operational rules for coding agents.
- [Architecture specification](docs/architecture/MASTER.md) — architecture, product decisions, guarantees, open risks.
- [Documentation governance](docs/README.md) — authority hierarchy and architecture change process.
- [Compose deployment and onboarding](docs/operations/compose-deployment.md) — the one-command product path.
- [Container runtime security](docs/security/container-runtime.md) — phase-zero measurements, capability exceptions and trust boundaries.
- [Office-first configuration](docs/product/office-configuration.md) — providers, projects, checks and delegations.
- [Capability registry](docs/product/capabilities.yaml) and [roadmap](docs/roadmap/ROADMAP.md) — canonical status and future slices.
- [Browser automation](docs/engineering/browser-automation.md) — pinned Playwright CLI, Implementer workflow and verifier boundary.
- [Security isolation](docs/security/isolation.md) — trust boundaries and the blocked ISOLATED gate.
- [Historical evidence](docs/evidence/history/README.md) — archived checkpoints, contracts and security findings.
- [Quality baseline](docs/quality/baseline.md) — non-negotiable acceptance requirements.
- [Engineering standards](docs/engineering/code-quality.md) — clean architecture, design, simplicity, and DRY.
- [Lint and static analysis policy](docs/engineering/linting.md) — stack-conditional tools and CI gates.
- [Telemetry](docs/observability/telemetry.md) — durable events, traces, metrics and logs from the first slice.

Documentation is the **versioned design source of truth**, not proof that features work. Every status must be supported by executable evidence. Architectural approvals and security authority remain governed by their respective policies.
