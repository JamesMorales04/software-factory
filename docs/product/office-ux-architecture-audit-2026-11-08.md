# Office UI, UX and architecture audit — 2026-11-08

**Status:** Findings from an owner-run review of the live developer-preview stack
at `http://127.0.0.1:8893`, plus code and database inspection on product head
`acaee8d`. This is an audit and delivery plan, not a certification.

## Executive summary

Office currently presents too many internal Factory objects as first-class
navigation and exposes legacy monetary/limit metadata that no longer protects a
subscription route. The product job is simpler:

1. The owner describes an outcome in the chat.
2. The Lead plans and asks for the decisions that require human authority.
3. The owner watches active work, exceptions and outcomes.
4. Everything else is telemetry or configuration, not a daily destination.

The current shell does not reflect that hierarchy. Lead, Delegations,
Executions, Workers, Actions, Overview and Runtime compete with Campaigns and
Decisions. Several screens surface reservation ceilings, per-request budgets and
price metadata that add operational noise without adding authority.

Three correctness defects were also found and reproduced:

- the Actions projection returned HTTP 500 because 13 historical Actions had no
  frozen `action_specs` row, which also broke Home status, Campaigns and
  Decisions;
- the Worker restart-looped on a deterministic candidate-path rejection instead
  of reporting a terminal Attempt failure, leaving a Run visible as "running";
- the Projects GitHub loader invalidated its own request when its `busy` state
  changed, leaving "Cargando repositorios…" indefinitely.

## Findings

| ID | Severity | Finding | Evidence | Disposition |
|---|---|---|---|---|
| UX-01 | High | Project and quality-profile selectors are duplicated in the global top bar and the Home composer. | Owner screenshot; `WorkspaceContextBar` mounted by `AppShell`. | **Fixed in this slice.** Top-bar selectors removed; Home remains the only place that chooses request defaults. |
| UX-02 | Critical | Factory status and Decisions show `database error: no rows returned by a query that expected to return at least one row`. | Reproduced with `GET /core-api/office/overview` and `/office/decisions`; `/actions` also failed. 13 `actions` rows had no `action_specs`. | **Fixed in this slice.** List projection skips historical rows with no executable specification; direct lookup remains `NotFound`. |
| UX-03 | High | A Run remained "En ejecución" while the Worker repeatedly failed the same candidate (`Cargo.lock` outside `src/tests`). | Worker logs and SQLite: Run `191b3e2d…`, Attempt `d72d0e02…`, 95 provider sessions. | **Mitigated in this slice.** Worker reports a terminal failure on candidate rejection instead of restart-looping. Stalled-run reconciliation is still a follow-up. |
| UX-04 | High | The Projects GitHub repository list never leaves the loading state. | Owner screenshot; effect dependencies included `repositoryBusy`, so setting busy invalidated the request. | **Fixed in this slice.** One-shot bootstrap uses a request ref and no self-invalidating cleanup. |
| UX-05 | High | Home requires an "inference authorization (USD)" even when the selected route is an owner-confirmed subscription. | Owner screenshot; production approval path uses `subscription_admission_id`, not `authorized_micros`. | **Removed from the normal composer.** The backend keeps the optional legacy envelope for non-subscription/test paths. |
| UX-06 | Medium | Provider settings expose "Metadatos de precios" even though those rates are legacy telemetry inputs, not limits or useful operator facts. | Owner screenshot; `settings.provider.advanced` details. | **Removed from Office.** Provider APIs retain rates for legacy cost telemetry; no price editor is shown. |
| UX-07 | High | Runtime leads with "Gasto medido" and "Techo reservado" while the product no longer enforces subscription productivity budgets. The reserved ceiling was a meaningless multi-billion number. | Owner screenshot; `Runtime.tsx` reservation sum. | **Replaced in this slice** with measured tokens and completed sessions. Model-call tables now show tokens, not ceilings. |
| UX-08 | High | Lead, Delegations, Executions and Workers are separate sidebar destinations although they are operational evidence, not daily operator jobs. | Owner screenshot; `navigation.ts`. | **Partially consolidated.** Removed from the sidebar; Activity & telemetry remains the primary destination and the routes remain deep-linkable. Full tab consolidation is planned. |
| UX-09 | Medium | Campaigns exposes manual Action selection, fixture authorization and run queueing, which duplicate the Lead flow and teach the owner to drive internals. | `Campaigns.tsx` create/authorize/queue controls. | Planned: make Campaigns read-only + cancel, with Lead as the only creation path. |
| UX-10 | Medium | Governed issue intake renders 14 policy fields and monetary caps before the operator can do the simple job. | Owner screenshot; `IssueIntakePanel.tsx`. | **Improved in this slice.** Advanced fields are collapsed and the description is plain-language. Further batch-flow simplification is planned. |
| UX-11 | Medium | Projects local import requires typing an absolute path; there is no folder browser. | Owner screenshot; local import form. | **Implemented in this slice.** Core exposes a read-only, root-confined directory listing; Office offers a folder browser and fills the selected path. |
| UX-12 | Medium | Several legacy caps remain in the active schema/configuration even after ADR-0034: provider `maxTurns`, `maxInputTokens`, `maxOutputTokens`, `maxCostMicros`; execution-profile `maxGenerationTokens`; issue-intake attempt/session/turn/wall-clock limits. Some are inert on subscription routes, but they still appear in payloads and can be mistaken for active policy. | `ProviderSettingsInput`, `ExecutionProfile`, `IssueIntakePanel`, `model_routing.rs`. | Planned: a dedicated ADR-0034 follow-up removes or freezes legacy fields, migrates profile defaults and proves subscription turns are uncapped. Do not silently reinterpret historical rows. |
| UX-13 | High | A Worker process death, expired lease or crashed provider session can leave a Run/Attempt `RUNNING`/`LEASED` indefinitely. | Live DB: expired lease from 2026-10-26; Run changed to `REPAIR_REQUIRED` only after manual/recovery activity. | Planned: a Core-owned stale-run reconciliation projection/operation and a visible "interrupted" state. Never infer success from absence. |
| UX-14 | High | Office has no UI to create the Owner-confirmed subscription admission required before a live MODEL assessment. Approval can consume an existing admission, but a fresh owner cannot reach the normal live-model path through Office alone. | `subscription_admission` endpoints, `RequestWorkspace` approval, live provider UAT used direct API. | Planned: a single owner-level "OpenCode Go Extra Usage is OFF" confirmation in Settings/request workspace, then Core-created per-request admission. Until then live MODEL acceptance remains API-assisted. |

