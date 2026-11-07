# Factory Office UI standard

**Status:** Normative internal standard, version 1.0, 2026-11-06. The visual
direction was requested by the Product Owner on 2026-11-06; formal visual
sign-off of the running revision remains recorded separately.
**Visual direction:** Product Owner-provided reference pack in
[`reference/factory-office-visual-reference-pack-20261028`](reference/factory-office-visual-reference-pack-20261028/README.md).
**Scope:** every Factory Office route, shell change, shared component, visual
state, responsive layout and browser acceptance artifact.

This standard exists so Factory Office keeps one recognizable product identity
while the underlying capabilities grow. It is not a marketing style guide and it
does not replace Core authority, the parity matrix or the security policy.

## 1. Product job and authority

Factory Office is the operator's engineering command center: a calm, dense
workspace to start requests, follow campaigns, inspect evidence, supervise
agents, resolve decisions and configure the Factory without raw API calls or a
terminal.

- **Core remains authoritative.** Every status, count, agent, progress value,
  finding, gate outcome and cost comes from a Core projection or durable event.
- **No invented state.** Never fabricate agent thoughts, activity, progress,
  budgets, acceptance or completion. A fixture or mock may support visual
  development but must be labeled and must not be presented as product evidence.
- **Every action is real.** A button must invoke an existing authenticated Core
  operation. If the operation does not exist, show an explicit unavailable state
  and register the missing capability in the parity matrix.
- **One Office experience.** Backend-only capabilities are not complete; see the
  [office-feature-parity](../.agents/skills/office-feature-parity/SKILL.md) rule.

## 2. Reference hierarchy

Use the approved references in this order:

1. [`01-office-isometrica-referencia-principal.png`](reference/factory-office-visual-reference-pack-20261028/01-office-isometrica-referencia-principal.png)
   — Agent Office scene, dark shell, compact overlays and warm studio.
2. [`02-layouts-office-referencia.png`](reference/factory-office-visual-reference-pack-20261028/02-layouts-office-referencia.png)
   — broad layout language: navy navigation, airy light canvas, blue/turquoise
   accents, practical cards and conversational Home.
3. [`03-home-campanas-layout-referencia.png`](reference/factory-office-visual-reference-pack-20261028/03-home-campanas-layout-referencia.png)
   — Home and Campaign composition. It is a direction, not a requirement to
   display every depicted panel or any imaginary value.

Concept-image text, values, progress bars and labels are **not** requirements.
Never copy imaginary numbers or claims from a concept into the running product.

## 3. Non-negotiable visual rules

- Deep-navy application rail and top bar; light, airy content surfaces.
- One controlled blue primary action and a turquoise active/signal accent.
- Neutral borders and surface contrast carry structure; avoid decorative
  gradients, glass effects, stacked shadows and a card for every element.
- Restrained motion only when it explains a state change; honor
  `prefers-reduced-motion`.
- Compact operational density with readable type and tabular numerals.
- The primary navigation is always dark; ordinary content is always light in
  the light theme. Dark mode changes content tokens, not the shell hierarchy.
- An active navigation item uses a filled navy-blue surface plus the turquoise
  signal rail. Do not replace it with a generic card highlight.
- Agent Office is a composed, dynamic scene over real participants, never a
  flattened reference image or a decorative illustration.
- No external CDN fonts, icon packs or overlapping component systems.

## 4. Design tokens

`office/src/styles/tokens.css` is the single source of truth for color, type,
spacing, radius, elevation, density and motion. Components **must** consume
tokens; do not add raw hex values, arbitrary pixel gaps or one-off transition
strings in a view.

