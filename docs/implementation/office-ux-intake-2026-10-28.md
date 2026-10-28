# Unified intake lane — 2026-10-28

**Status:** implementation slice on `campaign/office-ux-intake-20261028`,
reconciled onto the Office UX integration baseline. It is not independent
acceptance and does not claim a real browser/Compose run.

## Delivered

- Migration `0029_unified_intake.sql`: durable `intake_requests`,
  append-only `intake_messages`, immutable `lead_execution_context`,
  `execution_policy_grants` and `campaign_execution_profile_bindings`.
- `POST /intake/requests` resolves the Project/global profile default or an
  exact profile id, requires the current executable revision and an existing
  explicit non-autonomous R0 execution-policy grant, then creates the draft
  Action, Lead request, execution context and intake link in one transaction.
  Missing authority returns `409` with
  `authorization.reason = "EXECUTION_POLICY_REQUIRED"` and the exact
  profile/Project scope. No delegation is auto-issued.
- `GET /intake/requests` and `GET /intake/requests/{id}` derive state from
  `lead_requests`, `lead_withdrawals`, `campaigns` and gate decisions. There is
  no second lifecycle.
- `POST /intake/requests/{id}/assess` revalidates the exact current profile
  revision, executability, Project config/base and delegation, invokes the
  existing bounded model-assessment path, then records the assessment and
  proposal and atomically freezes/READY through `freeze_lead_plan_to_ready`.
  Without a model gateway it returns a typed `503 ASSESSMENT_UNAVAILABLE`; it
  does not fake checks.
- `POST /intake/requests/{id}/approve` re-reads the frozen request/plan/spec/
  config/delegation/profile identity, requires the exact plan hash, derives a
  conservative `NewCampaignAuthorization` and calls the shared
  `start_lead_campaign` with `StartAuthority::Operator` and an
  `ExpectedStartBinding` carrying the profile revision/hash. Exact replay
  returns the original Campaign.
- `POST /intake/requests/{id}/message` appends idempotent operator messages
  before freeze. After freeze it returns `409 INTAKE_PLAN_FROZEN`; Core cannot
  distinguish an informational note from a material amendment, so a frozen
  request needs a new request.
- `POST /intake/requests/{id}/cancel` records the authoritative
  `lead_withdrawals` fact. After start it returns `409 LEAD_ALREADY_STARTED`
  with the Campaign identity.
- `POST /projects/{id}/execution-policy` is an explicit operator grant. It
  creates one 30-day, non-autonomous, R0-only standing delegation from the
  exact profile implementer route and Project scope, with bounded cumulative
  capacity. Replaying the same acknowledgement returns the same grant; an
  exhausted/revoked revision needs a new explicit grant. Existing grants are
  never widened or replenished.
- Office Home always submits `REVIEW_PLAN` and states that every request stops
  for explicit operator approval; it does not offer or imply automatic start.
  The execution-policy panel shows the Core-returned objective scope,
  per-request limits and cumulative grant bounds before acknowledgement, then
  retries the same request with the same idempotency key. Request workspace uses
  the intake assess/approve/message/cancel routes and shows the frozen profile
  revision, plan hash and truthful authorization reason.

## Limits

- One active default model gateway: all profile roles must match it to be
  executable. Role-specific routing remains unimplemented.
- Aggregate token/cost/turn accounting across assessment plus implementation is
  not implemented. Approval uses the intersection of the profile campaign
  budget, the frozen implementer role route, plan, delegation and existing Core
  ceilings; it does not claim an aggregate ledger.
- Execution-policy grants use zero repair cycles, one parallel unit, four
  requests, profile attempts (capped at 5) and profile sessions. A profile with
  fewer than two sessions or a deadline below 120 seconds is rejected.
- The intake assessment path is model-only. No fixture fallback fabricates
  executable checks for a draft Action.
- Component tests and Rust DB/API tests are implementation evidence, not a real
  Compose/browser acceptance run.

## Key tests

- `native/src/db/intake.rs` module tests: policy requirement/replay,
  grant/revocation, profile revision change, exact profile/plan binding,
  approval replay and wrong binding, withdrawal fencing, message idempotency,
  frozen-message refusal, stale config and revoked delegation.
- `native/src/bin/core.rs` `tests::intake_api`: HTTP creation with/without
  policy, list/detail shape, assessment 503, message, cancel, approval replay,
  wrong plan hash and post-start fencing.
- `office/src/views/Home.test.tsx` and
  `office/src/views/RequestWorkspace.test.tsx`: `EXECUTION_POLICY_REQUIRED`
  setup, approval guard, frozen-plan messaging, cancellation fence and state
  projection.
