# WCAG 2.2 manual acceptance reference

Automated axe checks are a floor. They cover only part of WCAG and cannot judge
meaning, focus order or whether a control is operable. Record automated and
manual evidence separately.

## Minimum manual matrix per changed surface

| Check | How | WCAG |
|---|---|---|
| Keyboard reachability | Tab/Shift+Tab through every control | 2.1.1 |
| No keyboard trap | Tab out of dialogs/menus; Escape closes | 2.1.2 |
| Focus order | Follow visual/reading order | 2.4.3 |
| Focus visible | Every focused control has a visible indicator | 2.4.7 |
| Focus not obscured | Sticky header/banner does not cover focus | 2.4.11 |
| Target size | Touch targets at least 24×24 CSS px, or spacing equivalent | 2.5.8 |
| Dragging alternative | Every drag has a single-pointer/keyboard alternative | 2.5.7 |
| Reflow | 320 CSS px and 400% zoom, no horizontal scroll/lost content | 1.4.10 |
| Text spacing | 200% text size / spacing overrides do not clip | 1.4.12 |
| Contrast | Text and non-text controls meet AA | 1.4.3, 1.4.11 |
| Names/roles/states | Screen reader announces control and state | 4.1.2 |
| Error identification | Errors are text, associated and announced | 3.3.1, 3.3.3 |
| Status messages | Async success/error is announced without focus theft | 4.1.3 |
| Content meaning | Alt text, labels and headings are accurate | 1.1.1, 2.4.6 |
| Language | `lang` and Spanish/English strings are correct | 3.1.1, 3.1.2 |

## Manual procedure

1. Run axe and fix serious/critical violations.
2. Keyboard-only walk-through of the primary and destructive actions.
3. Zoom/reflow at 320 px and 400%; check sticky headers and dialogs.
4. Screen-reader pass on the primary journey (NVDA/VoiceOver or equivalent).
5. Review labels, errors, empty states and status messages for meaning.
6. Record what was not tested and why.

## Evidence

```text
Surface:      <route/state>
Automated:    <axe command/version and result>
Keyboard:     <order, focus, trap, Escape>
Reflow:       <320/400% result>
Screen reader:<browser/AT and observations>
Content:      <alt/label/heading review>
Not tested:   <...>
```

A clean axe report is not a conformance claim.
