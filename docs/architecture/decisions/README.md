# Architecture Decision Records (ADRs)

An ADR records the **context, alternatives, decision, and consequences** of a significant design choice. The [MASTER](../MASTER.md) summarizes current architecture; [contracts](../../contracts/README.md) specify externally observable behavior.

Statuses: `Proposed`, `Accepted`, `Rejected`, `Superseded`, `Deprecated`. Preserve accepted rationale; supersede with a new ADR and cross-link it. Routine code changes do not require an ADR. [Template](../../templates/adr.md).

- [ADR-0001 — Architecture documentation in Git](ADR-0001-documentation-in-git.md) — documented source of truth.
- [ADR-0002 — Global quality baseline](ADR-0002-global-quality-baseline.md) — agreed required acceptance guarantees.
- [ADR-0003 — Telemetry from the first slice](ADR-0003-telemetry-from-inception.md) — early instrumentation.
- [ADR-0004 — Independent real-user verification](ADR-0004-independent-user-journey-verification.md) — verification separate from the author.
- [ADR-0005 — Go for Core and Workers](ADR-0005-go-core-worker-language.md) — accepted; SEC-01 remains an independent security gate.
- [ADR-0006 — Early-development compatibility](ADR-0006-early-development-compatibility.md) — no speculative compatibility shims.
- [ADR-0007 — Maintainable, DRY code](ADR-0007-maintainable-code-and-dry.md) — code architecture quality principles.
- [ADR-0008 — Static quality gates](ADR-0008-static-analysis-gates.md) — language-appropriate linting.
- [ADR-0009 — Trusted-local execution](ADR-0009-trusted-local-execution.md) — accepted; owner risk acceptance for TRUSTED_LOCAL broad local access; SEC-01-STRICT not required for that mode, SEC-01-ISOLATED available-only-where-verified.
- [ADR-0010 — Frozen candidate quality pipeline](ADR-0010-frozen-candidate-quality-pipeline.md) — accepted; immutable spec/candidate, independent verifier/reviewer, deterministic Core gate and pinned agent-native browser automation.
