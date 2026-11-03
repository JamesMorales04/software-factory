# Wave 3 closure checkpoint — 2026-11-03

**Campaign branch:** `campaign/office-ux-wave3-20261028`
**Code candidate:** `54f653736a97eaf2c5cbb023feba37962ae4ccbe`
**Remote `main` (unchanged):** `d58725266d132fafb96a79970ff292acc876feb8`
**Production `test-support`:** absent; the feature is rejected outside debug
builds. **Paid inference:** none performed. **Deployment:** none.

This checkpoint records the integrated Wave 3 candidate. It does not promote
the campaign to `main`, certify isolation or claim live-model acceptance. The
[container-native status](container-native-status.md), [native Rust closure
ledger](native-rust-closure.md), [Office parity
matrix](../product/office-parity.json) and [reliability debt
register](../quality/reliability-debt.md) remain the status authorities.

## Integrated pull requests

| PR | Head | Merge commit | Review | Required checks |
|---|---|---|---|---|
| #84 SQLite execution-check race | `38f314f` | `6eb8553` | GPT-6 Luna **APPROVE** | architecture, compose-policy, rust, container-dev all PASS |
| #85 Go route identity + funding readiness | `ecb08a0` | `15b136b` | GPT-6 Luna **APPROVE** | all four PASS |
| #82 Agent Office V3 + visual repairs | `d871a0c` | `ac7e366` | GPT-6 Luna **APPROVE** (third round) | all four PASS |
| #79 real-provider acceptance harness | `ac4b523` | `0be64d7` | Harness-only; exact-head CI PASS | all four PASS |

Earlier in the campaign, #80 (conversational Home) and #83 (fail-closed
subscription funding) merged at `4f732ff` and `25e3478`. #81 is closed as
superseded.

Direct campaign integration commits after the PR merges:

- `85edf2e` — make `cargo xtask e2e dispatch/provider` pass the debug-only
  `test-support` feature, so the provider suite no longer reports a false pass
  with zero matching tests.
- `7e2290c` — add a `core-e2e` debug Core stage and select it only in
  `compose.e2e.yaml`; seed the disposable deterministic scopes.
- `603c761` — remove the approval-to-Campaign admission race with a debug-only
  `*` E2E admission that cannot override a persisted funding policy; add a
  regression.
- `54f6537` — correct the E2E admission comment.

