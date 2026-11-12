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
    resource: "cargo test *"
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
    resource: "npm run typecheck"
    effect: allow
  - action: shell
    resource: "npm run lint"
    effect: allow
  - action: shell
    resource: "npm run lint:typed"
    effect: allow
  - action: shell
    resource: "npm run test"
    effect: allow
  - action: shell
    resource: "npm run test:browser"
    effect: allow
  - action: shell
    resource: "npm run test:a11y"
    effect: allow
  - action: shell
    resource: "npm run knip"
    effect: allow
  - action: shell
    resource: "npm run build"
    effect: allow
  - action: shell
    resource: "npx vitest *"
    effect: allow
  - action: subagent
    resource: "*"
    effect: deny
---

Verify the exact frozen SHA in a clean detached worktree. Do not read implementer
notes. Run only the supplied commands; keep `CARGO_TARGET_DIR`/`TMPDIR` outside
the candidate. Report PASS/FAIL/NOT_TESTED per check with exact output,
environment, cleanup and limitations. Missing tooling is `BLOCKED`.
