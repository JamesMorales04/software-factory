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

`executable` is true only when every required role route can actually be built
from an enabled, configured provider. `warnings` must explain each false state.
A cross-provider profile is allowed only when Core can route each declared role;
until a role route is integrated end to end, the profile is not executable and
Core must reject an attempt to use it for that role.

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

The `opencode` and `opencode-go` providers are distinct connections. A route is
executable only if that provider is configured, enabled and has a decryptable
credential. Unknown providers/models are rejected; the UI may offer a clearly
labelled manual model ID only as a non-executable draft, never as a working
route. The registry may be extended only with tested adapter support.

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
- Select the latest active non-revoked delegation for the project that matches
  the profile route and R0 scope; if none exists, create a bounded **proposed**
  delegation derived from the profile (never `autonomousStart: true` unless the
  profile and Core policy explicitly allow it). Recording a proposed delegation
  does not authorize execution.
- Create the derived Action, delegation (if needed), and Lead request in one
  transaction with idempotent replay. The Action spec may start with empty
  checks/allowed paths; the Lead proposal supplies the concrete plan.
- Return `201` with the projection, `200` on idempotent replay.

`POST /intake/requests/{id}/assess` runs the existing bounded assessment path
using the frozen profile's `lead` route. It returns the refreshed projection.
No model route → `503` with a typed reason; do not fall back silently.

`POST /intake/requests/{id}/approve` body:

```json
{ "expected_plan_hash": "sha256-hex", "actor": "office-operator", "rationale": "..." }
```

- Requires a frozen plan and exact hash match.
- Derives the Campaign authorization from the profile's `implementer` route,
  the frozen plan's budgets/risk and the project config's allowed paths.
- If `mode` is `AUTONOMOUS` and the profile `autoStart` policy is enabled and
  Core policy permits it, this may be idempotently invoked by the submission
  flow; otherwise the UI must present an explicit approval.
- Never broadens the frozen plan, budget or path scope; returns `409` if the
  plan is stale or the authorization cannot be derived.

`POST /intake/requests/{id}/message` body
`{ "body": "...", "idempotency_key": "uuid" }` appends a durable operator
message. Messages never mutate a frozen plan. A material amendment after freeze
must create a new request/revision; return `409` with a typed reason.

`POST /intake/requests/{id}/cancel` body `{ "reason": "..." }` records an
intake-only cancellation. It does not cancel an authorized Campaign; the
existing Campaign cancellation operation remains authoritative.

`GET /intake/requests?state=&project_id=&limit=&offset=` and
`GET /intake/requests/{id}` are bounded and authenticated.

### 3.4 Idempotency and concurrency

- The request creation key hashes project, objective, profile revision, mode and
  operator identity.
- `approve` uses the plan hash as its concurrency guard.
- No client-supplied authorization budget is accepted.
- Frozen profile revision and plan identity are stored in the intake row and in
  the Campaign authorization.

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
