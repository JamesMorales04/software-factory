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
| PR #17 implementation baseline | `8c627bebf7f8245cc5ee4f904d70825017bd33f4` |
| Product | Software Factory, independent Rust implementation |
| Merge state | PR #17 Draft; not mergeable as migration completion |

PR #16 is a behavior/provenance oracle only. Its Go implementation is not
merged and is not authoritative.

## Scope classes

- `REQUIRED_FOR_MIGRATION` — current main behavior and replacement operation.
- `REQUIRED_FOR_SECURITY` — mandatory fail-closed boundary.
- `REQUIRED_FOR_PR16_PARITY` — S4-D A1–A12 behavior.
- `REQUIRED_FOR_VERIFICATION` — evidence needed before deletion/merge.
- `FUTURE_FEATURE` — retained in the roadmap, not implemented in this closure.
- `HISTORICAL_ONLY` — provenance; never current runtime authority.

See [capability decisions](../product/capability-decisions.md) for the complete
portfolio and the [S4-D recovery matrix](s4d-rust-recovery-matrix.md) as a dated
source-mapping snapshot; this ledger is the current status authority.

## S4-D A1–A12 disposition

| ID | Capability | Status | Rust state | Blocker / next evidence |
|---|---|---|---|---|
| A1 | Semantic intake and typed assessment | `PARTIAL` | Strict typed assessment/storage; deterministic fixture assessor and operator routes; no model assessor | Add direct provider assessor, observation, Action creation, reconciliation, process tests |
| A2 | Proposal validation and frozen plan | `PARTIAL` | Validation, config binding, freeze revalidation, per-unit specs, parallel/fan-in plans and Campaign/Run/Attempt creation | Full plan projection and crash recovery |
| A3 | Standing delegation | `PARTIAL` | Sealed grants, atomic reservation, exact route allowlist, revocation, pre-model release | Campaign integration and ratified autonomous policy |
| A4 | Authorized autonomous Campaign start | `PARTIAL` | Default-off R0 policy; single- and multi-unit plans create Campaign/Run/Attempts atomically | Ratified issuer policy, provider mode and browser multi-unit evidence |
| A5 | Immediate revocation containment | `PARTIAL` | Revocation blocks Lead/Campaign/Run/Attempt work and marks pending WorkGraph tasks/integrations failed | Provider session reconciliation and independent cleanup evidence remain |
| A6 | Bounded WorkGraph | `IMPLEMENTED_UNVERIFIED` | Validation, persisted units, parallel roots, fan-in and final integration nodes, per-unit specs; parallel/fan-in E2E reach ACCEPTED; failed dependency, pre-dispatch restart and integration-node failure process tests pass at `a32c6d5`/`d50bd0f`/`3840b39`; in-flight root-lease restart/recovery passes at `0435159`/`4bcd584` | Browser graph projection; valid disjoint scopes make a natural Git conflict unreachable in the fixture |
| A7 | Dependency-aware execution/recovery | `PARTIAL` | Parallel roots, sequential/fan-in promotion, integrated candidate recording; failed dependency and failed integration reach terminal FAILED without dependent dispatch; frozen fan-in graph resumes after Core restart before dispatch (`d50bd0f`); operator Campaign cancellation fences runs, attempts, tasks, integrations and leases across restart (`4bcd584`) | Cancelling an in-flight integration node and browser recovery projection remain untested |
| A8 | Deterministic Git integration | `IMPLEMENTED_UNVERIFIED` | Supervisor integration, integrated lineage, verifier support, parallel final and fan-in E2E to ACCEPTED; integration failure retains exact root-candidate lineage and produces no integrated candidate (`3840b39`) | Adversarial cross-project integration and conflict-path assessment |
| A9 | Atomic Lead/Campaign start | `PARTIAL` | Multi-unit Campaign/Run/Attempt creation in one transaction; parallel and fan-in E2E reach ACCEPTED | Add crash-injection and restart scheduler coverage |
| A10 | CAP-014 semantic evaluation | `IMPLEMENTED_UNVERIFIED` | Frozen Rust corpus, baseline, blinded scoring, thresholds, CLI tests | Independent model score and separately authorized live run remain absent; no CAP-014 certification |
| A11 | Office Lead/WorkGraph workflows | `PARTIAL` | Lead submit/assess/freeze/reload, Campaign detail and WorkGraph task projection are browser-tested; multi-unit process evidence exists | Browser multi-unit graph and recovery/cleanup visibility |
| A12 | Bounded live certification tooling | `PARTIAL` | Default-closed readonly provider smoke | Core-owned budget/session ledger and governed S4-D harness |

