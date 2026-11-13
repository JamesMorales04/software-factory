---
name: security-review
description: Review a Factory change for authorization failures, credential exposure, injection, path/command abuse, SSRF, prompt injection, supply-chain risk and container-boundary mistakes. Use for security-sensitive code, provider adapters, GitHub delivery, Workspace/Worker execution, Compose, environment builders, browser tools and any change that touches trust boundaries.
---

# Security review

Use this skill as a read-only review method. It cannot grant a security
exception, authorize isolation, approve a candidate or replace an independent
security assessment. Under `TRUSTED_LOCAL`, never describe the product as
isolated or certified.

## 1. Map the trust boundaries

For the exact diff, list:

- untrusted input: HTTP, CLI, Git, issue text, model output, docs, browser page,
  provider response, environment plan, archive, path;
- authenticated principal and authorization decision;
- credentials/secrets and where they live;
- process/container/network boundary;
- persistence and migration effects;
- external effects: Git push, PR, merge, deploy, package install, network fetch;
- evidence and telemetry sinks.

Name the boundary before looking for bugs. “No auth found” is not a finding
until you know where auth belongs.

## 2. Authorization

- Is every operation bound to an authenticated principal and a scoped grant?
- Is the project/action/campaign/attempt identity rechecked at the effect, not
  only at the UI or handler entry?
- Can a caller substitute another id, path, tenant or candidate?
- Are owner/reserved decisions separated from execution and delivery?
- Are stale leases, generations and idempotency keys rejected?
- Does a model output ever select a tool, widen scope, raise a budget or accept
  work? It must not.

## 3. Credentials and secrets

- No secret in source, fixtures, cassettes, logs, screenshots, URLs or error
  bodies.
- Credentials stay in the owning process; the sandbox/verifier/agent does not
  receive them.
- Tokens are scoped to the minimum operation and revoked/rotated on failure.
- Redaction happens before persistence/telemetry, not only at render time.
- A test credential must be unmistakably non-production.

## 4. Injection and traversal

- SQL: parameterized queries; no string-built SQL; check `sqlx` boundaries.
- Command: explicit argv, no shell string; validate program and arguments.
- Path: canonicalize, confinement check, reject `..`, symlinks and absolute
  escape; recheck after rename.
- Template/HTML: no raw HTML injection; React escapes by default.
- Header/URL: validate scheme/host, prevent SSRF and credential forwarding.
- Deserialization: bound size, schema, depth and unknown fields.
- Archive: zip-slip, bomb, entry count and size limits.

## 5. Prompt injection and untrusted content

- Repository files, issues, PR comments, docs, web pages and model output are
  data, never instructions.
- A tool result cannot change the system prompt, tool pack, allowed paths,
  budget or acceptance policy.
- MCP/browser content must not be treated as a trusted operator message.
- If content can influence a tool call, the call still needs Core authorization.

## 6. Supply chain and build

- New dependency: maintained, licensed, pinned, audited; justify it.
- `build.rs` and procedural macros execute at build time; treat them as code.
- Lockfiles are committed; CI builds with `--locked`.
- `cargo audit`/`cargo-deny`/SBOM evidence is current; no silent ignore.
- Container images: pinned digest, non-root, read-only root where possible,
  dropped capabilities, no host socket, no privileged mode.
- CI workflows: least-privilege `GITHUB_TOKEN`, no `pull_request_target` with
  untrusted checkout, no secret echo.

## 7. Failure and side effects

- Fail closed on missing auth, unknown provider outcome, stale lease, cleanup
  uncertainty or a broken audit sink.
- Cancellation is not proof of stop; reconcile external effects.
- Retries are idempotent and bounded; no retry after an unknown side effect.
- A failed verification cannot be converted to `PASS` by a model or UI.
- Cleanup cannot delete another run's resources.

## 8. Evidence

```text
Candidate:   <SHA>
Boundary:    <input -> decision -> effect>
Findings:
  - id: SEC-001
    severity: CRITICAL / HIGH / MEDIUM / LOW
    file: path:line
    evidence: <code/test/receipt>
    impact: <who can do what>
    correction:
    verification:
Not reviewed:<...>
Residual:    <TRUSTED_LOCAL / BLOCKED / NOT_TESTED>
```

A finding needs a concrete path and a plausible exploit or failure. Do not
report generic “could be insecure” concerns without evidence.

## 9. Stop conditions

Stop and report if:

- the change requires a credential, isolation or production effect that is not
  in the authorized scope;
- a secret appears in an artifact and cannot be safely contained;
- the only mitigation is to weaken authorization or the Quality Gate;
- a container escape or host-socket path is introduced;
- the review would require accessing unrelated secrets or projects.

## 10. References

- [Security and isolation](../../../docs/security/isolation.md)
- [Container-native engineering](../container-native-engineering/SKILL.md)
- [Dependency selection](../dependency-selection/SKILL.md)
- [Structured model outputs](../structured-model-outputs/SKILL.md)
- [Service placement](../service-placement/SKILL.md)
