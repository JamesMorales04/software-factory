# Agentic flow handoff — 2026-11-08

**Branch:** `factory/agentic-flow-20261108`
**PR:** #96 (draft)
**Base:** `main` at `8a677216638ef4b0f106d3a381e1d46ff19b5dde` (PR #95
merged)
**Code candidate:** `29221072834c124d8dd1186eedbaf7c2a65f185a` (previous verified code `11933a6` plus the verification fixes)
**Docs/evidence head:** `9961113dde0f724b5bb28eb32a3fb2022a56e7ca` plus the verification updates

## What is complete

- ADR-0037 agent topology/context/composition.
- ADR-0038 owner charter/delegated autonomy.
- `docs/contracts/agentic-flow.md` and
  `docs/implementation/agentic-flow-plan.md`.
- CAP-057…CAP-066 and milestone F1-S10 proposed; capability decisions and
  roadmap regenerated.
- `native/src/agentic.rs`: typed RequestKind, ChangeRoute, Assurance,
  TriageSignals, TriageDecision, AgenticTriageModel, StackProfile,
  AgentAssignment, ContextPolicy, ParticipantPlan, HandoffSummary,
  OwnerCharter and deterministic tests.
- Migration `0052_agentic_assessments.sql` and
  `native/src/db/agentic_assessments.rs`.
- Core endpoints:
  - `GET /agentic/portfolio`
  - `GET /agentic/requests/{id}`
  - `POST /agentic/requests/{id}/assess`
- Persisted, real-model semantic triage using the frozen Lead route and the
  existing model-call ledger. Core merges the model recommendation with
  non-weakening deterministic floors.
- Office `/director` route: owner portfolio, route/assurance, StackProfile,
  deterministic ParticipantPlan, context policy, semantic recommendation and
  measured usage.
- Component tests and browser evidence at
  `docs/evidence/director-projection-2026-11-08/`.
- **Phase 3 repair (operator-confirmed R0/R1):** `POST /campaigns/{id}/repair`
  advances the Project base to the exact rejected candidate, creates a new
  authorized autonomous request, links the immutable Gate decision, and reruns
  the full Gate. Real browser repair of candidate `dbbfa1d6` fixed the date
  validator and reached `ACCEPTED` at candidate `88ca73d7`; evidence is in
  `docs/evidence/repair-from-candidate-2026-11-08/`. R2/R3 are refused pending
  the owner charter. A Core watcher promotes the accepted repair candidate to
  the Project base without a second operator action.
- **Phase 2 operation manager and scheduler:** migration
  `0055_agentic_operations.sql`, durable operation records linked to each
  intake request, Director/Operation Manager identity, per-operation priority
  (0-1000), resource-derived start capacity from CPU and available memory,
  in-process priority queue, `GET /agentic/scheduler`,
  `POST /agentic/requests/{id}/priority`, Office operation panel and priority
  control. Terminal Campaign/Gate states reconcile the operation projection
  every five seconds; a cancelled Campaign can never leave an operation
  claiming to execute. Evidence is in
  `docs/evidence/operation-scheduler-2026-11-08/`.

## Live evidence

- Real OpenCode Go `deepseek-v4.1-flash` semantic triage from the browser on
  request `d13fed9d-2101-45e2-a961-86b6bad3b7f3`.
- Usage observed: 1451 input / 1139 output / 2590 total / 902 micros.
- 1440 and 390 px: zero console errors, zero unexpected 4xx/5xx, no horizontal
  overflow.
- Phase 2 browser smoke: request `b19cdff1-7e0f-4be6-9ceb-7d2f138332a8`
  reached `Executing` within 22 seconds through the real Office intake and
  resource governor; priority was updated 100 -> 10 from the browser; the
  Campaign was cancelled and the operation projection reconciled to
  `CANCELLED`. Scheduler capacity observed at 10 from CPU and available memory.

## What remains

### Phase 2 — Operation Manager and concurrency (partial)

- Durable `agentic_operations`, Director/Operation Manager identity, priority
  queue and resource-derived capacity are implemented.
- Remaining: two simultaneous operations, duplicate-runner proof, per-operation
  worktree isolation evidence, disk/provider backpressure and fairness tests.

### Phase 3 — Repair (operator-confirmed R0/R1 complete)

- RepairPacket from the exact rejected candidate, findings and failed checks.
- Repairer execution from the candidate, C+1 candidate assembly, evidence
  invalidation and full revalidation.