## Target information architecture

Primary navigation:

- **Inicio** — one chat surface, Project/Profile context and recent work.
- **Trabajo** — Campaigns, active/blocked/completed outcomes and the Decision
  hand-off.
- **Decisiones** — only actions that need the owner.
- **Proyectos** — import, configure and inspect Projects.
- **Incidencias** — a simple governed intake, advanced options collapsed.
- **Actividad y telemetría** — sessions, model calls, workers, runs, runtime
  diagnostics and logs in one place.
- **Configuración** — providers, profiles, runtime policy and access.

Advanced destinations that remain reachable but do not need daily navigation:
Overview, Actions, Lead, Delegations, Runs, Workers and Access & Devices.

## Telemetry design

The Activity workspace should answer:

- what is active, stalled, failing, waiting or completed;
- how long each Attempt/session took;
- which provider, model, role, project and Campaign were involved;
- measured input/output/total tokens and tool events;
- the last progress checkpoint, last error and cleanup receipt;
- whether the state is live, stale, unknown or disconnected.

Proposed views:

1. **Overview** — active Campaigns, completed/failed sessions (24 h), measured
   tokens, unresolved sessions, last meaningful events.
2. **Logs** — one filterable timeline across events, worker receipts, session
   transitions, model calls, progress checkpoints and cleanup. Filters: time,
   Project, Campaign, role, provider/model, severity, state.
3. **Sessions** — one row per provider session with duration, tokens, state,
   worker, model, cleanup receipt and error.
4. **Model calls** — role, route, purpose, state, tokens, duration, error. No
   reservation ceiling in the primary table.
5. **Workers & Runs** — registration freshness, active lease, last heartbeat,
   Run/Attempt status and a truthful interrupted state.

Do not show a monetary "ceiling" unless a real billable envelope is the current
authority for that exact Campaign. Subscription usage is telemetry.

## Delivery plan

| Phase | Scope | Status |
|---|---|---|
| P0 | Actions 500, Worker candidate-rejection loop, GitHub loader, top-bar duplication | This slice |
| P1 | Remove per-request inference budget and price editor; replace cost-led Runtime cards with tokens/sessions | This slice |
| P2 | Sidebar consolidation into Activity & telemetry; keep deep links | This slice (partial) |
| P3 | Projects managed-folder browser and Core directory API | This slice |
| P4 | Issue-intake progressive disclosure and plain-language summary | This slice |
| P5 | Campaigns read-only simplification; remove manual fixture controls from the normal path | Next |
| P6 | Full Activity logs/timeline with filters, duration and cross-object drill-down | Next |
| P7 | Legacy cap removal/migration under ADR-0034; stale-run reconciliation | Next, with tests and an ADR amendment if the money boundary changes |
| P8 | Real browser acceptance at desktop/tablet/narrow widths and independent verification | Before merge |

