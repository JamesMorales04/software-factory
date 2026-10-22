# ADR-0016 — Dual-mode autonomous engineering

**Status:** Accepted for product objectives and authority boundaries; implementation contracts for future milestones remain proposed. **Date:** 2026-10-21 UTC.

## Context

The Product Owner ratified two operating modes: autonomous product development from an authorized objective, and ongoing autonomous maintenance and evolution after delivery. AEH showed that elaborate orchestration and deterministic tests can coexist with poor real-world completion. The existing Factory already has one Core-owned Project/Action/Campaign path and candidate-bound quality pipeline. Splitting maintenance into a second controller would duplicate authority and evidence rules.

## Decision

Both modes share one authoritative Go Core, Project Registry, Action/Campaign execution path, Worker/Paseo integration, authorization and resource governance, quality/certification, delivery policy and durable evidence ledger. A maintenance signal becomes a Core-owned Incident and, when authorized, an Action in that same pipeline. Deterministic scheduling and event processing persist across restarts without a standing model session.

The Product Owner owns objectives, risk appetite and delegation limits. Standing delegation must name project, permitted change classes, budget, expiry, revocation and escalation conditions. Core enforces policy, leases, fencing, state transitions, budgets, security, quality and external effects. A Lead may interpret intent and coordinate a bounded plan; a Supervisor may track execution; specialist roles may explore, plan, implement or review according to need. Model output is advisory or scoped work product and cannot grant permissions, approve its own evidence or alter protected policy.

Planning must freeze an ActionSpec and a validated dependency plan before execution. Independent verification and semantic review remain separate from the author. The deterministic Quality Gate alone decides candidate acceptance. The Core Delivery Engine alone may create governed external delivery effects under separately approved credentials and policy. Acceptance, merge, deployment and controller promotion are distinct transitions.

The first maintenance pilot targets an external fixture project. Factory self-development later uses stable controller A and isolated candidate B; B cannot control its own authorization, verification or promotion. Rollback and recovery are required before promotion can be claimed. TRUSTED_LOCAL remains broad host access. SEC-01 isolation and credential confinement remain paused and uncertified; this ADR grants no new security mode, delivery permission, merge credential or paid session.

## Consequences and open contracts

- F1-S4-D must define standing delegation, plan validation, escalation and model-route contracts before implementation. F1-S4-E adds minimal task-specific specialist composition and bounded supervision.
- Maintenance monitoring, incident triage and portfolio allocation remain future deterministic Core services, not a second orchestration engine or always-running agent team.
- F1-S5 delivery, F1-S6 controller promotion and any security boundary change require their own protected Product Owner decisions and acceptance evidence.
- [The capability registry](../../product/capabilities.yaml) separates approved objectives from proposed implementations; [the generated roadmap](../../roadmap/ROADMAP.md) is planning, not execution authority.

This decision preserves ADR-0002, ADR-0004, ADR-0009 through ADR-0015. If a later contract conflicts with one of them, a new ADR must explicitly supersede it.
