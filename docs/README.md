# Documentation and governance

All normative software architecture and engineering guidance is maintained **in this repository, in English**. Documentation must track real project state; prose is never sufficient evidence that a feature is implemented.

## Reading order

1. [Master architecture](architecture/MASTER.md) — scope, decisions, boundaries and stage gates.
2. [Engineering standards](engineering/code-quality.md) and [linting](engineering/linting.md) — clean design and objective static checks.
3. [Contracts](contracts/README.md) — authoritative interfaces to finish during F0.
4. [Quality baseline](quality/baseline.md) — global acceptance and independent verification; [live certification policy](quality/live-certification.md) — deterministic, real Paseo and Product Owner UAT evidence levels.
5. [Security](security/isolation.md), [SEC-01 spike](security/paseo-sandbox-spike.md) and [2026-10-18 results](security/SEC-01-results-2026-10-18.md).
6. [Observability](observability/telemetry.md) — durable events, traces, metrics, logging.
7. [Factory Office](product/factory-office.md) — F1 UI and event projections.
8. [F0/F1 plan](implementation/f0-f1-plan.md), [F1-S4-A acceptance](implementation/f1-s4a-acceptance.md), [F1-S4-B acceptance](implementation/f1-s4b-acceptance.md), [F1-S4-C acceptance](implementation/f1-s4c-acceptance.md), [F1-S4-C-04 diagnosis](implementation/f1-s4c-04-diagnosis.md), [C-06 server lifecycle](implementation/f1-s4c-06-server-lifecycle.md), [C-07 live certification](implementation/f1-s4c-07-live-certification.md) and [campaign brief](implementation/campaign-prompt.md).
9. [Browser automation](engineering/browser-automation.md) — pinned agent-native Playwright CLI, Factory-managed Implementer capability, verifier boundary and MCP disposition.
10. [Decision records](architecture/decisions/README.md) — decisions, trade-offs, and supersession.

## Authority hierarchy

1. Explicit product-owner approvals and enforced Core authorization/security policies control permitted actions.
2. Accepted ADRs record significant decisions; applicable versioned contracts and policies define testable behavior.
3. [MASTER](architecture/MASTER.md) summarizes the agreed product design; it must remain consistent with ADRs and contracts.
4. `AGENTS.md` is a **short operational entry point**, not a second complete architecture specification.
5. Roadmaps, prompts, generated diagrams, skill text and agent recommendations are advisory unless incorporated through governed changes.

For contradictory normative text, stop claiming compliance, open an Action/ADR and reconcile; do not silently select the permissive interpretation.

## Documentation update rules

- **Every code PR** assesses docs, tests, examples, instrumentation and compatibility impact. If no architecture impact exists, state so in the PR.
- **Material design decisions:** Action → proposed ADR with alternatives and consequences → risk/authority approval → update MASTER/contracts/quality/tests/diagrams together → independent review.
- **Accepted ADRs are append-only in meaning:** supersede them explicitly with a new ADR and mark old records `Superseded` without rewriting their original rationale.
- Security, credential boundaries, merge policies, Quality Gate, evidence acceptance and Core updates have protected approval paths.
- Clearly label statuses `Agreed`, `Proposed`, `Unverified`, `Implemented` or `Verified`. `Implemented` alone does not mean integrated and certified.
- **Early-stage internal breaking changes are expected.** See [ADR-0006](architecture/decisions/ADR-0006-early-development-compatibility.md); remove obsolete shims and update all internal references in one change. Public/stable contracts and persisted data require separately reviewed migration decisions.
- New contributors and agents must start with `AGENTS.md` and this index, then retrieve focused documents; avoid duplicate rules across files.
- CI should check Markdown style, links, formatting, and referenced files as soon as practical. See [lint policy](engineering/linting.md).

## What does not belong in Git

Runtime telemetry, raw session transcripts, large traces, recordings, screenshots, build outputs, credentials and confidential payloads are artifacts governed by storage/retention policy, **not** architecture docs. Commit only small sanitized test fixtures and immutable references/digests where necessary.
