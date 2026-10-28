# Factory Office design system

**Status:** Design foundation implemented in the Office design-foundation
slice: semantic tokens, light/dark themes, comfortable/compact density, the
application shell, shared button/field/badge/surface/tooltip/dialog primitives,
command navigation and component tests. Operational Overview, Decision Inbox,
Action detail, the tabbed Campaign workspace, the Runtime console, the
provider Settings workspace, Agent Office and the interactive WorkGraph are
implemented against Core projections. Storybook, visual regression, the full
accessibility audit, Issues and Docs remain later slices. The Campaign
workspace now exposes a bounded candidate file diff and GitHub PR inspection;
real browser and live GitHub acceptance remain open. Nothing here says those modules exist beyond their documented
read-only projections.

**Product job:** the primary workspace where the Product Owner manages
engineering objectives, decisions, autonomous agents, evidence and multiple
projects without raw API calls or terminal inspection.

## 1. Direction

Enterprise engineering workspace with modern, restrained and technically
precise aesthetics. It should feel like an operations console built for sustained
work: calm surfaces, strong hierarchy, dense but readable data, and one
controlled interactive accent.

Avoid the generic SaaS-card kit: identical rounded cards for every hierarchy,
decorative gradients, tracked-out all-caps labels, a middle-dot beside every
metadata value, and scattered fade-in animations. Structural devices must carry
information.

The single memorable motif is the **signal rail**: a restrained vertical accent
on active navigation and on the left edge of the focused work surface. It is not
applied to every card.

## 2. Color tokens

Semantic tokens, not raw hex values, are used by components. The values below are
the starting palette; Slice 2 verifies contrast in both themes and may refine
values without changing their meaning.

| Token | Light | Dark | Use |
|---|---|---|---|
| `--canvas` | `#eef2f7` | `#07111f` | Application background |
| `--surface` | `#ffffff` | `#0d1a2b` | Primary work surface |
| `--surface-raised` | `#f8fafc` | `#132235` | Popovers, inspectors, elevated panels |
| `--surface-sunken` | `#e3eaf3` | `#091524` | Table headers, inset areas |
| `--border` | `#c9d5e3` | `#26384f` | Dividers and control borders |
| `--border-strong` | `#9fb0c7` | `#3b526f` | Emphasis boundaries |
| `--text` | `#0b1728` | `#e7eef8` | Primary text |
| `--text-muted` | `#52647b` | `#9fb0c7` | Secondary text and metadata |
| `--text-inverse` | `#ffffff` | `#07111f` | Text on accent |
| `--accent` | `#0f766e` | `#2dd4bf` | Interactive primary |
| `--accent-hover` | `#115e59` | `#5eead4` | Hover/active primary |
| `--attention` | `#b45309` | `#fbbf24` | Needs attention, warning |
| `--danger` | `#b91c1c` | `#f87171` | Critical failure, destructive |
| `--success` | `#15803d` | `#4ade80` | Verified success |
| `--info` | `#0369a1` | `#38bdf8` | Informational |
| `--focus` | `#0e7490` | `#67e8f9` | Focus ring |
| `--overlay` | `rgb(7 17 31 / 0.48)` | `rgb(0 0 0 / 0.62)` | Modal scrim |

Semantic state backgrounds use the state color at low opacity plus a solid
border/text token. Never indicate state by color alone; include a label, icon or
shape. `ACCEPTED`, `PASS`, `TRUSTED_LOCAL`, `UNKNOWN`, `NOT_TESTED`,
`DISCONNECTED`, `STALE` and `BLOCKED` are distinct labels.

## 3. Typography

- UI: IBM Plex Sans Variable, self-hosted, subject to pinned dependency and
  license validation in Slice 2; fall back to the system UI stack if the
  self-hosted package cannot be justified.
- Data/code: IBM Plex Mono, self-hosted, same validation; fall back to
  `ui-monospace` if necessary.
- Two families maximum. No external CDN font requests.
- Type scale: display 28/36, title 20/28, subheading 16/24, body 14/22, small
  12/18, data 13/20.
- Weights: 400 body, 500 controls/labels, 600 headings, 700 only for strong
  emphasis.