## Merge-readiness gates

| Gate | Status | Evidence / blocker |
|---|---|---|
| M1 Architecture | `PARTIAL` | One Rust Core/Worker/Supervisor/Office path exists; Go tree still active |
| M2 PR #16 parity | `PARTIAL` | A1–A12 not complete; see table |
| M3 Repository hygiene | `BLOCKED` | 96 tracked `.go` files after two retirement batches; Go module/lint/config and legacy jobs remain; strict scanner not green |
| M4 Runtime | `PARTIAL` | Deployed Core + managed Supervisor/Worker + named-volume execution verified at `5a323f8` |
| M5 Functional execution | `PARTIAL` | R0/FIXTURE and single-unit Lead Campaign end-to-end verified at `5a323f8`; Codex thread/turn terminal classification and session identity are fixture-tested at `c375d71`; provider coding fail-closed |
| M6 Security | `BLOCKED` | Same-UID host pathname race removed for sandbox mounts; credential confinement, provider-in-sandbox, independent isolation not certified |
| M7 Quality | `PARTIAL` | Sandboxed verifier and deterministic gate verified at `5a323f8`; independent deterministic diff reviewer process runs in fixture and Office E2E (`4bcd584`); no model review |
| M8 Office | `PARTIAL` | Rust Office fixture, Lead and Campaign-detail browser flows pass; recovery/cleanup visibility remains |
| M9 Testing | `PARTIAL` | Rust process/browser E2E at earlier candidates plus failed-dependency, integration-failure and in-flight recovery process tests at `a32c6d5`/`3840b39`/`0435159`; cancellation and independent-reviewer process tests at `4bcd584`; repository identity/event-kind vocabulary at `ec2e75b`; credential-redacting Core/Supervisor logging at `c0ed31d`; 235 tests + 2 ignored, audit, architecture, dispatch, negatives, recovery and Office browser pass locally; exact-head native Rust (both runs), Office browser and architecture CI PASS at `14506eab7345df3af2b2bc57658224906291be33`; legacy Go/Paseo browser jobs still fail |
| M10 Governance | `PARTIAL` | Rust architecture checker now covers retired Go checker and base-ref ADR history; the `architecture-consistency` CI job passed on `a367766`; strict hygiene, legacy live-entrypoint guard and Go CI remain |
| M11 Independent review | `NOT_STARTED` | No independent full-diff/security review at final candidate |
| M12 Delivery authorization | `PENDING` | Product Owner merge/scope decisions not yet requested |
| M13 Product independence | `PARTIAL` | Predecessor assessment removed, all 44 CAP IDs and opportunities present, strict audit empty; registry/roadmap decision and milestone consistency still being closed |

## Verified execution path

At the author checkpoint:

- rootless Podman named-volume execution produced a candidate;
- Supervisor state survived reconciliation and rejected path replacement;
- verifier executed frozen checks in a separate named-volume sandbox;
- reviewer receipt and deterministic gate reached `ACCEPTED`;
- containerized Core served the Rust Office;
- `factory start/stop/status` managed Core, Supervisor and Worker;
- real Chromium Office journey passed.

Candidate-bound evidence: [native-rust-closure-2026-10-24.json](../evidence/native-rust-closure-2026-10-24.json)
(`5a323f8`; author-run and exact-head CI). The reviewer/cancellation slice is
recorded in
[native-rust-closure-2026-10-24-review-cancel.json](../evidence/native-rust-closure-2026-10-24-review-cancel.json)
(`4bcd584`; author-run) and
[native-rust-closure-2026-10-24-repository-identity.json](../evidence/native-rust-closure-2026-10-24-repository-identity.json)
(`ec2e75b`; author-run) and
[native-rust-closure-2026-10-24-log-redaction.json](../evidence/native-rust-closure-2026-10-24-log-redaction.json)
(`c0ed31d`; author-run) and
[native-rust-exact-head-ci-2026-10-24.json](../evidence/native-rust-exact-head-ci-2026-10-24.json)
(`b91bd96`; GitHub Actions) and
[native-rust-exact-head-ci-2026-10-24-final.json](../evidence/native-rust-exact-head-ci-2026-10-24-final.json)
(`14506eab7345df3af2b2bc57658224906291be33`; GitHub Actions, same job set). None is provider coding, OS isolation
certification, independent acceptance or deployment authorization.

