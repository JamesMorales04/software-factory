# ADR-0014 — Same-session capability completion

**Status:** Accepted (F1-S4-C-04 implementation; final candidate evidence is recorded in PR #12). **Date:** 2026-10-21 UTC.

## Context

A real Factory-managed Implementer produced a valid Vlog candidate and passed
independent Chromium verification, but ended its turn after code/tests/server
startup without invoking the required Playwright capability. The skill and
wrapper were present, readable and had worked in an earlier live session. The
Quality Gate correctly refused acceptance, but the Factory had no mechanism to
request the missing interaction before closing a productive session.

## Decision

After a productive turn and evidence collection, if the frozen Run requires
`browser` and no successful Factory-managed `open` + observation/interaction
has been recorded, the Worker sends **one** bounded continuation to the same
Paseo session, re-observes the session/evidence, then proceeds with the normal
candidate and gate path.

- The continuation uses the existing session, lease, generation and Run; it is
  not a new paid session or a new Attempt.
- It is bounded by the remaining lease budget and the existing verify reserve.
- It does not grant new scope, credentials, endpoints or oracle authority.
- Failed commands, `--help`, a prompt claim or an orchestrator-run browser check
  never count as Implementer capability evidence.
- If interaction remains missing, the gate rejects and existing repair/recovery
  paths apply. If cleanup is unconfirmed or the session is uncertain, the
  attempt is orphaned; there is no silent retry.
- The total lifecycle continuation remains bounded: at most one empty-diff
  nudge and at most one capability nudge per Attempt.

The Quality Gate continues to require successful command outcomes for registered
browser specs. Product browser acceptance remains the Independent Verifier's
protected oracle; agent capability certification remains a separate observation.

## Consequences

- The Worker result records `capabilityNudges` and successful browser command
  evidence; budget/session identity is unchanged.
- Deterministic D1/D2 scenarios prove same-session completion and failed-command
  rejection.
- The live runner must still enforce its explicit attempt/repair budget; this
  ADR does not renew or expand paid authorization.
- No frozen contract or historical decision is modified.

## Alternatives rejected

- Accepting the candidate on independent browser PASS alone: hides that the
  Implementer did not use its granted capability.
- Weakening or removing the capability gate: turns a capability failure into a
  false success.
- Starting a new session automatically: spends paid budget and loses the
  productive session context.
- Unbounded nudging: violates bounded resource/authority policy.
