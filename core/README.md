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

Environment: `FACTORY_API_TOKEN` (required; a random ephemeral token is
generated and printed when unset), `OTEL_EXPORTER_OTLP_ENDPOINT` (optional;
business writes never depend on it), `LOG_LEVEL` (debug|info|warn|error).

## Layout

- `cmd/factory-core` — entrypoint, flags, startup/shutdown.
- `internal/domain` — Action lifecycle, event schema, validation (no deps).
- `internal/store` — SQLite WAL: atomic action+event writes, idempotency,
  replay cursors, restart retention.
- `internal/api` — auth, correlation IDs, JSON errors, SSE stream, static UI.
- `internal/telemetry` — redacted JSON logs, correlation IDs, OTel setup.

Full acceptance: `bash scripts/e2e-s1.sh` from the repo root.
Contracts: `docs/contracts/project-action-s1.md`, `docs/contracts/factory-events-s1.md`.