| Group | Canonical tokens | Rules |
|---|---|---|
| Canvas/surfaces | `--canvas`, `--surface`, `--surface-raised`, `--surface-sunken`, `--surface-inset` | Content hierarchy uses surface contrast and one-pixel borders. |
| Text | `--text`, `--text-muted`, `--text-subtle` | Use semantic roles, not opacity tricks. |
| Interactive | `--accent`, `--accent-hover`, `--accent-soft`, `--accent-contrast`, `--accent-teal` | Blue is action; teal is active/signal. Do not make both primary. |
| State | `--attention`, `--danger`, `--success`, `--info` + soft variants | Always pair color with a label, icon or shape. |
| Shell | `--shell-bg`, `--shell-bg-hover`, `--shell-border`, `--shell-text`, `--shell-text-muted`, `--shell-active-bg`, `--shell-accent` | The shell owns its own tokens so a theme change cannot erase hierarchy. |
| Focus | `--focus`, `--focus-inverse`, `--focus-soft` | Visible `:focus-visible` on every interactive element. |
| Type | `--font-sans`, `--font-mono`, `--text-*`, `--leading-*` | Two families maximum; identifiers and hashes use mono. |
| Space/shape | `--space-*`, `--radius-*`, `--control-height-*`, `--row-height` | 4/8/12/16/20/24/32/40/48/64 scale; 6/10/12/16 radii. |
| Layout | `--sidebar-width`, `--sidebar-collapsed`, `--topbar-height`, `--content-max`, `--page-gutter` | Shell dimensions are stable across pages. |
| Motion | `--transition-fast`, `--transition-panel` | 140–200 ms; never animate layout properties. |

Theme and density are user preferences stored outside Core state. `system`
follows `prefers-color-scheme`; explicit light/dark and comfortable/compact
overrides persist. Density never reduces a touch target below an accessible
minimum.

## 5. Application shell

### 5.1 Sidebar

- Expanded width **260 px**; collapsed **72 px**; the rail is sticky and
  full-height, with a continuous navy underlay on long pages.
- Brand block at the top: Factory Office mark, wordmark and a collapse control.
- Primary operator navigation, in this order:
  1. **New request** (`/`) — prominent blue entry, active for `/` and
     `/requests/:id`.
  2. **Campaigns** (`/campaigns`).
  3. **Projects** (`/projects`).
  4. **Issues** (`/issues`).
  5. **Agent Office** (`/agents`).
  6. **Settings** (`/settings`).
- **Advanced** disclosure: Decisions, Overview, Actions, Lead, Delegations,
  Runs, Workers, Runtime and Access & Devices. The disclosure is keyboard
  operable and remains open when an advanced route is active.
- Primary group labels are visually hidden; the navigation still has an
  accessible name and grouped semantics.
- Footer: owner/user card with a real session label and a lock action. Do not
  invent a human name when Core only provides a device name.
- Mobile: the sidebar becomes a dismissible drawer with a scrim, focus trap,
  Escape close and focus restoration to the menu trigger.

### 5.2 Top bar

- Height **60 px**, navy, sticky, with a visible health state, workspace
  defaults, page context and global actions.
- Workspace defaults:
  - **Project** selector populated from Core Projects.
  - **Quality profile** selector populated from Execution Profiles.
  - These select the default Project and profile for new requests. They are not
    an authorization boundary; Core still authorizes every request.
- Page context: the current route title. The Factory Office brand lives in the
  sidebar rail; the top bar does not repeat it. Keep the title text used by
  existing tests and deep-link labels.
- Global actions: search/command navigation (`Ctrl/Cmd+K`), language, theme,
  density and settings. The account lock belongs to the sidebar owner card. On
  narrow screens the theme and density controls may move into the drawer, but
  remain reachable. Icon-only actions need accessible names; do not duplicate
  an accessible name between a sidebar destination and a global action (for
  example use "Open settings" for the gear).
- On narrow screens, hide the workspace selects and non-essential controls, but
  keep the menu, page title, health/status and primary action reachable.

## 6. Page templates

### 6.1 Conversational Home (`/`)

**Job:** turn an operator intent into a reviewed Lead request without hiding
authorization.

Required composition:

1. Lead mark and one dominant instruction surface with the title
   “What do you want to build today?” (localized).
2. A large instruction textarea labeled to the Lead.
3. A compact row of suggestion chips (`Fix a GitHub issue`, `Improve
   performance`, `Add an integration`, `Refactor a module`, `Add tests`).
4. Context controls: Project, Quality/Execution Profile and the explicitly
   required Core inference authorization amount. Do not hide a required
   authorization behind a decorative button.
5. One obvious **Send to Lead** action with a send icon and keyboard hint.
6. Secondary two-column surface: **Recent work** and **Needs your attention**
   (the pending-decision list).
