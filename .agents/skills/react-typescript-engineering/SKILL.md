---
name: react-typescript-engineering
description: Implement and review Factory Office React 19 and strict TypeScript changes with explicit state, disciplined effects, accessible interaction, truthful Core-backed UI, i18n, responsive layout and browser-level tests. Use for any component, hook, form, data fetch, route, dialog or Office test change.
---

# React and TypeScript engineering

This skill is the procedural companion to the Office design system and the
UI-change acceptance skill. It is advisory: it cannot grant authority, accept a
candidate or replace real browser evidence.

## 1. Model the state before the JSX

Name:

- the Core projection or operation the surface represents;
- server state (authoritative) vs local UI state (transient);
- loading, empty, stale, unauthorized, offline, error, blocked, failed and
  completed states;
- the exact action and its confirmation/undo path;
- what must survive reload/back/forward.

Never invent progress, success, acceptance or a metric. A visible value must
come from Core or be an explicit local input.

## 2. Prefer derived values over effects

- Derive renderable values during render; do not mirror props into state.
- Use `useEffect` only to synchronize with an external system (subscription,
  timer, browser API, network).
- Do not use an effect to compute state that can be derived.
- A fetch effect must handle cancellation with `AbortController` or an `active`
  flag, and must not set state after unmount.
- Avoid `useEffect` chains; move orchestration into an event handler or a
  purpose-built hook.
- Do not suppress `react-hooks/exhaustive-deps` without a written reason.

## 3. Async and event handlers

- A promise returned to `onClick`/`onSubmit` must be explicitly handled:
  `void handler()` with internal error handling, or an awaited wrapper.
- Prevent duplicate submission with an explicit busy state and disabled action.
- Show pending, success and failure; keep the error actionable.
- Do not let a rejection escape as an unhandled promise.
- Use idempotency keys for mutating Core calls where the contract requires them.

## 4. Forms

- Every input has a label or accessible name.
- Validate on the client for usability; never as a substitute for Core
  validation.
- Preserve input on server rejection; show the field and the reason.
- Handle maximum length, malformed input, empty, conflict and retry.
- Use the correct `type`, `autoComplete`, `inputMode` and `name`.
- A destructive action needs an explicit consequence confirmation.

## 5. Accessibility

- Prefer native elements and visible text. Do not put `onClick` on a `div`.
- Every icon-only control has an accessible name.
- Dialogs trap focus, restore focus on close and close on Escape.
- Do not use `autoFocus` unless the workflow requires it; document why.
- Do not use positive `tabIndex`; keep DOM order equal to reading order.
- A scrollable code/diff region may be focusable; document the exception.
- Use `aria-live` for asynchronous status, not for every render.
- Check focus-visible, non-text contrast, target size and 320 px reflow.
- Automated axe is a floor, not conformance.

## 6. i18n, layout and visual quality

- Every new visible string goes through the catalog in English and Spanish.
- No concatenated sentences that cannot be translated; use parameters.
- Test long Spanish labels and 320/390 px; avoid horizontal overflow.
- Use design tokens and existing spacing/type scale; no raw hex colors.
- Motion respects `prefers-reduced-motion`.
- Dense operational screens prioritize scanability: state, identity, action.

## 7. Performance

- Do not memoize by default. Measure first.
- Avoid recreating large lists or expensive derived data on every keystroke.
- Virtualize only with a measured need and a stable row identity.
- Do not block the main thread with large JSON parsing; use a worker or a
  bounded rendering strategy if measured.
- Keep bundle changes intentional; no new heavy dependency without evaluation.

## 8. Testing

- Pure logic: Vitest.
- Component branches: Testing Library with role/name queries.
- Network boundary: MSW, not module stubs.
- Real browser: Vitest browser mode for focus/portals/CSS; Playwright for the
  full journey.
- Query by role and accessible name; avoid CSS selectors and test IDs unless
  there is no semantic option.
- New/changed Playwright specs: repeat 20 times; no retries to green.
- Assert what the user observes and what Core records; do not assert internal
  implementation details.

## 9. TypeScript

- Keep `strict`.
- Parse untrusted payloads at the boundary; do not cast to a domain type.
- Use discriminated unions for closed states.
- Avoid `any`; use `unknown` plus validation.
- Avoid non-null assertions; handle the absent case.
- Keep exhaustive switches exhaustive when the state is closed.
- Do not use `@ts-ignore`; a scoped `@ts-expect-error` needs a reason.

## 10. Review checklist

- Does every visible control invoke an existing Core operation?
- Are loading/empty/error/unauthorized/offline/recovery states real?
- Is the primary action singular and obvious?
- Is there a double-submit guard and a cancel/close path?
- Keyboard: reachable, logical order, visible focus, Escape, focus return.
- Screen reader: names, roles, states, live updates.
- 320/390/834/1440 px; EN/ES; light/dark; reduced motion.
- No console errors, failed requests or horizontal overflow.
- Tests cover the changed option matrix, not just a smoke route.

## 11. References

- [Office design system](../../../docs/product/office-design-system.md)
- [Office UI standard](../../../docs/product/office-ui-standard.md)
- [UI change acceptance](../ui-change-acceptance/SKILL.md)
- [Factory Office design](../factory-office-design/SKILL.md)
- [Office feature parity](../office-feature-parity/SKILL.md)
