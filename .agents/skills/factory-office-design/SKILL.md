---
name: factory-office-design
description: Design, build and review Factory Office as a professional engineering command center with one accessible design system, truthful Core-backed state and stable application navigation.
---

# Factory Office design

Use this skill for any change to Office routes, navigation, visual language,
shared components, tables, forms, dashboards, timelines, graphs, boards,
inspectors or responsive behavior. It defines product design guidance; Core
remains the authority for state, authorization and acceptance.

## 1. Product job

Factory Office is the primary place the Product Owner runs an autonomous
engineering organization. It must answer, without raw API calls or terminal
inspection:

- What is running, blocked, waiting for me, or recently accepted?
- Which Project, Action, Campaign, Run, Attempt, agent, candidate and evidence
  are involved?
- What can I safely do now, and what authority does that action consume?
- Is the information live, stale, unknown, disconnected or unavailable?
- Where is the source evidence for every claim?

The Office is not a generic SaaS dashboard, a marketing page or a collection of
unrelated admin forms. It is an operations workspace: dense, calm, scannable and
fast.

## 2. Information architecture

Organize around the operator's tasks, not internal domain types:

- **Overview** — active work, agents, decisions, blocked work, accepted
  candidates, gate outcomes, budgets, runtime health and meaningful events.
- **Decision Inbox** — actionable approvals, interventions and unresolved
  operational issues, separated from informational notifications.
- **Actions** — list, filters, create and detail with specification, scope,
  route, risk, campaigns, runs, candidates, evidence and delivery status.
- **Campaigns** — list and detail with WorkGraph, timeline, participants,
  attempts, candidates, budgets, receipts and gate.
- **Agent Office** — real recorded participants in a visual, table or timeline
  view with an inspector.
- **Issues** — a governed intake/board foundation tied to Actions and Campaigns.
- **Docs** — repository-backed, versioned project documentation with provenance.
- **Runtime & Settings** — providers, models, credentials, budgets, workers,
  cleanup, recovery and policy.
- **Global search/command navigation** — only implemented destinations and
  actions.

Use a persistent, collapsible application shell with project/workspace context,
breadcrumbs, page headers, health, inbox and account/lock entrypoints. Stable,
deep-linkable routes and URL-backed filters are required. Do not use a hash-only
router when the Core static serving path can be separated safely.

## 3. Visual language

Direction: enterprise engineering workspace, modern, restrained and technically
precise.

- Slate/navy foundations with neutral work surfaces.
- A controlled teal/cyan interactive accent.
- Amber for attention, semantic red for critical failure, green for verified
  success and blue for information.
- Compact but readable operational data; tabular numerals for numeric columns.
- Light and dark themes, following `prefers-color-scheme` with an explicit
  operator override.
- Comfortable and compact density modes.
- Minimal decorative motion; motion must explain a state change and respect
  `prefers-reduced-motion`.
- Strong visible focus, keyboard navigation and target sizes.

Define tokens for color, typography, spacing, radius, elevation, density and
motion in one source. Components consume tokens; do not scatter arbitrary hex
values, pixel gaps or transition strings across views. Use one accessible
primitives library and one icon family; do not install overlapping component
systems.

## 4. Component expectations

Shared components must cover:

- application shell, sidebar, top bar, breadcrumbs, page header;
- buttons, icon buttons, links, command palette, menus, tooltips;
- inputs, selects, comboboxes, checkboxes, switches, validation and help text;
- tables with sorting/filtering, empty states and responsive behavior;
- cards, panels, inspectors, tabs, drawers, modals and destructive
  confirmations;
- badges and semantic state labels;
- loading, skeleton, empty, offline, reconnecting, partial and error states;
- timelines, event lists, code/diff viewers and WorkGraph canvases;
- accessible drag-and-drop boards with a keyboard/tap equivalent.

Do not create a second version of an existing component because a page needs a
small variation. Extend the shared component with a documented variant.

## 5. Truthful data and authority

- Every displayed metric, agent, session, tool event, budget and status must come
  from a Core projection or a recorded durable event.
- Never invent agent thoughts, hidden reasoning, tool calls, activity, progress,
  cost or acceptance.
- Distinguish `UNKNOWN`, `NOT_TESTED`, `DISCONNECTED`, `STALE`, `BLOCKED`,
  `REPAIR_REQUIRED` and `ACCEPTED` explicitly. A disconnected view does not mean
  an agent stopped.
- Never show `ACCEPTED` unless the Core Quality Gate recorded `ACCEPTED` for the
  exact candidate. A model response or deterministic fixture is not automatic
  product acceptance.
- Show campaign authorization, run execution and gate outcomes as distinct
  facts. Do not collapse them into one ambiguous label.
- Every button must invoke an existing authenticated Core operation. If the
  operation is absent, render an explicit unavailable state and register the
  capability; do not create a decorative button.
- The browser never writes SQLite, controls Workers, reads provider credentials
  or acts as an authorization bypass.

## 6. States, errors and recovery

Every primary workflow needs deliberate:

- loading and cancellation behavior (no stale optimistic approval);
- empty states with a meaningful next action;
- offline/reconnecting state using cursor/sequence replay, not invented history;
- field-level validation and a clear fix path;
- destructive confirmation with an explicit consequence and no accidental
  approval;
- permission/authentication failure with a safe recovery;
- partial data and unknown values with provenance;
- redaction of credentials, tokens, prompts and sensitive values;
- route reconstruction after reload, back/forward and deep links.

Do not render a success state until the authoritative Core response confirms
it. Do not silently retry a paid or externally effectful action.

## 7. Accessibility and responsive behavior

Target WCAG 2.2 AA:

- semantic landmarks, headings and a skip link;
- visible `:focus-visible` treatment and logical focus order;
- labels for every control and accessible names for icon-only buttons;
- `aria-live` for asynchronous status that changes outside focus;
- keyboard equivalents for drag, hover and context-menu actions;
- no drag-only or color-only state;
- contrast and non-text contrast verified in both themes;
- reduced-motion behavior;
- test at desktop, tablet and narrow mobile widths;
- no horizontal overflow, clipped actions or unreachable content.

Automated accessibility checks are necessary but not sufficient; exercise the
journey with the keyboard and inspect the rendered states.

## 8. Testing and review

- Component tests for tokens, variants, states and keyboard behavior.
- Playwright journeys against the real Core/Compose stack for primary flows.
- Axe checks plus manual keyboard/focus review.
- Screenshots for major states at desktop, tablet and narrow widths.
- Visual regression for the design foundation where stable.
- Console errors, failed requests, 404s and broken links are acceptance
  failures, not cosmetic warnings.
- Storybook/mocks may support component development but cannot prove product
  readiness.

Apply the [office-feature-parity skill](../office-feature-parity/SKILL.md) for
the capability-to-Office impact matrix.

## 9. Stop conditions

Stop and report when a design requires invented backend state, a fake approval,
a credential in the browser, an unapproved external resource, a host-only
browser/tool dependency, an inaccessible drag-only interaction, or a claim that
exceeds recorded evidence.

## 10. References

- [Factory Office contract](../../../docs/product/factory-office.md)
- [Office-first configuration](../../../docs/product/office-configuration.md)
- [Office parity matrix](../../../docs/product/office-parity.json)
- [Design system documentation](../../../docs/product/office-design-system.md)
- [Container-native engineering](../container-native-engineering/SKILL.md)
- [Factory browser](../factory-browser/SKILL.md)
