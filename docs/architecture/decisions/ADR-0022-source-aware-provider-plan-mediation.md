# ADR-0022 — Source-aware provider plans and mediated tools

- **Status:** Proposed for implementation review.
- **Decision date:** 2026-10-24.
- **Base:** native Rust replacement branch after the Rust-only retirement.
- **Related:** ADR-0018, ADR-0009, ADR-0010, ADR-0012.

## Context

The Rust Core and Worker can authorize, lease and verify a candidate. Provider
credentials, the Podman socket and unrestricted host commands must not enter
the untrusted project sandbox. A first provider path proved a tool-free patch;
the next step supplies bounded source context and Factory-mediated tools without
giving the provider an execution boundary.

## Decision

Provider execution has two explicit, Core-authorized shapes:

1. **Tool-free patch:** the provider returns only bounded file paths/contents.
2. **Source-aware multi-turn plan:** the Worker supplies a bounded,
   credential-filtered snapshot of files under the frozen allowed paths. Up to
   the authorized turn ceiling, the provider may request one `write_file` or
   `run_command` tool per turn, receive the mediated tool result, and then
   continue. Every request, permission decision and result is durably recorded.

In both shapes:

- the trusted Worker holds the long-lived provider credential; it never enters
  the Supervisor, sandbox or provider prompt;
- the provider process runs in a bubblewrap user/pid/net/ipc/uts/cgroup
  namespace with an empty HOME, a read-only system image, only its private
  session directory writable and no host repository, host HOME, runtime socket
  or external network; a per-run Basic-auth capability, not the long-lived
  credential, reaches the provider;
- the provider receives an isolated empty session directory, not the host
  workspace path;
- only `write_file` and `run_command` can be authorized; a tool request outside
  that subset is rejected;
- `run_command` must exactly match a frozen acceptance check argv, and executes
  only through the rootless-Podman Supervisor in the exact Attempt workspace;
- the Supervisor applies writes, runs mediated commands and finalizes the
  immutable CandidateRevision;
- Supervisor allocation requires a Core-signed grant bound to the exact
  Attempt, worker, generation, repository, base commit and ActionSpec digest;
  workspace operations require a per-Attempt capability, not the broad
  Supervisor admin bearer;
- durable provider intent, active-session identity, terminal usage and cleanup
  receipts are recorded in Core; unresolved sessions block replacement dispatch;
- cancellations and revocations move unresolved sessions to
  `CANCEL_REQUESTED`, never to a claimed external stop.

Live Codex work turns with real tool execution, live provider model egress
through the sandbox and ISOLATED execution remain unimplemented and fail closed.
Multi-turn OpenCode tool-event mediation, the bubblewrap provider boundary and
Codex permission-decision protocol fixtures are deterministic only.

## Consequences

This is a real source-aware, tool-mediated, process-confined fixture path, not a
live-model or isolation certification. It preserves the tool-free restriction as
a distinct mode, keeps the long-lived provider credential and host control
outside the sandbox, and makes every provider-originated effect pass through
Core authorization, path validation, frozen-command matching and Supervisor
execution. A later ADR must define live multi-turn provider tool-event
reconciliation, sandboxed model egress, live permission enforcement, recovery of
confined sessions and independent isolation evidence before live provider coding
is authorized.
