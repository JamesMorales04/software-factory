# Agent Office V3 renderer evidence — 2026-10-29

## Candidate and scope

Source candidate: `e31e46298b42f13e80701913b43878336587ccb8` on
`factory/office-isometric-v3/20261029`. This includes the V3 studio renderer
and a follow-up fix for the 49+ participant non-terminating look assignment.
The replacement remains stable for the same participant set and provides
distinct palette combinations through the current 200-participant projection.
Screenshots in this evidence commit were captured
from that exact tracked source snapshot.

V3 replaces the individual flat role rooms with a continuous studio deck and
isometric floor grid. The renderer adds wall windows, sconces and light pools,
beveled station furniture, plants, storage, server and test carts, and more
dimensional original SVG characters. Deterministic station assignment remains
bound to distinct participant identities and recorded Core status. Empty role
stations are omitted and reported; the scene does not simulate agent activity.
The camera adapts to the available viewport, with pan, zoom and fit controls.

## Chromium fixture captures

The screenshots use the Vite `visual-harness.html` renderer fixture. Each page
shows `TEST FIXTURE — NOT LIVE CORE DATA`; these are styling observations, not
live Core, campaign or acceptance evidence. Captured with Chromium 1248 through
Playwright Core 1.64.1.

| Screenshot | Viewport | Fixture participants | Observed renderer result |
| --- | ---: | ---: | --- |
| [Desktop](desktop-1440.png) | 1440 × 1000 | 5 Implementers | 5 avatars and 5 identity controls; no document overflow or browser errors |
| [Tablet](tablet-834.png) | 834 × 1112 | 12 mixed roles | 12 avatars and 12 identity controls; no document overflow or browser errors |
| [Mobile](mobile-390.png) | 390 × 844 | 5 Implementers | 5 avatars and 5 identity controls; full studio centered at 35% zoom; no document overflow or browser errors |

The mobile capture exposed and then verified a camera-fit fix: the previous
55% fit floor cropped both sides of the studio on a narrow viewport. The fit
floor now allows 20%, while the mobile 5-participant scene fits at 35%.

Reference inspected side-by-side:
`docs/product/reference/factory-office-visual-reference-pack-20261028/01-office-isometrica-referencia-principal.png`.
The V3 renderer establishes the continuous studio and dimensional vector asset
direction, but remains visibly less painterly and less spatially rich than the
reference. These captures do **not** establish Product Owner visual acceptance.

## Renderer assessment

The current SVG/DOM renderer remains adequate for the tested 0, 1, 5, 12 and 30
participant layouts. Automated rendering tests verify one distinct avatar per
participant identity and stable, non-overlapping station slots. Regression
coverage checks 200 distinct palette combinations and exercises a 200-record
layout without collision probing. Browser captures show responsive camera
behavior through 12 participants. No evidence currently shows that a different
rendering engine is needed to meet the 30-agent layout requirement.

No PixiJS or alternate-engine spike was run. The visible gap against the
reference is chiefly in illustration fidelity, material lighting and perceived
depth. An engine migration would change scene submission and batching but would
not supply those art assets, so it is not yet a justified visual-quality fix.
Continue with original layered artwork, lighting and composition in SVG/DOM. If
measured browser performance at larger agent counts becomes inadequate, run a
bounded comparison of SVG/DOM and layered original assets in a 2D renderer
against the same identity, accessibility and performance checks.

## Verification

In a disposable Compose development-container snapshot whose tracked source
files matched the source candidate exactly:

- `npm --prefix office run typecheck` — PASS
- `npm --prefix office run lint` — PASS, Biome checked 141 files
- `npm --prefix office run test -- --reporter=dot` — PASS, 268 tests in 35 files
- `npm --prefix office run build` — PASS; Vite reports the existing 1.2 MB main
  JavaScript chunk warning
- Playwright Chromium renderer capture — PASS for the three viewport cases
  above; assertions checked participant counts, fixture banner, no document
  overflow and no page errors. Fixture only, no live Core or API calls.

The Office component tests cover keyboard and selection behavior, camera
controls, identity mapping and layouts at 0/1/5/12/30 participants. The
screenshots do not exercise the authenticated Office inspector against Core.
Independent review of the exact source candidate is pending, and no Owner visual
approval has been given.
