# Documentation and governance

> **Native Rust replacement branch:** [ADR-0018](architecture/decisions/ADR-0018-native-rust-replacement.md) is the Product Owner's superseding target decision. This branch is not cut over; historical Go/Paseo documents below describe the old implementation. See the [native Rust handoff](implementation/native-rust-handoff.md) for actual working state.

All normative software architecture and engineering guidance is maintained **in this repository, in English**. Documentation must track real project state; prose is never sufficient evidence that a feature is implemented.

## Reading order

1. [Native Rust decision](architecture/decisions/ADR-0018-native-rust-replacement.md), [native bootstrap](../native/README.md), and [handoff](implementation/native-rust-handoff.md) — active target and implemented scope on this branch.
2. [Rust engineering standards](engineering/rust-standards.md), [code quality](engineering/code-quality.md) and [linting](engineering/linting.md) — normative Rust design, correctness and objective static checks. The reusable agent workflow is [.agents/skills/rust-engineering](../../.agents/skills/rust-engineering/SKILL.md).
3. [Service placement policy](architecture/service-placement-policy.md) and [assessment template](templates/service-placement-assessment.md) — where components may execute and which trust boundaries are required. The reusable agent workflow is [.agents/skills/service-placement](../../.agents/skills/service-placement/SKILL.md).
4. [Quality baseline](quality/baseline.md) — candidate integrity, independent verification, and gate states; its old implementation references are historical.
4. [Decision records](architecture/decisions/README.md) — decisions and explicit supersession.

The [old master](architecture/MASTER.md), [contracts](contracts/README.md), [security spike](security/paseo-sandbox-spike.md), [live certification](quality/live-certification.md), [capability registry](product/capabilities.yaml), [roadmap](roadmap/ROADMAP.md), and [AEH assessment](architecture/AEH-CAPABILITY-ASSESSMENT.md) are historical input. They do not define the new runtime or certify the Rust branch. Their replacement is required before cutover.

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
