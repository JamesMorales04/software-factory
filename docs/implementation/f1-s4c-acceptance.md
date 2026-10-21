# F1-S4-C acceptance matrix

**Status:** Implemented and deterministically verified (2026-10-21 UTC; local session date 2026-10-20). The
implementation candidate is the PR head; deterministic evidence is
[`f1-s4c-deterministic-2026-10-21.json`](../evidence/f1-s4c-deterministic-2026-10-21.json).
Real Paseo live certification is **not achieved**; the two authorized initial
sessions and their truthful non-accepting results are recorded in
[`f1-s4c-live-2026-10-21.json`](../evidence/f1-s4c-live-2026-10-21.json).
TRUSTED_LOCAL only; control-plane scoping is not OS isolation and is never
called isolated or SEC-01 certified.

## External Projects

| Fixture | GitHub repository | Local repository | Baseline commit |
| --- | --- | --- | --- |
| Project A — Vlog | `https://github.com/JamesMorales04/factory-fixture-vlog` (private) | `/home/james/Escritorio/Development/factory-fixtures/factory-fixture-vlog` | `f52f877bd390223b9fe7d867355528a070118a3d` |
| Project B — Calculator | `https://github.com/JamesMorales04/factory-fixture-calculator` (private) | `/home/james/Escritorio/Development/factory-fixtures/factory-fixture-calculator` | `451a3a8cf0cae6cb556f87d3f169d9d592339b16` |

Each repository has independent history, a working baseline, README,
deterministic unit tests, syntax checks and a protected browser acceptance
oracle outside the Implementer's frozen allowed scope. No GitHub delivery or
Worker credentials were provisioned. CI materializes disposable local mirrors
from `scripts/fixtures/s4c-*` because private fixture remotes are not available
to GitHub Actions; the external repositories remain the authoritative fixtures.

## Evidence levels

- **DETERMINISTIC** — real Core/Worker/verifier/reviewer/Git/Chromium and a
  deterministic Paseo CLI simulator for the agent decision.
- **REAL BROWSER/TOOL** — real Playwright CLI and real Chromium executed; the
  agent decision may still be simulated.
- **LIVE MODEL** — a genuine paid Paseo language-model session; only the two
  recorded sessions qualify.

## Matrix

