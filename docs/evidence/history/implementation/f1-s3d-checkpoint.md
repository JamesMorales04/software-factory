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

## Luna review repairs

Luna (Codex GPT-6 Luna, read-only) found three material gaps on the second
candidate, all repaired here:

- dispatch filtered only the oldest 200 queued Runs, so 200 unresolved
  paseo Runs could starve later synthetic/eligible work. Dispatch eligibility
  is now a SQL predicate, with a 205-gated-run regression test;
- unbound-session discovery did not include the workspace label when the
  workspace ID was not yet bound. Recovery now discovers the exact-title
  workspace first and uses the full five-label session set; without an owned
  workspace, unbound cleanup is blocked;
- the D5 concurrent journey was not overlapped. The simulator now has a
  slow-cleanup mode, both recovery Workers are launched before the claim
  completes, and a store-level concurrent-claim test proves one winner.

Luna also flagged unbounded claims when a Worker dies before completion.
Claim grant now enforces the 8-attempt budget and records a
`retry-exhausted` blocked receipt.

## Final liveness repair

A targeted source inspection found that `ResolveBlockedPaseoRetries` scanned
the oldest 200 queued paseo Runs before evaluating recovery state. With 200
older pending/unconfirmed rows, a later terminal `blocked` Run could remain
queued indefinitely. A deterministic 201-Run regression reproduced the
starvation; the sweep now filters `blocked` recovery in SQL. The test also
proves pending/unconfirmed Runs are not failed and unrelated synthetic
dispatch is unaffected.

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
