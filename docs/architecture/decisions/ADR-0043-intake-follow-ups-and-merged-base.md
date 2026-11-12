# ADR-0043 — Intake follow-ups and merged base lineage

**Status:** Proposed — 2026-11-11. This ADR records the operator follow-up
contract for unified intake. It is not accepted and grants no merge, delivery
or autonomy authority until the protected approval path records acceptance and
independent review.

**Supersedes:** none.

**Extends:** [ADR-0029](ADR-0029-request-scoped-operator-approval.md),
[ADR-0031](ADR-0031-owner-confirmed-subscription-admission.md),
[ADR-0036](ADR-0036-owner-granted-autonomous-r0-intake.md) and
[ADR-0040](ADR-0040-risk-tiered-delivery-and-auto-merge.md).

**Contract:** [intake follow-up](../../contracts/intake-follow-up.md).

## Context

Unified intake deliberately refuses a message after a plan is frozen or a
Campaign has started. That protects frozen evidence, but it leaves no ordinary
way to continue an operator conversation once a request completes. Creating a
new request by hand also loses the Project/Lead thread, and the new request
would still plan from the old Project base because a delivery merge did not
update the Project config base commit.

The product needs a bounded continuation that is not an amendment:

- an additional authenticated operator message to a terminal request must be
  durable and visible in the same thread;
- it must start a real request, not mutate the frozen parent;
- the child must use the same Project and exact profile revision and the same
  owner-confirmed execution-policy path;
- the child must go through assessment, plan/environment/spec freeze and a new
  Campaign;
- after a successful Factory delivery merge on the protected default branch,
  the next request must build from that exact merge commit;
- a failed, blocked, reverted or other-branch delivery must never advance the
  base.

## Decision

### 1. One follow-up per terminal request

`POST /intake/requests/{id}/message` keeps its non-terminal behaviour: before a
plan is frozen it appends durable operator context. When Core derives a terminal
`COMPLETED` or `CANCELLED` state, the same authenticated message:

1. records the message on the parent thread;
2. creates exactly one child intake request in the same Project;
3. links the child to the parent request and the exact source message;
4. uses the message body as the child objective;
5. keeps the parent's exact execution-profile revision and intake mode;
6. resolves the same owner-confirmed execution-policy path.

A withdrawn request remains fenced and refuses messages. `BLOCKED`, `FAILED`,
`ASSESSED`, `AWAITING_APPROVAL` and `EXECUTING` keep their existing reasons; the
operator uses a new request or the Campaign workspace. A second distinct message
to a terminal parent returns `INTAKE_FOLLOW_UP_EXISTS`; the operator continues in
the created child. Replaying the same message idempotency key returns the same
message and child.

### 2. The child is new evidence, not an amendment

The child has its own Action, Lead request, execution context, policy binding,
submission snapshot, assessment, plan/environment/spec freeze and Campaign. It
must not copy the parent's:

- frozen Lead plan or proposal;
- ActionSpec or checks;
- candidate, tree, Gate decision or delivery attempt;
- Campaign, Run or Attempt.

Sending a follow-up message is not approval. The child still follows the normal
intake path, including the autonomous path only when its owner-confirmed policy
allows it.

### 3. Policy continuity may re-bind, never widen

A Core-owned base advance changes the Project config hash. When no current grant
matches that new hash, Core may re-bind the exact parent grant to the current
config only when all of the following hold:

- the parent grant is acknowledged, not revoked and not expired;
- the authenticated principal is the principal that confirmed the parent grant;
- the exact profile revision and content hash are unchanged and executable;
- the current Project paths, capabilities, route, R0 risk class, operation
  limits and progress policy match the parent grant;
- for `AUTONOMOUS`, the parent grant is autonomous and its subscription
  confirmation is still true.

Core copies the grant terms unchanged into a new versioned grant bound to the
current config. It does not add paths, capabilities, risk classes, autonomy,
subscription confirmation or budget. If any condition fails, the follow-up is
refused with `FOLLOW_UP_EXECUTION_POLICY_REQUIRED`.

### 4. Merged main becomes the next base

After a Factory delivery has verified that the protected default branch points
at the exact merge commit and any required post-merge check passed, Core writes a
new Project config revision whose base commit is that merge commit. The write is:

- idempotent for the same merge commit;
- guarded by the expected config revision;
- refused for a non-default branch;
- recorded as an append-only `project.base_advanced` event;
- never performed for a blocked, failed, reverted or other-branch delivery.

A later follow-up uses that config revision. The merge commit identity comes from
the provider-confirmed merge and branch read, never from a model or Office
projection. A branch-read error never falls through to base advancement. A
replay that repairs a missing base write re-proves the same default-branch head,
exact merge SHA and required post-merge success; absent or failed evidence leaves
the base unchanged.

### 5. Office thread projection

The Request workspace shows:

- terminal follow-up eligibility and an explanation that sending starts a new
  request;
- the parent link on the child;
- the created child and its truthful state on the parent;
- a refusal/attention state when the parent policy path is no longer valid.

No Office control approves, starts or mutates the child outside the existing Core
operations.

## Invariants

1. A follow-up never mutates the parent request, plan, spec, candidate or Campaign.
2. A terminal parent creates at most one child; replay resolves to it.
3. The child uses the same Project and exact profile revision.
4. Policy continuity re-binds exact terms; it never widens authority.
5. The child must pass assessment, freeze and a new Campaign.
6. Only a verified protected-default-branch merge advances the Project base.
7. Blocked, failed, reverted and other-branch deliveries never advance it.
8. Office is a projection; Core owns lineage, policy and base identity.
9. Message text is operator context, never model instruction or approval.

## Consequences

- Operators can continue a completed or cancelled thread without losing Project
  context or editing frozen evidence.
- The next request plans from merged main rather than a stale base.
- A base-only config advance does not force the owner to re-confirm an unchanged
  policy, while a real scope change still requires a new grant.
- Additional storage is required for parent/source lineage and a config revision
  event; historical rows remain valid.
- Advancing the base changes the Project config identity. An older unstarted
  request whose frozen context still names the previous config stays fenced by
  `PROJECT_CONFIG_CHANGED`; it is not silently rebased. The operator starts a new
  request or continues the completed thread as a follow-up.
- A real Chromium/Compose follow-up journey and independent acceptance remain
  required before this ADR can claim verification.
