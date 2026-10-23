# F1-S4-D resume checkpoint

**Checkpoint:** 2026-10-22. **Code candidate:**
`be592770b77ca81c491e5021913e1d23d5a228b7`. S4-D code is committed. At this
checkpoint capture, the evidence/report and registry follow-up were pending a
docs-only commit; no docs-inclusive SHA is recorded here. After committing,
resolve the final SHA with `git rev-parse HEAD`. The prior S4-C real-Paseo
evidence remains bound to Factory head `9567bc5` / runtime `14133cb7`; it does
not verify S4-D.

## Completion campaign resume (2026-10-22)

The Product Owner directed goal-level autonomy, reusable standing authority,
real multi-WorkUnit execution and CAP-014 completion. ADR-0017 and the S4-D
contract were revised accordingly. The implementation now has:

- reusable delegation with atomic cumulative request/session/attempt accounting
  and a migration removing the legacy one-request uniqueness constraint;
- autonomous start for explicitly delegated R0/R1 risk, with audit event and
  projection, while risky or insufficient authority remains manual/blocked;
- immediate revocation containment for open requests and pending/running
  Campaigns;
- a bounded 1–8 unit dependency graph, per-unit frozen specs, Core-generated
  integration, candidate lineage, Git workspace chaining/merge and final
  integrated acceptance;
- a frozen CAP-014 corpus and blinded scoring/threshold harness (no model run).

Product code candidate: `9e12dcf0871105652160e328980bdec53d4fabd5`. Deterministic evidence is recorded in
[f1-s4d-workgraph-deterministic-2026-10-22.md](../evidence/f1-s4d-workgraph-deterministic-2026-10-22.md).
The multi-unit live envelope is prepared in
[f1-s4d-completion-budget.md](../implementation/f1-s4d-completion-budget.md);
`scripts/e2e_s4d_graph_live.py` is default-closed and does not execute live
work. No paid/live model session was started.

The authorized live round is now recorded in
[f1-s4d-live-certification-2026-10-22.md](../evidence/f1-s4d-live-certification-2026-10-22.md).
E2E used 2 of 4 sessions and did not complete a journey; CAP-014 used all 11
sessions but only 6 fixtures produced decodable outputs, so no score exists.
All provider sessions are closed/archived. A new Product Owner numeric envelope
is required for another E2E journey and a revised CAP-014 attempt. CAP-014
remains blocked; S4-D real certification is not complete.

## Resume status (2026-10-22, after PR #16 lint failure)

The interrupted session's PR head `5907a25` failed pinned `golangci-lint`
v2.5.0 with 32 findings. Repair commits on `f1-s4-d-intelligent-lead` fixed
them at source; the pinned command now reports 0 issues. On lint-repaired
product code candidate `602d2352f90f66abd0819de78aa48e905c3689b0`,
gofmt/gofumpt, vet, full tests, race, Office checks, architecture consistency
and deterministic S4-A/B/C passed; S4-C recorded 51/51 checks in
[f1-s4d-s4c-regression-pass-602d235.json](../evidence/f1-s4d-s4c-regression-pass-602d235.json).
Commit `5a14c24` adds S4-D to the paid-entrypoint guard (24 cases, zero Paseo
calls) and changes no product code.

Independent review found no P0/P1; all P2 findings were fixed, including
cleanup evidence corruption and compact-fallback coverage. An independent
verifier reproduced the pinned lint, full/race Go checks, targeted S4-D checks,
default-closed S4-D live preflight and architecture consistency on `559a68d`;
it did not rerun S4-A/B/C end-to-end. No lint rule, threshold, tool version or
test was removed. No paid or live model session was started, and live
authorization remains closed. The sections below preserve the pre-repair
`be592`/`5907a25` record as historical evidence.

## Current state

