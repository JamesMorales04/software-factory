# Factory Office localization — rollout note

**Issue:** [#43](https://github.com/JamesMorales04/software-factory/issues/43)
**Status:** PARTIAL. English/Spanish infrastructure and the owned stable views are
implemented, but this is not a full Office localization and has no real browser
acceptance yet.

## What is implemented

- `office/src/i18n/` provides a typed `I18nProvider`/`useI18n`, English and
  Spanish catalogs with stable namespaced keys, named interpolation, `Intl`
  number/date formatting, English fallback, missing-key diagnostics in tests and
  development, `localStorage` persistence (`factory-office-locale`) and
  `document.documentElement.lang` synchronization.
- English is the default. An unknown or absent stored locale resolves to `en`.
- `LanguageSwitcher` is exported for later shell/Home integration and is mounted
  as a compact block at the top of Settings.
- The `nav.*` catalog keys are ready for the shell, but `AppShell.tsx` and
  `navigation.ts` were intentionally not edited by this lane.

## Views translated end-to-end

- Settings language block
- Access & Devices (`AccessDevices`)
- Device pairing claim (`DevicePairClaim`)
- Decision Inbox
- Projects
- Runs
- Workers
- Runtime (including its tabs, metrics, filters, tables and diagnostics)
- Shared `ParticipantSessionList` and `ModelCallResolution`, which are now
  translated wherever they render.

Machine identifiers, Core status codes, provider/model IDs, project repository
content, commit hashes, capability IDs and quality-profile IDs are not
translated.

## Views and surfaces still untranslated

- Application shell, navigation labels, route titles and the lock/session action
  (`AppShell.tsx`, `navigation.ts`, `App.tsx`).
- Bootstrap/recovery gate, connection failure and unknown-route text (`App.tsx`).
- Overview/Home, request intake (Home/Request is not present in this branch),
  Actions, Action Detail, Lead, Delegations, Campaigns, Campaign Detail, Agent
  Office and the Campaign cancel confirmation.
- Settings outside the language block: page header/description, tabs, provider
  catalog, runtime policy form, provider save/test/revoke notices and the
  configuration explainer. The Access & Devices tab body is translated, but the
  Settings chrome around it is not.
- Shared defaults in `LoadingState`/`ErrorState` remain English when an
  untranslated caller does not pass a translated label or title.
- Agents and Campaign views display the translated shared model-call resolution
  and session list inside otherwise English pages; those pages are partial, not
  localized.

## Responsive changes

- Localized views use a `table-responsive` region. At widths up to 640 px, table
  rows render as cards driven by `data-label` attributes, so actions and long
  identifiers stay visible instead of clipping.
- Language controls, filters, device actions, pairing verification and decision
  actions stack at 390 px and 320 px; primary buttons receive a 44 px minimum
  touch target. Essential operations are not hidden.
- QR pairing markup and the Core flow are unchanged.

## Evidence and limits

Component tests cover catalog key/placeholder parity, interpolation, `Intl`
formatting, missing-key fallback/diagnostics, persistence across remount,
`documentElement.lang`, keyboard reachability, route preservation, Spanish
rendering in key views and responsive table semantics. `npm --prefix office run
typecheck|lint|test|build` pass; the test suite reports 80 passing tests.

These are component checks only. jsdom cannot verify the actual 390 px/320 px
layout, focus visibility, overflow or the real QR pairing journey. A Chromium
run against the Compose stack at both widths remains `NOT_TESTED` and is not
claimed here.

## Next work

1. Wire `nav.*` and the switcher into `AppShell`/Home/Request in the IA lane
   without changing route behavior.
2. Preserve the Settings language block while the profiles lane edits Settings,
   then localize the remaining Settings provider/runtime surfaces.
3. Sweep the remaining stable views and bootstrap gates.
4. Add the real browser acceptance at 1440/834/390/320, including QR pairing,
   before recording journey 11 or 12 as `PASS`.
