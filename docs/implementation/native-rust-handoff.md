# Native Rust implementation handoff

## 2026-10-24 cutover campaign checkpoint

PR #17 remains Draft on `native-rust-replacement-20261023`. The starting code
HEAD was `102f6a046f1f907546e8bc60da622388f80bd95c` with 99 tracked Go
files. Batch 1 (`1d10f06`) replaced and deleted a protected `.go` test datum;
the S4-B B8 human-required oracle passed, while its separate legacy B11 browser
journey timed out. Batch 2 (`c2923ac`) replaced and deleted the two Go
architecture-checker files and wrapper. Sol cross-reviewed the port and fixed
evidence read confinement (`eb36cad`); independent review found missing
negative oracles, repaired in `2cdd773` and `4fb29e9`. Current Go count is 96.

Luna's WorkGraph process test exposed an infinite dependency-failure loop;
`a32c6d5` fixes it and makes Worker report failures visible. The real
Core/Supervisor/Worker failed fan-in dependency test passed, and an independent
review found no blocker in that slice. `d50bd0f` adds a pre-dispatch Core
restart to the real fan-in acceptance process test; independent review found
the recovery oracle meaningful, and the repaired test preserves HTTP 201.
Locally, seven Rust governance tests,
format, Clippy, architecture validation, the failed-dependency process test,
Python PR-impact and authorization tests, and the Rust Office browser journey
passed on their recorded code commits. `cargo test --workspace --locked`,
workspace build and docs passed at `d47f6cd`. `cargo audit` initially warned
that newly added `serde_yaml_ng` depended on unmaintained `unsafe-libyaml`.
Commit `dd91272` replaced it with `serde-saphyr` 2.0.0; seven checker tests,
the base-ref invocation and a clean `cargo audit` pass locally on that code.
The Rust `architecture-consistency` GitHub job passed on `6947b42` after the
PR impact declaration was rebound to its exact head. Commit `6bf9657` added
parser-negative coverage, bringing the checker to eight focused tests.
Luna's Codex thread/turn lifecycle fixture slice was integrated at `911ce5d`;
Sol fixed event replay and usage-overflow atomicity at `37e387b`, then an
independent reviewer found a pre-turn cancellation dead end, repaired at
`c375d71`. The reviewer verified both repairs. Local workspace tests,
format and strict Clippy passed on `c375d71` (132 library tests and all other
workspace targets); Codex interrupted-outcome regression coverage followed.
Luna's OpenCode SSE/HTTP response fixture helpers landed at `17bcac2`.
Independent review found that idle after an abort request cannot prove
cancellation; `5a5df79` records `AbortRequestedAndIdle` and adds a completion
race fixture. The reviewer verified this repair. No production prompt or abort
request path uses those helpers yet, and no paid turn occurred.
Luna's real-process fan-in failure test landed at `3840b39`: two root unit
candidates are recorded, Supervisor is stopped before the integration Worker,
and the Lead request and integration become FAILED. The dependent unit has no
Attempt, no integrated candidate exists, and the integration's stored lineage
matches the two root commit IDs. The focused test passed in Luna's worktree
and on the root candidate. Natural Git conflicts are unreachable in this
fixture because frozen unit scopes are disjoint; adversarial integration
assessment is still open. An independent reviewer found no blocking false
positive, verified the focused test, and confirmed this is Supervisor
unavailability rather than a Git merge-conflict oracle. The
`architecture-consistency` GitHub job passed again at `a367766` after its PR
impact declaration was rebound to the exact head.
Final exact-head GitHub CI, strict hygiene and final independent acceptance
remain pending.
Provider coding is still fail-closed; no paid provider turn ran. Isolation and
credential confinement are not certified. The current executable next step is
the real OpenCode prompt/abort/cleanup HTTP path with a local fake server,
followed by in-flight WorkGraph restart and security closure. See the
[closure ledger](native-rust-closure.md) and
[deletion map](native-rust-deletion-map.md) for exact blockers and files.

**Date:** 2026-10-23 UTC. **Repository:** `JamesMorales04/software-factory`. **Branch:** `native-rust-replacement-20261023`. **Base and entry HEAD:** `102d7ba29295554bd61ae71956347a025d219d7b` (`origin/main`). **Verified code commit:** `f255f0c348838e36d15555d7db7242e36d10f842`. Run `git rev-parse HEAD` for the current documentation tip; a commit cannot embed its own SHA. The entry S4-D checkout was dirty at `9c6aa4a` with seven tracked modifications and was left untouched. This branch was created as a separate worktree at `/home/james/Escritorio/Development/software-factory-native-rust`.

## Binding objective and decisions

The Product Owner authorized a clean Rust Core/Worker replacement with direct Codex App Server and OpenCode server integration, React/TypeScript Office, no completed Go/Paseo/predecessor active product, no speculative compatibility layer, and no host fallback for required isolation. [ADR-0018](../architecture/decisions/ADR-0018-native-rust-replacement.md) supersedes the old implementation choice. Core alone owns policy, risk, authorization, budgets, leases, evidence, gate and delivery. A model cannot grant authority. This branch is **not** a completed replacement and must not be merged as a cutover.

