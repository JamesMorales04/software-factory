# F1-S3-C engineering checkpoint

> **Status:** implementation and local evidence recorded on branch
> `f1-s3c-agent-observability`, based on `c065df6`. This note is a recovery
> checkpoint, not an acceptance verdict or a contract.

## Delivered slice

- Read-only `GET /api/v1/sessions` projection with exact status/provider
  filters, bounded limit and a durable `eventCursor`.
- Functional Agents view: live/history grouping, identity, Worker/Attempt/
  generation, `TRUSTED_LOCAL`, filters and explicit loading/empty/error/
  disconnected states.
- Run-detail Paseo panel: per-Attempt bindings, requested vs observed route,
  bounded S3-B evidence, cleanup confirmation and honest orphan warnings.
- Durable lifecycle timeline with whitelisted fields (no raw payload dump).
- S3-C worker hardening: `diffExcerpt` is no longer persisted; legacy
  event/SSE read projections recursively drop that key.
- Deterministic S3-C Core/Worker/Chromium acceptance lane using a stubbed
  `paseo` CLI, including reload, malformed-SSE reconnect, orphan and
  Core-restart durability phases.

## Local evidence commands

```sh
(cd core && gofmt -l . && go vet ./... && go tool golangci-lint run ./... && go test -race ./...)
(cd office && npm run typecheck && npm run lint && npm run build)
bash scripts/e2e-s1.sh
FACTORY_S3B_LIVE=0 bash scripts/e2e-s3b.sh
bash scripts/e2e-s3a.sh
bash scripts/e2e-s3c.sh
```

`scripts/e2e-s2.sh` Podman phase was BLOCKED locally by an unsupported
overlay-on-btrfs storage backend; the S2 browser journey passed separately
against an explicit `--exec-mode local` synthetic worker. That is a local
environment result, not Podman certification.

## Live validation

One bounded real session, `opencode/opencode-go/deepseek-v4.1-flash`
mode `build`, through Factory Core → Worker → Paseo: succeeded with a real
diff (284 bytes), format/test exit 0, `cleanup: ok`, and matching observed
route in Office. No second paid session was run.

## Known limitations

- Level 3 live per-tool streaming is **not implemented**; see
  [the S3-C contract](../contracts/agent-observability-s3c.md) for the exact
  capability gap and prerequisites.
- Legacy S3-B run/attempt result snapshots may retain a historical
  `diffExcerpt`; Office does not render it and event projections filter it.
  Removing stored legacy data needs an authorized retention decision.
- S3-D daemon/Worker restart reconciliation remains out of scope.
