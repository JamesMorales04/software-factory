# Native Factory

For a tested local developer-preview workflow, see [developer-preview-testing.md](../docs/implementation/developer-preview-testing.md). This is the native Rust implementation under migration closure. It supports Project/Action intake, project configuration revisions, authorized R0/FIXTURE and TRUSTED_LOCAL_PROVIDER Campaigns, managed Worker/Supervisor execution through `factory start`, named-volume rootless-Podman sandboxes, candidate lineage, sandboxed verification, deterministic Quality Gate, model-backed Lead/delegation/CAP-014 paths, explicit Lead-start authorization and a Rust-backed Office. OpenCode coding has a bounded managed live acceptance record in [managed-live-uat-2026-10-26.md](../docs/evidence/managed-live-uat-2026-10-26.md); a controlled self-development candidate reached `ACCEPTED` in [managed-self-development-2026-10-26.md](../docs/evidence/managed-self-development-2026-10-26.md). Codex App Server coding is implemented and deterministic-tested; native ChatGPT-authenticated live inference remains blocked; an operator API-key route is configured and preflighted, but live Codex inference is not tested. ISOLATED mode and certification remain fail-closed. Bounded parallel/fan-in WorkGraph execution, cancellation, revocation fencing and recovery are deterministically verified. [Migration closure](../docs/implementation/native-rust-closure.md) is the status authority.