## Verification matrix

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --all -- --check` | **PASS** | integrated candidate |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | **PASS** | integrated candidate |
| Workspace tests with `test-support` | **PASS** | 710 passed, 0 failed, 2 ignored across 33 test binaries; the two environment-dependent OpenCode sandbox tests were skipped on this host and ran in the container lane |
| Production `cargo build --bins --locked` (no feature) | **PASS** | integrated candidate |
| `cargo doc --workspace --no-deps` | **PASS** | integrated candidate |
| `cargo xtask migrate` | **PASS** | clean database, migrations through 0042 |
| `cargo xtask e2e recovery` | **PASS** | 1 test |
| `cargo xtask e2e dispatch` | **PASS** | 12 tests after the feature fix |
| `cargo xtask e2e negatives` | **PASS** | 5 tests |
| `cargo xtask e2e provider` | **PASS** | 1 provider-backed test |
| `factory-architecture-check --base-ref origin/main` | **PASS** | architecture governance |
| `factory-repo-hygiene --mode strict` | **PASS** | 0 findings |
| Office typecheck / Biome / Vitest / build | **PASS** | 145 files linted; 283 tests in 36 files |
| `cargo xtask e2e office` | **PASS** | real Core/Worker + Chromium; candidate `e9dd05cf573b66afaa4ee8effa3a77e71b8482df`, gate `ACCEPTED` |
| `cargo xtask e2e intake` | **PASS** | request `46f4765b`, Campaign `02092233`, `DETERMINISTIC_FAKE_MODEL` |
| `cargo xtask e2e intake-full` | **PASS** | candidate `a446324c`, verifier + MODEL reviewer, gate `ACCEPTED`, restart/reload persistence |
| `cargo xtask check-container` in the dev image | **PASS** | image `43cebaa1770f`; containerized Rust gates, docs and audit |
| Containerized Office gates | **PASS** | typecheck, Biome, Vitest and build in the dev image |
| Canonical `scripts/compose-e2e.sh` | **PASS** | request `edbfd5bd`, Campaign `1e00fe3d`, candidate `fca90be5`, gate `ACCEPTED`, `DETERMINISTIC_FIXTURE`, restart persistence, `paid_inference:false` |
| Real-Core Office captures | **PASS (self-test)** | `docs/evidence/agent-office-v3-real-core-2026-11-03/`; 1440/834/390 full-page captures and manifest |

The Compose E2E uses a debug-only `core-e2e` image with
`test-support`; the production `core` target remains a release binary with no
such feature and no admission endpoint. A persisted subscription policy still
takes precedence over the E2E wildcard.

## Agent Office V3 outcome

The rebase preserved the merged V2 duplicate-winner inspector and added the V3
continuous studio depth layout. Real-Core captures exposed and verified three
repairs:

1. a white-on-white inspector empty state caused by a missing studio
   `--surface-inset` token;
2. missing dark semantic foreground tokens for studio badges and controls;
3. avatar appearance assigned by list rank, which changed when an earlier row
   was filtered. Appearance is now seeded from the stable person `identity_id`
   within the injective look range, and probing compares rendered signatures.

GPT-6 Luna approved the final Office head. Product Owner visual sign-off is
**not claimed**. The 0/1/5/12/30 participant counts are exercised by component
tests; the real journey projects four participants, not 12 or 30.

## SQLite `Busy` flake outcome

The recorded CI failure occurred in
`db::provider_sessions::tests::execution_check_receipts_bind_the_frozen_command_and_candidate`.
The tool-event row was inserted before the execution check was validated, and a
rejected call relied on the asynchronous rollback queued by
`Transaction::drop`. Writer transactions also used SQLite's deferred `BEGIN`,
so a read-to-write upgrade could lose its snapshot and return
`SQLITE_BUSY_SNAPSHOT`, which bypasses the busy handler.

The fix resolves every execution-check fact before writing, starts writer
transactions with `BEGIN IMMEDIATE`, awaits rollback explicitly, and adds
regressions for rejected writes and ordered concurrent writers. The exact
interleaving was not reproduced locally; the failure mode is documented without
claiming a deterministic reproduction. [REL-001](../quality/reliability-debt.md)
remains open pending independent failure-injection and repeated clean-checkout
stress evidence.

## Subscription-only funding outcome

Production dispatch remains fail-closed. The campaign adds:

- exact `opencode-go` provider identity for the Go subscription route; plain
  `opencode` is rejected as an API-key route;
- authenticated `GET /funding/subscription/{provider}`, which reports
  `coreVerifiable:false`, `admissible:false`, `authenticationClass` from the
  stored credential, unknown quota and the exact missing evidence;
- a rebased real-provider harness that refuses a readiness-only response with
  `CORE_SUBSCRIPTION_READINESS_UNVERIFIED` before creating any Project or
  Request.

No provider-issued entitlement, no-overage guarantee, quota observation or
atomic request admission exists. An Owner Console observation remains
`OWNER_ATTESTED` and cannot be promoted by hashing or re-export. Live inference
is therefore **BLOCKED** pending separate Owner authorization and a reviewed
trust anchor.

## Access and security stack

PRs #36–#38 remain **open and unmerged**. The campaign already contains PR #36's
device-session code (`bda12f0` and the pre-hardening `c8bf930`) but not the
later access repairs (`7bca751`, `ad04aca`, `603f823`). Promotion of the campaign
to `main` would therefore carry partially hardened access code; this must be
reconciled before any promotion decision.

A read-only audit confirmed: legacy-token cleanup, device revocation, pairing
replay protection, restricted Office socket separation, Tailscale restrictions
and credential isolation are present in different degrees across the stack,
but the stack is not safe to promote as-is. The recovered branch
`factory/access-repairs/20261029` at `d496624` now carries two bounded repairs:

- the `./factory open/recover` health probe uses the bound `core-control` alias
  with a Host header and bounded curl timeouts;
- a close/reopen test covers session persistence, replay rejection, persisted
  revocation, the epoch fence and an isolated stale-approval rejection.

Neither repair is independently reviewed at the exact head. Authenticated
Tailscale Serve, mobile-data access, a real Compose first-run after the launcher
change, and full-stack promotion remain **NOT_TESTED**. Product Owner
authorization is required for any access-stack merge to `main`.

## NOT_TESTED

- Live OpenCode Go or Codex inference; verified subscription entitlement,
  no-overage guarantee, provider quota recovery or atomic request admission.
- Product Owner visual sign-off, 12/30 real-participant browser projections,
  and independent acceptance of the Office captures.
- Authenticated Tailscale Serve over the Office Unix socket, mobile-data access,
  real Compose first-run/recovery after the launcher repair, and access-stack
  promotion.
- ISOLATED execution, SEC-01 certification, deployment, Windows and WSL2.
- Docker Compose through the rootless-Podman socket (Podman is the exercised
  path).

## Promotion recommendation

**Do not promote to `main` yet.** The campaign is a coherent, tested Wave 3
candidate for continued review, but it is not a certified product: live-model
acceptance is blocked by funding evidence, the access stack is unresolved, and
the campaign contains unreconciled partial access code. Keep `main` at
`d587252`. Any promotion requires separate Owner authorization, access-stack
reconciliation and exact-head security review.

## Next Factory-native architecture milestone

A separate initiative, not part of this Wave 3 closure, should evaluate
measured deficiencies in: durable asynchronous events, WorkGraph suspension and
resumption, minimal subagent context, callback deduplication and fencing,
restart reconciliation, and conversation compaction. It must reuse Factory's
authoritative SQLx state and the existing Codex/OpenCode adapters, and must not
introduce a second orchestration engine or import the Infinity Runtime
experiment (`experiment/infinity-runtime-20261102` at `21f0366`).
