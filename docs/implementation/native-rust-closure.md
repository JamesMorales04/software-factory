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
portfolio and [S4-D recovery matrix](s4d-rust-recovery-matrix.md) for source
mapping.

## S4-D A1–A12 disposition

| ID | Capability | Status | Rust state | Blocker / next evidence |
|---|---|---|---|---|
| A1 | Semantic intake and typed assessment | `PARTIAL` | Strict typed assessment/storage; deterministic fixture assessor and operator routes; no model assessor | Add direct provider assessor, observation, Action creation, reconciliation, process tests |
| A2 | Proposal validation and frozen plan | `PARTIAL` | Deterministic validation, project config hash binding, freeze revalidation, durable rows | Create per-unit specs/Campaign and expose complete projection |
| A3 | Standing delegation | `PARTIAL` | Sealed grants, atomic reservation, exact route allowlist, revocation, pre-model release | Campaign integration and ratified autonomous policy |
| A4 | Authorized autonomous Campaign start | `NOT_STARTED` | Fail-closed rejection | Ratified policy; atomic start + audit; model cannot self-authorize |
| A5 | Immediate revocation containment | `PARTIAL` | New reservations blocked | Contain open requests/runs/sessions with durable evidence |
| A6 | Bounded WorkGraph | `PARTIAL` | Deterministic graph validation | Persist graph, per-unit specs, integration node |
| A7 | Dependency-aware execution/recovery | `NOT_STARTED` | No graph scheduler | Core scheduler, leases, fan-in, restart, failure propagation |
| A8 | Deterministic Git integration | `PARTIAL` | Tested integration primitives | Wire Worker/Core candidate lineage and Campaign integration |
| A9 | Atomic Lead/Campaign start | `NOT_STARTED` | No start transaction | Crash-injection and idempotent replay test |
| A10 | CAP-014 semantic evaluation | `IMPLEMENTED_UNVERIFIED` | Frozen Rust corpus, baseline, blinded scoring, thresholds, CLI tests | Independent model score and separately authorized live run remain absent; no CAP-014 certification |
| A11 | Office Lead/WorkGraph workflows | `PARTIAL` | Six route Rust Office; browser journey passes | Lead/delegation/detail/recovery projections and reload tests |
| A12 | Bounded live certification tooling | `PARTIAL` | Default-closed readonly provider smoke | Core-owned budget/session ledger and governed S4-D harness |

## Merge-readiness gates

| Gate | Status | Evidence / blocker |
|---|---|---|
| M1 Architecture | `PARTIAL` | One Rust Core/Worker/Supervisor/Office path exists; Go tree still active |
| M2 PR #16 parity | `PARTIAL` | A1–A12 not complete; see table |
| M3 Repository hygiene | `BLOCKED` | 99 tracked `.go` files plus Go module/lint/config; strict scanner not green |
| M4 Runtime | `PARTIAL` | Deployed Core + managed Supervisor/Worker + named-volume execution verified in author run |
| M5 Functional execution | `PARTIAL` | R0/FIXTURE Campaign end-to-end verified; provider coding fail-closed |
| M6 Security | `BLOCKED` | Same-UID host pathname race removed for sandbox mounts; credential confinement, provider-in-sandbox, independent isolation not certified |
| M7 Quality | `PARTIAL` | Sandboxed verifier and deterministic gate verified; reviewer is receipt-only |
| M8 Office | `PARTIAL` | Rust Office fixture browser journey `ACCEPTED`; S4-D views absent |
| M9 Testing | `PARTIAL` | Rust suite and process/browser E2E pass at current author checkpoint; exact final candidate pending |
| M10 Governance | `PARTIAL` | Architecture checker passes; strict hygiene, docs and capability registry still being closed |
| M11 Independent review | `NOT_STARTED` | No independent full-diff/security review at final candidate |
| M12 Delivery authorization | `PENDING` | Product Owner merge/scope decisions not yet requested |
| M13 Product independence | `VERIFIED` | Predecessor assessment removed; `capability-decisions.md` preserves all 44 CAPs and engineering opportunities; strict case-insensitive audit is empty at this checkpoint |

## Verified execution path

At the author checkpoint:

- rootless Podman named-volume execution produced a candidate;
- Supervisor state survived reconciliation and rejected path replacement;
- verifier executed frozen checks in a separate named-volume sandbox;
- reviewer receipt and deterministic gate reached `ACCEPTED`;
- containerized Core served the Rust Office;
- `factory start/stop/status` managed Core, Supervisor and Worker;
- real Chromium Office journey passed.

This is author-run fixture evidence. It is not provider coding, OS isolation
certification, independent acceptance or deployment authorization.

## Legacy deletion status

No legacy batch is `DELETED_AFTER_REPLACEMENT` yet. Go/Paseo removal requires the
relevant Rust behavior and tests first. The predecessor capability assessment
was removed only after [capability decisions](../product/capability-decisions.md)
preserved its selected portfolio and engineering opportunities.

Strict tracked-tree independence audit:

```sh
git grep -n -i -E 'AEH|agentic[-_ ]engineering[-_ ]harness' -- .
git ls-files | grep -Ei 'AEH|agentic[-_ ]engineering[-_ ]harness'
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