Pinned toolchain: Rust 1.90.0, Cargo.lock in repository. Persistence uses SQLx 0.8.6 (`sqlx-core` + `sqlx-sqlite` directly) with versioned migrations in `../migrations`. Run from repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --bins
cargo sqlx migrate run --source migrations   # disposable DATABASE_URL; CI check
```

Core requires distinct `FACTORY_OPERATOR_TOKEN`, `FACTORY_WORKER_BOOTSTRAP_TOKEN`, `FACTORY_VERIFIER_TOKEN` and `FACTORY_REVIEWER_TOKEN`; role tokens are at least 32 bytes. Core and the Supervisor additionally share `FACTORY_ATTEMPT_GRANT_KEY`; the Supervisor requires `FACTORY_SUPERVISOR_CAPABILITY_KEY`. These two keys are never given to the Worker. Core optionally accepts `FACTORY_DATABASE` and a loopback bind, and permits `0.0.0.0` only with explicit `FACTORY_CONTAINER_MODE=1`. Optional OTLP/HTTP log export is enabled only when `FACTORY_OTLP_ENDPOINT` names an absolute http(s) collector URL (a bare origin gains `/v1/logs`); export is bounded, sanitized and never gates Core writes. The Worker requires the bootstrap token, stable identity, registration key and `FACTORY_SUPERVISOR_URL`/`FACTORY_SUPERVISOR_TOKEN`; it does not access project paths or run project code. `TRUSTED_LOCAL_PROVIDER` assignments require `FACTORY_PROVIDER_SANDBOX=bwrap` and `FACTORY_PROVIDER_BINARY` (an absolute provider executable that listens on `FACTORY_PROVIDER_SOCKET`). The long-lived provider credential stays in the trusted Worker and is never sent to the Supervisor or sandbox. Optional model egress uses `FACTORY_PROVIDER_UPSTREAM_URL` (fixed upstream origin), `FACTORY_PROVIDER_UPSTREAM_TOKEN` (injected only by the host broker), `FACTORY_PROVIDER_EGRESS_BRIDGE` (absolute bridge binary) and an optional per-run `FACTORY_PROVIDER_EGRESS_TOKEN`; the sandbox receives only the bridge address and per-run token. `FACTORY_ALLOW_TEST_UPSTREAM=1` permits a loopback fake upstream only for tests. `FACTORY_PROVIDER_SANDBOX=unconfined` is a clearly labeled non-isolated fallback and requires explicit `FACTORY_ALLOW_UNCONFINED_PROVIDER=1` plus loopback `FACTORY_PROVIDER_BASE_URL`/`FACTORY_PROVIDER_PASSWORD`; it never activates implicitly. The Supervisor requires a private workspace/state root, explicit project roots, an exact image allowlist and a loopback bind. Keep all values out of Git and logs.

Core APIs: `GET /health`; operator-authenticated `GET/POST /projects`, `GET/POST /actions`, `GET/POST /campaigns`, `POST /campaigns/{id}/cancel`, `GET /runs`, `GET /attempts`, `GET /events` (optional bounded `?after=<sequence>&limit=<n>`), `GET /observability`, `GET/POST /model/calls`, `POST /model/calls/{id}/resolve`, and quality reads under `GET /quality/candidates/{id}` (plus `/receipts`, `/gate` and `/inspection`); bootstrap-authenticated `POST /worker/register`; session-authenticated `POST /worker/claim`, `/worker/heartbeat`, `/worker/report`, and `/worker/candidate`; verifier/reviewer-authenticated `POST /quality/receipts`; and session-authenticated `POST /worker/provider_intent` and `/worker/provider_terminal` for durable provider-session accounting; session-authenticated `POST /worker/provider_turn_authorize`, `/worker/provider_turn_bind`, `/worker/provider_turn_settle` and `/worker/provider_turn_release` for exact per-turn request binding and accounting; operator-authenticated `GET /provider/turn_reservations` and `POST /provider/turn_reservations/{id}/resolve` for bound-but-ambiguous turns. Create requests need an `idempotency_key`. Actions remain `HUMAN_REQUIRED` with `UNASSESSED` risk. Their versioned ActionSpecs are frozen execution inputs, not execution authorization. The Worker claim route returns a leased assignment only for an explicitly authorized R0 Campaign; provider mode requires `TRUSTED_LOCAL_PROVIDER` and ISOLATED remains disabled. The Worker sends the assignment to the Supervisor, which snapshots the worktree into a Podman named volume, executes, validates the exported tree and commits the candidate. No host workspace path is passed to Podman. Candidate scope is checked against the frozen ActionSpec and the Campaign authorization. Candidate and quality receipt rows are immutable; the deterministic gate is written once when independent verifier and reviewer receipts exist. The `factory-verifier` executes frozen checks in an independent named-volume sandbox; `factory-reviewer` independently re-verifies candidate identity, scope and bounded diff facts and submits the reviewer receipt. Provider mode supports a bounded tool-free patch and an authorized multi-turn tool loop. The provider may request only `write_file` or a frozen-check `run_command`; the Worker records each request/decision/result in Core, validates paths and commands, and the Supervisor applies writes or runs the exact check in the Attempt sandbox. The deterministic provider process runs under bubblewrap with no host HOME, repository, runtime socket or external network; the long-lived credential is not in that process.

Container runtime: the Core accepts `FACTORY_CONTAINER_MODE=1` with an explicit `0.0.0.0` bind for container publication; the host CLI publishes only `127.0.0.1`. When `FACTORY_OFFICE_DIR` is set, Core also serves the built React Office and exposes the same API under `/core-api` for the browser client. The `factory-supervisor` binary owns durable per-Attempt worktrees, named-volume execution and restart reconciliation. It is not isolation-certified. The verifier executes frozen checks in an independent named-volume sandbox.

## Core-owned model calls

Lead assessment, semantic review and CAP-014 evaluation share one Core-owned
authorization path. Core reserves a bounded token/cost envelope before any
upstream request, records measured usage, and refuses automatic retries after an
ambiguous or unknown outcome. The provider credential stays in Core; a Worker,
reviewer or evaluator never receives it.

```sh
export FACTORY_MODEL_URL=https://provider.example/v1
export FACTORY_MODEL_TOKEN=...                 # >= 16 bytes; never commit it
export FACTORY_MODEL_PROVIDER=opencode-go
export FACTORY_MODEL_NAME=deepseek-v4.1-flash
export FACTORY_MODEL_TIMEOUT_SECS=60
export FACTORY_LEAD_MODEL_MAX_COST_MICROS=300000
export FACTORY_REVIEW_MODEL_MAX_COST_MICROS=1000000
export FACTORY_EVALUATION_MODEL_MAX_COST_MICROS=2000000
export FACTORY_MODEL_GLOBAL_MAX_COST_MICROS=10000000
```

`FACTORY_ALLOW_TEST_UPSTREAM=1` permits plain loopback HTTP for deterministic
tests only. The legacy `FACTORY_LEAD_MODEL_URL` name is retired and Core refuses
to start with it. `POST /model/calls` accepts `semantic_reviewer` under the
reviewer credential and `evaluation` under the operator credential; `lead` is
internal-only and cannot be called over HTTP. An `AMBIGUOUS` or `UNKNOWN`
reservation blocks the same request identity until an operator resolves it via
`POST /model/calls/{id}/resolve`.

`factory-reviewer` remains deterministic by default. Set
`FACTORY_REVIEW_MECHANISM=MODEL` to request a strict, schema-validated semantic
review; malformed or uncertain output can never become `ACCEPTED`.

CAP-014 raw model-output collection is explicit and budgeted:

```sh
FACTORY_OPERATOR_TOKEN=... cargo run --locked --bin factory-s4d-evaluate -- \
  execute http://127.0.0.1:8765 cap014-run-1 /tmp/cap014-run-1.json
