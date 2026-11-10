# Quality-first planning implementation status

**Status:** implementation slice complete through the listed work packages; **not
certified**. This document records what is implemented, what was tested, and
what remains unverified. It is not acceptance evidence and does not claim
`TRUSTED_LOCAL` isolation.

- Branch: `factory/agentic-flow-20261108`
- Implementation commit: `ae7182e`
- Review-fix commits: `29ee61c`, `d774d3e`, `ae7182e`, `185ca99`, `a086d02`,
  `30e6ad2`, `0133318`, `feff8ed`
- Design authority: [ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md)
- Plans: [quality-first planning](../implementation/quality-first-planning-plan.md),
  [delivery](quality-first-delivery-plan.md),
  [reliability](agentic-reliability-plan.md)

## Work package status

| WP | Status | Implemented | Remaining |
|---|---|---|---|
| WP0 | Implemented | Typed planning contracts, validation, migrations `0060`, `0061`, `0063`, `0064`; exact plan/artifact hashes; authenticated owner attribution | — |
| WP1 | Implemented | Durable workflow state machine, revisions, steps, checkpoints, outbox, idempotency, restart recovery, conditional reclaim/finish fence | Purge job for expired idempotency rows |
| WP2 | Implemented | Deterministic lane floors, Planner every lane, Spec Manager `FORMAL_SPEC`, bounded prompts, planning artifacts, mandatory plan review, policy-bounded auto-approval, authenticated owner review, workflow transitions | Browser acceptance of the review surface |
| WP3 | Implemented for this slice | Existing/user stack decisions, architecture projection, tiered documentation plan, source-cited skill packs with FRESH/DUE/REFRESHABLE/STALE/MISSING classification, separate source-check and content-review evidence (15 packs content-reviewed; remaining source-availability-only packs block), a bounded Core refresh scheduler with durable idempotent events and a durable rotating project cursor, broader stack detection/catalog, Definition of Done, per-document checks | Content review of the remaining catalog packs; real Compose scheduler/restart evidence; independent browser acceptance of the Skills refresh surface |
| WP4 | Implemented at the library boundary | Lockfile/vendor/SBOM/license/advisory evidence binding, Cargo source-replacement parsing, private staging fetch/vendor, atomic promotion/rollback, aggregate bounds, timeout/output bounds, pinned cargo/rustup | Wiring the broker acquisition into the runtime-broker HTTP/Compose path |
| WP5 | Implemented | Bounded SSE reconnect, no hidden 300-second turn deadline, reservation settle/release policy, bounded structured-output repair policy | Full provider/worker fault-injection matrix and live long-turn UAT |
| WP6 | Existing implementation verified | Content-addressed workspace snapshots, generated-target exclusion, symlink/unsafe-archive rejection, cleanup paths | Startup orphan-volume reconciliation and a dedicated chaos run |
| WP7 | Implemented | Canonical `## ADDED Requirements` / `### Requirement:` / `#### Scenario:` and numeric task IDs; pinned OpenSpec CLI `1.16.1` validator and container image; profile-gated Compose validation job against a Core-rendered fixture; Core-owned idempotent archive transition and drift/immutability checks | Real Core-backed Compose run against a projection produced by the model planning pipeline |
| WP8 | Implemented | Deterministic Finding Router, durable OPEN dispositions, Gate blocks missing/OPEN findings even on reviewer PASS, authenticated resolve/refute, durable owner-decision validation, immutable closed dispositions | Browser acceptance of finding actions |
| WP9 | Implemented for this slice | Role definitions, bounded role prompts, identity-based independence validation, six-service Compose reviewer fleet with per-identity tokens/principals/roles, authenticated role binding, participant-plan role preservation, exact frozen role-to-principal assignment set, fail-closed Gate when assignments are missing/unexpected or a receipt is roleless/unrequired, and per-role receipt status in Director | Real two-reviewer Campaign/E2E with the fixture provider; independent acceptance |
| WP10 | Implemented for this slice | Director plan-review panel, lane/artifact/stack/architecture/docs/skills/DoD/OpenSpec summaries, approve/request-changes/reject, finding list and disposition form | Real Core/Compose browser acceptance and visual review |
| WP11 | Implemented at the projection boundary | Deterministic replay trace, redaction, comparison, event/payload bounds | Core/Office trace view wiring |
| WP12 | Partial | Golden planning-pipeline integration test, existing Rust/Office suites, workflow and provider unit tests | Chaos/fault-injection matrix and per-option browser matrix for the new surfaces |

## Verification run

From `native/`:

```text
cargo test -p factory-native --lib
745 passed; 0 failed; 2 ignored

cargo test -p factory-native --test planning_pipeline
1 passed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

From `office/`:

```text
npm test -- --run
301 passed across 45 files

npm run typecheck
clean

npm run lint
clean
```

One Office full-suite run reported a single transient failure; subsequent
consecutive reruns passed 297/297. The failure was not reproduced and is not
claimed as fixed.

Independent read-only Codex reviews at the review-fix commits reported no
remaining findings for WP0, WP1, WP2, WP3, WP4, WP7, WP8, WP9 and WP10. The
earlier WP9 review recorded the one-reviewer Compose fleet as a declared
limitation; WP9 now provisions the six-identity fleet and this document does not
claim that a live multi-reviewer Campaign has run.

## Security and authority

- Core remains the only authority for planning lanes, plan approval, finding
  closure, budgets, candidate acceptance and delivery.
- Model output is advisory. It cannot lower a deterministic lane floor, approve
  its own plan, close its own finding, refute a finding or accept work.
- Plan reviews bind the exact plan revision and SHA-256. Owner approval derives
  the principal from the authenticated device session.
- A refutation requires a durable resolved owner decision by the same
  authenticated owner, scoped to the request or project; the reviewer principal
  is preserved from the receipt.
- Dependency acquisition scrubs ambient credentials and pins the cargo/rustup
  toolchain. Network allowlisting remains the broker network namespace/egress
  policy's responsibility.
- Reviewer independence is identity, process, token and authenticated principal.
  Core derives the reviewer principal and role from trusted identity
  configuration and rejects a receipt whose principal or role does not match;
  a caller cannot choose its principal. Required role coverage and one receipt
  per role are enforced by the Gate. A required role set without an exact frozen
  principal assignment, with an unexpected assignment role, or with a roleless
  or extra-role receipt, blocks acceptance instead of being counted by role name
  alone. Provider/model equality is not a rejection criterion.
- The Compose topology provisions six reviewer services with separate tokens,
  principals, roles and state volumes. The plan fails closed with
  `REVIEWER_INDEPENDENCE_REQUIRED` when the configured identities cannot staff
  every required role. Model diversity remains only a non-blocking observation.
- No isolation or deployment certification is claimed.

## Not tested / open

- Live provider inference and a live long-turn/SSE-cut UAT.
- Real Core/Compose browser acceptance for plan review and finding actions.
- Independent acceptance and Product Owner visual approval.
- A real Compose Campaign with two or more reviewer role receipts and the Gate
  aggregating them; only deterministic DB/plan/Gate coverage was exercised.
- OpenSpec Compose validation against a projection produced by the real Core
  planning pipeline (the profile-gated job itself is implemented and exercised
  against a Core-rendered fixture; see
  [WP7 evidence](wp7-openspec-compose-evidence.md)).
- Runtime-broker HTTP wiring for dependency acquisition.
- Startup orphan-volume reconciliation and the full chaos matrix.
