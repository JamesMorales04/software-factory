---
- **Amended by:** ADR-0020 (editorial predecessor-independence cleanup; decisions unchanged)
id: ADR-0001
status: Accepted
date: 2026-10-18
owner: product-owner
---
# ADR-0001: Versioned architecture and decisions in Git

## Context
A predecessor engineering harness exposed the cost of duplicated architectural authority, stale descriptions and features declared ready without real verification. Coding agents need a concise starting point and an auditable design.

## Alternatives
1. Conversations/external wiki as primary authority: not reliably bound to commits and PRs.
2. Documentation stored alongside the product: reviewable, versioned and linked to code and tests.

## Decision
Adopt option 2. Maintain `docs/architecture/MASTER.md`, ADRs, contracts and engineering standards in this repository, with root `AGENTS.md` as a compact entry point. Update architecture-related docs together with implementation changes.

## Consequences
Easy audit and agent onboarding; documentation must be maintained and checked. It cannot supersede Core authorization or replace runtime evidence.

## Verification
Every materially architectural PR carries its Action/ADR context, updated references and justified scope. Do not mark any capability `Verified` without a real acceptance trace.