## Legacy deletion status

One fixture batch is `DELETED_AFTER_REPLACEMENT`: commit `1d10f065a2143cbe8fc84006113ec315610420bd`
replaced the protected S4-A `.go` test datum with `policy.txt` (99 to 98
tracked Go files). On that exact code commit, the deterministic S4-B oracle's
B8 protected-diff checks reached `HUMAN_REQUIRED` and prohibited repair. The
full script exited 1 at its separate B11 Chromium Campaigns-heading timeout;
the overall S4-B suite is not a Rust acceptance result. Go/Paseo removal still
requires the relevant Rust behavior and tests first. The predecessor capability
assessment was removed only after [capability decisions](../product/capability-decisions.md)
preserved its selected portfolio and engineering opportunities.

The Go architecture checker and wrapper were deleted at
`c2923aca1ae58df92f623b1670e40c0175273a39` (98 to 96 Go files) after
porting the registry, evidence, path, certification and ADR checks to Rust and
switching the `architecture-consistency` CI job. Sol's cross-review repair at
`eb36cade9749614742ad0b0cf7b823173abdc43f` added read-path confinement
and manifest-fragment negatives. Later commits `2cdd773` and `4fb29e9`
restored missing claim, manifest, reference and dependency regression cases
after independent review. Eight checker tests, Clippy and an actual
`--base-ref origin/main` invocation pass locally. The `architecture-consistency`
GitHub CI job passed on `6947b42` and `a367766` after each PR impact declaration was rebound
to that head; independent acceptance remains pending. The workflow still
installs Go for a separate legacy live-entrypoint guard.

Codex direct thread/turn protocol fixtures now reject empty identities, keep
terminal outcomes distinct, and bind completion to the started turn. Sol's
review repaired provider event replay and usage-overflow atomicity (`37e387b`);
independent review found and Sol repaired pre-turn cancellation deadlock
(`c375d71`). These are deterministic fixtures only. No paid provider coding
turn or credential/isolation certification occurred.

OpenCode prompt helpers now correlate matching session SSE idle/error events
with a synchronous assistant HTTP response in deterministic fixtures
(`17bcac2`). Independent review found an abort race: idle after a requested
abort did not prove cancellation. The outcome is now explicitly
`AbortRequestedAndIdle`, with a completed-response race fixture (`5a5df79`).
The reviewer verified the repair. The loopback HTTP prompt, abort, assistant
read and delete path with credential/redirect checks and event-before-session
ordering is exercised against a local fake server (`6cf3786`/`d0b8933`).
These helpers still have no production prompt or abort caller; provider
lifecycle remains incomplete.

The [deletion and replacement map](native-rust-deletion-map.md) classifies all
99 tracked Go files at `102f6a046f1f907546e8bc60da622388f80bd95c` by
replacement, missing oracle, deletion readiness, owner and verification command.
The map records the two completed batches and the remaining blockers.

The inventory is now 96 tracked `.go` files. No further file is independently
deletion-ready: all remaining Go packages are one connected module, and B2
`domain`, `candidate`, `project`, and `recovery` are imported by the remaining
C1/C2 commands, API, store, orchestrator, verifier and reviewer. Deleting any
of them requires retiring their dependents and the Go CI in the same batch,
which is not truthful while provider coding, durable session recovery,
telemetry and multi-project capability parity remain absent. Rust now owns the
recovery failure classifier, bounded candidate diff/content evidence, the
independent reviewer process, canonical repository identity binding, a closed
durable event-kind vocabulary and bounded credential redaction in Core and
Supervisor logs. The legacy event payload projection has
no production Rust equivalent because Rust never persists a payload field; any
future payload surface must re-introduce that read-boundary test. The exact
per-file dependency blocker is recorded in the deletion map.

Strict tracked-tree independence audit:

```sh
git grep -n -i -E 'A[E]H|agentic[-_ ]engineering[-_ ]harness' -- .
git ls-files | grep -Ei 'A[E]H|agentic[-_ ]engineering[-_ ]harness'
```

Both must be empty for M13. Go/path and strict hygiene audits remain separate.

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
