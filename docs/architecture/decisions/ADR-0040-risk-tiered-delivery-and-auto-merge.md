# ADR-0040 — Risk-tiered delivery and auto-merge

**Status:** Proposed — 2026-11-10. This ADR records the Product Owner's
program constraints for implementation. It is not accepted and grants no
implementation, merge or delivery authority until the protected approval path
records acceptance and independent review. Delivery-policy changes are protected
changes under [ADR-0038](ADR-0038-owner-charter-and-delegated-autonomy.md).

**Supersedes:** none.

**Extends:** [ADR-0010](ADR-0010-frozen-candidate-quality-pipeline.md),
[ADR-0033](ADR-0033-provider-agnostic-review-and-bounded-autonomy.md),
[ADR-0036](ADR-0036-owner-granted-autonomous-r0-intake.md),
[ADR-0038](ADR-0038-owner-charter-and-delegated-autonomy.md) and
[ADR-0039](ADR-0039-quality-first-planning-and-stack-selection.md).

**Contract:** [delivery-policy](../../contracts/delivery-policy.md).

**Security status:** `TRUSTED_LOCAL`. A merge is a repository write, not a
deployment, isolation certification or production release.

## Context

The Quality Gate already decides whether an exact candidate has the required
evidence. The current merge endpoint additionally requires an authenticated
operator, explicit confirmation, an exact expected head SHA and a fresh GitHub
pull-request and check read. That is a safe manual baseline, but it has four
gaps for the next program:

1. **Gate acceptance is being treated as if it implied delivery.** A candidate
   can be `ACCEPTED` and still be unsafe to merge because its head changed, its
   checks are stale, branch protection is not satisfied, a post-merge condition
   is missing, an incident is open, or the project has not authorized delivery.
   Quality evidence and repository-write authority are different decisions.
2. **There is no risk-tiered policy.** Every merge needs the same manual click,
   which makes R0 toil-heavy, while R1 has no bounded path that combines project
   opt-in, an Owner Charter grant and a post-merge check.
3. **There is no durable delivery record or idempotency contract.** A retry can
   re-read GitHub, but the intent, policy revision, expected head, checks,
   merge result and incident are not a first-class Core record. Duplicate
   requests and webhook replays therefore rely on ad hoc reconciliation.
4. **There is no incident or revert boundary.** A merge that later fails its
   post-merge check has no Core-owned incident, no automatic freeze of further
   auto-merges and no recorded manual revert path.

The owner wants routine R0 delivery to proceed without a click, R1 to proceed
only where the project and charter explicitly allow it, and R2/R3 to remain
manual. The model may plan, implement and review; it must never choose a
delivery target or perform a repository write.

## Decision

### 1. Delivery authority is separate from Gate acceptance

`GateOutcome::Accepted` means the deterministic Quality Gate accepted the exact
candidate's quality evidence. It does **not** authorize a merge, a push, a tag,
a release or a deployment.

Every repository write requires a separate Core `DeliveryDecision` bound to:

- the exact candidate identity (`candidateId`, commit SHA, tree SHA and content
  digest);
- the exact pull request number, base branch and expected head SHA;
- the exact Gate decision revision and outcome;
- the exact `DeliveryPolicy` revision and, for R1, the exact Owner Charter
  `delivery_autonomy` grant;
- the freshness and identity of the pull-request, branch-protection and check
  reads;
- the risk class revalidated by Core from the candidate diff, not a model label.

A model, reviewer, verifier, Worker, Office client or external webhook never
grants delivery authority. Only Core can perform the final write with its
Core-held credential.

### 2. Project delivery policy

A Project has a versioned, append-only delivery policy. The policy is disabled
by default (`MANUAL`). A Project must explicitly opt in to risk-tiered delivery;
a policy revision is an immutable audit record and a frozen candidate keeps the
policy revision it was evaluated under.

`RISK_TIERED` authorizes:

| Risk | Delivery behavior |
| --- | --- |
| `R0` | Automatic merge on the protected default branch after all preconditions pass. No per-merge owner click. |
| `R1` | Automatic merge on the protected default branch only when **all** of the following hold: the project policy explicitly enables R1 (`r1_enabled=true`), a current Owner Charter `delivery_autonomy` grant covers this project/repository/default branch and R1, and a required post-merge check is configured. The post-merge check must pass and be recorded before delivery is `DONE`. |
| `R2` | Manual merge only. An authenticated owner decision is required for every merge. |
| `R3` | Manual merge only. An authenticated owner decision and the applicable independent security review are required for every merge. |
| Unknown / unclassified | Manual merge only. Unknown risk is never auto-merged. |

`MANUAL` mode disables auto-merge for every risk class. A kill switch or an open
delivery incident freezes auto-merge for the Project even when the policy is
`RISK_TIERED`.