7. Right rail: **Lead conversation** with a real state badge, the latest
   recorded message/state, a link to continue, and **Factory status** with
   health, workers, active agents and a real recovery link.
8. Approval boundary note: the Lead plans; the operator approves before work
   starts.

Every value is a Core projection. Empty, loading, unavailable, stale and error
states each need a visible explanation and a real recovery action.

### 6.2 Campaign detail (`/campaigns/:id`)

**Job:** understand and supervise one Campaign without losing the conversation,
plan, agents, changes or evidence.

Required composition:

1. Page title is the Action objective (fallback: Campaign id), with a
   breadcrumb and Campaign status. Description shows the short Campaign id and
   that the page contains plan, agents, changes and evidence.
2. A compact **Operating picture** summary: Project, Action, frozen plan,
   authorization/risk, latest run/attempt/candidate and latest gate.
3. A tabbed working surface. Keep the existing deep-linkable tab ids and add
   labels that match the operator's task; at minimum Overview, Plan/WorkGraph,
   Execution, Changes/Candidates, Evidence, Quality Gate, Budget and Events.
4. A sticky **Campaign pulse** rail at ≥ 1200 px:
   - overall progress from recorded task/gate state;
   - agent rows for Lead, Assessor, Implementer, Semantic Reviewer and Verifier,
     each with a recorded status and role description;
   - latest Quality Gate outcome or an explicit pending state;
   - recent recorded events, not invented activity.
5. On narrow screens the pulse stacks below the working surface; tabs wrap or
   scroll without clipping actions.
6. Candidate changes use the shared diff viewer with explicit unified/split,
   binary, large, non-file, truncated and unavailable states.

Never collapse authorization, Run execution and Quality Gate into one status.
Only show `ACCEPTED` when Core recorded an accepting gate for the exact
candidate.

### 6.3 Agent Office (`/agents`)

- The primary view is the dynamic isometric studio scene over the recorded
  participant projection.
- Every distinct `participant_id` gets exactly one avatar; five implementers are
  five avatars, not one repeated sprite. Role stations, identity-stable slots
  and selection come from the deterministic layout helpers.
- Overlays are compact and readable; the inspector is contextual and optional.
- Provide list and timeline alternatives with the same data, keyboard selection
  and accessible names.
- Animations reflect recorded Core events only. Never animate a thought, hidden
  reasoning or fabricated progress.

### 6.4 Projects, Issues, Execution Profiles

- **Projects:** searchable repository/selection grid with local/GitHub tabs,
  visibility, binding status and one real action per project. Empty and
  permission states explain how to register a Project.
- **Issues:** governed Kanban/list foundation with columns, labels, priority,
  source provenance and a keyboard/tap move equivalent. A board card links to
  the real issue detail; no drag-only workflow.
- **Execution Profiles:** profile cards/table with provider/model matrix,
  executable/draft state, warnings, activation, and the autonomy point policy.
  Selecting a provider/model must use the configured Core catalog; never invent
  a model id. Do not render fixed token, time, retry or subscription-cost caps:
  those are telemetry and model/provider capability (ADR-0034).
- **Merge policy:** project-scoped or global policy from Core, with the
  effective policy and consequence shown before any change.

### 6.5 Mobile

- At ≤ 1000 px, the sidebar is a drawer and the workspace is one column.
- At ≤ 760 px, Home stacks composer → recent/decisions → conversation/status;
  controls become full-width; no horizontal overflow; primary actions remain
  visible.
- Tables use the responsive card/list fallback rather than clipped columns.
- Test at **1440**, **834** and **390** px widths, with keyboard and touch
  targets.

## 6.6 Implementation status (2026-11-06)

Implemented in this revision:

- the navy shell, primary/advanced navigation, owner card and workspace
  Project/Quality context;
- the conversational Home composition, recent work + pending decisions grid and
  Lead conversation/status rail;
- the Campaign objective header and sticky Campaign pulse (progress, agents,
  gate and recorded activity);
- the existing Agent Office isometric scene, Projects, Issues and Request
  workspace on the shared shell and tokens; Execution Profiles also gained
  wrapping tabs, fluid page actions and card-style profile tables below 900 px;
