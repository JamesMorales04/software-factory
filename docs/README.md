# Documentation and governance

> **Native Rust replacement branch:** [ADR-0018](architecture/decisions/ADR-0018-native-rust-replacement.md) is the Product Owner's superseding target decision. The tracked tree is Rust-only; the former implementation and CI are retired. Historical records are archived under [docs/evidence/history](evidence/history/README.md). See the [native Rust handoff](implementation/native-rust-handoff.md) for actual working state.

All normative software architecture and engineering guidance is maintained **in this repository, in English**. Documentation must track real project state; prose is never sufficient evidence that a feature is implemented.

## Reading order

1. [Native Rust decision](architecture/decisions/ADR-0018-native-rust-replacement.md), [container-native Compose decision](architecture/decisions/ADR-0024-container-native-compose-runtime.md), [container-native and Office parity decision](architecture/decisions/ADR-0025-container-native-and-office-parity.md), [native bootstrap](../native/README.md), the [migration closure ledger](implementation/native-rust-closure.md) and [container-native status](implementation/container-native-status.md) — active target and current implemented scope. The older [handoff](implementation/native-rust-handoff.md) is a historical checkpoint.
2. [Rust engineering standards](engineering/rust-standards.md), [code quality](engineering/code-quality.md) and [linting](engineering/linting.md) — normative Rust design, correctness and objective static checks. The reusable agent workflow is [.agents/skills/rust-engineering](../.agents/skills/rust-engineering/SKILL.md).
3. [Service placement policy](architecture/service-placement-policy.md) and [assessment template](templates/service-placement-assessment.md) — where components may execute and which trust boundaries are required. The reusable agent workflow is [.agents/skills/service-placement](../.agents/skills/service-placement/SKILL.md).
4. [Container-native engineering](engineering/container-native-engineering.md), [container-first development](engineering/container-first-development.md), [Compose deployment](operations/compose-deployment.md), [container runtime security](security/container-runtime.md), [Office-first configuration](product/office-configuration.md), [Office design system](product/office-design-system.md), [Office parity matrix](product/office-parity.json), [editor setup](engineering/editor-setup.md), [ADR-0019](architecture/decisions/ADR-0019-container-first-cross-platform.md), [ADR-0024](architecture/decisions/ADR-0024-container-native-compose-runtime.md), [ADR-0025](architecture/decisions/ADR-0025-container-native-and-office-parity.md), [ADR-0027](architecture/decisions/ADR-0027-optional-tailscale-access.md) and [automation standards](engineering/automation-standards.md) — supported environments, installation, trust boundaries, containerized development, Office parity, language-server setup and canonical engineering automation.
5. [Quality baseline](quality/baseline.md) — candidate integrity, independent verification, and gate states; its old implementation references are historical.
6. [Decision records](architecture/decisions/README.md) — decisions and explicit supersession.

The [migration closure ledger](implementation/native-rust-closure.md) is the current status authority. The tested local workflow is documented in the [developer-preview testing guide](implementation/developer-preview-testing.md). The current managed live acceptance records are the [OpenCode external-project UAT](evidence/managed-live-uat-2026-10-26.md) and the [controlled self-development test](evidence/managed-self-development-2026-10-26.md); the earlier [F1-S4-LIVE-UAT report](evidence/f1-s4-live-uat-2026-10-25.md) remains provenance. The [capability registry](product/capabilities.yaml), [capability decisions](product/capability-decisions.md) and [roadmap](roadmap/ROADMAP.md) are active product authorities. Historical evidence and accepted ADRs remain provenance; they do not certify the Rust branch.

## Authority hierarchy

1. Explicit product-owner approvals and enforced Core authorization/security policies control permitted actions.
2. Accepted ADRs record significant decisions; applicable versioned contracts and policies define testable behavior.
3. [MASTER](architecture/MASTER.md) summarizes the agreed product design; it must remain consistent with ADRs and contracts.
4. `AGENTS.md` is a **short operational entry point**, not a second complete architecture specification.
5. The capability registry and generated roadmap record approved objectives and proposed implementation slices; roadmap entries, prompts, skills and agent recommendations do not grant execution authority.

For contradictory normative text, stop claiming compliance, open an Action/ADR and reconcile; do not silently select the permissive interpretation.

## Documentation update rules

- **Every PR** runs the governance entry check. Protected runtime, policy, evidence, skill, CI and normative documentation changes require a PR-scoped architectural impact declaration bound to the actual base and head; an independent reviewer assesses its meaning. Ordinary application changes receive an explicit no-assessment classification. Code changes still assess relevant docs, tests, examples, instrumentation and compatibility.
- **Material design decisions:** Action → proposed ADR with alternatives and consequences → risk/authority approval → update MASTER/contracts/quality/tests/diagrams together → independent review.
- **Accepted ADRs are append-only in meaning:** supersede them explicitly with a new ADR and mark old records `Superseded` without rewriting their original rationale.
- Security, credential boundaries, merge policies, Quality Gate, evidence acceptance and Core updates have protected approval paths.
- Clearly label statuses `Agreed`, `Proposed`, `Unverified`, `Implemented` or `Verified`. `Implemented` alone does not mean integrated and certified.
- **Early-stage internal breaking changes are expected.** See [ADR-0006](architecture/decisions/ADR-0006-early-development-compatibility.md); remove obsolete shims and update all internal references in one change. Public/stable contracts and persisted data require separately reviewed migration decisions.
- New contributors and agents must start with `AGENTS.md` and this index, then retrieve focused documents; avoid duplicate rules across files.
- CI should check Markdown style, links, formatting, and referenced files as soon as practical. See [lint policy](engineering/linting.md).

## What does not belong in Git

Runtime telemetry, raw session transcripts, large traces, recordings, screenshots, build outputs, credentials and confidential payloads are artifacts governed by storage/retention policy, **not** architecture docs. Commit only small sanitized test fixtures and immutable references/digests where necessary.
