# Quality Wave 1/2 consolidation handoff

**Status:** consolidated implementation candidate — **not independently accepted,
certified, deployed or merged at the time of writing**. This document is the
handoff for the next agent. It records the objective, what is included, what was
verified, and the remaining work. It does not replace the
[quality-first planning status](quality-first-planning-status.md), the
[delivery/environment program](delivery-environment-program.md) or the
[capability registry](../product/capabilities.yaml).

- **Consolidation branch:** `fix/environment-recompile`
- **Runtime candidate:** `9b2345b6ac2ad14424b57afc58d39e13eeddb5ca`
- **Docs-only commit after the runtime candidate:** this handoff. It does not
  change runtime code, Compose, migrations, tests or evidence claims.
- **Base:** `main` at `5d2ade627e06d881e56ac60a225b47884ed0bd5a`
  (merge of PR #96)
- **Worktree used:** `/home/james/Escritorio/Development/sf-quality-wave12/live-envretry`
- **Security:** `TRUSTED_LOCAL`; not isolated and not certified. No paid
  inference was used for the consolidation or the deterministic harness.

## 1. Objective

Complete the remaining Wave 1 and Wave 2 work in
[quality-first-delivery-plan.md](quality-first-delivery-plan.md) and the
follow-on [delivery/environment program](delivery-environment-program.md), then
close the gaps found by the live vlog Campaign:

1. Durable workflow, idempotency, outbox and recovery.
2. Planning route, Planner, Spec Manager, plan review and stack/architecture/
   documentation/skill artifacts.
3. Dependency supply chain, provider reliability, workspace recovery and
   OpenSpec projection.
4. Finding Router, reviewer fleet, zero-finding Gate and Office plan-review
   surfaces.
5. Risk-tiered delivery and issue-linked lifecycle.
6. Agnostic environment compilation with Nix/devenv, a containerized builder,
   frozen check execution and greenfield stack selection.
7. Live-test repairs for triage, planning readiness, Gate observations,
   provider accounting, workspace/check staging and delivery.

The Factory remains the authority: Core owns plans, hashes, budgets, candidates,
findings, delivery and acceptance. Model output is advisory and never grants
authority, tools or acceptance.

## 2. What is consolidated

The branch contains more than 280 commits over `main`, including the Quality
Wave 12 integration and the subsequent live-environment work. The principal work
packages are:

| Area | Status | Main evidence |
| --- | --- | --- |
| WP0 contracts, schema, migrations | Implemented | `native/src/planning/contracts.rs`, migrations through `0078` (with documented gaps) |
| WP1 durable workflow/outbox | Implemented | workflow/checkpoint/idempotency tests, restart recovery |
| WP2 planning route/Planner/Spec Manager | Implemented | deterministic lanes, mandatory plan review, bounded auto-approval |
| WP3 stack/architecture/docs/skills | Implemented | stack precedence, skill packs, refresh scheduler, Office Skills |
| WP4 dependency supply chain | Implemented through broker boundary | vendor/cache, SBOM/license inventory, egress proxy, migration `0069` |
| WP5 provider/structured-output reliability | Implemented | SSE reconnect, reservation policy, schema repair, chaos matrix |
| WP6 workspace/execution reliability | Implemented | content-addressed snapshots, owner-scoped reconciliation, locks |
| WP7 OpenSpec renderer/validator/archive | Implemented | canonical renderer, pinned CLI `1.16.1`, archive/drift |
| WP8 Finding Router | Implemented | durable dispositions, zero-finding Gate, owner resolve/refute |
| WP9 reviewer fleet/Gate | Implemented | six identities, role coverage, exact assignments |
| WP10 Office plan review | Implemented | Director/Request plan, environment, finding and trace surfaces |
| WP11 observability/trace | Implemented | bounded trace, replay, redaction, alerts |
| WP12 tests/chaos/browser | Implemented and run | deterministic acceptance 20/20 on this candidate; Chromium matrix 39/39 on the WP12 integration candidate, not rerun after the environment work |
| Risk-tiered delivery | Implemented | R0 auto, R1 opt-in, R2/R3 manual, incidents, final source |
| Issue-linked lifecycle | Implemented | frozen links, derived states, comment/close receipts |
| Agnostic environment compiler | Implemented | conventions, plan/spec, Lead review, licence gate, freeze |
| Nix/devenv builder + broker | Implemented | bundle import, digest/ID verification, resource/network policy |
| Frozen environment checks | Implemented | Supervisor/Worker/Verifier broker path, cancellation, diagnostics |
| Greenfield stack selection | Implemented | one bounded model call, exact catalog option, environment proposal |
| Operator plan/environment recompile | Implemented | immutable revisions, typed conflicts, owner decision for stack changes |

### Consolidation fixes in this branch

- `f213555` closes the final WS-EnvRetry review: exact language/framework
  coverage, `argv[0]`-only command evidence, deferred dependency-plan
  attachment, and Office recompile visibility for retryable frozen specs.
- `581fa90` registers the new `environment-builder-bundle` one-shot service in
  the static Compose security policy and adds negative tests.
- `999170c` fixes the Compose interpolation of the Core builder-digest wait
  loop, widens the bounded wait, adds `raw.githubusercontent.com` to the Nix
  fetch allowlist, and corrects the stale builder-allowlist documentation.
- `9b2345b` restores the deterministic dispatch and negative E2E suites under
  the environment binding: test-support binaries apply the fixture binding and
  use the generic check path when no real environment execution row exists,
  while production and unit tests still fail closed; legacy candidates with no
  frozen reviewer role requirement accept role receipts; fixture verifier and
  reviewer principals align with the configured quality identity.
- `823c1a8` adds a scoped repo-hygiene exemption for Go command data in the
  language-pack catalog while continuing to reject Go invocations in Factory
  runtime code, and normalizes the already-rendered Compose config in the
  acceptance harness without re-interpolating escaped shell variables.

## 3. Verification actually run

On the exact clean candidate `9b2345b`:

```text
scripts/quality-wave12-acceptance.sh --require-clean
PASS: 20
FAIL: 0
SKIPPED: 0
RESULT: PASS
```

The harness covers `fmt`, full library tests, all bin tests, targeted
integration suites, workflow/quality/trace/review-role/Gate filters, three
provider-chaos repeats, Clippy, Office install/test/typecheck/lint, base and
OpenSpec Compose config, and both Compose policy suites.

Additional local results:

```text
cargo test -p factory-native --lib
979 passed; 0 failed; 4 ignored

cargo test -p factory-native --bins
all bin suites passed (core 67, runtime broker 75, worker 34, supervisor 14,
verifier 11, architecture check 8, repo hygiene 7, and the remaining bins)

cargo test -p factory-native --features test-support --bin factory-core
92 passed; 0 failed

cargo test -p factory-native --features test-support --bin factory-worker
34 passed; 0 failed

cargo test -p factory-native --features test-support --bin factory-verifier
11 passed; 0 failed

cargo xtask e2e recovery
1 passed; 0 failed

cargo xtask e2e dispatch
12 passed; 0 failed

cargo xtask e2e negatives
5 passed; 0 failed

cargo xtask e2e provider
1 passed; 0 failed

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean

cd office && npm test -- --run
50 files / 366 tests passed

npm run typecheck
clean

npm run lint
clean (173 files)
```

The four ignored library tests are opt-in real-Podman tests. They are
`SKIPPED`, not passes.

### What this verification does not prove

- It is implementer/orchestrator evidence, not independent acceptance.
- It does not start the installed Compose product or run a paid model.
- It does not prove a real Nix/devenv build or check in the installed stack.
- It does not prove a real multi-reviewer Campaign or real GitHub delivery.
- It does not prove every Office route, locale or viewport.

### Known dependency blocker

`cargo audit --file Cargo.lock` fails only `RUSTSEC-2026-0459` /
`CVE-2026-119375` (`libsqlite3-sys` 0.30.1 via sqlx 0.8.6). The patched
`libsqlite3-sys` 0.39.x cannot be selected because sqlx 0.8.6 and 0.9.0 require
`libsqlite3-sys <0.38.0`. The owner decision in
[RUSTSEC-2026-0459](../security/RUSTSEC-2026-0459-sqlite-wait.md) is to wait for
a patched sqlx release and keep the audit gate unsuppressed. The `rust` and
`container-dev` CI jobs therefore fail at the audit step. This is a known
temporary dependency blocker, not a clean audit or a security certification.
Do not add a permanent audit ignore without a new owner decision.

## 4. What remains

The next agent should treat these as the acceptance backlog, in priority order.

### P0 — full live UAT after consolidation

Run the two-project live test requested by the owner, from a clean database and
a clean target project:

1. **Calculator:** the model chooses the stack; verify the greenfield stack
   selection, environment proposal, Lead environment review, frozen checks and
   a follow-up request that adds a Vue web view.
2. **Pokemon:** React + Express consuming the Pokemon API and rendering images;
   a follow-up that adds a Pokedex view.
3. Exercise all triage options and non-specific messages; verify the agents make
   bounded decisions and use the intended roles.
4. Verify the Office flow end to end: intake, plan review, environment review,
   Campaign, findings, Gate, delivery and issues.
5. Verify GitHub issue creation/linking, PR creation, R0 auto-merge, R1 opt-in,
   post-merge checks and issue close/comment behavior. Create and push the
   target projects to GitHub first.
6. Verify Node/Playwright checks are provided by the frozen environment, not a
   host image or a hardcoded Node assumption.
7. Verify the UI is coherent in English and Spanish, at desktop/tablet/mobile
   widths, with no overflow or unnamed controls.

Do not use paid inference without a fresh authorization envelope. Use the real
browser for the operator journey; do not replace it with direct API calls.

### P1 — real container-native environment evidence

- Run a real `resolve` and `build` through the Compose runtime broker with a
  real project `EnvironmentPlan`/`EnvironmentSpec`, exact SBOM/output hashes and
  a warm repeat.
- Run a real frozen `check` through the Supervisor/Worker/Verifier broker path,
  including a Node/Playwright check, cancellation, timeout, over-budget and
  cleanup confirmation.
- Prove there is no host wrapper, host Podman socket, generic Rust image
  fallback or arbitrary image path.
- Reconcile the orphan `feat/environment-runtime-broker` branch
  (`5f98dae`, `f0d7572`). It contains an isolated real resolve/build evidence
  record and an older allowlist/prepull change. The evidence is useful, but the
  code is stale relative to this branch; do not merge it blindly.

### P1 — independent acceptance and reviewer fleet

- Run a real Compose Campaign with the six reviewer identities and confirm the
  Gate aggregates exact role receipts.
- Obtain independent verifier/reviewer acceptance and Product Owner visual
  approval for the Office surfaces.
- Do not claim `ACCEPTED`, `MERGED` or `DEPLOYED` from this handoff.

### P2 — delivery and issue lifecycle

- Real authorized GitHub delivery: issue link/comment/close, push, PR, R0
  auto-merge, R1 opt-in, post-merge check, incident freeze and manual revert.
- Verify the issue lifecycle hooks are called from the PR-open and merge paths;
  the deterministic tests and fake-upstream coverage are not a live journey.
- Verify a partial issue set stays open and a complete set closes only after the
  exact merge commit.

### P2 — residual product gaps

- Plan Compile before Lead assessment: the UI/Core fix is in `6b861de`, but the
  pre-assessment path was not live tested.
- Stagnation salvage attribution: deterministic, but not proven for a real
  accepted candidate.
- Model-call resolution/release and old test-data cleanup: use the guarded
  reset/resolution paths; do not delete evidence or live state.
- Full WCAG 2.2 AA, keyboard focus order, contrast, screen-reader traversal,
  Spanish locale and 320 px layouts.
- Deployment, Windows, independent security review and `SEC-01-ISOLATED`
  remain out of scope for the current `TRUSTED_LOCAL` product.

## 5. Guardrails for the next agent

- Work in a fresh worktree/branch from the merged `main`; do not reuse this
  worktree for unrelated changes.
- Keep Core as the only authority. Model output never selects tools, widens
  scope, approves a plan, closes a finding, accepts a candidate or merges.
- Use the canonical rootless Compose topology. No host-only runtime dependency,
  host Podman socket, privileged mode, host networking or Compose bypass.
- Preserve exact candidate/base/head identity in every evidence record.
- Do not relax a frozen specification, test, budget, sandbox or risk class to
  make acceptance pass.
- Report `PASS`, `ACCEPTED`, `MERGED` or `DEPLOYED` only with exact evidence.
  Distinguish `NOT_TESTED`, `BLOCKED`, `SKIPPED` and `NOT_APPLICABLE`.
- Re-run the affected acceptance after every repair and bind the new candidate.

## 6. Useful commands

```text
# Deterministic implementation harness
scripts/quality-wave12-acceptance.sh --require-clean

# Rust
cargo fmt --all -- --check
cargo clippy -p factory-native --all-targets --all-features -- -D warnings
cargo test -p factory-native --lib
cargo test -p factory-native --bins
cargo test -p factory-native --features test-support --bin factory-core

# Office
cd office
npm test -- --run
npm run typecheck
npm run lint

# Compose policy
docker compose -f compose.yaml config --format json > /tmp/compose.json
python3 .github/scripts/check-compose-policy.py /tmp/compose.json
COMPOSE_CONFIG_JSON=/tmp/compose.json python3 .github/scripts/test_check_compose_policy.py
```

## 7. Key references

- [Quality-first delivery plan](quality-first-delivery-plan.md)
- [Quality-first planning status](quality-first-planning-status.md)
- [Delivery and environment program](delivery-environment-program.md)
- [Agnostic environment compiler](environment-compiler-core.md)
- [Environment runtime broker](environment-runtime-broker.md)
- [Environment plan contract](../contracts/environment-plan.md)
- [Delivery policy contract](../contracts/delivery-policy.md)
- [Issue lifecycle contract](../contracts/issue-lifecycle.md)
- [ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md),
  [ADR-0040](../architecture/decisions/ADR-0040-risk-tiered-delivery-and-auto-merge.md),
  [ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md),
  [ADR-0042](../architecture/decisions/ADR-0042-issue-linked-delivery-lifecycle.md)
- [Office parity matrix](../product/office-parity.json)
- [Capability registry](../product/capabilities.yaml)

## 8. Delivery state

- Branch: `fix/environment-recompile`
- Runtime candidate: `9b2345b6ac2ad14424b57afc58d39e13eeddb5ca`
- PR: `#97` against `main`; merge only after the PR checks pass and the exact
  head is re-verified.
- Merge method: merge commit, preserving the work-package history.
- Known blocker: `RUSTSEC-2026-0459` keeps the `cargo audit` step failing by
  owner decision; the `rust` and `container-dev` jobs are expected to stop
  there. This is not a clean audit.
- Post-merge: rerun the deterministic harness on `main` and record the merge
  commit; the merge is not deployment or independent acceptance.
