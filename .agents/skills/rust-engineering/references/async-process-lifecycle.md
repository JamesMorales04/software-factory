# Async and process lifecycle reference

## Task ownership

Every spawned task needs a recorded owner and a way to observe failure:

```rust
let mut tasks = JoinSet::new();
tasks.spawn(async move { supervise_worker(shutdown.clone()).await });
// shutdown
shutdown.cancel();
while let Some(result) = tasks.join_next().await {
    if let Err(error) = result {
        tracing::error!(%error, "task panicked");
    }
}
```

- A bare `tokio::spawn` whose `JoinHandle` is dropped hides panics and errors.
  Keep the handle, use `JoinSet`, or run under a tracked supervisor.
- `TaskTracker`/`CancellationToken` are appropriate when tasks must cooperate
  with shutdown. Define whether shutdown drains, aborts or leaves external work
  for reconciliation.
- Bound every accept loop, channel and retry. Backpressure is a design
  decision, not an afterthought.

## Shutdown sequence

1. Stop admitting new work (close listener/claim path).
2. Signal cancellation to owned tasks.
3. Await completion with a timeout.
4. Persist or reconcile any external effect whose state is unknown.
5. Report incomplete cleanup explicitly and exit non-zero if required.

A graceful shutdown that logs "complete" while a provider turn or container may
still run is a defect.

## External processes and containers

- Use argument-based `Command` spawning. Do not use a shell for untrusted
  arguments.
- Capture bounded stdout/stderr; a full pipe can deadlock a child.
- Apply a bounded lifetime and terminate the process group on timeout.
- `kill_on_drop` is best-effort. Record the process identity durably so a
  restart can reconcile it.
- A container or provider session is external state. Persist its intent and
  identity before launch, then classify on restart: live owner, confirmed
  absent, orphaned by dead owner, unresolved.
- Never delete a resource without verifying the ownership label/token.

## Cancellation

Cancellation is a state transition:

- The request is durable.
- The external effect may still be running.
- Reconcile provider/container state before retrying.
- A late success from a canceled attempt must not become authoritative.
- Use a generation/fencing token on every report and compare it atomically.
- Distinguish `cancel_requested` from `confirmed_stopped`; do not collapse them.

## Blocking work

Do not run synchronous filesystem scans, process waits, CPU-heavy parsing or
hashing above a small bounded size directly on the async executor. Use
`spawn_blocking` or a dedicated runtime/thread. Never hold a `std::sync::Mutex`
guard across `.await`.

## Timeouts and unknown state

Every network, provider, process and cleanup operation has a timeout. On
timeout, record `UNKNOWN_EXTERNAL_STATE`, not success or failure, unless
independent evidence proves the outcome. Retries must be policy-bounded and
idempotent.

## Shutdown test matrix

- Crash before launch.
- Crash after durable intent, before external identity is recorded.
- Crash after launch, before first progress event.
- Crash during terminal receipt.
- Cancellation while a provider turn is active.
- Worker restart with a live, expired and missing lease.
- Cleanup failure with the external process still alive.
- Duplicate shutdown and duplicate reconciliation.
