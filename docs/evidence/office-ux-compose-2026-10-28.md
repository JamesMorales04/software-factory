# Supported Compose acceptance — 2026-10-28

**Status:** PASS for the unified-intake Compose lifecycle at code candidate
`7063321` (run `40765699005`). All runs are author-dispatched CI with
deterministic fixtures; no paid inference was used and this is not independent
acceptance.

All runs are author-dispatched CI, not independent acceptance or Product Owner
UAT. The model and coding provider are deterministic fixtures; no paid
inference was used.

## Run history

| Run | SHA | Result | Scope |
|---|---|---|---|
| [40738776286](https://github.com/JamesMorales04/software-factory/actions/runs/40738776286) | `770a200` | PASS | legacy Action/Campaign lifecycle, candidate `0df6eed6…`, attempt `3f7a92a2…`, Gate `ACCEPTED`, restart persistence `true` |
| [40742928074](https://github.com/JamesMorales04/software-factory/actions/runs/40742928074) | `056c502` | PASS | first unified-intake extension, request `bfafdc1b…`, Campaign `7464a25e…`, candidate `be2c44cd…`, Gate `ACCEPTED` |
| [40750678825](https://github.com/JamesMorales04/software-factory/actions/runs/40750678825) | `b4a15b6` | PASS | integrated Wave 2: legacy candidate `4a869af6…`; intake request `cce4f85e…`, Campaign `5bb3a2d6…`, attempt `f663d098…`, candidate `d89b6417…` at `50429082…`, verifier `32cbef7a…`, reviewer `cc8ea452…`, Gate `ACCEPTED`, restart persistence `true` |
| [40752448629](https://github.com/JamesMorales04/software-factory/actions/runs/40752448629) | `cf42718` | CANCELLED | hung acceptance step; not evidence |
| [40754605706](https://github.com/JamesMorales04/software-factory/actions/runs/40754605706) | `cf42718` | FAIL | intake attempt `9847e28f…` FAILED; Worker `Supervisor finalize failed: 409 Conflict` |
| [40757937905](https://github.com/JamesMorales04/software-factory/actions/runs/40757937905) | `b1042ce` | FAIL | intake attempt `999611fa…` FAILED; intake child execution exited `0` after ~6 s, but its 5 s operation grant had expired and the Supervisor fence cancelled the workspace; finalize `409` |
| [40763932432](https://github.com/JamesMorales04/software-factory/actions/runs/40763932432) | `7d0c67b` | **PASS** | after the operation-grant fix: legacy candidate `65e9cdf1…`; intake request `ffee44ca…`, Campaign `58564ae0…`, attempt `7f397451…`, candidate `25a6a4cc…` at `c271ce0a…`, verifier `27d19b09…`, reviewer `94e771d5…`, Gate `ACCEPTED`, restart persistence `true` |
| [40765699005](https://github.com/JamesMorales04/software-factory/actions/runs/40765699005) | `7063321` | **PASS** | final code candidate: legacy candidate `934e8dec…`; intake request `10ebbe0f…`, Campaign `e3fae868…`, attempt `e7d48446…`, candidate `9ef6983e…` at `5fe84636…`, verifier `bcd30867…`, reviewer `a0493267…`, Gate `ACCEPTED`, restart persistence `true` |

The `b4a15b6` tree contains the identical native runtime code used by
`cf42718`/`b1042ce`; the later commits changed Office localization and the E2E
harness only. The failure is therefore treated as an unresolved runtime
reliability defect, not a UX regression.

## Failure detail (failed runs)

`compose.yaml`/`compose.e2e.yaml` start the real rootless Compose stack:
Core/Office, Supervisor, Worker, nested runtime engine and broker, verifier,
reviewer, Git fixture and the E2E-only model fixture. The legacy Campaign
completes with a candidate, receipts and Gate `ACCEPTED`.

The unified-intake Campaign is assessed and approved, then its Worker fails
before producing a candidate:

- Worker log: `sandbox execution failed: Supervisor finalize failed: 409 Conflict`
- runtime broker: the intake child execution actually exits `0` after about six
  seconds; the earlier `exit_code=Some(1)` child belonged to a different
  workspace
- Core projection: one intake Attempt `FAILED`, no candidate, no receipts

The fixture provider writes `src/provider.txt`, then issues a frozen
`run_command` grep that takes about six seconds. Core issued the operation grant
with a five-second lifetime. The Supervisor operation fence polls Core every
500 ms; once the grant expired it cancelled the still-valid workspace with
`cancel_active`, leaving a cancellation marker, and the subsequent finalize
returned `409 Conflict`. This is a real Core/Supervisor contract defect, not a
model or fixture failure.

The fix extends the signed operation grant for `execute`/`run`/`integrate` to
the maximum bounded execution runtime plus a settlement margin; the fence still
revalidates the grant and the durable attempt/lease authorization with Core
every 500 ms. Quick unfenced `write`/`apply`/`finalize` operations keep a
five-second admission lifetime. The unified-intake Compose runs `40763932432` and
`40765699005` passed after this fix. The final code candidate is `7063321`.

## What the passing runs exercised

- Real rootless Podman Compose, Core/SQLite on a durable volume, init,
  Supervisor, Worker, nested runtime engine/broker, verifier, reviewer and
  disposable Git fixture.
- Project import from the disposable Git fixture.
- Legacy Action/Campaign: scoped delegation, authorization, Worker lease and
  candidate, verifier/reviewer receipts, deterministic Gate `ACCEPTED`.
- Unified intake: exact profile/plan/action identity, assessment through the
  deterministic model fixture, freeze, operator approval, Worker Lead Attempt,
  candidate, changed path `src/provider.txt`, verifier/reviewer receipts,
  deterministic Gate `ACCEPTED`, request/Campaign projections, and
  `compose down`/`up -d` restart persistence.
- The E2E-only `model-fixture` serves OpenAI-compatible responses from a
  read-only context volume and receives no operator, GitHub or provider
  credential.
- Test-only plain HTTP is limited to `FACTORY_ALLOW_TEST_UPSTREAM=1` plus an
  exact `FACTORY_TEST_UPSTREAM_HOSTS` entry; production remains HTTPS-only.

## Isolation defect fixed

The first CI run failed because `compose.yaml` declares `name:
software-factory`, overriding the E2E project guard. The script now forces
`podman compose -p "$PROJECT"` and prints bounded container diagnostics. This
was a real safety defect: without it, a failed lookup could target the owner's
stack.

## Limitations

- No live provider inference; no Compose Office/Chromium acceptance.
- No independent verifier/reviewer acceptance.
- The unified-intake Compose path is flaky and currently fails; do not cite it
  as a passing final acceptance.
- Local Compose remains blocked on this host by the buildah/btrfs
  context-overlay limitation; CI is the supported environment.
- Multi-unit fan-in, repair cycles, provider cancellation and real provider
  sessions remain unexercised.