| ID | Result | Evidence level | Oracle and evidence |
| --- | --- | --- | --- |
| C1 | PASS | DETERMINISTIC | `POST /projects` registers A/B with canonical identity, base policy, profile, scope, capabilities and revision 1; journey registers A through Office. |
| C2 | PASS | DETERMINISTIC | Invalid path `400`; symlink alias of the same identity `409`; C6 covers mismatched repository/base. Canonical identity is path + root-commit based. |
| C3 | PASS | DETERMINISTIC | A and B coexist with distinct Actions, specs, Campaigns, candidate SHAs and evidence bundles. |
| C4 | PASS | DETERMINISTIC | Project rename advances to config revision 2; frozen spec stays at revision 1, same content/binding hash; accepted evidence unchanged. |
| C5 | PASS | DETERMINISTIC | A candidate is bound to A's path/base `f52f877…`; B to B's path/base `451a3a8…`; Run/Attempt routing matches. |
| C6 | PASS | DETERMINISTIC | Forged Run project/action/spec `400`; A spec pointing at B's repo `403`; cross-project verifier evidence `403`; candidate/evidence identity checks in store/gate. |
| C7 | PASS | DETERMINISTIC | A Run capabilities include `browser`; B Run capabilities are exactly `repository-edit,node-toolchain`; spec requirements derive from profile and grants. |
| C8 | PASS | DETERMINISTIC | A browser Project with no eligible browser Worker terminates `BLOCKED` after a bounded grace with “no online Worker supports required capabilities …”; no fallback. |
| C9 | PASS | REAL BROWSER/TOOL + simulated agent | Factory-generated wrapper invoked pinned Playwright CLI 0.1.22 with real Chromium: `open,snapshot,close`; `browserInvoked=true`, skill digest and CLI version recorded. Live model-driven use: NOT_TESTED. |
| C10 | PASS | REAL BROWSER/TOOL | Core-spawned verifier used a clean read-only checkout and protected oracle: Chromium `156.0.8078.4`, consoleErrors=0, failedRequests=0, isolated profile. |
| C11 | PASS | DETERMINISTIC + real browser | Deliberately broken no-results UI yields browser check `FAIL`; gate not `ACCEPTED`; Campaign terminal `repair_required`. |
| C12 | PASS | DETERMINISTIC | B completed without browser capability; no browser invocation evidence; independent verifier still ran B's protected browser oracle. |
| C13 | PASS | DETERMINISTIC | Both Projects advanced through Core Campaign/Task machinery only; all four role tasks per Campaign succeeded without external orchestration. |
| C14 | PASS | DETERMINISTIC + Go | Concurrent A/B campaigns both accepted with one Worker; store test proves project rotation and capability filtering. |
| C15 | PASS | DETERMINISTIC | Cancel during active Implementer: Campaign `cancelled`, Run `cancelled`, no cross-project cleanup/candidate leakage. |
| C16 | PASS | DETERMINISTIC | Core killed during a live campaign and restarted: Campaign reached `accepted`; Project/spec/capability/campaign identities survived; no duplicate campaign identities. Existing S4-B crash recovery also passes. |
| C17 | PASS | REAL CHROMIUM | Office journey registered Project A, created Action, froze ActionSpec through the form, started the Campaign, observed Accepted candidate/gate, reloaded and rechecked. No custom Python bootstrap or direct spec/campaign API calls from the operator journey. |
| C18 | PASS (scoped) | REGRESSION | `go test -race ./...`, gofmt/vet/golangci-lint, Office typecheck/lint/build, strict S4-A and S4-B deterministic lanes, S1 E2E. S2 E2E is BLOCKED by host Podman overlay/btrfs, not by this change. |
| C19 | PASS | EXTERNAL | Two private GitHub repositories exist with distinct baseline commits and protected acceptance fixtures. |
| C20 | FAIL (live, final C-05) | LIVE MODEL | C-05 candidate `3aa64273…` passed independent browser verification and review, but the Implementer recorded only `--help`; the C-04 continuation was triggered but the model did not complete a successful `open` + interaction. Gate `REPAIR_REQUIRED`; the one authorized C-05 session is exhausted. |
| C21 | PASS (live, scoped) | LIVE MODEL | Calculator accepted candidate `393eab7b…` at implementation `5fa3c1c`, with browser PASS, review APPROVE and gate ACCEPTED. The final `cfa250e` delta is browser-capability-only and does not alter calculator dispatch, capability or verification paths; the remaining calculator session was conserved. |
| C22 | PASS deterministically; FAIL live | REAL BROWSER/TOOL / LIVE | Deterministic D1 recorded successful same-session `open`+`snapshot`. In C-05 the real model received the capability continuation but still produced only `--help`; no live capability claim is made. |
| C23 | PASS deterministic | DETERMINISTIC | Negative cross-project control-plane cases fail closed. TRUSTED_LOCAL host access is not contained; no cross-project confidentiality claim. |
| C24 | PASS deterministic | DETERMINISTIC | Candidate Git objects, spec, binding hash, browser/arithmetic checks, reviewer verdict and gate result correlate. Simulated runs are distinguishable from the two live sessions. |
| C25 | NOT ACHIEVED (full) | LIVE | C-05 tested implementation `65ca3a8`; Vlog gate `REPAIR_REQUIRED` after the capability continuation still yielded only `--help`. Full exact-SHA live certification is not claimed. |
| C26 | READY | DOCUMENTED | Manual UAT instructions and safe cleanup are in the handoff; the Product Owner journey was not silently executed. |

## Independent verification and review

- **Independent clean-checkout verifier:** PASS on exact code candidate
  `cfa250e` (`cfa250ee015ca61656e5c48e1c90583241077002`). Clean worktree,
  gofmt/vet/golangci-lint/race, Office check/build, strict S4-A/S4-B and
  S4-C 44/44 deterministic checks. No source modification.
