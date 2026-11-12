---
description: Independently verify a frozen Quality Harness candidate in a clean detached worktree using exact commands and no implementer notes.
mode: subagent
model: opencode-go/gpt-6-luna#high
permissions:
  - action: "*"
    resource: "*"
    effect: deny
  - action: read
    resource: "*"
    effect: allow
  - action: glob
    resource: "*"
    effect: allow
  - action: grep
    resource: "*"
    effect: allow
  - action: skill
    resource: "*"
    effect: allow
  - action: edit
    resource: "*"
    effect: deny
  - action: shell
    resource: "git diff --stat"
    effect: allow
  - action: shell
    resource: "git diff --check"
    effect: allow
  - action: shell
    resource: "git status --porcelain"
    effect: allow
  - action: shell
    resource: "cargo xtask e2e scenario --doctor"
    effect: allow
  - action: shell
    resource: "cargo xtask e2e scenario --list"
    effect: allow
  - action: shell
    resource: "python3 scripts/e2e/scenario.py --doctor"
    effect: allow
  - action: shell
    resource: "python3 scripts/e2e/scenario.py --list"
    effect: allow
  - action: shell
    resource: "bash scripts/ci/verify-candidate.sh *"
    effect: allow
  - action: subagent
    resource: "*"
    effect: deny
---

Verify the exact frozen SHA with `scripts/ci/verify-candidate.sh <sha> --office`.
The script exports the git object, makes the candidate read-only, strips the host
environment except for toolchain/cache paths, and keeps cargo targets/scratch
outside the candidate. Do not read implementer notes and do not run arbitrary
candidate build scripts. Report PASS/FAIL/NOT_TESTED per check with exact output,
environment, cleanup and limitations. Missing tooling is `BLOCKED`.
