# Agent Office V3 renderer evidence — 2026-10-29

## Candidate and scope

Source candidate: `a294a43f1c837b721f3b40f4cf217294255742cb` on
`factory/office-isometric-v3/20261029`. This
includes the V3 studio renderer, the 49+ participant look-assignment fix, and
staggered back/front workstation rows with perspective-scaled original vector
figures. Avatar appearance remains deterministic for the same participant set
and distinct through the current 200-participant projection. The Chromium
screenshots in this update were captured from that tracked source snapshot.

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
| [Desktop](desktop-1440.png) | 1440 × 1000 | 5 Implementers | 5 avatars in a staggered 3+2 formation; no document overflow or page errors |
| [Tablet](tablet-834.png) | 834 × 1112 | 12 mixed roles | 12 avatars across role-specific stations; no document overflow or page errors |
| [Mobile](mobile-390.png) | 390 × 844 | 5 Implementers | 5 avatars in the same 3+2 formation at fitted 64% zoom; no document overflow or page errors |

The mobile capture uses the existing responsive camera fit and keeps the full
5-participant studio in the clipped viewport without document overflow.

Reference inspected side-by-side:
`docs/product/reference/factory-office-visual-reference-pack-20261028/01-office-isometrica-referencia-principal.png`.
The V3 renderer establishes the continuous studio and dimensional vector asset
direction, but remains visibly less painterly and less spatially rich than the
reference. These captures do **not** establish Product Owner visual acceptance.

## Bounded visual spike

The reference places people, desks and shared furniture at different apparent
depths inside one richly illustrated studio. The previous five-person renderer
put a small crew in one horizontal row. This slice uses two stable workstation
rows for groups of 3–12 and scales vector figures from 0.90 in the back row to
1.00 in front; groups up to two remain on one compact row, and larger crews
continue to use the existing bounded multi-row layout. The five-person desktop
capture shows the 3+2 arrangement, while the tablet capture retains all 12
mixed-role identities.

The comparison did not justify replacing SVG/DOM with PixiJS: the current
renderer already handles the tested 0/1/5/12/30 densities and 200-record
identity assignment, and the main remaining visual gap is the reference's
layered illustration and material rendering. An engine change alone would not
provide those assets. The next useful visual work remains original scene art,
lighting and furniture composition; this bounded layout update is not a
fidelity claim.

## Renderer assessment

The current SVG/DOM renderer remains adequate for the tested 0, 1, 5, 12 and 30
participant layouts. Automated rendering tests verify one distinct avatar per
participant identity, staggered rows, depth scale, and stable, non-overlapping
station slots. Regression coverage checks 200 distinct palette combinations
and exercises a 200-record layout without collision probing. Browser captures
show responsive camera behavior through 12 participants. No evidence currently
shows that a different rendering engine is needed to meet the 30-agent layout
requirement.

No rendering-engine benchmark was run. The visual spike found the main remaining
gap is illustration fidelity, material lighting and perceived depth. An engine
migration would change scene submission and batching but would not supply those
art assets, so it is not yet a justified visual-quality fix. Continue with
original layered artwork, lighting and composition in SVG/DOM. If measured
browser performance at larger agent counts becomes inadequate, run a bounded
comparison of SVG/DOM and layered original assets in a 2D renderer against the
same identity, accessibility and performance checks.

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

The current canonical Compose command `cargo xtask e2e office` rebuilt the
candidate binaries, then failed before reaching Agent Office: its deterministic
Worker exited with status 1 at the `fixture Worker must succeed` assertion in
`office/e2e-browser/rust-journey.mjs`. This leaves Core/Compose Agent Office
acceptance NOT_TESTED for this run; it is separate from the passing Vite fixture
captures. The failing journey did not reach provider-session creation, and no
model request or live inference was attempted.
