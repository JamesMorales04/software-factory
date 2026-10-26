# Native Rust migration closure ledger

**Status:** current migration status authority. Updated 2026-10-25.
This ledger separates implemented behavior from verified evidence. It does not
replace candidate-bound test output, independent review or protected approval.

## Identity

| Item | Value |
|---|---|
| Authoritative main baseline | `102d7ba29295554bd61ae71956347a025d219d7b` |
| PR #16 behavioral reference | `19cb4951f7a3d42a1ca27b1800cf49401c81ecc6` |
| PR #17 branch | `native-rust-replacement-20261023` |
| Product | Software Factory, independent Rust implementation |
| Legacy implementation | retired from the tracked tree; retained only in Git history and archived provenance |
| Integration candidate | `067d26b9d229113b17c934dde0f5290af7008403` (local author-run gates); exact-head PR checks are authoritative |
| Merge state | PR #17 Draft; protected merge requires exact-head CI and owner authorization |

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
| A1 | Semantic intake and typed assessment | `IMPLEMENTED_UNVERIFIED` | Strict typed assessment/storage; model assessor now uses the Core-owned model-call gateway with strict schema, budget reservation and a model-Lead objective-to-ACCEPTED E2E | Live paid semantic quality is not tested; no human acceptance |
| A2 | Proposal validation and frozen plan | `PARTIAL` | Validation, config binding, freeze revalidation, per-unit specs, parallel/fan-in plans, Campaign/Run/Attempt creation and Office plan projection | Crash-injection and full restart scheduler coverage remain |
| A3 | Standing delegation | `PARTIAL` | Sealed grants, atomic reservation, exact route allowlist, revocation, provider authorization, and shared per-scope/global model-call budgets | Ratified autonomous issuer/risk policy remains an owner decision |
| A4 | Authorized autonomous Campaign start | `PARTIAL` | Default-off R0 policy; single- and multi-unit plans create Campaign/Run/Attempts atomically; model Lead E2E starts a Campaign | Live provider mode, browser multi-unit evidence and owner ratification remain |
| A5 | Immediate revocation containment | `PARTIAL` | Revocation blocks Lead/Campaign/Run/Attempt work and marks pending WorkGraph tasks/integrations failed; provider sessions are marked `CANCEL_REQUESTED`; unresolved sessions block replacement dispatch | Running model-call revocation and full provider-session cleanup remain untested |
| A6 | Bounded WorkGraph | `IMPLEMENTED_UNVERIFIED` | Validation, parallel roots, fan-in and final integration nodes, per-unit specs; parallel/fan-in E2E reach ACCEPTED; failed dependency, restart, integration failure and cancellation process tests pass | Browser graph projection and live-provider multi-unit execution remain |
| A7 | Dependency-aware execution/recovery | `PARTIAL` | Parallel roots, fan-in promotion, integrated candidate recording; failed dependency and integration reach terminal FAILED; frozen fan-in resumes after restart; cancellation fences runs/attempts/tasks/integrations/leases | In-flight integration cancellation is DB-tested but not process-tested |
| A8 | Deterministic Git integration | `IMPLEMENTED_UNVERIFIED` | Supervisor integration, integrated lineage, verifier support, parallel final/fan-in E2E to ACCEPTED; integration failure retains exact root lineage | Adversarial Git conflict path remains unreachable in the fixture |
| A9 | Atomic Lead/Campaign start | `PARTIAL` | Multi-unit Campaign/Run/Attempt creation in one transaction; parallel/fan-in E2E reach ACCEPTED | Crash injection and restart scheduler coverage remain |
| A10 | CAP-014 semantic evaluation | `IMPLEMENTED_UNVERIFIED` | Frozen corpus, baseline, blinded scoring, thresholds; `execute` collects raw outputs through Core with budgeted reservations and durable FAILED evidence on exhaustion | Independent blinded score and authorized live model-quality run remain absent; no CAP-014 certification |
| A11 | Office Lead/WorkGraph workflows | `PARTIAL` | Lead assessment/proposal/plan, WorkGraph, candidates, receipts, gate, model-call budgets, telemetry status, cancellation and runtime projections are browser-tested | Browser multi-unit graph and full recovery/cleanup interaction remain |
| A12 | Bounded live certification tooling | `PARTIAL` | Default-closed read-only provider smoke; durable provider intent/terminal/usage ledger and exact candidate path | Governed S4-D live harness and separately authorized live certification remain |