- R0 autonomous under policy; R1 bounded; R2/R3 owner charter/reserved.
- Current project-base and `projects.repository UNIQUE` constraints require a
  candidate-based repair workspace/project design; do not simply create a new
  request from the base commit.

### Phase 4 — Planner, Spec Manager, issues and kanban (implemented)

- Typed `WorkGraphModel`/`PlannedWorkUnit` with deterministic validation for
  cycles, unknown predecessors, unsafe or out-of-scope paths, unit bounds and
  shared write paths between independent units.
- Typed `SpecDraftModel` and deterministic `compile_spec`: requirement,
  assertion and check identities are derived from position and revision, never
  from model text; every requirement needs acceptance and negative cases.
- Real Planner and Spec Manager model calls through the frozen Lead route with
  distinct purpose, typed schemas and bounded output. Core validates and stores
  only the compiled record.
- Migration `0056_agentic_plans_specs.sql`, durable work graphs, draft/frozen
  spec revisions, supersession, publication and issue projection records.
- Operator-confirmed GitHub issue projection with deterministic body markers,
  bounded marker recovery, create/update and drift refresh; local board
  fallback when GitHub is unavailable.
- Office `/specs` kanban with Core-derived columns, requirement-to-check
  traceability, publication/drift state and local fallback; Director Planner
  and Spec Manager panels with freeze and revision controls.
- Real browser evidence in `docs/evidence/spec-board-2026-11-09/`: request
  `659636fa`, FORMAL_SPEC route, real `deepseek-v4.1-flash` Planner and Spec
  Manager calls, revision 2 frozen, 6 requirements, 20 assertions, local board,
  1440/390 with zero console errors and no overflow. GitHub live publication
  remains `NOT_TESTED`.

### Phase 5 — Explorer, Librarian, Context7 and skills (implemented, live use pending)

- Read-only Explorer inventories the exact Project `Cargo.toml` and
  `package.json` at the frozen base commit and records manifest evidence.
- Deterministic knowledge-gap gate: a named library must be evidenced by the
  objective or semantic skill needs and must not be already pinned in a
  manifest.
- Bounded Context7 MCP client with initialize/initialized/tool-call, JSON or
  SSE responses, optional session header, fixed HTTPS endpoint, no credentials,
  size/time limits and deterministic library ranking.
- Migration `0057_agentic_knowledge_skills.sql`, durable KnowledgePacks,
  skills, uses and adoptions.
- Read-only skill trust gate, RUN/PROJECT/GLOBAL scope, two successful uses,
  reviewed upstream license, weekly reevaluation, explicit owner decision for
  GLOBAL and revocation.
- Adopted skills are delivered in the Worker assignment and included in both
  OpenCode and Codex prompts as read-only guidance; they cannot widen tools or
  authority.
- Office Director `Knowledge and skills` panel and `/skills` registry.
- Real browser evidence in `docs/evidence/knowledge-skills-2026-11-09/`:
  request `d0004d44`, real Context7 MCP research for `chrono`, KnowledgePack
  SHA `d3818a21`, temporary skill `context7-chrono`, adoption for the
  implementer, 1440/390 with zero console errors and no overflow. Real
  participant use and promotion remain `NOT_TESTED`.

### Phase 6 — Dynamic fleets and cheap operations (implemented)

- `ParticipantPlan` now carries per-unit implementer routing with
  specialization, provider/model and paths, plus an explicit reviewer count.
  The plan is persisted in `agentic_participant_plans` and recorded at campaign
  start in `campaign_review_requirements`.
- Migration `0058_multi_reviewer_and_participant_plans.sql` rebuilds
  `quality_receipts` to allow multiple reviewer principals per candidate.
- The Quality Gate aggregates all reviewer receipts: all must bind the exact
  candidate and pass; one failure requires repair; duplicate principals or
  receipts are blocked; a passing set waits for the plan's reviewer count.
- Cheap Ops interpreter (`cheap_ops.rs`) restates verifier/reviewer receipts and
  the authoritative Gate as `OBSERVATION_ONLY`; it never passes, fails, repairs
  or accepts.
- Campaign detail exposes the Cheap Ops interpretation. Per-participant usage,
  cost and route already come from the Office participant projection; latency
  remains pending.
- Evidence: `docs/evidence/dynamic-fleets-2026-11-09/`. A live two-reviewer
  Campaign remains `NOT_TESTED`.

