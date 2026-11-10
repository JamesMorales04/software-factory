# WP10 — Office browser acceptance evidence

**Status:** Implementer self-test evidence. This is **not** independent
acceptance, Quality Gate acceptance, Product Owner visual approval or
certification. Browser/Compose acceptance and independent verification remain
pending. No paid model inference was used.

- Work package: WP10 — real browser acceptance for the integrated Quality-first
  Wave 1/2 Office surfaces (with WP12-style per-option evidence)
- Branch: `factory/quality-wp10-browser-acceptance`
- Base/integrated commit: `452bb40`
- Code candidate: `71b63fd4640a4ca34e28441e4c79b0bd9759adc4`
- Evidence run date: 2026-11-10 UTC
- Evidence directory:
  [`docs/evidence/wave12-office-acceptance-2026-11-10/`](../evidence/wave12-office-acceptance-2026-11-10/)
- Machine-readable manifest:
  [`manifest.json`](../evidence/wave12-office-acceptance-2026-11-10/manifest.json)

The manifest records the exact code candidate SHA, browser version, node
version, seeded request IDs, all 39 matrix rows, every screenshot and every
expected/unexpected HTTP status. The commit containing this document records
the evidence snapshot; the code candidate is `71b63fd`.

## 1. Scope and claim boundary

The run proves that the real Office browser application, served by Vite and
backed by an isolated real Rust Core process with a disposable SQLite database,
renders and invokes the following surfaces through their authenticated Core
APIs:

- Director plan review: planning lane, artifact summaries, stack,
  architecture, documentation, skills, Definition of Done, OpenSpec, pending
  review, approve, request changes, reject, missing-artifact blocking,
  rationale validation and the exact revision/hash guard.
- Reviewer independence: required roles, assigned principals, per-role
  receipts (`PASS`, `CHANGES_REQUESTED`, `FAIL`, missing) with evidence and the
  frozen-plan assignment mismatch alert.
- Reviewer findings: resolve with evidence, refute with evidence and an
  authenticated owner decision, empty/invalid states and success/error.
- Workflow trace: loading, no-run empty, Core error plus retry, populated run
  facts, duplicate/missing-transition/open/terminal/truncated/malformed
  alerts, step trace, ordered event log with redaction, rejected replay, long
  lists and EN/ES responsive layouts.
- Skills technology-pack refresh: due/refreshable/blocked counts, source-only
  content-review requirement, explicit refresh action, loading, error/retry
  and empty registry.

