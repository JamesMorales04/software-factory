# Proposed contract: Intelligent Lead planning (F1-S4-D)

**Status:** Proposed; current implementation shape, not a Product Owner grant or
acceptance claim. **Version:** `1` (experimental, pre-release). **Owner:** Core.
This contract does not authorize paid model use. TRUSTED_LOCAL only; this is not
SEC-01 isolation certification.

## 1. Scope and authority

Lead turns one request for an active registered Project into an advisory
assessment and, only when the intent is executable and validation succeeds, a
frozen ActionSpec and Campaign. It reuses the Project → Action → ActionSpec →
Campaign → Run → Candidate → Evidence → Quality Gate path. It does not add a
second scheduler, Run lifecycle, delivery path or acceptance authority.

The assessment mechanism is `MODEL`. Proposal validation and execution
authorization are deterministic; the end-to-end plan path is `HYBRID`. Core
owns identities, state, scope, budgets, route eligibility, expiry/revocation,
Campaign start and external effects. Model output is advisory. It cannot grant
capabilities, increase a grant, start a Campaign, approve its own evidence,
change the Quality Gate or authorize delivery.

Standing-delegation semantics remain **proposed pending Product Owner
decision**. The fields below do not establish who may issue a grant, what risk
classes are acceptable, whether paid use is allowed, or how to handle in-flight
work after revocation. No defaults grant authority. An implementation route or
local operator credential does not substitute for Product Owner approval.

## 2. Identities and immutable bindings

The domain types are `domain.StandingDelegation`, `domain.LeadPlanProposal`,
`domain.LeadWorkUnit` and `lead.Assessment`.

| Record | Binding |
| --- | --- |
| Standing delegation | Core-assigned `id`, `principalId`, `auditId`; `projectId`; positive `revision`; immutable `contentHash`; explicit scope and budgets; expiry; optional one-way `revokedAt` |
| Lead request | Core ID, project, objective, idempotency key, request digest, delegation ID and Project config hash |
| Assessment | Schema revision `1`, request/project/action IDs, evidence references and advisory judgments |
| Proposal | Project/action IDs, full Git base revision, Project config hash, objective class, checks, route/budgets and WorkUnits |
| Frozen plan | Proposal hash + delegation hash + Project config revision/hash, combined in `planHash` |
| Resulting execution | Existing ActionSpec and Campaign IDs; later state/evidence are read from those authoritative Core records |

The proposal's `BaseRevision` must be a full Git object ID. Domain validation
cannot establish that it is a current Project base tip; Core re-inspects the
repository and checks it against registered base refs before freezing the
ActionSpec. The Project configuration hash must match exactly at request,
assessment and plan-freeze boundaries.

## 3. Standing delegation

`StandingDelegation` has explicit fields:

```text
id, principalId, auditId, projectId, revision, contentHash,
objectiveClasses, allowedPaths, capabilities, riskClasses,
provider, model, paseoMode, maxAttempts, maxTimeoutSecs,
maxRepairCycles, maxWallClockSecs, maxParallel, maxSessions,
expiresAt, revokedAt?
```

The closed objective classes are `feature`, `maintenance`, and `refactor`.
Risk class uses the global `R0`–`R3` vocabulary. The provider/model/Paseo mode
triple must exactly match the existing `ValidatePaseoRoute` allowlist; there is
no fallback or substitution. Paths must be safe repository-relative paths.
Capabilities use the S4-C closed vocabulary and cannot exceed the Project
grant.

Current structural limits are those already enforced by domain contracts:
Implementer attempts `1..5`, timeout `30..600` seconds, repair cycles `0..3`,
wall clock `120..7200` seconds, parallelism `1..2`, and sessions `1..48` (the
existing campaign task/execution ceiling). Values must be supplied explicitly;
zero does not select a default. The proposal must fit within the grant, and
`1 + budget.maxAttempts × (maxRepairCycles + 1) ≤ maxSessions`; the leading
`1` is the single assessment session.

The grant is valid only for its named active registered Project, before
`expiresAt`, and while `revokedAt` is unset. Its content hash excludes the
mutable revocation timestamp. Revocation is one-way. Product Owner approval
must define grant issuer identity, revision rules, allowed classes/risk,
renewal, paid-use accounting and in-flight revocation semantics before this
contract can be treated as approved authority.

