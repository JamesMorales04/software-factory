# Factory Office localization — rollout note

**Issue:** [#43](https://github.com/JamesMorales04/software-factory/issues/43)
**Status:** APPLICATION SURFACES COMPLETE (EN/ES) at the
`campaign/office-ux-i18n-final-20261028` lane. Every implemented Office route,
including Overview, Delegations, Campaign detail and its diff/PR/evidence
panels, Project files, Settings provider/runtime chrome and Execution Profiles,
now uses the typed catalogs. Core-returned prose and machine identifiers remain
untranslated by design. No independent acceptance or live-provider evidence is
claimed from component or author-run browser tests.

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
- Overview: headings, KPI labels and details, quality-gate outcomes, decision
  and accepted-candidate panels, runtime health, recent events, and
  loading/error/empty states.
- Delegations: form, validation, creation/revocation notices, state table and
  usage summary.
- Campaign detail: header, tabs, lifecycle projection, WorkGraph visual/table
  and inspector, execution and attempts, participant sessions, candidates,
  evidence and review findings, Quality Gate, authorization/budget/usage,
  cancellation and recovery, and events.
- Candidate diff and PR/check panels: changed-file list, per-file patch,
  revisions, truncation/omission states, checks, comments, review request,
  authorized merge, opening a PR, and all confirmations/warnings.
- Project files: revision picker, breadcrumb, tree, filter/counts, file
  metadata, binary/large/symlink states, Markdown preview/source and code
  viewer.
- Settings: page header/description, tabs, provider cards, verification path,
  model catalog, budget/rate form, save/test/revoke notices, configuration
  audit, runtime review policy and configuration boundaries.
- Execution Profiles: list/table, defaults and effective-profile resolution,
  role matrix, model discovery states, campaign budgets, autonomous-start
  policy, validation and draft/stale/save notices.
- Previously translated by the i18n foundation and retained: Settings language
  block, Access & Devices, device pairing claim, Decision Inbox, Projects
  registry/configuration, Runs, Workers, Runtime, shared
  `ParticipantSessionList` and `ModelCallResolution`.

## What remains untranslated

- Machine identifiers, status codes, capability/quality-profile IDs, paths,
  SHAs, PR numbers, check names, repository content and raw Git/reviewer data.
- Core-returned error details, state details, authorization reasons, evidence
  prose, event details, provider notes and model prose are displayed as
  returned and may be English.
- Shared `LoadingState`/`ErrorState` defaults remain English only when a caller
  does not pass a translated label/title. Every route and component covered by
  this lane passes translated values.
- The prefilled name/description of a brand-new Execution Profile
  (`Balanced` / `Default supported routes`) remain operator-editable English
  content, not chrome; changing them would create Spanish profile records in
  Core.

## Responsive changes

- Localized tables use a `table-responsive` region. At widths up to 640 px rows
  render as cards driven by `data-label` attributes, so actions and identifiers
  stay visible instead of clipping. Issues no longer hides the label/author
  columns on narrow screens.
- Home, Request, Actions, Lead, Campaigns, Agent Office, Issues, Overview,
  Delegations, Campaign detail, Project files, Settings and Execution Profiles
  stack forms, filters, tabs, tables and primary actions at 390 px and 320 px.
  Primary buttons, form fields and tabs receive a 44 px minimum touch target.
  Navigation rows and the header language control are touch-sized on narrow
  screens.
- Campaign lifecycle, runs, attempts, lead reservations and receipt evidence
  tables use the same card fallback, including row headers. The WorkGraph table
  remains the horizontally scrollable accessible alternative.
- Agent Office tables and recorded tool events use the same card fallback.
- QR pairing markup, the pairing flow, Access & Devices behavior and device
  sessions are unchanged.

## Evidence and limits

The catalogs contain 2,307 keys in English and 2,307 keys in Spanish with
identical key sets and placeholders. Component tests cover catalog parity,
interpolation, `Intl` formatting, missing-key fallback/diagnostics, persistence
across remount, `documentElement.lang`, header language switching with
route/query preservation, English and Spanish rendering of every owned view and
panel, and responsive table semantics. `npm --prefix office run
typecheck|lint|test|build` pass; the suite reports 187 passing tests in 29 files
and lint reports no warnings or errors.

Author-run real Chromium evidence on 2026-10-28 at the lane HEAD: using a
production `vite preview` build and Playwright with mocked `/core-api` responses
at 834, 390 and 320 px, the Spanish Overview, Delegations, Campaign detail,
Project files, Settings providers and Settings execution-profile routes loaded
with `document.documentElement.lang === "es"`, no document-level horizontal
overflow and no content control below 44 px. All seven Campaign detail tabs
(including diff/PR unbound and receipt-evidence states) were opened at 320 px
without clipped tables or overflow. These are author-run, mocked-browser checks:
they do not prove Core-backed data, live providers, independent acceptance or
the verifier/reviewer gate.

Earlier author-run real Chromium evidence on 2026-10-28: the canonical Office
journey (`node office/e2e-browser/rust-journey.mjs` against locally built Core,
Worker, Supervisor and Reviewer binaries) passed through owner enrollment, the
complete Campaign lifecycle and `ACCEPTED` gate at 1440, 834, 390 and 320 px,
including a Spanish shell/locale switch and the then-translated Home, Campaigns,
Issues, Agent Office, Actions and Lead views.


## Next work

1. Record independent verifier/reviewer acceptance of the localization journey
   at 1440/834/390/320, including QR pairing and real Core-backed pages, before
   recording it as `PASS`.
2. Keep new Office surfaces on the typed catalog; catalog parity fails the suite
   when a key or placeholder is added to only one locale.
