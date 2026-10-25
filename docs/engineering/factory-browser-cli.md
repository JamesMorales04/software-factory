# Factory-managed browser server and check

**Status:** bounded Worker-side capability implemented as a library and
`factory-browser` binary. Worker/Core wiring is intentionally not part of this
slice; the Director integrates it. TRUSTED_LOCAL only; this is not isolation
and not SEC-01 certification.

The implementation is `native/src/browser.rs`; the fixed browser runner is
`office/e2e-browser/browser-check-runner.mjs`. The fixture server
`factory-browser-fixture` exists only for deterministic tests.

## Library surface

`factory_native::browser` exposes the four explicit operations a Worker or
verifier needs:

| Operation | API | Meaning |
| --- | --- | --- |
| start | `ManagedTestServer::start` | Start argv in the Attempt workspace, own the process group, capture bounded logs outside the repository, wait for 2xx readiness |
| check | `run_browser_check` | Run real Chromium through pinned `playwright-core` against the authorized loopback endpoint and write bounded evidence |
| stop | `ManagedTestServer::stop` / `stop_from_state` | Terminate the whole process group, verify port closure and process absence |
| cleanup | `ManagedTestServer::cleanup_runtime` / `cleanup_state_dir` | Remove disposable runtime/profile/state artifacts after a stopped or stale server |

`ManagedTestServerConfig` requires an absolute workspace `cwd`, an absolute
`log_dir` outside that workspace, a bounded argv vector (no shell string), a
safe absolute `ready_path`, a startup timeout in `5..=600` seconds, and a log
bound in `1 KiB..=16 MiB`. `endpoint: None` allocates a free loopback port;
an explicit endpoint is validated as plain `http://127.0.0.1:<port>`. An
already-occupied port is rejected with `PortOccupied`; an existing listener is
never adopted.

The managed server receives a cleared environment containing only `PATH`,
disposable `HOME`/`TMPDIR`, `HOST=127.0.0.1`, `PORT` and
`FACTORY_TEST_ENDPOINT`. Its stdout/stderr are bounded logs under `log_dir`.
Startup and readiness failures are infrastructure failures, not model failures.

The browser check receives only `PATH`, disposable `HOME`/`TMPDIR`,
`CHROMIUM_EXE`, `FACTORY_PLAYWRIGHT_CORE` and `PLAYWRIGHT_BROWSERS_PATH`. It
navigates the authorized URL, asserts a selector and/or visible body text,
records URL, title, console-error count, failed-request count and browser
version, optionally writes a bounded screenshot, and removes the disposable
profile. Missing tooling is `NOT_TESTED` (exit 3), never `PASSED`.

The server lifecycle is Linux-only and fails closed elsewhere. A TRUSTED_LOCAL
application can still ignore `HOST`/`PORT` and bind elsewhere; this contract is
control-plane policy, not network isolation. In-process `run`/`server` modes
enforce the log bound while streaming; detached `start` logs are tail-capped on
`stop`/`cleanup`, so a Worker must still bound the Run lifetime.

## CLI

The binary accepts argv only (`--` terminates server arguments):

```sh
# Full lifecycle: start, readiness, real Chromium, stop, cleanup.
FACTORY_PLAYWRIGHT_CORE=/path/to/playwright-core \
PLAYWRIGHT_BROWSERS_PATH=/path/to/ms-playwright \
factory-browser run \
  --cwd /abs/attempt/workspace \
  --log-dir /abs/worker-registry/browser/run-1 \
  --endpoint http://127.0.0.1:4173 \
  --ready-path /health \
  --selector '#app-ready' \
  --text 'Ready' \
  --screenshot /abs/worker-registry/browser/run-1/screenshot.png \
  --evidence /abs/worker-registry/browser/run-1/evidence.json \
  -- npm start
```

