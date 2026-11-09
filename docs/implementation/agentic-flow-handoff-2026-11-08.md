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

### Phase 4 — Planner, Spec Manager, issues and kanban

- Planner model contract and WorkGraph validation.
- Spec Manager model contract, formal specs with Gherkin/acceptance/negative
  cases and requirement→assertion→check traceability.
- Deterministic freeze/seal.
- Durable spec records and authorized GitHub issue projection; Core remains
  source of truth; local board fallback; drift detection.

### Phase 5 — Explorer, Librarian, Context7 and skills

- Read-only Explorer using deterministic repository evidence plus semantic
  interpretation.
- KnowledgeGate, KnowledgePack, Context7 MCP and ranked sources.
- SkillCandidate/SkillRegistry with RUN/PROJECT/GLOBAL scope, trust gate,
  compatibility, expiry/revocation.
- Promotion after two successful uses, weekly reevaluation, GLOBAL owner
  decision.
- Free navigation only for the read-only Librarian with egress broker and no
  credentials.

### Phase 6 — Dynamic fleets and cheap operations

- AgentTopology and deterministic ParticipantPlan persistence.
- Implementers per independent unit/specialization; conflict detection.
- Multiple independent reviewer receipts and Gate aggregation.
- Cheap output interpreter (tests/logs) without pass/fail authority.
- Role-aware model router and per-participant usage/latency/cost.
- Same model family is allowed; no mandatory provider/dimension diversity in
  this phase.

### Phase 7 — Owner charter and R1–R3

- Durable owner charter, reserved matters, execution/repair/delivery/policy
  autonomy separation, expiry/revocation.
- Director/Operation Manager enforcement; owner decision UI with options,
  evidence, recommendation and deadline.
- R0/R1/R2/R3 tests; reserved effects stay owner-controlled.

### Phase 8 — Real-model browser E2E

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
