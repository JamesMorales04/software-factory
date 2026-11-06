# Office Home, profiles and Campaign V2 contract — fixture visual evidence (2026-10-29)

**Status: FIXTURE-ONLY — NOT ACCEPTANCE AND NOT LIVE ACTIVITY.**
These screenshots render the real Office React components from a Vite dev
server whose `/core-api` requests are answered by deterministic local fixtures.
No Core process, Compose stack, provider or model was contacted. The orange
banner in every full-page image marks the fixture explicitly.

This directory also carries the repairing evidence for the two Office contract
defects reported by the read-only real Compose/Chromium UAT at
`/home/james/Escritorio/Development/sf-wave3/compose-uat/docs/evidence/compose-browser-uat-2026-10-29/`:

- Request workspace printed `Authorization undefined · undefined` because it
  read legacy snake_case fields.
- Campaign **Budget & usage** crashed on `undefined.join` because it read V1
  arrays and caps that a V2 authorization does not return.

- Branch: `factory/office-home-simplification/20261029`
- Base: `086a63f` (approved development candidate)
- Workstream: Wave 3 B — issues #61, #62 and #71, plus the UAT V2 contract repair
- Capture harness: `office/e2e-browser/office-home-fixture.mjs`
- Browser: Google Chrome for Testing `156.0.8078.4` (pinned Playwright Chromium)
- Vite: `7.3.7`

## Files

| File | Viewport | Content |
|---|---|---|
| `home-1440.png` | 1440 px | Conversation-first Home with Project, selected Balanced profile and route preview, one USD inference authorization, submit, recent requests, attention and compact status |
| `home-390.png` | 390 px | Same Home at narrow width; context controls, profile routes, authorization and actions stack without clipping |
| `execution-profiles-1440.png` | 1440 px | Profile list with routes, defaults/effective profile and role matrix; productivity caps are absent and technical operation limits are collapsed under **Advanced technical operation limits** |
| `execution-profiles-390.png` | 390 px | Same profiles surface as stacked cards and a stacked role matrix; hash/default values remain readable and actions are not clipped |
| `campaign-budget-v2-1440.png` | 1440 px | Campaign **Budget & usage** with camelCase V2 authorization: schema version, policy, monetary envelope, scope, operation limits and progress supervision; no legacy caps and no crash |
| `campaign-budget-v2-390.png` | 390 px | Same V2 budget surface at narrow width |
| `request-campaign-v2-1440.png` | 1440 px | Completed Request workspace with Campaign panel showing `R0 · TRUSTED_LOCAL_PROVIDER · opencode-go/deepseek-v4.1-flash`, no `undefined` labels |
| `request-campaign-v2-390.png` | 390 px | Same Request workspace at narrow width |

## Reproduce

```sh
cd office/e2e-browser && npm ci
CHROMIUM_EXE=/path/to/chrome \
  node office/e2e-browser/office-home-fixture.mjs
```

The harness starts Vite on `127.0.0.1:5199`, mocks `/core-api` (setup, session,
health, Projects, execution profiles, intake list/detail, Campaign detail,
actions, provider sessions, Office overview, provider catalog and project profile
resolution), sets the light theme, and writes the PNGs into this directory.
Override `FACTORY_SCREENSHOT_DIR` to capture elsewhere. The Campaign/request
fixtures use the V2 camelCase authorization shape returned by Core
`CampaignAuthorizationRecord`; the legacy V1 shape is covered by component tests.

## What these images support

- Home presents one Project, one natural-language objective, one execution
  profile, one explicit USD inference authorization and one submit action.
- Home does not expose turn, attempt, session, token, cost-micros or Campaign
  deadline inputs.
- Provider/model/reasoning routes are visible for a selected profile.
- Execution Profiles edits routes normally; technical per-operation limits and
  progress supervision are optional and collapsed under **Advanced**.
- Campaign **Budget & usage** is safe with V2 camelCase authorizations and empty
  arrays; it renders `policy.operations`, `policy.progress`,
  `monetaryEnvelopeId`, schema version and the recorded route/risk/mode.
- Request workspace reads the V2 risk class, execution mode and provider route
  without undefined labels.
- Legacy V1 caps render only in a separate historical section, and only when
  `authorization.legacy` is present (`CampaignDetail.test.tsx`).

## What these images do **not** prove

- Real Core/Compose behavior, persistence, authorization or recovery.
- Live provider inference, billing, Codex/OAuth or OpenCode Go connectivity.
- Playwright end-to-end acceptance, accessibility certification or visual
  regression.
- Product Owner visual approval. Owner approval remains **outstanding**.

The matching component evidence is `npm --prefix office run
typecheck|lint|test|build`; the exact implementation commit is reported in the
handoff because a file cannot contain its own containing commit hash. Real-Core
browser acceptance of the repaired V2 Campaign panels remains **NOT_TESTED**.
