# Wave 3 phase 3 checkpoint — 2026-10-29

**Integration branch:** `campaign/office-ux-wave3-20261028`
**Remote HEAD at checkpoint:** `f0f9a24`
**Real-UAT code candidate:** `40d44ec`
**`main`:** unchanged; no merge.

## Real acceptance

The post-repair real Compose + Chromium journey **PASSED** on `40d44ec`:
request `272f0fb2…`, Campaign `1322c7a4…`, candidate `cbdb3218…` /
`e0ac1a33…`, verifier `574e7eb2…`, reviewer `394fd54d…`, Gate **ACCEPTED**,
4 participants, `campaign_budget_projection` **PASS**, no findings. Evidence:
`docs/evidence/compose-browser-uat-2026-10-29-v2/`.

## Active implementation lanes

| Lane | Branch | Commit | State |
|---|---|---|---|
| Design System + Home (#62) | `factory/office-design-polish/20261029` | `967ca5b` | 256 Office tests; screenshots; PR **#77** |
| Agent Office v2 (#65/#70) | `factory/office-isometric-v2/20261029` | `8749394` | 255 Office tests; screenshots; PR **#78** |
| Access repairs (#36–#38) | `factory/access-repairs/20261029` | in progress | 28 files dirty; no PR yet |
| Provider acceptance prep (#60) | `factory/provider-acceptance/20261029` | `3290799` | 13 contract tests; PR **#79** |
| Director | `campaign/office-ux-wave3-20261028` | `f0f9a24` | integration/evidence |

### Design/Home (#62)

Navy shell, airy content, composer-first Home, compact project/profile selectors,
one explicit spend control, recent work/decisions secondary. Screenshots:
`docs/evidence/office-home-visual-polish-2026-10-29/`. Owner visual decision
required: accept direction A (propagate) or request variant B.

### Agent Office v2

More continuous studio floor, modular rooms, richer furniture, 3/4 characters,
density 0/1/5/12/30, camera/inspector preserved. Screenshots:
`docs/evidence/office-isometric-v2-2026-10-29/`. One avatar per real
`participant_id`; no simulated activity.

### Access security

Sol and Luna **BLOCK** the original stack: legacy `sessionStorage` operator
token (#36), Tailscale reachability to Core `8765` (#37), pairing surviving
revocation/recovery plus consume/issue race (#38), six-digit pairing code
entropy/throttling. Repairs are in progress on the same lane; no PR will open
until tests pass and exact-head Sol/Luna review is requested.

### Provider acceptance prep

Runbook and gated harness prepared. Accounting finding: OpenCode Go is a
subscription with included monthly dollar allowance; Extra Usage is an opt-in
paid fallback; raw Go `usage.cost` is 0 within allowances, while local OpenCode
message cost is a list-rate estimate, not a balance charge. Factory does not
persist Go allowance/balance; Console/invoice remain UNKNOWN. No inference was
run.

## Branch protection

Payload updated for the actual single-maintainer repository: 0 required GitHub
approvals (self-approval impossible), strict required checks
(`architecture-consistency`, `rust`, `compose-policy`, `container-dev`),
conversation resolution, linear history, no force push/delete, enforce admins.
Independent Sol/Luna review remains a process gate. **Not applied.**

## Owner decisions

1. **Visual:** accept direction A and propagate, or request variant B for Home;
   approve/refine Agent Office studio v2.
2. **Access:** authorize repairs and the #36 → #37 → #38 promotion order after
   exact-head review; authorize applying `main` protection.
3. **Provider:** choose funding mode (`GO_SUBSCRIPTION_INCLUDED`,
   `GO_EXTRA_USAGE_BILLABLE`, `PAY_AS_YOU_GO_API`), Extra Usage policy, current
   Console allowance/balance, and a cap (>0, <=5.00 USD).
4. **Promotion:** only after access repairs and branch protection, open a clean
   Wave 3 PR to `main` and rerun full gates.

No `main` merge, branch-protection change, paid inference or access-stack merge
occurred.
