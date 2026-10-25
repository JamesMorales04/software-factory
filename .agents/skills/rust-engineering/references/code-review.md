# Rust code review reference

Use with `docs/engineering/rust-standards.md`. Review correctness and
invariants before style. A green test run does not prove authority, isolation,
cleanup or independent acceptance.

## Severity

- **Blocking:** authority/security violation, data loss, fabricated evidence,
  incorrect terminal state, stale execution accepted, secret disclosure,
  unbounded resource use, host fallback for required isolation.
- **Major:** missing negative test, unchecked overflow, unobserved task,
  transaction across await, cleanup not reconciled, duplicated policy path,
  ambiguous error classification.
- **Minor:** naming, unnecessary clone, missing doc on a non-obvious invariant,
  local simplification.

Record blocking findings until repaired or explicitly accepted by the
responsible authority. Do not call a review complete when findings are hidden.

## Checklist

### Contract and authority

- Is the decision maker explicit? Does Core alone own policy, budgets, leases,
  candidate acceptance and delivery?
- Can model, tool, UI or caller input grant authority it should not?
- Are frozen specs, digests, project roots and candidate identities validated
  rather than trusted from the caller?
- Is the change atomic with all in-repo callers, tests and docs?

### Domain modeling

- Are interchangeable identities distinguished by newtypes?
- Are closed states enums with stable persisted representations?
- Are transitions guarded by preconditions, with typed refusals?
- Is there one giant optional-field struct where state-specific data belongs?
- Is typestate earning its complexity?

### Ownership, concurrency and lifecycle

- Who owns each task, and how are its error and completion observed?
- Is concurrency bounded? Are queues, buffers, retries and output limited?
- Is a mutex, transaction or borrow held across `.await`?
- Does cancellation reconcile external effects, or only set a flag?
- Are stale generations and late receipts rejected?
- Is shutdown graceful, bounded and truthful about incomplete cleanup?

### Persistence and idempotency

- Does one transaction commit state, event, receipt and counter together?
- Is the idempotency key bound to the operation and payload digest?
- Does claim/reservation serialize concurrent writers?
- Are migrations append-only and tested?
- Can restart reconstruct the same authoritative state?

### Errors and security

- Does the error type distinguish validation, authz, transient, protocol,
  budget, cancellation, timeout, conflict, stale lease and genuine failure?
- Any production `unwrap`/`expect`/`panic`, ignored result or silent fallback?
- Any shell interpolation, unvalidated path, symlink escape, credential leak or
  unredacted telemetry?
- Does the code fail closed when isolation or evidence is unavailable?

### Tests and evidence

- Do tests assert observable behavior, including negative paths?
- Are fixtures deterministic and bounded? Is the clock injected?
- Is a stub being presented as real-provider certification?
- Is evidence bound to the exact candidate SHA and environment?
- Are cleanup, cost and provider identities reported truthfully?

### Complexity and maintainability

- Is each abstraction justified by a real boundary or second consumer?
- Is business policy duplicated across handler, worker and UI?
- Are names clear, modules cohesive and visibility minimal?
- Does a modified implementation file exceed roughly 600–800 lines, or
  1,000 lines, and if so does it mix unrelated change reasons? Was a split or a
  written cohesion justification considered?
- Does any function over roughly 100 lines mix independent policies, deep
  nesting, hidden side effects or unrelated error paths?
- Are transaction, provider/network and projection concerns separated?
- Are tests discoverable and organized, with large fixtures/scenarios in a
  dedicated test submodule when that improves navigation?
- Did the change leave dead code or a compatibility shim with no consumer?
- Are lint exceptions scoped and documented rather than blanket?
- Do not demand a split based on line count alone; challenge a small module
  with poor boundaries or excessive complexity just as strongly.
