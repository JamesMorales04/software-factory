# Native Factory bootstrap

This is an incomplete Rust replacement branch. It supports local Project/Action intake, frozen ActionSpec storage, blocked Campaign identity, and provider protocol preflight. It has **no authorized agent dispatch**, campaign execution, accepted CandidateRevision, independent verification, Quality Gate, or Office connection. Do not use it as an engineering controller yet.

Pinned toolchain: Rust 1.90.0, Cargo.lock in repository. Run from repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --bins
```

Core requires distinct nonempty `FACTORY_OPERATOR_TOKEN` and `FACTORY_WORKER_BOOTSTRAP_TOKEN`, and optionally `FACTORY_DATABASE` and loopback `FACTORY_BIND`. It refuses non-loopback binding. The Worker requires `FACTORY_WORKER_BOOTSTRAP_TOKEN`, a stable `FACTORY_WORKER_ID`, `FACTORY_WORKER_REGISTRATION_KEY`, and optionally a loopback `FACTORY_CORE_URL`. Registration mints a Worker session token; Core stores only its digest and binds it to the Worker ID. Keep these values out of Git and logs.

Core APIs: `GET /health`; operator-authenticated `GET/POST /projects`, `GET/POST /actions`, `GET/POST /campaigns`, `GET /runs`, `GET /attempts`, and `GET /events`; bootstrap-authenticated `POST /worker/register`; and session-authenticated `POST /worker/claim`, `/worker/heartbeat`, and `/worker/report`. Create requests need an `idempotency_key`. Actions remain `HUMAN_REQUIRED` with `UNASSESSED` risk. Their versioned ActionSpecs are frozen execution inputs, not execution authorization. The Worker claim route explicitly returns `dispatch_supported=false`; lease and terminal receipt paths are fixture tested but cannot dispatch through the public API.

The read-only native probes are:

```sh
cargo run --bin factory-provider-probe -- codex
cargo run --bin factory-provider-probe -- opencode
cargo run --bin factory-provider-probe -- codex-reconcile <thread-id>
cargo run --bin factory-repo-hygiene -- --mode audit
cargo run --bin factory-architecture-check -- --root . --base-ref origin/main
```

These start and stop provider servers but do not create paid turns. The Codex probe checks initialization and model discovery only. `codex-reconcile` queries a **new** App Server for indexed and locally loaded thread metadata; absence cannot prove cleanup by an earlier process. The OpenCode probe uses a generated per-process password on loopback, reads the server's own bound address, hashes its OpenAPI schema, checks the first SSE `server.connected` event, and creates, inspects, then deletes one session bound to the canonical workspace. This does not test prompt progress, completion, reconnect, or real coding. Probe results are preflight observations, not a provider execution certificate. The hygiene tool currently reports the remaining legacy tree in audit mode; strict mode is required before final cutover.