```sh
# Foreground start/stop; SIGINT or SIGTERM stops and verifies the group.
factory-browser server --cwd ... --log-dir ... --endpoint ... -- npm start

# Detached start/stop through an explicit Worker-owned state directory.
factory-browser start --state-dir /abs/worker-registry/browser/state \
  --cwd ... --log-dir ... --endpoint ... -- npm start
factory-browser check --url http://127.0.0.1:4173/ --selector '#app-ready' \
  --evidence /abs/worker-registry/browser/evidence.json
factory-browser stop --state-file /abs/worker-registry/browser/state/<id>.json
factory-browser cleanup --state-dir /abs/worker-registry/browser/state

# Tooling preflight; exit 3 means NOT_TESTED.
factory-browser tools
```

`run` requires `--endpoint` so the browser target matches the authorized
Project endpoint. `start` allocates a loopback port when `--endpoint` is
omitted. Detached state records boot id, PID, process group and `/proc` start
time; `stop` refuses to signal a process whose identity cannot be verified.
`cleanup` removes stale state and artifacts, reports live owned servers, and
only stops them with `--stop-live`.

Exit codes: `0` operation passed, `1` failed, `3` browser tooling unavailable
(`NOT_TESTED`).

### Flags

| Flag | Used by | Meaning |
| --- | --- | --- |
| `--cwd` | run/server/start | Exact absolute Attempt workspace |
| `--log-dir` | run/server/start | Absolute Worker-owned log directory outside the workspace |
| `--endpoint` | run/server/start | Authorized `http://127.0.0.1:<port>` |
| `--ready-path` | run/server/start | Safe absolute readiness path (default `/`) |
| `--startup-timeout-secs` | run/server/start | `5..=600`, default `30` |
| `--max-log-bytes` | run/server/start/stop | Per-stream bound, default `1048576` |
| `--url-path` | run | Safe path on the authorized endpoint (default `/`) |
| `--selector` / `--text` | run/check | At least one required assertion |
| `--browser-timeout-secs` / `--timeout-secs` | run/check | Check timeout, default `60` |
| `--profile-dir` | run/check | Disposable profile directory, removed on completion |
| `--evidence` | run/check | Bounded JSON evidence path |
| `--screenshot` | run/check | Optional screenshot path |
| `--state-dir`, `--state-file`, `--id` | start/stop/cleanup | Durable detached-server state |
| `--node`, `--runner`, `--playwright-core`, `--chromium`, `--browsers-path` | all browser commands | Explicit tooling overrides |

## Environment

| Variable | Meaning |
| --- | --- |
| `FACTORY_PLAYWRIGHT_CORE` | Absolute `playwright-core` module directory (pinned 1.64.1) |
| `FACTORY_CHROMIUM_EXE` / `CHROMIUM_EXE` | Absolute Chromium executable (preferred / verifier parity) |
| `PLAYWRIGHT_BROWSERS_PATH` | Absolute Playwright browser cache root |
| `FACTORY_NODE_BINARY` | Node executable (default `node`) |
| `FACTORY_BROWSER_RUNNER` | Override the fixed runner path |

No Factory, provider, Git or operator credential is forwarded to the server or
browser process. Tooling absence is explicit `NOT_TESTED`.

## Evidence boundary

Managed-server health is **infrastructure evidence**, not Implementer
browser-capability evidence and not independent product acceptance. The browser
check records its own bounded provenance. A screenshot is an artifact, not
proof by itself.

## Integration instructions

1. In the Worker's `TRUSTED_LOCAL_PROVIDER`/browser path, start one
   `ManagedTestServer` per Attempt after workspace allocation and before the
   provider session, using the frozen Project `browserTestServer` argv,
   `readyPath` and `startupTimeoutSecs`.
2. Map `BrowserError::{PortOccupied, NotReady, StartupTimedOut}` to
   `INFRASTRUCTURE_BLOCKED`; do not open a provider session.
3. Pass the resolved endpoint/path to the model prompt as already running; the
   model must not restart it.
4. Use `BrowserTooling::from_env()` plus `run_browser_check` for a
   Factory-administered check. Treat `NotTested` as a blocked/not-tested
   prerequisite, never as acceptance.
5. Call `ManagedTestServer::stop()` on success, failure, cancellation and
   shutdown. Persist the `StopReceipt`; `port_closed` and
   `process_group_absent` must both be true.
6. Keep the independent verifier's protected browser check separate from this
   Implementer path.