Each delegation can reserve one distinct Lead request. Replaying that same
request with its original idempotency key and content returns the existing
reservation; a second distinct request under the consumed delegation is
rejected. The owner must issue another explicit delegation for another
objective.

`maxSessions` counts the single assessment session plus the proposal's bounded
Implementer attempt horizon. It is not provider billing evidence or a monetary
spend cap. Unknown cost and quota remain unknown.

The paid-assessment authorization gate is closed by default. Core requires all
of `--lead-paid-authorization-id`, `--lead-paid-project-id`,
`--lead-paid-base-revision`, `--lead-paid-provider`, `--lead-paid-model`,
`--lead-paid-paseo-mode`, and `--lead-paid-max-sessions` to open it. A grant
must match that exact Project, current base revision, provider/model/mode, and
total session ceiling. `POST /api/v1/delegations` additionally requires its
`Idempotency-Key` to equal the configured authorization ID; the durable
idempotent record represents one grant, not reusable permission to mint more.
The live runner has a separate opt-in: `FACTORY_S4D_LIVE=1`, matching
`FACTORY_LIVE_AUTHORIZATION_ID`, the owner authorization envelope, and the
exact clean frozen Factory SHA are required. These gates do not themselves
constitute Product Owner approval or authorize a live session.

## 4. Assessment schema and model boundary

`lead.Assessment` is a strict schema-revision-1 object:

```json
{
  "schemaRevision": 1,
  "projectId": "…",
  "actionId": "…",
  "requestId": "…",
  "evidenceRefs": ["repository-files"],
  "intent": "change",
  "complexity": "small",
  "effortEstimate": {
    "status": "estimated",
    "unit": "person-hours",
    "minPersonHours": 2,
    "maxPersonHours": 5,
    "assumptions": ["The change is limited to one component."],
    "unknownReason": ""
  },
  "knowledgeGaps": ["…"],
  "changeImpact": ["…"],
  "validationNeeds": ["…"],
  "assumptions": ["…"],
  "uncertainty": ["…"],
  "suggestedNextAction": "…",
  "proposal": { "…": "LeadPlanProposal" }
}
```

This is a schema sketch, not a directly decodable payload: the `proposal`
object is abbreviated and its required typed fields are listed in §5.
`effortEstimate` is a required, non-null object with required fields `status`,
`unit`, `minPersonHours`, `maxPersonHours`, `assumptions`, and
`unknownReason`:

- `estimated` requires `unit: "person-hours"`, integer bounds
  `1 ≤ minPersonHours < maxPersonHours ≤ 10000`, at least one and at most 16
  nonempty assumptions (each at most 500 characters), and an empty
  `unknownReason`.
- `unknown` requires an empty unit, zero bounds, and a nonempty
  `unknownReason` of at most 500 characters. Its assumptions array may contain
  up to 16 nonempty strings of at most 500 characters.
- `not_requested` requires an empty unit, zero bounds, an empty assumptions
  array, and an empty `unknownReason`.

Core currently requires an estimate other than `not_requested` for every
`change` intent or when the objective's lowercased text contains `estimate` or
`how long`, or contains both `effort` and `range`. Otherwise it requires
`not_requested`. This trigger uses deterministic substring matching; it does
not establish that the user semantically requested an estimate.

Each cited evidence reference must exist in the bounded input context and may
appear only once. Notes arrays hold at most 16 nonempty strings of at most 500
characters; evidence references hold `1..32` items; next action is bounded to
500 characters. Unknown fields, duplicate object keys, null values and
trailing JSON are rejected. A
non-`change` intent cannot include executable WorkUnits; it blocks for owner
clarification or non-executable handling. A `change` proposal must match the
Project, Action, config hash and inspected base revision supplied to the
assessment.

The current assessor supplies bounded repository tree names as evidence, not
arbitrary project file bodies. Its evidence context is bounded to 32 entries,
24 KiB total and 4 KiB per value. Paseo creates a temporary local workspace,
requests one session using the grant's exact route, checks the observed session
identity/route, bounds execution and output, then archives the workspace.
This boundary is not an isolation certification. A configured real assessor
can contact a model provider; this contract and docs change provide no paid
session authorization.

