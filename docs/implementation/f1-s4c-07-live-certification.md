# F1-S4-C-07 — Final bounded live Vlog certification

**Status:** `MERGE_READY_FOR_PRODUCT_OWNER_REVIEW`.
The bounded real-Paseo Vlog path passed with one paid Implementer session.
Product Owner UAT is **not** claimed; PR #12 remains unmerged.

- Tested Factory head: `9567bc5c144235bef341524adc44277d377f6167`
- Runtime code commit: `14133cb7ce95cf519147632bc04229046e6f56b6`
- Final evidence: [f1-s4c-07-live-2026-10-21.json](../evidence/f1-s4c-07-live-2026-10-21.json)
- Authorization: one Vlog campaign, at most 2 paid Implementer sessions, at
  most 1 corrective cycle, 1 concurrent Implementer. No Calculator, model
  fallback or automatic merge.
- Sessions used: **1 of 2**; one conserved. Repair cycles used: **0 of 1**.

## 1. Preflight

Preflight passed on the exact tested head:

- clean Factory worktree and clean build of Core, Worker, Verifier, Reviewer
  and Office;
- real Paseo 0.9.1, provider `opencode` available, exact model
  `opencode-go/deepseek-v4.1-flash` present;
- Vlog fixture clean at authorized base
  `f52f877bd390223b9fe7d867355528a070118a3d`;
- Vlog Project registered with `repository-edit,node-toolchain,browser`,
  endpoint `http://127.0.0.1:4173` and declarative
  `browserTestServer: {"command":["npm","start"],"readyPath":"/","startupTimeoutSecs":30}`;
- frozen spec carried the same browser contract, `trusted-local`, exact route,
  allowed paths `src/filter.js,src/app.js`, R0 and a 600s budget;
- campaign bounds were exactly 2 Implementer attempts, 1 repair, 1 parallel;
- Worker-managed server was ready before Paseo session creation, in the
  Attempt workspace; managed server state and port were clean afterward;
- no paid session was spent if preflight failed.

Deterministic preflight on the same runtime: S4-C 51/51, strict S4-A 53/53,
strict S4-B 37/37 locally (three consecutive passes). A separate independent
verification run saw one transient SQLite `database is locked` at S4-B B10,
then passed 2/3; this is recorded as a residual deterministic-lane risk, not a
C-07 live failure.

## 2. Live campaign result

| Field | Value |
| --- | --- |
| Campaign | `bd245745efa4d5ae26d67f77f1933f57` |
| Run / Attempt | `b72a6f10726ff072f69e032110da0dfa` / `575e2a639632b91adb5cf44e3abbc227` |
| Session | `73dd6dd2-2843-4292-9879-fe4469414fcf` |
| Workspace | `wks_69356fdd7fd71c4e` |
| Provider / model / mode | `opencode` / `opencode-go/deepseek-v4.1-flash` / `build` |
| Candidate | `e49f62df32c26e185c840b262435f7a21fea07fd` |
| Changed paths | `src/app.js`, `src/filter.js` |
| Implementer attempts | 1 of 2 |
| Repair cycles | 0 of 1 |
| Browser commands | `open`, `snapshot`, `click`, `snapshot`, `click`, `snapshot` |
| Capability continuation | not used (`capabilityNudges=0`) |
| Browser interaction phase | **initial model turn** |
| Verifier | PASS — real Chromium 156.0.8078.4, consoleErrors=0, failedRequests=0; format/test/functional/negative PASS |
| Reviewer | `APPROVE` |
| Quality Gate | `ACCEPTED` |
| Session cleanup | closed and archived |
| Workspace cleanup | archived/absent from active inventory |
| Managed server cleanup | stopped, port 4173 closed, no registry state |

The Implementer actually edited the authorized source, ran project checks,
opened the managed endpoint, snapshotted and clicked through the UI, and left
bounded invocation evidence. The Worker did not execute browser commands as a
substitute for the model. The managed server was started fresh for this
Attempt, which is why `testServerReused=false`; it was already healthy before
the session began.

## 3. Independent acceptance

- Clean-clone candidate verification passed: exact commit/tree/base, only
  `src/app.js` and `src/filter.js` changed, protected files unchanged;
  `npm run format`, `npm test`, the functional filter assertion and the
  protected `tests/acceptance/browser-check.mjs` all passed with real Chromium
  (`chromium 156.0.8078.4`, `consoleErrors=0`, `failedRequests=0`).
- Supplemental browser probe confirmed five controls, Travel=2, Film=0,
  All=5, empty-state behavior and `aria-pressed` transitions.
- Reconcilement of live evidence passed: one session for the Run label, exact
  provider/model/mode, initial-turn browser commands, candidate identity,
  independent verifier, reviewer and gate, session/workspace/server cleanup.
- Authorization was not exceeded: 1/2 paid sessions, 0/1 repairs, 0
  recoveries, one active Participant maximum.
- GPT-6 Luna read-only review: `APPROVE_WITH_NOTES`; no blocking candidate or
  evidence defect. Notes: raw browser CLI transcripts are not retained by
  design, and the `testServerReused=false` field reflects a fresh per-Attempt
  server rather than a contradiction.

## 4. What this does and does not certify

Certified (candidate-bound):

- the real Vlog Implementer path through Factory Core orchestration, real Paseo
  session, Worker-managed browser test server, model-mediated browser use,
  candidate freeze, independent verification, review and deterministic Quality
  Gate;
- exact tested Factory head `9567bc5` and candidate `e49f62d`.

Not certified:

- Product Owner UAT (`PRODUCT_OWNER_UAT_VERIFIED` not claimed);
- OS isolation or SEC-01 (`TRUSTED_LOCAL` remains broad local access);
- delivery, merge, S5/S6/GOV-01, remote Workers or multi-Provider fallback;
- live corrective repair (no corrective Attempt occurred in this campaign);
- Calculator live execution (not authorized/used in C-07).

## 5. Final PR reconciliation

- The live test was executed on `9567bc5`; subsequent changes are documentation
  and evidence only and do not invalidate the candidate-bound live evidence.
- Prior C-05 failures remain recorded unchanged. C-07 supersedes them as the
  final Vlog live result for the tested runtime.
- Exact-SHA CI on the final PR head is reported in the PR/CI evidence; the
  tested head had 38/38 checks passing before the evidence-only commit.
- PR #12 remains open and unmerged. Merge requires explicit Product Owner
  authority.
