# Quality-first planning implementation status

**Status:** implementation integrated on
`factory/quality-wave12-integration` through WP3, WP4, WP5, WP6, WP7, WP9 and
WP11; **not certified and not accepted**. WP10 implementation and WP12
browser/independent acceptance remain pending. WP12 adds a deterministic
chaos/cross-workstream matrix and a repeat-run acceptance script; its
candidate-bound results are implementer evidence only. This document records
what is implemented, what was tested, and what remains unverified. It is not
acceptance evidence and does not claim `TRUSTED_LOCAL` isolation. Exact merge
commits, conflicts and validation results are in
[Wave 12 integration evidence](wave12-integration-evidence.md).

- Branch: `factory/quality-wave12-integration`
- WP12 branch: `factory/quality-wp12-chaos-matrix` (base `452bb40`)
- WP12 matrix: [deterministic chaos and cross-workstream matrix](wp12-chaos-matrix.md)
- Baseline: `bcb8b5b` (`factory/agentic-flow-20261108`)
- Merged work packages: WP3 `35fcae7`, WP5 `bd81c27`, WP7 `0b7fc3d`, WP9
  `55c43e8`, WP11 `fbc105c`, WP6 `b5dd282`, WP4 `6cde43c`
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
| WP3 | Implemented and integrated | Existing/user stack decisions, architecture projection, tiered documentation plan, source-cited skill packs with FRESH/DUE/REFRESHABLE/STALE/MISSING classification, separate source-check and content-review evidence (15 packs content-reviewed; remaining source-availability-only packs block), a bounded Core refresh scheduler with durable idempotent events and a durable rotating project cursor, broader stack detection/catalog, Definition of Done, per-document checks, migrations `0067`/`0068`, and the Office Skills panel | Content review of the remaining catalog packs; real Compose scheduler/restart evidence; independent browser acceptance of the Skills refresh surface |
| WP4 | Implemented and integrated through the broker boundary | Lockfile/vendor/SBOM/license/advisory evidence binding, Cargo source-replacement parsing, private staging fetch/vendor, atomic promotion/rollback, aggregate bounds, timeout/output bounds, pinned cargo/rustup, `POST /v1/dependencies/acquire`, internal deny-by-default egress network with an allowlisted CONNECT proxy, pinned image ID verification, Supervisor operation grants, Worker pre-check dispatch, per-workspace lock, authorization guard, migration `0069`, and a fail-closed rejection of FIXTURE execution with a dependency plan | Real Compose dependency acquisition and independent acceptance |
| WP5 | Implemented and integrated | Bounded SSE reconnect, no hidden 300-second turn deadline, reservation settle/release policy, bounded structured-output repair policy, deterministic reconnect/duplicate/lifecycle chaos matrix, and no candidate on any provider failure, cancellation or reconnect exhaustion | Live provider long-turn/SSE-cut UAT and independent acceptance |
| WP6 | Implemented and integrated | Content-addressed workspace snapshots, generated-target exclusion, symlink/unsafe-archive rejection, per-owner runtime-broker credentials (supervisor/verifier/provider derived only from the authenticated token), owner-scoped startup orphan-volume/container reconciliation, bounded inventory/inspect streams, serialized per-resource create/delete with one lock identity per name, active-execution registry, cancellation-confirmed cleanup and real rootless-Podman orphan/foreign-owner probes | Compose end-to-end restart and HTTP inventory acceptance; independent acceptance |
| WP7 | Implemented and integrated | Canonical `## ADDED Requirements` / `### Requirement:` / `#### Scenario:` and numeric task IDs; pinned OpenSpec CLI `1.16.1` validator and container image; profile-gated Compose validation job against a Core-rendered fixture; Core-owned idempotent archive transition and drift/immutability checks | Real Core-backed Compose run against a projection produced by the model planning pipeline |
| WP8 | Implemented | Deterministic Finding Router, durable OPEN dispositions, Gate blocks missing/OPEN findings even on reviewer PASS, authenticated resolve/refute, durable owner-decision validation, immutable closed dispositions | Browser acceptance of finding actions |
| WP9 | Implemented and integrated | Role definitions, bounded role prompts, identity-based independence validation, six-service Compose reviewer fleet with per-identity tokens/principals/roles, reviewer identity registry, authenticated role binding, participant-plan role preservation, exact frozen role-to-principal assignment set, fail-closed Gate when assignments are missing/unexpected or a receipt is roleless/unrequired, Director per-role receipts, migrations `0065`/`0066` | Real two-reviewer Campaign/E2E with the fixture provider; independent acceptance |
| WP10 | Implemented; acceptance pending | Director plan-review panel, lane/artifact/stack/architecture/docs/skills/DoD/OpenSpec summaries, approve/request-changes/reject, finding list and disposition form | Real Core/Compose browser acceptance and visual review |
| WP11 | Implemented and integrated | Deterministic replay trace, bounded steps/events, redaction, comparison, typed alerts, operator-authenticated `GET /agentic/requests/{id}/trace` and the Office Request trace view | Real Core/Compose browser acceptance; independent acceptance |
| WP12 | Deterministic matrix implemented and run; acceptance pending | Reproducible chaos/fault-injection and cross-workstream matrix, repeat-run acceptance script, golden planning-pipeline integration test, workflow/provider/workspace/dependency/reviewer/OpenSpec/trace tests, existing Rust/Office suites and Compose policy checks | Independent acceptance and real Core/Compose browser acceptance of the WP10-owned Office surfaces; live/paid/provider/Podman paths remain `NOT_TESTED` |

