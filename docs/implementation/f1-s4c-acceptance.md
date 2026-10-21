# F1-S4-C acceptance matrix

**Status:** Implemented and deterministically verified (2026-10-21). The
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
| C20 | FAIL (live) | LIVE MODEL | The authorized vlog session ran but produced no candidate: the agent wrote `.server.log` in the workspace and candidate validation rejected the out-of-scope path. No retry. |
| C21 | FAIL (live) | LIVE MODEL | The authorized calculator session produced candidate `24e924f0…`; browser check PASS, review APPROVE, but the task-specific functional oracle rejected a valid throw-based division-by-zero contract. Gate `REPAIR_REQUIRED`; no retry. |
| C22 | PASS deterministically; FAIL live | REAL BROWSER/TOOL / LIVE | Deterministic simulated Implementer invoked the CLI for real. The failed vlog live Run did not retain browser-invocation evidence; no live claim. |
| C23 | PASS deterministic | DETERMINISTIC | Negative cross-project control-plane cases fail closed. TRUSTED_LOCAL host access is not contained; no cross-project confidentiality claim. |
| C24 | PASS deterministic | DETERMINISTIC | Candidate Git objects, spec, binding hash, browser/arithmetic checks, reviewer verdict and gate result correlate. Simulated runs are distinguishable from the two live sessions. |
| C25 | NOT ACHIEVED | LIVE | Live evidence applies to implementation `ad716db`; subsequent runtime repairs (runtime-artifact staging, oracle protection scope, legacy capability semantics) invalidate it for the final PR head. A new authorized live run is required. |
| C26 | READY | DOCUMENTED | Manual UAT instructions and safe cleanup are in the handoff; the Product Owner journey was not silently executed. |

## Live session facts

- Provider/model/mode: `opencode` / `opencode-go/deepseek-v4.1-flash` / `build`.
- Authorized and used: 2 initial Implementer sessions; no retries.
- Vlog: Campaign `ff4a4f00…`, Run `0c5898f8…`, Paseo workspace
  `wks_b8895de4333279cd`, session `1229b3fa-d8b5-4a55-93c4-f76b2515f5a4`.
  Result: failed before candidate; `.server.log` outside allowed scope.
- Calculator: Campaign `e01de48e…`, Run `2ce96c43…`, Paseo workspace
  `wks_0d0ecb5af58bb812`, session `f38d1ec7-01b3-4b79-b4c6-6946e35bd8f8`.
  Candidate `24e924f0…`; verifier browser PASS but functional FAIL; gate
  `REPAIR_REQUIRED`.
- No `REAL_PASEO_E2E_VERIFIED` or full `MERGE-READY` claim is made.

## F1-S4-C-03 autonomous recovery matrix

| ID | Result (deterministic) | Evidence |
| --- | --- | --- |
| R1 | PASS | Sequence `empty → good`: first pre-candidate failure authorized recovery; Campaign reached ACCEPTED. |
| R2 | PASS | Recovery row preserves failed Run/Attempt; new Run/lease generation; attempts used 2; old Run still `failed`. |
| R3 | PASS | Core scheduler alone created the corrective Run; no manual participant invocation. |
| R4 | PASS | `browser-broken → good`: gate REPAIR_REQUIRED, new candidate with parent lineage, fresh verifier/reviewer evidence, ACCEPTED. |
| R5 | PASS | `empty → empty` with budget 2: Campaign `failed` with “implementer attempt budget exhausted (2/2)” and one preserved recovery. |
| R6 | PASS | `outside-scope` protected-oracle change: Campaign `blocked` with `POLICY_OR_SECURITY_BLOCKED`; zero recoveries. |
| R7 | PASS | Defective functional oracle produced `repair_required`; a new frozen spec with corrected oracle produced ACCEPTED; old verification/decision IDs unchanged. |
| R8 | PASS | Repaired Campaign ends ACCEPTED with the complete failed-Run/recovery history. |
| R9 | PASS | S1, S4-A, S4-B, S4-C and Go race/lint/typecheck gates pass; exact-candidate evidence unchanged. |
| R10 | LIVE PENDING | Real Paseo recovery is attempted only if a recoverable failure occurs naturally; no artificial paid failure is injected. Forced failures are deterministic. |

Recovery is bounded by `maxImplementerAttempts` (explicit, 1..24) and the
existing repair/fairness/wall-clock limits. The live certification lane uses 4
attempts and 3 corrective cycles per Project within the C-03 authorization.

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
- Live certification must be rerun on the final runtime head; C-03 authorizes
  up to 4 additional Implementer sessions per Project, max 3 corrective
  cycles, with no manual role invocation.
- Recovery classification is deterministic and intentionally conservative;
  ambiguous failures escalate to `HUMAN_REQUIRED` rather than retry.
- S2 E2E remains blocked by the host's unsupported Podman graph driver.
- No S5 delivery, merge, remote Worker, MCP orchestration or SEC-01-ISOLATED
  certification is included.