## 5. Proposal and WorkGraph validation

`LeadPlanProposal` binds `projectId`, `actionId`, `baseRevision`,
`projectConfigHash`, objective and class, assumptions/risks, acceptance and
negative checks, quality profile, risk class, `SpecBudget`, exact provider /
model / Paseo mode, campaign repair/wall-clock/parallel/session limits and
`workUnits`.

S4-D accepts exactly one WorkUnit with kind `implementation`. It has a bounded
slug ID, `dependsOn`, safe `allowedPaths`, and closed `capabilities`. Since the
graph has one node, dependencies are unsupported: self-reference is reported
as a cycle and any other reference is missing. Zero or multiple units, other
roles, path escape, missing quality-profile capabilities, project/grant
capability expansion, unsupported risk/class/route, invalid checks, stale
bindings and budget overflow are rejected before spec or Campaign creation.

Quality profile must equal the registered Project profile. The proposal's
acceptance and negative checks must satisfy the existing profile-required
check kinds and `SpecCheck` validation. Core builds the ActionSpec from the
validated proposal and derives Project-bound repository/capability fields;
model-supplied copies do not replace the registered Project configuration.
Once frozen, the proposal and plan hashes are immutable. Execution uses the
existing Campaign repair/gate policy; `REPAIR_REQUIRED` alone can authorize a
bounded repair cycle.

## 6. API and state transitions

All routes use the existing authenticated `/api/v1/*` operator API.

| Method | Route | Meaning |
| --- | --- | --- |
| `POST` | `/api/v1/delegations` | Create explicit grant; requires `Idempotency-Key` |
| `GET` | `/api/v1/delegations/{id}` | Read grant and revocation state |
| `POST` | `/api/v1/delegations/{id}/revoke` | Revoke once; repeated revoke returns current state |
| `POST` | `/api/v1/lead/requests` | Reserve request idempotently, bind Action, assess and validate; requires `Idempotency-Key` |
| `GET` | `/api/v1/lead/requests/{id}` | Read request, assessment, proposal/plan hash, authorization and Campaign projection |
| `GET` | `/api/v1/lead/requests/{id}/events` | Read durable events for this Lead request |
| `POST` | `/api/v1/lead/requests/{id}/start` | Explicit authenticated owner transition to start a ready authorized Campaign after reviewing its plan/spec projection |

The request state machine is:

```text
submitted → assessing → planned → frozen → ready → running
     └──────────────┴────────┴────────┴────────┴────→ blocked
```

Submitting an objective can produce a `ready` plan but never starts its
Campaign. The Office renders the plan/spec projection before the owner invokes
the authenticated start transition. That transition rechecks current
delegation and Project binding; a plan becoming ready is not execution
authorization. No additional model prompts are inserted among the existing
Implementer, Verifier, Reviewer or Gate participants.

The GET projection includes the request, assessment, model observation,
proposal under `plan`, frozen `planHash`, Action/ActionSpec/Campaign links,
Campaign state, failure reason and computed authorization state. The Office
keeps the request ID in the URL and reloads this projection from Core after a
browser refresh. The request events endpoint returns the durable event list;
Office periodically refreshes that snapshot and does not claim an SSE stream.

Campaign completion is projected as `accepted`, `cancelled` or `failed` where
the API maps its terminal state. The Campaign remains authoritative for its
more precise terminal status and gate decision. Campaign `start` also rechecks
Lead authority; the orchestrator rechecks the frozen grant and bindings before
each scheduling tick. Revoked/expired grants or changed Project/config/spec
bindings cancel open Runs and block the Lead Campaign. This operational rule
is implemented fail-closed behavior; Product Owner policy for work already
running remains to be ratified.

