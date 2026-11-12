# Contract: intake follow-up and merged base lineage

**Status:** Proposed WP0-style contract — 2026-11-11. **Version:** `1`.
Experimental pre-release contract; breaking changes update all in-repo callers,
fixtures, docs and tests atomically ([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

**Decision:** [ADR-0043](../architecture/decisions/ADR-0043-intake-follow-ups-and-merged-base.md).
**Related:** [agentic flow](agentic-flow.md), [delivery policy](delivery-policy.md),
[issue lifecycle](issue-lifecycle.md), [ADR-0029](../architecture/decisions/ADR-0029-request-scoped-operator-approval.md),
[ADR-0031](../architecture/decisions/ADR-0031-owner-confirmed-subscription-admission.md),
[ADR-0036](../architecture/decisions/ADR-0036-owner-granted-autonomous-r0-intake.md).

## 1. Scope and authority

This contract defines how an additional authenticated operator message to a
terminal unified-intake request starts one follow-up request, and how a verified
Factory delivery merge advances the Project base.

It does not authorize a model, Office control or message to approve a plan,
start a Campaign, widen paths/capabilities/risk, reuse a candidate or delivery, or
advance the base for an unverified merge. Core remains the authority.

## 2. Terminal message semantics

A request is follow-up eligible only when Core derives `COMPLETED` or
`CANCELLED` from authoritative Lead, Campaign and Gate records.

For `POST /intake/requests/{id}/message` with an authenticated operator:

| Parent state | Result |
| --- | --- |
| `SUBMITTED`, `ASSESSING`, `PLAN_READY` | append durable operator message |
| `AWAITING_APPROVAL`, `EXECUTING` | typed refusal; use the existing request/Campaign path |
| `COMPLETED`, `CANCELLED` | record message and create one child follow-up |
| withdrawn | `INTAKE_WITHDRAWN` refusal |
| child already exists | `INTAKE_FOLLOW_UP_EXISTS` |
| replay of the same idempotency key | return the original message and child |

The message body becomes the child objective. The child uses the same Project
and the parent's exact execution-profile revision and intake mode.

## 3. Lineage

| Entity | Identity | Notes |
| --- | --- | --- |
| `IntakeFollowUp` | child `requestId` | `parentRequestId` and `sourceMessageId` are immutable |
| Parent follow-up | `(parentRequestId)` unique | At most one direct child per terminal parent |
| Message follow-up | `(sourceMessageId)` unique | Idempotent replay resolves to the same child |

The parent thread exposes the child and its state; the child exposes its parent.
Lineage is not an approval, execution binding or delivery claim.

## 4. New evidence only

The child must have a new:

```text
Action, Lead request, execution context, submission snapshot, policy binding,
assessment, plan/environment/spec freeze, Campaign, Run and Attempt.
```

The child must not reuse the parent's frozen plan/proposal, ActionSpec/checks,
candidate/tree/Gate decision, Campaign/Run/Attempt or delivery attempt.

A follow-up is not an amendment to the parent. Frozen parent evidence remains
read-only and historical.

## 5. Policy continuity

Core first tries the ordinary exact match for the authenticated principal,
Project, exact profile revision/content hash and current Project config hash.

When a Core-owned base advance changed only the config identity, Core may re-bind
the parent owner-confirmed grant to the current config. Re-binding requires:

1. the same authenticated principal that confirmed the parent grant;
2. an acknowledged, unrevoked, unexpired parent grant;
3. the same exact profile revision and content hash, still executable;
4. equal Project paths, capabilities, route, R0 risk class, operation limits and
   progress policy;
5. for autonomous mode, parent autonomy and subscription confirmation both true.

The new grant copies those terms unchanged. Any missing condition returns
`FOLLOW_UP_EXECUTION_POLICY_REQUIRED` or
`FOLLOW_UP_POLICY_PRINCIPAL_MISMATCH`. No path, capability, risk, route,
autonomy, subscription confirmation or budget is widened.

## 6. Merged base advance

`DeliveryService` advances the Project base only after:

1. the exact candidate merge is recorded `MERGED`;
2. the protected default branch is read and points at the exact merge commit;
3. any required post-merge check passed.

The typed database operation:

- validates a 40-hex merge commit and the Project default branch;
- appends a new versioned Project config revision with the merge as base;
- fences on the expected current config revision and the exact expected current
  base, and only conditionally updates that base;
- returns the current revision idempotently when the base already equals the
  merge commit;
- records `project.base_advanced`.

The delivery layer proves Git ancestry before the write: an equal base or a
merge that already descends from the current base does not move backward, while
an older ancestor or divergent merge is refused. After an optimistic-concurrency
conflict it re-reads the current base and repeats the ancestry check before any
retry. The database cannot perform Git ancestry itself.

It is not called for blocked, failed, reverted or other-branch deliveries. A
manual revert does not claim a base rollback.

The config advance changes the Project config identity. An older unstarted
request whose frozen context names the previous config remains fenced by
`PROJECT_CONFIG_CHANGED`; Core never silently rebases it. The operator starts a
new request or continues a completed thread as a follow-up.

## 7. Office projection

The Request workspace must show:

- follow-up eligibility, copy that sending starts a new request, and the exact
  parent/child link;
- the child state and a refusal when the parent policy path is unavailable;
- `follow-up-eligible`, `follow-up-created`, `follow-up-exists` and
  `follow-up-policy-required` states;
- no control that approves, starts or mutates a child outside Core;
- EN/ES copy and non-color state cues.

## 8. Failure codes

| Code | Meaning |
| --- | --- |
| `INTAKE_FOLLOW_UP_EXISTS` | The terminal parent already has one child |
| `FOLLOW_UP_EXECUTION_POLICY_REQUIRED` | The parent policy no longer matches the current Project/profile scope |
| `FOLLOW_UP_POLICY_PRINCIPAL_MISMATCH` | A different principal tried to continue the confirmed policy |
| `INTAKE_WITHDRAWN` | Withdrawn requests remain fenced |
| `INTAKE_PLAN_FROZEN` | Non-terminal frozen/approval request still needs the existing path |
| `CAMPAIGN_STARTED` | Active execution still uses the Campaign workspace |
| `PROJECT_BASE_UPDATE_CONFLICT` | The Project config revision/base fence changed |
| `PROJECT_BASE_DIVERGED` | The verified merge does not descend from the current Project base |
| `PROJECT_BASE_UPDATE_FAILED` | Delivery could not record merged main |
| `DEFAULT_BRANCH_UNCONFIRMED` | The protected default branch could not be read after merge or replay |
| `MERGE_IDENTITY_MISMATCH` | The default branch or post-merge evidence is bound to another merge |
| `POST_MERGE_CHECK_MISSING` | Required post-merge evidence is absent or not passing |
| `POST_MERGE_CHECK_FAILED` | The recorded post-merge check failed |

## 9. Acceptance cases

Positive:

- one message to a completed request creates exactly one child with the exact
  parent/source lineage, a new Action/Lead/context, no frozen plan/ActionSpec and
  no Campaign;
- idempotent replay returns the same message and child;
- a follow-up after a verified default-branch merge uses the merge commit as its
  assessment base and gets a fresh Campaign path;
- a base-only advance re-binds the unchanged parent policy terms.

Negative:

- a second distinct message cannot fork another child;
- a cross-project, withdrawn, unauthorized or stale-policy input is refused;
- a blocked, failed or other-branch delivery does not advance the base;
- a follow-up never inherits the parent plan, spec, candidate or Campaign;
- non-terminal frozen/approval states keep their existing refusal codes.

## 10. Compatibility

This is an experimental pre-release contract. The migration after `0078` adds
nullable lineage columns and preserves existing rows. Parent/source lineage is
immutable; a correction uses a new request/thread.
