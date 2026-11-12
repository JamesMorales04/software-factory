# Contract: issue-linked delivery lifecycle

**Status:** Proposed WP0 contract — 2026-11-10. This freezes the interface for
issue-linked delivery work packages; it is not implementation or acceptance
evidence. **Version:** `1`. Experimental pre-release contract; breaking changes
update all in-repo callers, fixtures, docs and tests atomically
([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

**Decision:** [ADR-0042](../architecture/decisions/ADR-0042-issue-linked-delivery-lifecycle.md).
**Related:** [ADR-0040](../architecture/decisions/ADR-0040-risk-tiered-delivery-and-auto-merge.md),
[delivery policy](delivery-policy.md), [GitHub Issues integration](../implementation/github-issues-integration-2026-10-28.md),
[governed issue intake](../implementation/issue-intake-policy-2026-10-28.md).
**Security:** issue titles/bodies/labels/comments are untrusted data. They are
never instructions, tool calls or authority.

## 1. Scope and authority boundary

This contract defines how a request is linked to exact GitHub issues, how that
link is frozen, how the issue delivery lifecycle is projected and how Core
comments on a PR and closes issues after a merge.

It does not authorize issue creation, arbitrary issue editing, label/assignee
writes, PR creation outside Core, merge, deployment or a second issue store. The
existing read-only snapshot and default-off intake policy remain the source and
eligibility boundary; this contract adds immutable delivery links and
post-merge issue writes.

## 2. Identities and durable records

| Entity | Identity | Notes |
| --- | --- | --- |
| `IssueLink` | random 32-hex `linkId`; `(requestId, repositoryId, issueId)` unique | Immutable after request creation |
| `IssueLinkSet` | `(requestId, linkSetSha256)` | Frozen set hash included in the request/spec |
| `IssueLifecycle` | `(linkId, revision)` | Append-only state/reason/evidence record |
| `IssueCommentReceipt` | `(requestId, prNumber, candidateSha, linkSetSha256)` | Idempotent PR-open comment |
| `IssueCloseReceipt` | `(linkId, mergeCommitSha)` | Idempotent close after merge |

### 2.1 IssueLink

```json
{
  "schemaVersion": 1,
  "linkId": "…",
  "requestId": "…",
  "projectId": "…",
  "repositoryFullName": "owner/repo",
  "repositoryGithubId": 123,
  "issueNumber": 42,
  "issueGithubId": 456,
  "issueUrl": "https://github.com/owner/repo/issues/42",
  "titleSnapshot": "bounded plain text",
  "stateAtLink": "open | closed",
  "updatedAtAtLink": "RFC3339",
  "contentDigest": "sha256-hex",
  "linkMode": "CLOSES | REFERENCES",
  "completionScope": "FULL | PARTIAL",
  "source": "OPERATOR | ISSUE_INTAKE | POLICY | API",
  "linkedBy": "authenticated-principal",
  "linkedAt": "RFC3339",
  "immutable": true
}
```

Rules:

- `repositoryGithubId` and `issueGithubId` are immutable identities. A number
  alone is not sufficient across renames/transfers.
- `contentDigest` binds the bounded snapshot used at link time; an edit later is
  detected, not silently followed.
- `linkMode=CLOSES` + `completionScope=FULL` is the only closable combination.
- `REFERENCES` and `PARTIAL` links are commented but never auto-closed.
- The link set is selected at request creation by an authenticated operator, the
  deterministic issue-intake policy or another authenticated Core operation. A
  model may not choose, add, remove, replace or reclassify a link at planning,
  implementation, review, merge or delivery time.
- `linkSetSha256` is included in the frozen request/spec and in the delivery and
  issue evidence. A changed/reopened issue requires a new request/link revision;
  it does not mutate the old link.

### 2.2 IssueLifecycle

```json
{
  "schemaVersion": 1,
  "linkId": "…",
  "issueNumber": 42,
  "revision": 4,
  "state": "READY | QUEUED | IN_PROGRESS | NEEDS_ATTENTION | IN_REVIEW | DONE",
  "reasonCode": "…",
  "detail": "bounded redacted text",
  "campaignId": "…",
  "candidateId": "…",
  "candidateSha": "…",
  "gateRevision": "…",
  "pullRequestNumber": 42,
  "mergeCommitSha": null,
  "deliveryAttemptId": "…",
  "postMergeCheckId": null,
  "closeReceiptId": null,
  "updatedAt": "RFC3339"
}
```

The lifecycle is append-only. `DONE` is terminal for one link; a correction or
reopen appends a new lifecycle/request, not a mutation of the completed record.

## 3. Lifecycle states and transitions

| State | Meaning | Typical evidence |
| --- | --- | --- |
| `READY` | Issue linked and eligible; no frozen work started | Link set, eligibility |
| `QUEUED` | Request/spec frozen; work queued | Frozen spec, queue state |
| `IN_PROGRESS` | Authorized Run/Attempt is implementing/verifying | Run/Attempt state |
| `NEEDS_ATTENTION` | Owner decision, failure, stale GitHub state, merge conflict, missing evidence, failed check or incident | Stable reason code |
| `IN_REVIEW` | Exact candidate passed the Core Gate and an open PR/check review is active | Gate revision, PR, checks |
| `DONE` | Exact PR merged, required post-merge check passed and closable links closed | Merge commit, post-merge receipt, close receipt |

Allowed transitions:

```text
READY -> QUEUED -> IN_PROGRESS -> IN_REVIEW -> DONE
  |        |            |             |
  +--------+------------+-------------+--> NEEDS_ATTENTION
NEEDS_ATTENTION -> QUEUED | IN_PROGRESS | IN_REVIEW
```

Rules:

- Any state except `DONE` may move to `NEEDS_ATTENTION` with a stable reason.
- `NEEDS_ATTENTION -> DONE` is forbidden; the work must re-enter the review
  path with fresh evidence.
- `DONE` cannot move backward or transition to `NEEDS_ATTENTION`; a reopened
  issue is a new link/request.
- A state change is a Core operation derived from durable evidence. A GitHub
  webhook, comment, label, model message or Office click cannot advance a state
  by itself.
- If the repository/issue identity no longer matches, the link moves to
  `NEEDS_ATTENTION`; it is never silently relinked.

## 4. PR-open comment

When Core opens a candidate PR, it posts at most one comment for
`(requestId, prNumber, candidateSha, linkSetSha256)`:

```text
Linked issues: #42, #57
Candidate: <exact-sha>
Gate: ACCEPTED (revision <id>)
Delivery: R0 auto-merge | R1 auto-merge | manual
Scope: full | partial (issues marked partial stay open)
```

Rules:

- The comment is idempotent; a duplicate open/replay returns the existing
  receipt and does not post twice.
- It uses `Refs`/plain linked notation, **not** closing keywords (`Fixes`,
  `Closes`, `Resolves`). Core owns closure so it can enforce "only after merge"
  and record a close receipt.
- The body is bounded, plain text and free of tokens, prompts, source bodies,
  raw issue bodies and unbounded provider output.
- A failed comment is `ISSUE_COMMENT_FAILED`; it is retryable and does not
  authorize merge or close.
- The comment is a projection, not delivery evidence by itself.

## 5. Merge and close

Core may close an issue only when:

1. `linkMode=CLOSES` and `completionScope=FULL`;
2. the PR is confirmed merged into the expected protected base;
3. the merged head/merge commit is bound to the exact candidate and delivery
   attempt;
4. every required pre-merge check passed;
5. for R1/tiered delivery, the required post-merge check passed;
6. no delivery incident is open for that merge;
7. the current issue identity matches the frozen link and the issue is open.

Closure order:

```text
merge confirmed -> post-merge check (when required) -> issue close -> DONE
```

Rules:

- No close on PR open, Gate acceptance, candidate freeze or check success alone.
- Partial issues stay open. A `PARTIAL`/`REFERENCES` link, or a `CLOSES` link
  whose frozen scope/evidence does not prove full delivery, is commented and
  left open.
- Multiple links are independent. Closing one issue does not close another; each
  needs its own `CLOSES`/`FULL` and evidence.
- If GitHub already closed the issue after the merge, record `ALREADY_CLOSED`
  as idempotent success.
- If the issue was closed before the merge or by an external actor, record
  `ISSUE_CLOSED_BEFORE_MERGE`/`EXTERNAL_CLOSE` and `NEEDS_ATTENTION`; never claim
  Factory delivery.
- A failed close is retryable; it does not flip the delivery back to `MERGED`
  success silently. The lifecycle stays `NEEDS_ATTENTION` until reconciled.
- Core never reopens an issue automatically. Reopening is an owner action or a
  new linked request.

## 6. Failure codes

| Code | Meaning |
| --- | --- |
| `ISSUE_LINK_REQUIRED` | Request has no frozen issue link where one is required |
| `ISSUE_LINK_IMMUTABLE` | Attempt to add/remove/replace a link after creation |
| `ISSUE_LINK_STALE` | Content/updated-at/identity no longer matches the link |
| `ISSUE_NOT_FOUND` | Issue/repository missing or inaccessible |
| `ISSUE_REPOSITORY_MISMATCH` | Repository identity differs from the binding/link |
| `ISSUE_CLOSED` | Issue was already closed before the valid delivery |
| `ISSUE_CLOSED_BEFORE_MERGE` | External/premature close observed |
| `ISSUE_STATE_INVALID` | State does not permit the requested transition |
| `ISSUE_PARTIAL_SCOPE` | Partial/reference link cannot close |
| `ISSUE_COMMENT_FAILED` | PR comment write failed; retryable |
| `ISSUE_COMMENT_DUPLICATE` | Existing receipt; idempotent, no second comment |
| `ISSUE_CLOSE_FAILED` | Close write failed; retryable |
| `ISSUE_CLOSE_BEFORE_MERGE` | Close attempted without confirmed merge |
| `ISSUE_CLOSE_WITHOUT_CHECK` | Required post-merge check missing/failed |
| `ISSUE_IDENTITY_CONFLICT` | Duplicate number/id with a different identity |
| `ISSUE_INCIDENT_OPEN` | Delivery incident blocks closure |
| `GITHUB_UNAVAILABLE` | Upstream unavailable/timeout; no success claim |
| `ISSUE_WRITE_CREDENTIAL_UNAVAILABLE` | Core issue-write credential absent/revoked |

## 7. Idempotency and reconciliation

- `linkSetSha256` is computed over the canonical frozen link set; a duplicate
  request creation returns the same set or `ISSUE_LINK_IMMUTABLE`.
- Comment is unique on `(requestId, prNumber, candidateSha, linkSetSha256)`;
  duplicate/replay returns the existing receipt.
- Close is unique on `(linkId, mergeCommitSha)`; duplicate/replay returns the
  existing receipt or `ALREADY_CLOSED`.
- A Core restart resumes from durable lifecycle/receipt state. An in-flight
  comment/close is reconciled against GitHub before retry.
- A webhook replay cannot create a second close/comment or advance a state
  without matching Core evidence.
- If GitHub reports a different repository/issue identity, the link moves to
  `NEEDS_ATTENTION`; no retry writes to the wrong issue.

## 8. Invariants

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

## 9. Evidence

An issue delivery retains:

1. the frozen link set and hash;
2. the bounded issue snapshot/digest and identity at link time;
3. the request/spec/candidate/Gate/delivery attempt bindings;
4. the PR number, exact candidate/head and merge commit;
5. the PR-open comment receipt (provider comment ID, body hash);
6. required pre/post-merge check receipts;
7. the close receipt and final issue state;
8. any incident, external-close or partial-scope reason;
9. the authenticated principals and timestamps.

Evidence is immutable and exact-candidate-bound. A GitHub UI screenshot, a
comment body or a model statement is not close evidence; the provider-confirmed
issue state and Core receipt are.

## 10. Office impact and accessibility

The implementation must extend the existing Issues/Request views (no new
authority is created by this contract):

- issue link selection only at request creation, with explicit `CLOSES`/
  `REFERENCES` and `FULL`/`PARTIAL` scope;
- immutable link display after creation, exact issue numbers/IDs/URLs and the
  link-set hash;
- all six lifecycle states, reason codes, partial scope and external/premature
  close;
- PR-open comment receipt, merge commit, post-merge check and close receipt;
- loading, empty (no links), error, offline, stale, attention, incident and
  `DONE` states;
- no control that adds/removes a link or closes an issue outside Core; no
  closing keyword in a generated PR body;
- keyboard operation, visible focus, screen-reader names, non-color state cues,
  no horizontal overflow at 320 px and EN/ES copy.

Real Chromium acceptance must cover each option/state; a screenshot or route
smoke is not acceptance. Implementation must update the Office parity matrix and
capability registry in the same delivery. WP0 does not change parity states or
capabilities.

## 11. Positive and negative acceptance cases

Positive:

- a request created with `CLOSES`/`FULL` link #42 comments once on PR open,
  merges, passes the post-merge check and closes #42 exactly once;
- a `PARTIAL`/`REFERENCES` link is commented and remains open;
- two linked issues are closed independently;
- duplicate open/comment/close requests are idempotent;
- an already-closed-after-merge issue returns `ALREADY_CLOSED` success.

Negative:

- a model tries to add/change an issue at delivery -> `ISSUE_LINK_IMMUTABLE`;
- a request with no link where required -> `ISSUE_LINK_REQUIRED`;
- close before merge -> `ISSUE_CLOSE_BEFORE_MERGE`;
- close without the required post-merge check -> `ISSUE_CLOSE_WITHOUT_CHECK`;
- a prematurely/externally closed issue -> `NEEDS_ATTENTION`, not `DONE`;
- a repository/issue transfer or identity mismatch -> `NEEDS_ATTENTION`;
- a PR body/comment with closing keywords is rejected or does not authorize
  closure;
- a token/issue body leak in a comment/error/evidence is rejected.

## 12. Compatibility

This is an experimental pre-release contract. Issue links and lifecycle rows are
append-only/immutable; a correction appends a new revision. Persisted issue-link
data changes require an explicit migration decision. See
[ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).
