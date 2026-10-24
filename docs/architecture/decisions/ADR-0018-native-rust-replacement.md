# ADR-0018 — Native Rust replacement and direct provider control

- **Status:** Accepted by Product Owner for target architecture; implementation is incomplete.
- **Decision date:** 2026-10-23.
- **Base:** `origin/main` at `102d7ba29295554bd61ae71956347a025d219d7b`.
- **Supersedes:** ADR-0005 and the Go/Paseo implementation choices in ADR-0016 and current MASTER. Historical observations and authority requirements remain valid.

## Context

The experimental Go Core/Worker and intermediary agent runtime have useful behavioral contracts but are not a certified production system. The Product Owner authorized a clean native Rust implementation and direct Codex/OpenCode integration. Existing PR #16 is open, Go based, and not merged. Its bounded planning and delegation concepts are reference behavior, not new Rust evidence.

## Decision

Factory Core and Workers use Rust. Office remains React/TypeScript. Core is a modular monolith; Worker is a separate process. Factory owns Projects, authorization, budgets, leases, workspace allocation, candidate identity, evidence, the deterministic Quality Gate, and protected delivery. Workers execute only fenced assignments. Provider adapters use Codex App Server JSON-RPC and OpenCode HTTP/OpenAPI/SSE directly from Rust. Provider switching is explicit. No intermediary coding-agent orchestrator, Node bridge, Go Core/Worker, or predecessor-harness-specific active architecture is part of the completed product.

The internal provider contract must preserve exact provider/model/session/attempt/workspace identities and report unsupported operations and unknown usage/cleanup truthfully. A response to `thread/start`, `turn/start`, or a stream disconnect is not successful completion. Codex success requires terminal `turn/completed` with `status=completed`; OpenCode requires reconciling session and message state after SSE events. Permission requests are governed by Core policy and fail closed without an authorization decision.

Use a new Rust SQLite schema for experimental data. Do not delete irreplaceable persisted user data. Core/Worker transport must be authenticated and independent of shared local paths. Git worktrees isolate revisions but do not provide a security boundary. Untrusted work requires tested OS/container confinement with controlled credential provisioning; absence of isolation blocks dispatch. Trusted local execution remains explicitly non-isolated.

Authorization to execute does not authorize delivery. Risk, budgets, standing delegation, revocation, verification, review, candidate identity, and gate outcomes remain Core-owned. Model plans are typed and bounded to 1–8 work units initially. WorkGraph dependency decisions are deterministic; semantic assessment is model informed under independent validation. No old campaign evidence certifies this replacement.

## Consequences and acceptance

This is a breaking internal replacement. Active Go/Paseo surfaces are removed only after a coherent Rust Core/Worker/Office run path exists, before cutover to main. Historical Git evidence stays intact. The implementation branch may temporarily contain old and new trees, but is not releasable until old active paths are removed. Each capability needs exact candidate evidence; protocol preflight is not a coding turn, and unit tests are not independent acceptance.

Current bootstrap: Rust Core registers canonical Git Projects and Action objectives, persists audit events, and rejects dispatch pending risk/spec/authority; a separate Worker authenticates but receives no assignment. Native provider probes validate Codex initialization/model discovery and OpenCode authenticated health/OpenAPI, first SSE connection, and no-prompt session create/inspect/delete. This does not satisfy provider-backed execution, workspace isolation, campaign, candidate, verifier, reviewer, gate, or Office requirements.