```

The command writes frozen-corpus, per-fixture raw-output, model provenance and
usage evidence. It does not score the outputs; independent blinded scoring and
`compare` remain separate. A failed or budget-exhausted run writes `FAILED`
evidence and exits non-zero.

## Live OpenCode provider (TRUSTED_LOCAL_PROVIDER)

The Worker supports sandboxed OpenCode and Codex App Server provider execution
for an explicitly authorized R0 `TRUSTED_LOCAL_PROVIDER` Campaign. The
OpenCode route is `opencode-go/deepseek-v4.1-flash`; the Codex route is
`codex/gpt-6-luna`. These are TRUSTED_LOCAL routes, not ISOLATED execution.
Required OpenCode Worker settings:

```sh
export FACTORY_PROVIDER_SANDBOX=bwrap
export FACTORY_PROVIDER_KIND=opencode
export FACTORY_PROVIDER_BINARY=/absolute/path/to/opencode   # 1.18.32 validated
export FACTORY_PROVIDER_BRIDGE=/absolute/path/to/factory-provider-bridge
export FACTORY_PROVIDER_EGRESS_BRIDGE=/absolute/path/to/factory-egress-bridge
export FACTORY_PROVIDER_UPSTREAM_URL=https://opencode.ai/inference/go/openai/v1
export FACTORY_PROVIDER_UPSTREAM_TOKEN=...                  # never commit
export FACTORY_PROVIDER_SESSION_ROOT=/short/tmp/provider-sessions
export FACTORY_PROVIDER_MEMORY_LIMIT_BYTES=17179869184     # Bun/OpenCode needs >4 GiB
```

The Worker generates the OpenCode provider configuration inside the sandbox,
connects the provider bridge over the private Unix socket, and exposes only the
egress bridge and per-run token to the sandbox. The egress broker pins the
upstream, injects the long-lived credential and `x-opencode-session`, and
enforces the Core turn reservation. `FACTORY_PROVIDER_TCP_PORT` defaults to
18081 (18080 is reserved for egress); `FACTORY_PROVIDER_MODEL_CONTEXT` and
`FACTORY_PROVIDER_MODEL_OUTPUT` set the generated model limits.

`factory start` can manage this route from a strict Worker-only `0600`
`FACTORY_PROVIDER_ENV_FILE`; it validates the allowlisted provider variables and
forwards the parsed values only to the host Worker, never to the Core container
or Supervisor. A separate strict Core-only `0600`
`FACTORY_CORE_MODEL_ENV_FILE` supplies the model gateway and policy variables
to the Core container as an additional `--env-file`. The CLI recreates a
running Core container when that file's fingerprint label changes. Neither file
authorizes a Campaign or changes a budget, and the Codex coding adapter remains
`BLOCKED`. See [factory-cli/README.md](../factory-cli/README.md) for exact
allowlists and redacted status output.

The Worker rejects an empty `final` response, corrects invalid model actions,
and preserves already-applied writes for independent verification if a live
turn ends with an error or the four-turn budget is exhausted. Applied writes
still require the independent verifier, semantic Reviewer and deterministic
Quality Gate; the model's final message is not authority. The Worker heartbeat
extends the lease by 60 seconds every 15 seconds, and a failed provider
check-command leaves the worktree available for a later write or finalize.

This path is TRUSTED_LOCAL, not SEC-01 isolation. Do not claim credential
confinement or ISOLATED certification. The current managed live acceptance
journey and its measured usage are recorded in
[the managed live UAT report](../docs/evidence/managed-live-uat-2026-10-26.md);
the earlier F1-S4-LIVE-UAT report remains historical evidence.

## Codex App Server provider (TRUSTED_LOCAL_PROVIDER)

The Worker can drive the installed Codex App Server (`0.156.1` validated) over
stdio inside the same bubblewrap boundary. Codex is selected by the frozen
Campaign route `codex/gpt-6-luna`; the Worker also requires
`FACTORY_PROVIDER_KIND=codex`. Codex is never granted native shell or file
authority: the Worker declares `write_file` and `run_command` as client dynamic
tools, and the egress broker replaces Codex's upstream tool list with exactly
that allowlist before forwarding. The Worker additionally denies other tool
names, rejects unsupported server requests and non-allowlisted item types, and
fails on native `commandExecution`/`fileChange` items. Codex 0.156.1 still
contains built-in code-mode, collaboration, goal and request-input surfaces;
the production invocation disables the known feature flags and the egress
tool-list replacement removes them from the model request. This is
version-sensitive; verify with the pinned binary and do not weaken the gate.

Required Worker settings in addition to the egress settings above:

```sh
export FACTORY_PROVIDER_SANDBOX=bwrap
export FACTORY_PROVIDER_KIND=codex
export FACTORY_PROVIDER_BINARY=/absolute/path/to/native/codex   # not the npm shim
export FACTORY_PROVIDER_EGRESS_BRIDGE=/absolute/path/to/factory-egress-bridge
export FACTORY_PROVIDER_UPSTREAM_URL=https://api.openai.com/v1
export FACTORY_PROVIDER_UPSTREAM_TOKEN=...                  # never commit
export FACTORY_PROVIDER_SESSION_ROOT=/short/tmp/provider-sessions
export FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION=...
export FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION=...
```

The binary must be the native vendor executable, not the npm/mise launcher
shim; the sandbox bind-mounts one executable and the shim's module lookup fails.
The Worker uses a private `CODEX_HOME` under the disposable session directory,
a generated Git session root, the loopback egress bridge, and no operator HOME
or `~/.codex`. It declares the dynamic tools at thread start, divides the turn
input/output ceiling across the bounded request sequence, and binds every
upstream `/v1/responses` body to the Core turn reservation (up to eight
requests per turn). It settles token usage and refuses success unless the
sandbox process is confirmed absent and the disposable session directory was
removed. Cost is computed from the two configured micro-dollar rates; if either
rate is absent, usage is not claimed as known and the candidate is refused.
Do not treat Codex archive, process exit alone, or an empty index as cleanup
proof. Cost is a post-request calculation; the Core gate rejects over-budget
usage, but the provider may already have executed the forwarded request.

The deterministic tests are:

```sh
cargo test -p factory-native --test codex_dynamic_tools --locked
CODEX_BIN=/absolute/path/to/codex \
  cargo test -p factory-native --test e2e_fixture_dispatch codex_ --locked