## Non-claims

This audit does not certify SEC-01 isolation, deployment, Windows, live paid
model quality or independent acceptance. `TRUSTED_LOCAL` remains broad local
access and is never described as isolated or sandboxed.

## Delivery update — autonomy/UX slice (2026-11-08)

This section records what changed after the findings above. It does not rewrite
the original evidence.

- Removed the Actions, Action Detail, Lead, Delegations, Runs, Workers and
  Overview pages. Their old URLs redirect to Campaigns, Requests, Settings,
  Runtime or Home; unknown routes return Home. Browser path URLs copied without
  the hash redirect to the hash route before root API aliases can answer.
- Added `/requests` with Project and state filters, human objectives, translated
  states and no raw request/delegation/Lead IDs in the primary surface.
- Made Campaigns read-only, added a Project filter, joined the owning Action's
  objective/project into the Campaign projection, and derived an effective
  lifecycle status from terminal Campaign state or the latest immutable Gate
  outcome when no Run is active. An accepted Campaign now reads `Accepted`, not
  `In progress`.
- Rebuilt Issues as a five-column board (Ready, Queued, Working, Needs
  attention, Done) with automatic stale sync, Project filter, issue detail and
  one-click governed intake. Added an explicit auto-start toggle that starts one
  ready issue at a time while Office is open. There is still no Core-owned
  scheduler.
- Replaced the managed-root folder browser with the operating system folder
  picker (`showDirectoryPicker`). The selected folder is copied into managed
  storage through a bounded Core ZIP import, Git is initialized when needed,
  and dependency/build directories are skipped. A direct host-path mount is not
  claimed.
- Added an owner-facing subscription confirmation in the Request workspace.
  Core accepts a request-scoped admission bounded to seven days instead of
  requiring the former fixed Wave 3 test-profile literal.
- Removed active productivity and monetary remnants: the semantic-review output
  cap, the hidden 120-second model-call reservation deadline, the Codex
  eight-tool-call cap, provider request-count enforcement, provider cap editing
  and the issue-intake monetary/attempt/session/turn/wall-clock inputs. Legacy
  database columns remain historical evidence; issue-intake V2 writes `0`
  meaning uncapped and no longer rejects a batch on a cost ceiling.
- Fixed a real execution failure found in the browser: frozen validation
  commands such as `cargo test` can rewrite `Cargo.lock`. The Supervisor now
  discards tracked and untracked changes outside the verified allowed paths
  after each validation command, before candidate finalization. The same live
  run then produced a candidate containing only the authorized new file and
  reached `ACCEPTED`.
- Real Chromium runs against Compose reached live OpenCode Go Lead inference,
  a frozen plan, a governed Campaign, a real implementer candidate, independent
  verifier and reviewer receipts, and both Gate outcomes:
  - a candidate that changed behavior in `src/pricing.rs` was correctly held at
    `REPAIR_REQUIRED` with `review requires changes` and
    `autonomous repair is disabled`;
  - a purely additive candidate (`src/live_inference_check.txt`) reached
    `ACCEPTED` with a one-file, one-line diff.
- Removed the remaining provider-settings token/cost clamp from model gateway
  construction. Subscription model-call authorization already reserved uncapped
  technical sentinels; persisted provider limits are now historical telemetry
  inputs, not an execution ceiling.
- Updated the maintained deterministic browser journeys to drive the new
  operating-system folder picker and the current Core request schema. The old
  managed-root browser controls and removed provider/runtime fields are gone.
- The Issues board now requests open and closed snapshots, shows Done for closed
  issues and completed linked requests, distinguishes a failed Project read from
  an empty board, and discloses the 100-issue projection limit.
- Archive import now uses a deterministic target for one idempotency key,
  replays the same registration on retry, and removes the extraction directory
  on every failure. A stubbed real-browser import created a managed Git Project
  through the new picker and Core ZIP endpoint.
- The browser runs found no console errors, no unexpected 404/5xx responses and
  no horizontal overflow on the audited routes. Screenshots and JSON evidence
  are recorded under `docs/evidence/office-autonomy-ux-2026-11-08/`.

Remaining gaps: Core-owned autonomous issue scheduling, autonomous Gate repair,
native desktop folder integration, Windows, GitHub App/OAuth onboarding, a true
independent security review, independent acceptance and Owner visual sign-off.
`TRUSTED_LOCAL` is never isolated or certified.
