# Home visual-polish design audit — first prototype (2026-10-29)

**Status: FIXTURE-ONLY PROTOTYPE. NOT VISUAL ACCEPTANCE. NOT LIVE CORE.**
This note compares the real Office Home at base `2907b17` with the two
Owner-approved style targets and records the token/component changes in this
slice. Issue #62 remains open until the Product Owner accepts or redirects the
visual direction.

Inputs inspected directly (not only their captions):

- `docs/product/reference/factory-office-visual-reference-pack-20261028/02-layouts-office-referencia.png`
- `docs/product/reference/factory-office-visual-reference-pack-20261028/03-home-campanas-layout-referencia.png`
- `baseline-home-1440.png` and `baseline-home-390.png` — the same fixture Home
  before this slice, captured from base `2907b17`
- the current source in `office/src/styles/` and `office/src/views/Home.tsx`

The reference collages contain fake status, activity and sample projects. They
are style targets only. No number, name or activity from them is rendered in
the product.

## 1. Comparison with the current baseline

| Dimension | Baseline at `2907b17` | Reference 02/03 direction | First-slice decision |
|---|---|---|---|
| Shell | Light sidebar and light top bar; shell and content share the same surface family. Teal is the only accent. | Deep-navy rail and top chrome against a spacious light content surface; blue primary action and turquoise product accent. | Added `--shell-*` semantic tokens, a navy rail/top bar, turquoise active rail and brand mark, and kept content light. |
| Hierarchy | Visible `Home` page title, description, meta line, `New request` kicker, composer heading and form help all compete inside one column. | The instruction composer is the first and dominant surface; page chrome is quiet. | Removed the visible page header from Home; one visually hidden `h1` remains. The composer heading is the only visible hero. |
| Controls | 34 px normal buttons, 36 px inputs, mostly 6 px radii, flat hover. | Comfortable 40–44 px actions, 10–14 px radii, clear selected/hover/focus/pressed states. | Buttons are 40 px (44 px large primary), inputs 40–42 px, radii 10/12/16 px, with hover lift, pressed reset, disabled opacity and focus rings. |
| Primary action | Teal `Submit request`, same visual weight as most controls; help text sits beside it. | One obvious blue primary action at the end of the composer. | 44 px blue primary with `Send` icon, spinner + `aria-busy` while Core creates the request, and a short intent note. |
| Home composition | Project/profile selects, profile route table, spend card with two explanations, approval callout and submit hint are all expanded. Recent requests, decisions and Core status are equal-weight panels. | Prompt, compact context selectors, one explicit authorization, recent work and decisions as lighter secondary surfaces. | Composer dominates; Project/profile are compact selects; profile routes collapse into a disclosure; spend is one compact control with one short sentence; recent work, decisions and status keep all Core facts but use lighter section headers and rows. |
| Copy density | Repeated explanations about Core, plans, profiles and spend. | Short contextual help; the composer carries the instruction. | Removed the objective help, Project help, profile help and spend note from the visible composer. Remaining approval/spend copy is one sentence each. |
| Empty/error/loading | Dashed `EmptyState` boxes and full-width callouts that look like additional forms. | Quiet secondary empty states; errors are visible but not structural. | Solid low-contrast empty states with an icon tile; callouts stay inline; loading is a compact status line. |
| Status semantics | `HEALTHY`/`UNHEALTHY` pill, four metric cells, snapshot and two links in a panel. | Status is secondary and compact. | Kept every Core fact and link; grouped metrics into a 2×2 inset grid with smaller headings and a section icon. |
| Mobile shell | Light top bar with menu, route title, health, search, language, theme, density and lock; theme/density leaked through the old CSS precedence. | Compact top bar; navigation and preferences remain reachable. | Deep-navy mobile top bar; theme/density move into the drawer footer at ≤640 px instead of disappearing; menu, route, health dot, search, language and lock remain. Rail becomes a drawer. |

## 2. Token and component changes in this slice

- **Semantic shell/content tokens**: `--shell-bg`, `--shell-bg-hover`,
  `--shell-border`, `--shell-text`, `--shell-text-muted`, `--shell-active-bg`,
  `--shell-accent`, `--shell-mark-bg/text`, `--focus-inverse`,
  `--content-glow`, `--surface-inset`, `--text-subtle`, `--accent-contrast`,
  `--accent-teal`, shadow and control-height tokens.
- **Type**: display clamp 28–36 px for the Home composer, tighter heading
  tracking, 11 px metadata labels, consistent 1.5–1.6 line height.