- a fixture layout guard that fails when any visible element escapes the
  1440/834/390 px viewport at
  `office/e2e-browser/office-home-fixture.mjs`, with the report in
  `docs/evidence/office-ui-standard-2026-11-06/layout-overflow-report.json`;
- a real Chromium/Compose journey covering enrollment, Project import, request
  approval, Campaign, Agent Office, Budget and Gate states at 1440 and 390 px,
  recorded in
  [`compose-browser-uat-2026-11-06`](../evidence/compose-browser-uat-2026-11-06/README.md)
  at candidate `bd90e990d31f7110c368b1253da813f729a729e6`;
- this standard and the fixture screenshot baseline in
  `docs/evidence/office-ui-standard-2026-11-06/`.

Intentionally deferred until a real Core operation exists:

- **Attach context** and **Branch (optional)** controls shown in the concept.
  Core intake does not currently accept attachments or branch selection, so the
  implemented Home does not render decorative controls for them.
- A Campaign **Conversation** tab backed by durable intake messages. The
  current Campaign page exposes the recorded operating picture, plan/WorkGraph,
  execution, candidates/changes, evidence, quality gate, budget and events; the
  pulse supplies the agent summary. Do not fake a conversation.
- Product Owner visual sign-off for this revision and live-provider inference.
  The Compose UAT is real-browser evidence over a deterministic fixture
  provider; it does not prove live-model behavior.

Known deviations are design debt, not permission to invent state. Record them
in the PR and the reliability/design backlog.

## 7. Component rules

| Component | Required rules |
|---|---|
| Button / IconButton | primary/secondary/quiet/destructive; busy + `aria-busy`; disabled reason; 40 px normal, 44 px primary; icon-only has an accessible name. |
| Field / Select / Textarea | visible or labeled-hidden label, help, field error, required, disabled, focus ring; no unlabeled control. |
| Panel / Card | one level of hierarchy; use a border and surface contrast; do not wrap every text block in a card. |
| Tabs | keyboard arrow/Home/End; visible selected state; URL-backed where material; wrap or scroll without clipping. |
| Badge / Status | label + semantic token; `UNKNOWN`, `NOT_TESTED`, `STALE`, `BLOCKED`, `REPAIR_REQUIRED` and `ACCEPTED` are distinct. |
| Progress | real numerator/denominator and an accessible `progressbar`; never synthesize movement. |
| Timeline / Activity | durable sequence/date and redacted detail; no fabricated entries. |
| Diff viewer | file headers, unified/split, wrapping control, binary/large/truncated/unavailable states. |
| Empty / Error / Offline | explain why; provide a real next action; preserve last-loaded time and cursor/replay status. |
| Dialog / Drawer | focus trap, Escape/cancel, restoration, explicit consequence for destructive actions. |

Extend a shared component with a documented variant instead of creating a
second component for one page.

## 8. Truthful state and content

- Distinguish `UNKNOWN`, `NOT_TESTED`, `DISCONNECTED`, `STALE`, `BLOCKED`,
  `REPAIR_REQUIRED` and `ACCEPTED` explicitly.
- Token, elapsed time, retry/request count and subscription-notional cost are
  observed telemetry. Do not present them as productivity caps; the automatic
  brake is the no-progress diagnosis breaker. Billable monetary envelopes,
  leases, credentials, egress binding and the Quality Gate remain authority.
- A disconnected view does not mean an agent stopped; show the last observation
  and recovery path.
- Model output is advisory; only Core can authorize, accept or deliver.
- Redact credentials, tokens, prompts and sensitive source. Never put a bearer
  token in browser storage.
- Use sentence case for labels and actions; use mono for identifiers, hashes and
  code; keep both EN and ES catalogs complete.
- Do not copy imaginary concept-image labels or values.

## 9. Accessibility

- WCAG 2.2 AA target: semantic landmarks, one `h1`, heading order, skip link.
- Visible `:focus-visible` treatment with contrast in both themes.
- Keyboard operation for menus, drawers, tabs, boards, graph and diffs.
- `aria-live` for asynchronous state changes; no focus stealing.
- Labels for every control; icon-only buttons named.
- Reduced-motion support; no essential information in color or motion alone.
- Screenshots and browser checks include 1440, 834 and 390 px.

