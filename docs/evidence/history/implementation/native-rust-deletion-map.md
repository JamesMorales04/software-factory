# Rust cutover deletion and replacement map

**Baseline:** `102f6a046f1f907546e8bc60da622388f80bd95c`; 99 tracked `.go` files. This is a live work map, not acceptance evidence. The closure ledger remains authoritative.

| Group | Classification and replacement | Behavioral oracle / missing work | Ready now? | Owner | Verification |
|---|---|---|---|---|---|
| B1 | Retired — Go architecture checker → `native/src/bin/architecture_check.rs`; governance CI | Eight Rust checker tests, base-ref check and `architecture-consistency` GitHub job passed at `6947b42` | Deleted | Luna/Sol | `cargo test --bin factory-architecture-check --locked`; run checker with `--base-ref origin/main`; governance CI |
| B2 | Small or bounded Rust regression gaps — project/candidate/recovery/event helpers → `native/src/{db,workspace,quality}.rs` and process tests | Canonical identity, conflict/candidate lineage, failure classification, event projection negatives; exact assertions still to port | No | Sol/Luna | `cargo test --workspace --locked`; `cargo xtask e2e recovery` |
| C1 | Substantial runtime behavior — CLI/Core/Worker/domain/scheduler/fixture execution → Rust Core, Worker, Supervisor and `factory` | Provider work lifecycle, WorkGraph failure/restart, cancellation, fenced concurrent execution, real use | No | Luna | `cargo test --workspace --locked`; `cargo xtask e2e recovery`; fixture dispatch E2E |
| C2 | Substantial authority and state — Go API/store/quality → Rust SQLx Core and verifier/reviewer | S1–S4 HTTP/event parity, independent reviewer, cross-project security, restart/receipts | No | Luna/Sol | Rust process/API E2E, quality and security tests |
| C3 | Substantial managed-browser parity — Go browser server → Rust Supervisor/browser capability | Browser lifecycle, ownership, timeout and process-group cleanup under Rust | No | Luna | `cargo xtask e2e office`; browser process fault tests |
| C4 | Substantial telemetry/security parity — Go logging/tracing → Rust structured telemetry | Secret redaction, exporter outage, no source-body leaks, log canaries | No | Luna | Rust process telemetry negatives and log inspection |
| D1 | Obsolete-by-design Paseo adapter and session types → direct Codex/OpenCode Rust provider sessions | Direct provider lifecycle and recovery plus independent isolation; do not port the intermediary | No | Luna | Provider protocol fixtures, security E2E; independently assessed isolation |
| E1 | Retired — legacy Go protected-file fixture → neutral protected-file fixture | S4-B B8 protected-diff HUMAN_REQUIRED and no-repair oracle passed at `1d10f06` | Deleted | Sol | `mise exec go@1.26.9 -- bash scripts/e2e-s4b.sh` (B8 passed, unrelated B11 failed) |

At this baseline, **no complete Go package is deletion-ready**. Existing Rust component tests do not by themselves replace process, browser or security assertions. Remove each group only with the named oracle passing on its exact candidate.

## Current blocker assessment (2026-10-24, 94 tracked `.go` files)

The inventory is now 94 files after batches 1–3. Batch 3 retired the B2
Go tests for `recovery.Classify` and `project.Inspect` after their exact
assertions were ported to `native/src/recovery.rs` and
`native/src/repository.rs`; both Go packages still build with `[no test files]`.
No production Go file-level batch is truthful yet. The remaining Go tree is one connected module: the B2 `domain`,
`candidate`, `project` and `recovery` packages are imported by the C1/C2
commands, `api`, `store`, `orchestrator`, verifier and reviewer. Removing any
B2 file without removing or rewriting its dependents breaks `go build` and the
legacy CI oracles.

Rust now owns the following former B2 behavior at `4bcd584`:

- pre-candidate failure classification (`native/src/recovery.rs`);
- bounded candidate diff, diff-stat, added-line and content-digest evidence
  (`native/src/workspace.rs`);
