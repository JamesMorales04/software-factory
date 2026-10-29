# Office Home visual-polish slice 1 — fixture visual evidence (2026-10-29)

**Status: FIXTURE-ONLY — NOT VISUAL ACCEPTANCE, NOT LIVE ACTIVITY.**
The PNGs render the real Office React app from Vite with every `/core-api`
request answered by deterministic local fixtures. No Core process, Compose
stack, provider, model or live GitHub connection was contacted. The orange
banner in every capture marks the fixture explicitly. The reference PNGs are
style targets, not product data.

- Branch: `factory/office-design-polish/20261029`
- Base: `2907b17`
- Workstream: Wave 3 workstream 1, issue #62 visual direction
- Candidate: this repository commit (the exact SHA is reported in the final
  handoff)
- Harness: `office/e2e-browser/office-home-visual-polish.mjs`
- Browser: Google Chrome for Testing `156.0.8078.4` (pinned Playwright
  Chromium)
- Vite: `7.3.7`
- Design audit: [`design-audit.md`](design-audit.md)

## Screenshots

| File | Viewport | Content |
|---|---|---|
| `home-1440.png` | 1440 px | Navy rail/top bar, conversation-first composer, compact Project/profile selects, explicit USD inference authorization, recent work, decisions and Core status |
| `home-834.png` | 834 px | Single-column composer and recent work, two-column decisions/status; drawer navigation |
| `home-390.png` | 390 px | Narrow composer, full-width primary action, stacked recent/decision/status surfaces |
| `home-320.png` | 320 px | Smallest supported layout; no clipped action or horizontal scroll |
| `home-390-es.png` | 390 px | Spanish composition with the same Core request behavior |
| `home-390-drawer.png` | 390 px | Mobile navigation drawer with the theme/density preferences moved into its footer |
| `home-1440-focus.png` | 1440 px | Focus ring on the instruction textarea |
| `home-1440-collapsed.png` | 1440 px | 72 px icon rail with labels available through the accessible name/title |
| `home-1440-dark.png` | 1440 px | Dark theme using the same shell/content semantic tokens |
| `home-1440-empty-projects.png` | 1440 px | Empty Project state; the composer is replaced by a truthful registration prompt and the side surfaces stay quiet |
| `baseline-home-1440.png` | 1440 px | Pre-slice fixture Home at base `2907b17` |
| `baseline-home-390.png` | 390 px | Pre-slice fixture Home at base `2907b17` |
| `visual-polish-evidence.json` | — | Machine-readable deterministic fixture checks; explicitly not acceptance |
| `overflow-report.json` | — | Per-viewport scroll-width and offender report |

## Reference comparisons

| File | Comparison |
|---|---|
| `reference-02-vs-home-1440.png` | Reference 02 layout target beside the fixture Home at 1440 |
| `reference-03-vs-home-1440.png` | Reference 03 Home/Campaign target beside the fixture Home at 1440 |
| `reference-03-mobile-vs-home-390-es.png` | Reference 03 phone mock beside the Spanish fixture Home at 390 |

These composites are visual-direction comparisons only. They do not reproduce
the reference collages' fake metrics, projects, activity or sample values.

## Exact checks run

From `office/` against this commit:

| Check | Result |
|---|---|
| `npm run typecheck` (`tsc --noEmit`) | PASS |
| `npm run lint` (`biome check src e2e-browser`) | PASS — 137 files, no fixes applied |
| `npm test` (`vitest run`) | PASS — 34 files, 256 tests |
| `npm run build` (`tsc --noEmit && vite build`) | PASS — 2149 modules; 125.63 kB CSS gzip 21.19 kB, 1,160.57 kB JS gzip 307.94 kB; existing >500 kB chunk warning remains |
| `CHROMIUM_EXE=… node office/e2e-browser/office-home-visual-polish.mjs` | PASS — 1440/834/390/320 + dark + ES; every viewport reported equal document/body scroll width and zero overflow offenders |
| `cargo run --manifest-path native/Cargo.toml --bin factory-architecture-check -- --root .` | NOT PASS — pre-existing failure in `CAP-050` and `CAP-052`, which still reference non-JSON README evidence on base `2907b17`; the changed `CAP-048` entry now points at `visual-polish-evidence.json` |