- Number columns and comparisons use `font-variant-numeric: tabular-nums`.
- Use `text-wrap: balance` for headings and `text-pretty` for prose.
- Line length for reading surfaces should stay below 80 characters.
- Code identifiers, hashes, commit SHAs and session IDs use the mono family and
  break safely.

## 4. Spacing, shape and layout

- Spacing scale: 4, 8, 12, 16, 20, 24, 32, 40, 48, 64 px.
- Radius: 4 px controls, 6 px panels, 8 px overlays, 12 px only for major
  surfaces. Do not pill every metadata item.
- Elevation: no decorative shadows. Use borders and surface contrast; a light
  shadow is reserved for overlays and popovers.
- Application shell: 248 px expanded sidebar, 64 px collapsed; 56 px top bar;
  content max width 1600 px with 24 px gutters; 360 px inspector/drawer.
- Data density: comfortable 40 px rows, compact 32 px rows. Density is a user
  preference, persisted; it does not change information or touch targets below
  the accessible minimum.
- Tables use sticky headers where useful, tabular numerals, explicit empty
  states and a responsive card/list fallback rather than clipped columns.
- The shell must remain usable without a mouse: skip link, landmarks, logical
  focus order and a visible focus ring.

## 5. Navigation and page structure

- Persistent collapsible sidebar grouped by operator task: Overview, Work
  (Actions, Campaigns, Agent Office, Issues, Docs), Operations (Decision Inbox,
  Runtime, Settings).
- Project/workspace context is visible and switchable where the Core supports
  it; do not imply a project scope that the API does not enforce.
- Contextual page header with breadcrumbs, status and primary action.
- Runtime health and Decision Inbox entrypoints are always discoverable.
- Global command/search navigation lists only implemented destinations and
  actions; no fake commands.
- Stable URLs, deep-linkable filters/pagination and back/forward reconstruction.
- Authenticated account/lock entry; no implied multi-user account system.

## 6. Core components

| Component | Required states and rules |
|---|---|
| Button / IconButton / LinkButton | primary/secondary/quiet/destructive; loading; disabled with reason; icon-only accessible name; no action without a Core operation |
| Input / Select / Combobox / Checkbox / Switch | label, help, error, required, disabled, keyboard operation, autocomplete where appropriate |
| Form validation | inline error next to the field, a summary for multi-error forms, focus moved to the first error, no silent submission |
| Table | sorting/filtering, pagination, empty/loading/error, row actions, keyboard access, responsive fallback |
| Card / Panel / Inspector | one level of hierarchy; no identical card for every content type |
| Tabs / SegmentedControl | keyboard arrow navigation, visible selected state, URL-backed where material |
| Dialog / Drawer / ConfirmDialog | focus trap, escape/cancel, focus restoration, consequence text, destructive action separated |
| Badge / Status | label plus semantic color; unknown/not-tested/blocked distinct; never color-only |
| Callout / InlineAlert | info/attention/danger; action; `aria-live` for asynchronous changes |
| CommandPalette | implemented commands only, keyboard-first, empty state |
| Timeline / EventList | durable sequence, cursor/replay awareness, redacted detail, no fabricated activity |
| Code/DiffViewer | monospace, line wrapping control, file headers, copyable identifiers, no unexpected HTML; the Campaign view exposes a bounded per-file unified diff with explicit binary, large, non-file, truncated and unavailable states |
| WorkGraph | deterministic dependency layout, parallel sibling lanes, exact recorded status plus explicitly derived ready/waiting/blocked states, keyboard node selection, accessible inspector and authoritative table alternative |
| Board | keyboard move menu, drag with pointer, no drag-only path, explicit column semantics |
| EmptyState | explains why, offers a real next action, no decorative illustration dependency |
| Loading / Skeleton | bounded, no layout jump, cancellation-safe |
| Offline / Reconnecting | last-loaded time, cursor/replay status, never imply an agent stopped |

## 7. Accessibility

Target WCAG 2.2 AA:

