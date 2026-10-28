# Unified intake — author-run Chromium evidence (2026-10-28)

**Status:** author-run deterministic browser evidence. The model is a local fake
OpenAI-compatible server; this is **not live provider inference**, not Compose,
not independent verification and not Product Owner acceptance.

## Candidate identity

| Item | Value |
|---|---|
| Factory implementation commit | `040e6be6379aa9b3ed5fa7a0dd8cbb7209ed5180` |
| Campaign baseline | `campaign/office-ux-baseline-20261028` at `56cfcd9` |
| Integration branch | `campaign/office-ux-integration-20261028` |
| Intake request | `6731e4e8-8b94-4f6e-ae56-dc5712d78671` |
| Campaign | `5b0d4ed4-e33e-4444-8e6f-9de19f5d9359` |
| Exact profile revision | `60dabcfa-c5bc-463a-a380-13bd0805159a` |
| Frozen plan hash | `fe1e8dea59e1d3d076693f0a8897d1c57d3f77ff4f75124f5aa2477663f0fa3f` |
| Model mechanism | `DETERMINISTIC_FAKE_MODEL` (local OpenAI-compatible test server) |
| Autonomous start | `FACTORY_ENABLE_AUTONOMOUS_START=0` |

## Exact command

```bash
CARGO_TARGET_DIR=/home/james/Escritorio/Development/sf-office-ux/target \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
FACTORY_BROWSER_NO_SANDBOX=1 \
node office/e2e-browser/intake-journey.mjs
```

Result (exit 0):

```json
{"ok":true,"request":"6731e4e8-8b94-4f6e-ae56-dc5712d78671","campaign":"5b0d4ed4-e33e-4444-8e6f-9de19f5d9359","profileRevision":"60dabcfa-c5bc-463a-a380-13bd0805159a","planHash":"fe1e8dea59e1d3d076693f0a8897d1c57d3f77ff4f75124f5aa2477663f0fa3f","mode":"DETERMINISTIC_FAKE_MODEL"}
```

## What was exercised

- Real Rust Core with SQLite migrations through `0030`, real device-session
  enrollment and real Chromium.
- A Project was registered, an execution profile created and set as the global
  default, and a deterministic fake model gateway registered as the active
  provider route.
- The browser Home accepted Project, objective, profile and `REVIEW_PLAN` mode.
  Core returned the typed `EXECUTION_POLICY_REQUIRED` 409; the UI displayed the
  exact profile/Project scope, required an acknowledgement, created the bounded
  non-autonomous R0 policy, and retried the same request.
- The browser Request workspace ran the assessment. Core called the model
  gateway, validated the strict assessment/proposal, derived the first
  executable ActionSpec and froze/READY the plan.
- The browser displayed the exact plan hash and profile revision, required a
  rationale and scope acknowledgement, and invoked the operator-approved start.
- Core created the Campaign with `TRUSTED_LOCAL_PROVIDER` authorization while
  `FACTORY_ENABLE_AUTONOMOUS_START=0`. The request projection reached
  `EXECUTING`; the Campaign was `AUTHORIZED`, the profile revision/hash matched
  the frozen intake, and the Action identity matched.
- No browser page errors and no unexplained console errors; the expected
  policy-required 409 and a known pending gate 404 are explicitly filtered.

## Explicit limitations

- **The model is fake.** The assertion that a real semantic model proposes
  checks is not tested. A real authorized provider session remains
  `NOT_TESTED`.
- **No execution completion.** The Campaign was created/authorized; no Worker,
  verifier, semantic reviewer, candidate, Quality Gate acceptance or delivery
  occurred in this journey.
- **Project and provider/profile setup used the Core API**, not the full
  first-use Office setup UI. The Home policy/request/approval path was browser
  driven.
- **No Compose.** Local Compose is blocked on this host by the buildah btrfs
  context-overlay limitation recorded in the integration evidence.
- **Not independent.** No verifier/reviewer or Product Owner UAT accepted this
  candidate.
- Role-specific routing beyond the active default gateway and aggregate
  token/cost accounting remain unimplemented; the UI and lane doc say so.

## What this supports

It supports a truthful claim that the ordinary operator path from Home through
governed policy setup, durable intake, bounded assessment, frozen plan and
explicit operator approval to an authorized Campaign works end to end with a
deterministic model substitute. It does not support a claim of live semantic
quality, completed execution, Quality Gate acceptance or product certification.
