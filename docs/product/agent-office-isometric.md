# Agent Office isometric floor (V0)

**Status:** V0 presentation implemented in the Agent Office visual tab, with a
2026-10-28 visual amendment toward the Owner's primary isometric reference. The
floor is a deterministic rendering of the existing participant projection, not
a live simulation and not product acceptance. Component tests cover layout,
visual identity, selection and camera behavior; real browser acceptance against
the Compose stack remains candidate-bound work. See the
[visual amendment](agent-office-visual-amendment.md) for the reference
comparison and remaining gaps.

**Product job:** let the operator read the observed engineering organization at
a glance — who is recorded, in which role station, with which truthfully
reported observation — then open the existing participant inspector for
evidence.

## 1. Renderer decision

V0 uses one custom SVG/DOM scene:

- SVG studio shell, corridors, rooms, wall treatments, furniture, workstations
  and figures.
- DOM `<button>` elements for avatars and the toolbar, so focus, names and
  activation are native browser behavior.
- CSS transforms for pan/zoom; React state only, no per-frame game loop, no
  requestAnimationFrame loop, no canvas renderer and no animation dependency.

**PixiJS 8 is deferred, not adopted.** The decision follows the dependency
discipline in [dependency selection](../engineering/dependency-selection.md)
(applied to the browser runtime as well as Rust infrastructure):

| Consideration | V0 finding |
|---|---|
| Capability actually needed | Static isometric placement, legible labels, selection, pan/zoom |
| Existing dependencies | React, SVG and CSS already satisfy the capability |
| Bundle/runtime impact | A WebGL renderer adds a measurable runtime and asset pipeline before any measured need exists |
| Accessibility | Native buttons and text are simpler to name, focus and test than a canvas scene |
| Text | Participant identity and observation labels must be real, selectable, high-contrast text |
| Motion | The product explicitly forbids simulating activity; no continuous animation requirement exists |
| Testability | Pure layout and DOM assertions work without a WebGL test harness |
| Scale | 0, 1, 5, 12 and 30 observed entities fit the current bounded projection |

Revisit PixiJS only when a measured requirement exists for sprite animation,
pathfinding, large sprite atlases or sustained scenes beyond the projection's
200-participant bound. That evaluation must include a measured bundle budget,
a rendering performance target, an accessibility and keyboard equivalent, and
the same Core-authority rules. The 2026-10-28 visual amendment kept the
SVG/DOM renderer: it added room composition, lighting, furniture and individual
avatar identities without requiring a per-frame renderer.

## 2. Scene model

The scene is global. Project and Campaign filters remain Core-backed query
parameters; the floor renders exactly the participant page returned for those
filters. It never mixes a second client-side source of participants.

- One avatar per distinct `participant_id`; duplicates are ignored and
  explicitly reported. Five observed Implementers produce five avatars.
- Role selects a station: Lead → Planning, Implementer → Implementation,
  Verifier → Verification, Reviewer → Review, Worker → Operations.
- Slot order is the rank of `participant_id` inside the station after sorting.
  Re-rendering, polling or receiving the same records in a different order
  cannot move an avatar. Adding or removing a participant can renumber later
  slots in the same station; the next render is still deterministic.
- Avatar pitch is larger than the avatar bounds (110 × 118 px pitch for a
  92 × 112 px button), so avatar buttons never overlap at any tested density.
  Seats are grouped in clusters of three with an explicit aisle gap included
  in both the room size and the slot coordinates. Depth ordering remains only
  a painting concern, not a hit-target or legibility dependency.
- Each room reserves a side lane for role furniture; avatar bounds are kept out
  of that lane. The continuous studio shell, corridors, wall art, plants,
  lamps, bookshelf, coffee station, test cart, lounge, server rack and pendant
  lights are static original SVG/CSS geometry and carry no activity meaning.
- Conflicting duplicate projections of one `participant_id` are resolved
  deterministically: `CURRENT` before unresolved/terminal observations, then
  the newest `last_observed_at`, then a stable lexical field tiebreak. Reversing
  the input page cannot change the winner, station or appearance.
- Painter-order depth follows the station feet position, so nearer rows render
  in front. Selection raises only the selected avatar.
- The world grows with the largest station (up to six columns and five rows per
  station in the tested range) and is centered by a fitted camera. Room plaques
  and avatar overlays counter-scale within a bounded factor for readability at
  the fitted 30-person zoom.

## 3. Truthful state

Every avatar comes from a `ParticipantRecord`. The scene derives only:

- role and identity;
- recorded status text;
- observation `CURRENT`, `STALE`, `FINAL`, `NOT_EXPOSED` or `UNKNOWN`;
- last-observed timestamp, retained in the inspector.

The scene adds no movement, thoughts, tool calls, progress, usage, cost or
acceptance. A lamp and nameplate glyph distinguish observations by shape and
text, not color alone; the full observation is part of the accessible name.
A `CURRENT` observation means Core holds a matching live lease generation.
A `STALE` observation means the recorded session is unresolved without a
matching live lease — it is not proof that an external session stopped.
Fixtures used in component tests are renderer fixtures and are never presented
as live activity.

## 4. Interaction and accessibility

- Avatars are native buttons. Enter/Space opens the existing participant
  inspector; `aria-pressed` reflects selection.
- Arrow keys move focus to the nearest avatar in the requested direction.
- Drag pans; the wheel zooms around the pointer; explicit pan, zoom-in,
  zoom-out and fit controls work without a pointer. Zoom is clamped to
  45–200% and panning is clamped so the floor cannot be lost.
- The zoom percentage is announced with `aria-live="polite"`.
- Every station is a labelled region; each avatar's accessible name includes
  role, participant identity, recorded status and observation.
- The scene respects `prefers-reduced-motion`; there is no essential motion.
- The viewport hides overflow, the toolbar wraps, and the scene is exercised at
  desktop, tablet and narrow widths. The table and timeline remain available as
  non-visual alternatives over the same records.

## 5. Limitations

- V0 is static. It does not animate walking, conversation, progress or tools.
  Furniture, plants, wall art and studio lighting are static scene geometry and
  do not imply that an agent is working.
- There is no drag-and-drop reassignment: station assignment is Core-derived,
  not an operator action. No Core operation exists to move a participant.
- The scene is not a second source of truth and does not stream events; the
  page polls the bounded projection.
- `office/visual-harness.html` is a developer-only fixture renderer for visual
  review. Its captures are not live-agent activity and are not browser
  acceptance against the real Core/Compose stack.
- The page projects at most 200 participants. Larger organizations require
  Campaign/project scope or a future paging design.
- Visual regression, Storybook and a full axe/manual accessibility audit remain
  open. Component tests are necessary but not sufficient. The 2026-10-29 visual
  revision's Chromium fixture captures are author self-test evidence only; see
  the [visual amendment](agent-office-visual-amendment.md).
- Real browser acceptance against the real Core/Compose stack must be recorded
  separately and bound to the exact candidate commit.
