# ADR-0015 — Worker-managed browser test server

**Status:** Accepted (F1-S4-C-06 implementation; final candidate evidence is recorded in PR #12). **Date:** 2026-10-21 UTC.

## Context

A real Implementer's background `npm start` command was interrupted by the
agent shell tool (`Tool execution aborted`, no exit code/stderr). The app never
started, the model ended without using Playwright, and the capability gate
correctly rejected the candidate. Agent-managed background process lifecycle
is not reliable enough to be the prerequisite for browser capability
certification.

## Decision

A Project granting `browser` may declare a bounded `browserTestServer` contract
(argv, readiness path, startup timeout). The Worker starts it in the exact
Attempt workspace, waits for the authorized loopback endpoint, owns the process
group, records ownership, refuses to adopt unrelated listeners, and stops or
reaps it. Startup failure is reported as infrastructure, before any Paseo
session.

The managed server is infrastructure evidence only; it never populates
Implementer browser-invocation evidence. The model must still perform
successful `open` + observation/interaction; the Independent Verifier still
owns product browser acceptance.

## Consequences

- Agent-managed server startup is removed from the critical path for projects
  that adopt the contract.
- New Project/Spec columns and binding-hash coverage; additive migration.
- Start/readiness/port-ownership failures do not consume paid model sessions.
- The model prompt states the server is already running and must not be
  restarted.
- The contract is control-plane, not OS network isolation; a TRUSTED_LOCAL
  process can still misuse the host.
