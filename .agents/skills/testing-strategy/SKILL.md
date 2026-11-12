---
name: testing-strategy
description: Design and review the cheapest trustworthy verification for a Factory change, including deterministic unit/property/contract/component/Compose layers, provider and GitHub fixtures, flake triage and candidate-bound evidence. Use when planning tests, judging coverage, investigating a flaky or late E2E failure, or deciding whether a check belongs in the PR, nightly or authorized UAT lane.
---

# Testing strategy

Use this skill when choosing what to test, where a check belongs, or whether a
green result actually proves the claimed behavior. It is a development aid, not
an authority: it cannot grant budgets, merge rights, isolation, acceptance or a
Quality Gate outcome.

## 1. Start with the claim

Write the exact observable claim before choosing a tool:

- state transition or persisted row;
- API/CLI result and status;
- emitted event or projection;
- external side effect and its receipt;
- failure classification, recovery and cleanup;
- absence of an effect (negative case).

A test that cannot fail for a plausible defect is documentation, not an oracle.

## 2. Place the check on the determinism ladder

| Level | Use for | Typical tools | Runs |
|---|---|---|---|
| L0 static | types, lints, policy, supply chain | fmt, Clippy, `tsc`, Biome, typed ESLint, `cargo-deny`, Semgrep | every commit |
| L1 unit/property | pure logic, parsers, budgets, state machines | `cargo-nextest`, `proptest`, `insta`, Vitest | every PR |
| L2 contract | schemas and boundaries | OpenAPI/projection schemas, Worker RPC, provider cassettes, fake GitHub | every PR |
| L3 component | real DOM, CSS, focus, portals | Vitest browser mode, Playwright, axe | changed UI |
| L4 system | real Compose with fixture provider | `cargo xtask e2e scenario` | PR smoke/nightly |
| L5 real E2E | real model/GitHub/Nix | authorized UAT | nightly/UAT |

Push each check to the cheapest level that can actually catch the defect. Do not
use L5 to discover a bug that an L2 contract or L3 component test can prove.

## 3. Make non-deterministic boundaries explicit

- **Model/provider:** fixture for PRs, recorded cassette for trajectories, real
  provider only under a fresh authorization envelope.
- **GitHub:** fake for contracts, sandbox repository for nightly, real target
  only for UAT.
- **Environment:** fixture bundle for PRs, real Nix/devenv for nightly/UAT.
- **Clock/IDs:** inject fixed time and seeded identities; never let `now()` or a
  random UUID enter a snapshot or provider request.
- **Network/storage:** loopback, disposable SQLite, unique volumes and ports.

A cassette asserts the trajectory: tool calls, order, arguments, files, checks,
side effects and final status. It does not assert exact model prose. Re-record
only for an intended behavior change and review the cassette diff.

## 4. Determinism rules

1. New or changed Playwright specs run 20 times
   (`--repeat-each=20`). One failure is a flake.
2. CI uses `--fail-on-flaky-tests` and does not retry to green.
3. No `waitForTimeout` or sleeps; use web-first assertions.
4. Namespace Compose project, ports, volumes, temp dirs, Git fixture and browser
   profile by run ID. Cleanup owns only its own namespace.
5. A hang is a failure. Every external wait has a deadline.
6. A missing tool is `BLOCKED` or `NOT_TESTED`, never `PASS`.
7. Assert typed JSON fields, not substrings of a pretty-printed projection.
8. Capture artifacts on the first failure: logs, DB, trace/video/HAR, console,
   network, provider cassette and exact candidate SHA.

## 5. Test design checklist

- Happy path, boundary, invalid input, unauthorized, stale/duplicate, timeout,
  cancellation, partial failure, recovery and cleanup.
- One test per behavior; no assertion counted twice as two checks.
- Table-driven cases where the table is the specification, not a dumping ground.
- Property tests for algebraic laws: idempotency, ordering, round-trip,
  monotonicity, budget conservation.
- Snapshot tests for stable projections; redact volatile IDs and timestamps.
- Mutation sampling for critical policy/state modules; coverage alone is not
  adequacy.
- Test names state the behavior and the expected failure mode.

## 6. Flake triage

Classify before changing code:

| Class | Evidence | Action |
|---|---|---|
| Product race | fails under load/another checkout | fix ownership/synchronization |
| Test isolation | shared port/temp/registry/DB | namespace and cleanup ownership |
| Timing | passes with retry/sleep | replace with condition-based wait |
| Environment | image/tool/version drift | pin, doctor, pre-pull |
| Oracle | passes while behavior is wrong | strengthen assertion or add negative |
| External | provider/GitHub/network | fixture/cassette; do not retry blindly |

Do not quarantine without an owner, reason, issue and expiry. A quarantined test
is not evidence.

## 7. Evidence template

```text
Candidate:  <SHA>
Claim:      <behavior>
Level:      L0/L1/L2/L3/L4/L5
Oracle:     <why a defect fails this check>
Commands:   <exact commands and versions>
Fixtures:   <provider/GitHub/time/ID/environment>
Negatives:  <invalid/unauthorized/stale/timeout/cleanup>
Flake:      <repeat count and result>
Artifacts:  <paths>
Result:     PASS / FAIL / BLOCKED / NOT_TESTED
Limitations:<what this does not prove>
```

## 8. Stop conditions

Stop and report if:

- a check would require paid inference without a fresh authorization;
- the only oracle is a model judgment or a screenshot;
- a test must be weakened, skipped or retried to pass;
- a failure cannot be reproduced and would be hidden by a quarantine;
- the environment cannot provide the required tool or isolation;
- evidence would not be bound to the exact candidate.

## 9. References

- [Quality baseline](../../../docs/quality/baseline.md)
- [Live certification](../../../docs/quality/live-certification.md)
- [Reliability debt](../../../docs/quality/reliability-debt.md)
- [Browser automation](../../../docs/engineering/browser-automation.md)
- [UI change acceptance](../ui-change-acceptance/SKILL.md)
- [Factory real E2E](../factory-real-e2e/SKILL.md)
- [Rust testing reference](../rust-engineering/references/testing.md)