## 10. Change process (required)

Every Office change must follow this order:

1. **Identify the Core projection/action.** If it does not exist, stop and
   register the gap; do not build a decorative surface.
2. **Map the parity row.** Update
   [`office-parity.json`](office-parity.json) and the capability registry/roadmap
   in the same delivery.
3. **Change tokens or shared components first.** A page-specific workaround is a
   review finding. Color/typography/spacing changes land in
   `office/src/styles/tokens.css` and the shared component, not in the view.
4. **Implement the page template** using the shell and shared primitives.
5. **Capture fixture screenshots** with
   `office/e2e-browser/office-home-fixture.mjs` at 1440 and 390 px; the same
   harness asserts no layout escape at 1440, 834 and 390 px. Label screenshots
   as fixtures. Do not claim product acceptance from fixtures.
6. **Exercise the real browser journey** against the real Compose stack,
   including loading, empty, error, permission, reconnection and destructive
   states, at 1440/834/390 px.
7. **Add the PR impact declarations** (`architecture-impact`, `office-impact`,
   `container-native-impact` where applicable) with exact base/head binding.
8. **Review against this document** and the reference pack; record residual
   visual debt explicitly.

The standard is versioned. A new visual direction, shell change or cross-page
layout change requires an updated version of this document and Product Owner
approval. Reference-pack images are direction only; they are never runtime
assets.

## 11. Review checklist

- [ ] Navy shell, light canvas, one blue primary, turquoise signal rail.
- [ ] Primary navigation order and active/deep-link behavior match §5.
- [ ] Project and Quality profile selectors are real Core data; no fake scope.
- [ ] Home uses one dominant composer, suggestion chips, required authorization,
      recent work + decisions, and a truthful Lead conversation.
- [ ] Campaign page shows objective title, working tabs and a recorded pulse.
- [ ] Agent Office renders one avatar per real participant and a list alternative.
- [ ] All states (loading, empty, error, offline, unauthorized, stale) are present.
- [ ] Every button invokes a real Core operation or is explicitly unavailable.
- [ ] 1440/834/390 layout checks pass; 1440 and 390 screenshots captured; no
      console errors, failed requests or 404s in the journey.
- [ ] EN/ES catalogs complete; i18n parity tests pass.
- [ ] Keyboard/focus/reduced-motion/contrast checks recorded.
- [ ] Parity matrix, capability registry/roadmap and evidence updated.

## 12. Implementation map

| Concern | Canonical file(s) |
|---|---|
| Tokens, themes, density | `office/src/styles/tokens.css`, `office/src/theme.tsx` |
| Shell, sidebar, top bar | `office/src/components/AppShell.tsx`, `office/src/styles/layout.css` |
| Workspace context | `office/src/components/WorkspaceContextBar.tsx`, `office/src/workspace.ts` |
| Navigation contract | `office/src/navigation.ts` |
| Home | `office/src/views/Home.tsx`, `office/src/views/HomeThread.tsx`, `office/src/styles/home.css` |
| Campaign detail and pulse | `office/src/views/CampaignDetail.tsx`, `office/src/components/office/CampaignPulse.tsx`, `office/src/styles/views.css` |
| Agent Office | `office/src/views/AgentOffice.tsx`, `office/src/components/office/AgentOfficeIsometric.tsx`, `office/src/styles/agent-office.css` |
| Shared primitives | `office/src/components/ui/*` |
| Fixture screenshots | `office/e2e-browser/office-home-fixture.mjs` |
| Real browser acceptance | `office/e2e-browser/*.mjs`, `scripts/compose-product-browser-uat.mjs` |
| Parity | `docs/product/office-parity.json`, `docs/product/capabilities.yaml` |

## 13. Verification commands

```sh
cd office
npm run typecheck
npm run lint
npm test
npm run build
cd ..
CHROMIUM_EXE=/path/to/chrome \
  node office/e2e-browser/office-home-fixture.mjs
python3 .github/scripts/check-office-parity.py --root .
cargo run --locked --bin factory-architecture-check -- --root .
cargo run --locked --bin factory-repo-hygiene -- --mode strict
```

Real browser acceptance is separate from these commands and must use the real
Compose topology. Fixture screenshots are design evidence only.
