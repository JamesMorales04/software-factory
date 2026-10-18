# F0 contract inventory

**Status: proposed, not yet specified or certified.** This index is a contract-development checklist, not a set of completed APIs.

| Contract | Scope | Required adversarial scenarios |
|---|---|---|
| Project–Action | identities, frozen specs, risk, lifecycle | duplicates, cancellation, spec drift |
| Campaign–Run | task dependencies, attempts, ownership, budgets | retries, crashes, leases, contention |
| Core–Worker | registration, dispatch, heartbeat, result, cancellation | disconnects, replay, fencing, worker loss |
| Agent Session | Paseo provider/model, sandbox, capabilities, events | startup failure, session loss, timeouts, incompatibility |
| Quality–Delivery | candidate, evidence, independent review, PR/merge | pending CI, changed SHA, duplicate webhook, reclassified risk |
| Factory Events | persisted IDs, sequence, subscriptions, replay and RBAC | reconnects, duplicates, omissions, secret redaction |

Each completed contract must specify owner, version and stability level; input/output schemas and identities; state machine; authorization; idempotency; error handling; liveness/resource limits; telemetry; positive/negative acceptance cases; and compatibility policy. Do not invent RPCs or provider capabilities before F0 viability tests.

**Pre-release policy:** internal schemas can change incompatibly without shims; synchronize every caller, fixture and related test in one change. Any stable external or persisted-data contract requires explicit versioning and approval; see [ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).