- semantic landmarks, one page `h1`, hierarchical headings and a skip link;
- visible `:focus-visible` ring with sufficient contrast and offset;
- keyboard operation for every interactive element, including board moves,
  tabs, menus, dialogs and graph selection;
- controls labeled; icon-only buttons named; errors linked to fields;
- asynchronous status announced with `aria-live` without stealing focus;
- reduced motion honored; no essential information in motion or color alone;
- target sizes appropriate for touch; no inaccessible hover-only actions;
- contrast verified in both themes for text, controls, focus and state labels;
- tested at desktop, tablet and narrow widths.

Automated axe checks are a floor, not proof. Record the keyboard and visual
review.

## 8. Data and interaction truthfulness

- Every metric links to the authoritative Core projection or durable event.
- Unknown, unavailable, stale and disconnected values are displayed truthfully.
- A model response is not acceptance; a fixture run is not a live provider run;
  an `ACCEPTED` gate is bound to the exact candidate.
- Campaign authorization, Run execution and Quality Gate outcome are distinct
  facts, not one status pill.
- Agent views show recorded participants, sessions and bounded tool events; no
  hidden reasoning, invented thoughts or simulated activity.
- Credentials and tokens are never rendered, persisted in browser storage or
  included in screenshots/telemetry.
- A destructive action requires an explicit consequence and confirmation; a
  paid or externally effectful action is never silently retried.
- Buttons without a real authenticated Core operation are not rendered; show an
  explicit unavailable state and register the missing capability.

## 9. Motion and theming

- Motion: 120 ms for state feedback, 180 ms for panels, 240 ms for larger
  transitions. Prefer opacity/transform; do not animate layout properties.
- `prefers-reduced-motion: reduce` disables non-essential transitions.
- Theme: system by default, explicit light/dark override persisted; set
  `color-scheme` and a background color before first paint to avoid flash.
- Density: comfortable and compact, persisted; both preserve accessible target
  sizes and reading order.

## 10. Responsive and testing conventions

- Verify at 1440 px desktop, 1024 px and 834 px tablet, and 390 px narrow
  mobile.
- Collapse the sidebar to a drawer or icon rail; do not hide primary navigation
  without an accessible alternative.
- Tables become cards or a horizontally scrollable region with a clear first
  column; never clip actions.
- Test loading, empty, error, offline, permission, long-content and destructive
  states.
- Playwright covers primary journeys against the real Core/Compose stack; axe
  covers automated accessibility; screenshots cover major states.
- Component tests cover tokens, variants, focus, keyboard behavior and state
  transitions.

## 11. Implementation stack decision

- Keep React 19.3.1, TypeScript 5.9.3, React Router 7.18.4 and Vite 7.3.7.
- One accessible primitives library: Radix UI Dialog and Tooltip, pinned, with
  custom CSS tokens. Do **not** add Tailwind/shadcn on top of the custom token
  system in the same slice; that would be a second styling stack.
- Icons: Lucide React 1.62.0, one family.
- Typography: self-hosted IBM Plex Sans Variable and IBM Plex Mono 5.3.0.
- Component tests: Vitest 5.0.5 + Testing Library 16.3.3 + jsdom 30.1.3.
- Data tables: TanStack Table when sorting/filtering/virtualization justifies
  it; use native tables for simple read-only projections. Not installed yet.
- WorkGraph: a custom CSS-positioned node canvas with an SVG edge layer is
  implemented for the 1–8 unit bound, with keyboard movement and a table
  alternative. React Flow is not installed. No scheduling progress is inferred
  from graph position.
- Storybook is a component workshop, not product acceptance; evaluate it in the
  design foundation or add it only when component isolation justifies the cost.
- Playwright + axe for real browser acceptance; visual regression only when the
  foundation is stable. Axe/visual regression are later slices.

## 12. References

- [Factory Office contract](factory-office.md)
- [Office-first configuration](office-configuration.md)
- [Office parity matrix](office-parity.json)
- [Factory Office design skill](../../.agents/skills/factory-office-design/SKILL.md)
- [Office feature parity skill](../../.agents/skills/office-feature-parity/SKILL.md)
- [ADR-0025](../architecture/decisions/ADR-0025-container-native-and-office-parity.md)
