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

The remaining F0 rows below (Campaign–Run beyond S2's slice, Agent Session, Quality–Delivery) are still proposed.

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
