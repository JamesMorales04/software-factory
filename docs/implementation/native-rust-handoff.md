# Native Rust implementation handoff

**Date:** 2026-10-23 UTC. **Repository:** `JamesMorales04/software-factory`. **Branch:** `native-rust-replacement-20261023`. **Base and entry HEAD:** `102d7ba29295554bd61ae71956347a025d219d7b` (`origin/main`). Run `git rev-parse HEAD` for the current tip; the tip cannot be embedded into its own commit. The entry S4-D checkout was dirty at `9c6aa4a` with seven tracked modifications and was left untouched. This branch was created as a separate worktree at `/home/james/Escritorio/Development/software-factory-native-rust`. Its own working tree should be checked with `git status --short --branch` after checkout.

## Binding objective and decisions

The Product Owner authorized a clean Rust Core/Worker replacement with direct Codex App Server and OpenCode server integration, React/TypeScript Office, no completed Go/Paseo/AEH active product, no speculative compatibility layer, and no host fallback for required isolation. [ADR-0018](../architecture/decisions/ADR-0018-native-rust-replacement.md) supersedes the old implementation choice. Core alone owns policy, risk, authorization, budgets, leases, evidence, gate and delivery. A model cannot grant authority. This branch is **not** a completed replacement and must not be merged as a cutover.

Decision mechanisms: canonical Git identity, idempotency, API auth and current non-dispatch are `DETERMINISTIC`. Future semantic assessment is `MODEL` input under `HYBRID` Core validation. Candidate acceptance and gate remain `DETERMINISTIC`. No model assessment or gate is implemented here.

## Implemented files and actual behavior

- `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`, `native/Cargo.toml`: Rust 1.90.0 workspace and dependency lock.
- `native/src/db.rs`: SQLite WAL schema, canonical Git Project registration with frozen observed base commit, Action objective intake, SHA-256 objective digest, durable events, operation/payload-bound idempotency. Actions remain `UNASSESSED` and `HUMAN_REQUIRED`; the digest is **not** an ActionSpec.
- `native/src/bin/core.rs`: Axum loopback API, separate operator and Worker bearer tokens, project/action/event endpoints, Worker handshake. No dispatch path exists.
- `native/src/bin/worker.rs`: separate process that validates a loopback Core URL and authenticates. It receives `dispatch_supported=false` and does not execute work.
- `native/src/protocol.rs`, `native/src/runtime.rs`, `native/src/bin/provider_probe.rs`: direct Rust Codex JSON-RPC preflight and password-protected OpenCode HTTP/OpenAPI preflight. These validate startup/protocol reachability and wait for child cleanup on success and error paths. They do **not** start paid turns, handle permissions, stream work, or reconcile sessions.
- `native/README.md`: build/run scope and environment variables. Documentation index, MASTER banner, ADR-0005 metadata and ADR index mark supersession while older docs remain historical reference until full cutover.

## Evidence and limits

