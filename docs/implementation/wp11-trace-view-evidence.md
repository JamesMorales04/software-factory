# WP11 — workflow trace view evidence

**Status:** Implemented and implementer self-tested. This is **not** independent
verification, **not** Quality Gate acceptance and **not** a certification claim.
Browser/Compose acceptance is `NOT_TESTED`.

- Work package: WP11 — observability, trace, replay and the Core/Office trace view
- Branch: `factory/quality-wp11-trace-view`
- Baseline: `bcb8b5ba32090a19552637493ef53164928f230b`
- Code candidate: `40f9419dc7fa442a3c421e6758b81f5d09324b79`
- Date: 2026-11-09 (local) / 2026-11-10 UTC

## 1. Delivered behavior

### Core

`GET /agentic/requests/{id}/trace?limit=<n>` is an operator-authenticated,
bounded read of the latest durable workflow run for an intake request.

- Requires the operator bearer/device-session authorization used by the other
  `/agentic` routes; reviewer/worker tokens are rejected.
- Verifies the request exists (`404` otherwise).
- Returns `run: null` with an empty projection when no workflow run exists.
- Loads at most the newest bounded page of durable `events` rows in ascending
  `sequence` order (default 500, hard maximum 2,000). `recorded_at` is display
  data only and never orders the ledger.
- Maps rows into `ReplayEvent` and reuses `replay::build_trace`; there is no
  second transition or step-state projection.
- Returns the full `ReplayTrace`, a display event list with redacted payloads,
  durable step rows with replay-open markers, and structured alerts:
  `DUPLICATE_SEQUENCE`, `MISSING_TRANSITION`, `OPEN_STEP`, `TERMINAL_STATE`,
  `NON_TERMINAL_STATE`, `TRACE_TRUNCATED`, `REPLAY_REJECTED`,
  `STATE_DIVERGENCE`, `NO_EVENTS`, `MALFORMED_EVENT`.
- A rejected or truncated replay is surfaced as an alert/error. Core never
  invents missing state.
- The durable write path now records `run.created`,
  `workflow.step.started`, `workflow.step.succeeded|failed|blocked` and
  `workflow.checkpoint.recorded` events in the same transaction as the state
  change so the trace has real step and checkpoint evidence.

### Office

The Request workspace (`/requests/:requestId`) renders a `Workflow trace` panel
below the plan. It calls only the authenticated Core trace read.

- Loading, empty (no run), error/retry, stale/offline-after-background-refresh,
  truncated and replay-rejected states are explicit.
- Run identity, durable state, revision, replayed final state, terminal/in-progress
  outcome and bounded event count are shown from Core data.
- Alerts are localized badges with structured sequence/step/state detail.
- Step trace shows durable status, attempt, last event sequence, update time and
  an `Open in replay` anomaly badge.
- Event log is ordered by durable sequence, shows transition/step identity and a
  redacted payload `<details>` view. Payload keys/values come from Core's
  redaction projection; free-form step detail is not part of the response. The
  Office also refuses secret/prompt-shaped payload keys again as a
  defense-in-depth display guard.
- Manual refresh re-invokes the authenticated Core GET. There are no decorative
  or local-only action buttons.
- EN and ES strings are present for every new visible label.
- Narrow layout collapses step facts and payload key/value pairs to one column.

## 2. Changed files

| File | Purpose |
|---|---|
| `native/src/trace.rs` | Bounded trace response contract, alert derivation, redaction integration and unit tests |
| `native/src/db/events.rs` | Durable event loader, `ReplayEvent` mapping and DB-level tests |
| `native/src/db/workflow.rs` | Transactional run/step/checkpoint event writes |
| `native/src/db/mod.rs` | Registers the events module and step/checkpoint event kinds |
| `native/src/lib.rs` | Exposes the trace module |
| `native/src/bin/core.rs` | Authenticated bounded trace route, handler and endpoint test |
| `office/src/coreApi.ts` | `WorkflowTraceView` types and `getRequestTrace` |
| `office/src/views/RequestTrace.tsx` | Request trace panel |
| `office/src/views/RequestTrace.test.tsx` | Loading/empty/error/alerts/steps/long-list/ES and redaction tests |
| `office/src/views/RequestWorkspace.tsx` | Mounts the trace panel |
| `office/src/views/RequestWorkspace.test.tsx` | Adds the trace API mock |
| `office/src/i18n/catalog.en.ts` / `catalog.es.ts` | EN/ES trace strings |
| `office/src/styles/request.css` | Trace panel responsive styles |
| `docs/observability/telemetry.md` | Documents the operator trace read |
| `docs/product/office-parity.json` | CAP-049/CAP-011 trace API, states and acceptance rows |

## 3. Commands and exact results

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS (no output) |
| `cargo test -p factory-native --lib trace:: -- --nocapture` | PASS — 7 passed |
| `cargo test -p factory-native --lib db::events:: -- --nocapture` | PASS — 3 passed |
| `cargo test -p factory-native --bin factory-core request_trace -- --nocapture` | PASS — 1 passed |
| `cargo test -p factory-native --lib` | PASS — 741 passed, 0 failed, 2 ignored |
| `cargo clippy -p factory-native --all-targets --all-features -- -D warnings` | PASS |
| `npm ci` (Office) | PASS — 214 packages installed |
| `npm run typecheck` | PASS |
| `npm run lint` (`biome check src e2e-browser`) | PASS — 163 files |
| `npm test -- --run` | PASS — 46 files, 303 tests |
| `npx vitest run src/views/RequestTrace.test.tsx` | PASS — 6 tests |
| `npx vitest run src/views/RequestTrace.test.tsx src/views/RequestWorkspace.test.tsx` | PASS — 21 tests |

