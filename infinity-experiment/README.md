# Infinity-first Factory experiment

**Status:** experimental implementation, not a production migration.
**Baseline:** `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e` (reviewed Wave 3 code baseline).
**Campaign HEAD (documentation checkpoint, not used as code base):**
`0bcefa0cbdf9cc57d657e54463ac9b5ed50597c0`.
**Branch:** `experiment/infinity-runtime-20261102`.
**Worktree:** `/home/james/Escritorio/Development/sf-wave3/infinity-runtime`.
**Infinity pin:** `1abc6fcff1f186124f924b56aa92c3a3684c90c5`.

This crate is deliberately outside the production Factory execution path. It
exists to answer one question with running code: **should Factory replace its
custom agent-execution and orchestration infrastructure with Infinity Runtime?**

No real provider inference was performed. The model is a deterministic,
offline `ModelProvider`; all provider routes are guarded by fail-closed
subscription attestations.

## What runs

One Director root thread delegates to two implementers with
`fresh_context: true`, then to an independent reviewer. Each child:

1. receives a self-contained task in its own thread;
2. calls `engineering_task`, which is an asynchronous Infinity tool;
3. journals an external-effect intent, applies a verifiable marker, and
   returns a callback;
4. reports to the parent and closes.

The reviewer integrates both disposable workspaces, runs the full Python test
suite, creates a deterministic Git candidate, then re-runs the suite in a
separate copy and independently recomputes the candidate digest. The Director
aggregates the three reports into a candidate summary.

The RAP test is a second, narrower journey: Infinity calls `run_command`, a
local RAP server performs one idempotent fixture effect, POSTs a
`RapCallback::ToolResult`, and the bridge resumes the Infinity thread.

```text
Director (root, fresh child contexts)
├── implementer-multiply ── engineering_task ── effect journal ── callback
├── implementer-tests    ── engineering_task ── effect journal ── callback
└── reviewer             ── integrate + test + candidate + independent re-test
        │
        └── report_to_parent ── Director aggregation
```

## Durable model

`SqliteStore` implements both `ConversationStore` and `StateStore`. It uses
SQLx/SQLite, WAL, `synchronous=FULL`, `BEGIN IMMEDIATE` for append/spawn/effect
transactions, a `(thread_id, dedup_id)` uniqueness constraint, and per-thread
ordinals. The `EffectJournal` adds:

```text
INTENT -> STARTED -> APPLIED -> COMPLETED
                 \-> AMBIGUOUS (never replayed automatically)
```

A callback is unique per `(effect_key, attempt)`, claimed atomically
`PENDING -> DELIVERING`, and fenced by attempt. A stale callback from an old
attempt is rejected. A crash after the marker but before the callback is
recovered by verifying the marker hash, promoting the effect to `COMPLETED`,
and re-delivering the callback under its original identity. An ambiguous effect
gets one explicit "operator must reconcile" tool result so the thread cannot
become a zombie.

This matters because the pinned Infinity revision does **not** contain upstream
PR #178 (crash-safe append-only daemon persistence). Its daemon still rewrites
whole JSON snapshots with `std::fs::write` and ignores errors. The prototype
therefore does not reuse that persistence as if it were production-safe.

## Run it

```sh
cd infinity-experiment

# deterministic journey
cargo run --bin infinity-experiment -- run \
  --root /tmp/factory-infinity-demo --run-id demo \
  --out /tmp/factory-infinity-demo/report.json

# crash after the reviewer's external marker, then recover in a new process
cargo run --bin infinity-experiment -- start \
  --root /tmp/factory-infinity-restart --run-id restart \
  --crash-after-effect review
# exit code is 23 by design
cargo run --bin infinity-experiment -- resume \
  --root /tmp/factory-infinity-restart --run-id restart \
  --out /tmp/factory-infinity-restart/report.json

# subscription-route matrix (deterministic JSON; no inference)
cargo run --bin infinity-experiment -- subscription \
  --out evidence/subscription/feasibility-matrix.json
```

## Test and evidence commands

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

scripts/restart_evidence.sh evidence/restart-evidence.json
scripts/compare.sh evidence/comparison-$(date +%Y%m%d)
```

`scripts/compare.sh` measures the prototype and the existing Factory
deterministic fixture-dispatch E2E with a process-tree sampler. The Factory
test includes Core/Supervisor/Worker/Verifier/Reviewer governance, so the
comparison is **system-level**, not an agent-loop microbenchmark. See
`evidence/comparison-20261103/summary.md`.

## Subscription constraints

The matrix records four distinct facts rather than conflating them:

- **OpenCode Go / DeepSeek V4.1 Flash** is an official subscription completion
  route through the Go inference endpoint. It is allowed only behind an Owner
  attestation that Extra Usage and credit/balance fallback are disabled, no
  API-key billing credential is present, the model is allowlisted, and a
  separate live-test authorization exists. `opencode.allowance` and
  `billing_source` are **detectors**; they cannot undo a charge.
- **Codex app-server / SDK** is an official external coding agent using local
  ChatGPT authentication. It is not a basic completion provider. Wrapping it as
  `ModelProvider` would lose approvals, tool semantics, context and
  cancellation boundaries; the prototype refuses to do that.
- **Sign in with ChatGPT** for third-party apps requires a registered client
  ID. Factory has none in this experiment, so the route is blocked.
- **API-key pay-as-you-go, Extra Usage, credits/balance fallback, AWS Bedrock,
  and unregistered ChatGPT OAuth completion** are forbidden.

No entitlement API is fabricated. No response metadata is claimed to prevent a
charge that already occurred.

## Limitations

- Deterministic fake model only; no live engineering quality or subscription
  consumption result.
- The default workspace runs fixed commands on the host with a cleared
  environment and timeout; **not** a security sandbox. An optional rootless
  Podman mode runs the same fixed commands with no network, dropped
  capabilities, read-only rootfs, resource limits and a workspace mount. It was
  exercised on this host, but it is **not** SEC-01 certified isolation.
- No Office projection, Core authorization/budget/delegation, Quality Gate,
  GitHub delivery, or production candidate acceptance.
- The local Infinity driver is single-process (`Rc`/`LocalSet`); a production
  multi-tenant embedding would need a per-session worker boundary and a
  durable input inbox.
- `cargo test` durability/integration tests were run on this Linux host; this
  is not independent verification and not a Windows result.
- Independent GPT-6 Luna review is **BLOCKED**: no separate real-inference
  authorization envelope exists. The packet is in
  `evidence/luna-review-request.md`; do not run it without Owner authorization.
