# Agent Office V3 — real Core/Chromium engineering captures

**Date:** 2026-11-03
**Harness:** `cargo xtask e2e office` with
`FACTORY_E2E_SCREENSHOT_DIR` set. The journey starts the real Rust Core and
Worker, a Vite Office build and Playwright Chromium with disposable state.
**Journey result:** `{"ok":true,"candidate":"1b642871cd286bde26815544eec5d921228d76ee","gate":"ACCEPTED"}`
The candidate id is the fixture Campaign candidate created by the journey, not
the Office branch commit.

## What the captures show

- `agent-office-1440.png` — docked participant inspector and the full studio
  floor at 1440×900.
- `agent-office-834.png` — two-column studio layout at 834×900.
- `agent-office-390.png` — single-column layout at 390×844, including the
  floor and inspector; no document overflow.
- `manifest.json` — candidate, gate, capture time and dimensions.

The projection contains four participants from the journey's real Core records.
The journey also asserts no unexpected 404, console error or document overflow
at the widths above.

## Visual defect found and repaired

The first capture showed a white inspector empty state on the dark studio
surface with white-on-white text. The studio scopes overrode `--surface` but
not `--surface-inset`, so `.ui-empty` retained the light-theme inset. The
scopes now set `--surface-inset: #081421`. They also define the dark semantic
foregrounds (`--success`, `--attention`, `--danger`, `--info`, `--text-subtle`,
`--accent-contrast`) so badges and controls keep contrast in both themes.

Avatar appearance is now seeded from the stable person `identity_id` against
the extended palette, with deterministic in-scene collision resolution. A
participant no longer changes appearance merely because an earlier filtered
row disappeared.

## Scope of this evidence

- This is Implementer/Engineering Director self-test evidence, not independent
  acceptance and not Product Owner visual sign-off.
- Counts 0/1/5/12/30 participant layouts are covered by component tests, not by
  this four-participant real journey.
- No paid inference, deployment or security certification is implied.