The intake endpoint processes the assessment inline and synchronously in the
HTTP request. Code candidate `be592770b77ca81c491e5021913e1d23d5a228b7` sets
Core `WriteTimeout` to 300 seconds and the live runner's Lead POST timeout to
360 seconds. This gives headroom beyond bounded assessment/session work;
candidate deterministic checks passed. These checks do not provide live
session evidence or certification. Retries use the same idempotency key.
Before assessment,
`CreateLeadAction` atomically creates the Action, appends `ActionCreated`, and
binds the Action ID to the reserved Lead request in one transaction; replay
cannot leave an unowned Action. A missing assessor, non-change intent, invalid
assessment/proposal, base mismatch, workspace/session failure, or interrupted
external assessment is recorded as `blocked` with a bounded reason. On
restart, an `assessing` request is blocked for reconciliation and is not
automatically retried.

## 7. Idempotency, errors and resource bounds

- Lead intake requires a bounded idempotency key. Same key + same canonical
  request, delegation and Project config digest returns the original request;
  same key + different content conflicts. Replays do not create a second Action
  or assessment.
- Delegation creation is idempotent by key and grant intent; different intent
  under the same key conflicts. Assessment, proposal and plan are each
  single-write per request and bind stable request-derived keys/hashes.
- ActionSpec, Campaign and plan links are write-once. No client route mutates a
  frozen proposal, expands a grant, changes a route or bypasses Campaign.
- Invalid input maps to the existing `400 invalid`; expired/revoked or
  unauthorized scope to `403 unauthorized`; reused keys with changed content,
  stale configuration or illegal lifecycle to `409 conflict`; missing records
  to `404 not_found`; unexpected storage/runtime errors to `500 internal`.
- Assessment input/output, timeouts, context, paths, checks, retries and
  Campaign budgets are bounded by this contract and the referenced domain
  limits. Provider price and quota cannot be inferred from these limits.

## 8. Negative acceptance cases

The candidate-bound deterministic lane must reject:

- unknown/inactive/legacy Project, terminal or cross-project Action, mismatched
  Project config hash, stale/unregistered base tip, expired/revoked grant;
- missing authority fields or zero/omitted bounds used as implicit defaults;
- objective class/risk/route outside exact grant, including provider/model/mode
  substitution and unknown route;
- absolute, traversal, `.git`, outside-grant or outside-workspace paths;
- Project/grant capability expansion or missing profile-required capabilities;
- malformed/unknown-field assessment JSON, trailing JSON, mismatched identity,
  duplicate/missing evidence references, invalid intent/complexity, unbounded
  notes and executable work proposed for a non-change intent;
- malformed/missing/cyclic dependency, unknown WorkUnit kind, empty graph or
  more than one executable unit;
- missing profile check coverage, malformed `SpecCheck`, invalid budget,
  assessment plus Implementer attempts above the session ceiling, or any grant
  escalation;
- second distinct request on a consumed delegation;
- duplicate intake with changed content, duplicate assessment/proposal/plan
  with different identities, and attempts to rewrite frozen links;
- direct Run execution of a Lead-managed spec, campaign start after revocation,
  and scheduler progression after expiry, Project/config drift or stale
  frozen-plan binding;
- Core restart during `assessing` causing automatic model/session retry.

## 9. Evidence needed for acceptance

The current domain and store code establish implementation shape only. Before
this contract is called verified, run the deterministic API/store negatives,
restart and idempotency cases; exercise the real Product Owner-approved journey
with separately recorded live-session authorization; compare semantic
assessment against a reasonable deterministic baseline; and have an
independent verifier inspect the exact frozen candidate. The prepared live
runner checks the successful path by reconciling the request's assessment
session with the Implementer Run's durable session bindings, then validating
the exact Run, Attempt, frozen candidate, verification, review, Gate decision,
session count and observed routes. The runner has not been executed; this is
prepared checking logic, not evidence of a successful run. No paid session was
run or authorized by this contract-writing work. The implementation and Office
changes are committed in code candidate
`be592770b77ca81c491e5021913e1d23d5a228b7`; they have no live model result or
certification.

## 10. Compatibility

This is an experimental pre-release contract. Internal schemas may change
incompatibly under ADR-0006; update all callers, fixtures and docs atomically.
Persisted grant, request, assessment and plan records must retain their audit
and evidence history. Stable external use requires an explicit versioning and
migration decision.