### 3. Exact candidate and head

Auto-merge is eligible only when:

- the candidate is the latest frozen candidate for its Attempt and has not been
  superseded or rejected;
- the pull request is open, non-draft, and targets the Project's protected
  **default branch**. Auto-merge never targets a non-default branch; a
  configured non-default protected base remains manual-only until a later ADR
  explicitly changes this rule (manual merges may use such a base);
- the pull request head SHA equals the candidate commit SHA exactly;
- the candidate tree SHA and content digest still resolve to that commit;
- the pull request base SHA is the current protected base, or the merge is
  rejected as stale;
- `expected_head_sha` is sent to GitHub and equals the candidate commit, so the
  merge cannot silently apply to a later push.

A new commit, force-push, rebase, base update or branch-protection change
invalidates the decision. The exact candidate SHA must be re-read; a decision
never follows a moving branch.

A candidate finalized by the Worker salvage path follows the same exact-candidate
and Gate rules, but has an additional delivery restriction. Its
`candidate_final_source` record (reserved migration `0075`) must expose the exact
final source and salvage evidence. R0 salvage is eligible for the ordinary Gate
and ordinary R0 delivery policy. R1/R2/R3 salvage requires an authenticated human
review of the final-source and salvage evidence and is **never auto-merged**,
even if the Gate accepts it.

### 4. Fresh pull request and check read

The PR, branch-protection and check state are read in the same authenticated
Core delivery operation, immediately before the merge. A cached read, a webhook
payload or a read older than 120 seconds is not sufficient. The checks must:

- be bound to the exact pull-request head SHA;
- be complete and successful for every required check;
- reflect the current branch-protection/ruleset requirements;
- not be a substitute for the Core Quality Gate.

If the read cannot prove branch protection, required checks or the exact head,
Core fails closed with `BRANCH_PROTECTION_UNVERIFIED`, `CHECKS_STALE` or
`HEAD_MISMATCH`; it does not infer success from the absence of a failing check.

### 5. Branch protection remains authoritative

Factory never disables, bypasses or rewrites branch protection, rulesets,
required reviewers or required status checks. GitHub's server-side enforcement
is part of the authorization. Core requires the protected branch to reject
force-push and deletion and to require the configured checks/reviews. If the
repository cannot prove that configuration through its supported API, auto-merge
is blocked; manual merge may still be performed by an owner after the same
fresh read, but Factory makes no bypass claim.

### 6. Idempotency and reconciliation

Each delivery intent is a durable `DeliveryAttempt` with an idempotency key over
`(candidateId, policyRevision, expectedHeadSha, operation)`. A repeated request:

- returns the existing attempt and its recorded outcome;
- does not create a second merge, comment or incident;
- if GitHub already merged the PR, reconciles the merge commit and marks the
  attempt `MERGED` instead of failing blindly;
- if GitHub reports a different merged commit or head, records an incident and
  never retries the write automatically.

External effects are reconciled before retry. Duplicate webhooks, duplicate
Office submissions and Core restart/retry are safe.

### 7. Post-merge check, incident and manual revert

R1 auto-merge requires a configured post-merge check that runs against the merge
commit or the updated default branch. Its result is durable delivery evidence.
R0 may have an optional post-merge observation, but it is not a substitute for
the Gate.

A failed post-merge check, a branch-protection drift, an unexpected merge
commit, a failed check after merge or an owner-reported regression opens a
`DeliveryIncident`, freezes further auto-merge for the Project, notifies the
owner and leaves the delivery `NEEDS_ATTENTION`. Factory does **not** auto-revert.
The owner decides whether to revert; a revert is a normal reviewed change (or a
Core-opened revert PR) that is never auto-merged, never force-pushed and never
bypasses branch protection. The incident remains linked to the merge commit and
the revert commit.

### 8. Core-only write credential

The GitHub write credential is held only by Core, encrypted at rest, revocable
and scoped to the minimum required repository permissions. It is never returned
to Office, written to a cookie/local storage, passed to a Worker, provider,
model, skill or tool, or included in logs, evidence, prompts or error bodies.
Office and all external callers invoke authenticated Core operations; they never
receive a git remote credential or a GitHub token. Credential rotation or
revocation fences in-flight delivery attempts before another write.

### 9. What this does not authorize

This ADR does not authorize a deployment, release, tag, package publication,
production change, force-push, branch-protection change, credential change,
data migration or cross-project delivery. It does not make R2/R3 automatic and
does not make an R1 grant valid outside its exact project, repository, branch,
risk class, expiry and revision. It does not make a container an isolation
boundary. It does not certify live GitHub behavior until a real authorized run
is recorded against the exact candidate.

## Invariants

