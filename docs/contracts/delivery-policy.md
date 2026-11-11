# Contract: risk-tiered delivery and auto-merge

**Status:** Proposed WP0 contract — 2026-11-10. This freezes the interface for
delivery work packages; it is not implementation or acceptance evidence.
**Version:** `1`. Experimental pre-release contract; breaking changes update all
in-repo callers, fixtures, docs and tests atomically
([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

**Decision:** [ADR-0040](../architecture/decisions/ADR-0040-risk-tiered-delivery-and-auto-merge.md).
**Related:** [ADR-0038](../architecture/decisions/ADR-0038-owner-charter-and-delegated-autonomy.md),
[ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md),
[quality pipeline](quality-pipeline-s4a.md).
**Security:** `TRUSTED_LOCAL`; a merge is a repository write, not a deployment,
isolation certification or release.

## 1. Scope and authority boundary

The Quality Gate answers "does this exact candidate have the required quality
evidence?" This contract answers "may Core write this exact candidate to this
repository now?"

`GateOutcome::Accepted` is necessary but **never sufficient** for delivery.
Every repository write requires a separate Core `DeliveryDecision`. No model,
reviewer, verifier, Worker, Office client, webhook or external skill can grant
delivery authority.

## 2. Identities and durable records

| Entity | Identity | Notes |
| --- | --- | --- |
| `DeliveryPolicy` | `(projectId, revision)` + `policySha256` | Append-only; disabled by default |
| `DeliveryAttempt` | random 32-hex `attemptId` + idempotency key | One intent to merge one exact candidate/head |
| `DeliveryDecision` | `(candidateId, gateRevision, policyRevision, expectedHeadSha)` | Authorization record; not a model output |
| `DeliveryIncident` | random 32-hex `incidentId` | Post-merge failure/drift; freezes auto-merge |
| `PostMergeCheck` | `(attemptId, checkId, mergeCommitSha)` | Required for R1; exact check receipt |
| `RevertRecord` | `(incidentId, revertPrNumber, revertMergeSha)` | Manual revert evidence |

### 2.1 DeliveryPolicy

```json
{
  "schemaVersion": 1,
  "projectId": "…",
  "revision": 3,
  "policySha256": "…",
  "mode": "MANUAL | RISK_TIERED",
  "r1Enabled": false,
  "autoMergeBaseBranch": "main",
  "manualBaseBranches": ["main", "release"],
  "requiredChecks": ["…"],
  "postMergeCheck": {
    "requiredForR1": true,
    "checkId": "default-branch-smoke",
    "timeoutSecs": 900
  },
  "killSwitch": false,
  "incidentFreeze": false,
  "charterGrantId": null,
  "createdBy": "owner-principal",
  "createdAt": "RFC3339",
  "expiresAt": null
}
```

Rules:

- The default is `MANUAL`; a missing policy is `DELIVERY_POLICY_REQUIRED`.
- `RISK_TIERED` enables R0 auto-merge. `r1Enabled=true` is a second explicit
  opt-in and requires a matching Owner Charter `delivery_autonomy` grant covering
  this project/repository/`autoMergeBaseBranch` and R1.
- `autoMergeBaseBranch` MUST equal the Project's protected default branch.
  Auto-merge never uses any other branch.
- `manualBaseBranches` may list the default and additional protected branches
  for **manual** delivery only. A non-default protected base remains manual-only
  until a later ADR explicitly changes this rule.
- `postMergeCheck.requiredForR1` is `true`; disabling it makes R1 ineligible.
- `killSwitch` or `incidentFreeze` blocks all auto-merge for the Project.
- Policy revisions are immutable; an update appends a new revision. A frozen
  candidate/attempt keeps the revision it was evaluated under.
- The policy cannot enable R2/R3 auto-merge, auto-merge on a non-default branch,
  a branch-protection bypass, a force-push, a deployment or a cross-project
  write.

### 2.2 DeliveryAttempt

```json
{
  "schemaVersion": 1,
  "attemptId": "…",
  "idempotencyKey": "…",
  "projectId": "…",
  "campaignId": "…",
  "candidateId": "…",
  "candidateSha": "…",
  "candidateTreeSha": "…",
  "contentDigest": "…",
  "gateRevision": "…",
  "gateOutcome": "ACCEPTED",
  "policyRevision": 3,
  "policySha256": "…",
  "riskClass": "R0 | R1 | R2 | R3",
  "authorization": "POLICY_R0 | CHARTER_R1 | OWNER_MANUAL",
  "charterGrantId": null,
  "pullRequestNumber": 42,
  "baseBranch": "main",
  "expectedHeadSha": "…",
  "mergeMethod": "merge | squash | rebase",
  "state": "PENDING | AUTHORIZED | MERGING | MERGED | BLOCKED | NEEDS_ATTENTION | REVERTED",
  "mergeCommitSha": null,
  "incidentId": null,
  "postMergeCheckId": null,
  "createdAt": "RFC3339",
  "updatedAt": "RFC3339"
}
```

`expectedHeadSha` must equal `candidateSha`. For auto-merge, `baseBranch` must
equal `autoMergeBaseBranch` and the Project's protected default branch; for
manual delivery it must be in `manualBaseBranches` and protected. `authorization`
records why Core believed the write was allowed; it is validated against the live
policy/charter, not trusted as a caller input.

### 2.3 DeliveryDecision

```json
{
  "candidateId": "…",
  "candidateSha": "…",
  "gateRevision": "…",
  "gateOutcome": "ACCEPTED",
  "policyRevision": 3,
  "policySha256": "…",
  "charterGrantId": null,
  "riskClass": "R0 | R1 | R2 | R3",
  "expectedHeadSha": "…",
  "baseBranch": "main",
  "checksReadAt": "RFC3339",
  "branchProtectionReadAt": "RFC3339",
  "decision": "ALLOW | DENY",
  "reasons": ["…"],
  "decidedAt": "RFC3339"
}
```

A `DeliveryDecision` is an authorization snapshot. A later head, base, check,
policy, charter or Gate revision invalidates it.

## 3. Eligibility matrix

| Risk | `MANUAL` | `RISK_TIERED` |
| --- | --- | --- |
| `R0` | Owner confirmation | Auto-merge on the protected default branch after all preconditions |
| `R1` | Owner confirmation | Auto-merge on the protected default branch only with `r1Enabled=true`, a current matching Owner Charter `delivery_autonomy` grant and a configured post-merge check |
| `R2` | Owner confirmation | Owner confirmation (never auto) |
| `R3` | Owner confirmation + applicable independent security review | Owner confirmation + applicable independent security review (never auto) |
| Unknown | Owner confirmation | Owner confirmation (never auto) |

Auto-merge rows use only `autoMergeBaseBranch` (the protected default branch).
A configured non-default protected base in `manualBaseBranches` does not make
that base auto-mergeable.

Risk class is revalidated by Core from the actual candidate diff. A model label,
a historical Gate label or an Office selection cannot downgrade risk. If the
risk changes after the Gate, a new Gate decision is required.

## 4. Preconditions

Auto-merge is allowed only when every condition holds:

1. The Project has an enabled `RISK_TIERED` policy and the exact revision is
   current.
2. The candidate is the latest frozen candidate for its Attempt, not superseded,
   rejected or from a failed Attempt.
3. The Core Gate decision is `ACCEPTED` and is the current effective revision.
4. The candidate commit/tree/content digest resolve exactly.
5. The PR is open, non-draft and targets the Project's protected default branch
   (`autoMergeBaseBranch`). Auto-merge on any other base is `BASE_NOT_DEFAULT`;
   a configured non-default protected base is manual-only.
6. The PR head SHA equals `expectedHeadSha` equals the candidate commit SHA.
7. The PR base SHA is the current protected base (or GitHub's merge would be
   stale; reject).
8. Branch protection/rulesets require the configured checks/reviews and reject
   force-push/deletion; the proof was read in this operation.
9. Required checks are complete, successful and bound to the exact head SHA.
10. The PR/check/protection reads are no older than 120 seconds at the merge
    call; otherwise re-read.
11. No delivery incident or kill switch is active.
12. For R1: `r1Enabled=true`, a current matching charter grant for this
    project/repository/`autoMergeBaseBranch` and R1 exists, and the post-merge
    check is configured.
13. For R2/R3: an authenticated owner decision exists for this exact candidate,
    head and base (and security review for R3).
14. The Core-only write credential is present, unexpired and not revoked.

### 4.1 Salvage-finalized candidates

A candidate finalized by the Worker salvage path is not a separate quality or
delivery path. It must have an immutable `candidate_final_source` record
(reserved migration `0075`) containing:

- the exact final candidate commit/tree/content digest;
- the frozen `EnvironmentSpec` hash;
- the salvage checkpoint, frozen-check receipts and the normal
  verifier/reviewer/Gate lineage;
- the exact source used to produce the candidate, so a later workspace change
  cannot be mistaken for the salvaged source.

Rules:

- **R0 salvage** is visible through `candidate_final_source` and eligible for the
  ordinary verifier/reviewer/Gate and the ordinary R0 delivery policy. It gets no
  special acceptance claim.
- **R1/R2/R3 salvage** requires an authenticated human review of the final-source
  and salvage evidence, even if the Gate later accepts it. It is **never
  auto-merged**; delivery is manual only. A model cannot self-approve.
- Missing final-source or salvage evidence is `FINAL_SOURCE_REQUIRED` or
  `SALVAGE_REVIEW_REQUIRED`; it cannot be treated as `ACCEPTED`, `MERGED` or
  `DONE`.

## 5. Merge protocol

1. Core re-reads the candidate, Gate, policy, charter, PR, base branch (which
   must equal `autoMergeBaseBranch` for auto-merge), branch
   protection/rulesets and checks. No cached state is used for the write.
2. Core builds a `DeliveryDecision`. Any `DENY` returns a stable failure code and
   performs no write.
3. Core creates or resumes one `DeliveryAttempt` by idempotency key.
4. Core calls GitHub's merge API with `expected_head_sha` (or the provider's
   equivalent conditional write). It never force-pushes and never disables
   protection.
5. On success, Core records the merge commit, state `MERGED`, and the exact
   observed head/base. It does not infer success from an HTTP 200 alone; it
   confirms the merged state or reconciles it.
6. For R1, Core runs the configured post-merge check against the merge commit or
   updated default branch and records the receipt.
7. If the post-merge check fails or the merge commit/head is unexpected, Core
   opens an incident, sets `NEEDS_ATTENTION`, freezes auto-merge for the Project
   and notifies the owner. It does not auto-revert.
8. Core closes eligible issue links only after the merge/check evidence, per the
   [issue lifecycle](issue-lifecycle.md).

## 6. Idempotency and reconciliation

- `idempotencyKey = SHA256(projectId, candidateId, policyRevision,
  expectedHeadSha, operation)`.
- A duplicate request returns the existing attempt and outcome; it does not
  create a second merge, comment, incident or issue close.
- If GitHub reports the PR already merged with the expected commit, Core
  reconciles and marks `MERGED`; this is idempotent success.
- If GitHub reports a different merged commit/head, Core records
  `MERGE_IDENTITY_MISMATCH`, opens an incident and does not retry the write.
- Retry after a transient failure re-reads GitHub/policy/charter/checks before
  another write. A retry never reuses a stale `DeliveryDecision`.
- Core restart resumes from durable attempt state; an attempt in `MERGING` is
  reconciled before any new write.

## 7. Post-merge check, incident and manual revert

- R1 requires a configured, bounded post-merge check. A missing/unavailable
  check is `POST_MERGE_CHECK_MISSING` and makes R1 ineligible before merge.
- A failing post-merge check is `POST_MERGE_CHECK_FAILED`; it opens a
  `DeliveryIncident` and freezes further auto-merge.
- Branch-protection drift, unexpected merge identity, a regression reported by
  the owner or a failed default-branch check also opens an incident.
- The incident state is `OPEN | ACKNOWLEDGED | REVERT_PENDING | REVERTED |
  CLOSED`. It remains linked to the merge commit and every revert record.
- Revert is a manual, reviewed change. Core may open a revert PR only after an
  authenticated owner decision; that PR is never auto-merged, never force-pushed
  and never bypasses protection. A revert commit is recorded in `RevertRecord`.
- Auto-merge remains frozen for the Project until the owner resolves the
  incident and records a new policy revision or explicit unfreeze decision.

## 8. Core-only write credential

- The GitHub write credential is encrypted at rest and held only by the Core
  process. It is never returned to Office, a cookie/local storage, a Worker,
  provider, model, skill, tool, log, prompt or evidence artifact.
- The credential uses the minimum repository permissions required for PR merge,
  issue comment and issue close. It does not include organization administration,
  branch-protection bypass or credential management.
- Office and other callers invoke authenticated Core operations; they never
  receive a git remote URL with credentials or a GitHub token.
- Rotation/revocation fences in-flight attempts before the next write. A
  revoked/absent credential is `DELIVERY_CREDENTIAL_UNAVAILABLE`; no fallback to
  a developer token, `gh` CLI or host git credential.
- The write token is separate from the read/sync credential where the provider
  permits; a read token never gains write scope implicitly.

## 9. Invariants

1. Gate `ACCEPTED` is necessary, never sufficient, for delivery.
2. No model, reviewer, Worker, Office client, webhook or external skill grants
   merge authority.
3. Every merge is bound to one exact candidate, expected head, base, Gate
   revision, policy revision and (for R1) charter grant.
4. Auto-merge is fail-closed: missing, stale or unverifiable evidence blocks it.
5. R0 auto-merge requires an enabled `RISK_TIERED` policy; R1 adds `r1Enabled`,
   a matching current charter grant and a post-merge check.
6. R2 and R3 are always manual; unknown risk is manual.
7. Branch protection is never bypassed, disabled or rewritten.
8. Every write has an idempotency key and every retry reconciles external state.
9. A post-merge failure creates an incident and freezes auto-merge; revert is
   manual, reviewed and never auto-merged.
10. The Core write token never leaves Core and never enters Office, a Worker,
    model, prompt, log or evidence.
11. Delivery policies, attempts, decisions, incidents and revert records are
    append-only/immutable; corrections append a revision.
12. Auto-merge uses only the protected default branch
    (`autoMergeBaseBranch`); a non-default protected base is manual-only.
13. R0 salvage is eligible for the ordinary Gate/policy; R1+ salvage requires
    human review and is never auto-merged.

## 10. Failure codes

| Code | Meaning |
| --- | --- |
| `DELIVERY_POLICY_REQUIRED` | No project delivery policy |
| `DELIVERY_POLICY_DISABLED` | `MANUAL` mode and no owner merge decision |
| `DELIVERY_POLICY_STALE` | Attempt/decision used a non-current policy revision |
| `GATE_NOT_ACCEPTED` | Gate missing, not `ACCEPTED` or superseded |
| `RISK_NOT_ELIGIBLE` | Risk tier is not auto-mergeable under the policy |
| `R1_OPT_IN_REQUIRED` | R1 attempted without `r1Enabled=true` |
| `CHARTER_GRANT_REQUIRED` | R1 has no current matching `delivery_autonomy` grant |
| `CHARTER_GRANT_EXPIRED` | Grant expired/revoked/out of scope |
| `CANDIDATE_SUPERSEDED` | Candidate is not current/latest |
| `HEAD_MISMATCH` | PR head differs from candidate/expected head |
| `BASE_MISMATCH` | PR base is not a permitted protected base |
| `BASE_NOT_DEFAULT` | Auto-merge requested for a non-default protected base; manual only |
| `PR_NOT_OPEN` | PR closed, missing or not open |
| `PR_DRAFT` | PR is a draft |
| `PR_ALREADY_MERGED` | Already merged; reconcile idempotently |
| `CHECKS_NOT_READY` | Required checks pending/incomplete |
| `CHECKS_FAILED` | Required check failed |
| `CHECKS_STALE` | Checks not bound to the exact head or read is old |
| `BRANCH_PROTECTION_UNVERIFIED` | Protection/ruleset cannot be proven |
| `MERGE_BLOCKED` | GitHub refuses the merge (conflict/protection/queue) |
| `MERGE_IDENTITY_MISMATCH` | Merged commit/head differs from the decision |
| `POST_MERGE_CHECK_MISSING` | R1 has no configured/available check |
| `POST_MERGE_CHECK_FAILED` | Required check failed after merge |
| `FINAL_SOURCE_REQUIRED` | Salvage-finalized candidate lacks the exact final-source record |
| `SALVAGE_REVIEW_REQUIRED` | R1+ salvage lacks authenticated human review; never auto-merge |
| `DELIVERY_INCIDENT_OPEN` | Incident/kill switch freezes auto-merge |
| `DELIVERY_CREDENTIAL_UNAVAILABLE` | Core write credential absent/revoked |
| `IDEMPOTENCY_CONFLICT` | Same key with different exact inputs |
| `GITHUB_UNAVAILABLE` | Upstream unavailable/timeout; no success claim |
| `OWNER_APPROVAL_REQUIRED` | R2/R3 or reserved matter needs owner decision |

## 11. Evidence

A delivery retains:

1. the exact candidate, tree, content digest and Gate revision;
2. policy revision/hash and the matching charter grant (if R1);
3. the `DeliveryDecision` with check/protection read timestamps;
4. PR number, base, expected head and observed head/base;
5. required check results bound to the exact head;
6. branch-protection/ruleset evidence (provider response summarized without
   secrets);
7. merge method, merge commit SHA and merged-at time;
8. post-merge check receipt (R1);
9. incident/revert records and owner decisions;
10. issue link/comment/close receipts per the issue contract;
11. for a salvage-finalized candidate, the `candidate_final_source` record and
    authenticated human-review decision (R1+).

Evidence is immutable and exact-candidate-bound. A webhook or UI claim is not
evidence of a merge; only the provider-confirmed merge state and Core attempt
record are.

## 12. Office impact and accessibility

The implementation must expose delivery on the existing Campaign/candidate
surface (no new authority is created by this contract):

- risk tier, policy revision, charter grant and eligibility/blocked reasons;
- exact candidate/head/base, protected default branch and Gate revision; a
  non-default base is visibly manual-only;
- check freshness, branch-protection status and required checks;
- R0 auto-merge state; R1 opt-in/charter/post-merge-check state; R2/R3 manual
  approval; owner decision form where required;
- salvage final-source state and the R1+ human-review requirement (never
  auto-merge);
- merge result, merge commit, incident, freeze and manual revert record;
- loading, empty, error, offline, blocked, stale, incident and merged states;
- no control that can merge without a fresh Core eligibility read; no token or
  raw provider response in the browser;
- keyboard operation, visible focus, screen-reader names, non-color state cues,
  no horizontal overflow at 320 px and EN/ES copy.

Real Chromium acceptance must cover each option/state; a screenshot or route
smoke is not acceptance. Implementation must update the Office parity matrix and
capability registry in the same delivery. WP0 does not change parity states or
capabilities.

## 13. Positive and negative acceptance cases

Positive:

- R0 `RISK_TIERED` with clean protection/checks merges exactly once and records
  the merge commit;
- R1 with project opt-in, matching charter grant and a passing post-merge check
  reaches `MERGED`/`DONE`;
- R2/R3 with an exact owner decision merges manually and records the decision;
- R0 salvage with an exact final-source record passes the ordinary Gate and
  ordinary R0 policy;
- a duplicate request returns the same attempt and does not merge twice;
- a retry after a confirmed merge reconciles as success.

Negative:

- `ACCEPTED` Gate with no policy does not merge;
- R1 without `r1Enabled` or charter grant does not merge;
- R2/R3 without owner decision does not merge;
- auto-merge on a non-default protected base is `BASE_NOT_DEFAULT`, never a
  silent auto-merge;
- R1/R2/R3 salvage without authenticated human review is
  `SALVAGE_REVIEW_REQUIRED`, never auto-merged;
- draft/closed PR, wrong base, wrong head, stale checks, failed checks, unknown
  protection or an open incident does not merge;
- a changed head after the decision invalidates it;
- a candidate/check/delivery with a different environment hash is rejected;
- a caller/model supplying a token or merge target is rejected;
- a post-merge failure opens an incident, freezes auto-merge and never
  auto-reverts.

## 14. Compatibility

This is an experimental pre-release contract. Delivery policies, attempts,
decisions and incidents are append-only/immutable; a correction appends a new
revision. Persisted delivery data changes require an explicit migration
decision. See
[ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).