- independent deterministic final-diff review and receipt submission
  (`native/src/review.rs`, `native/src/bin/reviewer.rs`);
- canonical repository root-identity binding and base-tip validation
  (`native/src/repository.rs`, migration 0010);
- a closed durable event-kind vocabulary (`native/src/db/mod.rs`);
- bounded Core/Supervisor log credential redaction (`native/src/telemetry.rs`);
- campaign cancellation of a queued fan-in integration row and both lease
  generations in the DB-level test (`8f1be47`).

C4 telemetry still lacks OTLP export/outage behavior and a process-level secret
canary. The legacy event payload read projection has no production Rust equivalent
because Rust events store only bounded detail strings, never a `diffExcerpt`
payload. Any future payload field must reintroduce that projection test. Even
after these ports, deletion requires retiring the whole dependent Go runtime
and CI in one verified cutover. The next safe deletion batch is therefore the
full legacy module retirement, not another isolated file.

## Completed batch 1

Commit `1d10f065a2143cbe8fc84006113ec315610420bd` deleted the E1
`policy.go` test datum and replaced it with `policy.txt` in the same protected
path prefix. The S4-B deterministic oracle passed its B8 protected-diff,
`HUMAN_REQUIRED` and no-repair assertions on that code commit. The complete
script failed at the separate B11 Chromium Campaigns-heading wait; it is not a
whole-suite pass. Tracked Go count: **99 → 98**. E1 is retired; the exact-file
list below remains the 99-file baseline inventory.

## Completed batch 2

Commit `c2923aca1ae58df92f623b1670e40c0175273a39` deleted the B1 Go
architecture checker (`main.go`, `main_test.go`) and its Go wrapper, ported its
registry/evidence/reference/certification checks to Rust, and switched the
`architecture-consistency` job to the Rust binary. Sol's review repair at
`eb36cade9749614742ad0b0cf7b823173abdc43f` confines evidence and manifest
reads and rejects manifest fragments. Independent review found missing legacy
regression assertions; `2cdd773` and `4fb29e9` restored claim, manifest,
reference and dependency negatives. A later parser-negative test brought the
checker to eight focused Rust tests. Clippy, an actual base-ref checker
invocation, and the `architecture-consistency` GitHub job on `6947b42`
passed. Final independent acceptance remains pending. Tracked Go count:
**98 → 96**. B1 is retired.

## Legacy CI and scripts (not counted as `.go` files)

`.github/workflows/{s1,s2,s3a,s3b,s3c,s3d,s4a,s4b,s4c}.yml` and Go/Paseo `scripts/e2e-*.sh`, `scripts/*paseo*`, `core/go.mod`, `core/go.sum` and Go lint configuration remain active. The governance workflow now invokes Rust but still installs Go for the legacy live-entrypoint guard. The replacement is `.github/workflows/native-rust.yml`, Rust `cargo xtask` process/browser lanes, and the Rust architecture checker. Retire each script and workflow with its equivalent assertions; preserve required check names until protection is reviewed. The `main` branch currently reports no branch-protection required-check configuration (GitHub API 404); this does not authorize reducing verification.

## Exact tracked Go files by group

### B1 (2)

- `core/cmd/architecturecheck/main.go`
- `core/cmd/architecturecheck/main_test.go`

### B2 (8)

- `core/internal/candidate/git.go`
- `core/internal/candidate/git_test.go`
- `core/internal/domain/event.go`
- `core/internal/domain/event_test.go`
- `core/internal/project/repository.go`
- `core/internal/recovery/classify.go`

### C1 (35)

