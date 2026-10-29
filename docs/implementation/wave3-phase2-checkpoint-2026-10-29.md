# Wave 3 phase 2 checkpoint — real UAT, UX and promotion readiness

**Date:** 2026-10-29
**Code candidate:** `086a63f730f0a3db25c178b7dbcac2951d2db7b4`
**Integration branch:** `campaign/office-ux-wave3-20261028` (docs/tests only after
the code candidate). **`main` is unchanged.**

## A. Real Compose + Chromium product UAT (#60)

Evidence commit: `factory/compose-browser-uat/20261029` at `b2395f1`
(`docs/evidence/compose-browser-uat-2026-10-29/`).

**Result: `PASS_WITH_FINDINGS`.** Author-run, not independent acceptance.

| Item | Result |
|---|---|
| Rootless Compose topology, Core, Office, Worker, verifier, reviewer, nested runtime | REAL on this host |
| Chromium against `http://127.0.0.1:8898` | REAL, no `/core-api` interception |
| Owner enrollment / project import / request / assessment / approval | PASS |
| Worker execution (deterministic fixture provider) | PASS |
| Independent verifier + reviewer receipts | PASS (`8574af0a…`, `1295034f…` in the passing run) |
| Deterministic Quality Gate | **ACCEPTED** |
| Candidate + diff lineage | PASS (`cc46cc80…` at `5e895eb6…`) |
| Agent observability | PASS, 4 participants |
| PR projection | UNBOUND — no GitHub write attempted |
| Live provider inference | **NOT TESTED**, no paid inference |
| Independent acceptance / SEC-01 | **NOT CLAIMED** |

The UAT found two Office V2 contract defects:

1. `RequestWorkspace` shows `Authorization undefined · undefined`.
2. `CampaignDetail` Budget & usage crashes with `TypeError: Cannot read
   properties of undefined (reading 'join')`.

Cause: `office/src/coreApi.ts` still models the legacy snake_case
`CampaignAuthorization`; Core serializes camelCase V2 (`executionMode`,
`riskClass`, `policy.operations`, `policy.progress`, `monetaryEnvelopeId`,
`schemaVersion`, `legacy`). This is a real operator-facing block, not a fixture
artifact.

Screenshots: `home-1440.png`, `agent-office-1440.png`, `campaign-*.png`,
`campaign-budget-1440.png` (crash evidence), all under the evidence directory.

## B. UX continuation (#61/#62/#71)

Branch `factory/office-home-simplification/20261029` at `58d217e`.

- Conversational Home: one objective, one Project, one profile with route
  visibility, one explicit USD inference spend, one submit action.
- Execution Profiles: no required attempt/session/turn/deadline/token/cost
  controls; technical operation limits are advanced settings.
- Settings/provider selector retained.
- Office: typecheck, lint, **244 tests**, build pass.
- Screenshots: `docs/evidence/office-home-simplification-2026-10-29/`
  (Home and Execution Profiles, 1440/390, clearly labelled mocked-Core fixture).

This branch is **not integrated** into the Wave 3 candidate yet. The
CampaignDetail/RequestWorkspace V2 authorization contract is still open; it must
be fixed before Office promotion.

## C. Access stack and promotion readiness (#36/#37/#38)

Independent reviews:

- **Sol: BLOCK all three.**
  - #36: legacy `sessionStorage["factory-api-token"]` remains in upgraded tabs.
  - #37: Tailscale can reach Core's full `0.0.0.0:8765` API, not only the
    Office-only listener.
  - #38: approved pairing survives logout-all/revoke-all/recovery and can be
    exchanged after revocation; consume/issue race.
- **Luna: BLOCK.** Main protection unverified; pairing code is six digits and
  needs throttling/rejection sampling.

Prepared, **not applied**:
`docs/operations/main-branch-protection-2026-10-29.md` and
`main-branch-protection-payload.json` (required checks:
`architecture-consistency`, `rust`, `compose-policy`, `container-dev`; 1 review;
stale-approval dismissal; linear history; no force push/delete).

No access PR was merged. PRs #36–#38 remain drafts.

## Exact next promotion decision for the Owner

1. Authorize **applying the prepared `main` protection** (or request changes to
   the payload) before any merge.
2. Authorize **repair + promotion order** for the access stack: fix #36 legacy
   token removal, #37 network isolation, #38 revocation fencing; re-review;
   merge #36 → #37 → #38 only after checks and approval.
3. Decide the **live inference authorization**: confirm whether OpenCode Go is
   subscription-based or billable API, and approve a bounded spend envelope if
   billable. No live inference is claimed today.
4. Approve or reject the **Office visual slice** after the CampaignDetail V2
   contract fix and screenshots.
5. Only then open a clean Wave 3 promotion PR to `main`; rerun full Rust/Office/
   Compose gates and obtain Sol/Luna + Owner approval.

No `main` merge, branch protection change or paid inference occurred in this
checkpoint.