### Phase 7 — Owner charter and R1–R3 (implemented, delivery effects pending)

- Migration `0059_owner_charter_decisions.sql`, durable charters and owner
  decisions.
- Separate execution, repair and delivery autonomy; policy autonomy is never
  delegable. Reserved matters, risk appetite, domains, expiry and revocation
  are enforced.
- `/charter` Office page: create/revoke charter, open reserved decision, inspect
  evidence/recommendation/deadline and resolve with an owner option and
  rationale.
- Repair enforces the charter for R1+ and creates an owner decision for
  reserved matters; autonomous starts are refused when the charter does not
  delegate execution, the risk exceeds appetite or a reserved matter triggers.
- Evidence: `docs/evidence/owner-charter-2026-11-09/`. Live routine R1/R2
  execution under the charter and revocation remain pending.

### Phase 8 — Real-model browser E2E (complete as implementer self-test)

- A real autonomous request `5a2be8ae` ran from Office through Lead assessment,
  plan freeze, dispatch, Implementer, deterministic Verifier, model Reviewer and
  Gate `ACCEPTED` at candidate `60c2577a` / commit `d665b08d`. The diff changed
  `src/lib.rs` and `tests/next_leap_year.rs` (46 insertions).
- Reviewer usage: 6741 input / 1003 output tokens; provider/model
  `opencode-go/deepseek-v4.1-flash`.
- Informational request `c0f6e8b0` and audit request `6df5bed8` both completed
  assessment as `ASSESSED` with no plan or Campaign mutation.
- Concurrent operations `574fbd15` and `b7bafdd9` were created back-to-back and
  coordinated by the Director; one completed and one blocked.
- Formal spec (`659636fa`), repair (`0a81b768`), owner decision (`bee1d2f1`)
  and scheduler evidence are linked from the same manifest.
- Four real deviations were found and fixed:
  1. The reviewer lacked unchanged source context and falsely blocked a
     test-only diff; the review bundle now includes bounded unchanged source
     context and the prompt requires checking it.
  2. Informational/audit assessments could carry an unnecessary effort estimate;
     advisory effort is normalized for non-change intents.
  3. A non-change assessment without a proposal blocked the request; it now
     records the assessment and projects a derived `ASSESSED` disposition.
  4. Blocked/review-plan operations remained `QUEUED` and could resume after
     restart; operation creation and reconciliation now reflect the Lead/intake
     state, and resume filters terminal/withdrawn requests.
- Evidence: `docs/evidence/full-agentic-e2e-2026-11-09/`.
- Remaining gaps: independent verifier/Reviewer acceptance, a live two-reviewer
  Campaign, live GitHub issue publication, skill use/promotion inside a
  Campaign, delivery effects, Windows and deployment. `TRUSTED_LOCAL` is not
  isolation or certification.

### Independent review corrections (2026-11-09)

An independent read-only review of the PR found High/Medium issues. The
following were fixed on candidate `11933a6`:

- Deterministic triage floors are now computed first; a semantic assessment is
  merged into that decision and can only raise a floor.
- A Change assessment without a proposal is rejected; only Informational/Audit
  may project `ASSESSED` with no plan.
- Reviewer receipt principals are bound to the authenticated reviewer role in
  production; the queue retains a candidate until the plan's required reviewer
  count is reached; the single-reviewer Compose topology caps a plan at one
  reviewer to avoid an undispatchable second receipt.
- Reserved matters are global for autonomous starts, charter domains are
  enforced for autonomous starts and repair, and expired owner decisions cannot
  be resolved.
- Skill use requires an existing adoption and a reviewed non-`NOASSERTION`
  license.
- Repair base advancement validates candidate ancestry.
- Spec freezing rejects a WorkGraph changed after draft authoring; WorkGraph
  independent path overlap uses prefix semantics.
- Review source context excludes secret-bearing content; Cheap Ops uses the
  camelCase contract; the owner decision selector defaults to no option.
- `ASSESSED` is accepted by the intake filter; documentation and evidence
  references were corrected.

Final real campaign on candidate `11933a6`: request `38a4c431`, campaign
`4c01ce12`, candidate `386b7a56`, commit `37f9bc17`, Gate `ACCEPTED`,
`src/lib.rs` (+32), reviewer usage 6463/2089 tokens. The earlier campaign
`5a2be8ae` remains as the first accepted run before these corrections.