Local deterministic commands (run from this branch with `mise exec rust@1.90.0 --` if Rust is not on PATH):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --bins
cargo run --bin factory-provider-probe -- codex
cargo run --bin factory-provider-probe -- opencode
```

At handoff authoring, formatter, strict Clippy, six unit tests, bin build, and both provider preflight probes passed; rerun all gates on the committed SHA. Local Core HTTP smoke used a disposable Git repository: unauthorized project request 401, project registration 201, same-key replay returned same ID, Action creation 201 with `HUMAN_REQUIRED`/`UNASSESSED`, unauthorized Worker 401, Worker handshake success with dispatch false, two events, and one Action persisted after Core restart. This is author self-test, not independent acceptance.

Codex CLI `0.156.1` generated the v2 schema locally; the full local schema SHA-256 was `995fc3b8f8c469f6787e8fc5be4038c4f31359025edd8480b862e83355f3bf3b`. Codex initialize/initialized/model-list probe succeeded, with cleanup confirmed. OpenCode `1.18.32` authenticated health and `/doc` OpenAPI 3 probe succeeded; served schema SHA-256 was `46db986090aae41846cd6dbe16225a1d883f0bbcb4c48814008d3f6ce140aa5c`, cleanup confirmed. No paid provider turn was authorized or run. `LIVE_NOT_TESTED` for coding compatibility. No provider session was created by this slice; unresolved runtime session count is zero for these probes only.

Luna independently inspected architecture and code and independently ran six unit tests, formatter and strict Clippy after early fixes. Findings resolved in the current tree: idempotency payload/operation binding including HEAD-advance replay, Worker URL userinfo exfiltration path, misleading `spec_sha256`, fake `/worker/claim` semantics, and preflight child cleanup waiting. Open findings: provider probes have no work lifecycle; OpenCode random port allocation has a bind race; native authority/leases/sandbox/candidate/gate/UI are absent. Luna's final review of the eventual candidate remains pending. No independent E2E verifier or Quality Gate result exists.

## Repository and delivery state

At campaign entry, only PR #16 was open: `f1-s4-d-intelligent-lead` head `19cb4951f7a3d42a1ca27b1800cf49401c81ecc6`, base main `102d7ba29295554bd61ae71956347a025d219d7b`. PR #12 and #13 are merged into main. PR #16 has historical Go/Paseo S4-D work with incomplete real multi-agent and semantic certification; do not merge it into this branch or transfer evidence. No existing PR was closed or merged. This branch has not been merged or deployed. Check current PR/CI with `gh pr list --state open` and `gh pr checks <number>`; do not infer a green state from old checks.

## Next tasks, in dependency order

1. Complete provider adapters in Rust. Codex: typed initialize, thread/start/resume, turn/start, event correlation, permission requests, terminal `turn/completed` status, interrupt/cancel, timeout, process cleanup, restart reconciliation. OpenCode: authenticated supervised server, explicit workspace/session, prompt submission, SSE, permission handling, session inspection and abort, reconnect reconciliation and cleanup. Use installed schemas and deterministic protocol fixtures. Do not call a started turn successful without terminal evidence.
2. Create Core-owned frozen ActionSpec, risk and delegation authority, per-Action budgets, authenticated Worker assignment with lease/generation/fencing and idempotent receipts. A Worker must not dispatch when isolation or authority is absent.
3. Add Factory-owned per-Attempt workspace allocation and tested sandbox/credential confinement. A Git worktree alone is insufficient. Test restart and stale-Worker recovery.
4. Implement provider-backed Attempt → candidate commit → independent clean-checkout verifier → reviewer → deterministic gate, then a Rust-backed Office journey. Use a disposable external fixture and exact candidate evidence.
5. Add bounded WorkGraphs, semantic evaluation, delegation, independent acceptance, exact-SHA CI, and only then remove obsolete Go/Paseo/AEH active code/docs/CI in one coherent cutover. Keep Git history.

**Next executable acceptance:** create a disposable fixture and prove, with deterministic fixtures first, that each adapter can start a session, submit work, observe progress, reject unauthorized permission, distinguish failed/interrupted/completed terminal states, cancel, inspect after reconnect, and record cleanup truthfully. A live paid turn needs a separately explicit bounded authorization and cannot be inferred from this handoff.

**Security and budget blockers:** no isolated sandbox or credential confinement tested; no production-safe execution; no paid-provider budget specified; no delivery/merge authority. No real sessions or protected delivery effects were created. Never mark these PASS from unit tests or preflight probes.

**DeepSeek V4.1 Flash continuation:** check out this branch, read `AGENTS.md`, ADR-0018, `native/README.md`, this handoff, and `native-rust-progress.json`; run `git status`, `git rev-parse HEAD`, the pinned Rust gates, then inspect the two local provider schemas. Continue from task 1 above. Preserve explicit non-dispatch until Core authority, sandbox and budget are implemented. Keep all statements candidate-bound and update this handoff/progress file with every coherent commit. Do not resume the S4-D campaign, spend provider usage without a bounded grant, merge PR #16, or claim cutover based on this bootstrap.