One concurrent `npm test` + `npm run build` run hit a 5 s timeout in the
pre-existing `AgentOfficeIsometric` 30-entity test. Re-running `npm test` alone
passed 34 files / 256 tests; this slice does not modify that component. The
serial result is the one reported above.

The harness writes `overflow-report.json` and fails the run if the document
scroll width exceeds the viewport or any visible element crosses the viewport
edge. The reported results are:

| Capture | Viewport | Document scroll width | Offenders |
|---|---:|---:|---:|
| `home-1440.png` | 1440 | 1440 | 0 |
| `home-834.png` | 834 | 834 | 0 |
| `home-390.png` | 390 | 390 | 0 |
| `home-320.png` | 320 | 320 | 0 |
| `home-390-drawer.png` | 390 | 390 | 0 |
| `home-1440-dark.png` | 1440 | 1440 | 0 |
| `home-390-es.png` | 390 | 390 | 0 |
| `home-1440-empty-projects.png` | 1440 | 1440 | 0 |

Component tests added or extended in this slice:

- `office/src/components/ui/Button.test.tsx` — large primary button, in-flight
  `aria-busy`, spinner, disabled duplicate submission and unchanged accessible
  name.
- `office/src/components/ui/Field.test.tsx` — help/error `aria-describedby`,
  invalid state, `labelHidden` accessible name, keyboard focus and disabled
  select.
- `office/src/views/Home.test.tsx` — existing Core behavior preserved: Project
  and objective validation with focus, positive USD authorization in integer
  micros, REVIEW_PLAN submission, duplicate-submit guard, missing endpoint
  fallback, profiles-unavailable fallback, empty Project state, status
  unavailable and the explicit execution-policy grant.
- `office/src/views/Home.styles.test.ts` — responsive grid contract, long-text
  wrapping, shell tokens, drawer breakpoint, focus visibility.

## Reproduce

```sh
npm --prefix office ci
npm --prefix office/e2e-browser ci
CHROMIUM_EXE=/path/to/chrome \
  node office/e2e-browser/office-home-visual-polish.mjs
```

The harness starts Vite on `127.0.0.1:5201`, mocks `/core-api` (setup, session,
health, Projects, execution profiles, intake list, Office overview), forces the
light/dark theme and EN/ES locale, asserts no horizontal overflow and writes
the PNGs into this directory. Override `FACTORY_SCREENSHOT_DIR` to capture
elsewhere.

## What these images support

- The shell now has a deep-navy rail/top bar and a separate light content
  surface built from semantic tokens rather than raw hex values.
- Home presents one dominant instruction composer, compact Project/profile
  selectors, one explicit Core-required USD inference authorization, one
  obvious primary action, and lighter recent-work/decision/status surfaces.
- Recent work, decisions and status still show only Core-backed fields; the
  fixtures are clearly labelled and no metric or activity is invented.
- The layout is usable at 1440, 834, 390 and 320 px without horizontal scroll.
- Focus is visible on the composer; dark theme, collapsed rail and Spanish UI
  render without layout collapse.
- Button/Field busy, disabled, invalid, focus and label-hidden behavior is
  covered by component tests.

## What these images do **not** prove

- Real Core/Compose behavior, persistence, authorization, recovery or restart.
- Live provider inference, billing, Codex/OAuth or OpenCode Go connectivity.
- Real-browser acceptance against Core, axe certification or visual
  regression.
- Contrast of every state in both themes (only the values listed in the audit
  were calculated).
- Product Owner visual approval. Owner visual acceptance remains
  **outstanding**.

## Visual gaps and Product Owner decision

The audit lists the gaps in detail: no Agent Office scene on Home, a different
mobile header from the reference phone mock, native select truncation for long
profile labels at 320–390 px, the always-visible spend envelope required by
Core, and other screens not yet migrated.

The Owner is asked to either:

- **A. Accept** the navy shell / blue primary / turquoise accent /
  composer-first Home direction for later propagation; or
- **B. Request a variant** before propagation (for example teal primary, a
  visible Home title, a mobile brand/project chip, or a collapsed authorization
  affordance).

Issue #62 is not closed by green tests or these fixture screenshots.
