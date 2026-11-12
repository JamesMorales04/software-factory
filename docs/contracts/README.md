# F0 contract inventory

**Status:** These contracts define Rust-era protocol and state invariants. The current implementation is governed by [ADR-0018](../architecture/decisions/ADR-0018-native-rust-replacement.md), the [migration closure](../implementation/native-rust-closure.md) and the [capability registry](../product/capabilities.yaml). This index is a protocol map, not certification by itself. Legacy provider-intermediary contracts are archived under [docs/evidence/history/contracts](../evidence/history/contracts).

## Frozen contract expectations (implementation status varies)

| Contract | File | Slice | Covers |
| --- | --- | --- | --- |
| Project–Action | `project-action-s1.md` | S1 | identities, schema, lifecycle, auth, idempotency, errors |
| Factory Events | `factory-events-s1.md` | S1 | event schema, durability, replay transport, redaction |
| Run–Worker | `run-worker-s2.md` | S2 | Run/Attempt/Worker identities, lifecycles, dispatch, leases, fencing, recovery |
| Agent observability projection | `agent-observability-s3c.md` | S3-C | cross-Run session listing, Agent/Run-detail projections, evidence levels, redaction |
| Frozen candidate quality pipeline | `quality-pipeline-s4a.md` | S4-A | ActionSpec, immutable candidate, independent verifier/reviewer, deterministic gate, browser checks |
| Native campaign orchestration | `campaign-orchestration-s4b.md` | S4-B | Core-owned Campaign/Task plan, native dispatch, repair, budgets, restart and Office projection |
| Agent Office and interactive WorkGraph | `agent-office-s4.md` | S4 | participant/tool-event projections, redaction, filters, visual Office and dependency graph |
| Environment plan and spec | `environment-plan.md` | WP0 / program | ProjectConventions, Nix/devenv plan, Lead approval, license gate, frozen EnvironmentSpec, builder sandbox and evidence |
| Risk-tiered delivery | `delivery-policy.md` | WP0 / program | DeliveryPolicy, exact-head merge, R0/R1/R2/R3 eligibility, fresh checks, idempotency, incidents and manual revert |
| Issue-linked delivery lifecycle | `issue-lifecycle.md` | WP0 / program | frozen issue links, six lifecycle states, PR comment, close-after-merge and partial-issue handling |
| Intake follow-up and merged base lineage | `intake-follow-up.md` | WP0 / program | one child per terminal request, exact profile/policy continuity, no parent evidence reuse, and verified default-branch base advance |

The WP0 program contracts are interfaces for the
[delivery and environment program](../implementation/delivery-environment-program.md)
and its [language-agnostic audit](../implementation/language-agnostic-audit.md).
The intake follow-up contract extends the unified-intake and delivery lifecycle
without adding a second authority.
Issue lifecycle and risk-tiered delivery have native implementations (migrations
0072/0073/0075/0077/0078); the environment compiler has Core planning, freeze and
check binding (migration 0076) but its production runtime-broker builder wiring
is still pending. Capability and parity states remain the source of truth.

The remaining F0 rows below (Agent Session, Quality–Delivery beyond S4-A/S4-B) are still proposed. Legacy session-identity and recovery contracts are archived; their invariants must be re-expressed against direct provider adapters when provider execution is implemented.

| Contract | Scope | Required adversarial scenarios |
| --- | --- | --- |
| Project–Action | identities, frozen specs, risk, lifecycle | duplicates, cancellation, spec drift |
| Campaign–Run | task dependencies, attempts, ownership, budgets | retries, crashes, leases, contention |
| Core–Worker | registration, dispatch, heartbeat, result, cancellation | disconnects, replay, fencing, worker loss |
| Agent Session | direct provider/model, sandbox, capabilities, events | startup failure, session loss, timeouts, incompatibility |
| Quality–Delivery | candidate, evidence, independent review, PR/merge | pending CI, changed SHA, duplicate webhook, reclassified risk |
| Factory Events | persisted IDs, sequence, subscriptions, replay and RBAC | reconnects, duplicates, omissions, secret redaction |

Each completed contract must specify owner, version and stability level; input/output schemas and identities; state machine; authorization; idempotency; error handling; liveness/resource limits; telemetry; positive/negative acceptance cases; and compatibility policy. Do not invent RPCs or provider capabilities before F0 viability tests.

**Pre-release policy:** internal schemas can change incompatibly without shims; synchronize every caller, fixture and related test in one change. Any stable external or persisted-data contract requires explicit versioning and approval; see [ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).
- [Multi-project registry, capabilities and browser capability (F1-S4-C)](multi-project-s4c.md) — Project-bound specs, capability matching, Factory-managed Playwright and cross-project rejection.
- [Bounded autonomous Implementer recovery (F1-S4-C-03)](campaign-recovery-s4c03.md) — failure classes, fenced recovery Runs, explicit attempt budgets and immutable oracle evidence.
- [Worker-managed browser test server (F1-S4-C-06)](browser-test-server-s4c06.md) — declarative startup, readiness, ownership, cleanup and evidence boundary.
- [Semantic review (S4-D)](semantic-review-s4c.md) — Core-authorized model review, strict schema, findings and deterministic gate boundary.