## Merge-readiness gates

| Gate | Status | Evidence / blocker |
|---|---|---|
| M1 Architecture | `PARTIAL` | One Rust Core/Worker/Supervisor/Office path; no legacy tracked implementation. Provider coding and isolation remain fail-closed. |
| M2 PR #16 parity | `PARTIAL` | A1–A12 concepts substantially implemented; crash/live/browser multi-unit gaps remain |
| M3 Repository hygiene | `PASS` | Zero tracked Go files; strict hygiene passes at `067d26b9d229113b17c934dde0f5290af7008403`; all 44 CAP IDs preserved |
| M4 Runtime | `IMPLEMENTED_UNVERIFIED` | Full local lifecycle at `067d26b`: container image build, `factory start`/`doctor`/`stop`, host Supervisor/Worker, Office HTTP, Project/Action/Campaign/Attempt, candidate, verifier, reviewer, ACCEPTED, restart persistence and clean stop |
| M5 Functional execution | `IMPLEMENTED_UNVERIFIED` | R0/FIXTURE, model Lead and deterministic source-aware provider paths reach ACCEPTED; semantic Reviewer and CAP-014 use the Core gateway; live provider coding remains fail-closed |
| M6 Security | `BLOCKED` | The long-lived provider credential remains in the trusted Worker; bubblewrap and egress hardening are implemented. Monitor-based storage limits, running model-call revocation and human certification remain open. ISOLATED remains blocked. |
| M7 Quality | `PARTIAL` | Sandboxed verifier, deterministic reviewer, model semantic reviewer with mandatory deterministic pre-review, and deterministic gate pass; no live model-quality or human acceptance |
| M8 Office | `IMPLEMENTED_UNVERIFIED` | Rust Office browser journey passes with WorkGraph, plan, findings, model-call budgets, telemetry, cancellation and runtime projections |
| M9 Testing | `PARTIAL` | 317 Rust tests discovered; workspace tests, browser real-Chromium E2E, CAP-014, observability, WorkGraph, recovery/cancellation, provider and Office journeys pass locally; exact-head PR checks are authoritative |
| M10 Governance | `PARTIAL` | Rust architecture checker, roadmap regeneration and strict hygiene pass; browser real-Chromium lane added to the office job; conditional Podman/provider-compat lanes remain manual |
| M11 Independent review | `PARTIAL` | Independent same-model security/architecture/release reviews were performed and findings repaired; human security certification remains absent |
| M12 Delivery authorization | `PENDING` | Product Owner merge decision and exact-head CI remain |
| M13 Product independence | `PARTIAL` | All 44 CAP IDs and opportunities preserved; registry/roadmap updated; live model-quality and human certification remain |

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
- Not claimed: live provider execution from this start route, real paid model
  quality, Codex coding, ISOLATED execution, human security review or final
  merge. The Office journey uses the deterministic fixture assessor and does
  not submit real provider credentials.

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

This addendum does **not** upgrade the merged tree. The repair PR is not merged;
`factory start` still cannot configure the live provider/model route without
manual host processes. Failed provider sessions can leave unmeasured usage or
missing cleanup receipts. Autonomous live-provider Campaign start remains
FIXTURE-only. Codex GPT-6 Luna coding was previously BLOCKED because no Codex
coding adapter/tool loop existed; a follow-up branch now implements a Codex
App Server adapter with client-mediated dynamic tools, bounded multi-request
egress binding and deterministic real-executable tests, but it is not yet
independently reviewed, merged, or live-paid tested. Paid cancellation and
revocation were not exercised.
ISOLATED, Windows, deployment, human security review and final merge remain
uncertified. See the [F1-S4-LIVE-UAT report](../evidence/f1-s4-live-uat-2026-10-25.md).

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

A focused branch adds a native Codex `0.156.1` App Server coding path for the
frozen route `codex/gpt-6-luna`. It drives the real executable over stdio in
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

This is not a live GPT-6 Luna coding PASS, does not prove independent security
review, and does not certify isolation, cleanup, or cost accuracy. Codex
built-in goal/code-mode/collaboration compatibility is version-sensitive.
