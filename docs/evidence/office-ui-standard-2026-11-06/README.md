# Factory Office UI standard — fixture visual baseline

**Status:** author-run fixture baseline, 2026-11-06, paired with a deterministic
Compose browser UAT (real Core HTTP API and Chromium, fixture provider). **Not**
live-model acceptance, independent acceptance, Product Owner visual
certification or an isolation claim.

This folder records the first visual baseline for
[the Factory Office UI standard](../../product/office-ui-standard.md). Every
screenshot renders the real Office React components with deterministic mocked
Core responses and carries a visible `TEST FIXTURE` banner.

## Contents

| File | View |
|---|---|
| `home-1440.png`, `home-390.png` | Conversational Home |
| `campaign-detail-1440.png`, `campaign-detail-390.png` | Campaign detail with the Budget & usage tab and Campaign pulse |
| `execution-profiles-1440.png`, `execution-profiles-390.png` | Settings → Execution profiles editor |
| `request-campaign-v2-1440.png`, `request-campaign-v2-390.png` | Request workspace |
| `layout-overflow-report.json` | Per-capture viewport, scroll-width and escaped-element checks at 1440, 834 and 390 px |

The machine-readable manifest is
[`office-ui-standard-2026-11-06.json`](../office-ui-standard-2026-11-06.json).
Real Core/Compose browser evidence for the same candidate is recorded in
[`compose-browser-uat-2026-11-06`](../compose-browser-uat-2026-11-06/README.md).

## Capture

Run from the repository root. The harness starts its own Vite server, answers
`/core-api/**` locally and fails if a visible element escapes the viewport.

```sh
CHROMIUM_EXE=/absolute/path/to/chrome \
  node office/e2e-browser/office-home-fixture.mjs
```

`FACTORY_SCREENSHOT_DIR` overrides the output directory. `FACTORY_FIXTURE_PORT`
overrides the Vite port.

## What this evidence does not prove

- Real Core projections, provider inference, agent activity or Quality Gate
  acceptance.
- Keyboard/focus traversal, screen-reader announcements or contrast in every
  state.
- The responsive drawer, dark theme or Spanish terminology for every view.
- Product Owner visual approval.

Those checks belong to the real Compose browser journey and independent
acceptance; record their exact candidate and environment before claiming PASS.
