# Wave 3 execution checkpoint — 2026-10-29

**Integration branch:** `campaign/office-ux-wave3-20261028`
**Verified remote integration HEAD:** `f7479c0c5f936d1097426dd1293ef43ea6e72fb6`
**Integration worktree:** `/home/james/Escritorio/Development/sf-wave3/integration` (local checkpoint branch has documentation commits ahead of the remote)
**`main`:** unchanged. No automatic merge or release is authorized.

This checkpoint supersedes earlier phase-3 planning text wherever it conflicts
with the current Owner policy below. Historical candidate evidence remains
provenance and is not current campaign certification.

## Owner policy

- Inference is limited to verified subscription-included entitlements.
- OpenCode Go Extra Usage is OFF; no balance fallback, API-billed credits,
  automatic top-up or billable API-key fallback.
- Codex requires ChatGPT-plan authentication. Other providers must prove the
  included entitlement and no-charge route.
- No positive USD spend cap is required for subscription-only use. Provider
  quota, authorization and technical limits still apply. Unknown billing
  behavior fails closed.
- No live inference until the subscription route, no-extra-billing condition
  and specific test authorization are established.
- No automatic merge to `main`; no provider billing settings may be changed.

## Active implementation lanes

| Lane | Worktree / branch | Base / current state | Owner | Next deliverable |
|---|---|---|---|---|
| Subscription funding V3 | `sf-wave3/subscription-funding` / `factory/subscription-funding/20261029` | `fe1729a70a0c065010a5bd8009dd6c3b03023833`; follow-up repair active | Implemented by Luna subagent `funding_shared_admission`; independent review BLOCK by `review_home_523b288` | Fix missing-row legacy V1 fallthrough and Lead admission bypass with negative tests; re-review exact fix SHA |
| Conversational Home (#62) | `sf-wave3/ux-conversation` / `factory/office-conversation/20261029` | `6482f0c9b68f7fec3933a1b4dbe50e8f6e1e225c`, pushed | Implemented by Luna subagent `home_conversation`; exact-SHA review PASS; draft PR #80 | Fix stale label in browser journey and rerun gates; Owner visual sign-off remains pending |
| Agent Office V3 (#65/#70) | `sf-wave3/isometric-v3` / `factory/office-isometric-v3/20261029` | `5961f0ba8411929ffe6bd55be8c58bfac57c94da` source + `5f2241d307ad7083b34d1cbd704eb649f8bd8a89` evidence; untracked `office/node_modules` retained | Luna subagent `agent_office_v3`; independent exact-SHA review requested | Review candidate and screenshot comparison; update parity documentation before PR; Owner visual acceptance remains pending |
| Access repairs (#36–#38) | `sf-wave3/access-repairs` / `factory/access-repairs/20261029` | `c8bf930bc34e7d7062b13821d0c84f734e394a18` plus dirty repairs | Luna subagent `access_security_repairs` | Complete negative security tests and ordered repairs; require exact-SHA independent review and Owner authorization |
| Provider acceptance / PR #79 | `sf-wave3/provider-acceptance` / `factory/provider-acceptance/20261029` | `32907995c84cbe6f348be17acd61073a18f8b566` plus dirty harness/runbook/tests | Unassigned; preserve | Reconcile with subscription-only policy after funding boundary; no live inference |
| Capability evidence shape | `sf-wave3/capability-evidence-shape` / `factory/capability-evidence-shape/20261029` | `13adff74a8de434fd46c527ea92608b695a98d03`; draft PR #81 | Director | Collect CI and exact-SHA independent review; integrate only after gates pass |

No active Cargo or frontend writer was observed in the funding, Home, access,
or V3 worktrees before assignment. A long-running Vite server has its working
directory in `sf-wave3/isometric-v3`; no file-writing process was observed, and
the server plus untracked `office/node_modules` were left untouched. The current
Codex checkout also has untracked files in `software-factory-native-rust`; they
were left untouched.

## Pull requests and current delivery state

- #77 — Design System and Home visual foundation: integrated into Wave 3.
- #78 — Isometric Agent Office V2: integrated into Wave 3.
- #79 — subscription-only provider UAT preparation: open draft; Owner requires
  removal of the positive USD cap for included subscription usage, explicit
  no-overage verification and fail-closed behavior. Latest remote checks
  reported failures for `architecture-consistency`, `rust`, and `container-dev`;
  `compose-policy` passed. Dirty local changes are not yet committed or reflected
  in those remote check results.
- #80 — conversational Home: open draft against Wave 3 at `6482f0c`. Exact-SHA
  independent review passed. Architecture declarations are now correctly
  base/head-bound, but CI found invalid README evidence references in CAP-050/
  CAP-052 and an old browser journey selector for the textarea's previous
  accessible label. A separate capability-registry cleanup and Home browser
  assertion repair are in progress.
- #81 — capability evidence-reference correction: open draft against Wave 3 at
  `13adff74a8de434fd46c527ea92608b695a98d03`. Canonical container architecture
  governance passed after retrying with the worktree's common Git metadata
  mounted read-only. Remote CI is still running.
- #36 → #37 → #38 — open security stack; do not promote without exact-head
  independent review and required Owner authorization.

Historical deterministic Compose/Chromium acceptance at `40d44ec` and prior
live OpenCode evidence at `1225bae` are candidate-bound history. They do not
authorize new paid inference, certify current changes or imply campaign
completion.

## Completed and pending evidence

- Funding Milestone 1: committed domain, migration 0041, persistence and quota
  pause representation at `5b902a4`; dispatch remains disabled.
- Funding Milestone 2 first fail-closed slice: `fe1729a` adds a typed evaluator
  plus one shared DB guard at candidate-bound Core model-call admission,
  dispatch recheck, Worker turn admission and egress binding. Implementer
  reports fmt, focused funding/Core/Worker tests, `factory-native` library
  suite (498 passed, 2 ignored) and strict Clippy passed. Proposals remain
  disabled; no subscription provider route is enabled. Exact-SHA independent
  independent review **BLOCKED** the slice: missing subscription-policy rows
  still permit Legacy V1 fallback, and `ModelCallRole::Lead` bypasses the
  campaign guard. The implementation agent is adding absent-row and Lead
  regression coverage and correcting admission. Quota pause/re-admission, true
  revocation/revision wiring, and request-level Lead/Intake funding remain
  follow-up work.
- Home conversational slice: typecheck, lint, 270 Office tests, production
  build, parity check and fixture Chromium captures passed on `6482f0c`. The
  production build reports an existing >500 kB chunk warning. Screenshots at
  1440/834/390/320 px are in
  `docs/evidence/office-conversation-home-2026-10-29/`; fixture data was used,
  so real Core/Compose acceptance was initially `NOT_TESTED`; the deterministic
  Compose harness result below now supplies Core/Compose evidence. Independent
  review passed, and Owner visual sign-off is pending.
- Independent Home review of `523b288`: **BLOCK**, high correctness finding:
  overlapping or late `getIntakeRequest` responses could overwrite the selected
  current thread. Commit `6482f0c` adds a request-generation guard, serial
  timeout polling and deferred-response regressions for stale selection and
  overlapping polls. Exact-SHA re-review passed.
- Home fix candidate `6482f0c`: implementer reports focused Home tests 24/24,
  full Office suite 270/270, typecheck, Biome, build and fixture Chromium PASS.
  The >500 kB chunk warning persists. Deterministic Core/Compose evidence is
  recorded separately below; live-provider and Owner visual acceptance remain
  `NOT_TESTED`.
- Home candidate `6482f0c` ran the canonical deterministic Compose flow with
  `FACTORY_E2E_PROJECT=software-factory-home-e2e` and port 8895. It passed with
  fixture candidate `558756c0ce2959ebff97c89dc8579f5075d53bbf`, Gate
  `ACCEPTED`, restart persistence true, `DETERMINISTIC_FIXTURE`, and
  `paid_inference=false`. Isolated containers and volumes were removed. This is
  not live provider evidence or independent acceptance.
- Draft PR #80 is open against `campaign/office-ux-wave3-20261028`; exact-SHA
  independent review passed. A canonical container-dev browser journey failed
  because it still queries the old textarea accessible name. The Home agent is
  updating that assertion and will rerun the real browser journey. The PR is not
  integrated; product-level visual acceptance remains pending.
- The CAP-050/CAP-052 evidence registry check rejects their `README.md` fixture
  summaries in deterministic JSON evidence lists. A separate branch removes
  those invalid entries while retaining the referenced passing JSON records;
  canonical container architecture governance passed. Draft PR #81 is open;
  rust, Compose policy and container-dev checks are still running.
- Agent Office V3 source commit `5961f0b` plus evidence commit `5f2241d` contain
  the renderer refinement, camera
  fit regression. Implementer reports typecheck, Biome (141 files), Vitest
  (266/266), production build and fixture browser checks passed. Captures are
  `/tmp/office-v3-final-{desktop,tablet,mobile}.png`. Director visual inspection
  found responsive framing improved but the scene still materially flatter and
  less dimensional than the approved reference; the candidate is not Owner
  accepted and needs independent review plus further visual evaluation.
- Access repairs remain in progress; no acceptance/review claim made.
- No live provider inference ran in this campaign session.

## Immediate sequence

1. Collect the independent review of funding candidate `fe1729a`; repair any
   findings, rerun invalidated checks, then proceed to quota/restart fencing and
   the separate request-level subscription envelope boundary.
2. Collect the Home browser-selector repair, rerun its containerized Chromium
   journey, and clear the separate CAP-050/CAP-052 registry validation before
   rerunning PR #80 CI. Keep visual Owner acceptance pending.
3. Reconcile PR #79 only after subscription admission and no-overage evidence
   are represented accurately. Keep the live run gated and draft until reviewed.
4. Collect Agent Office V3 visual commits and screenshot comparisons; preserve
   explicit Owner visual sign-off as pending.
5. Collect access repairs in dependency order #36 → #37 → #38. Do not merge the
   stack while high findings or required Owner authorization remain open.
6. Integrate only coherent, reviewed slices into this Wave 3 branch after their
   relevant deterministic gates pass. Do not merge to `main`.

**Independent review:** pending for current session changes.
**Campaign status:** in progress; Wave 3 is not a certified product release.
