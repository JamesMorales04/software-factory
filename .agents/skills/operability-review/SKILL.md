---
name: operability-review
description: Review a Factory change for startup/readiness, graceful shutdown, cancellation versus external stop, recovery, resource bounds, observability, backup/restore and failure diagnostics. Use for Core/Worker/Supervisor/Verifier/runtime, Compose, environment builders, provider sessions and any change with a long-running process or durable state.
---

# Operability review

Use this skill as a read-only runtime review. It does not certify deployment or
isolation, and it cannot waive a required check or acceptance criterion.

## 1. Lifecycle

- Startup order: dependencies are validated before accepting work; failure is
  explicit and bounded.
- Readiness means the service can do its job, not merely that a port is open.
- Liveness does not restart a service that is making progress but slow.
- Shutdown stops new work, drains or cancels bounded work, releases leases and
  closes external sessions.
- A cancellation request is not a stopped process/container/provider turn.
  Reconcile the external effect before claiming cleanup.
- Restart preserves durable identity and does not duplicate side effects.
- Orphan reconciliation owns only its own resources and never another run's.

## 2. Bounds and backpressure

- Every queue, channel, retry, loop, response body and log line has a bound.
- Concurrency limits are explicit; no unbounded `spawn` per request.
- Timeouts exist at the network, provider, browser, check and stage layers.
- A timeout is a typed failure, not a slow pass.
- Backpressure rejects or queues with a visible state; it does not silently
  drop work.
- Disk/memory/CPU limits are declared and observed; a full disk is a failure.
- Budget reservations are atomic and released on every terminal path.

## 3. Failure classification

Distinguish:

- validation vs authentication vs authorization;
- transient network vs provider refusal vs protocol incompatibility;
- budget exhaustion vs cancellation vs timeout;
- conflict vs stale lease/generation;
- unknown outcome vs failed outcome;
- cleanup incomplete vs cleanup verified;
- degraded dependency vs total outage.

Do not collapse these into one opaque error or HTTP status.

## 4. Recovery

- Every durable state has a recovery path and an idempotency key.
- A crash between intent and effect is recoverable without duplicating the
  effect.
- A repair creates a new candidate identity; it never mutates the rejected one.
- A stale worker cannot reclaim an active lease or overwrite a newer candidate.
- Restart tests use the same volumes and assert the exact identity, not just
  “the process is up”.
- A failed recovery is reported as incomplete; it is not hidden by a retry.

## 5. Observability

- Health, readiness and dependency status are visible without reading logs.
- Structured logs include correlation IDs (project/action/campaign/run/attempt/
  candidate), but no secret, raw prompt or source body.
- Metrics cover queue depth, latency, failures by class, retries, budget and
  cleanup; telemetry export failure does not stop Core.
- Traces are bounded and redacted; a trace is not acceptance evidence by itself.
- The operator can answer: what is running, why, since when, with what budget,
  what failed and what remains.
- Diagnostics are bounded; no unbounded log tail or full response body.

## 6. Resource and environment review

- Compose services have healthchecks, restart policy, stop grace period,
  read-only root where possible, dropped capabilities and no host socket.
- Named volumes have an owner and a backup/restore story.
- Image tags/digests are pinned; a missing image is `BLOCKED`, not a silent
  fallback.
- Ports, temp dirs and project names are unique per run.
- The host is not required for normal product operation; container-native
  development remains the path.

## 7. Review checklist

- What happens on SIGTERM during a provider turn?
- What happens if the DB is locked/busy for longer than the timeout?
- What happens if the provider returns an unknown outcome?
- What happens if the browser/worker crashes after a partial write?
- What happens if GitHub returns 429/5xx after the branch push?
- What happens if the disk fills during a build or artifact write?
- What is preserved across restart, and what is intentionally disposable?
- How does an operator recover without editing the database by hand?
- Are cleanup and orphan checks idempotent and owner-scoped?

## 8. Evidence

```text
Candidate:  <SHA>
Runtime:    <Compose services and versions>
Lifecycle:  <startup/readiness/shutdown/cancel/restart>
Failure:    <injected failures and observed classes>
Bounds:     <timeouts/queues/limits>
Recovery:   <how and proof>
Cleanup:    <verified / incomplete / blocked>
Observability:<health/logs/metrics/trace>
Residual:   <NOT_TESTED / BLOCKED>
```

## 9. Stop conditions

Stop and report if:

- a cancellation or timeout can be mistaken for a successful stop;
- cleanup can delete another run's data;
- a resource is unbounded;
- a failure is silently retried or hidden;
- the only recovery is manual database editing;
- the change weakens a health/readiness/cleanup requirement.

## 10. References

- [Container-native engineering](../container-native-engineering/SKILL.md)
- [Rust async/process lifecycle](../rust-engineering/references/async-process-lifecycle.md)
- [Delivery and GitHub](../delivery-and-github/SKILL.md)
- [Browser automation](../../../docs/engineering/browser-automation.md)
- [Reliability debt](../../../docs/quality/reliability-debt.md)
