# ADR-0042 — Issue-linked delivery lifecycle

**Status:** Proposed — 2026-11-10. This ADR records the Product Owner's
program constraints for implementation. It is not accepted and grants no GitHub
write, merge, close or delivery authority until the protected approval path
records acceptance and independent review.

**Supersedes:** none.

**Extends:** [ADR-0038](ADR-0038-owner-charter-and-delegated-autonomy.md),
[ADR-0039](ADR-0039-quality-first-planning-and-stack-selection.md) and
[ADR-0040](ADR-0040-risk-tiered-delivery-and-auto-merge.md).

**Contract:** [issue-lifecycle](../../contracts/issue-lifecycle.md).

**Relates to:** the existing read-only [GitHub Issues integration](../../implementation/github-issues-integration-2026-10-28.md)
and the governed [issue intake policy](../../implementation/issue-intake-policy-2026-10-28.md).

## Context

The current issue integration is deliberately read-only. It synchronises
bounded issue snapshots and supports a default-off intake policy that can
reconcile a manually acknowledged issue batch into an ordinary `REVIEW_PLAN`
proposal. It does not create an Action/Campaign relationship, write a PR
comment, close an issue or prove that a merge delivered the issue.

The next program needs traceable delivery:

- a request must say which issue(s) it is delivering before work starts;
- delivery must not let a model choose a different issue, add an issue after the
  fact or close an issue that was only partially addressed;
- Office and GitHub must show the same state without inventing progress;
- a PR must reference its issues when it opens, but an issue may close only
  after the exact PR is merged and any required post-merge check passes;
- partial work must leave the issue open.

The risk is not only accidental closure. A model could be asked "which issue
does this fix?" at delivery time and choose a plausible issue that was never in
scope. That would create false traceability after the candidate and evidence
were already frozen. Issue identity must be decided before the work and must be
immutable for that request.

## Decision

### 1. Issue links are frozen at request creation

An authenticated operator, the deterministic issue-intake policy or another
authenticated Core operation selects the exact issue links when the request is
created. Each link records:

- Project, repository full name and immutable GitHub repository ID;
- issue number, immutable GitHub issue ID, URL and a bounded title snapshot;
- the issue state at link time and the content/updated-at digest used to detect
  change;
- the link mode: `CLOSES` (this request is intended to deliver the issue) or
  `REFERENCES` (context only);
- the completion scope: `FULL` or `PARTIAL`;
- the authenticated principal and source (`OPERATOR`, `ISSUE_INTAKE`,
  `POLICY`, `API`);
- the link-set hash included in the frozen request/spec.

The link set is immutable for the request. A later edit, relink, repository
transfer, issue edit or issue deletion does not silently change it; it produces
`NEEDS_ATTENTION` or a new request/link revision. A model may summarize or quote
bounded issue text during planning, but it may not choose, add, remove, replace
or reclassify a link at planning, implementation, review, merge or delivery
time. The delivery path reads the frozen link set only.

A `CLOSES` link with `FULL` scope is eligible to close after merge. A `PARTIAL`
scope, or a `REFERENCES` link, never closes automatically; it is commented and
left open. The operator must create a new request/link or explicitly mark a
later request as the full delivery; a model cannot do that.

### 2. Issue lifecycle states

Each issue link has one durable lifecycle record with these states:

| State | Meaning |
| --- | --- |
| `READY` | The issue is linked and eligible; no frozen work has started. |
| `QUEUED` | The request/spec is frozen and awaiting or queued for execution. |
| `IN_PROGRESS` | At least one authorized Run/Attempt is implementing or verifying the linked scope. |
| `NEEDS_ATTENTION` | Blocked by an owner decision, failure, stale GitHub state, merge conflict, missing evidence, failed check, incident or an unprovable link. |
| `IN_REVIEW` | The exact candidate passed the Core Gate and an open PR/check review is the active delivery step. |
| `DONE` | The exact PR was merged, any required post-merge check passed and every `CLOSES`/`FULL` link was closed with evidence. |

Allowed transitions:

```text
READY -> QUEUED -> IN_PROGRESS -> IN_REVIEW -> DONE
  |        |            |             |
  +--------+------------+-------------+--> NEEDS_ATTENTION
NEEDS_ATTENTION -> QUEUED | IN_PROGRESS | IN_REVIEW
```

`DONE` is terminal for that link. A reopened or newly edited issue is a new
lifecycle/request; it never rewinds a completed delivery record. `NEEDS_ATTENTION`
is not a failure claim; it is an explicit human/owner attention state with a
stable reason code.

### 3. PR-open comment

When a candidate PR is opened through Core, Core posts at most one comment per
`(request, PR, candidate, link-set hash)` that lists the linked issues, the exact
candidate SHA, the Gate outcome and the delivery tier. The comment is
idempotent, bounded, plain text and contains no token, prompt, source body or
unbounded issue text. It uses `Refs #...`/linked-issue notation, not closing
keywords such as `Fixes #...`; Core owns closure so it can enforce "only after
merge" and record the close receipt.

The PR comment is a projection, not delivery evidence by itself. A failed
comment does not authorize a merge or a close; it records a retryable
`ISSUE_COMMENT_FAILED` and leaves the lifecycle truthful.

