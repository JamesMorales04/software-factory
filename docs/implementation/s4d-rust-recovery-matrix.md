# S4-D capability recovery matrix (PR #16 → native Rust)

**Purpose:** account for every material F1-S4-D capability in legacy PR #16 and
track whether the Rust replacement preserves the product behavior and
invariants without copying the obsolete Go/Paseo implementation.

**Reference state:** PR #16 `f1-s4-d-intelligent-lead` head
`19cb4951f7a3d42a1ca27b1800cf49401c81ecc6`; last product code candidate
`9e12dcf0871105652160e328980bdec53d4fabd5`; base `main`
`102d7ba29295554bd61ae71956347a025d219d7b`. Verify these SHAs again before
relying on this document; it is a recovery inventory, not proof that PR #16 is
certified.

**Rust target:** the native Rust workspace on
`native-rust-replacement-20261023` (eventual cutover layout moves it to
`core/`). Existing Rust status is stated truthfully below; most S4-D behaviors
are **not implemented** in Rust.

## Status vocabulary

| Status | Meaning |
|---|---|
| `RUST_NOT_IMPLEMENTED` | No Rust behavior exists; PR #16 is the behavior oracle only. |
| `RUST_PARTIAL` | Some Rust type, table or pure function exists, but no integrated behavior or acceptance. |
| `RUST_IMPLEMENTED_UNVERIFIED` | Behavior exists but lacks the required deterministic/negative/process tests. |
| `RUST_VERIFIED_DETERMINISTIC` | Candidate-bound deterministic tests pass at the exact SHA. |
| `RUST_VERIFIED_LIVE` | Exact-SHA, authorized real provider evidence exists. Never inferred. |
| `NOT_TRANSFERABLE` | Runtime-specific or obsolete; behavior must be redesigned, not ported. |
| `HISTORICAL` | Evidence/provenance only; never reused as Rust certification. |

Deletion readiness is **NO** until the corresponding Rust behavior and required
evidence exist. No row authorizes merging PR #16, live spend, risk-policy
change, merge or deployment.

## A. Product capabilities

