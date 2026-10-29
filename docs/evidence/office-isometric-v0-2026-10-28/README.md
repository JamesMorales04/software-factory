# Isometric Agent Office V0 — renderer self-test evidence

**Level:** author self-test, deterministic browser fixtures. **Not** live agent
activity, **not** independent acceptance and **not** Compose acceptance.

- Code candidate: `c06432d` (repair commit above `489bc48`)
- Browser: Google Chrome for Testing 156.0.8078.4 via pinned
  `playwright-core` 1.64.1, headless, isolated temporary profile.
- App: the real Office Vite bundle at `/#/agents`; Core API responses were
  intercepted with deterministic `ParticipantRecord` fixtures (12 entities,
  2 Campaigns, 1 Project) and a visible `MOCK CORE FIXTURE` banner was injected
  before capture. No Core, Worker, provider or Compose process was contacted.
- Scene density, hover/selection, Spanish and empty-state screenshots use
  `office/visual-harness.html`, served only by the Vite dev server, excluded
  from the production build, with a permanent
  `TEST FIXTURE — NOT LIVE CORE DATA` banner.

## Results

| Check | Result |
|---|---|
| 12 avatars at 1440 / 834 / 390 / 320 px | 12 at each width; document overflow 0 px |
| Console errors / failed requests | 0 / 0 |
| Keyboard selection | focus first avatar + Enter → `#/agents/lead:participant-00`, inspector opens |
| Role filter | `#/agents?role=IMPLEMENTER` → 3 avatars, URL preserved |
| Inspector collapse | hide/show the contextual side panel without losing view or filters |
| 5 all-current Implementers | 5 distinct avatar appearances in Implementation |
| 30 all-current Implementers | 30 avatars; 30 unique looks; avatar bounds non-overlapping; narrow widths use a pannable crop |
| Avatar bounds | every pair of 84 × 100 avatar bounds is non-overlapping at 0/1/5/12/30 and 30 Implementers (tested) |
| Duplicate identities | CURRENT > STALE/FINAL/UNKNOWN, then newest `last_observed_at`, then lexical tiebreak; reverse input yields the same station/state/appearance (tested) |
| Mixed observations | STALE/FINAL/UNKNOWN stand without a workstation; CURRENT is seated |
| 0 participants | truthful empty state; zero avatars |
| Spanish | EN/ES labels and room plaques render without missing-key fallback |

Screenshots: `page-1440.png`, `page-834.png`, `page-390.png`, `page-320.png`,
`page-selected-1440.png`, `page-filtered-1440.png`; scene fixtures
`scene-0-1440.png`, `scene-0-390.png`, `scene-1-1440.png`, `scene-1-390.png`,
`scene-5-implementers-1440.png`, `scene-5-implementers-390.png`,
`scene-12-1440.png`, `scene-12-390.png`, `scene-30-1440.png`,
`scene-30-390.png`, `scene-30-320.png`, `scene-12-es-1440.png`,
`scene-5-selected-1440.png`, `scene-5-hover-1440.png`.

The screenshots were captured after the layout-spacing and duplicate-resolution
repair (`c06432d`). Side-by-side comparisons with the Owner's primary reference
are `comparison-primary-vs-full-page.jpg` and
`comparison-primary-vs-30-active.jpg`; they are visual-direction comparisons,
not fidelity or approval claims. The page captures use a mocked Core API with a visible
`MOCK CORE FIXTURE` banner; scene captures use the fixture harness with a
permanent `TEST FIXTURE — NOT LIVE CORE DATA` banner. At 390/320 px the scene
itself is a pannable crop that intentionally exceeds the clipped, non-scrolling
viewport; the document does not overflow. Room plaques and overlaid identity
cards are small at those widths.

## NOT_TESTED

Real Core projection, Compose, independent verification, Quality Gate, live
provider inference, axe/manual accessibility audit, visual regression and
Owner visual approval.
