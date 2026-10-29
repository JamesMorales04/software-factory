# Office Home and Execution Profiles — fixture visual evidence (2026-10-29)

**Status: FIXTURE-ONLY — NOT ACCEPTANCE AND NOT LIVE ACTIVITY.**
These screenshots render the real Office React components from a Vite dev
server whose `/core-api` requests are answered by deterministic local fixtures.
No Core process, Compose stack, provider or model was contacted. The orange
banner in every image marks the fixture explicitly.

- Branch: `factory/office-home-simplification/20261029`
- Base: `086a63f` (approved development candidate)
- Workstream: Wave 3 B — issues #61, #62 and #71
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

## Reproduce

```sh
cd office/e2e-browser && npm ci
CHROMIUM_EXE=/path/to/chrome \
  node office/e2e-browser/office-home-fixture.mjs
```

The harness starts Vite on `127.0.0.1:5199`, mocks `/core-api` (setup, session,
health, Projects, execution profiles, intake list, Office overview, provider
catalog and project profile resolution), sets the light theme, and writes the
four PNGs into this directory. Override `FACTORY_SCREENSHOT_DIR` to capture
elsewhere.

## What these images support

- Home presents one Project, one natural-language objective, one execution
  profile, one explicit USD inference authorization and one submit action.
- Home does not expose turn, attempt, session, token, cost-micros or Campaign
  deadline inputs.
- Provider/model/reasoning routes are visible for a selected profile.
- Execution Profiles edits routes normally; technical per-operation limits and
  progress supervision are optional and collapsed under **Advanced**.
- The simplified provider selector and advanced-only manual configuration are
  covered by component tests, not by this Home/Profiles capture.

## What these images do **not** prove

- Real Core/Compose behavior, persistence, authorization or recovery.
- Live provider inference, billing, Codex/OAuth or OpenCode Go connectivity.
- Playwright end-to-end acceptance, accessibility certification or visual
  regression.
- Product Owner visual approval. Owner approval remains **outstanding**.

The matching component evidence is `npm --prefix office run
typecheck|lint|test|build`; the exact implementation commit is reported in the
handoff because a file cannot contain its own containing commit hash. Real-Core browser acceptance of the simplified path remains
**NOT_TESTED**.