## Verification run

Combined integration branch `factory/quality-wave12-integration`; exact commands
and per-suite results are recorded in
[Wave 12 integration evidence](wave12-integration-evidence.md).

From `native/`:

```text
cargo test -p factory-native --lib
830 passed; 0 failed; 4 ignored

cargo test -p factory-native --bins
all binary suites passed (Core 38, Supervisor 9, Verifier 7, Worker 19,
runtime broker 29)

cargo test -p factory-native --test planning_pipeline --test provider_chaos \
  --test dependency_broker_http --test openspec_compose_validation \
  --test migration_skill_refresh
29 passed; 0 failed

cargo test -p factory-native --test dependency_broker_podman \
  --test dependency_acquire_live
2 passed; opt-in real-Podman/live-registry bodies skipped without their
environment variables

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

From `office/`:

```text
npm test -- --run
317 passed across 46 files

npm run typecheck
clean

npm run lint
clean
```

Compose policy:

```text
check-compose-policy.py (base)                 PASS
check-compose-policy.py (openspec-validation)  PASS
test_check_compose_policy.py                   36 tests passed (base, 8 skipped)
test_check_compose_policy.py                   36 tests passed (profile, 5 skipped)
```

Migration smoke: fresh and upgrade-from-0064 databases apply migrations
`0065`-`0069` exactly once in order.

### WP12 acceptance run

Candidate `ec4519ec293f0fd8d49f94ee6a2ff52e9143210d` (clean tree
`25d72c49e61ff9b5cfced8012a666ade924c1d50`) was exercised with
`scripts/quality-wave12-acceptance.sh --require-clean`. Exact results and the
per-step logs are recorded in the
[WP12 manifest](../evidence/quality-wave12-chaos-2026-11-10/manifest.json).

```text
scripts/quality-wave12-acceptance.sh --require-clean
20/20 steps PASS; 0 failed; 0 skipped steps; 236 s

scripts/quality-wave12-acceptance.test.sh
PASS (PASS/PARTIAL/FAIL results; missing/stale/fresh --no-install dependencies)

cargo test -p factory-native --locked --lib
830 passed; 0 failed; 4 ignored

cargo test -p factory-native --locked --bins
125 passed; 0 failed

