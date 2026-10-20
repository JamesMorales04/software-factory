# SEC-01 S2-B — threat model, provider verdicts and S2-C boundary

**Status:** investigation synthesis for S2 completion. No certification is granted here. SEC-01 remains **PAUSED / BLOCKED** for credential-bearing autonomous execution **under an isolation claim (ISOLATED)**; per [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md), SEC-01-STRICT is not required for owner-authorized TRUSTED_LOCAL, which proceeds under its own entry conditions. Factory candidate `1bef07b`; S2-C product changes (below) are review-bound, not certified.

Related: [isolation invariants](isolation.md), [spike plan](paseo-sandbox-spike.md), [recorded results](SEC-01-results-2026-10-18.md), [Run–Worker contract](../contracts/run-worker-s2.md).

## 1. Trust topology (enforced target)

```text
Owner / Core (trusted, canonical state, policy, budgets)
  │  scoped worker token (protocol only) + allowlisted env
  ▼
Worker supervisor (trusted Go process, no provider credentials)
  │  fixed argv, disposable /workspace, attempt receipt
  ▼
Per-Attempt rootless Podman (untrusted execution, no HOME/auth/
sockets/network, read-only root, dropped caps, no-new-privs,
bounded resources, pinned image)
```

A future provider process is a **separate** OS compartment holding
credentials; untrusted model input must never share its environment,
filesystem view, or tool routes. A same-UID colocated process is **not**
separation (recorded D2-C FAIL stands). Model refusal, a missing tool, or
a config flag is **not** an OS boundary.

## 2. Provider verdicts (S2-D input, not certification)

### Codex 0.156.1 — NEEDS-AUTH-PLAN (conditional path exists, uncertified)

- Unauthenticated direct-sandbox probes show real OS denials under a
  custom allowlist profile (`:minimal` + workspace read, no writes,
  no network, approval never, env inherit none): workspace readable,
  writes `EROFS`, outside paths masked (`ENOENT`), NoNewPrivs,
  seccomp enforced, network `EPERM`.
- The legacy `read-only` profile still exposes outside files (D1
  reproduced in principle) — never equate it with a boundary.
- Gaps requiring the gated auth plan: real model tool coverage for
  every route (shell, patch, reads, code mode, subagents, background),
  local-MCP launcher (no shell sandbox applied), remote MCP
  (`sandbox: None`), resume accepting weaker policy, hooks/plugins/
  cloud surfaces, Paseo integration.
- Exact owner-approval-gated test plan: disposable VM, dedicated
  revocable identity, 30-minute / 12-turn cap, synthetic-only fixtures,
  preflight matrix before any login, per-route oracles distinguishing
  OS denial vs policy rejection vs unavailable vs refusal, VM destroy as
  final cleanup oracle. Full plan held by the investigation record; it
  runs only after explicit owner approval of identity, budget, image,
  egress and route matrix — **not started**.

### OpenCode 1.18.32 — BLOCKED (no OS boundary available via configuration)

- Permissions, `plan`/`build` modes, tool denials, plugin admission and
  MCP routing are in-process application controls (source-pinned to
  upstream tag `545f51d`).
- `plan` does not deny `bash`; hidden `--yolo` /
  `--dangerously-skip-permissions` exist; plugins are arbitrary host
  code; command-template expansion executes outside the `bash` tool;
  subagents run in the same runtime and can exceed parent agent
  restrictions; local MCP inherits environment.
- No inspected config or flag provides process-level exclusive broker
  mediation. Stock OpenCode must not hold credentials reachable by any
  untrusted route. Options: a code-level hardened adapter removing
  native routes, or a topology keeping credentials out of the OpenCode
  process and children entirely (separate credential/proxy service).
- Real-agent bypass tests remain NOT_TESTED; exact approval-gated test
  plan held by the investigation record.

## 3. S2-C controls implemented (smallest enforceable set)

1. **Worker environment allowlist** (`core/cmd/factory`): the Worker
   child inherits only `PATH`, `HOME`, `XDG_RUNTIME_DIR`, `TMPDIR` plus
   its scoped token. Synthetic-canary regression tests prove provider/
   GitHub/operator secrets do not propagate.
2. **Fail-closed Podman default** (`core/cmd/factory-worker`): missing
   Podman or fixture image refuses startup; `--exec-mode local` is
   explicit-only for tests/dev and always reported.
3. **Container argv audit** (`core/internal/exec`): the disposable
   invocation is built by a pure function with a unit test locking the
   restriction set; any flag drift fails the test.
4. **Image digest evidence**: resolved digest recorded in startup logs
   and attempt results for drift detection.
5. **Orphan workspace reaper**: Worker sweeps its own disposable
   pattern at startup; in-process cleanup on every attempt return;
   `podman rm -f` discipline proven by the PoC.
6. **Scoped broker PoC** (`scripts/sec01-poc-broker.sh`, synthetic-only,
    disposable, RESTRICTED/research basis): fixed-op workspace-bound broker, traversal rejection,
   unconstrained-shell control reads (proving mediation is required),
   container battery (canary/env/network/socket), orphan cleanup,
   canary-absent broker I/O. ALL CHECKS PASSED on this host.

## 4. What S2-D still requires

Per-provider real-route certification with the approved auth plans,
independent verifier evidence on exact candidate SHAs, fresh Luna
review, and an explicit owner decision. One passing provider does not
certify the other; S3-ISOLATED scope must be reduced to the certified provider,
or stay blocked. (S3-TRUSTED_LOCAL is not gated by provider certification;
it follows its ADR-0009 entry conditions.) CI success and Reviewer APPROVE are necessary but not
sufficient for security acceptance.