- **Shape/elevation**: 6/10/12/16 px radii; 40 px normal and 44 px primary
  controls; one-pixel panel border plus a wide, very soft ambient shadow. No
  glassmorphism, noise, glow text or gradient stacks.
- **Button**: primary/secondary/quiet/danger, `sm/md/lg`, hover lift, pressed
  reset, disabled reason via native `disabled`, `busy` spinner and
  `aria-busy`, visible focus.
- **Field**: 40 px inputs/selects, 12 px textarea padding, hover/focus border,
  3 px focus ring, invalid ring, disabled surface, `labelHidden` for the
  composer prompt.
- **Surface**: 12 px panel radius and soft elevation; solid low-contrast empty
  state; compact loading; callouts align an icon, copy and action.
- **Tabs/Badge**: clearer selected tab, tab focus, smaller badges with semantic
  borders.
- **Shell/nav**: 260/72 px rail, 60 px top bar, turquoise 3 px active rail,
  navy active fill, shell-specific focus ring, mobile drawer scrim, continuous
  navy underlay on long pages.
- **Home**: composer-first layout, compact Project/profile selects, profile
  route disclosure, one explicit USD inference authorization, one approval
  note, lightweight recent work, decisions and Core status.

## 3. Deliberately not copied from the reference

- The isometric Agent Office scene and its decorative room composition.
- Fake projects, metrics, activity, model latency, review statuses or budget
  figures.
- The full campaign-detail, projects, issues and settings collages. This slice
  only propagates tokens/primitives to the shell and Home.
- Suggestion chips and account/avatar footer. Both are plausible next steps,
  but they either add copy density or imply identity data the shell does not
  currently own.
- A wholesale font or framework migration. IBM Plex Sans remains the approved
  font stack; no Tailwind/SCSS was introduced.

## 4. Accessibility and responsive checks in this prototype

- 320/390/834/1440 px Chromium captures with no horizontal document overflow;
  see `overflow-report.json`.
- Focus ring on the content (`#2563eb` on white) and on the navy shell
  (`#7dd3fc`). Primary blue with white text is 5.17:1; shell muted text is
  6.7:1+ against `#0a1a2f`; accent text on `--accent-soft` is 4.65:1 after
  lightening the tint.
- Labeled controls, visible focus, keyboard-operable native selects and
  disclosure summaries, and `aria-busy` on the in-flight button.
- Global reduced-motion rules are unchanged; the new hover lift is a
  transform/box-shadow transition and is disabled by the existing
  `prefers-reduced-motion` rule.
- One `h1` per page: the shell's duplicate visually hidden `h1` was removed;
  Home owns a hidden `h1` and the composer owns `h2`.
- Empty Project and empty recent-work states remain truthful; the fallback
  screenshot is in `home-1440-empty-projects.png`.

## 5. Visual gaps for the Owner to judge

1. **No right-hand product scene.** The reference fills the right side with the
   Agent Office. Home intentionally does not render that expensive scene, so
   the 1440 composition has more whitespace after the status panels.
2. **No owner/account block in the rail.** The reference has a named owner
   card. The shell does not currently receive a display identity, so adding one
   would invent data.
3. **Mobile header differs.** Reference 03 shows a compact brand + project
   chip. This slice keeps the route title and moves theme/density into the
   drawer; it does not add a global Project switcher to the top bar.
4. **Selector truncation.** Long profile labels truncate in native selects at
   320–390 px (for example `predeterminado global · predet…`). The accessible
   name and selected value remain complete.
5. **Expanded spend envelope.** Core requires an explicit amount, so it remains
   visible rather than behind a disclosure. It is one compact row, but still
   more prominent than the reference's collapsed authorization affordance.
6. **Other screens.** Projects, Campaigns, Settings and Agent Office inherit
   the token/primitive changes but keep their existing composition.
7. **Evidence level.** No real Core/Compose browser run, live inference,
   independent verification or visual regression is claimed. `HEALTHY` and
   request objectives remain machine/Core prose in Spanish.

## 6. Product Owner decision requested

Please choose one:

- **A. Accept the direction** — navy shell, light airy content, blue primary,
  turquoise product accent, composer-first Home — and authorize propagation to
  Projects, Campaigns and Settings in a later slice.
- **B. Redirect before propagation** — for example keep the teal primary,
  restore a visible Home page title, add a mobile brand/project chip, or move
  the spend control behind a compact authorization disclosure.

This prototype does **not** close #62 and does not claim visual acceptance.
