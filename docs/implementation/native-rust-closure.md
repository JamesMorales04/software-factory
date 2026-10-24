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
| A6 | Bounded WorkGraph | `IMPLEMENTED_UNVERIFIED` | Validation, persisted units, parallel roots, fan-in and final integration nodes, per-unit specs; parallel and fan-in process E2E reach ACCEPTED | Failure/restart process tests and browser graph projection |
| A7 | Dependency-aware execution/recovery | `PARTIAL` | Parallel roots, sequential/fan-in promotion, integrated candidate recording, failure/cancellation state propagation | Conflict and restart process tests |
| A8 | Deterministic Git integration | `IMPLEMENTED_UNVERIFIED` | Supervisor integration, integrated lineage, verifier support, parallel final and fan-in E2E to ACCEPTED | Conflict/failure process test and adversarial cross-project integration |
| A9 | Atomic Lead/Campaign start | `PARTIAL` | Multi-unit Campaign/Run/Attempt creation in one transaction; parallel and fan-in E2E reach ACCEPTED | Add crash-injection and restart scheduler coverage |
| A10 | CAP-014 semantic evaluation | `IMPLEMENTED_UNVERIFIED` | Frozen Rust corpus, baseline, blinded scoring, thresholds, CLI tests | Independent model score and separately authorized live run remain absent; no CAP-014 certification |
| A11 | Office Lead/WorkGraph workflows | `PARTIAL` | Lead submit/assess/freeze/reload, Campaign detail and WorkGraph task projection are browser-tested; multi-unit process evidence exists | Browser multi-unit graph and recovery/cleanup visibility |
| A12 | Bounded live certification tooling | `PARTIAL` | Default-closed readonly provider smoke | Core-owned budget/session ledger and governed S4-D harness |

## Merge-readiness gates

| Gate | Status | Evidence / blocker |
|---|---|---|
| M1 Architecture | `PARTIAL` | One Rust Core/Worker/Supervisor/Office path exists; Go tree still active |
| M2 PR #16 parity | `PARTIAL` | A1–A12 not complete; see table |
| M3 Repository hygiene | `BLOCKED` | 99 tracked `.go` files plus Go module/lint/config; strict scanner not green |
| M4 Runtime | `PARTIAL` | Deployed Core + managed Supervisor/Worker + named-volume execution verified at `5a323f8` |
| M5 Functional execution | `PARTIAL` | R0/FIXTURE and single-unit Lead Campaign end-to-end verified at `5a323f8`; provider coding fail-closed |
| M6 Security | `BLOCKED` | Same-UID host pathname race removed for sandbox mounts; credential confinement, provider-in-sandbox, independent isolation not certified |
| M7 Quality | `PARTIAL` | Sandboxed verifier and deterministic gate verified at `5a323f8`; reviewer is receipt-only |
| M8 Office | `PARTIAL` | Rust Office fixture, Lead and Campaign-detail browser flows pass; recovery/cleanup visibility remains |
| M9 Testing | `PARTIAL` | Rust suite/process/browser E2E and exact-head CI pass at `5a323f8`; final candidate pending later docs/fixes |
| M10 Governance | `PARTIAL` | Architecture checker passes including base-ref ADR history after ADR-0021; strict hygiene and legacy CI remain |
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
(`5a323f8`; author-run and exact-head CI). It is not provider coding, OS
isolation certification, independent acceptance or deployment authorization.

## Legacy deletion status

No legacy batch is `DELETED_AFTER_REPLACEMENT` yet. Go/Paseo removal requires the
relevant Rust behavior and tests first. The predecessor capability assessment
was removed only after [capability decisions](../product/capability-decisions.md)
preserved its selected portfolio and engineering opportunities.

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