| # | Capability | PR #16 source and behavior | Invariants and constraints | Rust destination | Reusable oracle / fixture | Required adaptation | Rust status | Verification | Remaining risks | Delete legacy? |
|---|---|---|---|---|---|---|---|---|---|---|
| A1 | **Semantic intake and typed assessment.** Natural-language objective → advisory assessment with intent, complexity, effort, knowledge gaps, change impact, validation needs, uncertainty and suggested next action. | `core/internal/lead/assessment.go` (`Assessment`, `DecodeAssessment`, `Validate`, `EffortEstimate`); `core/internal/lead/paseo_assessor.go`; contract §4; `docs/implementation/f1-s4d-checkpoint.md`. | Mechanism `MODEL` under `HYBRID` Core validation. Evidence-bound judgments; explicit unknowns; bounded notes/evidence refs; closed schema; unknown fields/duplicate keys/trailing JSON rejected; proposal cannot execute for non-`change` intent without owner clarification; model output never authorizes. | New Rust `domain::lead` assessment types and a `provider::Assessor` port; Core validator in `quality`/`application`; no Paseo. | Assessment JSON schema and negative cases in `assessment.go`; contract examples; CAP-014 fixtures S4D-01..11; the 5 live failure observations as defect lessons. | Recreate strict schema with `serde(deny_unknown_fields)` plus a shape/duplicate-key validator, typed effort states, and Core-owned identity binding. Direct Codex/OpenCode assessor later; deterministic baseline remains comparison only. | `RUST_PARTIAL` (typed domain only) | 14 native `lead` unit tests cover strict schema, identity, evidence, effort and non-change executable rejection. No model call, persistence or API. CAP-014 live collection captured 6/11 and produced no score; do not reuse as evidence. | Model quality unproven; timeout reliability poor (5 failures); schema drift; prompt/evidence leakage. | NO |
| A2 | **Deterministic proposal validation and plan freezing.** Validates model proposal against Project/Action/base/config/delegation; derives immutable per-unit specs and `planHash`. | `core/internal/domain/lead_plan.go` (`LeadPlanProposal`, `Validate`, `validateWorkUnits`, `UnitScope`, `ContentHash`); `core/internal/api/lead_handlers.go` (`freezeLeadSpecs`, `freezeLeadSpec`); `core/internal/store/lead.go` (`CreateLeadProposal`, `FreezeLeadPlan`). | Full Git base ID; Project config hash match; exact grant route/risk/class/scope/capability; no path escape; check-kind validity; session/attempt ceilings include assessment; write-once proposal/spec/campaign links; plan hash binds proposal + delegation + config. | Rust `domain::lead::proposal`, `application::lead::freeze`, persistence tables; reuse Rust `ActionSpec` and Campaign creation. | `lead_plan_test.go` table (binding, scope, capability, risk, route, budget, session escalation, graph shape); live failure where invalid `syntax` check kind was rejected (attempt 1). | Preserve deterministic validation; replace Go Project/config types with Rust typed IDs and digest-bound snapshots; derive specs through Rust ActionSpec constructors. | `RUST_PARTIAL` (domain validation only) | Native `LeadPlanProposal` validation and plan hashing covered by the same 14 `lead` tests, including binding, scope, route, risk, budget and graph negatives. No persistence/freeze. | A model could exploit permissive unknown-field handling, stale base/config, or check-kind drift if adaptation is loose. | NO |
| A3 | **Standing delegation.** Reusable, scoped, cumulative authority for multiple requests. | `core/internal/domain/lead_plan.go` (`StandingDelegation`, `ComputeContentHash`, `Validate`); `core/internal/store/lead.go` (`CreateStandingDelegation`, `reserveDelegationTx`, `GetDelegationUsage`); `core/internal/api/lead_handlers.go`; ADR-0017 §1; contract §3. | Authenticated issuer/principal and audit identity; one Project; immutable revision/content hash excluding revocation; allowed paths/capabilities/classes/risk/route; per-Campaign and cumulative request/session/attempt ceilings; atomic reservation; idempotent creation; expiry; one-way revocation; old schema fails closed; no implicit defaults; model cannot mint or expand. | Rust `domain::authorization::StandingDelegation`, SQLite tables + atomic reservation, Core API; distinct from Worker registration credential. | `lead_plan_test.go` delegation cases; `store/lead_test.go` reservation/concurrency/migration cases named in the deterministic evidence. | Keep HMAC/HMAC-derived secret handling separate from grant authority. Rust must not accept a bootstrap Worker credential as a grant. Land policy fields only when ratified. | `RUST_NOT_IMPLEMENTED` (only Project/Action and Worker credentials exist). | None. | Unratified issuer/risk policy; cumulative budget evasion through child objects; clock/expiry semantics; revocation race. | NO |
| A4 | **Autonomous Campaign start and audit.** Start a frozen plan without per-transition human approval only within delegated R0/R1 authority. | `core/internal/api/lead_handlers.go` (`maybeAutoStartLeadRequest`, auto-start event); `core/internal/store/campaigns.go` (`StartLeadCampaign`); ADR-0017 §2–3; contract §6. | Only active/unrevoked/unexpired grant; exact bindings; risk in `autonomousRiskClasses`; R2/R3 explicit approval; cumulative reservation succeeds; Campaign pending and fully covered; durable audit event naming delegation/risk/basis; manual start remains; model cannot start itself. | Rust `application::campaign::start` and authorization policy; same Campaign lifecycle. | Campaign start crash test; autonomous/revocation tests in `store/lead_test.go`; `f1-s4d-workgraph-deterministic` list. | Do not implement until policy is ratified; fail closed with exact missing authority. One transaction for Campaign + Lead projection + audit. | `RUST_NOT_IMPLEMENTED` | None. | Policy not ratified; activating autonomy accidentally would be a protected change. | NO |
| A5 | **Immediate revocation containment.** Revocation blocks new reservations/dispatch and contains in-flight work without fabricating termination. | `core/internal/store/lead.go` (`RevokeStandingDelegation`, `ContainDelegationRevocation`, `CheckLeadCampaignAuthorization`); `core/internal/domain/lead_plan.go`; ADR-0017 §4. | One-way revocation; no new model sessions/repairs/effects; pending/running Campaigns blocked/cancelled; active Runs cancelled through fencing; unresolved external sessions stay unresolved; evidence preserved; repeated revoke idempotent. | Rust authorization + lifecycle transaction with durable events; provider session reconciliation port. | Revocation containment test list; live run cleanup observations. | Reconcile provider/container state explicitly; a cancellation request is not a stopped session. Avoid claiming cleanup without evidence. | `RUST_NOT_IMPLEMENTED` | None. | Unresolved-session containment; stale dispatch; partial cleanup. | NO |
| A6 | **Bounded dependency-aware WorkGraph.** 1–8 units with typed dependencies, per-unit scope/capabilities/specs, integration node. | `core/internal/domain/lead_plan.go` (`LeadWorkUnit`, `validateWorkUnits`, `pathsOverlap`, `TerminalUnits`, `NeedsIntegration`); `core/internal/domain/campaign.go` (`BuildLeadCampaignPlan`). | Unique slug IDs; up to 8 predecessors; DAG/no cycles/self-deps/unknown refs; parallel path-overlap rejection; per-unit checks or global fallback; Core-generated integration only for multiple terminals; final integrated acceptance only. | Rust `domain::work_graph` + `application::scheduler`; one Campaign/Run/Attempt lifecycle. | `lead_plan_test.go` graph table; sequential/parallel/fan-in/failure/restart tests in `f1-s4d-workgraph-deterministic`; fixture `scripts/fixtures/s4d-graph/**`. | Implement as bounded data, not a general workflow DSL. Reject before spec/Campaign creation. Unit IDs and plan hash immutable. | `RUST_PARTIAL` (domain validation only) | Native WorkGraph validation covers 1–8 units, DAG/cycle/self/unknown/duplicate, parallel path overlap, per-unit scope/capabilities, closed check kinds, integration need and session ceilings. No scheduler/dispatch. | Scope overlap; budget miscount; second scheduler; frozen-plan drift. | NO |
| A7 | **Dependency-aware execution and restart.** Ready only after all dependencies succeed; sequential base chaining, parallel independent units, multi-predecessor fan-in; restart idempotency. | `core/internal/store/campaigns.go` (`PromoteReadyTasks`, `DispatchImplementer`, `dependencyCandidatesTx`, `ClaimParticipantTask`); `core/cmd/factory-worker/candidate.go`; deterministic evidence list. | One authoritative scheduler; lease/fencing; failed dependency stops downstream; parallel candidates not invalidated by siblings; restart reconstructs state; no double dispatch. | Rust scheduler and Worker claim/dispatch; CandidateRevision lineage. | `TestWorkGraphSequentialBaseChaining`, `TestWorkGraphParallelFanInIntegration`, `TestWorkGraphParallelUnitCompletionDoesNotInvalidateSiblingCandidate`, `TestWorkGraphOrdinaryUnitFanInConsumesBothPrerequisites`, `TestWorkGraphDependencyFailureStopsDownstreamDispatch`, `TestWorkGraphRestartPreservesDependencyAndDispatchIdempotency`. | Port test intent, not Go package structure. Add process-level Core/Worker restart tests and stale-generation negatives. | `RUST_NOT_IMPLEMENTED` (fixture lease/fencing only; public dispatch disabled). | None. | No public dispatch; no crash recovery; no verified fencing end-to-end. | NO |
| A8 | **Deterministic Git integration.** Conflict-free merge of unit candidates with no model session; honest conflict failure. | `core/cmd/factory-worker/candidate.go` (`integrate`, stage on base); `core/cmd/factory-worker/paseo_task.go` (`git-integration`, `providerSession: false`); `candidate_workspace_test.go`; `core/internal/domain/candidate.go` integration rules. | Exact parents recorded; integrated candidate identity; deterministic merge only; conflicts fail closed without paid recovery; intermediate unit completion is not final acceptance; no provider/session budget consumed. | Rust `git` module and CandidateRevision validator; Worker integration step. | `TestPrepareSpecWorkspaceSequentialAndIntegration`, `TestPrepareSpecWorkspaceRejectsIntegrationConflict`, integrated lineage test; fixture graph. | Port algorithms and failure semantics; use Rust process spawning and verified Git identity. Do not add a model repair loop. | `RUST_NOT_IMPLEMENTED` (Rust workspace validates a single frozen base parent for a candidate). | None. | Candidate parent/lineage for multi-parent integration; conflict classification; allowed-path containment. | NO |
| A9 | **Crash-consistent Lead/Campaign start.** Campaign start, Lead ready→running and audit are one transaction with idempotent retry. | `core/internal/store/campaigns.go` (`StartLeadCampaign`); `TestStartLeadCampaignIsAtomicAcrossCrashBoundary`; checkpoint §completion. | No partial state after failure injection; replay is idempotent; projection cannot disagree with Campaign; failure leaves no half-start. | Rust persistence transaction + recovery test; same authoritative Campaign. | Crash failure-injection test; restart idempotency cases. | Use `BEGIN IMMEDIATE` and transaction-scoped event/audit; test with real SQLite and process kill. | `RUST_NOT_IMPLEMENTED` (Rust Campaign creation is separate and blocked). | None. | Split transactions; duplicate start; projection drift. | NO |
| A10 | **CAP-014 semantic evaluation.** Frozen 11-fixture corpus, deterministic baseline, blinded scoring, thresholds. | `core/internal/lead/{baseline,evaluation}.go`; `core/internal/lead/testdata/s4d-evaluation-corpus.json`; `core/cmd/s4d-evaluate`; `core/cmd/s4d-baseline`; `docs/quality/s4-d-semantic-evaluation-fixtures.md`. | Corpus/digest frozen before outputs; baseline frozen; independent blinded evaluator; dimensions and critical failures; no exclusion of difficult fixtures; no post-result edits; new corpus version + new digest for any change; baseline is not model-quality evidence. | Rust `evaluation` module/tool + versioned fixture files; CODEOWNERS-like custody is process, not code. | Corpus prose and machine JSON; baseline rules `S4D-RULES-1`; scoring tests; the 6 captured raw outputs as historical input only. | Reuse fixture semantics; if Rust requires a new schema, publish `S4D-SEMANTIC-2` with documented changes and a new digest. Keep original corpus immutable. Rust tool must never contact a provider for scoring. | `RUST_NOT_IMPLEMENTED` (Rust has a separate Quality Gate evaluator, not CAP-014). | No independent score exists; PR #16 live run 6/11 captured, 5 failed. | Frozen threshold adaptation; evaluator independence; model/baseline comparability; no proof of semantic quality. | NO |
| A11 | **Office Lead and Campaign views.** Submit objective/delegation, show assessment/uncertainty/risks/checks/budget, request state, Campaign graph and audit, reload by URL. | `office/src/views/Lead.tsx`, `Campaigns.tsx`, `CampaignDetail.tsx`; `office/e2e-browser/s4d-journey.mjs`; `s4d-graph-live.mjs`. | Office is projection only; no client authority; authenticated routes; reload reconstructs from Core; no claim of SSE if using polling snapshots. | React/TypeScript retained, rewired to Rust Core API and SSE/event cursor. | Mocked journey assertions; browser fixture `scripts/fixtures/s4d-graph`. | Replace Go DTO shapes; type safe API client; do not carry Paseo-specific fields. | `RUST_NOT_IMPLEMENTED` (Office still targets legacy API). | Only legacy/mocked journeys exist; no Rust-backed journey. | API shape drift; reload/reconnect; unauthorized view. | NO |
| A12 | **Bounded live certification runners and budget ledger.** Default-closed, exact-SHA/route/budget-bound runners, session discovery/cleanup, durable evidence. | `scripts/e2e_s4d_graph_live.py`, `e2e_s4d_semantic_live.py`, `s4d_budget_ledger.py`, `e2e_s4d_live.py`; `docs/implementation/f1-s4d-completion-budget.md`; evidence. | No live work without explicit envelope; exact clean SHA; exact provider/model/mode; numeric session ceiling; discover every session; stop on overrun; cleanup/archive; preserve raw evidence; never fabricate cost or cleanup. | Rust-native authorized test harness or governed script, reusing Rust Core APIs and a durable budget ledger. | `test-s4d-budget-ledger.py`, guard tests (30 cases, zero provider calls), default-closed preflight. | Do not port runner internals verbatim; preserve gates and evidence schema. Rust Core must own reservations; runner must not become a second budget authority. | `RUST_NOT_IMPLEMENTED` | Historical runs used 2/4 E2E and 11/11 CAP-014; incomplete, not certified. | Cost/quota unobservable; session discovery gaps; cleanup false positives. | NO |

