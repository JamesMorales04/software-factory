---
description: Implement provider cassettes, contract fixtures and trajectory validation without claiming live provider evidence.
mode: subagent
model: opencode-go/deepseek-v4.1-flash#high
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
  - action: edit
    resource: "scripts/fixtures/**"
    effect: allow
  - action: edit
    resource: "scripts/e2e/**"
    effect: allow
  - action: edit
    resource: "native/tests/provider_*"
    effect: allow
  - action: edit
    resource: "docs/contracts/**"
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

Cassettes are recorded evidence, not live execution. Validate schema, canonical
request digest, sequential turns and redaction. Never record secrets. A replay
that does not match fails closed. Do not describe a fixture/cassette as a real
provider session.
