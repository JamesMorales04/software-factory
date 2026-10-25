# Independent provider-boundary review — 2026-10-25

**Reviewer:** independent Codex subagent (`gpt-6-sol`, high reasoning), not the implementer.
**Reviewed commit:** `e0bb7d2f06a7949bd4bb23be2f6edbd7ee89c8c3`.
**Mode:** static read-only review; no tests or provider turns run by the reviewer.
**Status:** independent model review, **not human security certification**. It does
not certify isolation or authorize live provider coding.

## Findings and dispositions

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | HIGH | Workspace capability was not fenced by the current lease; revoked/expired work could still mutate before capability expiry. | **REPAIRED**: Supervisor mutations now require a Core-signed operation grant issued only after checking the live lease, worker, generation, expiry, campaign state and active provider session. Grants expire in 5 seconds and are bound to workspace, attempt, generation, operation and scope. Residual: a revocation inside the 5-second grant window is not instantly observable by the Supervisor. |
| 2 | HIGH | Supervisor trusted caller-supplied allowed paths and run argv; a stolen capability could bypass frozen scope. | **REPAIRED**: operation grants carry the Core-authorized allowed paths and frozen check argv. Supervisor rejects mismatched paths and any run argv not in the signed checks. |
| 3 | MEDIUM | Bridge authenticated only the first request then copied the connection; pipelined/unbounded paths could bypass per-request checks. | **REPAIRED**: bridge parses Content-Length, rejects chunked framing and oversized bodies, permits only exact provider routes, forces `Connection: close`, and caps connections/time. |
| 4 | MEDIUM | Local clients could exhaust bridge connections/streams. | **REPAIRED**: connection semaphore, header/body bounds, total bridge timeout and bounded stderr capture. |
| 5 | HIGH | Provider session bind mount, CPU, memory, PID, output and lifetime had no hard quota. | **REPAIRED (candidate)**: `prlimit` fsize/CPU/address-space/nofile bounds, 64 MiB tmpfs, 64 MiB session monitor with SIGKILL, bounded stderr, lifetime and connection limits. A hostile writer can still race the monitor between samples; this is not an OS filesystem quota. |
| 6 | HIGH residual | Compromised Worker exposes host authority and credentials. | **NOT FIXED**: Worker remains trusted TCB; no separate OS identity or credential service. Documented explicitly. |
| 7 | MEDIUM | Recovery could call an unknown external session clean after a crash before ACTIVE recording. | **PARTIALLY REPAIRED**: provider intent now records the session directory before process start; sandbox identity (pid/start time/boot id) is durable; recovery verifies absence or terminates a confirmed surviving process before cleanup. Full Worker-kill E2E at every boundary remains outstanding. |
| 8 | MEDIUM | Source filtering missed ordinary credential forms such as `GITHUB_TOKEN`. | **REPAIRED**: added GitHub/gh token and token-assignment patterns plus an ordinary-file E2E canary. This remains defense-in-depth, not a secret-proof guarantee. |
| 9 | LOW | Same-UID pathname races remain in host worktree writes. | **OPEN**: documented residual; no separate OS identity. |

## What the review establishes

The reviewer found no demonstrated direct path from the confined provider to
Core, Supervisor, host-local services or metadata endpoints at the reviewed
commit. The review did not run tests and did not establish Podman isolation,
same-UID resistance, live provider behavior or human certification.

## Required next work

- Independently rerun the repaired boundary and crash-recovery tests.
- Replace the session monitor with a hard storage quota or a tmpfs/evidence
  split that removes the writable host bind mount.
- Add a separate confined Worker identity/credential service or keep the Worker
  explicitly in the trusted computing base.
- Complete confined-session kill/restart E2E at every durable boundary.
- Keep live provider coding and ISOLATED fail-closed until separately authorized.

## Follow-up review of `90f9302`

A second read-only pass reviewed the first repair candidate. It confirmed that
signed `/write`, `/apply` and `/run` scope closed the original caller-supplied
scope bypass, but found remaining gaps: allocation generation was not compared,
operation grants were reusable and not bound to the request, `execute` was
available to provider-mode Attempts, recovery identity lived in the
provider-writable session directory, egress did not disable environment
proxies or enforce request/model limits, and the egress socket had no read
deadline.

Those gaps were repaired in code candidate `0c70a85`:

- allocation grant generation is compared with the request and existing
  workspace records;
- capability and operation grants are generation-bound;
- operation grants carry a one-time nonce and a request SHA-256;
- `execute` is restricted to FIXTURE Attempts;
- sandbox identity is stored outside the provider-writable mount;
- egress disables environment proxies, rejects special-use addresses, bounds
  request reads, and enforces authorized model and request-count limits;
- tests cover generation mismatch, operation scope/digest/expiry and identity
  placement outside the writable session directory.

Remaining open items: long-running `/execute` or `/run` can complete after a
lease change because the Supervisor cannot consult Core during the operation;
cleanup remains capability-based; the Worker is still trusted; storage limits
remain monitor-based; the egress broker bounds requests and response bytes but
does not itself enforce Core token/cost accounting; full Worker-kill/restart E2E
at every boundary and human certification remain absent. Live provider coding
and ISOLATED remain fail-closed.


## Runtime-reliability follow-up at `7117bba45de8e39da858e45d77378da39d2b2cac`

- Running-operation fencing now has a Core validation endpoint and a Supervisor
  watcher that cancels the active container after two failed checks. The
  manager-level kill/late-retry test passes; full Supervisor/Core long-operation
  E2E is not run.
- Real rootless Podman named-volume and Alpine confinement tests pass on the
  local host. The conditional Podman CI lane is not run by ordinary GitHub CI.
- Actual OpenCode 1.18.32 and Codex 0.156.1 executables both reached a
  deterministic mock upstream through the egress broker with injected real
  credentials and no paid calls. This is host-level executable compatibility,
  not sandboxed full-work-turn certification.
- The writable session mount still uses a monitor rather than an OS quota;
  cgroup PID/memory/CPU limits and Core per-turn token/cost broker accounting
  remain open.
