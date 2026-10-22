# F1-S4-D resume checkpoint

**Checkpoint:** 2026-10-22. **Branch HEAD/base:** `102d7ba29295554bd61ae71956347a025d219d7b`.
S4-D changes are uncommitted. There is no frozen S4-D candidate SHA and no
S4-D PR. The prior S4-C real-Paseo evidence remains bound to Factory head
`9567bc5` / runtime `14133cb7`; it does not verify these changes.

## Current state

Core contains Lead intake, one-use standing delegations, MODEL assessment,
typed one-unit proposal validation, immutable plan/spec binding, and reuse of
the existing Campaign lifecycle. One delegation reserves one distinct request;
idempotent replay returns that request. Its session ceiling includes one
assessment session plus the bounded Implementer attempt horizon. It is not a
provider cost cap. `CreateLeadAction` atomically creates the Action, appends its
creation event, and binds it to the reserved request; retries cannot leave an
unowned Action.

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

Synchronous intake timeout changes have landed in the uncommitted tree: Core
`WriteTimeout=300s`; live runner Lead POST timeout `360s`. These provide headroom
for bounded assessment work but still need exact-candidate verification. The
prepared live runner reconciles assessment and Implementer session identities,
then checks Run, Attempt, frozen candidate, verification, review, Gate and
observed route/session count on success. It has not been run, so this is test
logic only, not a live result.

S4-D supports exactly one implementation WorkUnit. CAP-013–017 remain proposed
pending Product Owner review of ADR-0017 and delegation semantics. CAP-014's
semantic comparison has no scored results. CAP-043 is an approved product
objective in progress, not a completed capability.

## Open reviewer finding

The reviewer found that the contract's assessment example omitted the
runtime-required `effortEstimate` object. Current working-tree edits add the
required fields, status rules, and deterministic request trigger to the
contract, and add typed `unit` rendering to the Office projection and fixture.
Office `npm run typecheck` and `npm run lint` pass on this uncommitted tree.
Keep this finding open until the Core/Office owners report completion and the
reviewer confirms the contract and projection match the implementation.
Other independent review findings are pending recheck; no review certification
is claimed.

## Ownership and validation

| Area | Owner / status |
| --- | --- |
| Core Lead implementation and Go tests | Core implementer; targeted and full Go unit suite results below |
| Office and S4-D docs/registry | Office/docs implementer; changes remain uncommitted |
| Independent verification and review | Separate verifier/reviewer; not run |
| Paid/live model journey | Product Owner authorization required; not authorized |

Reported on the current uncommitted tree: `go test ./...` **PASS**;
`go vet ./...` **PASS**; targeted race check
`go test -race ./internal/domain ./internal/store ./internal/api ./internal/lead ./internal/orchestrator`
**PASS**. These results are not frozen-candidate evidence. Broad race coverage,
`golangci-lint`, exact-SHA consolidated checks and prior S1–S4-C acceptance
workflows remain pending. The timeout changes are implemented on this
uncommitted tree but exact-candidate verification remains pending; callers
should retry intake with the same idempotency key if the response is lost.

Office `npm run typecheck`, `npm run lint`, and `npm run build` **PASS**. The
mocked `office/e2e-browser/s4d-journey.mjs` **PASS** on loopback Vite with all
API traffic intercepted; it exercised submit, projection, event snapshot,
reload and explicit start without contacting Core, Paseo or a model provider.
`scripts/check-architecture-consistency --write-roadmap` and the read-only
consistency check **PASS**.

## Next tasks, in dependency order

| Order | Task | Owner | Dependency / done when |
| --- | --- | --- | --- |
| 1 | Finish Core changes; repair findings | Core implementer | Rerun affected tests |
| 2 | Reconcile assessment schema and Office projection | Office/docs + Core | Office typecheck/lint pass; Core owner confirms schema and reviewer rechecks the matching contract/projection |
| 3 | Freeze/check candidate; rerun S1–S4-C | Release owner | Steps 1–2 done; SHA/results recorded |
| 4 | Recheck independent findings and assess security | Independent reviewer | Fixes landed; review all findings on frozen candidate; no certification until disposition |
| 5 | Decide remaining delegation/paid-use policy and authorize any live session | Product Owner | ADR-0017 reviewed; explicit candidate/route/session envelope; gates alone are insufficient |
| 6 | Run authorized live journey | Live-run owner | Frozen clean candidate, exact SHA and owner authorization; record outcome and session evidence |
| 7 | Prepare S4-D PR | Release owner | Candidate checks, review disposition, and required acceptance evidence recorded |

## Limits and known risks

- No S4-D real model result, paid session, independent verification, Product
  Owner UAT, deployment or isolation certification is claimed.
- TRUSTED_LOCAL retains broad host access. SEC-01 isolation remains blocked and
  unverified; this work does not change that boundary.
- SQLite transient lock contention remains open in the reliability register.
- One WorkUnit does not provide parallel/dependency-aware execution. A
  delegation/session ceiling does not establish provider billing or quota.
- Exact-candidate checks, independent review, live authorization and PR remain
  outstanding.