- **GPT-6 Luna read-only review:** `APPROVE_WITH_NOTES` on `cfa250e`, using
  Codex `--sandbox read-only`. Prior review findings (recovery class at the
  persistence boundary, exact failed-Run binding, date wording) were repaired
  and re-reviewed. Residual note: the browser gate proves successful CLI
  command names, not semantic interaction with a specific target/session; the
  Independent Verifier remains the authoritative browser acceptance authority.
- No independent human verdict is claimed.

## Live session facts

- Provider/model/mode: `opencode` / `opencode-go/deepseek-v4.1-flash` / `build`.
- F1-S4-C-03 authorization: 4 additional sessions per Project, max 3
  corrective cycles. Used: Vlog 4/4, Calculator 3/4 (one conserved).
- No paid failure was injected; the recovery path was forced deterministically.
- C-05 final Vlog trial: Campaign `e2daf085…`, Run `59a0bbf3…`, workspace
  `wks_aa6e1ff4e395ae1a`, session `ae35d976-220d-4bcb-98f1-b75b43991b2b`.
  Candidate `3aa64273…`; independent browser PASS and review APPROVE;
  `capabilityNudges=1` but successful commands remained `--help`; gate
  `REPAIR_REQUIRED`; session/workspace closed and archived.
- Calculator accepted trial: Campaign `210c7794…`, Run `0089ca7c…`,
  workspace `wks_9602f3602a298eca`, session
  `ee106eed-ab33-4183-b101-f550f2706c60`; candidate `393eab7b…`; gate
  `ACCEPTED`; closed and archived.
- Earlier Vlog trial produced an accepted candidate under a weaker
  `--help`-permitted gate; it was superseded after the gate was tightened.
- No `REAL_PASEO_E2E_VERIFIED` claim is made for final implementation
  `65ca3a8`. `PRODUCT_OWNER_UAT_VERIFIED` is not claimed.
- C-05 authorization: exactly 1 Vlog session, used 1/1; 0 repair cycles.
  No further paid session is authorized.
- Vlog sessions used: 4/4. Calculator sessions used: 3/4 (one conserved).
  Every Paseo session/workspace above was observed closed and archived.

## F1-S4-C-03 autonomous recovery matrix

| ID | Result (deterministic) | Evidence |
| --- | --- | --- |
| R1 | PASS | Sequence `empty → good`: first pre-candidate failure authorized recovery; Campaign reached ACCEPTED. |
| R2 | PASS | Recovery row preserves failed Run/Attempt; new Run/lease generation; attempts used 2; old Run still `failed`. |
| R3 | PASS | Core scheduler alone created the corrective Run; no manual participant invocation. |
| R4 | PASS | `browser-broken → good`: gate REPAIR_REQUIRED, new candidate with parent lineage, fresh verifier/reviewer evidence, ACCEPTED. |
| R5 | PASS | `empty → empty` with budget 2: Campaign `failed` with “implementer attempt budget exhausted (2/2)” and one preserved recovery. |
| R6 | PASS | `outside-scope` protected-oracle change: Campaign `blocked` with `POLICY_OR_SECURITY_BLOCKED`; zero recoveries. |
| R7 | PASS | Defective functional oracle produced `repair_required`; a new frozen spec row/content hash with corrected oracle produced ACCEPTED; old verification/decision IDs unchanged. The new spec remains `version=1`; the immutable row/content hash is the revision identity. |
| R8 | PASS | Repaired Campaign ends ACCEPTED with the complete failed-Run/recovery history. |
| R9 | PASS | S1, S4-A, S4-B, S4-C and Go race/lint/typecheck gates pass; exact-candidate evidence unchanged. |
| R10 | FAIL (live completion) | The C-05 live model received the C-04 same-session capability continuation but did not complete browser interaction; no accepted live capability recovery. Forced R1–R8 scenarios remain deterministic PASS. |

Recovery is bounded by `maxImplementerAttempts` (explicit, 1..24) and the
existing repair/fairness/wall-clock limits. The live certification lane uses 4
attempts and 3 corrective cycles per Project within the C-03 authorization.