The two ignored library tests require rootless Podman with a cached
`alpine:3.20` image and were not part of this WP.

No paid model inference was run.

## 4. Trace endpoint tests

`native/src/bin/core.rs::tests::intake_api::request_trace_is_operator_only_bounded_and_redacted`
verifies:

1. Empty trace before a run (`run`/`trace` null, zero events).
2. A real run with transition and step lifecycle events returns
   `run.state = ASSESSING`, four ordered events, a `SUCCEEDED` step and the
   exact step ID.
3. A transition detail containing `token=super-secret-token-value` and a step
   finish detail containing `raw prompt body` never appear in the serialized
   response.
4. `limit=1` returns exactly one newest event with `truncated=true` and
   `eventLimit=1`.
5. A reviewer token gets `401`; a missing request gets `404`.

`native/src/db/events.rs` also verifies durable ordering, step mapping,
gap→`MISSING_TRANSITION`, truncation, and the empty no-run view.

`native/src/trace.rs` verifies terminal/non-terminal alerts, duplicate
sequences, gaps, open steps, state divergence, malformed rows, replay rejection,
truncation and payload redaction.

## 5. Office per-option matrix

| ID | Surface | Control/state | Expected result | Evidence | Result |
|---|---|---|---|---|---|
| WP11-UI-01 | `/requests/:requestId` | Trace loading | Loading label while Core reads | `RequestTrace.test.tsx` loading test | PASS |
| WP11-UI-02 | same | No workflow run | Empty state with real next-step explanation | empty test | PASS |
| WP11-UI-03 | same | Core error + Retry trace | Error text, retry invokes the authenticated GET again | error/retry test | PASS |
| WP11-UI-04 | same | Manual Refresh | Re-reads trace from Core; button busy/disabled during read | component implementation; covered indirectly by retry/read tests | PASS (component) |
| WP11-UI-05 | same | Run facts | Run ID, durable state, revision, replayed final state, outcome, count | populated test | PASS |
| WP11-UI-06 | same | Replay alerts | Duplicate/gap/open/malformed/non-terminal labels + sequence/state detail | populated test | PASS |
| WP11-UI-07 | same | Step trace | Status, kind, attempt, update time, last sequence, open badge | populated test | PASS |
| WP11-UI-08 | same | Event log | Sequence-ordered event rows, redacted payload values, hash | populated test + endpoint redaction test | PASS |
| WP11-UI-09 | same | Long list / truncation | 150-event list renders; bounded/truncated callout | long-list test | PASS |
| WP11-UI-10 | same | Spanish | Trace heading, event log and alert labels localized | ES test | PASS |
| WP11-UI-11 | same | Negative redaction | Raw prompt/token strings never render | populated test negative assertions | PASS |
| WP11-UI-12 | same | Background refresh failure | Stale/offline callout with last-loaded time and retry | implementation present; timed background failure | NOT_TESTED |
| WP11-UI-13 | same | Keyboard / screen reader | Headings, summary disclosures, labels, no hidden-only data | component semantics reviewed | NOT_TESTED (browser) |
| WP11-UI-14 | same | Desktop/tablet/narrow visual | No overflow; one-column facts/payload at ≤640 px | CSS reviewed | NOT_TESTED (browser) |
| WP11-UI-15 | same | Light/dark/density | Uses shared tokens only | CSS reviewed | NOT_TESTED (browser) |

## 6. Security and redaction impact

- The trace route is operator-authenticated and read-only.
- Response payloads are produced by `replay::redact_payload`; secret/prompt-shaped
  keys are hashed and unknown keys are `[REDACTED]`. The Office independently
  refuses to render secret/prompt-shaped payload keys as a display safety net.
- Non-JSON event detail is hashed, never returned verbatim.
- Step `detail` is deliberately not serialized; only hashes, statuses, kinds and
  timestamps are exposed.
- Event ordering is by durable sequence, so wall-clock manipulation cannot
  reorder the replay.
- Bound: default 500 events, hard maximum 2,000; step rows are the durable run's
  own rows. Replay errors are control-character-stripped and capped at 512
  characters.
- No provider credential, prompt, source body or authorization header is read or
  returned.
- The Office view displays only Core-provided data and renders no success or
  acceptance claim.

## 7. Limitations and remaining integration work

- `NOT_TESTED`: real Core/Compose browser journey for the trace panel at
  1440/834/390/320 px, including keyboard focus, console/network cleanliness and
  visual review in both themes.
- `NOT_TESTED`: background-refresh failure/offline banner under a real network
  interruption.
- `NOT_TESTED`: trace of a genuinely long production run with more than 2,000
  events and an interrupted/recovered Worker.
- `NOT_TESTED`: independent verifier/reviewer acceptance and Quality Gate
  decision for this candidate.
- The Office trace read polls Core; there is no SSE/live-stream claim.
- Other request surfaces (Director, Decision Inbox) link to the request
  workspace; the trace itself is intentionally one coherent surface rather than
  duplicated panels.
- Existing historical runs recorded before this change may have few step events.
  Their trace will truthfully show the durable transitions, `NO_EVENTS` where
  applicable and no invented step history.

## 8. Claim boundary

This evidence supports an **implemented, unit/component-tested, endpoint-tested**
trace view. It does **not** claim product acceptance, independent verification,
clean-install Compose acceptance, live provider evidence or browser acceptance.