Decision mechanisms: canonical Git identity, idempotency, API auth and current non-dispatch are `DETERMINISTIC`. Future semantic assessment is `MODEL` input under `HYBRID` Core validation. Candidate acceptance and gate remain `DETERMINISTIC`. No model assessment or gate is implemented here.

## Implemented files and actual behavior

- `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`, `native/Cargo.toml`: Rust 1.90.0 workspace and dependency lock.
- `native/src/db.rs`: SQLite WAL schema, canonical Git Project registration with frozen observed base commit, Action objective intake, SHA-256 objective digest, durable events, operation/payload-bound idempotency. Project registration normalizes nested paths to the Git top-level directory. Git top-level output strips only its final LF; an identity test uses a directory ending in a space to guard against trimming meaningful path characters. Actions remain `UNASSESSED` and `HUMAN_REQUIRED`; the digest is **not** an ActionSpec.
- `native/src/bin/core.rs`: Axum loopback API, separate operator and Worker bearer tokens, project/action/event endpoints, Worker handshake. No dispatch path exists.
- `native/src/bin/worker.rs`: separate process that validates a loopback Core URL and authenticates. It receives `dispatch_supported=false` and does not execute work.
- `native/src/protocol.rs`, `native/src/runtime.rs`, `native/src/sse.rs`, `native/src/bin/provider_probe.rs`: direct Rust Codex initialize/model-list probe and password-protected OpenCode probe. OpenCode reserves an ephemeral port (`--port 0`) and reads the server's actual loopback listen address. Its result field is `sse_handshake_tested`; it validates only the first `/event` SSE event, specifically `server.connected`, and does not validate progress or reconnect. The probe creates/gets/deletes a session, validates the returned session directory against the canonical workspace, and confirms server cleanup. In the occupied-port negative case, a process already held port 4096; the probe exited 0 using a different server-selected port and made no connection to occupied port 4096. `native/src/sse.rs` has a bounded SSE parser with four unit tests. These are bounded protocol/session checks; they do **not** submit coding work, start paid turns, or establish full adapter compatibility.
- `native/README.md`: build/run scope and environment variables. Documentation index, MASTER banner, ADR-0018 and ADR index mark supersession while older docs remain historical reference until full cutover. Historical ADR-0005 remains unchanged.

## Evidence and limits

