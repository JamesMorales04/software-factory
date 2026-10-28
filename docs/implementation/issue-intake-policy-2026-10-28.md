# Governed issue-intake policy, batch acknowledgement and proposal queue (P2)

**Status:** implementation of commits 1–2 of the independently designed
governed issue-intake contract (design branch commit `271c821`,
`docs/implementation/office-ux-autonomy-contract-2026-10-28.md`; not merged
into this lane). Execution remains **disabled**. This document is not an ADR
approval and does not authorize autonomous Campaign start.

**Candidate base:** `campaign/office-ux-baseline-20261028` at `924e026`.
**Branch:** `campaign/office-ux-autonomy-20261028`.

## Delivered boundary

| Layer | Delivered | Not delivered |
|---|---|---|
| Policy | Append-only Project revisions, default OFF, explicit acknowledgement, exact repository/author/label/profile/path/R0/budget bounds | Open-ended subscriptions, automatic policy widening |
| Batch | Finite acknowledgement of exact issue IDs, content digests and updated-at snapshots; non-autonomous delegation; audit | Automatic Campaign start, model assessment, Worker dispatch |
| Queue | Durable Core rows, bounded leases, direct-read freshness check, retry/backoff, restart recovery, replay | A second scheduler; an independent acceptance authority |
| Reconciliation | Ordinary `REVIEW_PLAN` intake request, draft Action, Lead request and execution context | Provider call, assessment, plan freeze, Campaign/Run/Attempt |
| Office | Policy form, manual checkbox selection, eligibility reasons, batch/queue states, explicit create-proposal action | Navigation changes, autonomous-start control |

A profile `autoStart` flag is never read by the issue-intake policy, batch,
claim or reconciliation path. The linked delegation is always
`autonomous_start = false` with an empty autonomous-risk list. The ordinary
operator approval/freeze/start journey remains the only route to a Campaign.

## Core routes

All routes require the authenticated operator bearer credential and the normal
CSRF/browser middleware. Request bodies use snake_case JSON.

| Method | Route | Purpose |
|---|---|---|
| `GET` | `/projects/{id}/issue-intake` | Truthful policy, revisions, batches, items, queue, audit and remaining-capacity projection |
| `PUT` | `/projects/{id}/issue-intake/policy` | Create a new policy revision with explicit acknowledgement |
| `POST` | `/projects/{id}/issue-intake/policy/disable` | Create a disabled revision and revoke active batches/delegations |
| `POST` | `/projects/{id}/issue-intake/batches` | Acknowledge exact issue snapshots and create a finite batch |
| `POST` | `/projects/{id}/issue-intake/batches/{batch_id}/revoke` | Revoke one batch without replenishing its reservations |
| `POST` | `/projects/{id}/issue-intake/queue/dispatch` | Claim one due queue item, perform one direct GitHub read, and reconcile at most one proposal |

`queue/dispatch` returns `IDLE` when nothing is due. `RECONCILED` returns the
ordinary intake proposal. `STALE`, `INELIGIBLE`, `BLOCKED`, `EXPIRED` and
`REVOKED` return `409` with stable reason codes and `campaign_started=false`,
`provider_calls=0`.

## Durable records

Migration `0033_issue_intake_policy.sql` adds:

- `issue_intake_policy_revisions` and `issue_intake_policy_heads` (absence of a
  head means OFF);
- `issue_intake_batches` and `issue_intake_batch_items` bound to immutable
  GitHub repository/issue IDs and content digests;
- `issue_intake_queue` with bounded lease/retry fields;
- append-only `issue_intake_audit`;
- immutable GitHub repository, author-ID, association and PR identity columns
  on the existing issue snapshot and binding tables.

Policy revisions are immutable by trigger. Audit rows are append-only. Batch
content and item snapshots are immutable; only status, reservation, link and
timestamps may change. Existing databases keep their current rows; identity
fields that predate the migration remain `NULL` and fail closed for issue
intake until a fresh sync/rebind populates them.

## Deterministic eligibility

Evaluated before any paid assessment and again immediately before a proposal is
created:

1. exact Project, repository binding and immutable repository ID;
2. active GitHub credential revision and enabled connection;
3. current enabled, unexpired policy revision and active unexpired batch;
4. exact issue number, GitHub issue ID, content digest, updated-at and author
   identity recorded in the batch item;
5. open, unlocked, non-PR, untruncated snapshot;
6. all required labels and no excluded label;
7. immutable author ID in the allowlist and current association in
   `OWNER`/`MEMBER`/`COLLABORATOR` and in the acknowledged set;
