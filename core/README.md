# Factory Core and Worker (F1-S1 + F1-S2)

Go modular monolith: domain rules, SQLite WAL persistence, authenticated
HTTP API, durable event ledger, best-effort OpenTelemetry — plus the S2
local Worker, Core–Worker dispatch with leases and fencing, and crash
recovery.

## Toolchain (pinned)

- Go `1.26.9` via `mise.toml` (`go-version-file: core/go.mod` in CI).
- `golangci-lint` via the `tool` directive in `go.mod`; run `go tool golangci-lint run ./...`.
- SQLite driver is pure Go (`modernc.org/sqlite`); no C toolchain needed.

## Commands (run from `core/`)

```sh
go build ./...              # compile
gofmt -l .                  # must print nothing
go vet ./...                # static checks
go tool golangci-lint run ./...  # curated lint gates
go test ./...               # unit + integration
go test -race ./...         # concurrency (hub, SSE)
go run ./cmd/factory-core --addr 127.0.0.1:8471 --db ./data/factory.db
```

Environment: `FACTORY_API_TOKEN` (optional for direct Core runs; when unset,
a process-local token is generated but its value is not displayed),
`FACTORY_WORKER_TOKEN` (scoped Worker-protocol credential; the lifecycle
CLI provisions it, the Core accepts it only on Worker routes),
`OTEL_EXPORTER_OTLP_ENDPOINT` (optional; business writes never depend on it),
`LOG_LEVEL` (debug|info|warn|error).

## Layout

- `cmd/factory-core` — entrypoint, flags, startup/shutdown, boot
  reconciliation, lease sweeper and the native campaign orchestrator.
- `cmd/factory-worker` — S2 local Worker: registration, lease poll,
  heartbeats, synthetic fixture execution (rootless Podman by default,
  `--exec-mode local` for explicit pure-Go test/dev use only), fencing, and
  S4-A implementer candidate commit/push/registration.
- `internal/orchestrator` — S4-B deterministic campaign scheduler: transactional
  task claims, spec-bound Implementer dispatch, Core-spawned verifier/reviewer
  processes, bounded repair, cancellation and restart recovery.
- `cmd/factory-verifier` — independent verifier process (dedicated credential):
  clean read-only candidate checkout, frozen acceptance/negative/browser checks.
- `cmd/factory-reviewer` — read-only reviewer process (dedicated credential):
  revalidates the exact candidate and submits a bounded verdict.
- `cmd/factory` — lifecycle CLI (`factory start|stop|status`,
  `factory worker start|stop|status`, `factory logs [-n] [-f] [worker]`).
- `internal/domain` — Action lifecycle, Run/Attempt/Worker rules, event
  schema, validation (no deps).
- `internal/store` — SQLite WAL: atomic action+event and run+event writes,
  idempotency, leases, fencing, expiry, reconciliation, replay cursors,
  restart retention, frozen specs/candidates/evidence, gate decisions and
  campaign/task orchestration state.
- `internal/candidate` — independent git candidate validation (parent, ref,
  tree/content digest, scope) and clean checkout helpers.
- `internal/quality` — deterministic final-diff risk and the Core Quality Gate.
- `internal/exec` — credential-free synthetic fixture (Podman + local).
- `internal/api` — auth (operator + scoped worker tokens), correlation IDs,
  JSON errors, SSE stream, read-only Agent-observability projection, static UI.
- `internal/telemetry` — redacted JSON logs, correlation IDs, OTel setup.

Core acceptance: `scripts/e2e-s1.sh` from the repo root. S2 acceptance:
`scripts/e2e-s2.sh` (API/CLI journeys, crash/restart/fencing, Podman
probes, S1 regression) and `scripts/e2e-s2-browser.sh` (real-Chromium
Runs/Workers journey). S3-C acceptance: `scripts/e2e-s3c.sh`
(deterministic Paseo CLI stub, session observability, reload/reconnect,
orphan and Core-restart durability, real Chromium). S4-A acceptance:
`scripts/e2e-s4a.sh` (real Core/Worker/verifier/reviewer + deterministic Paseo
CLI + real Chromium candidate/Office journeys); `scripts/e2e-s4a-live.sh` runs
one bounded real Paseo session when authorized. S4-B acceptance:
`scripts/e2e-s4b.sh` (two concurrent Core schedulers, native campaign graph,
Core-spawned verifier/reviewer, automatic repair, crash/restart and the Office
campaign journey). The compiled
lifecycle experience is exercised by `scripts/e2e-factory-cli.sh`; it builds
the distribution, runs `factory` from outside the source directory, and checks
auth, readiness, logs, stop/restart, and SQLite retention.
Contracts: `docs/contracts/project-action-s1.md`, `docs/contracts/factory-events-s1.md`,
`docs/contracts/run-worker-s2.md`, `docs/contracts/run-paseo-s3a.md` (S3-A:
execution modes, pinned routing, durable session identity),
`docs/contracts/agent-observability-s3c.md` (S3-C: cross-Run session
projection and evidence levels), `docs/contracts/quality-pipeline-s4a.md`
(S4-A: frozen spec/candidate, independent evidence, deterministic gate, browser
checks), and `docs/contracts/campaign-orchestration-s4b.md` (S4-B: Core-owned
Campaign/Task scheduling, repair/recovery and Office projections).

## Local lifecycle CLI

Build a compiled local distribution from the repository root:

```sh
scripts/build-factory.sh
```

Put `build/factory` on `PATH` or invoke that binary directly. The distribution
contains the Go `factory` CLI, Go `factory-core`, and built Office assets. The
CLI works from any current directory and serves the Office bundle through Core:

```sh
factory start
factory status
factory worker start
factory worker status
factory logs -n 50
factory logs -n 50 worker
factory worker stop
factory stop
```

The Worker child process receives a minimal explicit environment
(`PATH`, `HOME`, `XDG_RUNTIME_DIR`, `TMPDIR` plus its scoped
`FACTORY_WORKER_TOKEN`); nothing else from the operator shell — no
provider credentials, no GitHub tokens, no operator API token — is
propagated. Its default execution mode is rootless Podman and fails
closed when Podman is unavailable; `--exec-mode local` selects the
explicit pure-Go fixture for tests/dev only. SEC-01 remains
PAUSED/BLOCKED: this boundary is credential hygiene, not a certified
sandbox.

By default, its private state directory is `~/.local/share/software-factory`
(or `$XDG_DATA_HOME/software-factory`); set `FACTORY_HOME` to choose another.
`FACTORY_ADDR` changes the loopback address `127.0.0.1:8471`; non-loopback
addresses are refused. Start
waits for the Core API readiness endpoint and prints the Office URL, database
path, log path, and API token file path. The token is stored with mode `0600`,
passed only to the Core process environment, and never printed or written to
persistent logs. Paste its value into the Office token gate.
