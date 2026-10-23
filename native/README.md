# Native Factory bootstrap

This is an incomplete Rust replacement branch. It is useful for local Project/Action intake and provider protocol preflight. It has **no authorized agent dispatch**, campaign execution, candidate, independent verification, Quality Gate, or Office connection. Do not use it as an engineering controller yet.

Pinned toolchain: Rust 1.90.0, Cargo.lock in repository. Run from repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --bins
```

Core requires distinct nonempty `FACTORY_OPERATOR_TOKEN` and `FACTORY_WORKER_TOKEN`, and optionally `FACTORY_DATABASE` and loopback `FACTORY_BIND`. It refuses non-loopback binding. The Worker requires `FACTORY_WORKER_TOKEN` and optionally a loopback `FACTORY_CORE_URL`. The tokens are operator supplied; never commit them.

Core APIs: `GET /health`, authorized `GET/POST /projects`, `GET/POST /actions`, `GET /events`, and Worker-only `POST /worker/hello`. Create requests need an `idempotency_key`. Actions remain `HUMAN_REQUIRED` with `UNASSESSED` risk. `objective_sha256` is an objective digest, not a frozen ActionSpec or candidate identity. The Worker handshake explicitly reports `dispatch_supported=false`.

The read-only native probes are:

```sh
cargo run --bin factory-provider-probe -- codex
cargo run --bin factory-provider-probe -- opencode
```

These start and stop provider servers but do not create paid turns. The Codex probe checks initialization and model discovery only. The OpenCode probe uses a generated per-process password on loopback, reads the server's own bound address, hashes its OpenAPI schema, checks the first SSE `server.connected` event, and creates, inspects, then deletes one session bound to the canonical workspace. This does not test prompt progress, completion, reconnect, or real coding. Probe results are preflight observations, not a provider execution certificate.
