# Isolation, credentials and trust boundaries

**Agreed:** sandbox by default; host access only through an explicit, audited, scoped exception — except in owner-authorized TRUSTED_LOCAL mode, where broad local access is explicitly accepted with documented risks (see [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md) and [trusted-local policy](trusted-local.md)). **Technology demonstrated in disposable fixtures:** rootless Podman isolated checked host paths, but the actual authenticated provider/tool boundary is **not certified**. **F1-S3-ISOLATED blocker:** real Paseo isolation and credential behavior. S3-TRUSTED_LOCAL proceeds under its own entry conditions per ADR-0009. See [SEC-01 results](SEC-01-results-2026-10-18.md); further strict-isolation tests paused by the product owner.

## Execution modes

| Mode | Meaning | SEC-01 disposition |
|---|---|---|
| TRUSTED_LOCAL | Owner-authorized local execution with broad host access within user privileges. Accepted risk, not a boundary. | SEC-01-STRICT not required; historical FAIL/NOT TESTED findings remain valid observations but do not block this mode. |
| ISOLATED | Execution claiming an OS-enforced boundary between credentials and untrusted tools. | SEC-01-ISOLATED available-only-where-verified against actually tested paths; never inferred from a single fixture PASS. |
| RESTRICTED | Credential-free synthetic or disposable-fixture execution (current F1-S2 scope). | Broker-certification obligations (mandatory mediation of all native paths) apply here and to ISOLATED; they do not gate TRUSTED_LOCAL. |

Rules:

- Never call TRUSTED_LOCAL isolated, sandboxed, certified, or proof of credential confinement.
- Never present historical FAIL/NOT TESTED results as PASS in any mode.
- ISOLATED guarantees are narrow and evidence-bound: state exactly which topology, provider/runtime, image digest, and boundary checks were tested.
- Silent mode downgrade is forbidden: moving a Run or session from ISOLATED to LOCAL (including TRUSTED_LOCAL) requires explicit owner authorization recorded with actor, reason, and scope. A Podman or broker failure fails closed; it never silently becomes TRUSTED_LOCAL.

## Authority

The Core owns delivery credentials, authorization policy, budgets, canonical state and resource governance. Workers materialize strictly scoped assignments. Paseo owns agent sessions, not Factory acceptance or GitHub merge authority. Repository tests, downloaded skill scripts and agent shell commands are **potentially untrusted**.

## Per-Run workspaces

Use a sealed source identity and an isolated checkout for each Run. Avoid shared mutable worktrees across projects or attempts. Git worktrees provide source management, **not OS security isolation**. The Reviewer/Verifier receive read-only candidate snapshots and independent oracles.

## Required protections

- No host HOME, privileged container sockets, Core DB, GitHub merge tokens, other-project folders or provider secrets visible to untrusted commands.
- Bound filesystem, process, UID/user namespace, CPU/RAM/PIDs, networking and egress; no silent fallback to direct host shell.
- Tool/LLM instructions can express intent but do not enforce privileges. Effective authorization belongs at the OS boundary and Core-mediated side effects.
- Host exceptions require resource, actor, reason, expiry, narrow permissions and audit evidence; there is no implied `all-host-access` grant.
- **Critical:** running provider CLI and untrusted shell in the same credential-bearing sandbox does not demonstrate secret protection. Evaluate process separation, mount namespaces and mediation; block unsupported topologies.
- Any new executable external skill must undergo provenance, integrity, sandbox and capability checks before admission; skills never self-authorize.

## Negative acceptance tests

1. Untrusted execution cannot read or mutate host/other-project files.
2. Untrusted scripts cannot inspect Core secrets or provider authentication artifacts within the defined threat model.
3. Alternative shell/MCP/skill invocation cannot bypass capabilities.
4. A real Paseo agent can start, run, be inspected and end while the boundary holds.
5. Required egress works and forbidden destinations fail.
6. Crash/restart/cleanup do not leak privileged orphan sessions, processes or workspaces.

These are actual test obligations, not proof of absolute security. Follow [SEC-01](paseo-sandbox-spike.md).