1. Gate `ACCEPTED` is necessary, never sufficient, for delivery.
2. No model output, review, check, webhook or Office action grants merge
   authority.
3. Every merge is bound to one exact candidate, one exact expected head and one
   exact policy/charter revision.
4. Auto-merge is fail-closed: missing, stale or unverifiable evidence blocks it.
5. R0 auto-merge requires an enabled `RISK_TIERED` project policy; R1 adds an
   explicit R1 opt-in and a matching, current Owner Charter grant.
6. R2 and R3 are always manual.
7. Branch protection is never bypassed.
8. Every write has an idempotency key and every retry reconciles external state.
9. Every post-merge failure creates an incident and freezes auto-merge until the
   owner resolves it.
10. Reverts are manual, reviewed and never auto-merged.
11. The Core write token never leaves Core.
12. Delivery records and evidence are immutable and append-only; corrections
    append a new attempt or incident revision.
13. Auto-merge uses only the Project's protected default branch. A non-default
    protected base is manual-only, and R1+ salvage is always manual.

## Failure and stop conditions

The contract defines the full failure-code table. The stop conditions for this
ADR are:

- no authenticated project delivery policy or no exact policy revision;
- policy disabled, R1 not enabled, charter missing/expired/revoked/out of scope;
- Gate not `ACCEPTED` or a newer Gate revision exists;
- candidate superseded, rejected or not the latest candidate;
- PR closed, draft, already merged, wrong base or wrong head;
- auto-merge requested for a non-default protected base;
- checks missing, failing, stale or not bound to the exact head;
- branch protection/ruleset unverifiable;
- policy/read older than the freshness bound;
- an open incident or kill switch;
- post-merge check missing for R1;
- credential unavailable or revoked;
- any model attempt to supply a merge target, token or authorization.

## Consequences

- R0 becomes routine and auditable instead of click-heavy.
- R1 can be delegated narrowly without turning every merge into an owner task.
- R2/R3 retain the strongest human control.
- Delivery now has durable attempts, idempotency, incidents and reconcileable
  external effects rather than a single handler response.
- A new set of tables, endpoints, Office states and tests is required; the
  Quality Gate itself does not change.
- Branch protection and GitHub availability become explicit delivery
  dependencies. Unverifiable protection means no auto-merge.
- Manual revert is slower than automatic revert, but it keeps a human in control
  of a repository write that may need judgment.

## Alternatives considered

1. **Treat Gate `ACCEPTED` as delivery authority.** Rejected: quality evidence
   does not prove branch protection, exact head, project opt-in or risk policy.
2. **Use GitHub native auto-merge as the authority.** Rejected as the authority;
   it can be a transport detail only if Core still performs the same exact-head,
   fresh-read and policy checks. Factory must not delegate its delivery decision
   to a setting that can outlive the candidate.
3. **Auto-merge every risk class.** Rejected: R2/R3 can be irreversible,
   security-sensitive or public-contract changes.
4. **Let the model or reviewer merge.** Rejected: model output never grants
   authority and the write credential must remain Core-only.
5. **Auto-revert on post-merge failure.** Deferred/rejected for this ADR: a
   revert is a judgment-bearing write; the first implementation records the
   incident, freezes auto-merge and requires a manual decision.
6. **Keep the current manual endpoint only.** Rejected: it leaves R0 toil and
   does not provide durable delivery records, idempotency or incidents.

## Verification

- Deterministic fake-GitHub tests for every risk tier, disabled policy, missing
  charter, expired/revoked grant, stale head, changed base, draft/closed PR,
  failing/incomplete/stale checks, unverified branch protection, non-default
  protected base (`BASE_NOT_DEFAULT`), duplicate idempotency key, retry after
  merge, merge-commit mismatch, post-merge failure, incident freeze and manual
  revert record.
- Salvage tests: R0 final-source visible and eligible for the ordinary Gate;
  R1+ final-source requires authenticated human review and is never auto-merged,
  even after Gate acceptance.
- Database tests for append-only policy revisions, immutable delivery attempts,
  unique idempotency keys, incident state and candidate/policy/gate binding.
- Core API tests proving no token, model or Office path can merge without the
  exact decision; negative tests for malformed or replayed decisions.
- Office per-option Playwright acceptance for eligibility, blocked reasons,
  manual approval, R0/R1/R2/R3 states, incident, revert, loading/empty/error/
  offline, keyboard and screen-reader labels at desktop, tablet and 320 px in
  EN and ES. A screenshot alone is not acceptance.
- A real authorized GitHub run remains `NOT_TESTED` until an exact candidate and
  credential are approved; the deterministic path does not certify live GitHub
  behavior.

## Supersession

None. If a later ADR changes delivery tiers or the write-credential boundary, it
must supersede this ADR explicitly and preserve this record.
