# Native Rust migration closure ledger

**Status:** current migration status authority. Updated 2026-10-26.
This ledger separates implemented behavior from verified evidence. It does not
replace candidate-bound test output, independent review or protected approval.

## Identity

| Item | Value |
|---|---|
| Authoritative main baseline | `003d92ca0dea9c8d397f0a37a04cf90bd479c99f` (PR #27 merged) |
| PR #16 behavioral reference | `19cb4951f7a3d42a1ca27b1800cf49401c81ecc6` (closed as superseded) |
| Product | Software Factory, independent Rust implementation |
| Legacy implementation | retired from the tracked tree; retained only in Git history and archived provenance |
| Historical combined candidate | `ea2d92724d57d4f96838d84216f39db04b7c5511` (author-run gates and managed live acceptance); exact-head PR checks are authoritative |
| Container-native implementation | `30cb0db7efbd1b9d4ed8123b4be3d3f8e2e34e9e` (author-run deterministic Compose evidence; PR #28 not merged) |
| Container-native hardening candidate | `d9de876f5533827a5a44b07f00c676ac24d7516c` (post-review quality/provider/policy fixes; deterministic Compose E2E PASS; live inference NOT_TESTED) |
| Merge state | PRs #17–#27 merged. PR #28 (container-native Compose iteration) remains open. Its hardening candidate has exact-head local evidence but awaits an independent re-review, a separately authorized paid UAT and Product Owner approval; it is not merged. |

PR #16 is a product-concept and provenance reference only. Its implementation is
not merged, is not built or tested by this repository, and is not authoritative.
Historical evidence and legacy-era contracts are archived under
[docs/evidence/history](../evidence/history/README.md).

## Scope classes

- `REQUIRED_FOR_MIGRATION` — approved product behavior and replacement operation.
- `REQUIRED_FOR_SECURITY` — mandatory fail-closed boundary.
- `REQUIRED_FOR_PR16_PARITY` — S4-D A1–A12 product concepts.
- `REQUIRED_FOR_VERIFICATION` — evidence needed before merge.
- `FUTURE_FEATURE` — retained in the roadmap, not implemented in this closure.
- `HISTORICAL_ONLY` — provenance; never current runtime authority.

See [capability decisions](../product/capability-decisions.md) for the complete
portfolio; this ledger is the current status authority.

## S4-D A1–A12 disposition

| ID | Capability | Status | Rust state | Blocker / next evidence |
|---|---|---|---|---|
| A1 | Semantic intake and typed assessment | `IMPLEMENTED_VERIFIED` | Strict typed assessment/storage; Core-owned model gateway with strict schema and budget; a real model Lead produced the frozen plan for the managed `ACCEPTED` journey | CAP-014 live semantic quality and human acceptance remain open |
| A2 | Proposal validation and frozen plan | `PARTIAL` | Validation, config binding, freeze revalidation, per-unit specs, parallel/fan-in plans, Campaign/Run/Attempt creation and Office plan projection | Crash-injection and full restart scheduler coverage remain |
| A3 | Standing delegation | `PARTIAL` | Sealed grants, atomic reservation, exact route allowlist, revocation fencing of leases/sessions, provider authorization and shared model-call budgets; deterministic revocation test added | Ratified autonomous issuer/risk policy and paid live revocation remain open |
| A4 | Authorized autonomous Campaign start | `PARTIAL` | Default-off R0 policy; single- and multi-unit plans create Campaign/Run/Attempts atomically; explicit MODEL Lead start was exercised with a real provider and reached `ACCEPTED` | Browser multi-unit evidence and owner ratification remain |
| A5 | Immediate revocation containment | `PARTIAL` | Revocation blocks Lead/Campaign/Run/Attempt work, marks pending tasks/integrations failed, fences leases, marks provider sessions `CANCEL_REQUESTED` and blocks replacement dispatch; new deterministic test covers an active provider session and lease | Paid in-flight model-call revocation and external-session termination remain untested |
| A6 | Bounded WorkGraph | `IMPLEMENTED_UNVERIFIED` | Validation, parallel roots, fan-in and final integration nodes, per-unit specs; parallel/fan-in E2E reach ACCEPTED; failed dependency, restart, integration failure and cancellation process tests pass | Browser graph projection and live-provider multi-unit execution remain |
| A7 | Dependency-aware execution/recovery | `PARTIAL` | Parallel roots, fan-in promotion, integrated candidate recording; failed dependency and integration reach terminal FAILED; frozen fan-in resumes after restart; cancellation fences runs/attempts/tasks/integrations/leases | In-flight integration cancellation is DB-tested but not process-tested |
| A8 | Deterministic Git integration | `IMPLEMENTED_UNVERIFIED` | Supervisor integration, integrated lineage, verifier support, parallel final/fan-in E2E to ACCEPTED; integration failure retains exact root lineage | Adversarial Git conflict path remains unreachable in the fixture |
| A9 | Atomic Lead/Campaign start | `PARTIAL` | Multi-unit Campaign/Run/Attempt creation in one transaction; parallel/fan-in E2E reach ACCEPTED | Crash injection and restart scheduler coverage remain |
| A10 | CAP-014 semantic evaluation | `IMPLEMENTED_UNVERIFIED` | Frozen corpus, baseline, blinded scoring, thresholds; `execute` collects raw outputs through Core with budgeted reservations and durable FAILED evidence on exhaustion | Independent blinded score and authorized live model-quality run remain absent; no CAP-014 certification |
| A11 | Office Lead/WorkGraph workflows | `PARTIAL` | Lead assessment/proposal/plan, WorkGraph, candidates, receipts, gate, model-call budgets, telemetry, cancellation and runtime projections are browser-tested; the managed live Office showed the accepted candidate and gate | Browser multi-unit graph, recovery/cleanup interaction and unexplained 404 console entries remain |
| A12 | Bounded live certification tooling | `PARTIAL` | Default-closed read-only provider smoke; durable provider intent/terminal/usage ledger and exact candidate path; used for the authorized managed OpenCode external-project and self-development journeys | Governed general S4-D certification harness and Codex native ChatGPT route remain open |

## Merge-readiness gates

| Gate | Status | Evidence / blocker |
|---|---|---|
| M1 Architecture | `PARTIAL` | One Rust Core/Worker/Supervisor/Office path; no legacy tracked implementation. OpenCode and Codex adapters are implemented; OpenCode has managed live acceptance. Native ChatGPT Codex auth and ISOLATED execution remain blocked. |
| M2 PR #16 parity | `PARTIAL` | A1–A12 concepts substantially implemented; crash/browser multi-unit and paid failure-injection gaps remain |
| M3 Repository hygiene | `PASS` | Zero tracked Go files; strict hygiene and architecture consistency pass on the current branch; all 44 CAP IDs preserved |
| M4 Runtime | `PASS (author-run)` | Managed container build, `factory start`/`doctor`/`status`/`stop`, host Supervisor/Worker, Office HTTP, real OpenCode Lead/Implementer, rootless-Podman verifier, semantic Reviewer, `ACCEPTED`, restart persistence and clean stop. Evidence: [managed live UAT](../evidence/managed-live-uat-2026-10-26.md). |
| M5 Functional execution | `PASS (author-run)` | R0/FIXTURE, model-backed Lead, source-aware OpenCode tool loop and Codex App Server deterministic adapter; managed external-project candidate reached `ACCEPTED`. Live Codex native ChatGPT auth remains blocked. |
| M6 Security | `BLOCKED` | Provider credentials remain in trusted Core/Worker; bubblewrap and egress hardening are implemented. Native ChatGPT credential extraction/pass-through, running model-call revocation and human certification remain open. ISOLATED remains blocked. |
| M7 Quality | `PASS (author-run)` | Rootless-Podman verifier, mandatory deterministic Reviewer pre-check, live semantic Reviewer with measured provenance and deterministic `ACCEPTED` gate. No human certification. |
| M8 Office | `PASS (author-run)` | Rust Office served from the managed Core and browser-observed with candidates, receipts, gate and runtime accounting. Two unexplained 404 console entries and non-terminal Campaign/Lead projection labels remain. |
| M9 Testing | `PARTIAL` | Full workspace tests, strict Clippy/format, real-Chromium Office journey, CAP-014, observability, WorkGraph, cancellation/revocation/recovery/lease and provider tests pass locally; exact-head PR checks remain authoritative. |
| M10 Governance | `PARTIAL` | Rust architecture checker, roadmap/registry checks and strict hygiene pass; conditional Podman/provider-compat lanes remain manual. |
| M11 Independent review | `PARTIAL` | Earlier independent same-model security/architecture/release reviews were performed; independent review of the managed acceptance branch and human security certification remain pending. |
| M12 Delivery authorization | `PENDING` | Product Owner merge decision and exact-head CI remain. |
| M13 Product independence | `PARTIAL` | All 44 CAP IDs and opportunities preserved; registry/roadmap updated; live semantic quality for CAP-014 and human certification remain. |

## Verified execution path

At the author checkpoint:

- rootless Podman named-volume execution produced a candidate;
- Supervisor state survived reconciliation and rejected path replacement;
- verifier executed frozen checks in a separate named-volume sandbox;
- independent deterministic reviewer submitted a candidate-bound receipt;
- deterministic gate reached `ACCEPTED`;
- containerized Core served the Rust Office;
- `factory start/stop/status` managed Core, Supervisor and Worker;
- real Chromium Office journey passed, including Workers and Campaign reload.

Candidate-bound evidence lives under [docs/evidence](../evidence). None of it is
provider coding, OS isolation certification, independent human acceptance or
deployment authorization.

## Managed live acceptance (2026-10-26)

At implementation commit `ea2d92724d57d4f96838d84216f39db04b7c5511`, the normal
`factory start` path launched the Core container, host Supervisor and host
Worker with private Core-model and Worker-provider env files on loopback. A
disposable external Rust repository then completed the full journey with real
`opencode-go/deepseek-v4.1-flash` inference:

- model-backed Lead assessment and frozen one-unit WorkGraph;
- authorized R0 `TRUSTED_LOCAL_PROVIDER` Campaign;
- real OpenCode Implementer session writing only `src/` and `tests/`;
- candidate `177060b3-5271-4b0f-a20f-2c5649426484`, commit
  `067c8569e651e0deb9b3edf4bfd7430b99a243b1`;
- role-independent executed verifier in a rootless-Podman named volume
  (backend author-attested; the receipt does not persist it);
- semantic MODEL Reviewer with measured provenance;
- deterministic gate `ACCEPTED`;
- Office presentation, restart persistence and clean stop.

The committed Core export records 86,412 tokens and 14,774 micros at
configured rates; adding a reconstructed 5,457-token stale-image entry Lead
call from the first disposable install gives 91,869 / 16,516. The combined
figure is a lower-bound accounting total, not a complete exported ledger.
Follow-up commit `6ae3241` makes the first-write instruction conditional for
run_command-only authorizations and strengthens deterministic tests; it does
not change the write_file prompt text used by this journey. A failed provider session, a stale-image
entry Lead call, an immutable verifier-harness FAIL candidate and one reserved
unknown turn are recorded truthfully rather than hidden. Evidence:
[managed live UAT](../evidence/managed-live-uat-2026-10-26.md).

Codex managed preflight with the real native App Server binary passed without
inference. Native ChatGPT-authenticated codex live inference is blocked by the
credential boundary; the only supported Codex authentication mode is an
operator API key at `https://api.openai.com/v1`, and live Codex inference is
not tested.

## Controlled self-development (2026-10-26)

In a separate managed stack and a disposable clone at `ea2d927`, one bounded
Implementer session generated focused `one_line` unit tests in
`factory-cli/src/text.rs` only. Candidate
`b00d3aaeb5f1e4e7a51f132860357a6d5a68f358` passed an independent
rootless-Podman verifier (3 tests), semantic review and the `ACCEPTED` gate.
The candidate was not merged and the disposable stack was removed. Evidence:
[managed self-development](../evidence/managed-self-development-2026-10-26.md).

## Trusted-local provider fixture path

At `81978fd146f79aa9335791e0bd9111f738701b56`, a deterministic fake OpenCode server runs under bubblewrap and proves the source-aware, tool-mediated path: authorized Action → Campaign → Worker claim → durable provider intent →
ACTIVE session binding → bounded source context → provider file plan plus a
frozen-check command → Supervisor mediated write/run/finalize → immutable
CandidateRevision → sandboxed verifier → independent reviewer → `ACCEPTED` gate.
Core enforces an exact provider/model/mode route, active delegation, allowed
paths, session/turn/token/cost limits, a deadline and an explicit
`write_file`/`run_command` subset. The provider may use up to four turns; each
tool request, permission decision and result is durable. Commands must exactly
match a frozen ActionSpec check. The Worker holds the provider credential; the Supervisor and
sandbox never receive it. Sensitive source names are excluded from context.
Unauthorized tools (including an attempted `read_file`), out-of-scope paths,
duplicate call IDs, non-frozen commands and unmeasured usage are rejected;
credential-shaped source names are excluded from context. Durable
INTENT/ACTIVE/terminal/cleanup states support a recovery Worker that deletes a
confirmed orphan after Core restart; unresolved sessions block replacement
dispatch; cancellation/revocation mark sessions `CANCEL_REQUESTED`. The
tool-free file-only mode remains available. The provider process has an empty
HOME, no host repository or `/run/user` socket, no external network, and no
long-lived provider credential; a loopback bridge requires a per-run Basic-auth
capability. Supervisor allocation requires a Core-signed Attempt grant; workspace
operations require a per-Attempt capability and reject the broad admin bearer.
Mutation authority is now a short-lived Core-signed operation grant bound to
the live lease, generation, workspace, operation and frozen scope. Provider
egress is confined to a pinned OpenAI-compatible broker with a per-run token and
injected credential. Sandbox CPU, memory, file-size, descriptor, lifetime,
output and session-size limits are enforced; confined recovery verifies process
absence or terminates a confirmed survivor. Evidence:
[native-rust-provider-hardening-2026-10-25.json](../evidence/native-rust-provider-hardening-2026-10-25.json),
[native-rust-provider-boundary-2026-10-24.json](../evidence/native-rust-provider-boundary-2026-10-24.json) and
[native-rust-provider-tools-2026-10-24.json](../evidence/native-rust-provider-tools-2026-10-24.json).
This is an author-run candidate boundary, not independent isolation
certification.

## Final campaign integration (2026-10-25)

The following capabilities were integrated and author-run at
`067d26b9d229113b17c934dde0f5290af7008403`. They are not human or ISOLATED
certification.

- **Core-owned model calls:** durable reservations, per-scope and global token/cost
  caps, output rewriting, idempotent settlement, ambiguity fencing, operator
  resolution and Core-held credentials. Lead, semantic review and CAP-014 share
  this path. Evidence: [native-rust-final-closure-2026-10-25.json](../evidence/native-rust-final-closure-2026-10-25.json).
- **Intelligent Lead:** strict model assessment and frozen WorkGraph proposal
  reach an authorized Campaign and ACCEPTED gate in a deterministic mock-model
  process E2E. Live model quality is not claimed.
- **Semantic Reviewer:** strict closed schema, bounded review bundle, durable
  findings/provenance and request-digest binding. A model `approve` is
  corroborating only; a deterministic pre-review must also approve before the
  gate can receive a `PASS` reviewer receipt. Malformed output yields durable
  `FAIL`/`REPAIR_REQUIRED`, never `ACCEPTED`.
- **CAP-014:** `factory-s4d-evaluate execute` collects 11 raw fixture outputs
  through Core with 11 settled Evaluation reservations; a denied budget writes
  `FAILED` evidence and exits non-zero. Independent blinded scoring is still
  required.
- **Observability:** bounded sanitized OTLP/HTTP log export with a durable cursor,
  drop/backpressure accounting and process-level collector-outage isolation.
  Traces and metrics completeness are not claimed.
- **Browser capability:** a standalone Worker-side managed test server and real
  Chromium check. UAT-5 passed with Chromium 156.0.8078.4, zero console errors,
  zero failed requests, confirmed port closure and process-group absence.
  Frozen Project-config and Worker/Core wiring remain incomplete.
- **Full local lifecycle:** UAT-7 built the Core/Office image, ran `factory
  doctor`, started containerized Core plus host Supervisor/Worker, created a
  Project/Action/Campaign/Attempt, produced a candidate, ran the verifier and
  reviewer to ACCEPTED, restarted the installation, verified Project/Campaign/
  gate persistence, and stopped cleanly. The disposable data volume was removed.
- **Container packaging fix:** `deploy/Containerfile.core` now includes
  `native/fixtures`, required by the compile-time CAP-014 corpus.
- **CLI fix:** the host Supervisor now receives `FACTORY_CORE_URL` from
  `factory start`; without it host execution could not start.

Not claimed: live paid model quality, human security certification, SEC-01
ISOLATED certification, Windows, deployment, or exact external spend.

## Lead live-start authorization (2026-10-26, uncommitted branch candidate)

This branch adds an explicit, operator-supplied authorization path for starting
a frozen Lead plan whose assessment mechanism is `MODEL`. It is author-run
deterministic verification only; no paid model inference, live provider turn or
independent acceptance is claimed.

- `POST /lead/requests/{id}/start` keeps the bodyless deterministic `FIXTURE`
  start. A `MODEL` plan requires the same strict shape as
  `POST /campaigns/{id}/authorize` and must use `TRUSTED_LOCAL_PROVIDER`.
- Core validates the exact frozen proposal route, active delegation, R0 policy,
  ActionSpec/frozen-plan paths and budgets. A missing, invalid, wider or
  mismatched payload is rejected with a stable 4xx; the whole start
  transaction rolls back, so no Campaign, authorization, Run or Attempt row is
  committed.
- The Campaign, authorization, first Run and Attempt are inserted in one
  transaction using the shared Campaign-authorization validation and insertion
  code. Lead start does not reserve a second request or session envelope: the
  Lead request and freeze already own those, and start reserves only the
  authorized attempts.
- `GET /lead/requests/{id}` now includes Lead model-call provenance
  (provider/model/reservation/state/measured usage, without prompts or
  responses). The Office Lead view shows the mechanism, Start controls and the
  explicit MODEL authorization form; Campaign detail shows execution mode,
  route, Lead FAILED/BLOCKED state, Reviewer model provenance including
  `model_request_sha256`, and links from Candidate/Run Campaign cells.
- Deterministic verification: Lead DB tests (including MODEL-without-payload,
  no-row rejection, exact authorization, FIXTURE and no-double-reservation),
  the mock-model Lead-start process E2E with an explicit provider payload,
  parallel/fan-in WorkGraph fixture E2E and the real-Chromium Office journey.
  Office typecheck and Biome checks pass.
- At that checkpoint, live provider execution from this start route, real paid
  model quality, Codex coding, ISOLATED execution, human security review and
  final merge were not claimed. The Office journey used the deterministic
  fixture assessor. A later managed live run exercised the MODEL start route
  with a real OpenCode provider and reached `ACCEPTED`; see the managed live
  acceptance section above.

## Legacy retirement

The complete former tracked implementation was removed at `0b94e09ecd9b0e5067295d9a21c43799028c6015` together
with its module files, Go-only lint configuration, legacy entrypoints, test
harnesses, scripts and workflows. The
tracked Go inventory is **0**. Historical contracts, checkpoints, security
findings and the former deletion map are archived under
[docs/evidence/history](../evidence/history/README.md); Git history preserves
the former source. Evidence: [native-rust-go-retirement-2026-10-24.json](../evidence/native-rust-go-retirement-2026-10-24.json) and [native-rust-retirement-ci-2026-10-24.json](../evidence/native-rust-retirement-ci-2026-10-24.json) (`3f229f37ebd856ef3fba37b7e6456a2406ad3284`).

The deletion does not change any capability from `PARTIAL` or `BLOCKED` to
`VERIFIED`. Functional gaps remain as recorded above.

Strict tracked-tree independence audit:

```sh
git grep -n -i -E 'A[E]H|agentic[-_ ]engineering[-_ ]harness' -- .
git ls-files | grep -Ei 'A[E]H|agentic[-_ ]engineering[-_ ]harness'
git ls-files '*.go'
```

All three must be empty. The strict repository hygiene binary is the required
CI gate.

## Live UAT addendum (2026-10-25)

A separate F1-S4-LIVE-UAT campaign exercised the merged baseline plus a repair
candidate on Linux TRUSTED_LOCAL with real `opencode-go/deepseek-v4.1-flash`
inference through OpenCode `1.18.32`. A genuine model-generated Lead assessment,
a frozen one-unit WorkGraph, a real provider Implementer session, a Git
candidate, an independent sandboxed verifier PASS, a real MODEL semantic
Reviewer PASS and an `ACCEPTED` Quality Gate were recorded for candidate
`8569839d-6c06-4999-af42-9a8525035c2d` (commit
`9a5e892df83091529b87ed3127edddcc4bafe318`). The model changed only
`src/pricing.rs` and `tests/discounts.rs`; the public API was unchanged. The
Implementer did not emit a final prose explanation; the Worker preserved its
applied writes for independent verification.

The repair candidate fixes live OpenCode/egress wiring and session headers,
sandbox path selection, SSE streaming limits, lease heartbeat, empty-final and
post-write error finalization, check-command/finalize resilience, and the
Reviewer output cap. It also fixes the unsafe process-name cleanup in the
disposable UAT script. The full workspace format, strict Clippy and test suite
pass on the repair candidate.

This historical addendum described a branch state before PR #19 and the
managed-live-start work merged. Those limitations were subsequently removed:
`factory start` now configures strict Core-model and Worker-provider files, the
OpenCode live route completed a new managed acceptance, and the Codex App
Server adapter merged with deterministic real-executable coverage. Paid
cancellation/revocation injection and native ChatGPT Codex inference were not
exercised. ISOLATED, Windows, deployment, human security review and final merge
remain uncertified. See the [F1-S4-LIVE-UAT report](../evidence/f1-s4-live-uat-2026-10-25.md)
and the newer [managed live UAT](../evidence/managed-live-uat-2026-10-26.md).

## Product Owner decisions required

1. Ratify or amend standing-delegation issuer/risk/autonomy policy (A3/A4).
2. Authorize any final paid provider certificate separately; none is authorized
   by this ledger.
3. Decide release scope for Windows/Podman Machine if unverified.
4. Approve final merge after exact-head CI and all mandatory gates.
5. Accept that the developer-preview release remains TRUSTED_LOCAL and does not
   certify ISOLATED execution or human security review.

## Update rule

Every status change must cite an exact candidate SHA and executable evidence.
Documentation, compilation, a probe or a fixture is not `VERIFIED` behavior.
Historical evidence is never rewritten to look like Rust certification.


## Codex App Server adapter addendum (2026-10-26, author-run)

The merged native Codex `0.156.1` App Server coding path supports the frozen
route `codex/gpt-6-luna`. It drives the real executable over stdio in
the existing bwrap boundary, declares only `write_file`/`run_command` dynamic
tools, replaces the upstream tool list with that exact allowlist, rejects
non-allowlisted server requests and item types, binds every actual
`/v1/responses` body to a Core turn reservation, and records a sandbox
process-absence receipt. Up to eight upstream requests are allowed per turn;
the input/output ceiling is divided across that sequence. Native Codex
built-in surfaces are removed at the egress boundary and rejected fail-closed.

Author-run deterministic evidence:

- `cargo test -p factory-native --test codex_dynamic_tools --locked` with the
  installed 0.156.1 binary passed against a loopback Responses mock.
- `CODEX_BIN=... cargo test -p factory-native --test e2e_fixture_dispatch codex_ --locked`
  passed both the fixture-stdio adapter E2E and the real-executable E2E; the
  latter reached a Git candidate with two Core-bound upstream requests and
  `ACCEPTED` is not claimed because verifier/reviewer were not run by that
  focused test.
- OpenCode fixture-provider regression passed.
- Full workspace tests and strict Clippy passed at the author checkpoint.

## Codex live UAT addendum (2026-10-26)

A separately authorized, bounded live Codex UAT used genuine `gpt-6-luna`
inference through the real App Server. The model used only `write_file` and
the frozen `cargo test` `run_command`, and produced candidate
`635336d3d525a20ab4079ae04586c4e2f7f1ab74` (`pub fn discount` returning
`cents - cents / 10`). Core recorded one session, three bound `/v1/responses`
requests, 6,342 input + 186 output tokens, a 7,086-micro configured-rate cost,
and `sandbox_process_identity_absent` cleanup. The independent verifier
executed `cargo test` successfully, the deterministic Reviewer PASSed, and the
Quality Gate returned `ACCEPTED`.

The live run used a test-only local proxy mapping Factory `/v1/responses` egress
to the authenticated ChatGPT Codex backend; that proxy is not part of the
merged runtime. The Reviewer was deterministic, not a semantic model. No
cancellation/revocation, native ChatGPT-backend configuration, exact external
invoice cost, human security certification, ISOLATED execution or deployment
was tested. Evidence:
[Codex GPT-6 Luna live UAT](../evidence/codex-gpt6-luna-live-uat-2026-10-26.md).
Codex built-in goal/code-mode/collaboration compatibility remains
version-sensitive and fail-closed.

### Codex managed configuration addendum (2026-10-26)

`factory start` and `factory doctor` accept a strict Codex Worker profile with
the native App Server binary, egress bridge, pinned OpenAI API upstream and
both explicit cost rates. A placeholder-token preflight passed without
inference; no Campaign or provider session was created. Native ChatGPT OAuth
authentication is **BLOCKED** by the credential boundary: the broker injects a
single pinned bearer credential and cannot import or pass through Codex-owned
OAuth refresh/account headers. The only supported Codex live authentication mode
is an operator API key at `https://api.openai.com/v1`; live Codex inference is
not tested. Evidence:
[Codex managed configuration](../evidence/codex-managed-config-2026-10-26.md).

## Container-native Compose iteration (2026-10-26)

A Product Owner campaign directive authorized a container-native installation
target. The implementation branch adds:

- a mandatory rootless Compose stack for Core/Office, containerized Supervisor
  and Worker, verifier/reviewer, a dedicated nested Podman engine and narrow
  runtime broker;
- first-run Office enrollment, encrypted Core-owned provider settings,
  revocable credentials and a semantic-review policy toggle;
- managed Git import, Office project/capability configuration, typed checks and
  scoped delegation creation;
- automatic independent verifier and reviewer dispatch and deterministic gate
  projection;
- per-service configuration volumes, a bundle-based provider image transfer and
  startup convergence without host toolchains.

Author-run evidence: a deterministic clean Compose deployment imported a Git
project, produced a candidate through the fixture provider in a real nested
container, executed independent verifier/reviewer evidence, reached
`ACCEPTED`, and preserved Project/Campaign/gate state across restart. The
same stack passed real-Chromium first-run onboarding and provider
save/test/revoke; two Workers produced two concurrent accepted candidates.
See [container-native status](container-native-status.md),
[phase-zero evidence](../evidence/container-native-phase0-2026-10-26.md) and
[Compose deployment](../operations/compose-deployment.md).

This iteration does **not** claim a paid provider UAT, live OpenCode-in-container
inference, Codex API-key live inference, containerized Codex stdio, native
ChatGPT OAuth, SEC-01 isolation, Windows, independent security review or merge
authorization. The historical host-managed acceptance remains provenance.
