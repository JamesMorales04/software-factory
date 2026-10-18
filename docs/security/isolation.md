# Isolation, credentials and trust boundaries

**Agreed:** sandbox by default; host access only through an explicit, audited, scoped exception. **Technology proposed:** rootless Podman/OCI, not yet certified. **F0/F1 blocker:** real Paseo isolation and credential behavior.

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
