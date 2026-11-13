---
name: delivery-and-github
description: Prepare and review candidate-bound GitHub delivery: branch/PR creation, required checks, issue linkage, risk-tiered merge eligibility, R0 auto-merge boundaries, post-merge verification and rollback. Use when a change is ready to leave the workspace, when reviewing delivery evidence, or when a PR/merge decision is uncertain.
---

# Delivery and GitHub

Use this skill for delivery preparation and review. Only the Core Delivery
Engine owns merge credentials; a coding agent, reviewer or model cannot merge,
approve its own work or grant delivery authority. This skill cannot waive branch
protection, required checks or the Quality Gate.

## 1. Freeze the candidate

- Record `git rev-parse HEAD` and `git status --short --branch`.
- The worktree must be clean; no untracked test artifacts.
- The candidate must be the exact SHA that was verified and reviewed.
- A repair creates a new SHA; invalidated checks must rerun.
- Record base branch/SHA, merge base and the exact diff.

Do not open a PR from a dirty or moving worktree.

## 2. Classify risk from the final diff

| Tier | Examples | Delivery |
|---|---|---|
| R0 | bounded low-risk, no protected effect | eligible for autonomous merge only after all checks/review |
| R1 | ordinary change | policy-gated; explicit eligibility |
| R2 | security, data/schema migration, public contract | protected approval |
| R3 | authority, secrets, sandbox, Quality Gate, merge logic, live Core upgrade | strictly protected |

Reclassify from the actual diff, not the request label. A low-risk label cannot
carry a protected effect.

## 3. Required evidence

- exact candidate SHA and base;
- frozen spec/objective;
- format/lint/typecheck/build/test results;
- functional and negative checks;
- independent verifier result on a clean checkout;
- reviewer disposition;
- deterministic Quality Gate outcome;
- browser evidence when the spec requires it;
- cleanup result and any `NOT_TESTED`/`BLOCKED`;
- security/authorization impact.

A description, screenshot or model opinion is not a substitute.

## 4. GitHub lifecycle

- Create/confirm the issue link before or with the PR; do not invent a link.
- The authorized Core Delivery Engine performs branch/PR mutations. A coding
  agent prepares the exact candidate, branch name, PR body and evidence, but
  does not push, open, approve or merge unless an existing Action, delegation
  and compiled tool pack explicitly grant that effect.
- Never push to the protected base.
- Required checks must run on the exact PR head SHA.
- Branch protection and required reviews are not bypassed by an agent.
- R0 auto-merge is allowed only when policy, checks, review and branch
  protection all match the exact candidate.
- R1/R2/R3 follow their explicit gates; reserved effects stay owner-controlled.
- On merge, verify the merged commit and post-merge checks; close/comment the
  issue only with a recorded receipt.
- On failure, leave the PR open or revert; never rewrite the candidate SHA.

## 5. Rollback and recovery

- Know the previous known-good SHA/image and the restore path before merge.
- A migration needs a forward/rollback plan; destructive data changes are R2/R3.
- A failed post-merge check is an incident, not a silent retry.
- A revert is a new candidate with its own evidence; do not edit history.
- A live Core upgrade needs a canary/rollback path outside this skill.

## 6. Review checklist

- Is the PR head the exact verified candidate?
- Are checks green on that head, not an earlier commit?
- Is the risk tier computed from the final diff?
- Is the issue linked to the frozen spec, not just mentioned?
- Does the PR avoid secrets, raw prompts, DBs and unbounded logs?
- Is auto-merge eligibility explicit and policy-backed?
- Is the merge commit/target correct?
- Is post-merge state verified and the issue closed with evidence?
- Is rollback possible and documented?

## 7. Evidence template

```text
Candidate:   <SHA>
Base:        <branch@SHA>
Risk:        R0/R1/R2/R3 (reason)
Issue:       <link and frozen spec identity>
PR:          <link and head SHA>
Checks:      <exact commands/results on head>
Verifier:    <PASS/FAIL/NOT_TESTED and identity>
Reviewers:   <roles and verdicts>
Gate:        <outcome and evidence>
Merge:       <eligible/not eligible; policy and branch protection>
Post-merge:  <checks and issue receipt>
Rollback:    <path>
Residual:    <NOT_TESTED / BLOCKED>
```

## 8. Stop conditions

Stop and report if:

- the candidate is dirty or has moved after verification;
- a required check is missing, skipped or only passed on retry;
- branch protection or required review is absent;
- the risk tier includes a reserved effect without owner authority;
- a secret, credential or private prompt would enter the PR;
- the merge would exceed the authorized scope or budget;
- rollback is impossible.

## 9. References

- [Quality baseline](../../../docs/quality/baseline.md)
- [Live certification](../../../docs/quality/live-certification.md)
- [ADR-0038 owner charter](../../../docs/architecture/decisions/ADR-0038-owner-charter-and-delegated-autonomy.md)
- [Agentic flow contract](../../../docs/contracts/agentic-flow.md)
- [Security review](../security-review/SKILL.md)