- `core/cmd/factory-core/main.go`
- `core/cmd/factory-reviewer/main.go`
- `core/cmd/factory-verifier/main.go`
- `core/cmd/factory-worker/candidate.go`
- `core/cmd/factory-worker/capabilities.go`
- `core/cmd/factory-worker/capabilities_test.go`
- `core/cmd/factory-worker/main.go`
- `core/cmd/factory-worker/main_test.go`
- `core/cmd/factory-worker/recovery.go`
- `core/cmd/factory-worker/recovery_test.go`
- `core/cmd/factory/lifecycle.go`
- `core/cmd/factory/logs.go`
- `core/cmd/factory/main.go`
- `core/cmd/factory/main_test.go`
- `core/cmd/factory/state.go`
- `core/cmd/factory/worker_env_test.go`
- `core/cmd/factory/worker_lifecycle.go`
- `core/internal/domain/action.go`
- `core/internal/domain/action_test.go`
- `core/internal/domain/campaign.go`
- `core/internal/domain/campaign_test.go`
- `core/internal/domain/candidate.go`
- `core/internal/domain/errors.go`
- `core/internal/domain/project.go`
- `core/internal/domain/project_test.go`
- `core/internal/domain/run.go`
- `core/internal/domain/run_test.go`
- `core/internal/domain/spec.go`
- `core/internal/domain/spec_test.go`
- `core/internal/exec/lock_other.go`
- `core/internal/exec/lock_unix.go`
- `core/internal/exec/synthetic.go`
- `core/internal/exec/synthetic_test.go`
- `core/internal/orchestrator/orchestrator.go`
- `core/internal/orchestrator/orchestrator_test.go`

### C2 (31)

- `core/internal/api/api_test.go`
- `core/internal/api/campaign_api_test.go`
- `core/internal/api/campaign_handlers.go`
- `core/internal/api/handlers.go`
- `core/internal/api/hub.go`
- `core/internal/api/paseo_api_test.go`
- `core/internal/api/paseo_handlers.go`
- `core/internal/api/project_handlers.go`
- `core/internal/api/quality_api_test.go`
- `core/internal/api/quality_handlers.go`
- `core/internal/api/run_api_test.go`
- `core/internal/api/run_handlers.go`
- `core/internal/api/server.go`
- `core/internal/api/static.go`
- `core/internal/quality/gate.go`
- `core/internal/quality/gate_test.go`
- `core/internal/quality/risk.go`
- `core/internal/store/actions.go`
- `core/internal/store/campaigns.go`
- `core/internal/store/campaigns_test.go`
- `core/internal/store/events.go`
- `core/internal/store/projects.go`
- `core/internal/store/projects_test.go`
- `core/internal/store/quality.go`
- `core/internal/store/quality_test.go`
- `core/internal/store/recovery.go`
- `core/internal/store/recovery_test.go`
- `core/internal/store/runs.go`
- `core/internal/store/runs_test.go`
- `core/internal/store/sqlite.go`
- `core/internal/store/store_test.go`

### C3 (4)

- `core/internal/browserserver/process_other.go`
- `core/internal/browserserver/process_unix.go`
- `core/internal/browserserver/server.go`
- `core/internal/browserserver/server_test.go`

### C4 (4)

- `core/internal/telemetry/env.go`
- `core/internal/telemetry/log.go`
- `core/internal/telemetry/redact_test.go`
- `core/internal/telemetry/tracing.go`

### D1 (14)

- `core/cmd/factory-worker/paseo_core.go`
- `core/cmd/factory-worker/paseo_evidence.go`
- `core/cmd/factory-worker/paseo_task.go`
- `core/cmd/factory-worker/paseo_task_test.go`
- `core/internal/domain/paseo.go`
- `core/internal/domain/paseo_test.go`
- `core/internal/paseo/adapter.go`
- `core/internal/paseo/cli.go`
- `core/internal/paseo/cli_test.go`
- `core/internal/paseo/fake.go`
- `core/internal/paseo/procsys_other.go`
- `core/internal/paseo/procsys_unix.go`
- `core/internal/store/paseo.go`
- `core/internal/store/paseo_test.go`

### E1 (1)

- `scripts/fixtures/s4a-ui/core/internal/quality/policy.go`

Inventory checksum: 99 files at the original baseline. Current count after batches 1–3: **94**. Regenerate classification after each deletion batch and compare with `git ls-files "*.go" | wc -l`.