```

`factory doctor` and `factory start` accept the same strict Codex Worker
profile with the native vendor binary, egress bridge, pinned upstream and both
`FACTORY_PROVIDER_CODEX_*_COST_MICROS_PER_MILLION` rates. A placeholder-token
managed preflight passed without inference; see the
[Codex managed configuration evidence](../docs/evidence/codex-managed-config-2026-10-26/doctor.log).
The only supported live authentication mode is an operator API key at `https://api.openai.com/v1`; no live Codex inference has been run.
Native ChatGPT OAuth authentication is **BLOCKED** by the current egress
boundary: Codex owns and refreshes the ChatGPT access token and account header,
while the broker injects one pinned bearer credential and cannot safely import
or pass through those credentials. The earlier test-only ChatGPT proxy is not
part of the product.

`codex_dynamic_tools` drives the real App Server against a loopback Responses
mock. `codex_real_executable_reaches_a_candidate_and_binds_upstream_requests`
drives the real binary through Core, Worker, Supervisor and the egress broker
to a Git candidate with two bound upstream requests; it also verifies that the
upstream tool list is exactly `write_file`/`run_command` and does not contain
built-in exec, request-input or collaboration tools. Neither test makes a paid
model call. A separately authorized bounded GPT-6 Luna live UAT passed on
2026-10-26 with candidate `635336d3d525a20ab4079ae04586c4e2f7f1ab74`, an
independent deterministic verifier and an `ACCEPTED` gate. It used a test-only
local proxy to the authenticated ChatGPT Codex backend; native ChatGPT-backend
configuration, Codex-native semantic model review, native cost verification and
ISOLATED execution remain open. See
[the live evidence](../docs/evidence/codex-gpt6-luna-live-uat-2026-10-26.md).
The managed API-key route is configured and preflighted, but live Codex inference
is not tested; the ChatGPT proxy is not packaged as production authentication.

