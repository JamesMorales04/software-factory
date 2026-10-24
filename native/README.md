# Native Factory bootstrap

This is the native Rust implementation under migration closure. It supports Project/Action intake, project configuration revisions, authorized R0/FIXTURE Campaigns, managed Worker/Supervisor execution, named-volume rootless-Podman sandboxes, candidate lineage, sandboxed verification, deterministic Quality Gate, Lead/delegation/CAP-014 scaffolds and a Rust-backed Office. Provider-backed coding, unrestricted/multi-unit autonomous start, parallel/fan-in WorkGraph execution and ISOLATED mode remain fail-closed. [Migration closure](../docs/implementation/native-rust-closure.md) is the status authority.

Pinned toolchain: Rust 1.90.0, Cargo.lock in repository. Persistence uses SQLx 0.8.6 (`sqlx-core` + `sqlx-sqlite` directly) with versioned migrations in `../migrations`. Run from repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --bins
cargo sqlx migrate run --source migrations   # disposable DATABASE_URL; CI check
```

Core requires distinct `FACTORY_OPERATOR_TOKEN`, `FACTORY_WORKER_BOOTSTRAP_TOKEN`, `FACTORY_VERIFIER_TOKEN` and `FACTORY_REVIEWER_TOKEN`; role tokens are at least 32 bytes. It optionally accepts `FACTORY_DATABASE` and a loopback bind, and permits `0.0.0.0` only with explicit `FACTORY_CONTAINER_MODE=1`. The Worker requires the bootstrap token, stable identity, registration key and `FACTORY_SUPERVISOR_URL`/`FACTORY_SUPERVISOR_TOKEN`; it does not access project paths or run project code. The Supervisor requires a private workspace/state root, explicit project roots, an exact image allowlist and a loopback bind. Keep all values out of Git and logs.

Core APIs: `GET /health`; operator-authenticated `GET/POST /projects`, `GET/POST /actions`, `GET/POST /campaigns`, `GET /runs`, `GET /attempts`, `GET /events`, and quality reads under `GET /quality/candidates/{id}` (plus `/receipts` and `/gate`); bootstrap-authenticated `POST /worker/register`; session-authenticated `POST /worker/claim`, `/worker/heartbeat`, `/worker/report`, and `/worker/candidate`; and verifier/reviewer-authenticated `POST /quality/receipts`. Create requests need an `idempotency_key`. Actions remain `HUMAN_REQUIRED` with `UNASSESSED` risk. Their versioned ActionSpecs are frozen execution inputs, not execution authorization. The Worker claim route returns a leased assignment only for an explicitly authorized R0/FIXTURE Campaign; provider and ISOLATED modes remain disabled. The Worker sends the assignment to the Supervisor, which snapshots the worktree into a Podman named volume, executes, validates the exported tree and commits the candidate. No host workspace path is passed to Podman. Candidate scope is checked against the frozen ActionSpec and the Campaign authorization. Candidate and quality receipt rows are immutable; the deterministic gate is written once when independent verifier and reviewer receipts exist.

Container runtime: the Core accepts `FACTORY_CONTAINER_MODE=1` with an explicit `0.0.0.0` bind for container publication; the host CLI publishes only `127.0.0.1`. When `FACTORY_OFFICE_DIR` is set, Core also serves the built React Office and exposes the same API under `/core-api` for the browser client. The `factory-supervisor` binary owns durable per-Attempt worktrees, named-volume execution and restart reconciliation. It is not isolation-certified. The verifier executes frozen checks in an independent named-volume sandbox.

The read-only native probes are:

```sh
cargo run --bin factory-provider-probe -- codex
cargo run --bin factory-provider-probe -- opencode
cargo run --bin factory-provider-probe -- codex-reconcile <thread-id>
cargo run --bin factory-repo-hygiene -- --mode audit
cargo run --bin factory-architecture-check -- --root . --base-ref origin/main
```

These start and stop provider servers but do not create paid turns. The Codex probe checks initialization and model discovery only. `codex-reconcile` queries a **new** App Server for indexed and locally loaded thread metadata; absence cannot prove cleanup by an earlier process. The OpenCode probe uses a generated per-process password on loopback, reads the server's own bound address, hashes its OpenAPI schema, checks the first SSE `server.connected` event, and creates, inspects, then deletes one session bound to the canonical workspace. This does not test prompt progress, completion, reconnect, or real coding. Probe results are preflight observations, not a provider execution certificate. The hygiene tool reports the remaining legacy tree in audit mode; strict mode is required before final cutover. Lead fixture assessment is available only with explicit `FACTORY_LEAD_FIXTURE_ASSESSOR=1`; it is not a model and cannot start a Campaign. CAP-014 commands are `cargo run --bin factory-s4d-evaluate -- corpus|baseline|validate|compare`.
