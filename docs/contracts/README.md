# F0 contract inventory

**Status: proposed, not yet specified or certified.** This index is a contract-development checklist, not a set of completed APIs.

## Frozen for F1 (implemented, review-bound)

| Contract | File | Slice | Covers |
| --- | --- | --- | --- |
| Project–Action | `project-action-s1.md` | S1 | identities, schema, lifecycle, auth, idempotency, errors |
| Factory Events | `factory-events-s1.md` | S1 | event schema, durability, replay transport, redaction |
| Run–Worker | `run-worker-s2.md` | S2 | Run/Attempt/Worker identities, lifecycles, dispatch, leases, fencing, recovery |
| Run–Paseo session identity | `run-paseo-s3a.md` | S3-A | execution modes, pinned routing, session binding lifecycle, intend-before-create, fencing, recovery |
| Agent observability projection | `agent-observability-s3c.md` | S3-C | cross-Run session listing, Agent/Run-detail projections, evidence levels, redaction |
| Paseo recovery and cleanup | `paseo-recovery-s3d.md` | S3-D | external correlation, recovery receipts, claim/complete protocol, retry gate, events, bounded cleanup |
| Frozen candidate quality pipeline | `quality-pipeline-s4a.md` | S4-A | ActionSpec, immutable candidate, independent verifier/reviewer, deterministic gate, browser checks |
| Native campaign orchestration | `campaign-orchestration-s4b.md` | S4-B | Core-owned Campaign/Task plan, native dispatch, repair, budgets, restart and Office projection |

The remaining F0 rows below (Agent Session, Quality–Delivery beyond S4-A/S4-B) are still proposed. Campaign–Run beyond S2's execution slice is now covered by the S4-B contract.

| Contract | Scope | Required adversarial scenarios |
| --- | --- | --- |
| Project–Action | identities, frozen specs, risk, lifecycle | duplicates, cancellation, spec drift |
| Campaign–Run | task dependencies, attempts, ownership, budgets | retries, crashes, leases, contention |
| Core–Worker | registration, dispatch, heartbeat, result, cancellation | disconnects, replay, fencing, worker loss |
| Agent Session | Paseo provider/model, sandbox, capabilities, events | startup failure, session loss, timeouts, incompatibility |
| Quality–Delivery | candidate, evidence, independent review, PR/merge | pending CI, changed SHA, duplicate webhook, reclassified risk |
| Factory Events | persisted IDs, sequence, subscriptions, replay and RBAC | reconnects, duplicates, omissions, secret redaction |

Each completed contract must specify owner, version and stability level; input/output schemas and identities; state machine; authorization; idempotency; error handling; liveness/resource limits; telemetry; positive/negative acceptance cases; and compatibility policy. Do not invent RPCs or provider capabilities before F0 viability tests.

**Pre-release policy:** internal schemas can change incompatibly without shims; synchronize every caller, fixture and related test in one change. Any stable external or persisted-data contract requires explicit versioning and approval; see [ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).
- [Multi-project registry, capabilities and browser capability (F1-S4-C)](multi-project-s4c.md) — Project-bound specs, capability matching, Factory-managed Playwright and cross-project rejection.
- [Bounded autonomous Implementer recovery (F1-S4-C-03)](campaign-recovery-s4c03.md) — failure classes, fenced recovery Runs, explicit attempt budgets and immutable oracle evidence.
- [Worker-managed browser test server (F1-S4-C-06)](browser-test-server-s4c06.md) — declarative startup, readiness, ownership, cleanup and evidence boundary.
