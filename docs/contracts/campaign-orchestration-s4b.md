# Contract: Native campaign orchestration (F1-S4-B)

**Status:** Implemented for F1-S4-B; final candidate acceptance is recorded in
the PR. **Version:** `1`. Experimental pre-release contract; breaking changes
update all in-repo callers, fixtures, docs and tests atomically (ADR-0006).
TRUSTED_LOCAL only; this is not isolation certification.

S4-B does not replace the S1 Action, S2 Run/Attempt/Worker, S3 Paseo or S4-A
candidate/evidence lifecycles. It binds one frozen ActionSpec to a durable,
Core-owned task graph.

## 1. Identities

| Entity | Identity | Notes |
| --- | --- | --- |
| Campaign | 16 random bytes hex (32 chars) | One per `(actionId, specId)`; at most one active campaign per Action |
| Plan | SHA-256 of the immutable `CampaignPlan` JSON | `planHash`; no update route |
| Campaign task | 16 random bytes hex | Unique `(campaignId, taskKey)` |
| Participant identity | `verifier-<campaign8>-<cycle>` / `reviewer-<campaign8>-<cycle>` | Passed to the separate process and stored as evidence actor |
| Implementer execution | existing Run/Attempt/generation | No second attempt lifecycle |
| Candidate | S4-A `CandidateRevision` | Verification/reviewer tasks bind exactly one |
| Decision | S4-A one `GateDecision` per candidate | Only source of acceptance/repair |

Campaign creation derives project, Action, spec hash and policy from the frozen
spec; caller-supplied copies are ignored. The repetitive operator request is
idempotent by `(actionId, specId)` and optional `Idempotency-Key`. Completed or
cancelled Actions cannot start new campaigns, and a pending/running campaign
blocks a second active campaign for the same Action (concurrent cross-spec
campaigns are outside F1-S4-B).

## 2. Campaign schema

```json
{
  "id": "…", "projectId": "default", "actionId": "…", "specId": "…",
  "specHash": "sha256", "policyRevision": "quality-baseline-s4a-v1",
  "planHash": "sha256", "plan": {"revision":1,"maxRepairCycles":1,"tasks":[],"parallelism":2,"wallClockSeconds":1800},
  "status": "pending | running | accepted | repair_required | blocked | human_required | failed | cancelled",
  "terminalReason": "…", "maxRepairCycles": 1, "repairCycles": 0,
  "maxAttempts": 2, "maxWallClockSecs": 1800, "maxParallel": 2,
  "createdAt": "…", "startedAt": "…", "completedAt": "…", "updatedAt": "…"
}
```

Bounds: repair cycles `0..3`, wall clock `120..7200s`, parallelism `1..2`,
task executions `1..3`, task graph `≤16` rows. Campaign `maxAttempts` is the
total Implementer-attempt budget across the initial cycle and authorized
repairs (`spec.maxAttempts × (maxRepairCycles+1)`), enforced before dispatching a
new implementer Run. Unknown provider quota/cost remains `unknown`; no zero is
fabricated.

## 3. Task graph and states

Each cycle contains `implementer-<n>`, `verifier-<n>`, `reviewer-<n>` and
`gate-<n>`. `verifier`/`reviewer` depend on the implementer; the gate depends on
both independent roles. Cycle `n>0` begins dormant and is authorized only by a
`REPAIR_REQUIRED` decision for cycle `n-1`, while the repair ceiling remains.

```text
pending → ready → dispatched → succeeded
                    ↓  ↗ (bounded retry only)
                  failed
pending/ready/dispatched → cancelled
pending/ready → blocked
```

Tasks are durably claimed by conditional update (single writer per claim),
carry execution counts, dispatch owner/expiry, idempotency key, participant
identity, candidate/Run links, outcome and failure reason. A task never
expands its dependencies, candidate scope or credentials.

## 4. Scheduling semantics

- One operator `start` moves `pending → running` and promotes cycle-zero tasks.
- Implementer dispatch creates the spec-bound Run and task binding in one
  transaction; the Worker still leases/executes with S2 fencing.
- An implementer task completes only when its Run is `succeeded` and a frozen
  candidate exists for that exact Run/spec.
- Verifier/reviewer tasks become ready only after that candidate exists and
  consume exactly its identity. Core spawns the separate binary with its
  scoped token, `--candidate`, participant identity and deterministic
  idempotency key.
- Every candidate whose spec is owned by a Campaign is campaign-bound from
  the moment it is frozen, not only after its implementer task completes.
  Evidence for such a candidate is accepted only when its idempotency key and
  actor match an assigned verifier/reviewer task; pre-claim, alternate-key and
  mismatched-actor evidence is rejected.
- A participant task completes only from durable evidence with that exact key.
  Process exit without evidence is a failure, never success.
- The gate task evaluates the S4-A bundle through the same Core decision path
  as the operator route. Exactly one decision is persisted.
- `ACCEPTED` finishes the campaign; `REPAIR_REQUIRED` authorizes the next cycle
  or terminates as `repair_required` when exhausted; `BLOCKED` and
  `HUMAN_REQUIRED` finish without repair; `NOT_TESTED` cannot advance.
- Unresolved S3-D recovery blocks the campaign before any replacement.
- Wall-clock exhaustion, exhausted participant retries, cancellation and
  missing mandatory evidence terminate or block with an explicit reason.

## 5. Resource and recovery rules

Campaigns enforce maximum implementer attempts (from the frozen spec), repair
cycles, wall clock, per-campaign active verifier/reviewer count and a bounded
Core participant slot pool. Child process output is bounded and never becomes
authoritative evidence.

Core restart preserves the campaign graph. Existing S2 boot reconciliation
expires live Attempts; the existing S3-D retry gate prevents replacement until
recovery is confirmed/absent. Expired participant dispatches retry within the
task budget or block. Concurrent Core instances contend through transactional
claims and unique decisions; this is not a distributed consensus design.

## 6. HTTP routes

| Method | Route | Auth | Meaning |
| --- | --- | --- | --- |
| POST | `/api/v1/campaigns` | operator | Create a campaign from a frozen spec |
| GET | `/api/v1/campaigns` | operator | List campaigns (`status`, `limit`) |
| GET | `/api/v1/campaigns/{id}` | operator | Authoritative projection |
| GET | `/api/v1/campaigns/{id}/events` | operator | Campaign aggregate replay |
| POST | `/api/v1/campaigns/{id}/start` | operator | Start once; idempotent while running |
| POST | `/api/v1/campaigns/{id}/cancel` | operator | Cancel and propagate to open tasks/Runs |

There is no route to forge task state, change a plan, bypass a dependency,
authorize repair, or expand a budget.

## 7. Events and Office

`CampaignCreated`, `CampaignStarted`, `CampaignTaskReady`,
`CampaignTaskDispatched`, `CampaignTaskCompleted`, `CampaignTaskFailed`,
`CampaignTaskCancelled`, `CampaignRepairAuthorized`, `CampaignFinished` and
`CampaignCancelled` are recorded on `campaign/<id>` in the existing S1 ledger.
Office renders Core projections (campaign, tasks, participants, candidate,
evidence, repair budget, gate decision and restart/disconnected state) and
refreshes via SSE replay; it has no scheduler.

## 8. Non-goals

No GitHub delivery/merge, S5/S6, general workflow DSL, dynamic skill registry,
adaptive model promotion, remote Workers, general MCP orchestration, animated
Office or isolated/SEC-01 certification.
