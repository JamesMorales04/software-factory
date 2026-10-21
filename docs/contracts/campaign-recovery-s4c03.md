# Contract: Bounded autonomous Implementer recovery (F1-S4-C-03)

**Status:** Implemented; final candidate acceptance is recorded in PR #12.
**Version:** `1`. Experimental pre-release contract; breaking changes update all
in-repo callers, fixtures, docs and tests atomically (ADR-0006). TRUSTED_LOCAL
only; this is not isolation certification.

This contract extends the S4-B campaign graph and the S4-C capability model. It
does not create a second Run/Attempt lifecycle, a second scheduler or a
model-authority path.

## 1. Campaign attempt budget

`POST /api/v1/campaigns` accepts optional `maxImplementerAttempts` (1..24).
When omitted, Core uses `spec.budget.maxAttempts × (maxRepairCycles + 1)`.
The value must be at least the spec attempt budget. It counts every
Implementer Run for the campaign's frozen spec, including failed recovery Runs,
not only the latest Run bound to the task.

Unknown provider token/quota/cost remains `unknown`. The budget is a hard
execution bound, not a cost estimate.

## 2. Failure classes

| Class | Automatic action |
| --- | --- |
| `RECOVERABLE_IMPLEMENTATION_FAILURE` | Bounded new Run under the same Campaign/Task |
| `RECOVERABLE_EXECUTION_FAILURE` | Bounded new Run only with confirmed cleanup/fencing |
| `QUALITY_REPAIR_REQUIRED` | Existing new repair cycle via the Quality Gate |
| `ORACLE_DEFECT_SUSPECTED` | `HUMAN_REQUIRED`; new frozen spec required, old evidence immutable |
| `POLICY_OR_SECURITY_BLOCKED` | `BLOCKED`; never retried |
| `UNRESOLVED_EXTERNAL_RECOVERY` | `BLOCKED`; never retried |
| `INFRASTRUCTURE_BLOCKED` | `BLOCKED`; repair infrastructure before another session |

Classification is deterministic and uses the observed Run error/result,
attempt budget, S3-D recovery state and cleanup status. It never calls a model.

## 3. Recovery identity and state

On an eligible failure:

1. The failed Run, Attempt, Paseo binding/session, error and event remain
   immutable.
2. Core inserts a `campaign_recoveries` row with campaign/task, attempt index,
   failure class, bounded reason, previous Run/Attempt and (after dispatch)
   the new Run.
3. The Implementer task returns to `ready` with a failure-shaped error; its
   previous `RunID` is not reused.
4. The next dispatch creates a new Run and lease generation with the same
   frozen spec, Project scope and capability snapshot.
5. The normal `CandidateRevision → Verifier → Reviewer → Quality Gate`
   sequence resumes.

A stale dispatch token, unresolved external recovery, unconfirmed cleanup,
missing authority or exhausted budget prevents recovery. A failed Run is
never rewritten as succeeded.

## 4. Repair instruction

The new prompt includes the original goal, frozen scope/capabilities, observed
failure class and reason, attempt index, remaining attempt budget and a
requirement to retest. It must not be a verbatim repetition of the failed
prompt.

## 5. Source artifacts and security

Known runtime artifacts (logs, browser profiles, dependency/build output) are
excluded from candidate staging. This is a practicality boundary, not a
security boundary. Any changed source path outside the authorized scope, any
protected oracle modification, forged/cross-project evidence or credential
expansion remains `POLICY_OR_SECURITY_BLOCKED` with no retry.

## 6. Oracle defects

The Implementer cannot edit the frozen oracle to turn a failure into success.
If independent assessment establishes a defective oracle:

1. record the assessment and limitations;
2. freeze a new ActionSpec revision (new immutable row with a new content
   hash; the Project binding hash may remain unchanged because it covers the
   Project/scope/base/profile/allowed-path binding, not acceptance criteria);
3. run a new Campaign against the new spec;
4. preserve the old candidate, verification, reviewer verdict and gate
   decision unchanged.

Ambiguous cases terminate `HUMAN_REQUIRED`. Protected policy changes require
the applicable approval.

## 7. Projection and events

Campaign projections expose `recoveries` and
`budget.recoveryAttempts`. `CampaignImplementerRecoveryAuthorized` is recorded
on the campaign aggregate with failure class, reason, previous Run/Attempt,
recovery index and attempt usage. Office renders the recovery history and the
previous/new Run links.

## 8. Bounds and non-goals

No unlimited retry, paid-model fallback, delivery/merge authority, remote
Workers, general MCP orchestration, oracle self-editing or SEC-01 isolation.
Live certification budgets are operator parameters; they are not a permanent
hidden Factory policy.