## F1-S4-C-04 same-session capability completion

| ID | Result | Evidence |
| --- | --- | --- |
| D1 | PASS | Productive `omit-browser` Implementer received one same-session capability continuation; `capabilityNudges=1`; successful `open,snapshot,close`; accepted with 1 attempt and 0 recoveries. |
| D2 | PASS | `browser-fails` attempts were recorded as failed; no successful open; candidate remained `REPAIR_REQUIRED`. |
| Diagnosis | PASS | Actual OpenCode session records show the final agent read the skill, implemented and started the app, then ended without wrapper use; no tool/timeout error. The final campaign was configured 1 attempt/0 repairs by the runner. See [diagnosis](f1-s4c-04-diagnosis.md). |
| Preflight | PASS | Real Core/Worker preflight registered Vlog with `browser`/`trusted-local`, froze a bound spec and created a pending campaign with `maxAttempts=1`, `maxRepairCycles=0`; zero paid sessions. Skill/wrapper reachable; earlier live session used the wrapper successfully; same-session completion verified deterministically. No paid C-04 session spent. |

For C-04, both product browser acceptance (independent verifier) and agent
browser-capability certification (successful model-mediated open + interaction)
remain required. No frozen contract or historical decision was changed.

## F1-S4-C-06 browser test-server lifecycle

| ID | Result | Evidence |
| --- | --- | --- |
| D3 | PASS | Worker-managed `npm start` server in the Attempt workspace; readiness, current-code browser check, stop and port closure. |
| D4 | PASS | Unrelated healthy listener on the authorized port rejected as `INFRASTRUCTURE_BLOCKED`; no Paseo session. |
| D5 | PASS | Immediately failing start command diagnosed within 5s; no Paseo session or model attempt. |
| D6 | PASS | Campaign cancellation stops the server; registry and port clean. |
| D7 | PASS (unit) | Server serves the exact workspace marker; unowned listener is not adopted. |
| D8 | PASS (unit) | Reaper kills stale managed servers; owned reuse preserves PID; spaced command names are probed correctly. |
| Preflight | PASS | Real Vlog app + real wrapper + real Chromium `open/snapshot/close` with zero paid sessions; unauthorized URL rejected. |

The C-05 failure was an interrupted agent-managed background shell command, not
an application exit. See [C-06 diagnosis](f1-s4c-06-server-lifecycle.md).

## Commands

```sh
# deterministic S4-C lane (real Core/Worker/verifier/reviewer/Chromium; stub agent)
bash scripts/e2e-s4c.sh

# bounded real-Paseo lane; Product Owner opt-in, max two sessions
FACTORY_S4C_LIVE=1 S4C_LIVE_PROJECTS="vlog calculator" bash scripts/e2e-s4c-live.sh

# core gates
(cd core && gofmt -l . && go vet ./... && go tool golangci-lint run ./... && go test -race ./...)
# office gates
(cd office && npm run typecheck && npm run lint && npm run build)

# regressions
FACTORY_S4A_STRICT=1 bash scripts/e2e-s4a.sh
FACTORY_S4B_STRICT=1 bash scripts/e2e-s4b.sh
bash scripts/e2e-s1.sh

# GitHub Actions: .github/workflows/s4c.yml (core, office, S4-C e2e and S4-A/B regressions)
```

## Known limitations

- TRUSTED_LOCAL is not isolated. Project scoping rejects unauthorized
  control-plane operations but cannot contain hostile host-level code.
- The deterministic agent simulator is not live-model evidence.
- Vlog live certification is blocked on the exhausted C-03 budget; further
  sessions require Product Owner authorization. The final gate correctly
  rejected the vlog candidate for missing browser interaction rather than
  accepting a prompt claim.
- Recovery classification is deterministic and intentionally conservative;
  ambiguous failures escalate to `HUMAN_REQUIRED` rather than retry.
- S2 E2E remains blocked by the host's unsupported Podman graph driver.
- No S5 delivery, merge, remote Worker, MCP orchestration or SEC-01-ISOLATED
  certification is included.