## Factory-managed browser checks

`factory-browser` provides a bounded external test-server lifecycle and a real
Chromium check through the pinned Playwright tooling. It is a Worker-side
capability, not an isolation claim:

```sh
FACTORY_PLAYWRIGHT_CORE=$PWD/office/e2e-browser/node_modules/playwright-core \
PLAYWRIGHT_BROWSERS_PATH=$HOME/.cache/ms-playwright \
cargo run --locked --bin factory-browser -- run \
  --cwd /abs/attempt/workspace \
  --log-dir /abs/worker-registry/browser/run-1 \
  --endpoint http://127.0.0.1:4173 \
  --ready-path /health \
  --selector '#app-ready' \
  --evidence /abs/worker-registry/browser/run-1/evidence.json \
  -- npm start
```

Missing browser tooling exits 3 (`NOT_TESTED`), never `PASS`.

The read-only native probes are:

```sh
cargo run --bin factory-provider-probe -- codex
cargo run --bin factory-provider-probe -- opencode
cargo run --bin factory-provider-probe -- codex-reconcile <thread-id>
cargo run --bin factory-repo-hygiene -- --mode strict
cargo run --bin factory-architecture-check -- --root . --base-ref origin/main
```

These start and stop provider servers but do not create paid turns. The Codex probe checks initialization and model discovery only. `codex-reconcile` queries a **new** App Server for indexed and locally loaded thread metadata; absence cannot prove cleanup by an earlier process. The OpenCode probe uses a generated per-process password on loopback, reads the server's own bound address, hashes its OpenAPI schema, checks the first SSE `server.connected` event, and creates, inspects, then deletes one session bound to the canonical workspace. This does not test prompt progress, completion, reconnect, or real coding. Probe results are preflight observations, not a provider execution certificate. The hygiene tool enforces the strict zero-legacy tracked-tree gate in CI. Lead fixture assessment is available only with explicit `FACTORY_LEAD_FIXTURE_ASSESSOR=1`; it is not a model. A FIXTURE Lead plan can start under the deterministic FIXTURE authorization when autonomous start is enabled. A MODEL Lead plan requires an explicit `TRUSTED_LOCAL_PROVIDER` authorization matching the frozen proposal and delegation; neither path is live model-quality or ISOLATED certification. CAP-014 commands are `cargo run --bin factory-s4d-evaluate -- corpus|baseline|validate|compare|execute`.
