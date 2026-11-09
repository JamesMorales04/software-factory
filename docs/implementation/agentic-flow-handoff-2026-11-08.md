# Agentic flow handoff — 2026-11-08

**Branch:** `factory/agentic-flow-20261108`
**PR:** #96 (draft)
**Base:** `main` at `8a677216638ef4b0f106d3a381e1d46ff19b5dde` (PR #95
merged)
**Code candidate:** `0e74ee7398eadc752cd205308591f09d27c5f7b4`
**Docs head:** `cca51e74134b43376c9249ac77c691aab6f8dc4f` plus this handoff
commit

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

### Phase 8 — Real-model browser E2E (next)

- Full campaign through Office only, no API/command shortcut.
- Real OpenCode Go `deepseek-v4.1-flash` for roles without a defined model.
- Cover informational, audit, direct, formal spec, repair, concurrent operations
  and owner decision.
- Fix every E2E deviation and record candidate-bound evidence.

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