Local deterministic commands (run from this branch with `mise exec rust@1.90.0 --` if Rust is not on PATH):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build --bins
mise exec rust@1.90.0 -- cargo audit --file Cargo.lock
cargo run --bin factory-provider-probe -- codex
cargo run --bin factory-provider-probe -- opencode
```

On exact code SHA `f255f0c348838e36d15555d7db7242e36d10f842`, author-run checks passed: `cargo fmt --all -- --check`; strict Clippy (`cargo clippy --workspace --all-targets --all-features -- -D warnings`); `cargo test --workspace` (10/10: 9 library, 1 Worker); `cargo build --bins`; `mise exec rust@1.90.0 -- cargo audit --file Cargo.lock` with cargo-audit 0.22.2 (192 dependencies; RustSec DB HEAD `84d943a6deeb8cc6c6f632531aaf4f1c62275239`); and `scripts/check-architecture-consistency --base-ref 102d7ba29295554bd61ae71956347a025d219d7b`. Codex initialize/model-list and the OpenCode health/OpenAPI/limited SSE handshake/session create-get-delete probes passed; no paid turn was run. The Git trailing-space identity bug was found in independent Luna review and fixed by the author. Local Core HTTP smoke used a disposable Git repository: unauthorized project request 401, registration 201, same-key replay returned same ID, Action creation 201 with `HUMAN_REQUIRED`/`UNASSESSED`, unauthorized Worker 401, Worker handshake success with dispatch false, two events, and one Action persisted after Core restart. This smoke is author-run, not independent acceptance.

Dependency audit on exact code SHA `f255f0c`: pinned `cargo-audit` 0.22.2 **passed** for 192 dependencies using RustSec advisory database HEAD `84d943a6deeb8cc6c6f632531aaf4f1c62275239`. The CI workflow includes an audit step; its PR run is part of the checks currently in progress.

Codex CLI `0.156.1` generated the v2 schema locally; the full local schema SHA-256 was `995fc3b8f8c469f6787e8fc5be4038c4f31359025edd8480b862e83355f3bf3b`. On code SHA `f255f0c`, the Codex probe remains initialize/initialized/model-list only, with probe cleanup confirmed. A separate bounded pre-turn experiment created thread `01a169da-763d-7240-a953-6937793f8360`; resume/archive returned “no rollout,” and a subsequent `thread/read` reported that the thread was not loaded. Durable cleanup for that thread is **not conclusively confirmed**. No turn was started and no paid provider execution occurred. OpenCode `1.18.32` authenticated health and `/doc` OpenAPI 3 probe succeeded; served schema SHA-256 was `46db986090aae41846cd6dbe16225a1d883f0bbcb4c48814008d3f6ce140aa5c`. On `f255f0c`, its no-paid probe starts `opencode serve --port 0`, parses its own loopback listen address, validates the authenticated `/event` first SSE event (`server.connected` only), creates/gets/deletes a session, confirms the returned session directory matches the canonical workspace, and confirms cleanup. No prompt/work was submitted and no paid provider turn was authorized or run. Overall `LIVE_NOT_TESTED` for coding compatibility. OpenCode probe cleanup was confirmed; Codex durable cleanup remains unresolved for the identified thread.

Luna independently reviewed the initial implementation and found a Git trailing-space identity bug; the author fixed it. Luna independently reviewed exact code SHA `f255f0c`, ran formatter, strict Clippy with `--locked`, all 10 tests, binary build, and both no-paid probes, and reported no new high-severity issue in the bootstrap paths. This review does not cover a provider work lifecycle, independent end-to-end acceptance, or a Quality Gate result. Open findings: provider probes have no work submission/turn lifecycle; native authority/leases/sandbox/candidate/gate/UI are absent.

## Repository and delivery state

At campaign entry, only PR #16 was open: `f1-s4-d-intelligent-lead` head `19cb4951f7a3d42a1ca27b1800cf49401c81ecc6`, base main `102d7ba29295554bd61ae71956347a025d219d7b`. PR #12 and #13 are merged into main. PR #16 has historical Go/Paseo S4-D work with incomplete real multi-agent and semantic certification; do not merge it into this branch or transfer evidence. No existing PR was closed or merged. **Draft PR #17** is OPEN; run `gh pr view 17 --json headRefOid,statusCheckRollup` for its current head and checks. On inspected code head `f255f0c`, some checks had succeeded and others remained in progress; the full exact-head CI result was not yet known. PR #17 is not merged or deployed.

## Next tasks, in dependency order

1. Reconcile Codex thread `01a169da-763d-7240-a953-6937793f8360` through a supported inspection/cleanup path and record whether it remains unresolved. Then implement and fixture-test the Codex thread/turn lifecycle (response/event correlation, permissions, terminal status, cancel, timeout, restart reconciliation) and extend OpenCode from the passing health/SSE/session/workspace probe to prompt submission, progress/tool capture, permission handling, inspection/abort, reconnect reconciliation and cleanup. Do not call a started turn successful without terminal evidence. No paid turn is authorized by this handoff.
2. Create Core-owned frozen ActionSpec, risk and delegation authority, per-Action budgets, authenticated Worker assignment with lease/generation/fencing and idempotent receipts. A Worker must not dispatch when isolation or authority is absent.
3. Add Factory-owned per-Attempt workspace allocation and tested sandbox/credential confinement. A Git worktree alone is insufficient. Test restart and stale-Worker recovery.
4. Implement provider-backed Attempt → candidate commit → independent clean-checkout verifier → reviewer → deterministic gate, then a Rust-backed Office journey. Use a disposable external fixture and exact candidate evidence.
5. Add bounded WorkGraphs, semantic evaluation, delegation, independent acceptance, exact-SHA CI, and only then remove obsolete Go/Paseo/predecessor active code/docs/CI in one coherent cutover. Keep Git history.

**Next executable acceptance:** reconcile Codex thread `01a169da-763d-7240-a953-6937793f8360` with the provider's supported inspection/cleanup path and record whether it remains unresolved. Then use deterministic fixtures to validate both adapters' work lifecycle, permissions, failed/interrupted/completed terminal distinctions, cancellation, reconnect inspection, and truthful cleanup. The OpenCode no-paid probe checks startup, authenticated health, a limited SSE handshake (`server.connected`), session create/get/delete, canonical workspace binding, and server cleanup; progress and reconnect remain untested. A live paid turn needs a separately explicit bounded authorization; none is authorized here.

**Security and budget blockers:** no isolated sandbox or credential confinement tested; no production-safe execution; no paid-provider budget specified; no delivery/merge authority. No provider work turn or protected delivery effect was created. A Codex thread object was created in the no-turn experiment and its durable cleanup remains unresolved as recorded above. Never mark these PASS from unit tests or preflight probes.

**DeepSeek V4.1 Flash continuation:** check out this branch, read `AGENTS.md`, ADR-0018, `native/README.md`, this handoff, and `native-rust-progress.json`; run `git status`, `git rev-parse HEAD`, the pinned Rust gates, then inspect the two local provider schemas. Continue from task 1 above. Preserve explicit non-dispatch until Core authority, sandbox and budget are implemented. Keep all statements candidate-bound and update this handoff/progress file with every coherent commit. Do not resume the S4-D campaign, spend provider usage without a bounded grant, merge PR #16, or claim cutover based on this bootstrap.


## Current uncommitted slice handoff (2026-10-23; supersedes earlier provisional notes above)

The code tree described below is committed on this branch. Run `git log -1 --format=%H` for the exact candidate SHA; this text cannot embed its own commit. Evidence was rerun at that tree before commit and remains author-run until independent review. The dated evidence above for `f255f0c` remains historical and does not describe this slice.

The intended cutover layout remains to move the staged Rust backend from `native/` into `core/` after verified Go deletion. The migration matrix is [native-rust-migration-matrix.md](native-rust-migration-matrix.md); all rows remain incomplete and legacy deletion is not yet ready. The Rust implementation is still additive beside the active Go Core/Worker and old Office integration.

### Current implementation and evidence

- Core now persists immutable versioned ActionSpecs with digest validation, blocked Campaigns, and durable Run/Attempt identities. Worker registration uses a bootstrap secret and a per-Worker session credential; the registration-key verifier is HMAC-derived rather than stored in plaintext. Fixture-only lease/fencing flows exist. Public claim/dispatch remains disabled and returns no assignment.
- Workspace code allocates per-Attempt Git worktrees and validates CandidateRevision parentage. Rootless Podman fixture code has an ownership ledger and orphan reconciliation; the optional ignored Alpine fixture passes locally in 12.55 s (author run). Host pathname mounting was replaced by a validated named-volume snapshot, so path replacement fails closed. No provider has run inside a sandbox, so required isolation and credential confinement remain **BLOCKED**.
- Codex has a fixture-only typed thread/turn lifecycle with response/event correlation, permission denial, timeout interrupt requests, and terminal event validation. No live turn was started. The earlier Codex pre-turn thread remains unresolved: a new App Server process did not find it in its indexed state DB, which does not establish ownership or cleanup by the original process.
- OpenCode’s no-paid probe passed for the expanded health/OpenAPI/SSE/session inspection and cleanup path, including matching `session.created`, session status/messages, workspace binding, DELETE, and subsequent absence. No prompt or provider work was submitted. Both providers remain `LIVE_NOT_TESTED` for coding-work compatibility; no paid turns were authorized or run.
- A pure deterministic Quality evaluator exists with typed evidence and explicit outcomes. It is not connected to authenticated durable receipt ingestion or separate verifier/reviewer processes, so the Rust system has no operational independent Quality Gate.
- The native architecture checker is only a partial governance port. Rust CI is wired for all PR changes, including docs/config, but the Go workflows and prior governance checker remain active. The whole-tree hygiene audit reports **313 findings** and strict hygiene fails; no legacy deletion has occurred. The Office still targets the former backend.

Provisional local results for this uncommitted combined slice: **62 library tests, 4 architecture-check tests, 7 hygiene tests, and 2 binary tests passed**. Formatter, strict Clippy, binary build, dependency audit, and no-paid provider probes passed. The optional ignored Podman fixture passed locally in 12.55 s (author run). These results are author-run, not independent acceptance; rerun all applicable checks after commit. No paid provider turns were performed. No Core-authorized Worker dispatch, candidate-producing execution, independent verification/review journey, or Rust-backed Office journey has been exercised.

### Completion gates at this handoff

| Gate | Status | Current evidence / outstanding work |
|---|---|---|
| G1 Architecture | **INCOMPLETE** | Rust remains additive beside the legacy Go Core/Worker. |
| G2 Runtime | **INCOMPLETE** | Codex lifecycle is fixture-only; OpenCode is a no-paid probe only; no live coding turn. |
| G3 Cleanup | **INCOMPLETE** | Legacy Go/runtime tree remains active; hygiene audit reports 313 findings. |
| G4 Governance | **INCOMPLETE** | Native workflow and partial checker exist; Go workflows/checker remain; hygiene gate fails. |
| G5 Functional execution | **BLOCKED / INCOMPLETE** | Public dispatch is disabled; Worker does not execute a provider-backed Action or create a candidate. |
| G6 Quality | **INCOMPLETE** | Pure evaluator only; no authenticated durable evidence ingestion or independent verifier/reviewer processes. |
| G7 Office | **INCOMPLETE** | Office still targets the former backend; Rust journey and restart/reload are unverified. |
| G8 Security | **BLOCKED** | Podman fixture passes locally; same-UID path race unresolved; provider credential isolation and in-sandbox provider execution untested. |
| G9 Evidence | **NOT SATISFIED** | Current results are provisional uncommitted evidence; no paid provider work or full user journey. |
| G10 Review | **NOT SATISFIED** | Independent review of the full tracked tree and exact PR diff remains outstanding. |

### Next executable task and continuation

Resolve the Podman fixture timeout and same-UID pathname race, then have Luna independently review the ownership ledger, orphan reconciliation, provider lifecycle fixtures, HMAC registration binding, and the whole-tree migration matrix. Keep dispatch disabled while security and Core authorization remain incomplete. Next implement durable authenticated evidence ingestion and independent verifier/reviewer processes, followed by the provider-backed Worker lifecycle and Rust Office journey. Only after replacement behavior and acceptance are demonstrated should legacy files/workflows be removed. Commit coherent work and update this handoff with exact SHA-bound outcomes.

**DeepSeek V4.1 Flash continuation:** inspect `git status`, `git rev-parse HEAD`, PR #17 and the matrix. Treat this combined slice’s results as provisional until rerun at its committed SHA. Keep PR #17 draft and public dispatch disabled. Do not claim any G1–G10 gate PASS, coding-provider compatibility, sandbox certification, or completed replacement. No paid provider work is authorized by this handoff.

## Continuation: committed checkpoint and Rust standards (2026-10-23)

The combined slice above is committed at `f79f04f4e0840b795fbea989a029c621e912b16e`. Exact-tree author checks before commit: `cargo fmt --all -- --check` PASS; strict Clippy PASS; `cargo test --workspace --locked` PASS with 62 library + 4 architecture-check + 7 hygiene + 2 binary tests; the ignored rootless Podman Alpine fixture PASSED in 12.55 s when run explicitly; `cargo build --workspace --locked` PASS; `cargo doc --workspace --no-deps --locked` PASS; `cargo audit --file Cargo.lock` PASS for 192 dependencies (RustSec DB loaded 1,351 advisories). These remain author-run.

New normative and recovery artifacts:

- [Rust engineering standards](../engineering/rust-standards.md) — canonical Rust design, ownership, errors, async/process lifecycle, security, transactions, protocols, testing, dependencies, observability, performance and review rules.
- [Rust engineering skill](../../.agents/skills/rust-engineering/SKILL.md) with six focused references; frontmatter and reference links validated locally. The skill is discoverable under the repository `.agents/skills/` convention and referenced from `AGENTS.md`.
- [S4-D Rust recovery matrix](s4d-rust-recovery-matrix.md) — every material PR #16 capability is accounted for with source commit/paths, invariants, Rust destination, reusable oracle, adaptations, truthful status and deletion readiness.
- [Migration matrix](native-rust-migration-matrix.md) updated to link the S4-D inventory and reconcile the 313 hygiene findings and Podman fixture result.

S4-D Rust status remains `RUST_NOT_IMPLEMENTED` for semantic assessment, standing delegation, autonomous start/revocation, WorkGraph scheduling, deterministic integration, CAP-014 Rust evaluation and the Rust Office journey. Historical PR #16 evidence is provenance only; it does not certify the Rust branch. ADR-0017 remains Proposed and unratified; no live grant or provider spend is authorized.

**Next executable task (superseded below by the quality-evidence checkpoint):** continue the native execution pipeline with Core-owned authorization/dispatch. Keep public provider dispatch disabled until authority, isolation/credential confinement and budgets are implemented; no paid turn is authorized.

## Continuation checkpoint: Lead domain and quality evidence (2026-10-23)

**Candidate:** `b964cc6beff62bc37db4c6b850f7e8acbbe6606a`. **Predecessors:**
`01bc5272a58b890f40a4acebbb4fe5fbe98d2e57` (Rust standards, skill, S4-D
recovery matrix), `05828db394e842d05e93eedc09ef158493b24ef4` (typed Lead
domain) and `706399d422415033ffb586483e067821f3a88479` (candidate/quality
ingestion). See [native-rust-lead-quality-2026-10-23.md](../evidence/native-rust-lead-quality-2026-10-23.md)
for candidate-bound evidence.

- `native/src/lead.rs` now holds the typed advisory assessment, immutable
  delegation snapshot, proposal validation, plan hashing and bounded 1–8 unit
  WorkGraph validator, with 14 tests ported from the PR #16 oracles. It performs
  no I/O and grants no authority.
- Core now verifies a Worker candidate claim against real Git object identity
  and a frozen-base single parent, persists immutable candidates, requires
  separate verifier/reviewer bearer tokens, ingests immutable exact-candidate
  receipts, rejects implementer self-review and role collusion, and writes a
  deterministic gate once both receipts exist. Quality requests authenticate
  before JSON parsing.
- Public Campaign authorization and Attempt sandbox readiness still have no
  grant path. Worker claim still returns `dispatch_supported=false`. No provider
  session, paid turn, sandbox certification, independent verifier/reviewer
  process or Office journey exists.

Author-run gates at `b964cc6`: formatter, strict Clippy, tests (83 library, 4
architecture, 3 Core binary, 7 hygiene, 1 Worker; 1 ignored Podman fixture),
binaries and docs PASS. The ignored Podman fixture was not rerun in this slice;
its last local run passed at `f79f04f` without certifying isolation. Exact-SHA
CI and independent review remain outstanding. PR #17 stays draft.

**Next executable task (superseded below by the SQLx/automation checkpoint):** add the durable Lead delegation/request/proposal/freeze
store and API with autonomous policy closed, or connect Core-owned Campaign
authorization and Worker dispatch behind an explicit fail-closed isolation
gate. Do not enable provider execution until the isolation/credential and
budget boundaries are independently verified. No paid turn is authorized.

## Continuation checkpoint: SQLx, xtask, container-first, and E2E (2026-10-23)

**Candidate:** `22b816418f52258d1e99b066016790a92682701b`. See the
[E2E migration matrix](e2e-migration-matrix.md), [automation standards](../engineering/automation-standards.md),
[service placement policy](../architecture/service-placement-policy.md) and
[ADR-0019](../architecture/decisions/ADR-0019-container-first-cross-platform.md).

- Persistence is now SQLx 0.8.6 with SQLite only: `sqlx-core` + `sqlx-sqlite`
  directly, `sqlx-macros` for embedded migrations. `rusqlite` is removed. A
  bounded async pool, explicit write serialization, versioned
  `migrations/0001_initial.sql`, and immutable/trigger-enforced candidate,
  receipt and gate rows replace the bootstrap schema. P1–P10 tests cover fresh
  and repeated migrations, legacy adoption, idempotency, rollback injection,
  referential integrity, concurrent claims, restart, fencing and `Send + Sync`.
- `cargo xtask` is the canonical engineering entry point: `check`, `test`,
  `migrate`, `security`, `validate-architecture`, `governance` and
  `e2e recovery`. The recovery suite starts the real Core binary, creates
  durable Project/Action state, kills and restarts the process, and verifies
  the same frozen spec and events. Xtask never starts a paid provider session.
- The service-placement policy (LEVEL A–E), assessment template and skill are
  committed. Container-first ADR-0019, the Podman-compatible `.devcontainer`
  and Linux/Windows operational guides are committed as configuration/docs;
  the image was **not built**, no container was run and no Windows/Podman
  Machine path was exercised. The unified `factory` CLI is `NOT_IMPLEMENTED`.
- `cargo audit` passes for 214 dependencies. The E2E matrix records legacy
  coverage honestly: many component-level tests are migrated; process-level
  recovery is verified by the new test; provider live suites, scheduler/
  WorkGraph, independent roles, Office, recovery/orphan suites and legacy CI
  remain `NOT_IMPLEMENTED`/`BLOCKED`/`RETAINED`.

Author-run gates at `22b8164`: `cargo xtask check` PASS (formatter, strict
Clippy, 104 tests + 1 ignored, build, docs, audit); `cargo xtask migrate` PASS;
`cargo xtask validate-architecture` PASS; `cargo xtask e2e recovery` PASS.
Independent review, exact-SHA CI, container build, Windows and paid-provider
evidence remain outstanding. Public dispatch stays disabled.

**Next executable task (continued):** standing-delegation persistence is now
committed at `038eb9015e8bd72ad12136388d964672ab65f95f`. Author-run gates:
107 tests passed + 1 ignored, strict Clippy/formatter/build/docs/audit pass.
`cargo xtask e2e recovery`, `migrate` and `validate-architecture` pass. The new
`native/src/db/delegations.rs` stores sealed immutable grants, one-way
revocation and atomic cumulative request/session/attempt reservations with
three tests; it grants no API or autonomous activation.

**Next executable task (completed increment):** the development-container image
was built locally and the in-container Rust gates were exercised. Candidate
`038eb9015e8bd72ad12136388d964672ab65f95f` adds durable sealed standing
delegations and atomic cumulative reservations; `5ca9e6d40bf94a86430625b2ac3170ff0697dfb4`
records the container verification and Containerfile audit tool.

Container evidence (author-run, Linux only):

- `podman build --network=host -t factory-devcontainer-test -f .devcontainer/Containerfile .`
  succeeded, image `4e4e407f0c7a14bb5efb84827cbb5ee0d10f9b4a9d538dcd5783c781faeb27ae`.
  The default build network failed in this host environment with
  `pasta failed ... Failed to open() /dev/net/tun`; that is an environment
  blocker, not a Dockerfile pass.
- Inside the image with `--network=host` and a mounted worktree:
  `cargo xtask check` PASS (fmt, strict Clippy, 107 tests + 1 ignored, build,
  docs, audit), `cargo xtask migrate` PASS, and `cargo xtask e2e recovery` PASS.
- Not verified: default rootless container networking, Office build inside the
  container, application runtime image, Quadlet, Windows 11/Podman Machine, the
  unified `factory` CLI, and isolation/credential confinement. Rootless Podman
  remains a candidate runtime, not certified isolation.

**Next executable task (completed increment):** durable Lead request,
assessment, proposal and frozen-plan persistence is committed at
`1a62d8242858f76a20226710b7a2734d79dad153`. It includes idempotent request
creation with assessment-session reservation, explicit state transitions,
strict assessment reassembly, validated proposal storage and immutable plan
freezing. Autonomy, model calls, API exposure and Campaign start remain
disabled. Author-run tests: 110 passed + 1 ignored; fmt, strict Clippy and the
full suite pass. Migration CI must be re-run for the third migration in the
next checkpoint.

**Next executable task (completed increment):** `f707eb533966903ff9f085a1ec5b3ea6ed457da8`
adds authenticated operator APIs for non-autonomous delegation creation, usage
reads, revocation and Lead request creation/projection. Autonomous delegation
creation is rejected while ADR-0017 is unratified. 111 tests pass + 1 ignored;
fmt and strict Clippy pass. Migration/architecture checks are recorded from the
immediately preceding candidate; re-run `cargo xtask check` at the final
candidate before stronger claims.

**Next executable task (completed increments):** Campaign authorization,
Worker dispatch, independent verification and Office unification are now
committed through `35687a3603ff037a852101b4c15d90b6057f2b45`.

- R0/FIXTURE Campaign authorization, run/attempt APIs, lease assignment,
  deterministic Worker worktree execution, candidate submission and failure
  reporting are implemented. Provider/ISOLATED modes remain fail-closed.
- ActionSpecs now carry frozen allowed paths and argv checks. Core enforces raw
  changed paths and rejects symlink/gitlink/out-of-scope changes; the
  independent verifier rechecks identity/scope, runs checks with a cleared
  environment, bounds output and submits the receipt.
- `native/tests/e2e_fixture_dispatch.rs` / `cargo xtask e2e dispatch` exercises
  authorization → run/attempt → Worker → candidate → verifier → reviewer
  receipt → ACCEPTED gate → Core restart. Author-run only.
- Office is a single Rust-backed React app for Dashboard, Projects, Actions,
  Campaigns and Runs. The legacy Paseo client/views and the temporary duplicate
  Native page were removed. Browser E2E still targets the former API and is not
  migrated.
- `factory doctor` and `factory status` are implemented in a host-native Rust
  CLI. `start/stop/restart/logs/update/dev/install` remain `NOT_IMPLEMENTED`.

Author-run at `35687a3`: 131 tests + 1 ignored, `cargo xtask check`, `migrate`,
`e2e recovery`, `e2e dispatch`, Office typecheck/lint/build and audit pass. The
independent review found blocking issues in the first dispatch slice; the
authorization/path/Git/credential fixes are in `13d4a5e`/`35687a3`, but
same-UID verification, orphan cleanup, absolute lease limits and project-scoped
Worker capability remain open. No provider or Windows run is claimed.

**Next executable task (completed increments):** through
`7b30918a2987f1c17fd94a6b764d9303e9df9b0c` the branch adds negative dispatch
coverage, a single Rust-backed Office, `factory doctor/status`, and the
structured-output/dependency doctrines.

- Typed model outputs: `schemars` generates the Lead assessment schema;
  `jsonschema` validates it; a conservative provider keyword gate fails closed;
  the schema digest is stored with each assessment (migration 0005). Model
  output remains advisory.
- Library decisions are recorded in
  [dependency-evaluation-2026-10-23.md](../engineering/dependency-evaluation-2026-10-23.md):
  `schemars`/`jsonschema` adopted; hardened Git CLI retained; Podman CLI
  retained pending a supervisor endpoint spike; `octocrab` adopt-later;
  `cap-std`/`rustix` evaluated with the same-UID race still blocking;
  `cargo-deny` evaluate before adoption.
- Negative tests reject R1/ISOLATED authorization, out-of-scope candidates and
  symlink/gitlink changes.
- Office is one React app backed by the Rust client for Dashboard, Projects,
  Actions, Campaigns and Runs. Legacy Paseo views/client were removed. Browser
  E2E still targets the former API and is not migrated.
- `factory doctor`/`status` are real host-native commands; lifecycle commands
  remain `NOT_IMPLEMENTED`.

Author-run at `7b30918`: 136 tests + 1 ignored, Office typecheck/lint/build,
strict Clippy, five migrations and audit pass. `cargo xtask check` must be rerun
at this exact SHA (last recorded at `35687a3`). Same-UID verifier isolation,
orphan cleanup, absolute lease limits, project-scoped Worker capability,
provider lifecycle, runtime image/Quadlet, `factory start`, browser E2E and
Windows verification remain open. No paid provider or Windows run is claimed.

**Next executable task (completed increments):** through
`5edfc46c5a60f39197ba7bb232948b8a81b9500c` the branch now has a sandbox
supervisor candidate, a Core OCI image, functional `factory start/stop/logs`
and negative dispatch coverage.

- `factory-supervisor` is an authenticated loopback service that validates a
  direct-child workspace, exact image allowlist, bounded argv/timeout and
  delegates to hardened rootless Podman; it returns fixture results and can
  reconcile orphans. It is **not** isolation-certified; same-UID races,
  credential confinement and provider-in-sandbox remain blockers.
- Core accepts a non-loopback bind only with `FACTORY_CONTAINER_MODE=1` and an
  IPv4 unspecified address. The host CLI publishes only `127.0.0.1`.
- `deploy/Containerfile.core` builds a minimal non-root runtime image
  (`edf267bbc475` locally). `/health` was verified in an isolated `--network=none`
  container joined by an Alpine network namespace.
- `factory start/stop/logs/status` are implemented with rootless checks,
  private generated tokens, read-only root, dropped capabilities, no host
  network and no Podman socket. Full `factory start` is **blocked on this host**
  because rootless Podman networking requires `/dev/net/tun`
  (`pasta failed ... No such device`).
- Negative dispatch E2E covers out-of-scope candidates, failing frozen checks
  (`REPAIR_REQUIRED`), unauthorized runs and stale lease fencing.

Author-run at `5edfc46`: 164 tests + 1 ignored, `cargo xtask check`, five
migrations, `e2e dispatch`, `e2e negatives`, Office typecheck/lint/build, Core
image build, container `/health` and audit pass. `cargo xtask e2e office` now
runs a real Chromium journey through the unified Rust Office and reaches an
`ACCEPTED` gate after reload (`230ad0b`). The branch was published to
PR #17; exact-head CI is pending. The legacy governance gate initially failed
on stale Office capability references and YAML quoting; both were repaired in
`5d9af40` without restoring removed Paseo views. At code head `99d4b57`, exact-head **native Rust**, **governance/architecture-impact** and
**office-browser** checks all passed. The new `office-browser` job passed at
`6e2f4aa` and again at `99d4b57`. Legacy Go/Paseo browser jobs fail on removed Office
selectors/views (old `FACTORY_API_TOKEN`, Dashboard/Agents/Projects/Campaigns
legacy journeys) while their non-browser Go assertions pass. The Office now
also exposes a Rust-backed Workers view (registered credentials only, not
fabricated liveness) at `b7131b3`. Those legacy browser jobs are not
Rust-era evidence. Exact-head CI for `230ad0b`, independent acceptance,
provider lifecycle, recovery/multi-project Office, same-UID isolation, Windows
and portable container networking remain open.

**Next executable task (current):** exact-head native Rust, governance and
`office-browser` CI now pass at code head `99d4b57`/published `e74899b`. The
unified Rust Office browser journey and Workers view are verified. The next
task is the bounded one-turn read-only provider smoke recorded in
[provider-live-authorization-2026-10-24.md](provider-live-authorization-2026-10-24.md):
Codex `gpt-6-luna` and OpenCode `opencode/deepseek-v4.1-flash`, one turn each,
1024-token ceiling, no tools/write, no retries. Live results are not claimed
until the tool runs and evidence is recorded.


## Live provider smoke checkpoint (2026-10-24)

At candidate `5cd0619` the authorized read-only smoke produced:

- **Codex:** `gpt-6-luna` on CLI/App Server `0.156.1`, one turn, 29 output
  tokens, `outputSchema` + strict validation PASS, thread archive response.
  Evidence: [provider-live-smoke-2026-10-24-codex.json](../evidence/provider-live-smoke-2026-10-24-codex.json).
  Classification: `LIVE_STRUCTURED_OUTPUT_VERIFIED`. This is not sandbox,
  coding-work or campaign certification.
- **OpenCode:** server `1.18.32`, `opencode/deepseek-v4.1-flash`, one prompt
  attempt returned HTTP 401 `Invalid credential` before model output. Zero
  tokens/cost reported; no retry; session deleted and verified absent.
  Evidence: [provider-live-smoke-2026-10-24-opencode.json](../evidence/provider-live-smoke-2026-10-24-opencode.json).
  Classification: `LIVE_PROVIDER_CREDENTIAL_BLOCKED`. The operator must renew
  the OpenCode credential before another bounded authorization.

Do not silently substitute another provider/model or retry within this
envelope. Continue deterministic provider lifecycle fixtures, sandbox/credential
confinement and recovery/Office parity.


## Host runtime and OpenCode Go routing checkpoint (2026-10-24)

At `2dd4757` the repaired CachyOS host (kernel `7.3.0-3-cachyos`, `tun`
loaded, rootless Podman 6.1.4, pasta working) passed real `factory start`:

- `factory doctor`/`status` PASS; loopback-only `127.0.0.1:18766`; Core health;
  authenticated API 200/unauthenticated 401; the built Rust Office served from
  the image and unlocked in real Chromium; Worker registration persisted across
  stop/start; repeated start was idempotent; stop retained the data volume;
  token env file 0600; read-only root, no host network, no Podman socket.
- See [native-rust-host-runtime-2026-10-24.md](../evidence/native-rust-host-runtime-2026-10-24.md).
  No isolation or Windows certification is claimed.

**OpenCode Go:** `opencode models opencode-go` confirms
`opencode-go/deepseek-v4.1-flash`. The earlier smoke used the distinct ordinary
`opencode` provider and failed 401. The native route parser now preserves
`opencode-go` exactly, rejects unknown/malformed providers and classifies
auth/protocol failures; deterministic tests pass. No paid retry was made under
the old envelope. A fresh bounded authorization is required for a real
OpenCode Go turn.

**Next executable task:** fresh bounded OpenCode Go authorization, deterministic
provider lifecycle fixtures, then Supervisor/credential confinement and
recovery/multi-project Office parity. Keep provider/ISOLATED execution
fail-closed.


## OpenCode Go live verification (2026-10-24)

`LIVE_STRUCTURED_OUTPUT_VERIFIED` at candidate
`0601a6d8af59314b62ed8e9a92d8c7958f8bc7f3`.

- Provider `opencode-go`, OpenCode `1.18.32`, model `deepseek-v4.1-flash`.
- One request `msg_16becba690022DVSPG0M81bBkL`, session
  `ses_e941345e9ffe32yqIb9BacUrZ7`.
- Requested output ceiling 256; output 67 tokens. Total usage 7,372 (input
  5,513, cached 1,792); cost 0.000872526. The output ceiling is not a total
  usage cap.
- Native JSON Schema structured output accepted and strictly validated.
- Session deleted; GET returned 404; no retry.
- Evidence: [provider-live-smoke-2026-10-24-opencode-go.json](../evidence/provider-live-smoke-2026-10-24-opencode-go.json).

Both Codex and OpenCode Go now have bounded read-only structured-output
evidence. Coding lifecycle, sandbox isolation and independent acceptance remain
open. Next: deployed Project/Attempt workspace access and Supervisor-mediated
execution, still fail-closed for provider/ISOLATED coding.

## Closure checkpoint 2026-10-24 — `be22102`

The current status authority is [native-rust-closure.md](native-rust-closure.md).
This checkpoint adds:

- Supervisor-owned named-volume execution, durable workspace identity,
  path-replacement rejection and reconciliation.
- Deployed `factory start/stop/status` management of Core, Supervisor and
  Worker with explicit read-only project roots.
- Sandboxed verifier and real deployed fixture Campaign to `ACCEPTED`.
- Project configuration revisions/hash binding, exact provider route allowlist,
  delegation release and revocation containment.
- Frozen CAP-014 corpus, deterministic baseline, blinded scoring and CLI.
- Authoritative capability decisions; predecessor assessment removed with a
  preservation gate and empty strict independence audit.
- Deterministic Lead fixture lifecycle and default-off single-unit autonomous
  start, including a process E2E to `ACCEPTED`.
- Sequential WorkGraph tasks, per-Attempt specs, candidate-driven promotion,
  Lead Office view and Campaign detail projection.

Remaining: parallel/fan-in integration, provider coding lifecycle, Go/Paseo
removal and CI migration, strict hygiene, independent review and final merge
authorization. Provider and ISOLATED execution remain fail-closed.
