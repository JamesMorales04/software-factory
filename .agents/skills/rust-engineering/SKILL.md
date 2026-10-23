---
name: rust-engineering
description: Apply the Software Factory's idiomatic Rust engineering standards when implementing, reviewing, refactoring, testing, or debugging Rust Core, Worker, provider adapters, concurrency, persistence, and security-sensitive code.
---

# Rust engineering

Use this skill for any change to first-party Rust in this repository. It is a
development aid, not an authority: it cannot grant budgets, risk acceptance,
merge rights, sandbox exceptions or Quality Gate outcomes.

## Procedure

1. **Read the governing rules.** Load
   `docs/engineering/rust-standards.md` and the applicable ADR, contract,
   frozen specification and `docs/engineering/code-quality.md`. Skim only the
   references relevant to the change.
2. **Locate the boundaries.** Name the domain decision, persistence
   transaction, provider/protocol boundary, workspace/process lifecycle and
   external effect involved. Identify what Core must own and what must remain
   untrusted input.
3. **State observable acceptance.** Write the condition that proves the
   behavior: typed state transition, persisted row, emitted event, HTTP/CLI
   result, provider terminal evidence, or failure classification. Include the
   negative cases that could invalidate it.
4. **Choose the simplest design.** Prefer concrete types, small modules,
   explicit ownership and composition. Introduce a trait, generic, typestate or
   newtype only for a real boundary, invariant or second implementation. Do not
   add a layer for symmetry.
5. **Implement with strong types and explicit errors.** Parse untrusted input
   once into validated values. Use enums for closed alternatives, checked
   arithmetic for counters and budgets, `Result` with typed errors, and no
   production `unwrap`/`expect`/`panic`. Never hold a transaction, lock or
   borrow across `.await` or an external call.
6. **Run focused checks, then the full gates.** Use the pinned toolchain:
   `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
   `cargo test --workspace --locked`, `cargo build --workspace --locked`,
   `cargo doc --workspace --no-deps --locked` and
   `cargo audit --file Cargo.lock`.
7. **Inspect the hard invariants.** Verify task ownership and bounded
   lifetime, cancellation versus external stop, stale-generation rejection,
   cleanup and orphan reconciliation, idempotency binding, budget atomicity,
   path/credential confinement, and fail-closed behavior. A cancellation
   request is not a stopped process.
8. **Check architectural consistency.** Domain must not import transport,
   database or provider representations. No second scheduler, policy engine or
   authority path. No obsolete compatibility adapter. Update callers, tests,
   docs and fixtures atomically when an interface changes.
9. **Review for avoidable complexity.** Remove dead code, speculative
   abstractions, duplicated policy, unbounded loops, silent fallbacks and
   evidence claims that exceed what ran. Apply scoped, justified lint
   exceptions only when the invariant is documented.
10. **Produce candidate-bound evidence.** Record the exact commit, commands,
    results, provider/session/workspace identities, cleanup outcome, and what
    remained `NOT_TESTED` or `BLOCKED`. Re-run invalidated checks after any
    repair; update `docs/implementation/native-rust-handoff.md` and
    `native-rust-progress.json`.

## Stop conditions

Stop and report rather than guessing when:

- authority, budget, risk class, allowed paths or isolation policy is missing;
- required isolation or credential confinement is unavailable (fail closed,
  never fall back to host execution);
- a provider outcome is unknown or lacks terminal evidence;
- a migration would discard unreplicated behavior or evidence;
- a test would require a paid provider turn without a bounded authorization;
- a security or quality rule would need to be relaxed to proceed.

## References

Load only what the task needs:

- [Code review](references/code-review.md) — review checklist and severity
  framing.
- [Async and process lifecycle](references/async-process-lifecycle.md) —
  ownership, shutdown, cancellation, cleanup and reconciliation.
- [State machines](references/state-machines.md) — Core/Worker transitions,
  leases, fencing and idempotency.
- [Provider adapters](references/provider-adapters.md) — Codex App Server and
  OpenCode boundaries, terminal evidence and permissions.
- [Persistence and transactions](references/persistence-and-transactions.md) —
  SQLite/WAL, atomic reservations, receipts and recovery.
- [Errors and security](references/errors-and-security.md) — typing failures,
  redaction, path and credential boundaries.

## Evidence template

```text
Candidate:  <exact commit SHA>
Scope:      <behavior and files>
Commands:   <formatter, clippy, tests, build, doc, audit with versions>
Real use:   <process/API/provider evidence and exact identities>
Negatives:  <invalid auth, duplicate, stale lease, timeout, cleanup, ...>
Cleanup:    <confirmed | unresolved | blocked, with evidence>
Review:     <independent reviewer and disposition, or pending>
Residual:   <NOT_TESTED / BLOCKED / known risks>
```
