# Factory Office localization — rollout note

**Issue:** [#43](https://github.com/JamesMorales04/software-factory/issues/43)
**Status:** OWNED SURFACES COMPLETE (EN/ES), OFFICE NOT FULLY LOCALIZED. The
Wave 2 i18n/mobile lane translated the shell, Home/Request, Actions, Lead,
Campaigns, Agent Office, Issues and their dialogs/empty/error/loading/offline
states. Views owned by other Wave 2 lanes remain untranslated and are listed
below. No real-browser acceptance is claimed from component tests.

## What is implemented

- `office/src/i18n/` provides a typed `I18nProvider`/`useI18n`, English and
  Spanish catalogs with stable namespaced keys, named interpolation, `Intl`
  number/date formatting, English fallback, missing-key diagnostics in tests and
  development, `localStorage` persistence (`factory-office-locale`) and
  `document.documentElement.lang` synchronization.
- English is the default. An unknown or absent stored locale resolves to `en`.
- The global header mounts the compact `LanguageSwitcher`. It persists the
  locale, updates `document.documentElement.lang`, keeps the current route,
  search and hash context, and stays reachable at 390 px and 320 px. The
  labelled Settings block remains available with a distinct accessible name
  (`Interface language`) from the header control (`Display language`).
- Navigation group labels, every destination, the active route title, command
  palette results, skip link, theme/density/lock controls, Core health badge and
  the application bootstrap/recovery gate are translated.
- Editing a language does not navigate. Detail routes, query-backed routes and
  request workspaces keep their context.

## Views translated end-to-end

- Application shell, navigation, route titles, command navigation, health badge
  and bootstrap/recovery gate.
- Home: composer, validation, execution-profile state, governed-execution
  grant, recent requests, Core status, active work and pending decisions.
- Request workspace: assessment, plan, approval, conversation, details,
  cancellation, loading/empty/error/offline states.
- Actions and Action detail.
- Lead: request form, list, selected request, assessment, model-call provenance,
  proposal, frozen plan and explicit start authorization.
- Campaigns list and the shared Campaign cancellation confirmation.
- Agent Office: filters, visual office zones, table, timeline, participant
  inspector, quality evidence, usage and recorded tool events.
- Issues: GitHub connection, repository binding, filters, synchronized table,
  pagination and issue detail.
- Previously translated by the i18n foundation and retained: Settings language
  block, Access & Devices, device pairing claim, Decision Inbox, Projects
  registry/configuration, Runs, Workers, Runtime, shared
  `ParticipantSessionList` and `ModelCallResolution`.

Machine identifiers, Core status codes, provider/model IDs, project repository
content, commit hashes, capability IDs and quality-profile IDs are not
translated. Recommendation text stored in Core is shown as recorded.

## Views and surfaces still untranslated

- Overview (`/overview`) and Delegations (`/delegations`).
- Campaign detail (`/campaigns/:campaignId`) including its tabs, WorkGraph,
  candidate/diff/evidence components and PR/checks panels. The shared
  cancellation confirmation used on that page is translated; the surrounding
  Campaign detail page is owned by the Campaign diff/PR lane.
- Project file browser (`/projects/:projectId/files`).
- Settings outside the language block: page header/description, tabs, provider
  catalog, runtime policy form, provider save/test/revoke notices and the
  configuration explainer. Access & Devices tab body is translated.
- Execution profiles settings are untranslated.
- Shared `LoadingState`/`ErrorState` defaults remain English when a caller does
  not pass a translated label/title. All views owned by this lane pass them.
- Core-returned error details, state details, authorization reasons and model
  prose are displayed as returned and may be English.

## Responsive changes

- Localized tables use a `table-responsive` region. At widths up to 640 px rows
  render as cards driven by `data-label` attributes, so actions and identifiers
  stay visible instead of clipping. Issues no longer hides the label/author
  columns on narrow screens.
- Home, Request, Actions, Lead, Campaigns, Agent Office and Issues stack forms,
  filters, pagination and primary actions at 390 px and 320 px. Primary buttons
  receive a 44 px minimum touch target. Navigation rows and the header language
  control are touch-sized on narrow screens.
- Agent Office tables and recorded tool events use the same card fallback.
- QR pairing markup, the pairing flow, Access & Devices behavior and device
  sessions are unchanged.

## Evidence and limits

Component tests cover catalog key/placeholder parity, interpolation, `Intl`
formatting, missing-key fallback/diagnostics, persistence across remount,
`documentElement.lang`, header language switching with route/query preservation,
English and Spanish rendering of every owned view, and responsive table
semantics. `npm --prefix office run typecheck|lint|test|build` pass; the suite
reports 139 passing tests. Lint reports one pre-existing warning in
`office/e2e-browser/intake-journey.mjs` (unused variable) and no new errors.

Author-run real Chromium evidence on 2026-10-28: the canonical Office journey
(`node office/e2e-browser/rust-journey.mjs` against locally built Core, Worker,
Supervisor and Reviewer binaries) passed through owner enrollment, the complete
Campaign lifecycle and `ACCEPTED` gate at 1440, 834, 390 and 320 px. The same
journey now also switches the shell to Spanish, verifies `documentElement.lang`,
persisted locale, translated navigation, the narrow navigation drawer and the
Spanish Home, Campaigns, Issues, Agent Office, Actions and Lead views at 834,
390 and 320 px without document overflow. This is author-run evidence only; it
is not independent acceptance and does not replace the verifier/reviewer gate.
The journey is deterministic and does not exercise a live provider.


## Next work

1. Localize Overview and Delegations in their owning lanes.
2. Localize Campaign detail/WorkGraph/candidate diff in the Campaign diff/PR
   lane, and Project files in the Projects lane.
3. Localize the remaining Settings provider/runtime/profiles chrome without
   removing the Settings language block.
4. Record real browser acceptance at 1440/834/390/320, including QR pairing,
   before recording the localization mobile journey as `PASS`.