### 4. Close only after merge

Core closes a linked issue only when:

- the link mode is `CLOSES` and the completion scope is `FULL`;
- the PR is confirmed merged into the expected protected base;
- the merged head/merge commit is bound to the exact candidate and delivery
  attempt;
- every required pre-merge check and, when the delivery policy requires it, the
  post-merge check has passed;
- no delivery incident is open for that merge;
- the issue identity still matches the frozen link and the issue is still open.

GitHub's own "closing keyword" auto-close is not the authority. If an external
actor adds a closing keyword or closes the issue, Core records the observed
state and does not claim Factory delivery; a premature close is
`ISSUE_CLOSED_BEFORE_MERGE` and `NEEDS_ATTENTION`. If GitHub already closed the
issue after the merge, Core records `ALREADY_CLOSED` as an idempotent success.

Partial issues stay open. A request that implements one slice, one acceptance
criterion or one linked issue among several must not close a `PARTIAL` or
`REFERENCES` link, and must not close a `CLOSES`/`FULL` issue if the frozen
request/evidence does not prove the full linked scope. The PR comment and Office
state say "partial" explicitly.

### 5. Delivery state is derived, not invented

The issue lifecycle state is a Core projection over frozen link identity, work
state, candidate/Gate state, PR/check state, delivery attempt, merge and
post-merge evidence. Office and GitHub comments never advance a state on their
own. A webhook, a model message or a UI action that is not an authenticated Core
operation cannot move a link to `IN_REVIEW` or `DONE`.

External reconciliation is idempotent and may move a link to
`NEEDS_ATTENTION` when the issue/PR/repository identity no longer matches. It
never silently relinks, closes a different issue or marks a delivery `DONE`
without merge and check evidence.

### 6. What this does not authorize

This ADR does not authorize issue creation, arbitrary issue editing, label or
assignee writes, PR creation outside the existing Core path, merge, deployment,
or closing an issue without the exact merge evidence. It does not let a model
read issue text as instructions or authority. It does not change the default-off
issue-intake policy or enable autonomous Campaign start. It does not certify a
live GitHub write until a real authorized run is recorded.

## Invariants

1. The issue link set is fixed at request creation and immutable.
2. No model chooses, adds, removes or reclassifies an issue at delivery.
3. `CLOSES` + `FULL` is the only closable link; `PARTIAL`/`REFERENCES` stay open.
4. A PR comment is idempotent and uses no closing keyword.
5. An issue closes only after a confirmed merge and required checks.
6. A premature/external close is an attention state, not a Factory success.
7. State is derived from Core evidence; Office/GitHub are projections.
8. Every GitHub write is Core-only, authenticated, bounded and redacted.
9. `DONE` is terminal for one link; a reopened issue needs a new request.
10. Issue text is untrusted data, never instruction or authority.

## Consequences

- Delivery becomes traceable from request to issue to PR to merge to closure.
- Models can still read/summarize issue context, but cannot manufacture a link
  after the fact.
- Partial and multi-issue work is explicit and does not produce false closures.
- The implementation needs issue-link persistence, lifecycle projections,
  PR-comment/close receipts and GitHub write permissions.
- A stale or externally changed issue requires an owner/attention state rather
  than a best-effort relink.
- The existing read-only issue integration remains a valid snapshot source; the
  governed intake path must be extended rather than replaced.

## Alternatives considered

1. **Let the model choose the issue at delivery.** Rejected: post-hoc choice
   creates false traceability and cannot be validated against the frozen scope.
2. **Use GitHub closing keywords only.** Rejected as the authority: closure can
   happen outside Core, before all checks, or for a partial scope; Core could not
   prove or idempotently record it.
3. **Close on PR open or Gate acceptance.** Rejected: the change is not merged
   and may still be rejected or reverted.
4. **Close every linked issue on merge.** Rejected: partial/multi-issue work
   would falsely close remaining scope.
5. **Make issue links mutable during work.** Rejected: the frozen request/spec
   and evidence would no longer describe what was delivered.
6. **Keep issues entirely read-only.** Rejected: it leaves no delivery
   traceability and no truthful completion state.

## Verification

- Deterministic tests for link freezing, immutable hash, model-cannot-relink,
  all state transitions, invalid transitions, partial/REFERENCES non-closure,
  multiple issues, premature/external close, already-closed idempotency, PR
  comment idempotency, merge identity and post-merge-check gating.
- Fake-GitHub tests for 401/403/404/429/redirect refusal and bounded retry; no
  token or issue body leak in errors/comments/evidence.
- Database tests for append-only link/lifecycle records, unique close/comment
  receipts and restart recovery.
- Office per-option Playwright acceptance for link creation (only at request
  creation), immutable link display, all six states, partial state, blocked/
  attention reasons, PR/merge/close evidence, loading/empty/error/offline,
  keyboard and screen-reader labels at desktop, tablet and 320 px in EN and ES.
- A real authorized GitHub comment/close run remains `NOT_TESTED` until an
  exact candidate, credential and owner authorization exist.

## Supersession

None. A later ADR that changes link immutability, closure authority or issue
write permissions must supersede this ADR explicitly.