## Key invariants

- Core owns authority, identity, budgets, leases, candidate truth, Gate and
  effects.
- Model output never selects tools, widens scope, changes frozen specs or
  accepts work.
- No second orchestrator outside Core.
- Context handler: 70% effective context, tool clearing/memory first, at most
  two consecutive compactions, audited handoff at a stable milestone with probes.
- Owner charter separate from model prompts; reserved matters are hard.
- `TRUSTED_LOCAL` is not isolation or certification.

## Commands

```bash
cargo fmt --manifest-path native/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path native/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --locked --manifest-path native/Cargo.toml --all-features --lib
cargo test --locked --manifest-path native/Cargo.toml --all-features --bin factory-core
cargo run --quiet --manifest-path native/Cargo.toml --bin factory-architecture-check -- --root .
python3 .github/scripts/check-office-parity.py --root .
cd office && npm run typecheck && npm run lint && npm test -- --run && npm run build
```

Live stack: `http://127.0.0.1:8893`; Core image rebuilt with
`podman compose -f compose.yaml up -d --build core`.

## External CI blocker (2026-11-09)

The Rust and container-dev CI jobs fail `cargo audit` on
`RUSTSEC-2026-0459` (`libsqlite3-sys` 0.30.1 via sqlx 0.8.6/0.9.0,
CVE-2026-119375, medium). sqlx 0.9.0 still pins 0.30.1, so upgrading the
toolchain alone does not fix it; a patched sqlx/libsqlite3-sys or an explicit
owner risk acceptance is required. This is an upstream dependency blocker, not
a regression from the agentic-flow code. Do not merge by ignoring the advisory
without an owner decision.

## Independent verification (AI verifier, 2026-11-09)

A fresh independent verifier session checked commit `9961113` from a clean
worktree and the live stack without mutations. Build, lint, 606 lib tests, 38
Core bin tests, Office 296 tests, architecture/parity and the final live E2E
identities passed. The verifier found three HIGH projection/trust defects and
several evidence inconsistencies:

- Skill use did not require a reviewed non-`NOASSERTION` license. Fixed: a use
  now fails before license review; the DB test records the review first.
- Informational/audit Lead assessments still projected as CHANGE/FORMAL_SPEC in
  the Director because the triage context ignored the assessment intent. Fixed:
  the Lead intent (and semantic request kind) now sets the explicit
  informational/audit signals before deterministic triage; ambiguity is not
  fabricated for those intents.
- Charter domains were not checked by autonomous starts. Fixed: autonomous
  starts require the `repository` domain.
- Lead participants on an accepted Campaign still showed `RUNNING`. Fixed: the
  participant projection now uses the derived Lead request projection and marks
  terminal states FINAL.
- The owner-decision selector still displayed the first option. Fixed: the value
  no longer falls back to `options[0]`.
- Evidence route/assertion/assurance/charter/timestamp inconsistencies were
  corrected: final route DIRECT, 21 assertions, persisted-plan assurance noted,
  active/revoked charter identities separated, captured_at corrected.
- The multi-reviewer plan cap is intentional for the current single-reviewer
  Compose topology; multi-reviewer aggregation remains deterministically tested
  but a live two-reviewer Campaign is still pending.

The independent verifier did not re-run Playwright; screenshots were inspected
and the live API identities were queried. This remains an AI verification, not
independent human certification. The external RUSTSEC-2026-0459 advisory still
blocks merge unless the owner accepts the risk or a patched sqlx/libsqlite3-sys
is adopted.

### Post-verification closure (2026-11-09)

The independent verifier's HIGH findings were fixed on code candidate
`2922107` and verified live:

- The NOASSERTION skill now rejects use before license review (live 409
  `skill license must be reviewed before the skill is used`).
- Informational request `c0f6e8b0` now projects `INFORMATIONAL` / `NO_AGENT`;
  audit request `6df5bed8` projects `AUDIT` / `DIRECT`; both remain `ASSESSED`
  with no Campaign.
- The accepted campaign's Lead participant now reports `COMPLETED` / `FINAL`
  instead of `RUNNING`.
- Autonomous starts now require the `repository` charter domain.
- The owner decision selector no longer displays an implicit APPROVE option.
- Evidence route/assertion/assurance/charter/timestamp claims were corrected;
  the code candidate is now recorded in every manifest.

The remaining merge blocker is still the external RUSTSEC-2026-0459 advisory.
