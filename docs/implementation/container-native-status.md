# Container-native iteration status (2026-10-26)

**Status authority:** this document supplements the
[native Rust closure ledger](native-rust-closure.md) for the container-native
iteration. It does not rewrite historical acceptance or certify a paid UAT.

## Implemented and author-run

| Area | State | Evidence |
|---|---|---|
| Phase-zero nested engine | PASS (measured) | [phase-zero record](../evidence/container-native-phase0-2026-10-26.md) |
| Compose one-command stack | PASS (fresh volumes) | deterministic Compose E2E script |
| First-run Office enrollment | PASS | Chromium onboarding self-test |
| Provider settings, encryption, masking, audit, revocation | PASS (dummy credential; no inference) | Chromium provider self-test + Core audit |
| Project Git import and Office configuration | PASS | deterministic Compose E2E + browser |
| Action checks and delegation UI | PASS | deterministic Compose E2E + browser |
| Provider session in nested container (fixture) | PASS | E2E candidate/gate |
| Automatic verifier + deterministic reviewer + gate | PASS | candidate-bound receipts and `ACCEPTED` |
| Restart persistence | PASS | E2E down/up + Project/Campaign/gate |
| Parallel real attempts | PASS (two Workers, two nested containers) | candidate/attempt and gate records |
| Real Chromium Office | PASS (self-test) | zero console errors/404 |
| Existing Rust tests, fmt, Clippy | PASS (author-run) | local exact commands |

## Independent review and remediation (2026-10-26)

An independent same-model security/architecture review of the first candidate
returned **FINDINGS**. The following were remediated. Tests cover the rootless-map parser, TurnBinder
origin, semantic toggle for model calls and receipts, Compose policy negatives,
master-key permissions/symlink rejection and turn-release binding. Recovery,
reaper/start-failure and provider-profile endpoint behavior are covered by code
inspection and SQL parsing rather than full process tests:

- rootful outer namespace accepted by the runtime: entrypoint/broker now reject
  an initial user namespace;
- container OpenCode TurnBinder rejected `http://core:8765`: container-mode
  service-origin validation plus a regression test;
- semantic-review policy was advisory: Core now rejects semantic model calls and
  MODEL reviewer receipts while the policy is disabled, with a route test;
- provider profile disclosure was not bound to an attempt: Core now requires a
  live attempt/generation lease and the exact authorized route;
- unresolved provider sessions were not worker-scoped: recovery listing and
  directory validation are scoped; container cleanup rejects paths outside the
  configured root;
- provider containers had no lifetime reaper: the broker removes them at expiry
  and attempts removal on start failure; `FAILED` sessions with unconfirmed
  cleanup are rejected by Core and left for startup recovery;
- turn release did not bind session/worker/generation: the release query and
  Worker request now do;
- master-key permissions were not enforced: 0600/0640 and non-symlink regular
  file required;
- static Compose policy was weak: per-service posture, config-volume isolation,
  host-bind/device/sysctl rejection and negative tests were added;
- the E2E harness could target the production project: it now refuses a project
  name without `e2e`.

Residual findings are not claimed fixed:

- provider session containers share one outer volume and UID (same-UID
  cross-session access possible after provider compromise);
- total engine-volume quota is not enforced (64 MiB per-file `fsize` and
  provider lifetime reaper only);
- broker mount-path TOCTOU after canonicalization;
- verifier and reviewer receipts are bearer-token self-attestations, not signed
  execution proofs; verifier has read-write project-volume access for its
  worktree;
- the real Compose E2E is a manual workflow, not a required PR check; PR CI
  covers static Compose policy, Rust tests and browser regressions;
- live OpenCode container inference **PASSED** the owner-authorized UAT below;
  live Codex, semantic live calls, SEC-01, Windows and human acceptance remain
  open.

## Baseline hardening (2026-10-27)

A second independent read-only review of PR #28 returned **BLOCKED** on four
concrete paths. The branch now includes and locally verifies:

- a verifier `FAIL` now records `REPAIR_REQUIRED` immediately instead of
  waiting for a reviewer receipt that the queue never dispatches;
- strict egress request-digest matching is derived from the resolved provider
  adapter (fixture only), not the deployment environment, and the base Compose
  stack defaults to the live OpenCode route while the deterministic fixture is
  selected only by the explicit E2E override;
- semantic review, when enabled with a configured default provider, requires
  MODEL reviewer evidence; a deterministic reviewer `PASS` is rejected at
  ingestion and again at gate acceptance, including a receipt recorded before
  the policy change;
- provider credential disclosure now requires a `LEASED` attempt, an
  `AUTHORIZED` Campaign, the exact lease generation and an unexpired
  authorization deadline;
- provider-turn input budget is reserved across concurrent reservations;
- `init` and `provider-bundle` are read-only, capability-reduced root one-shots
  with static policy coverage, and the broker-token Debug leak is removed.