targeted integration suites
29 passed; 0 failed
(planning 1, provider chaos 16, dependency HTTP 6, OpenSpec 5, migration 1)

focused filters
workflow 16; quality 12; trace 17; review_roles 9; gate_ 18; all passed

provider_chaos repeat
3/3 runs; 16 passed each; 0 failed

cargo clippy -p factory-native --locked --all-targets --all-features -- -D warnings
clean

cd office && npm test -- --run
317 passed across 46 files

npm run typecheck
clean

npm run lint
clean (163 files)

podman compose -f compose.yaml config
PASS (14 services)

podman compose --profile openspec-validation -f compose.yaml config
PASS (15 services)

compose policy (base/profile)
PASS/PASS; 36 policy tests each (8/5 profile-specific skips)
```

The other flaky-sensitive integration suites and the focused filters were
repeated three times each after the script; all passed. Only a full run can
print `RESULT: PASS`; `--skip-office` or `--skip-compose` produce
`RESULT: PARTIAL` with exit 3, and `--no-install` fails closed when Office
dependencies are missing or stale. `scripts/quality-wave12-acceptance.test.sh`
asserts those control-flow cases without running the full suite. The four
ignored library tests are the opt-in real-Podman tests and are `SKIPPED`, not
passes.
Live provider inference, real Compose dependency acquisition, a real
multi-reviewer Campaign, a real Core-backed OpenSpec projection, browser
acceptance and independent acceptance remain `NOT_TESTED`/`BLOCKED` as listed
in the manifest. No paid inference was used.

Independent read-only Codex reviews at the work-package review-fix commits
reported no remaining findings for WP0, WP1, WP2, WP3, WP4, WP7, WP8, WP9 and
WP10. The earlier WP9 review recorded the one-reviewer Compose fleet as a
declared limitation; WP9 now provisions the six-identity fleet and this
document does not claim that a live multi-reviewer Campaign has run.

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
- Dependency acquisition is authenticated with the Supervisor owner token only;
  the broker reuses its owner-label check, per-resource lock and
  active-execution registry before launching a credential-free container on a
  dedicated internal network. The container has no direct route, the CONNECT
  proxy allows only configured registry/GitHub hosts, the imported image ID is
  re-verified before each acquisition, and the offline check container remains
  `--network=none`. Core authorizes the exact dependency plan before Campaign
  start; the Worker acquires after provider writes and before offline checks.
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

- Live provider inference and a live long-turn/SSE-cut UAT (WP5).
- Real Core/Compose browser acceptance for plan review, finding actions, the
  Skills refresh panel and the workflow trace view (WP10/WP12).
- Independent acceptance and Product Owner visual approval.
- A real Compose Campaign with two or more reviewer role receipts and the Gate
  aggregating them; only deterministic DB/plan/Gate coverage was exercised.
- OpenSpec Compose validation against a projection produced by the real Core
  planning pipeline (the profile-gated job itself is implemented and exercised
  against a Core-rendered fixture; see
  [WP7 evidence](wp7-openspec-compose-evidence.md)).
- A real Compose dependency acquisition through the internal network and
  allowlisted proxy; only deterministic HTTP and opt-in live test bodies were
  exercised.
- Compose end-to-end workspace restart/inventory recovery (WP6).
- Real Compose scheduler restart evidence for skill-pack refresh (WP3).
- The deterministic WP12 chaos/cross-workstream matrix and acceptance script are
  recorded in [wp12-chaos-matrix.md](wp12-chaos-matrix.md) and the
  [WP12 manifest](../evidence/quality-wave12-chaos-2026-11-10/manifest.json);
  those are implementer evidence, not independent acceptance. Real
  Core/Compose browser acceptance of the WP10 surfaces, live provider/Podman
  and dependency paths, a real multi-reviewer Campaign and the per-option
  browser matrix remain `NOT_TESTED`.
