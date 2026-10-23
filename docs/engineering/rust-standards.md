# Engineering standard: idiomatic Rust

**Status:** normative for first-party Rust in this repository. **Toolchain:** Rust 1.90.0, pinned by `rust-toolchain.toml` and `Cargo.lock`; Rust 2024 edition. **Applies to:** Core, Worker, provider adapters, persistence, governance tools, tests and fixtures.

This document is the canonical Rust-specific engineering standard. [Code quality](code-quality.md) defines cross-language design principles and [lint policy](linting.md) defines the objective toolchain. `AGENTS.md` remains a short operational entry point and links here instead of restating these rules. The [Rust engineering skill](../../.agents/skills/rust-engineering/SKILL.md) is the procedural companion for agents.

Primary references, verified against the pinned toolchain rather than assumed:

1. [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
2. [Rust Design Patterns](https://rust-unofficial.github.io/patterns/)
3. [The Rust Programming Language](https://doc.rust-lang.org/book/)
4. [Rust 2024 Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/)
5. [Clippy documentation](https://doc.rust-lang.org/clippy/)
6. [Tokio graceful shutdown](https://tokio.rs/tokio/topics/shutdown)
7. [Asynchronous Programming in Rust](https://rust-lang.github.io/async-book/)
8. [Agent Skills specification](https://agentskills.io/specification)

When this standard conflicts with an accepted ADR or a frozen contract, stop and reconcile through the documented governance path. A skill, prompt or model output cannot waive these rules.

## 1. Architecture style

Use **pragmatic modular monolith + hexagonal boundaries + idiomatic Rust + selective domain-driven design**.

- Core is one authoritative modular monolith. Worker is a separate process. Provider adapters, persistence, workspace operations, delivery and time are ports at genuine boundaries.
- Keep domain decisions independent of Axum handlers, SQLite row shapes, JSON-RPC envelopes, HTTP payloads and provider SDK types.
- Use a trait when there are multiple real implementations, an independent test seam, or genuine runtime polymorphism. Use a concrete type for a stable internal implementation.
- Do not create a trait per struct, a generic repository, a dependency-injection framework or Java-style trait hierarchies. Composition of small modules beats inheritance-shaped design.
- A module boundary should correspond to a change boundary. Move code only when the move reduces coupling or protects an invariant.
- Enforce dependency direction: domain policy must not import transport, database or provider crates. The compiler module graph is part of the design review.

The intended crate layout is one Cargo workspace owning Core, Worker, domain, persistence, providers, quality and governance tools. A temporary `native/` tree beside legacy code is a migration state, not the target architecture.

## 2. Naming

Follow Rust conventions without local dialects.

| Item | Convention | Example |
|---|---|---|
| Crate, module, function, method, local | `snake_case` | `claim_attempt`, `workspace_root` |
| Type, trait, enum variant | `UpperCamelCase` | `CandidateRevision`, `LeaseToken` |
| Constant, static | `SCREAMING_SNAKE_CASE` | `MAX_WORK_UNITS` |
| Conversion | `as_`, `to_`, `into_` by cost | `as_str`, `to_owned`, `into_inner` |
| Fallible conversion | `try_` prefix | `try_from_payload` |
| Boolean predicate | Question-shaped | `is_expired`, `has_capacity` |

- Prefer expressive names over abbreviations. `attempt_id` is clearer than `att`; `candidate_revision` is clearer than `cand_rev`.
- Avoid names that restate the type (`project_string`). Include units where a bare integer would be ambiguous (`timeout_ms`, `expires_at_ms`).
- Do not name a type or function after an implementation detail that may change.
- Public API names are a contract: renaming requires updating all in-repo callers and docs atomically.

## 3. Modules and crates

- Organize by capability, not by file kind. A `provider` module owns provider policy and adapters together; do not scatter one feature across `models/`, `services/` and `utils/`.
- Keep `lib.rs` a thin module map. Put behavior in named modules.
- Keep `main.rs` / binary entry points thin: parse environment, construct dependencies, start supervised tasks, translate top-level errors, exit non-zero on failure.
- Avoid `utils` and `helpers` catch-alls. Name the responsibility (`git_identity`, `bounded_reader`).
- A crate boundary is justified by independent compilation, a security boundary, a reusable tool, or a dependency graph that must be enforced. Do not split for aesthetic symmetry.
- Visibility is a design tool: default to private, expose the minimum `pub(crate)`/`pub` surface, and make invariant-bearing fields private with validated constructors.
- No cyclic module dependencies. If two modules need each other, move the shared policy to a third module or pass a narrow value across the boundary.

## 4. API design

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) where they apply.

- Accept borrowed inputs (`&str`, `&Path`, `&[T]`) when ownership is unnecessary; return owned values only when the caller needs them or an invariant requires it.
- Implement standard traits where they are free and correct: `Debug`, `Clone` only when meaningful, `PartialEq`/`Eq` for value types, `Error`, `From` for lossless conversions, `Default` only when a default is valid domain state.
- Do not derive `Serialize`/`Deserialize` on domain types that must never cross an untrusted boundary. Use explicit transport DTOs and convert at the boundary.
- Seal traits that are not intended for external implementation. Avoid exposing internal generics in public signatures.
- Prefer `impl AsRef<Path>`/`impl IntoIterator` at API edges, not in domain signatures where the concrete type is part of the meaning.
- Document every public item that carries an invariant, a security assumption, a failure mode or a lifecycle obligation. `missing_docs` is a review expectation for public contract crates, not automatically enabled everywhere.
- Do not expose mutable internals. Return domain results, not raw rows, handles or connection objects.
- New public API requires a caller and an acceptance test. Speculative public surface is technical debt.

## 5. Domain modeling

Use strong types for identities that must not be interchangeable:

```rust
pub struct ProjectId(Uuid);
pub struct ActionId(Uuid);
pub struct CampaignId(Uuid);
pub struct RunId(Uuid);
pub struct AttemptId(Uuid);
pub struct WorkerId(String);
pub struct LeaseToken(String);
pub struct CandidateRevisionId(String);
```

- Newtypes protect against argument transposition and accidental cross-entity use. Derive only the traits the identity needs; keep construction behind a validating constructor when the value has a format.
- Represent closed alternatives with enums, not magic strings. Persist an explicit, stable wire representation and reject unknown values on ingress.
- Parse untrusted input into validated domain values once at the boundary (`TryFrom<Request>`), then pass typed values inward. Do not repeat string validation in handlers.
- Frozen specifications, digests and evidence identities are immutable value types. A revision gets a new identity; do not mutate history.
- Model state transitions with explicit preconditions and explicit errors. A transition function should either produce the next state or a typed refusal, never a silently coerced state.
- Avoid a giant struct with many unrelated `Option` fields representing lifecycle states. Use enum states with state-specific data, and use typestate only where compile-time enforcement materially reduces risk without excessive generics.
- Critical authority transitions are never only UI state, caller-supplied booleans or model claims. Core persists and validates them.
- Do not use floating point for money, budgets, durations or counters. Use integer units and checked arithmetic.

## 6. Ownership and borrowing

- Prefer references and borrowing when ownership transfer is unnecessary. Clone only when the value is genuinely needed beyond the borrow or crosses a task boundary.
- Do not default to `Arc<Mutex<_>>`. Narrow mutable state, name its owner and synchronize at the smallest scope.
- Never hold a blocking mutex guard across `.await`. Use `tokio::sync::Mutex` only when the lock must be held across an await; otherwise use `std::sync::Mutex` with a short critical section.
- Avoid `RefCell`, `RwLock` and atomics without a concrete concurrency requirement and documented invariant.
- No global mutable state for Core authority. Pass explicit state into functions and tasks.
- RAII is appropriate for files, locks and guards, but `Drop` is not proof that an external process, container or provider session stopped. Asynchronous and durable cleanup requires explicit reconciliation.
- Prefer iterators where they clarify a transformation; use explicit loops where control flow, fallibility or early exit is clearer.
- `Cow` is justified when borrowing is the common case and ownership is the exception, not as a generic performance ornament.

## 7. Errors and failure classification

Use `Result<T, E>` and `?`. Library and domain code defines structured errors:

```rust
#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("lease is expired")]
    Expired,
    #[error("stale generation {observed}; expected {expected}")]
    StaleGeneration { observed: i64, expected: i64 },
    #[error("persistence failure: {0}")]
    Persistence(#[from] rusqlite::Error),
}
```

- Use `thiserror` for typed, matchable domain/library errors. Use `anyhow` only at binary or application boundaries where a rich context chain is useful, never to erase a domain distinction the caller must handle.
- Preserve enough information to distinguish: validation; authentication/authorization; transient provider failure; protocol incompatibility; budget exhaustion; cancellation; timeout; persistence conflict; stale lease/fencing token; incomplete cleanup; genuine execution failure.
- Do not use `unwrap()`, `expect()` or `panic!()` on recoverable production paths. `expect` is acceptable only when the invariant is local, proven and documented, and the panic is a true programmer error rather than an input-dependent condition.
- Tests may use explicit `unwrap`/`expect` when the failure is a test bug; test evidence must not depend on production panics.
- Do not swallow errors with `.ok()`, `let _ =`, default fallbacks or ignored join results when doing so changes correctness, cleanup or evidence. If an error is intentionally non-fatal, log it with structured context and record the residual risk.
- Errors must not include secrets, tokens, raw prompts, source bodies or unredacted environment values. Redact at the boundary where context is attached.
- `From` conversions preserve meaning. Do not map distinct failures into one opaque variant merely for convenience.
- Exit codes and HTTP statuses derive from typed failure classes, not string matching.

## 8. Async and concurrency

Use Tokio according to its [graceful shutdown guidance](https://tokio.rs/tokio/topics/shutdown) and the [async book](https://rust-lang.github.io/async-book/).

Every long-lived task has:

- a named owner;
- a bounded lifetime or explicit lifecycle;
- cancellation handling;
- error observation (never a silently detached `tokio::spawn`);
- shutdown behavior;
- cleanup and reconciliation responsibilities.

- Prefer structured concurrency: `JoinSet`, tracked `JoinHandle`s, `CancellationToken`, `TaskTracker` or an owned supervisor. A detached task whose failure cannot be observed is a defect.
- On shutdown, stop accepting work, cancel or drain owned tasks, await completion with a bound, then reconcile durable and external state. Log incomplete cleanup honestly; never claim a clean stop.
- Do not hold database transactions, locks or borrows across external network calls or long model execution.
- Do not block the async executor with synchronous filesystem, process or CPU-heavy work. Use `spawn_blocking` or a dedicated thread where the cost is material; use async process APIs for supervision.
- Bound concurrency, queues, retries, request sizes, output buffers and resource consumption. Every loop that can grow needs a limit or a measured reason it cannot.
- Treat cancellation as a state transition with possible external effects. A cancellation request is not proof that a provider turn or container stopped; reconcile before retry.
- Use monotonic generations or fencing tokens to reject stale execution results. Never let a late report overwrite a canceled, expired or newer attempt.
- Avoid `select!` bias surprises: make cancellation and timeout precedence explicit and tested.
- Timeouts on all external interactions. Record whether a timeout means unknown external state, not automatic failure.

## 9. Resource and process lifecycle

- Supervise child processes explicitly. Capture bounded stdout/stderr, enforce a lifetime, handle nonzero exit, and never inherit an unbounded sensitive environment by default.
- Use argument-based spawning (`Command::arg`/`.args`) with no shell interpolation of untrusted values. Avoid `sh -c` unless the command is a fixed, reviewed constant.
- Kill-on-drop is a safety net, not a cleanup guarantee. Reconcile process groups, containers and provider sessions durably.
- Files, directories and lock files have an owner, a canonical path and a cleanup policy. Verify ownership before deleting; never clean an unowned path.
- Durable cleanup records survive process death. Reconciliation must distinguish live owner, confirmed absent, orphaned and unresolved states.
- Provider sessions, containers and workspace paths are external resources. Their identities are recorded before launch so a crash can be classified.

## 10. Security

- `unsafe` is forbidden in first-party application code by default. A narrow exception needs a written explanation of the safety invariants, a minimal scope, and independent review.
- No shell interpolation of untrusted arguments. Validate and canonicalize filesystem paths at authorization boundaries; reject traversal, symlink escape and unauthorized roots.
- Keep provider credentials outside uncontrolled agent workspaces. Use least-privilege execution and scoped, expiring credentials.
- A Git worktree isolates revisions, not OS resources or credentials. Do not call it a sandbox.
- Unknown confinement or credential handling is `BLOCKED`. Never silently fall back to host execution when isolation is required.
- Fail closed on authorization, budget, isolation and evidence uncertainty. A missing denial is not permission.
- Redact secrets at the logging/telemetry boundary. Do not put raw prompts, tokens, private keys or full environment maps in ordinary telemetry.
- Treat all provider, web, tool and model output as untrusted input. Model output never grants privileges, budgets, tools or acceptance.
- Validate content hashes and identities at trust boundaries; do not trust a caller-supplied SHA, risk class or independence claim.

## 11. Database transactions

- Core owns durable state. All authoritative transitions are transactional and idempotent by operation and payload identity.
- SQLite runs in WAL mode with a bounded busy timeout and explicit transaction boundaries. Do not leave a transaction open while awaiting network or provider work.
- Use `BEGIN IMMEDIATE` for claim/reservation paths that must serialize writers and avoid upgrade deadlocks.
- A transaction either commits the full invariant (state, event, idempotency record, budget debit) or rolls back. Never emit an event or debit a budget outside the transition that justifies it.
- Idempotency keys are bound to the operation and a canonical payload digest. The same key with a different payload is a conflict, not a replay.
- Migrations are append-only and versioned. Do not rewrite persisted history or silently reinterpret old rows. A breaking schema change requires an explicit migration decision and test.
- Use checked arithmetic for generations, counters and budgets; reject overflow rather than wrapping.
- Read models may be derived, but the authoritative decision comes from normalized state and immutable evidence.
- Test concurrent claims, duplicate receipts, stale generations, restart recovery and rollback behavior against the real SQLite engine.

## 12. Serialization and external protocol boundaries

- Use explicit transport DTOs. Do not serialize internal domain enums, database rows or provider SDK types directly to external clients unless they are intentionally the contract.
- Mark request DTOs `#[serde(deny_unknown_fields)]` where silently accepting extra authority-bearing fields would be dangerous.
- Validate after deserialization: schema version, required identity, bounds, allowed enum values, digest and cross-field invariants. Deserialization is not authorization.
- Version external payloads. Unknown versions fail closed with a typed protocol-incompatibility error.
- Treat JSON-RPC IDs, SSE event IDs, session IDs and provider status strings as exact values. Do not infer success from a response, connection or stream open.
- Provider success requires documented terminal evidence (for example Codex `turn/completed` with `status=completed`, or reconciled OpenCode session/message state). A disconnect is unknown state, not completion.
- SSE parsing must be incremental, bounded and reconnect-aware. Preserve last-event identity and tolerate keep-alives without duplicating effects.
- Never log or persist raw credentials embedded in URLs, headers or handshakes.
- Fixture transcripts are protocol compatibility tests, not live provider certification.

## 13. Testing

Test observable behavior and invariants, not private implementation shape.

| Layer | Purpose |
|---|---|
| Unit | Pure domain policy, parsers, state transitions, bounded readers |
| Integration | Core/Worker contracts, SQLite transactions, provider transports, workspace/candidate creation |
| Process | Crash, restart, cancellation, cleanup, orphan reconciliation |
| Independent acceptance | Clean candidate, independent oracle, verifier and reviewer evidence |

- Every bug fix gets a regression test that fails on the repaired behavior.
- Test negative paths: invalid authorization, duplicate idempotency with a different payload, concurrent claims, expired leases, revoked delegation, provider disconnection, partial protocol responses, process termination without a terminal event, incomplete cleanup, unauthorized paths, dependency cycles, budget exhaustion and candidate SHA mismatch.
- Prefer deterministic fixtures and injected clocks over sleeps. A test that depends on wall-clock races is not evidence.
- Do not assert on incidental log text. Assert typed state, persisted rows, emitted events and exit behavior.
- Use bounded, explicit timeouts. A hanging test is a failure, not a slow pass.
- Tests may be author-run evidence but not independent acceptance. Do not treat a protocol stub as real-provider certification.
- Keep test fixtures immutable and versioned; a changed fixture changes the evaluation corpus and requires a new digest.

## 14. Documentation

- Explain invariants, trade-offs, security assumptions and non-obvious intent. Do not narrate what the code already says.
- Every module with a boundary or lifecycle obligation starts with `//!` documentation stating what it owns and what it does not.
- Public functions document failure modes, cancellation behavior, cleanup responsibility and idempotency where applicable.
- ADRs record decisions and consequences; contracts record testable invariants; the handoff records candidate-bound state. Do not duplicate normative rules across all three.
- Avoid stale examples. Update docs and callers in the same change as a behavior or interface change.
- Document deliberate lint exceptions next to the code and in the review evidence.

## 15. Dependency management

- The workspace is locked (`Cargo.lock`) and built with `--locked` in CI. Do not add a dependency without a real requirement and a review of maintenance, license, transitive size and security posture.
- Prefer the standard library and an existing reviewed dependency over a new crate for trivial functionality.
- Avoid duplicate crates providing the same capability. Consolidate when practical; document a justified exception.
- `cargo audit` is a required pinned CI gate. Evaluate `cargo-deny` for source/license policy before adopting it; do not add tools speculatively.
- Do not enable `feature = "full"` reflexively when a narrower feature set satisfies the requirement; however, do not fragment features into an unmaintainable matrix.
- Keep the pinned Rust version until a tested, justified toolchain change. A toolchain upgrade updates `rust-toolchain.toml`, `Cargo.lock` if needed, CI and evidence together.

## 16. Observability

- Use structured `tracing` fields. Include stable identities (`project_id`, `action_id`, `campaign_id`, `run_id`, `attempt_id`, `worker_id`, `generation`, provider/model/session identity) rather than interpolated prose.
- Log at the decision boundary: accepted, refused, retried, timed out, canceled, reconciled. Include the typed failure class.
- Redact secrets, prompts and personal data at emission. Never rely on a downstream collector to protect them.
- Distinguish absence of evidence from a negative result. An unreported provider outcome is unknown.
- Telemetry is not authoritative state. Durable events and database records are.
- Avoid per-token or per-byte logging in hot paths; bound cardinality and rate where necessary.

## 17. Performance

- Measure before optimizing. Prefer clear code until a profile or a real budget shows a problem.
- Avoid needless allocation and cloning in hot paths, but do not contort an API to save an allocation on a cold path.
- Bound memory and output. Readers, channels, retries and caches have limits.
- Do not hold large payloads in memory when a streaming or incremental parser is practical.
- Model calls, container starts and process spawns are expensive external operations; batch, reuse or skip them when policy permits.
- A performance claim requires a measured result on the exact candidate and environment. Do not claim speed from design alone.

## 18. Review and refactoring

Review for correctness and invariants before style.

- Trace the change from entry point through domain, persistence, external effect and evidence. Can the reviewer see the exact authority and state transition?
- Check error classification, cancellation, ownership transfer, cleanup and rollback.
- Check that concurrency is bounded and supervised, and that no lock or transaction spans an await or network call.
- Check trust boundaries: validated input, no secrets in logs, no path escape, no host fallback.
- Check for duplicated policy and for abstractions without a second real consumer.
- Check tests for negative behavior, not just happy-path coverage.
- Refactor when it reduces a demonstrated change cost or risk. Do not invent universal line-count or cyclomatic-complexity thresholds; use review judgment and targeted, measurable lints.
- Remove dead code and superseded compatibility paths in the same coherent change. Keep Git history for provenance rather than keeping dead branches alive.
- Record residual risk explicitly; do not hide it behind a green test run.

## 19. Tooling and required gates

Run from the repository root with the pinned toolchain (this environment uses `mise exec rust@1.90.0 --`):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
cargo doc --workspace --no-deps --locked
cargo audit --file Cargo.lock
```

- Keep the exact commands, tool versions, dependency count and advisory database commit in evidence.
- Clippy: start from the default `warn`/`-D warnings` posture and `clippy::all`. Add `correctness`, `suspicious` and selected `pedantic`/`restriction` lints only when they catch real defects. Do not blanket-enable `pedantic` or `restriction`, and do not disable a warning category to green CI.
- A scoped `#[allow(...)]` needs a reason, owner and, if persistent, a review reference. Never add a crate-wide blanket allow.
- `cargo-nextest` and `cargo-deny` may be evaluated for adoption; do not install or require them without a measured benefit and CI pinning.
- Formatting and Clippy are automated gates. Architectural cleanliness, naming and domain modeling remain review obligations; do not pretend a line-count lint enforces them.

## 20. Exceptions

A deviation from this standard must be explicit, narrow and reviewable:

1. State the rule and why the concrete case needs an exception.
2. Bound the exception to a module, function or line.
3. Document the invariant that keeps it safe.
4. Obtain independent review for security-relevant exceptions.
5. Revisit before cutover; an exception is not permanent policy.

An exception never grants authority, weakens authorization, permits host fallback for required isolation, or changes budget/quality policy. Those require a Product Owner decision and an ADR.

## 21. Definition of done for a Rust change

- The smallest coherent behavior is implemented, with callers and docs updated atomically.
- Domain identities, errors and state transitions are typed and explicit.
- Concurrency is owned, bounded, cancellable and observed; cleanup is truthful.
- Security boundaries are validated and logs are redacted.
- Formatting, strict Clippy, tests, build, docs and audit pass at the exact candidate SHA.
- New behavior has positive and negative tests; recovery behavior has a process-level test where applicable.
- Candidate-bound evidence and residual risk are recorded in the handoff.
- No obsolete compatibility path or dead code remains.
- Independent review is complete or explicitly pending; no gate is claimed without evidence.
