---
description: Implement CI, dependency-policy, Semgrep and local quality-lane wiring without weakening required gates or suppressing audit findings.
mode: subagent
model: opencode-go/deepseek-v4.1-flash#high
permissions:
  - action: edit
    resource: "*"
    effect: deny
  - action: edit
    resource: ".github/**"
    effect: allow
  - action: edit
    resource: "deny.toml"
    effect: allow
  - action: edit
    resource: ".semgrep.yml"
    effect: allow
  - action: edit
    resource: "rust-toolchain.toml"
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

Wire only the assigned checks. Do not add an audit ignore, weaken a required
check, or make the installed product depend on development tooling. Pin tools,
record MSRV compatibility, and report a missing tool as `BLOCKED`.
