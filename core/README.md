# Factory Core (F1-S1)

Go modular monolith: domain rules, SQLite WAL persistence, authenticated
HTTP API, durable event ledger, best-effort OpenTelemetry.

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
`OTEL_EXPORTER_OTLP_ENDPOINT` (optional; business writes never depend on it),
`LOG_LEVEL` (debug|info|warn|error).

## Layout

- `cmd/factory-core` — entrypoint, flags, startup/shutdown.
- `internal/domain` — Action lifecycle, event schema, validation (no deps).
- `internal/store` — SQLite WAL: atomic action+event writes, idempotency,
  replay cursors, restart retention.
- `internal/api` — auth, correlation IDs, JSON errors, SSE stream, static UI.
- `internal/telemetry` — redacted JSON logs, correlation IDs, OTel setup.

Core acceptance: `scripts/e2e-s1.sh` from the repo root. The compiled
lifecycle experience is exercised by `scripts/e2e-factory-cli.sh`; it builds
the distribution, runs `factory` from outside the source directory, and checks
auth, readiness, logs, stop/restart, and SQLite retention.
Contracts: `docs/contracts/project-action-s1.md`, `docs/contracts/factory-events-s1.md`.

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
factory logs -n 50
factory logs -f
factory stop
```

By default, its private state directory is `~/.local/share/software-factory`
(or `$XDG_DATA_HOME/software-factory`); set `FACTORY_HOME` to choose another.
`FACTORY_ADDR` changes the loopback address `127.0.0.1:8471`; non-loopback
addresses are refused. Start
waits for the Core API readiness endpoint and prints the Office URL, database
path, log path, and API token file path. The token is stored with mode `0600`,
passed only to the Core process environment, and never printed or written to
persistent logs. Paste its value into the Office token gate.