8. exact executable profile revision/hash, config hash and R0 ceiling;
9. allowed paths/capabilities inside the Project configuration, with protected
   policy/credential/sandbox/delivery/migration paths rejected;
10. positive batch issue/cost capacity, no other active Project work and no
    prior reconciled item for the same Project/repository/issue.

The policy records an R0 ceiling, but commits 1–2 run no model or semantic
classification. A reconciled proposal is therefore **not risk-certified** and
remains a `HUMAN_REQUIRED` draft; it cannot execute through this lane.

Unknown or missing identity, association, profile execution, credential,
capacity or freshness facts fail closed. Issue text is rendered into the
proposal objective inside explicit `BEGIN UNTRUSTED GITHUB ISSUE` /
`END UNTRUSTED GITHUB ISSUE` boundaries. It is never treated as instruction or
authority.

## Queue, freshness and recovery

- One Core write transaction claims a queue item; a second concurrent claim for
  the same Project returns `IDLE` while a non-expired lease exists.
- The dispatch handler performs one direct `GET /repos/{owner}/{repo}/issues/{number}`
  read through the existing bounded GitHub client. Redirects are refused.
- The observed digest, identity, updated-at and author must equal the
  acknowledged item and the durable snapshot. A mismatch marks the item
  `STALE`; no proposal is created.
- Transient upstream failures retry within three attempts with 30s/120s
  backoff. Authentication, permission, not-found, redirect and identity
  failures are terminal.
- Credential rotation/revocation revokes active batches and their
  non-autonomous delegations before any proposal. A direct read that GitHub
  denies (`401`/`403`) also revokes the batch and its delegation.
- `Database::open` recovers leases left by a previous Core process: committed
  proposal links become `RECONCILED`; uncommitted leases return to `PENDING` or
  `BLOCKED` after the retry budget.
- Exact replay returns the original intake/proposal IDs without a second
  reservation, provider call or Campaign, even after revocation.

## Office states

The Issues view contains a **Governed issue intake** panel:

- default OFF with no policy;
- policy form with bound repository/immutable ID, executable profile revision,
  required/excluded labels, author IDs, associations, paths, capabilities,
  R0 ceiling, budgets, expiry and explicit acknowledgement;
- enabled policy summary and disable confirmation;
- manual checkbox selection only (no select-all, subscription or automatic
  inclusion);
- finite batch acknowledgement with exact IDs/digests and a batch budget;
- stored and current eligibility reasons, stale/ineligible/revoked states,
  remaining issue/cost capacity and audit;
- queue status, attempt counts, next retry, reason codes and an explicit
  **Create proposal** action;
- recovery/error states for missing binding identity, disconnected/revoked
  credentials, stale snapshots, policy/profile changes and budget exhaustion.

The panel never renders a Campaign start control or claims that a proposal has
started work.

## Verification performed

- `cargo fmt --all`
- `cargo check -p factory-native --all-targets`
- `cargo clippy -p factory-native --all-targets`
- `cargo test -p factory-native --lib` (all library tests)
- targeted `cargo test -p factory-native --bin factory-core`
- Office `tsc --noEmit`, Biome, Vitest for the panel/Issues views and full view
  build

Database tests assert: default OFF; missing policy; unacknowledged batch;
new/edited/closed/excluded/contributor/untrusted-author items; budget
exhaustion; credential rotation; stale snapshots; duplicate reconciled issues;
queue restart/lease recovery; cross-project lookup isolation; exact replay;
adversarial issue text; and **zero** `model_call_reservations`, **zero**
Campaigns for both eligible reconciliation (proposal only) and ineligible
items.

## Explicitly NOT_TESTED / disabled

- No real authorized GitHub issue read, real provider assessment, Campaign
  start, Worker dispatch, PR write, merge or delivery was exercised.
- No real Compose/browser acceptance run is claimed for this lane.
- Real GitHub credentials, fresh provider spending authorization and a live
  end-to-end issue Campaign remain **NOT_TESTED/BLOCKED**.
- ISOLATED execution remains fail-closed. TRUSTED_LOCAL is not isolated.
- Commits 1–2 do not call the autonomous start path and do not enable
  `FACTORY_ENABLE_AUTONOMOUS_START`.

## Proposed ADR amendments (not approved)

A protected ADR must, before commit 3:

- amend ADR-0029's profile-bound intake restriction for validated autonomous
  issue envelopes;
- define the standing-autonomy issuer, risk and revocation policy;
- reconcile the shared authorization/MASTER contracts;
- require independent architecture/security review, real Compose/browser
  acceptance and a fresh owner-authorized live run.

This lane grants none of that authority.