## B. Engineering mechanisms worth preserving

| # | Mechanism | Legacy source | Rust destination / rule | Status |
|---|---|---|---|---|
| B1 | Strict closed-schema JSON with duplicate-key/trailing-input rejection | `core/internal/lead/assessment.go` | `serde(deny_unknown_fields)` plus explicit pre-validation for duplicate keys and trailing input; typed effort enum | `RUST_PARTIAL`: typed decoder and negatives pass; no provider input |
| B2 | Proposal/plan content hashing and write-once freezing | `lead_plan.go` `ContentHash`; `store/lead.go` freeze functions | Canonical JSON digest; immutable frozen rows; hash excludes mutable revocation only | `RUST_PARTIAL`: proposal/delegation/plan hash domain implemented; no freeze persistence |
| B3 | Cumulative atomic reservation | `reserveDelegationTx`, `releaseDelegationTx`, concurrency test | SQLite `BEGIN IMMEDIATE`; all counters in one transaction; checked arithmetic; idempotent replay | `RUST_NOT_IMPLEMENTED` |
| B4 | Idempotency bound to operation + payload digest | store/API across Lead and Campaign | Same rule as existing Rust Project/Action intake; different payload = conflict | `RUST_PARTIAL` (base pattern exists; Lead not implemented) |
| B5 | Atomic Action creation + Lead binding | `CreateLeadAction`, `SetLeadActionID` | One transaction creates Action/event and binds request; retry cannot orphan | `RUST_NOT_IMPLEMENTED` |
| B6 | Fail-closed boot reconciliation | `ReconcileLeadRequestsOnBoot` | `assessing` with unknown external state → blocked, no automatic provider retry | `RUST_NOT_IMPLEMENTED` |
| B7 | Deterministic check-kind vocabulary | S4-D repair commits (`syntax` invalid); contract §5 | Closed enum for check kinds shared by spec validation and fixtures | `RUST_PARTIAL`: typed closed `CheckKind` and negative expectations validated; not wired to specs |
| B8 | Assessment timeout/transcript capture lesson | `e03d097`, `c97c64c`; CAP-014 timeout failures | Read outcome before archiving workspace; bounded timeout; classify unknown; no silent retry | `RUST_NOT_IMPLEMENTED` |
| B9 | Session reconciliation and route verification | `paseo_assessor.go`, live runners | Provider port returns exact identity/route/terminal status; Core verifies before accepting | `RUST_PARTIAL` (Codex/OpenCode probes; no work lifecycle) |
| B10 | Budget ledger hardening and concurrency | `9c6aa4a`, `scripts/s4d_budget_ledger.py` | Durable Core-side reservations, not a side ledger with independent authority | `RUST_NOT_IMPLEMENTED` |

