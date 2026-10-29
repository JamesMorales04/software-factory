# Agent Office isometric V2 — renderer self-test evidence

**Level:** author self-test, deterministic browser fixtures. **Not** live agent
activity, **not** independent acceptance, **not** Compose acceptance and **not**
Owner visual approval.

- Code candidate: `a9f191e` (visual V2 on `main` base `2907b17`)
- Visual capture candidate: `3f13242`; `a9f191e` is a test-only timeout commit with identical renderer output.
- Browser: Chromium `156.0.8078.4` via pinned `playwright-core` 1.64.1,
  headless, isolated temporary profile.
- App: the real Office Vite bundle at `/#/agents`; Core API responses were
  intercepted with deterministic `ParticipantRecord` fixtures (12 entities, 1
  Campaign, 1 Project, empty tool-event projection) and a visible
  `TEST FIXTURE — NOT LIVE CORE DATA` badge was injected before capture. No
  Core, Worker, provider or Compose process was contacted.
- Scene harness: `office/visual-harness.html`, served only by the Vite dev
  server and excluded from the production build, with a permanent
  `TEST FIXTURE — NOT LIVE CORE DATA` banner. It renders the same
  `AgentOfficeIsometric` component with deterministic fixtures.

## Captures

All scene captures are 20 files: `scene-{0,1,5,12,30}-{1440,834,390,320}.png`.
`scene-0-*` is the truthful empty state (no floor, no fabricated avatars). The
other counts render exactly that many distinct participant identities.

Additional captures:

| File | Purpose |
|---|---|
| `scene-5-implementers-1440.png` | five all-CURRENT Implementers, five distinct looks |
| `scene-30-implementers-1440.png`, `scene-30-implementers-390.png` | 30 Implementers, six-column/five-row bullpen |
| `scene-12-selected-1440.png` | selection outline and identity card |
| `scene-12-es-1440.png` | Spanish labels |
| `page-1440.png`, `page-selected-1440.png`, `page-390.png`, `page-scene-390.png` | real Office shell, filters, scene and contextual inspector with a visible fixture badge |

Measurements (from `office-isometric-v2.json`): every scene capture at
1440/834/390/320 reported `0` document overflow, `0` console errors, `0` failed
requests, and the expected avatar count. The fitted camera reported 93% for
1/5 participants, 79% for 12 and 69% for 30 mixed participants at 1440 px. The
320/390 px renders are intentional pannable crops inside the clipped scene
viewport; the document does not overflow.

## Comparisons

- `comparison-primary-vs-page-1440.jpg`
- `comparison-primary-vs-30-implementers-1440.jpg`

Reference:
`docs/product/reference/factory-office-visual-reference-pack-20261028/01-office-isometrica-referencia-principal.png`.
Both comparisons are visual-direction aids only, not fidelity, approval or
acceptance claims. The reference contains imaginary labels, data and progress
bars that the implementation does not copy.

## Functional checks

- `npm run typecheck` — PASS (`tsc --noEmit`)
- `npm run lint` — PASS (`biome check src e2e-browser`)
- `npm run test` — PASS (33 files, 255 tests)
- `npm run build` — PASS (existing >500 kB chunk-size warning only)
- Component tests cover identity/deduplication, stable slots, cluster aisles,
  the reserved furniture lane, 0/1/5/12/30 density, 5/30 distinct looks,
  CURRENT-seated versus other-standing truthfulness, keyboard selection,
  filters, inspector, table/timeline separation and camera clamps.

## Truthfulness

- One avatar per distinct `participant_id`; duplicate projections are resolved
  deterministically by the existing layout.
- Only `CURRENT` observations are seated at a workstation. `STALE`, `FINAL`,
  `UNKNOWN` and `NOT_EXPOSED` observations stand with a shaped observation
  ring. No movement, progress, reasoning, tool use or simulated activity is
  shown.
- Furniture, wall art, plants, lamps and static LEDs carry no activity meaning.
- The scene is original SVG/DOM/CSS geometry. No game art, screenshot or static
  background image is embedded.

## NOT_TESTED

Real Core projection, Compose, independent verification, Quality Gate, live
provider inference, axe/manual accessibility audit, visual regression tooling
and Owner visual approval. The Product Owner visual decision is **PENDING**.
