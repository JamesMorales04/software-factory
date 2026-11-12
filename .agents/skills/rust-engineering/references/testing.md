# Rust testing reference

## Choose the oracle

- Unit test: pure function, parser, state transition, budget arithmetic.
- Property test: round-trip, idempotency, ordering, conservation, monotonicity.
- Snapshot: stable JSON/CLI/projection; redact volatile IDs and timestamps.
- Contract: Worker JSON-RPC, provider payloads, OpenAPI projections.
- Integration: real SQLite, filesystem, process or Compose boundary.
- E2E: only the small set of journeys that need the real composition.

A test name states the behavior; a test body proves it can fail.

## `cargo-nextest`

- Runs each test in a separate process, with per-test timeouts and JUnit output.
- Use a CI profile with `retries = 0`; a test that passes only on retry is flaky.
- Do not use nextest to hide shared-state or cleanup defects.
- Keep doctests in the normal `cargo test --doc` lane if nextest does not run
  them in your version.

## Property tests

Use `proptest` for algebraic laws and state machines:

```rust
proptest! {
    #[test]
    fn reservation_is_conserved(amount in 0u64..1_000_000, spend in 0u64..1_000_000) {
        let reserved = reserve(amount);
        let settled = settled(&reserved, spend);
        prop_assert!(settled.spent <= amount);
        prop_assert_eq!(settled.spent + settled.remaining, amount);
    }
}
```

Commit `proptest-regressions/` so a shrunk counterexample becomes permanent.

## Snapshot tests

- `insta` is appropriate for stable JSON, CLI output and canonical projections.
- Redact timestamps, random IDs, paths and credentials.
- Review the snapshot diff; do not run `INSTA_UPDATE=always` in CI.
- A snapshot is not a substitute for an explicit invariant.

## Mutation sampling

`cargo-mutants --in-diff` answers whether tests detect a behavior change.
Prioritize authorization, budgets, leases, state transitions, idempotency and
Quality Gate policy. A surviving mutant is a test gap or an equivalent mutant;
record which.

## Determinism

- Inject clocks, ID generators, ports and temp roots.
- Never use the real network/provider in a unit test.
- Namespace temp dirs; do not share a global registry.
- Assert cleanup ownership, not just cleanup success.
- A hanging test is a failure with a deadline, not a slow pass.
- Run new/changed E2E specs repeatedly; a single green run is not evidence.

## Negative cases

Always consider:

- invalid/empty/oversized input;
- unauthorized/forbidden/expired/revoked;
- stale lease/generation, duplicate request, replay;
- timeout/cancellation/partial effect;
- DB busy/locked/conflict;
- provider unknown outcome;
- cleanup incomplete.

## Evidence

Record the exact command, toolchain, feature flags, test filter, repeat count,
and whether the result was `PASS`, `FAIL`, `SKIPPED`, `BLOCKED` or `NOT_TESTED`.