## C. Runtime-specific implementation (do not port)

| Legacy surface | Why not transferable | Rust replacement direction |
|---|---|---|
| `core/internal/lead/paseo_assessor.go`, `core/internal/paseo/`, `core/cmd/factory-worker/paseo_*` | Intermediary Paseo runtime is excluded by ADR-0018. | Direct Codex App Server / OpenCode adapters behind a Rust provider port. |
| Go domain/store/API (`core/internal/**`) | Different type system, schema and lifecycle; mechanical translation would carry defects and policy drift. | Native Rust domain, SQLite schema, Axum API; port behavior and tests only. |
| `core/cmd/s4d-*` Go tools | Runtime-specific CLI. | Rust evaluation/baseline tool with the same frozen corpus semantics. |
| Go/Paseo CI and lint | Obsolete toolchain. | Rust fmt/clippy/test/build/doc/audit + native governance. |
| Office Paseo view adapters/DTOs | Coupled to former runtime and backend shapes. | Typed Rust Core client and Rust event/SSE contracts. |
| Python live runners as authority | Scripts currently encode budget/session authority. | Core owns reservations and budgets; a governed harness only orchestrates and records. |

## D. Historical evidence (provenance only)

| Evidence | Permitted use | Prohibited use |
|---|---|---|
| `docs/evidence/f1-s4d-workgraph-deterministic-2026-10-22.md/.json` | Understand intended behavior, test names, deterministic oracle design. | Claiming Rust deterministic or live verification. |
| `docs/evidence/f1-s4d-live-certification-2026-10-22.md`, graph live JSONs, `f1-s4d-semantic-live-310add3634a0/**` | Defect lessons (invalid check kind, timeout, transcript ordering, session cleanup), corpus inputs for a new versioned run. | Any Rust certification, independent score, cost claim or cleanup guarantee. |
| PR #16 checkpoints/budget docs | Policy questions and envelope shape. | Authorization for new spend or evidence of completion. |

