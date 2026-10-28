# Office UX campaign — shared contracts (2026-10-28)

**Status:** provisional integration contract for the active Office UX campaign
(umbrella issue #39). It is normative for lanes working in parallel; Core and
the integration owner may amend it with a recorded decision. It is not evidence
that an endpoint exists.

**Baseline:** `campaign/office-ux-baseline-20261028`.

## 1. Trust boundaries

- Core owns profiles, revisions, intake records, authorization, budgets,
  idempotency, frozen plan identity, Campaign lifecycle and Quality Gate.
- Office is a client. It may select a profile and request an effect; it never
  writes SQLite or grants authority.
- A profile is configuration, not a capability grant. Submitting a request is an
  operator intent; starting a Campaign still requires a Core-owned authorization
  derived from the frozen plan and the exact profile revision.
- Model output can propose a plan; it cannot create a profile, expand a budget,
  enable autonomous start, or approve its own plan.
- The deterministic Verifier is not a generative role and has no profile row.

## 2. Execution profiles

### 2.1 Roles

| Role | Applied by |
|---|---|
| `lead` | Semantic assessment and plan proposal (the current architecture's assessor and Lead are one bounded model call) |
| `implementer` | Campaign authorization route for the implementation session(s) |
| `semanticReviewer` | Semantic review model call, when semantic review is enabled |

A future split between a semantic assessor and a plan synthesizer may add a
role only when a real separate model call exists. Do not add decorative roles.

### 2.2 Profile projection

```json
{
  "id": "uuid",
  "name": "Balanced",
  "description": "Default supported routes",
  "category": "BALANCED",
  "revision": 3,
  "revisionId": "uuid",
  "contentHash": "sha256-hex",
  "globalDefault": true,
  "projectDefaults": ["project-uuid"],
  "roles": {
    "lead": {
      "provider": "opencode-go",
      "model": "deepseek-v4.1-flash",
      "reasoning": "medium",
      "maxInputTokens": 128000,
      "maxOutputTokens": 8192,
      "maxCostMicros": 300000
    },
    "implementer": {
      "provider": "opencode-go",
      "model": "deepseek-v4.1-flash",
      "reasoning": "medium",
      "maxInputTokens": 40000,
      "maxOutputTokens": 12000,
      "maxCostMicros": 500000
    },
    "semanticReviewer": {
      "provider": "codex",
      "model": "gpt-6-luna",
      "reasoning": "medium",
      "maxInputTokens": 400000,
      "maxOutputTokens": 8192,
      "maxCostMicros": 1000000
    }
  },
  "budgets": {
    "maxAttempts": 3,
    "maxSessions": 2,
    "maxTurns": 4,
    "maxInputTokens": 40000,
    "maxOutputTokens": 12000,
    "maxCostMicros": 500000,
    "deadlineSecs": 600
  },
  "autoStart": { "enabled": false, "riskClasses": [] },
  "executable": false,
  "warnings": [
    "semanticReviewer route codex/gpt-6-luna is declared but not currently executable"
  ],
  "createdAt": "RFC3339",
  "updatedAt": "RFC3339"
}
```

`executable` is true only when every required role route can actually be **applied**
by Core, not merely when a provider connection is configured.

**Amended 2026-10-28 by lane R.** Exact per-role routing is now implemented:
each role resolves its own provider connection, credential, adapter and model.
A profile may assign distinct providers/models to Lead, Implementer and
Semantic Reviewer. A role is routable only when its provider is configured,
enabled, credentialed, its adapter implements that role (OpenAI-compatible chat
for Lead and semantic review; provider session for implementation) and Core can
substantiate the model (configured model, baseline tested route or real
discovery). A Codex App Server connection is executable for implementation but
not for one-shot assessment or review. Unsubstantiated roles remain
non-executable drafts and cannot become a default or reach a request.
`warnings` must explain each false state.

`GET /execution-profiles` returns:

```json
{
  "profiles": [ExecutionProfileProjection],
  "supportedRoutes": [{ "provider": "opencode-go", "model": "deepseek-v4.1-flash" }]
}
```

`GET /execution-profiles/{id}` returns the projection directly. Clients must not
assume a bare array.

Reasoning is an optional bounded string and must not be invented for providers
that do not support it.

### 2.3 Create/update

`POST /execution-profiles`:

```json
{
  "name": "Balanced",
  "description": "...",
  "category": "BALANCED",
  "roles": { "lead": {}, "implementer": {}, "semanticReviewer": {} },
  "budgets": {},
  "auto_start": { "enabled": false, "risk_classes": [] }
}
```

`PUT /execution-profiles/{id}` adds `"expected_revision": 3`. A mismatch returns
`409` with `{"error": "execution profile revision is stale"}`. Every edit creates
an immutable revision row; the profile row points at the current revision.

`POST /execution-profiles/{id}/default` body:

```json
{ "scope": "global" }
{ "scope": "project", "project_id": "uuid" }
```

`DELETE /execution-profiles/{id}/default` with the same body clears the binding.
At most one global default exists. At most one project default exists per project.
A project with no explicit default resolves the global default.

`GET /projects/{id}/execution-profile`:

```json
{
  "projectId": "uuid",
  "profileId": "uuid",
  "revisionId": "uuid",
  "revision": 3,
  "source": "PROJECT_DEFAULT"
}
```

`source` is `PROJECT_DEFAULT | GLOBAL_DEFAULT`; `404` means no default exists.

### 2.4 Supported-route validation

The supported route registry is explicit. At the contract baseline it accepts:

- `opencode` or `opencode-go` with `deepseek-v4.1-flash`, mode `build`;
- `codex` with `gpt-6-luna`, mode `build`.

The `opencode` and `opencode-go` providers are distinct OpenAI-compatible
connections. A route is executable only if that provider is configured,
enabled, has a decryptable credential and an implemented adapter for the role,
and the model is substantiated for that connection. The baseline registry is
`deepseek-v4.1-flash` for `opencode`/`opencode-go` and `gpt-6-luna` for `codex`;
additional model IDs become substantiated through authenticated provider model
discovery. A manual model ID is saved only as a clearly labelled unverified,
non-executable draft. Unknown providers are rejected. The registry may be
extended only with tested adapter support.

## 3. Unified intake

### 3.1 Projection

```json
{
  "id": "uuid",
  "projectId": "uuid",
  "objective": "Add a CSV export endpoint",
  "mode": "REVIEW_PLAN",
  "state": "AWAITING_APPROVAL",
  "stateDetail": "Frozen plan ready for explicit start",
  "profile": {
    "id": "uuid",
    "revisionId": "uuid",
    "revision": 3,
    "name": "Balanced",
    "contentHash": "sha256-hex"
  },
  "actionId": "uuid",
  "delegationId": "uuid",
  "leadRequestId": "uuid",
  "campaignId": null,
  "authorization": {
    "required": true,
    "available": true,
    "reason": null
  },
  "assessment": { "...": "LeadAssessmentView or null" },
  "proposal": { "...": "LeadPlanProposalView or null" },
  "plan": { "...": "FrozenLeadPlanRecord or null" },
  "campaign": { "...": "CampaignDetail or null" },
  "messages": [
    {
      "id": "uuid",
      "role": "operator",
      "body": "Please keep the API backward compatible.",
      "createdAt": "RFC3339"
    }
  ],
  "createdAt": "RFC3339",
  "updatedAt": "RFC3339"
}
```

List responses omit `assessment`, `proposal`, `plan`, `campaign` and `messages`
and include `campaignStatus`, `gateOutcome` and `updatedAt` summaries only when
Core can derive them truthfully.

### 3.2 State derivation

Intake does **not** own a second lifecycle. It derives:

| Source | Intake state |
|---|---|
| Lead request `SUBMITTED` | `SUBMITTED` |
| Lead request `ASSESSING` | `ASSESSING` |
| Lead request `PLANNED` | `PLAN_READY` |
| Lead request `FROZEN` / `READY` | `AWAITING_APPROVAL` |
| Lead request `RUNNING` or Campaign active | `EXECUTING` |
| Lead request `BLOCKED` | `BLOCKED` |
| Lead request `FAILED` | `FAILED` |
| Campaign/gate terminal accepted | `COMPLETED` |
| operator cancellation | `CANCELLED` (stored intake-only terminal flag) |

### 3.3 Endpoints

`POST /intake/requests` body:

```json
{
  "project_id": "uuid",
  "objective": "...",
  "execution_profile_id": "uuid-or-omitted",
  "mode": "REVIEW_PLAN",
  "idempotency_key": "uuid"
}
```

- Resolve the project default, then global default, when the profile is omitted.
- Validate the active project config and the exact profile revision.
- Require an existing active, non-revoked grant for the project that matches the
  exact profile revision's implementer route, capabilities, paths, risk scope,
  expiry and remaining cumulative capacity. **Intake never auto-issues a
  delegation.** If none matches, return `409` with
  `authorization.reason = "EXECUTION_POLICY_REQUIRED"`.
- Create the derived Action draft and Lead request in one transaction with
  idempotent replay. The executable ActionSpec is inserted at freeze (ADR-0029);
  the draft carries the assessment input and the Lead proposal supplies the
  concrete plan.
- Return `201` with the projection, `200` on idempotent replay.

**Execution-policy setup.** A separate explicit operator operation
`POST /projects/{id}/execution-policy` (body: profile id, exact revision/hash,
expected project config hash, acknowledgement, idempotency key) may create the
bounded **non-autonomous** delegation from the profile. It is a deliberate
authorization grant, not intake side-effect. It records the exact profile
revision, scope/capabilities/paths, route, budgets and expiry; it never enables
autonomous start. Re-enabling after a profile revision change creates a new
grant only after the operator's explicit acknowledgement. Existing grants are
never widened or replenished by a profile edit.

`POST /intake/requests/{id}/assess` runs the existing bounded assessment path
using the frozen profile's `lead` route. It returns the refreshed projection.
No model route → `503` with a typed reason; do not fall back silently.

`POST /intake/requests/{id}/approve` body:

```json
{ "expected_plan_hash": "sha256-hex", "actor": "office-operator", "rationale": "..." }
```

- Requires a frozen plan, the exact profile revision and an exact plan hash
  match. The authenticated principal is the approver; a client-supplied display
  label is not identity.
- Calls the shared Lead start boundary with `StartAuthority::Operator`
  (ADR-0029). It permits the bounded non-autonomous execution-policy grant but
  retains every scope/risk/expiry/route/budget/execution-mode check and stores
  immutable start evidence.
- Derives the Campaign authorization from the profile's `implementer` route,
  the frozen plan's budgets/risk and the project config's allowed paths.
- If `mode` is `AUTONOMOUS` and the profile `autoStart` policy is enabled and
  Core policy permits it, this may be idempotently invoked by the submission
  flow; otherwise the UI must present an explicit approval. Autonomous authority
  is never synthesized from a manual approval.
- Never broadens the frozen plan, budget or path scope; returns `409` if the
  plan is stale, withdrawn or the authorization cannot be derived.

`POST /intake/requests/{id}/message` body
`{ "body": "...", "idempotency_key": "uuid" }` appends a durable operator
message. Messages never mutate a frozen plan. A material amendment after freeze
must create a new request/revision; return `409` with a typed reason.

`POST /intake/requests/{id}/cancel` body `{ "reason": "..." }` records an
immutable `lead_withdrawals` fact through the shared boundary (ADR-0029).
Withdrawal and start serialize: after withdrawal, assessment dispatch, freeze,
start and alternate Campaign creation reject the request. If a Campaign already
started, return `409` with its identity; the existing Campaign cancellation
operation remains authoritative. An intake-only terminal flag is forbidden.

`GET /intake/requests?state=&project_id=&limit=&offset=` and
`GET /intake/requests/{id}` are bounded and authenticated.

### 3.4 Idempotency and concurrency

- The request creation key hashes project, objective, profile revision, mode and
  operator identity.
- `approve` uses the plan hash as its concurrency guard.
- No client-supplied authorization budget is accepted.
- Frozen profile revision and plan identity are stored in the intake row and in
  the Campaign authorization.

## 3.5 Known blockers (independent review, 2026-10-28)

A read-only `gpt-6.1-sol` review of this contract against the actual code found
the following. **Intake must not be implemented as originally written until
these are resolved and re-reviewed.**

1. **Explicit approval cannot compose the existing Lead start unchanged.** The
   current start requires an autonomous delegation and the autonomous-start
   policy. A non-autonomous delegation cannot start after explicit operator
   approval. A shared start operation needs an explicit-approval entry condition
   that is at least as strong as, and separate from, standing autonomy.
2. **An empty Action spec has no supported transition to a frozen executable
   spec.** `action_specs` is immutable and start requires exact plan/spec
   equality. Intake cannot create an empty Action and later mutate it. A
   deliberate draft-to-frozen spec lineage is required.
3. **A recorded "proposed delegation" is not an enforced authority state.**
   Existing consumers accept any active non-autonomous delegation. Automatic
   issuance from profile configuration could mint usable bounds or replenish
   cumulative ceilings. Proposal bounds must stay outside executable grants
   until authorized, or the state must be enforced by every consumer.
4. **Profile and plan concurrency guards are incomplete.** The frozen plan hash
   binds proposal/delegation/config but not the profile revision; campaign
   authorization stores neither. Start must validate an expected plan hash and
   the exact profile/spec/config/delegation bindings inside its transaction.
5. **Independent role routing is not implemented.** One gateway serves Lead and
   semantic review; a profile cannot be `executable: true` merely because a
   provider is configured. Exact per-role gateway construction, Lead/implementer
   separation and candidate-to-profile reviewer lookup must exist first.
6. **Budget composition is underspecified.** Per-call and whole-Campaign limits
   must be defined, including the assessment session, deadline intersection and
   model-call reservations. Profiles may never raise Core/provider policy caps.
7. **Cancellation needs fencing.** An intake-only `CANCELLED` flag leaves a
   READY Lead request executable through the existing endpoint and can hide a
   started Campaign. Cancellation and approval/start must serialize.
8. **Atomic creation/replay needs transaction-scoped operations.** Existing
   creation methods take the write lock and own transactions; calling them
   consecutively is not atomic. Freeze/READY is split across commits and replay
   checks lifecycle before replay. Extract shared transaction-scoped operations
   and add upgrade/concurrency/replay tests.

The concrete design is recorded in
[ADR-0029](../architecture/decisions/ADR-0029-request-scoped-operator-approval.md)
as **Proposed**. Until the three foundation commits are implemented and
re-reviewed, the intake endpoints and the Home approval action remain
provisional UI states only. The implementation sequence is:

1. `feat(domain): freeze intake context and defer executable ActionSpec insertion`
2. `feat(auth): add protected operator Lead start and withdrawal fencing`
3. `feat(execution): enforce frozen role routes and aggregate intake budgets`

The operators of the campaign must not enable autonomous start by default to
make manual approval work.

## 4. Integration rules

- Shared files (`native/src/bin/core.rs`, `native/src/lib.rs`,
  `native/src/db/mod.rs`, `office/src/coreApi.ts`, `office/src/App.tsx`,
  `office/src/navigation.ts`, shared CSS, parity JSON) are merged by the
  integration owner.
- Migrations: profiles `0026`, intake `0027`; later lanes allocate only after
  checking the integration baseline.
- A lane that cannot compile against a dependency must keep its module behind a
  provisional typed boundary and report the exact blocker; it must not duplicate
  the dependency's state or database logic.
- Every endpoint added here must be authenticated with the operator/device
  session scope. Office is never the enforcement point.
