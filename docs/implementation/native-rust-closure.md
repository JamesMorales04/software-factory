# Native Rust migration closure ledger

**Status:** current migration status authority. Updated 2026-10-24.
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
| Merge state | PR #17 Draft; not mergeable as migration completion |

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
| A1 | Semantic intake and typed assessment | `PARTIAL` | Strict typed assessment/storage; deterministic fixture assessor and operator routes; no model assessor | Add direct provider assessor, observation, Action creation, reconciliation, process tests |
| A2 | Proposal validation and frozen plan | `PARTIAL` | Validation, config binding, freeze revalidation, per-unit specs, parallel/fan-in plans and Campaign/Run/Attempt creation | Full plan projection and crash recovery |
| A3 | Standing delegation | `PARTIAL` | Sealed grants, atomic reservation, exact route allowlist, revocation and pre-model release; provider authorization reserves request/session/attempt budget atomically | Ratified autonomous policy and broader provider spend controls |
| A4 | Authorized autonomous Campaign start | `PARTIAL` | Default-off R0 policy; single- and multi-unit plans create Campaign/Run/Attempts atomically; explicit trusted-local provider authorization is available | Ratified issuer policy, live provider mode and browser multi-unit evidence |
| A5 | Immediate revocation containment | `PARTIAL` | Revocation blocks Lead/Campaign/Run/Attempt work and marks pending WorkGraph tasks/integrations failed; unresolved provider sessions block replacement dispatch | Provider-session cancellation reconciliation and independent cleanup evidence remain |
| A6 | Bounded WorkGraph | `IMPLEMENTED_UNVERIFIED` | Validation, persisted units, parallel roots, fan-in and final integration nodes, per-unit specs; parallel/fan-in E2E reach ACCEPTED; failed dependency, pre-dispatch restart and integration-node failure process tests pass; in-flight root-lease recovery and operator cancellation pass | Browser graph projection; valid disjoint scopes make a natural Git conflict unreachable in the fixture |
| A7 | Dependency-aware execution/recovery | `PARTIAL` | Parallel roots, sequential/fan-in promotion, integrated candidate recording; failed dependency and failed integration reach terminal FAILED without dependent dispatch; frozen fan-in graph resumes after Core restart; cancellation fences runs, attempts, tasks, integrations and leases across restart | Process-level in-flight integration cancellation and browser recovery projection remain untested |
| A8 | Deterministic Git integration | `IMPLEMENTED_UNVERIFIED` | Supervisor integration, integrated lineage, verifier support, parallel final and fan-in E2E to ACCEPTED; integration failure retains exact root-candidate lineage and produces no integrated candidate | Adversarial cross-project integration and conflict-path assessment |
| A9 | Atomic Lead/Campaign start | `PARTIAL` | Multi-unit Campaign/Run/Attempt creation in one transaction; parallel and fan-in E2E reach ACCEPTED | Crash-injection and restart scheduler coverage |
| A10 | CAP-014 semantic evaluation | `IMPLEMENTED_UNVERIFIED` | Frozen Rust corpus, baseline, blinded scoring, thresholds and CLI tests | Independent model score and separately authorized live run remain absent; no CAP-014 certification |
| A11 | Office Lead/WorkGraph workflows | `PARTIAL` | Lead submit/assess/freeze/reload, Campaign detail and WorkGraph task projection are browser-tested | Browser multi-unit graph and recovery/cleanup visibility |
| A12 | Bounded live certification tooling | `PARTIAL` | Default-closed read-only provider smoke; durable provider intent/terminal/usage ledger and exact candidate path | Governed S4-D live harness and separately authorized live certification |

## Merge-readiness gates

| Gate | Status | Evidence / blocker |
|---|---|---|
| M1 Architecture | `PARTIAL` | One Rust Core/Worker/Supervisor/Office path; no legacy tracked implementation. Provider coding and isolation remain fail-closed. |
| M2 PR #16 parity | `PARTIAL` | A1–A12 product concepts not complete; see table |
| M3 Repository hygiene | `PASS` | Zero tracked Go files; strict hygiene passes locally and in exact-head CI at `3979e32bc94b387571d1d3c715022740c2fd216d` |
| M4 Runtime | `PARTIAL` | Deployed Core + managed Supervisor/Worker + named-volume execution verified at `5a323f8` |
| M5 Functional execution | `PARTIAL` | R0/FIXTURE and single-unit Lead Campaign end-to-end verified; deterministic source-aware multi-turn provider plan in a bubblewrap-confined provider process with mediated write_file/run_command, scoped egress broker and permission denial reaches independent verifier, reviewer and ACCEPTED gate; live provider coding fail-closed |
| M6 Security | `BLOCKED` | The long-lived provider credential remains in the trusted Worker; the deterministic provider runs under bubblewrap with no host HOME/repository/runtime socket or unrestricted network. Supervisor allocation and workspace reuse are generation-bound; mutations require a one-time, request-digest-bound, live-lease-bound operation grant and signed scope. A narrow egress broker, quotas and confined recovery primitives are implemented. The [independent model review](../security/independent-review-2026-10-25.md) HIGH findings are repaired; Worker compromise, long-operation revocation, monitor-based storage limits and human certification remain open. |
| M7 Quality | `PARTIAL` | Sandboxed verifier, independent deterministic reviewer process and deterministic gate pass; model review absent |
| M8 Office | `PARTIAL` | Rust Office browser journey passes, including the new Runtime provider-session/cleanup projection; deeper recovery actions remain |
| M9 Testing | `PARTIAL` | 267 Rust tests passed + 2 ignored, audit, architecture, generation/nonce/digest operation-grant negatives, bridge/egress/quota/identity tests, bubblewrap provider E2E, recovery/cancellation and Office browser pass locally and in exact-head CI at `2dcf03c888003b4f6bcb592a65934e66358e14bc`; actual provider and Podman lanes are local/conditional; human acceptance absent |
| M10 Governance | `PARTIAL` | Rust architecture checker, PR-impact gate and strict hygiene are the active CI path at `2dcf03c888003b4f6bcb592a65934e66358e14bc`; conditional Podman/provider-compat lanes are not run by ordinary CI; legacy workflows removed |
| M11 Independent review | `PARTIAL` | Independent Codex model review and two follow-ups; HIGH repairs plus running-operation fencing, real Podman and actual OpenCode/Codex compatibility evidence; human certification absent |
| M12 Delivery authorization | `PENDING` | Product Owner merge/scope decisions not yet requested |
| M13 Product independence | `PARTIAL` | Predecessor assessment removed; all selected CAP IDs and opportunities present; strict independence audit remains required |

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

## Product Owner decisions required

1. Ratify or amend standing-delegation issuer/risk/autonomy policy (A3/A4).
2. Authorize any final paid provider certificate separately; none is authorized
   by this ledger.
3. Decide release scope for Windows/Podman Machine if unverified.
4. Approve final merge after independent review and all mandatory gates.

## Update rule

Every status change must cite an exact candidate SHA and executable evidence.
Documentation, compilation, a probe or a fixture is not `VERIFIED` behavior.
Historical evidence is never rewritten to look like Rust certification.
