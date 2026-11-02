# Wave 3 checkpoint — subscription-only policy and visual foundation (2026-11-02)

**Integration HEAD:** `f2322fc` (remote). **`main`:** unchanged; no merge.
**Code-UAT candidate carried forward:** `40d44ec` (real Compose/Chromium PASS).

## Funding policy correction

- Authoritative policy: **subscription-included only**; no paid Extra Usage,
  pay-as-you-go API, credits, top-ups or silent fallback.
- PR **#79** head **`7366488`** updated: `GO_SUBSCRIPTION_INCLUDED` only; no
  positive USD spend cap; supplied cap rejected; `EXTRA_USAGE` must be
  `DISABLED`; enabled/unknown/unverifiable fails closed; paid modes return
  `PAID_MODE_DISABLED`; separate entitlement/quota, measured tokens, list-rate
  equivalent, provider-reported actual charge and unknown; quota pause is
  recoverable; explicit Owner go-ahead still required.
- Harness verification: **29 tests passed**, `node --check`, `bash -n`, Compose
  override config and parity checks pass. No live inference.
- Sol funding contract recorded at
  `docs/implementation/subscription-funding-policy-contract-2026-11-02.md`.
  Core Funding Policy V3 + migration 0041 is **not implemented yet**.
- Console Extra Usage state and remaining allowance: **UNVERIFIED**; live UAT
  remains blocked until the Owner verifies them and gives a separate go-ahead.

## Visual integration

| PR | State | Evidence |
|---|---|---|
| #77 Home/design-system | **MERGED into Wave 3** (`8c4e1ff`) | 256 Office tests at review; Chromium 1440/834/390/320; `docs/evidence/office-home-visual-polish-2026-10-29/` |
| #78 Agent Office V2 | **MERGED into Wave 3** (`f7479c0`, final inspector fix `f2322fc`) | 265 Office tests after final merge; Chromium 0/1/5/12/30; `docs/evidence/office-isometric-v2-2026-10-29/` |
| #79 provider prep | OPEN draft (`7366488`) | 29 contract tests; subscription-only gate |

The Home direction is approved as a design foundation, not final conversational
acceptance. The Office V2 is accepted functionally; final visual fidelity and
the next studio/renderer iteration remain outstanding. The next Home/Office v3
subagents could not start because the OpenCode Go subscription **usage limit was
exhausted**.

## Security repairs

- Sol/Luna BLOCKED the original access stack: legacy `sessionStorage` token,
  Tailscale reachability to Core `8765`, pairing surviving
  revocation/recovery/consume-issue race, six-digit pairing entropy/throttling.
- The access-repairs branch reached `ad04aca` with Tailscale socket/compose
  policy work and a revocation-fencing proposal. A few policy-test/compose files
  remain uncommitted in the worktree; no access PR was opened or merged.
- `main` protection remains prepared, not applied. Payload uses 0 required
  GitHub approvals (single maintainer), strict required checks
  (`architecture-consistency`, `rust`, `compose-policy`, `container-dev`),
  linear history, no force push/delete, admins enforced.

## Current worktrees / branches

| Lane | Branch | Head | State |
|---|---|---|---|
| Integration | `campaign/office-ux-wave3-20261028` | `f2322fc` | active |
| Access repairs | `factory/access-repairs/20261029` | `ad04aca` + 4 dirty | paused |
| Provider prep | `factory/provider-acceptance/20261029` | `7366488` | PR #79 draft |
| Design | `factory/office-design-polish/20261029` | `967ca5b` | merged |
| Office V2 | `factory/office-isometric-v2/20261029` | `17aa3e9` | merged |
| Conversation v3 | `factory/office-conversation/20261029` | base | blocked: Go quota |
| Office v3 | `factory/office-isometric-v3/20261029` | base | blocked: Go quota |

## Blockers / Owner decisions

1. **OpenCode Go usage limit exhausted** for new DeepSeek subagents; campaign
   implementation is paused. Durable state and worktrees are preserved. Resume
   when the subscription allowance resets; do not switch to paid API.
2. **Core Funding Policy V3** must be implemented (typed subscription binding,
   no mandatory monetary envelope for subscription mode, quota pause/recovery,
   Office subscription/quota projection) before live subscription UAT.
3. **Console verification**: Owner must confirm Go Extra Usage OFF and available
   included allowance, then give a separate narrow live-UAT go-ahead.
4. **Access repairs** need completion and exact-head Sol/Luna review.
5. **Visual decisions**: Home conversational refinement and Agent Office studio
   v3 remain to be reviewed; current merged work is a foundation only.
6. **Branch protection** remains prepared, not applied; Owner authorization
   required.

No paid inference, `main` merge, access-stack merge or branch-protection change
occurred.
