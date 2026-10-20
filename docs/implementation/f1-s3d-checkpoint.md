# F1-S3-D engineering checkpoint

> **Status:** implementation and local evidence recorded on branch
> `f1-s3d-paseo-recovery`, based on `baf7aa6` (S3-C merged). This note is a
> recovery checkpoint, not an acceptance verdict or a contract.

## Delivered slice

- Durable correlation key `factory-s3d:<runId>:g<generation>` and exact
  session labels; workspaces discovered by the full deterministic title.
- Orphan recovery records with transactional claims, 5-minute cleanup claim
  fencing, bounded retries, two-check absence and separate receipts.
- Worker recovery loop and `--recovery-only` coordinator mode; exact
  identity validation before any stop/archive.
- `paseo-task` retry gate: no replacement Attempt until the prior external
  generation is confirmed or absent; blocked recovery fails the Run closed.
- `PaseoSessionRecoveryRecorded` on the existing run ledger.
- Orphan transitions materialize a pending cleanup target immediately, so
  the Office projection can show "cleanup pending" before any claim.
- Recovery status in the Agents and Run-detail projections.
- Deterministic D1-D7 suite, pending/confirmed/blocked browser visibility,
  and an opt-in single-session live lane.

## Local evidence commands

```sh
(cd core && gofmt -l . && go vet ./... && go tool golangci-lint run ./... && go test -race ./...)
(cd office && npm run typecheck && npm run lint && npm run build)
bash scripts/e2e-s1.sh
bash scripts/e2e-s3a.sh
FACTORY_S3B_LIVE=0 bash scripts/e2e-s3b.sh
bash scripts/e2e-s3c.sh
bash scripts/e2e-s3d.sh
```

Local S2 Podman acceptance remains BLOCKED by the host's overlay-on-btrfs
storage backend. It is not reported as PASS; CI is the authoritative S2
environment for that gate.

## Independent verification repairs

The first independent verification found contract-fidelity defects (year-1
zero timestamps in pending/claimed projections, missing `retryCount` in the
recovery event, no explicit `retry-exhausted` reason, completion returning
409 instead of 404 without a receipt, and no pending browser check). All were
repaired on the new candidate; the browser lane now proves pending,
confirmed and blocked recovery states. The verifier's functional findings
(fencing, concurrency, exact identity, cleanup confirmation) were PASS.

## Limitations

- Real Paseo daemon restart is BLOCKED unless an isolated daemon is
  available; D3 uses the deterministic simulator.
- Legacy S3-B unbound rows (no correlation key, no session/workspace ID)
  cannot be positively identified and remain blocked rather than guessed.
- Core fencing guarantees one authoritative result, not exactly-once
  external Paseo side effects.
## Bounded real acceptance (implementer evidence)

One paid session was authorized and used: `opencode/opencode-go/
deepseek-v4.1-flash`, mode `build`, through Factory Core → Worker → Paseo.
The owned Worker was SIGKILLed after the durable binding
(`run 188ff35a9a703f1d4d9c468edd13f05f`, session
`71b68c1d-16ad-43bc-9918-b9dcb3dd4911`, workspace
`wks_1b815878fcc85f10`). The recovery-only Worker archived the session and
workspace; the real Paseo CLI independently reported the session archived
and the workspace absent. The binding stayed `orphaned`, the Run stayed
failed, no `RunCompleted` was recorded, and no second paid session was
started. This is implementer evidence, not independent verification.