Core contains Lead intake, reusable standing delegations, MODEL assessment,
typed multi-unit WorkGraph validation, immutable per-unit plan/spec binding,
Core-generated integration, and reuse of the existing Campaign lifecycle.
Cumulative reservations cover requests, sessions and attempts across all
requests under the grant; idempotent replay returns the same request. The
Campaign ceiling covers assessment, every WorkUnit, integration and the bounded
repair horizon, and is not a provider cost cap. `CreateLeadAction` atomically
creates the Action, appends its creation event, and binds it to the reserved
request; retries cannot leave an unowned Action.

Office submits the Project, objective and required delegation ID; renders the
assessment, uncertainty, risks, proposal checks/budget, immutable base/profile/
route, permission, plan hash and Action/ActionSpec/Campaign links; and reloads
the Core projection and request event snapshot by URL ID. Starting a ready
Campaign remains a separate authenticated owner action after plan/spec review.
Office refreshes the event snapshot by GET; no SSE publication is claimed.

Paid assessment is default-closed. Core requires the complete
`--lead-paid-authorization-id`, `--lead-paid-project-id`,
`--lead-paid-base-revision`, `--lead-paid-provider`, `--lead-paid-model`,
`--lead-paid-paseo-mode`, and `--lead-paid-max-sessions` tuple. The grant POST
`Idempotency-Key` must equal the authorization ID and is idempotent to one
durable grant. The separate live runner requires `FACTORY_S4D_LIVE=1`, matching
`FACTORY_LIVE_AUTHORIZATION_ID`, an owner authorization envelope, and a clean
exact frozen Factory SHA. These guards are not live authorization; no paid run
has occurred.

Synchronous intake timeout changes are in the code candidate: Core
`WriteTimeout=300s`; live runner Lead POST timeout `360s`. The candidate suites
pass; the independent reviewer reported no P1 blocker on clean `be592`. The
prepared live runner reconciles assessment and Implementer session identities,
then checks Run, Attempt, frozen candidate, verification, review, Gate and
observed route/session count on success. It has not been run, so this is test
logic only, not a live result.

S4-D now supports a bounded 1–8 unit dependency graph with per-unit frozen
specs and Core-generated integration. CAP-013/015/017 bounded deterministic
behavior is verified on the completion code candidate. CAP-014's schema and
context fixtures plus the new blinded scoring harness are prepared, but its
deterministic acceptance still requires scored model-versus-baseline outputs,
so verification remains blocked. CAP-016 deterministic execution, dependency
chaining, fan-in integration, restart idempotency and conflict handling are
verified; real model/Paseo execution remains blocked. CAP-043 remains an
approved product objective in progress, not a completed capability. ADR-0017
and standing-delegation policy remain proposed pending Product Owner review.

## Open reviewer finding

The reviewer found that the contract's assessment example omitted the
runtime-required `effortEstimate` object. The schema, rules and deterministic
request trigger were corrected; Office now types and displays `unit`. These
changes are included in the code candidate and pass the listed Office checks.
Formal reviewer recheck and independent review/verifier on the final clean
code candidate `be592` completed: reviewer reported no P1 blocker; independent
verifier passed the scoped checks below. This does not establish independently
rerun S4-A/B/C end-to-end suites or live/model acceptance, or review of a future
docs-inclusive SHA.

## S4-C C17 browser regression

The first exact-candidate S4-C run at `3ad4836` failed C17 after 15 seconds
waiting for the Projects heading. Its durable result is
[f1-s4d-s4c-first-failure-3ad4836.json](../evidence/f1-s4d-s4c-first-failure-3ad4836.json).
A focused reproduction traced the React crash to the
legacy `Default Project` projection returning `browserEndpoints: null` while
Office unconditionally called `.join()`. Office now types this field as
nullable and renders it safely. Typecheck, lint and build pass.

