# Architecture Decision Records (ADRs)

An ADR records the **context, alternatives, decision, and consequences** of a significant design choice. The [MASTER](../MASTER.md) summarizes current architecture; [contracts](../../contracts/README.md) specify externally observable behavior.

Statuses: `Proposed`, `Accepted`, `Rejected`, `Superseded`, `Deprecated`. Preserve accepted rationale; supersede with a new ADR and cross-link it. Routine code changes do not require an ADR. [Template](../../templates/adr.md).

The new target architecture is [ADR-0018](ADR-0018-native-rust-replacement.md). The Rust branch remains an incomplete replacement and does not inherit old certification.

- [ADR-0001 — Architecture documentation in Git](ADR-0001-documentation-in-git.md) — documented source of truth.
- [ADR-0002 — Global quality baseline](ADR-0002-global-quality-baseline.md) — agreed required acceptance guarantees.
- [ADR-0003 — Telemetry from the first slice](ADR-0003-telemetry-from-inception.md) — early instrumentation.
- [ADR-0004 — Independent real-user verification](ADR-0004-independent-user-journey-verification.md) — verification separate from the author.
- [ADR-0005 — Go for Core and Workers](ADR-0005-go-core-worker-language.md) — superseded by ADR-0018; historical rationale preserved.
- [ADR-0006 — Early-development compatibility](ADR-0006-early-development-compatibility.md) — no speculative compatibility shims.
- [ADR-0007 — Maintainable, DRY code](ADR-0007-maintainable-code-and-dry.md) — code architecture quality principles.
- [ADR-0008 — Static quality gates](ADR-0008-static-analysis-gates.md) — language-appropriate linting.
- [ADR-0009 — Trusted-local execution](ADR-0009-trusted-local-execution.md) — accepted; owner risk acceptance for TRUSTED_LOCAL broad local access; SEC-01-STRICT not required for that mode, SEC-01-ISOLATED available-only-where-verified.
- [ADR-0010 — Frozen candidate quality pipeline](ADR-0010-frozen-candidate-quality-pipeline.md) — accepted; immutable spec/candidate, independent verifier/reviewer, deterministic Core gate and pinned agent-native browser automation.
- [ADR-0011 — Native campaign orchestration](ADR-0011-native-campaign-orchestration.md) — accepted; Core-owned Campaign/Task plan, native participant dispatch, bounded repair and durable recovery.
- [ADR-0012 — Multi-project registry and scoped capabilities](ADR-0012-project-registry-and-capabilities.md) — accepted; Project-bound specs, capability grants, Factory-managed Playwright and project-fair dispatch. TRUSTED_LOCAL only.
- [ADR-0013 — Bounded autonomous Implementer recovery](ADR-0013-bounded-autonomous-recovery.md) — accepted; deterministic failure classes, fenced new Runs, immutable failed identity, protected oracle and explicit attempt budgets.
- [ADR-0014 — Same-session capability completion](ADR-0014-same-session-capability-completion.md) — accepted; one bounded continuation in the same provider session when a required browser interaction is missing.
- [ADR-0015 — Worker-managed browser test server](ADR-0015-worker-managed-browser-test-server.md) — accepted; declarative test-server lifecycle, ownership and infrastructure-failure boundary.
- [ADR-0016 — Dual-mode autonomous engineering](ADR-0016-dual-mode-autonomous-engineering.md) — accepted product objectives and shared Core authority; future implementation and protected decisions remain separate.
- [ADR-0018 — Native Rust replacement](ADR-0018-native-rust-replacement.md) — accepted target architecture; implementation incomplete.
- [ADR-0019 — Container-first cross-platform development and runtime](ADR-0019-container-first-cross-platform.md) — approved Linux/Windows-through-Podman-Machine direction; implementation and Windows verification incomplete.
- [ADR-0020 — Predecessor independence and editorial ADR amendments](ADR-0020-predecessor-independence-and-editorial-amendments.md) — accepted; narrow editorial amendment mechanism and independent Factory terminology.
- [ADR-0021 — Rust runtime implementation status](ADR-0021-rust-runtime-implementation-status.md) — accepted; current CLI/container/Supervisor implementation status and remaining gaps.
- [ADR-0022 — Source-aware provider plans and mediated tools](ADR-0022-source-aware-provider-plan-mediation.md) — proposed; first trusted-local provider path with Worker-held credentials, bounded source context, write/command mediation, validated candidate application and durable session evidence.
- [ADR-0024 — Container-native Compose runtime and Office-first configuration](ADR-0024-container-native-compose-runtime.md) — approved developer-preview direction; dedicated nested engine/broker, per-service secrets and automatic quality; SEC-01 and Windows remain open.
- [ADR-0025 — Container-native engineering and Office-first parity](ADR-0025-container-native-and-office-parity.md) — accepted; Compose and the containerized development workflow are mandatory, Office-first parity is a permanent product rule, and the design system/parity matrix are enforced by CI and review.
