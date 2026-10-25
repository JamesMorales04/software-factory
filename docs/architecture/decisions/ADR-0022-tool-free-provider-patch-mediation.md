# ADR-0022 — Tool-free provider patch mediation

- **Status:** Proposed for implementation review.
- **Decision date:** 2026-10-24.
- **Base:** native Rust replacement branch after the Rust-only retirement.
- **Related:** ADR-0018, ADR-0009, ADR-0010, ADR-0012.

## Context

The Rust Core and Worker can authorize, lease and verify a candidate, but the
first provider-backed path must not put provider credentials, the Podman socket
or an unrestricted provider process inside the untrusted project sandbox. The
Codex/OpenCode adapters are not yet a complete mediated-tool runtime.

## Decision

The first trusted-local provider mode is **tool-free patch generation**:

1. Core authorizes `TRUSTED_LOCAL_PROVIDER` with an active standing delegation,
   exact project/provider/model/mode, allowed paths, session/turn ceilings,
   token/cost budgets, a deadline and an explicit empty provider-tool list.
2. The trusted Worker holds the provider credential. It records durable session
   intent in Core before contacting the provider.
3. The provider receives an isolated empty session directory and a bounded
   objective/path prompt. It must return one JSON object containing only file
   paths and contents. Any tool part, unsafe path, duplicate path or oversized
   output is rejected.
4. The Supervisor validates the patch against the frozen allowed paths and
   applies it to the exact Attempt worktree, then commits the immutable
   CandidateRevision. The sandbox never receives the provider credential and
   the provider never receives the host workspace path.
5. Core records terminal provider evidence, known usage and a cleanup receipt.
   Completed sessions require confirmed cleanup and measurable usage. An
   unresolved intent/active session blocks replacement dispatch.
6. Provider tool mediation, live Codex work turns, model-backed Lead assessment
   and ISOLATED execution remain unimplemented and fail closed.

## Consequences

This establishes a real, end-to-end, deterministic fake-provider path without
surrendering credentials, authority, workspace ownership or Quality Gate
independence. It is not a full coding-agent capability: the provider does not
receive repository source context in this slice, and live provider execution
requires separate bounded authorization. A future ADR must define mediated
tool events, permission decisions and source-context delivery.
