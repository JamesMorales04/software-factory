# Rust performance reference

## Measure before changing

- Use a benchmark (`criterion`/`divan`) or a trace, not intuition.
- Separate suite wall-clock (`nextest`) from code performance (`criterion`).
- Record hardware/container limits with the result.
- Optimize the measured bottleneck; do not add caches or `Arc<Mutex<_>>` as a
  default.

## Ownership and allocation

- Borrow at the boundary; return owned values only when the caller needs them.
- Avoid cloning a large value to satisfy a borrow conflict; narrow the borrow
  or move the value.
- `Cow` is for a measured borrow-common/own-rare case, not decoration.
- Prefer iterators for linear transforms; use explicit loops for fallible or
  early-exit control flow.
- Reuse buffers only when the lifetime is clear and the allocation is measured.

## Async

- Never hold a blocking mutex, transaction or DB connection across `.await`.
- Do not call blocking filesystem/CPU work on the async runtime; use
  `spawn_blocking` or a bounded worker.
- Bound channels and queues; unbounded is a memory outage waiting to happen.
- Name task ownership and cancellation. A dropped handle is not a stopped task.
- Avoid `select!` bias surprises; test cancellation precedence.
- Watch for N+1 queries and per-row provider calls; batch explicitly.

## Database

- Use one transaction for a multi-row invariant.
- WAL still allows one writer; a larger pool does not increase write throughput.
- Add indexes from query plans, not guesses.
- Bound query results and response bodies.
- Measure lock contention and busy retries; do not hide them with a longer
  timeout.

## Profiling tools

- `cargo flamegraph` for CPU.
- `tokio-console` for task stalls/blocking.
- `dhat` for allocation/leak investigation.
- SQLite `EXPLAIN QUERY PLAN` for query shape.
- `hyperfine` for CLI startup, not for code microbenchmarks.

## Review checklist

- Is there a measured claim?
- Is the fast path allocation-free where it matters?
- Is any lock held across await/I/O?
- Are channels/queues/results bounded?
- Is there an N+1 or per-row external call?
- Does the optimization preserve error, cancellation and cleanup semantics?
