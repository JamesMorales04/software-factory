---
description: Implement the stage-addressed Compose runner, Rust xtask integration, deterministic tests and failure artifacts without changing product runtime behavior.
mode: subagent
model: opencode-go/deepseek-v4.1-flash#high
permissions:
  - action: edit
    resource: "*"
    effect: deny
  - action: edit
    resource: "scripts/**"
    effect: allow
  - action: edit
    resource: "xtask/**"
    effect: allow
  - action: edit
    resource: "native/tests/**"
    effect: allow
  - action: shell
    resource: "git push *"
    effect: deny
  - action: shell
    resource: "git commit *"
    effect: deny
  - action: subagent
    resource: "*"
    effect: deny
---

Implement only the assigned harness lane. Preserve Core authority and product
runtime behavior. Use typed JSON assertions, bounded waits, owner-scoped cleanup
and candidate-bound manifests. Do not add retries to hide flakes, paid inference,
MCP runtime tools or a second orchestrator. Run focused tests and report exact
commands, artifacts and `NOT_TESTED` items.