At hardening candidate `d9de876f5533827a5a44b07f00c676ac24d7516c`, the
deterministic Compose E2E still reaches an `ACCEPTED` candidate
(`b9b22a75-082f-4ca8-bc71-b92a7bbdd697`) and preserves state across restart.
The owner-authorized live OpenCode container UAT then passed at PR head
`1225baeb123f7a652bb3a927f53562e28c573e26`; see below. Evidence:
[container-native hardening](evidence/container-native-hardening-2026-10-27.json).

## Live OpenCode container UAT (2026-10-27)

The Product Owner approved the exact envelope in
[the authorization request](container-native-paid-uat-authorization-request-2026-10-27.md)
and entered the OpenCode Go API key directly in the disposable Office. The key
was revoked immediately after the run and never printed, committed or placed in
browser storage.

| Item | Observation |
|---|---|
| Candidate | `1225baeb123f7a652bb3a927f53562e28c573e26` |
| Project / Campaign | `db6cf81b…` / `0c524008…` |
| Run / Attempt | `40fc5c15…` / `37d3b08c…` |
| Candidate | `e8bd7464…`, commit `52a583a4fd82e21db53e7c99688a409bdbb07eb8`, tree `b42a3ac7…` |
| Diff | `src/provider.txt | 1 +`; content `live uat candidate` |
| Provider session | `ses_e84442dd3ffeIug0t00CTODIqi`, `opencode-go/deepseek-v4.1-flash`, mode `build` |
| Usage | 7,817 input / 234 output / 16,693 total reported tokens; 1,914 micro configured-rate cost; usage known |
| Quality | verifier `PASS` EXECUTED, reviewer `PASS` DETERMINISTIC, gate `ACCEPTED` |
| Cleanup | `session_delete_verified_absent`; no unresolved sessions |
| Budget | one session of two authorized; one attempt; no fallback or retry |

The Campaign authorization label remains `AUTHORIZED`; the Run (`ACCEPTED`) and
Quality Gate are the authoritative execution and quality states. This is a
single real-provider container journey, not independent security certification,
SEC-01, deployment, Windows or Product Owner personal UAT. Evidence:
[container-native live UAT](evidence/container-native-live-uat-2026-10-27.json)
and the
[certification manifest](evidence/container-native-live-uat-certification-manifest.json).

## Real-provider acceptance preparation (2026-10-29)

A bounded OpenCode Go UAT for the current candidate is **prepared but not run**:
[real-provider UAT runbook](real-provider-uat-runbook-2026-10-29.md). It uses
the real Compose Core/Worker/verifier/reviewer path, stops at the exact frozen
plan for Owner approval, and requires an explicit accounting mode, Extra Usage
state and spend cap. The dry-run contract test uses a local fake Core and makes
no inference.

Accounting finding: Core does not persist the raw OpenCode
`opencode.allowance`, `opencode.billing_source` or API `usage.cost`. OpenCode's
local message `cost` is a list-rate estimate (observed nonzero for Go messages),
not a balance charge. The live evidence must therefore mark Go allowance
consumption **UNKNOWN** and the Owner must read the OpenCode Console. No paid
inference is authorized or performed by this preparation.

## Not tested / blocked

- OpenCode live inference inside the provider container: **PASS (owner-authorized,
  author-run)** for the one candidate above. The deterministic fixture path
  remains the PR CI acceptance path.
- Codex API-key live inference: **NOT_TESTED**. Native ChatGPT OAuth:
  **BLOCKED**. Containerized Codex stdio transport: **NOT_IMPLEMENTED**.
- Semantic Reviewer live model call: **NOT_TESTED** (policy and Core budgets
  were exercised with a dummy provider only).
- Multi-unit WorkGraph parallel execution with a real provider and integrated
  fan-in candidate: **NOT_TESTED**. Parallel evidence uses two independently
  authorized Campaign attempts and two Workers.
- SEC-01 ISOLATED / credential confinement: **BLOCKED**. Rootless containers
  and the dedicated engine are `TRUSTED_LOCAL`.
- Windows/Podman Machine, SELinux/AppArmor hosts, multi-arch builds, crash
  injection during an active provider session, browser multi-unit evidence,
  independent security review and human acceptance: **NOT_TESTED / OPEN**.
- `factory start` host lifecycle: retained for development but not the
  container-native install path; not re-certified by this iteration.

## Merge gates

Exact-head CI, an independent security/architecture review of the hardening
candidate, the authorized paid UAT and Product Owner approval remain required.
The candidate-bound deterministic and live UAT passes above are not a merge
authorization. Two independent reviews found and closed the hardening gaps; the
focused re-review of the semantic-review gate fix returned **MERGE_READY**.
Product Owner merge approval remains pending.
