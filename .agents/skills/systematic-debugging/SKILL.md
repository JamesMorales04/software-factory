---
name: systematic-debugging
description: Reproduce, isolate, instrument, explain and repair a failing Factory test, Compose journey, provider turn, browser flow or Rust/React defect with a minimal verified change. Use when a test fails intermittently, a late E2E stage fails, logs are noisy, or an agent is tempted to change code before understanding the failure.
---

# Systematic debugging

Use this skill for diagnosis, not for speculative edits. A fix without a
reproduction and a regression check is a guess. This skill cannot grant
authority, budgets, credentials, isolation or acceptance.

## 1. Preserve the failure

Before changing anything:

- record candidate SHA and `git status --short --branch`;
- save the exact command, exit status, stdout/stderr and artifacts;
- record the failing stage, scenario run ID and identities;
- copy logs, DB, trace/video/HAR, console and network before cleanup;
- note the last known good SHA if one exists.

Never delete a failed run directory, database or container before the evidence
is captured.

## 2. Reproduce at the cheapest level

| Symptom | First reproduction |
|---|---|
| Pure logic/state machine | unit/property test with the exact input |
| JSON/API contract | schema/contract test or fixture replay |
| Browser state | component/browser test with the same state |
| Compose stage | `cargo xtask e2e scenario --from <stage> --to <stage> --keep` |
| Provider turn | replay the cassette; do not call the provider |
| Restart/persistence | stop/start the same disposable stack |

If it cannot be reproduced, record it as `NOT_REPRODUCED` with the evidence. Do
not “fix” it.

## 3. Isolate the boundary

Ask which boundary changed:

- input validation or authorization;
- database transaction/lock/WAL;
- task ownership, lease or cancellation;
- provider protocol, usage or terminal evidence;
- environment/image/tool version;
- browser focus/CSS/portal;
- test data, clock, ID, port, temp dir or shared registry;
- deployment/Compose networking or health.

Use bisect or a minimal diff when the cause is not obvious. `git bisect` is
evidence; “it feels like a race” is not.

## 4. Instrument deliberately

Add the smallest observation that distinguishes hypotheses:

- `RUST_LOG` target filter and structured fields, not unbounded debug output;
- `cargo expand` for macro/protocol shape;
- `tokio-console` for task hangs/blocking;
- Playwright trace, console and network for browser state;
- `podman logs`, `podman inspect`, `compose config` for runtime wiring;
- a temporary assertion or counter, removed before final.

Do not log secrets, tokens, cookies, raw prompts or source bodies. Redact before
sharing.

## 5. State a falsifiable hypothesis

Write one sentence:

```text
The stale gate is reused because the candidate identity is matched by attempt ID
while a repair creates a new candidate for the same attempt.
```

Then identify the observation that would prove it false. Change one variable at
a time.

## 6. Fix the cause, not the symptom

- Prefer the smallest change at the owning boundary.
- Add a regression test that fails before the fix and passes after.
- Preserve authorization, idempotency, cancellation and cleanup semantics.
- Do not add retries, sleeps, timeouts, `unwrap`, `any`, `@ts-ignore` or a
  quarantine as the fix.
- If the root cause is outside the allowed scope, stop and report.

## 7. Verify and record

```text
Candidate:   <SHA>
Failure:     <command/stage/identity>
Reproduced:  <n/n, exact inputs>
Root cause:  <one sentence>
Fix:         <files and behavior>
Regression:  <test that fails before and passes after>
Checks:      <focused + full commands>
Artifacts:   <paths>
Residual:    <NOT_TESTED / BLOCKED / flake observations>
```

Re-run the failing check at least three times, and new/changed Playwright specs
20 times. A repair that only passes on retry is not repaired.

## 8. Stop conditions

Stop and report if:

- the failure cannot be reproduced after bounded attempts;
- the only explanation requires a paid provider call without authorization;
- the fix would widen scope, credentials, tools or protected paths;
- logs/artifacts contain secrets and cannot be safely retained;
- the failure is caused by a shared host resource outside your ownership;
- a proposed fix weakens a required check or test.

## 9. References

- [Rust engineering](../rust-engineering/SKILL.md)
- [Testing strategy](../testing-strategy/SKILL.md)
- [Container-native engineering](../container-native-engineering/SKILL.md)
- [Browser automation](../../../docs/engineering/browser-automation.md)
- [Reliability debt](../../../docs/quality/reliability-debt.md)