The run does **not** prove live model inference, a model-produced planning
campaign, Compose topology behavior, scheduler restart recovery, a real
multi-reviewer Campaign, Windows, independent security review, deployment or
independent acceptance. It also does not prove product readiness. See
[§8](#8-limitations-and-not-tested).

## 2. Environment and fixture provenance

| Item | Value |
|---|---|
| Core | Real `factory-core` debug binary built with the debug-only `test-support` feature |
| Core transport | Loopback HTTP on a dedicated random port; disposable temp SQLite database |
| Office | Real Vite dev server on a dedicated random loopback port |
| Browser | Chromium `156.0.8078.4` via the pinned `playwright-core` cache |
| Node | `v22.23.2` |
| Viewports | 1440, 834, 390 and 320 px |
| Languages | English and Spanish (persisted locale switch) |
| Isolation | Dedicated temp project root, database, reviewer token files and ports; never the installed stack |

### Seeded state (labelled)

The Core handlers `/test-support/wave12/seed` and
`/test-support/wave12/trace-fault/{id}` are compiled only with the
`test-support` feature. `native/src/lib.rs` rejects that feature outside debug
builds. The seed endpoint writes exact durable rows for:

- a FORMAL_SPEC request with PLAN, STACK, ARCHITECTURE, DOCUMENTATION, SKILL,
  DOD and OPENSPEC artifacts, a validated WorkGraph, a frozen spec and an
  OpenSpec projection;
- a missing-DOD variant, a reject variant, a stale-review-hash variant and a
  reviewer-assignment-mismatch variant;
- a Campaign, candidate and four reviewer receipts (FUNCTIONAL PASS,
  ARCHITECTURE PASS, CODE_QUALITY CHANGES_REQUESTED, SECURITY FAIL; TESTING has
  no receipt);
- two OPEN findings and one owner decision resolved as `APPROVED` by the
  authenticated browser device principal;
- populated, gap, duplicate, long, rejected and retry workflow traces;
- persisted `rust` and `typescript` due/refreshable technology packs, a
  source-only `typescript` revision and a due `legacy-tool` revision with no
  builtin refresh source.

The browser always used the real Core HTTP API and SQLite projections. The
fixture is not model output, grants no authority and is clearly labelled in the
manifest. The duplicate-sequence projection is returned by a test-support
in-memory override because the durable `events.sequence` primary key cannot
store a duplicate sequence; the override still calls the real
`factory_native::trace::project` projection and redaction path.

The trace fault is a one-shot (two-read) test-support fault. React StrictMode
invokes the trace effect twice in development, so two reads fail before the
explicit Retry reads the real projection.

## 3. Per-option matrix

| ID | Surface | Control or state | Expected result | Evidence | Result |
|---|---|---|---|---|---|
| WP10-UI-001 | `/director` | Director plan lane, artifacts, stack, architecture, docs, skills, DoD and OpenSpec | all summary facts come from the seeded Core planning projection | `WP10-UI-001-ready-1440.png` | PASS |
| WP10-UI-002 | `/director` | Plan review rationale validation | an empty rationale is rejected before any Core write | Browser assertion; `manifest.json` | PASS |
| WP10-UI-003 | `/director` | Approve the exact plan revision | OWNER_APPROVED review and FROZEN workflow state | `WP10-UI-003-approved-1440.png` | PASS |
| WP10-UI-004 | `/director` | Missing Definition of Done artifact | the missing artifact is named and Approve is disabled while request-changes remains available | `WP10-UI-004-missing-1440.png` | PASS |
| WP10-UI-005 | `/director` | Request changes on the plan | CHANGES_REQUESTED review and REPLANNING workflow state | Browser assertion; `manifest.json` | PASS |
| WP10-UI-006 | `/director` | Reject the plan | REJECTED review and BLOCKED workflow state | `WP10-UI-006-rejected-1440.png` | PASS |
| WP10-UI-007 | `/director` | Exact revision/hash guard | a stale plan hash is refused by Core and the review stays pending | `WP10-UI-007-stale-error-1440.png` | PASS |
| WP10-UI-008 | `/director` | Director English/Spanish responsive layout | all review controls remain reachable without document overflow at 1440/834/390/320 | `WP10-UI-008-es-390.png` | PASS |
| WP10-UI-009 | `/director` | Keyboard and naming semantics | interactive controls have accessible names and the document language follows the switch | Browser assertion; `manifest.json` | PASS |
| WP10-UI-010 | `/director` | Reviewer independence required roles and assigned principals | five roles and five distinct assigned principals | Browser assertion; `manifest.json` | PASS |
| WP10-UI-011 | `/director` | Per-role reviewer receipts | two PASS, one CHANGES_REQUESTED and one missing receipt are visible with evidence | Browser assertion; `manifest.json` | PASS |
| WP10-UI-014 | `/director` | Frozen reviewer assignment mismatch | the alert is shown when required roles have no exact principal assignment | `WP10-UI-014-mismatch-1440.png` | PASS |
| WP10-UI-015 | `/director` | Resolve a reviewer finding with evidence | the OPEN finding becomes RESOLVED and the Core disposition is projected | `WP10-UI-015-resolved-1440.png` | PASS |
| WP10-UI-017 | `/director` | Refute requires an owner decision | the Refute control stays disabled until a decision reference is entered | Browser assertion; `manifest.json` | PASS |
| WP10-UI-018 | `/director` | Invalid owner decision reference | Core rejects the refutation and the Office shows the error | Browser assertion; `manifest.json` | PASS |
| WP10-UI-019 | `/director` | Refute with the approved owner decision | the OPEN finding becomes REFUTED with the authenticated owner principal | `WP10-UI-019-refuted-1440.png` | PASS |
| WP10-UI-020 | `/requests/:requestId` | Workflow trace loading state | the loading label appears while the Core read is pending | Browser assertion; `manifest.json` | PASS |
| WP10-UI-021 | `/requests/:requestId` | No-run empty state | the trace explains that no durable run exists | Browser assertion; `manifest.json` | PASS |
| WP10-UI-022 | `/requests/:requestId` | Core error and retry | a Core failure shows the error state and Retry reads the real projection | `WP10-UI-022-error-1440.png` | PASS |
| WP10-UI-023 | `/requests/:requestId` | Populated run facts | run identity, durable state, revision, replayed state, terminal outcome and counts | `WP10-UI-023-populated-1440.png` | PASS |
| WP10-UI-024 | `/requests/:requestId` | Trace alerts: malformed, open step and terminal | each structured alert is labelled and carries its sequence or step detail | `WP10-UI-023-populated-1440.png` | PASS |
| WP10-UI-025 | `/requests/:requestId` | Missing-transition alert | a gap in the durable state chain is reported | `WP10-UI-025-gap-1440.png` | PASS |
| WP10-UI-026 | `/requests/:requestId` | Duplicate-sequence alert | the test-support projection surfaces a duplicate sequence without inventing state | `WP10-UI-026-duplicate-1440.png` | PASS |
| WP10-UI-027 | `/requests/:requestId` | Long lists and truncation | 200 events and 500 steps are shown with explicit truncation alerts | `WP10-UI-027-truncated-1440.png` | PASS |
| WP10-UI-028 | `/requests/:requestId` | Step trace and event log | durable steps, open marker, ordered events and redacted payloads | `WP10-UI-028-event-log-1440.png` | PASS |
| WP10-UI-029 | `/requests/:requestId` | Negative redaction | tokens, prompts, content, API keys and unknown values never reach the DOM | `WP10-UI-028-event-log-1440.png` | PASS |
| WP10-UI-030 | `/requests/:requestId` | Rejected replay state | a rejected replay is shown as unknown/rejected, never as in progress | `WP10-UI-030-replay-rejected-1440.png` | PASS |
| WP10-UI-031 | `/requests/:requestId` | Trace English/Spanish responsive layout | the trace panel has no document overflow and keeps the event log readable at all widths | `WP10-UI-031-es-390.png` | PASS |
| WP10-UI-032 | `/skills` | Technology pack due, refreshable and blocked counts | Core reports three due/refreshable packs and a non-zero blocking count | `WP10-UI-032-counts-1440.png` | PASS |
| WP10-UI-033 | `/skills` | Source-only content-review requirement | a source-only pack is labelled and its reason is visible | `WP10-UI-033-source-only-1440.png` | PASS |
| WP10-UI-034 | `/skills` | Run the explicit refresh pass | Core refreshes exactly the due/refreshable persisted packs and keeps the blocked pack visible | `WP10-UI-034-refreshed-1440.png` | PASS |
| WP10-UI-035 | `/skills` | Skills loading state | the loading label appears while the Core reads are pending | Browser assertion; `manifest.json` | PASS |
| WP10-UI-036 | `/skills` | Skills error and retry | a failed read shows the error state and Retry recovers from real Core | `WP10-UI-036-error-retry-1440.png` | PASS |
| WP10-UI-037 | `/skills` | No registered skills empty state | the registry explains that no skill has been recorded | Browser assertion; `manifest.json` | PASS |
| WP10-UI-038 | `/skills` | Skills English/Spanish responsive layout | the refresh panel and pack list have no document overflow at all widths | `WP10-UI-038-es-390.png` | PASS |
| WP10-UI-039 | `/director` | Keyboard focus order and visible focus on plan review controls | Tab moves from the rationale to each review button with a visible focus style | Browser assertion; `manifest.json` | PASS |
| WP10-UI-040 | Core/Office boundary | Browser console, page and network cleanliness | no page errors and no unexpected 4xx/5xx responses | Browser assertion; `manifest.json` | PASS |
| WP10-UI-041 | EN + ES Office routes | No literal translation placeholders | visible EN and ES surfaces do not render unresolved {placeholder} tokens | Browser assertion; `manifest.json` | PASS |
| WP10-UI-042 | Core/Office boundary | Unauthorized Core read | an operator projection without the device session is rejected with 401 | Browser assertion; `manifest.json` | PASS |

## 4. Negative checks

The matrix above includes these negative conditions, not only happy paths:

| Negative condition | Evidence |
|---|---|
| Unauthenticated `GET /agentic/portfolio` with `credentials: omit` | `WP10-UI-042`: Core returned 401 |
| Empty plan-review rationale | `WP10-UI-002`: local error; no Core write |
| Missing Definition of Done artifact | `WP10-UI-004`: DOD named; Approve disabled |
| Stale/tampered plan SHA-256 | `WP10-UI-007`: Core returned 409 `PLAN_REVIEW_STALE`; review stayed `PENDING` |
| Required role without a principal assignment | `WP10-UI-014`: mismatch alert and five `Not assigned` rows |
| Missing reviewer receipt | `WP10-UI-011`: `No receipt` for TESTING |
| Changes-requested and failing reviewer receipts | `WP10-UI-011`: danger-toned `CHANGES_REQUESTED` and `FAIL` with `EXECUTED` evidence |
| Finding resolve with no evidence | `WP10-UI-015`: local error before the Core call |
| Refute with no owner decision | `WP10-UI-017`: Refute disabled |
| Refute with an unknown owner decision | `WP10-UI-018`: Core returned 400; error rendered |
| Duplicate trace sequence | `WP10-UI-026`: `Duplicate sequence` alert |
| Missing state transition | `WP10-UI-025`: `Missing transition` alert |
| Open step in a terminal run | `WP10-UI-024`: critical `Open step` alert |
| Malformed transition row | `WP10-UI-024`: `Malformed event` alert |
| Truncated event and step lists | `WP10-UI-027`: 200 events, 500 steps and both truncation alerts |
| Rejected replay | `WP10-UI-030`: `Unknown — replay rejected`, never `In progress` |
| Token/prompt/content/API-key/unknown payload leakage | `WP10-UI-029`: none of the seeded secret markers appeared in the DOM |
| Same rendered label for two step IDs | `WP10-UI-028`: distinct suffix labels asserted |
| Literal translation placeholders | `WP10-UI-041`: no unresolved `{placeholder}` on Director, trace or Skills |
| Horizontal document overflow | `WP10-UI-008`, `031`, `038` at 1440/834/390/320 px |
| Unexpected console/page/network failures | `WP10-UI-040`: no unexpected page errors, 4xx/5xx or console errors |

## 5. UI/Core defects found and fixed

| Defect | Impact | Fix | Evidence |
|---|---|---|---|
| Finding refutation required owner-decision status `RESOLVED`, but the owner-decision schema only permits `PENDING`/`APPROVED`/`DENIED`/`EXPIRED` | Refutation could never succeed through the real API | `owner_decision_allows_refutation` now requires the authenticated owner's scoped `APPROVED` decision; error copy and a Rust regression test updated | `WP10-UI-019`; `refutation_requires_the_authenticated_owners_approved_decision` |
| Director action notices/rationale/finding form state leaked across request selection | A success message from one request appeared on another request's plan-review panel | Reset action-scoped notices, rationale, finding fields and errors when the selected request changes; clear stale detail/plan/spec/planning projections while the new request loads | `WP10-UI-004` asserts the prior success notice is absent |
| A FORMAL_SPEC request with no spec yet said “This route does not require a formal specification” | Contradicted the lane and the visible Author specification action | Use `director.spec.empty` for the no-draft/no-frozen state; keep `notRequired` only for non-FORMAL routes | `WP10-UI-014` asserts the correct copy |
| `Order: {order}` rendered literally | Visible unresolved interpolation token | Catalog key is now the plain label `Order`/`Orden`; the value renders separately | `WP10-UI-041` |
| Step IDs rendered as `step-666` for multiple steps | Two rows were visually indistinguishable | Show the final eight hex characters of `step-<uuid>`; the full ID stays in the title | `WP10-UI-028` uniqueness assertion |
| Duplicate React key warning on the duplicate-sequence fixture | Console noise and potentially omitted payload rows | The test-support duplicate projection now runs payloads through `replay::redact_payload` instead of bypassing the redaction contract | `WP10-UI-040` console cleanliness |

No decorative action or invented state was added. The repaired Core operation
keeps Core as the authority: the browser cannot choose its principal, the
decision must be scoped to the request or project and the authenticated owner
must have approved it.

### Independent review repairs (2026-11-10)

The independent Codex GPT-6 Luna review at `fea8169` found two additional
issues. Both were repaired in the new code candidate.

| Finding | Resolution | Evidence |
|---|---|---|
| P2 — refutation accepted an unrelated owner approval | Core now requires the exact owner-decision matter `FINDING_REFUTATION`, the exact selected option `APPROVE_REFUTATION`, membership of that option in the decision's resolved option set, the authenticated owner, the `APPROVED` status and request/project scope. Unrelated matters, unrelated options and a missing option-set entry are negative tests. | `refutation_requires_the_authenticated_owners_approved_decision` |
| P3 — debug test-support trace-fault map was unbounded | The endpoint accepts only the seeded retry request (`bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb`), is bounded to one map entry, rejects unknown IDs with `400 TEST_TRACE_FAULT_ID_INVALID`, and returns `429 TEST_TRACE_FAULT_BOUNDED` if the bound is already occupied. It remains authenticated and test-support-only. | `trace_fault_is_bounded_and_restricted_to_the_seeded_retry_request` |

The exact refutation contract is therefore:

```text
matter          = FINDING_REFUTATION
status          = APPROVED
selected_option = APPROVE_REFUTATION
options         contains APPROVE_REFUTATION
resolved_by     = authenticated owner principal
scope           = the same request, or a project decision for the same project
```

A generic owner approval for deployment, policy or another matter cannot close a
reviewer finding.

## 6. Commands and results

All commands ran in this worktree on the code candidate unless stated.

### Rust

```text
cd native

cargo build -p factory-native --bin factory-core --features test-support
  PASS

cargo fmt --all -- --check
  PASS

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
  PASS

cargo test -p factory-native --lib
  829 passed; 0 failed; 4 ignored

cargo test -p factory-native --lib --features test-support wave12_fixture
  1 passed; 0 failed

cargo test -p factory-native --bin factory-core
  39 passed; 0 failed

cargo test -p factory-native --bin factory-core --features test-support
  43 passed; 0 failed

cargo test -p factory-native --bin factory-core --features test-support \
  refutation_requires
  1 passed; 0 failed

cargo test -p factory-native --bin factory-core --features test-support \
  trace_fault_is_bounded
  1 passed; 0 failed
```

An earlier full-lib run failed with `database or disk is full` because the
shared 16 GiB `/tmp` tmpfs was full of accumulated test SQLite artifacts from
earlier runs. After removing stale `/tmp/factory-*` test artifacts, the same
`cargo test -p factory-native --lib` command passed. This was an environment
cleanup, not a code or test change.

### Office

```text
cd office

npm test -- --run
  46 files, 317 tests passed (three consecutive clean runs)

An earlier run launched in parallel with typecheck and lint showed one
transient failure that did not reproduce in the three consecutive isolated
runs. The final isolated result is green.

npm run typecheck
  PASS

npm run lint
  PASS (164 files)
```

### Real browser acceptance

```text
# from the repository root
node office/e2e-browser/wave12-acceptance.mjs
  39 checks passed; 0 failed
  manifest: docs/evidence/wave12-office-acceptance-2026-11-10/manifest.json
```

The script starts its own isolated Core and Vite processes, seeds the fixture
after device enrollment, drives Chromium, captures screenshots and writes the
manifest. It does not touch the installed Compose stack. It was run after the
code candidate commit so the manifest is candidate-bound to `71b63fd`.

## 7. Evidence inventory

- `manifest.json` — candidate SHA, environment, seeded IDs, 39-row matrix,
  screenshots, expected/unexpected network statuses, console/page errors and
  limitations.
- 23 screenshots by matrix ID under
  `docs/evidence/wave12-office-acceptance-2026-11-10/`.
- The screenshots cover the plan-review panel, missing artifact, stale hash,
  mismatch, finding resolve/refute, populated trace, event log, gap, duplicate,
  truncation, rejected replay, trace error/retry, Skills counts, source-only
  reason, refresh result and the ES 390 px layouts.

## 8. Limitations and not tested

- **Compose acceptance — NOT_TESTED.** The run used an isolated host dev
  harness (real Core binary + Vite + Chromium) rather than the integrated
  Compose topology. Container-native runtime behavior, scheduler restart,
  nested Podman and the six reviewer services were not exercised here.
- **Model-produced planning — NOT_TESTED.** Requests, artifacts, reviewer
  receipts, findings, owner decision and traces were seeded test-support rows.
  No live provider, paid inference or model-generated plan was used.
- **Live multi-reviewer Campaign — NOT_TESTED.** Reviewer receipts are seeded
  durable rows; a real Campaign with an independent reviewer fleet and Gate
  aggregation was not run.
- **Duplicate sequence — seeded projection.** The durable ledger cannot store a
  duplicate primary key; the duplicate alert was exercised through the
  test-support in-memory override using the real trace module.
- **Skills read failure — browser-boundary simulation.** Loading and 503 states
  were produced by Playwright route delays/failures; the successful data and
  refresh action used real Core.
- **Trace fault — test-support injection.** The Core error/retry state used a
  seeded one-shot fault rather than a crashed or unavailable Core process.
- **Offline/background refresh — NOT_TESTED.** The trace's background-refresh
  offline banner was not exercised with a real network interruption.
- **Visual/owner approval — PENDING.** The screenshots were reviewed by the
  implementer for layout and copy defects; Product Owner visual approval is
  outstanding.
- **Independent acceptance and security review — PENDING.** This is
  implementer self-test evidence only.

## 9. Claim boundary

This document claims only that the enumerated Office controls and states were
exercised in Chromium against an isolated real Core process using labelled
test-support fixtures, and that the listed automated checks passed on the
recorded candidate. It does **not** claim independent acceptance, Quality Gate
acceptance, live inference, Compose certification, deployment, production
readiness or `TRUSTED_LOCAL` isolation. Any future certification must rerun
independent acceptance against the exact candidate and the canonical Compose
topology.
