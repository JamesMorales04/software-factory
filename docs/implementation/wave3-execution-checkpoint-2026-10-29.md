# Wave 3 execution checkpoint — 2026-10-29

**Integration branch:** `campaign/office-ux-wave3-20261028`
**Verified integration HEAD:** `f7479c0c5f936d1097426dd1293ef43ea6e72fb6`
**Integration worktree:** `/home/james/Escritorio/Development/sf-wave3/integration` (clean at checkpoint)
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
| Subscription funding V3 | `sf-wave3/subscription-funding` / `factory/subscription-funding/20261029` | `fe1729a70a0c065010a5bd8009dd6c3b03023833`, clean | Implemented by Luna subagent `funding_shared_admission`; exact-SHA independent review pending in `sf-wave3/funding-review` | Review; then continue with quota recovery/re-admission and request-level Home funding boundary; no paid fallback |
| Conversational Home (#62) | `sf-wave3/ux-conversation` / `factory/office-conversation/20261029` | `6482f0c9b68f7fec3933a1b4dbe50e8f6e1e225c`, pushed | Implemented by Luna subagent `home_conversation`; exact-SHA independent review PASS from `review_home_523b288`; draft PR #80 | Await CI and real Core/Compose acceptance; Owner visual sign-off remains pending |
| Agent Office V3 (#65/#70) | `sf-wave3/isometric-v3` / `factory/office-isometric-v3/20261029` | `f7479c0c5f936d1097426dd1293ef43ea6e72fb6` plus dirty SVG/DOM renderer work and untracked `office/node_modules` | Unassigned; preserve | Reassess after Home lane frees frontend capacity; do not discard existing changes |
| Access repairs (#36–#38) | `sf-wave3/access-repairs` / `factory/access-repairs/20261029` | `c8bf930bc34e7d7062b13821d0c84f734e394a18` plus dirty repairs | Unassigned; preserve | Resume only with safe exclusive ownership; require negative tests and exact-SHA independent security review |
| Provider acceptance / PR #79 | `sf-wave3/provider-acceptance` / `factory/provider-acceptance/20261029` | `32907995c84cbe6f348be17acd61073a18f8b566` plus dirty harness/runbook/tests | Unassigned; preserve | Reconcile with subscription-only policy after funding boundary; no live inference |

No active Cargo or frontend writer was observed in the funding, Home, or V3
worktrees before assignment. A separate Vite server is running from
`sf-wave3/isometric-v2`; it is unrelated and was left untouched. The current
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
  review is pending. Quota pause/re-admission, true revocation/revision wiring,
  and request-level Lead/Intake funding remain follow-up work.
- Home conversational slice: typecheck, lint, 268 Office tests, production
  build, parity check and fixture Chromium captures passed on `523b288`. The
  production build reports an existing >500 kB chunk warning. Screenshots at
  1440/834/390/320 px are in
  `docs/evidence/office-conversation-home-2026-10-29/`; fixture data was used,
  so real Core/Compose acceptance remains `NOT_TESTED`. Independent exact-SHA
  review and Owner visual sign-off are pending.
- Independent Home review of `523b288`: **BLOCK**, high correctness finding:
  overlapping or late `getIntakeRequest` responses could overwrite the selected
  current thread. Commit `6482f0c` adds a request-generation guard, serial
  timeout polling and deferred-response regressions for stale selection and
  overlapping polls. Re-review is pending; do not integrate until disposition.
- Home fix candidate `6482f0c`: implementer reports focused Home tests 24/24,
  full Office suite 270/270, typecheck, Biome, build and fixture Chromium PASS.
  The >500 kB chunk warning persists; real Core/Compose remains `NOT_TESTED`.
- Draft PR #80 is open against `campaign/office-ux-wave3-20261028`; exact-SHA
  independent review passed. The PR is not integrated and no acceptance is
  claimed until CI and real Core/Compose browser evidence are complete.
- Office V3 and access repairs: dirty work preserved; no acceptance/review
  claim made.
- No live provider inference ran in this campaign session.

## Immediate sequence

1. Collect the independent review of funding candidate `fe1729a`; repair any
   findings, rerun invalidated checks, then proceed to quota/restart fencing and
   the separate request-level subscription envelope boundary.
2. Monitor PR #80 CI; obtain real Core/Compose browser evidence with no live
   provider inference, then integrate only if gates pass. Keep visual Owner
   acceptance pending.
3. Reconcile PR #79 only after subscription admission and no-overage evidence
   are represented accurately. Keep the live run gated and draft until reviewed.
4. Reassign frontend capacity to preserved Agent Office V3 work; retain explicit
   Owner visual sign-off as pending.
5. Resume access repair/review in dependency order #36 → #37 → #38. Do not merge
   the stack while high findings or required Owner authorization remain open.
6. Integrate only coherent, reviewed slices into this Wave 3 branch after their
   relevant deterministic gates pass. Do not merge to `main`.

**Independent review:** pending for current session changes.
**Campaign status:** in progress; Wave 3 is not a certified product release.
