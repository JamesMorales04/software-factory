# Infinity Runtime experiment — implementation record

- **Branch:** `experiment/infinity-runtime-20261102`
- **Base:** `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e`
- **Infinity pin:** `1abc6fcff1f186124f924b56aa92c3a3684c90c5`
- **Crate:** `infinity-experiment/`
- **Decision:** [ADR-0030](../../architecture/decisions/ADR-0030-infinity-runtime-experiment.md)

This is an implementation checkpoint, not production certification. No real
inference, no merge and no Wave 3 alteration were performed.

## Reproduce

```sh
cd infinity-experiment
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

scripts/restart_evidence.sh evidence/restart-evidence.json
scripts/compare.sh evidence/comparison-$(date +%Y%m%d)
cargo run --bin infinity-experiment -- subscription \
  --out evidence/subscription/feasibility-matrix.json
```

## Executed evidence

`cargo test` runs:

- `tests/journey.rs` — deterministic Director → 2 implementers → independent
  reviewer → candidate; asserts 3 effects completed, PASS review, no duplicate
  effects, and lifecycle events.
- `tests/durability.rs` — six tests: process restart after the review marker;
  ambiguous effect refusal; verified-marker recovery and exactly-once enqueue;
  32-way concurrent callback fencing; stale attempt rejection; fresh child
  context reconstruction; cancellation with no delivered effects.
- `tests/rap.rs` — Infinity → RAP invocation → idempotent server effect →
  callback → Infinity thread resume; duplicate invocation does not duplicate
  history or the external effect.
- `tests/subscription.rs` — matrix classification, default-deny attestation,
  OpenCode Go no-overage controls, fake billing-source rejection, and
  Codex-app-server agent-vs-completion semantics.

Evidence files:

- `evidence/restart-evidence.json` — crash exit 23; before resume 3 effects,
  1 APPLIED, 2 callbacks, 3 external applications; after resume 3 completed,
  3 delivered callbacks, still 3 external applications, 0 duplicate
  applications; candidate stable.
- `evidence/comparison-20261103/summary.md` and `summary.json` — measured
  prototype and Factory baseline process trees.
- `evidence/prototype-journey-report.json` — one full event/callback/effect
  report.
- `evidence/subscription/feasibility-matrix.json` — route evidence with
  official sources.

## Important defects/limits found

1. The pinned Infinity daemon persistence is **not** crash-safe: whole-file
   `std::fs::write`, ignored errors, no version/fsync/atomic rename. Upstream
   PR #178 (`e9dad5c`) is newer than the pin. The prototype therefore uses its
   own SQLx store.
2. The pinned runtime can leave orphaned tool calls after a restart; upstream
   `01e889e` is also newer than the pin. The prototype settles ambiguous
   effects with one explicit recovery-result message and refuses replay.
3. SQLite deferred transactions produced `SQLITE_BUSY_SNAPSHOT` (code 517)
   under concurrent appends. The store now uses `BEGIN IMMEDIATE` for
   append/spawn/effect claims. This was observed before the fix and is covered
   by the repeated concurrent tests.
4. Infinity’s local driver uses `Rc`/`LocalSet`; it is not a multi-tenant,
   Send-safe server runtime. A production embedding needs a per-session worker
   boundary and a durable input inbox.
5. Run judgment is not attached. `FAILED`/`BLOCKED`/`NOT_TESTED` distinctions:
   - live provider subscription consumption: **NOT_TESTED / BLOCKED**;
   - GPT-6 Luna independent review: **BLOCKED** (no inference authorization);
   - Podman/SEC-01 isolation: **NOT_TESTED** (local process fixture is not a
     sandbox);
   - Windows: **NOT_TESTED**;
   - production Office/Core integration: **NOT_IMPLEMENTED**.

## Checkpoint

The experiment is restartable from the branch and the commands above. The only
unfinished deliverable is the independent Luna review, which requires a
separate Owner authorization for a specific subscription-only inference.