The first post-fix focused C17 attempt reached Project registration, Action,
ActionSpec and Campaign start, then stopped because the ad hoc Worker omitted
the browser environment required by the harness. Its durable result is
[f1-s4d-s4c-focused-incomplete-worker-env.json](../evidence/f1-s4d-s4c-focused-incomplete-worker-env.json).
The fixed code candidate `be592` then passed the full S4-C deterministic suite
(51 checks), including C17, in
[f1-s4d-s4c-regression-pass-be592770.json](../evidence/f1-s4d-s4c-regression-pass-be592770.json).
That suite used a deterministic Paseo stub, not a provider.

## Ownership and validation

| Area | Owner / status |
| --- | --- |
| Core Lead implementation and Go tests | Core implementer; full candidate Go checks passed |
| Office and S4-D docs/registry | Office/docs implementer; code is in candidate; docs follow-up was pending commit at checkpoint capture |
| Independent verification and review | Separate verifier/reviewer; final clean `be592` review and scoped verification complete |
| Paid/live model journey | Product Owner authorization required; not authorized |

On code candidate `be592770b77ca81c491e5021913e1d23d5a228b7`:
`go test ./...`, `go vet ./...`, and `go test -race ./...` **PASS**; gofmt,
Office typecheck/lint/build and architecture consistency **PASS**. The
candidate-bound S4-A, S4-B and full S4-C deterministic scripts also **PASS**.
Evidence and limitations are summarized in
[f1-s4d-deterministic-2026-10-22.md](../evidence/f1-s4d-deterministic-2026-10-22.md).
Independent review of clean `be592` reported no P1 blocker. Independent
verification passed targeted Go/race/vet, 10 race-enabled grant reservation
attempts, API no-spec guard, Office typecheck/lint/build, and Chromium null
fixture. It did not independently rerun S4-A/B/C end-to-end or perform live
model acceptance. After committing the docs follow-up, resolve final HEAD with
`git rev-parse HEAD` and record that docs-inclusive SHA; the test results above
remain bound to the code candidate `be592`. An interrupted intake caller must
retry with the same idempotency key.

Office `npm run typecheck`, `npm run lint`, and `npm run build` **PASS**. The
mocked `office/e2e-browser/s4d-journey.mjs` **PASS** on loopback Vite with all
API traffic intercepted; it exercised submit, projection, event snapshot,
reload and explicit start without contacting Core, Paseo or a model provider.

## Next tasks, in dependency order

| Order | Task | Owner | Dependency / done when |
| --- | --- | --- | --- |
| 1 | Resolve CI lint blockers | Release owner | DONE: pinned linter reports 0 issues on `602d235` |
| 2 | Re-run deterministic checks on the lint-repaired candidate | Release owner | DONE: Go/race, Office, architecture consistency and S4-A/B/C; S4-C 51/51 on `602d235` |
| 3 | Cover S4-D in the paid-entrypoint guard | Release owner | DONE: `5a14c24`; 24 cases and zero Paseo calls |
| 4 | Decide delegation/paid-use policy and authorize any live session | Product Owner | Pending; ADR-0017 is not ratified and gates are not authorization |
| 5 | Run authorized live journey | Live-run owner | Blocked pending explicit candidate/route/session envelope; record outcome |
| 6 | Deliver PR #16 for review | Release owner | Update PR, inspect CI, do not merge without Product Owner review |

## Limits and known risks

- No S4-D real model result, paid session, Product Owner UAT, deployment or
  isolation certification is claimed. Scoped independent verification passed;
  no independent S4-A/B/C reruns or live model acceptance are claimed. Local
  S4-A/B/C reruns passed on `602d235`; the final head adds only the S4-D
  entrypoint guard and documentation.
- TRUSTED_LOCAL retains broad host access. SEC-01 isolation remains blocked and
  unverified; this work does not change that boundary.
- SQLite transient lock contention remains open in the reliability register.
- One WorkUnit does not provide parallel/dependency-aware execution. A
  delegation/session ceiling does not establish provider billing or quota.
- CI on the final docs-inclusive head is pending; live authorization and
  Product Owner decisions remain outstanding. Scoped review found no P1
  blocker; it is not a real-model or independently rerun S4-A/B/C
  certification.
