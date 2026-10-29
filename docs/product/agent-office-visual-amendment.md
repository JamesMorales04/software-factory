# Agent Office visual amendment (2026-10-28)

**Status:** renderer visual revision on top of the V0 functional floor. It is
**not** Owner visual approval, not a screenshot-copy of the concept art and not
independent acceptance. The Product Owner's generated reference is visual
direction only.

## 2026-10-29 visual revision

A second, still author-run revision turns the five separate room slabs into one
continuous studio and increases the amount of original scene art. It keeps the
same participant projection, layout invariants, filters, selection, keyboard
model, inspector, table and timeline.

- **Continuous studio shell.** A single clipped studio platform, tile
  corridors, a continuous back wall with windows, a compact studio sign and
  static ambient light pools now sit behind the role rooms. The room floors are
  inlays on that deck rather than isolated floating cards.
- **Modular rooms.** Every room has a cutaway wall return, panel seams,
  skirting, wall sconces, pendant lights and a role-specific wall treatment:
  planning board, implementation and review pinboards, verification and
  operations toolboards, shelves and a wall clock where space allows. Decor is
  omitted rather than overlapped on narrow rooms.
- **More furniture.** Floor furniture is role-specific and lives in a reserved
  side lane: bookshelf for Planning, coffee station and recycling bin for
  Implementation, mobile test cart for Verification, lounge and coffee table
  for Review, server rack and UPS for Operations. Plants, floor lamps, rugs and
  desk accessories remain static scene geometry.
- **Workstation clusters.** Seats are grouped three-by-three with a real aisle
  (`OFFICE_CLUSTER_SIZE` / `OFFICE_CLUSTER_GAP`) included in both the zone size
  and the slot coordinates. A 30-Implementer floor becomes a six-column,
  five-row bullpen with two visible aisles instead of a uniform grid. The
  reserved furniture lane is excluded from avatar bounds.
- **More dimensional people.** Avatars are now 92 × 112 with a 3/4 head,
  visible ear, jaw shading, varied build, optional beard/goatee/moustache/
  stubble, jacket or overshirt layers, cuffs, trouser palette and reworked hair
  silhouettes. Seated CURRENT figures have a chair with a wheeled base and
  armrests, a desk in perspective, two angled monitors and a keyboard. The
  appearance still derives only from the stable `lookIndex`.
- **Readable overlays at a distance.** Room plaques and hover/selection cards
  counter-scale within a bounded factor so they remain legible when the
  clamped fit camera shrinks a 30-person floor. The scene still has no
  animation, movement, progress or fabricated activity.

The Studio Inspector remains the ordinary participant inspector with the dark
studio token set, and the Table/Timeline alternatives are untouched.

Remaining visual gaps after this revision:

- characters are still illustrated SVG figures, not fully modelled 3/4
  sprites, and a 30-person fit view depends on zoom for facial detail;
- rooms are modular 2D/isometric compositions, not a fully modelled 3D set
  with ceiling, windows through walls or independent prop physics;
- at 320/390 px the default fit remains a pannable crop; document overflow is
  still zero and the crop is intentional;
- there is no motion, pathfinding or decorative animation by design;
- Owner visual approval, independent acceptance and real Core/Compose browser
  acceptance remain outstanding.

## Reference context

The Product Owner supplied `01-office-isometrica-referencia-principal.png` as
the primary Agent Office anchor, with `02-layouts-office-referencia.png` and
`03-home-campanas-layout-referencia.png` as broader composition references and
`README-visual-contract.md` as the written contract.

The implementation remains an original, programmatic SVG/DOM scene. It does not
embed, trace or ship the PNGs, and the imaginary data, labels and progress bars
inside those generated images are not requirements.

## Implemented in this amendment

- **Studio scene rather than a light card grid.** The floor has a deep-navy
  canvas, isometric grid, warm light pools, wood-floored rooms, clipped slab
  edges and cutaway back walls with labelled wall plaques.
- **Furnished rooms.** Each role area has a board, planting, practical floor
  lamp and warm ambient light. Every active workstation adds a wood desk,
  chair, dual glowing monitors, keyboard and mug.
- **Individualized people.** Appearance is derived from the stable participant
  `lookIndex` in the layout: skin, hair colour, hair silhouette, clothing,
  accent and accessory. Five Implementers receive five distinct signatures
  inside their station; 30 Implementers receive 30 unique signatures.
- **Truthful non-working treatment.** Only `CURRENT` participants use the
  seated workstation pose. `STALE`, `FINAL`, `UNKNOWN` and `NOT_EXPOSED`
  participants stand without a monitor/desk and carry a shaped observation
  ring. This deliberately prevents completed or stale records from looking
  like active work.
- **Compact overlays.** Role, identity and recorded status/observation appear
  only on hover, keyboard focus or selection. A tiny observation lamp remains
  visible on the figure; the room signs stay compact wall plaques rather than
  permanent cards over furniture.
- **Contextual studio inspector.** The Visual Office side panel uses the
  existing participant inspector with a dark contextual token set, and can be
  collapsed to give the scene the full canvas width. The Table and Timeline
  views are unchanged.
- **Responsive camera.** Fit, pan and zoom remain clamped; the mobile default is
  a readable crop with pan/zoom rather than an unreadably tiny whole-floor
  overview. No horizontal document overflow is present at 1440/834/390/320.

## Comparison against the primary reference

Aligned:

- deep-navy scene, warm practical light and wood/metal studio materials;
- isometric room composition with labelled areas and cutaway walls;
- modular desks, chairs, monitors, plants and lamps;
- distinct stylized humans, including five independently recognizable
  Implementers;
- compact selection overlays and a dark contextual inspector;
- no simulated motion, progress, cost or activity.

Remaining gaps (deliberately not hidden):

- the Visual Office route uses a scoped dark studio shell, canvas and
  contextual inspector; Table and Timeline intentionally return to the ordinary
  light application theme, so switching views changes the chrome;
- walls are cutaway bands with painted detail, not fully modelled 3D walls,
  windows, shelves or lounge furniture;
- characters are front-facing stylized figures rather than the reference's
  more dimensional 3/4 illustrations;
- small-count rooms still contain more empty floor than the richly furnished
  concept;
- at 320/390 px the scene is a pannable crop and room plaques are small;
- there is no animation or pathfinding, by design.

## Evidence and reproduction

The renderer fixtures are test-only and are not a live Core projection. The
preview harness is served only by the Vite development server; it is excluded
from the production build. Every preview carries a permanent
`TEST FIXTURE — NOT LIVE CORE DATA` banner.

- `/visual-harness.html?count=5&mode=implementers&allActive=1`
- `/visual-harness.html?count=30&mode=implementers&allActive=1`
- `/visual-harness.html?count=12&selected=participant-03-session`

Chromium 156 (Playwright 1.64.1) screenshots were captured for 0/1/5/12/30 at
1440, 834, 390 and 320 CSS pixels, plus five all-current Implementers, 30
all-current Implementers, hover and selection states, and Spanish labels. The
handoff records the exact paths. The full-page screenshot used a mocked Core
API only to exercise layout and selection; it is labelled as a fixture and is
not live activity or acceptance evidence.

Functional coverage remains in
`office/src/components/office/officeSceneLayout.test.ts` and
`office/src/components/office/AgentOfficeIsometric.test.tsx`: identity,
deduplication, stable slots, 0/1/5/12/30, five distinct Implementers,
non-working final/stale treatment, filters, selection, keyboard focus,
pan/zoom clamps, responsiveness measurements and no fabricated entities.
