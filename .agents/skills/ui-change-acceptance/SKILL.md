---
name: ui-change-acceptance
description: Prove every new or changed Factory Office route, control, form, state and action with a per-option browser and component matrix, responsive and accessibility checks, visual review and candidate-bound evidence.
---

# UI change acceptance

Use this skill for every change that adds, moves, renames, removes or changes the
behavior of a user-facing Office route, page, panel, tab, dialog, drawer, form,
field, button, link, filter, toggle, empty state, loading state, error state,
offline state, recovery flow or operator action.

A route smoke test is not acceptance of a page. A screenshot is not acceptance of
an interaction. "It renders" is not acceptance of a workflow.

## 1. Define the change boundary

Record:

- the user job and the exact route(s);
- every added, changed or removed control and its Core operation, or explicitly
  `no operation` for navigation/display-only controls;
- the exact candidate commit and environment;
- the data fixtures needed to reach ready, empty, loading, stale, offline,
  unauthorized, error, blocked, failed and completed states;
- the desktop, tablet and narrow widths that will be exercised;
- any state that cannot be reached with the available environment and why.

Do not begin with a screenshot. Begin with the control inventory.

## 2. Build the per-option matrix

Create a table with one row per option. At minimum:

| ID | Route / surface | Control or state | Expected result | Evidence | Result |
|---|---|---|---|---|---|
| UI-001 | `/route` | primary action | exact Core call, state transition, confirmation | test/screenshot id | PASS / FAIL / NOT_TESTED |

Every control must have an expected result that is observable in the UI or in a
Core projection. For destructive or authority-changing controls, include the
cancel path and the confirmation path separately.

Cover, where applicable:

- primary and secondary buttons, links, icon buttons and row actions;
- tabs, disclosures, drawers, dialogs and keyboard close/Escape;
- every form field: valid input, missing input, malformed input, maximum length,
  server rejection and retry;
- selectors and filters, including URL persistence and back/forward/reload;
- loading, empty, stale, disconnected, unauthorized, forbidden, conflict,
  blocked, failed and completed states;
- double-submit and concurrent-click guards;
- keyboard-only order, focus return, visible focus and screen-reader names;
- narrow widths where controls move into drawers or scroll regions;
- light and dark themes, comfortable and compact density when supported;
- English and Spanish labels for every new visible string.

A control that has no reachable Core operation is a defect. A control that only
changes local state to fake progress is a defect. A success state that is not
backed by a Core projection is a defect.

## 3. Exercise it in a real browser

Use the pinned Playwright/Chromium harness against the canonical Compose stack
when the change is part of the installed product. Use component tests for
exhaustive branches, but do not substitute them for browser evidence on the
critical journey.

For each matrix row:

1. Navigate by the same path a human would use; do not inject React state.
2. Click, type, select, drag or use the keyboard as the UI exposes it.
3. Capture the observable result and the relevant Core response.
4. Capture a screenshot only for visual/state evidence; name it by matrix ID.
5. Record console errors, page errors, unexpected 404/5xx responses and
   horizontal overflow.

Native OS dialogs, secure browser surfaces and third-party screens that cannot
be automated must be marked `NOT_TESTED` or replaced with a documented stub that
still exercises the real upload/operation boundary. Never call a stubbed native
picker a real picker test.

## 4. Review the experience, not only the wiring

Apply the [Office design system](../../../docs/product/office-design-system.md),
the [Office UI standard](../../../docs/product/office-ui-standard.md) and the
[factory-office-design skill](../factory-office-design/SKILL.md). Review:

- hierarchy: can the owner tell what this page is for in five seconds?
- primary action: is it obvious, singular and truthful?
- language: human nouns, objectives and states before machine identifiers;
- density: is the page scannable, or is it a wall of internal objects?
- feedback: does every action show pending, success, failure and recovery?
- error quality: can the owner fix the problem without reading logs?
- empty quality: does the empty state explain the next real action?
- mobile quality: does the layout remain usable at 320/390 px with touch targets?
- consistency: spacing, typography, badges, colors and control placement match
  the rest of Office;
- accessibility: contrast, focus, labels, roles, live regions and reduced motion.

Record visual and usability findings even when the control works. A working
control with a confusing label is still a finding.

## 5. Evidence

Store a candidate-bound manifest under `docs/evidence/<slice>-<date>/` with:

- candidate SHA, environment and browser version;
- matrix rows and results;
- screenshots by matrix ID;
- console/network findings;
- explicit limitations and `NOT_TESTED` rows;
- whether the run is implementer self-test or independent acceptance.

Never claim "all options tested" unless the matrix enumerates all options in the
change boundary and every row has evidence.

## 6. Stop conditions

Stop and report if:

- a visible control has no authenticated Core operation;
- a state cannot be distinguished from another state;
- the only evidence is a mock, a screenshot without interaction, or a host-only
  run for a container-native product;
- the change removes or renames a route without a redirect/not-found decision;
- accessibility, mobile layout or error recovery is knowingly broken;
- the matrix is incomplete but the change is being described as accepted.

## 7. References

- [Office feature parity](../office-feature-parity/SKILL.md)
- [Factory Office design](../factory-office-design/SKILL.md)
- [Feature removal](../feature-removal/SKILL.md)
- [Browser automation](../../../docs/engineering/browser-automation.md)
- [Quality baseline](../../../docs/quality/baseline.md)