Preserve these records. Do not rewrite old commits or evidence to match Rust.

## E. Unresolved policy decisions (blockers, not implementation details)

1. **ADR-0017 is Proposed, not ratified.** Who may issue standing grants, which
   risk classes may be autonomous, renewal rules, and in-flight revocation
   semantics remain Product Owner decisions. Implement structural support but
   keep autonomous start fail-closed until ratified.
2. **No live budget is authorized.** Any provider-backed verification needs a
   new explicit numeric envelope; the old envelopes are exhausted/incomplete.
3. **CAP-014 is blocked.** No independent score exists. A retry needs a revised
   timeout/evidence protocol and a new corpus digest if fixtures change.
4. **R2/R3 always require specific approval.** No standing grant may make them
   autonomous.
5. **Isolation is unresolved.** TRUSTED_LOCAL is not a sandbox. Rust dispatch
   must stay disabled where required isolation/credential confinement is not
   verified. The same-UID workspace pathname race remains open.
6. **Provider billing is unobservable.** Session ceilings are not cost caps;
   unknown cost stays unknown.
7. **Merge/deployment/controller promotion** remain separate protected
   capabilities, never implied by recovered S4-D autonomy.

## F. Rust recovery sequence

| Order | Increment | Depends on | Done when |
|---|---|---|---|
| 1 | Rust Lead assessment domain, strict schema, deterministic proposal validation, plan hash | ADR-0017 structural fields (not activation) | Unit negatives for schema/scope/graph/budget pass; no provider call |
| 2 | Rust standing delegation persistence with atomic cumulative reservation, expiry, revocation; autonomous policy gate closed by default | 1; policy fields explicit | Concurrency, replay, exhaustion, revocation and fail-closed tests pass |
| 3 | WorkGraph campaign plan and one scheduler; per-unit frozen specs; restart/idempotency | 1, 2; Rust Campaign lifecycle | Sequential/parallel/fan-in/dependency-failure/restart tests pass |
| 4 | Deterministic Git integration and CandidateRevision lineage | 3; Rust workspace | Conflict, multi-parent, allowed-path and lineage tests pass |
| 5 | Crash-consistent start/revocation transactions and process-level recovery | 2, 3 | Failure-injection and restart tests pass |
| 6 | CAP-014 Rust corpus/evaluator (frozen original or new versioned corpus) | Independent evaluator protocol | Deterministic scoring tests pass; no model claim until authorized |
| 7 | Rust Office Lead/Campaign journey with reload/replay | Rust API; provider lifecycle | Browser journey against Rust Core; no mocked API acceptance |
| 8 | Authorized live verification | Ratified policy + explicit envelope | Exact-SHA provider evidence, independent score/review where required |

Rows remain `RUST_NOT_IMPLEMENTED`/`RUST_PARTIAL` and legacy deletion remains
`NO` until the corresponding behavior and candidate-bound evidence exist.
