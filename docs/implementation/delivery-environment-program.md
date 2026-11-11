# Delivery and environment program — work packages and ownership

**Status:** Proposed WP0 plan — 2026-11-10. This document defines the execution
plan for
[ADR-0040](../architecture/decisions/ADR-0040-risk-tiered-delivery-and-auto-merge.md),
[ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md)
and
[ADR-0042](../architecture/decisions/ADR-0042-issue-linked-delivery-lifecycle.md).
It does not implement a capability and does not grant implementation, merge,
deployment or delivery authority.

**WP0 baseline:** `ba2dda9` on `docs/delivery-environment-contracts`.

**Contracts:**
[environment plan](../contracts/environment-plan.md),
[delivery policy](../contracts/delivery-policy.md),
[issue lifecycle](../contracts/issue-lifecycle.md).

**Audit:** [language-agnostic execution audit](language-agnostic-audit.md).
**License evidence:** [Nix/devenv license findings](../evidence/nix-devenv-license.md).

## 1. Scope

This program replaces three hidden assumptions with explicit, frozen records:

1. **Environment:** every project executes in a Core-frozen Nix/devenv
   environment compiled from its own conventions; no host/language-image
   fallback.
2. **Delivery:** Gate `ACCEPTED` is not delivery authority; R0/R1/R2/R3 follow
   the risk-tier policy, exact-head merge, fresh checks and incident rules.
3. **Issue traceability:** links are frozen at request creation, states are
   derived from Core evidence, PR comments are idempotent and issues close only
   after merge; partial issues stay open.

The program is docs/contracts first. Implementation work packages must not start
from this document alone; they need an authorized Action, the relevant ADRs, the
frozen contracts and a fresh candidate.

## 2. Work packages

| WP | Name | Depends | Owns (primary files) | Deliverable | Acceptance |
| --- | --- | --- | --- | --- | --- |
| WP0 | Contracts, ADRs, audit, index/reservations | — | `docs/architecture/decisions/ADR-0040..0042`, `docs/contracts/{environment-plan,delivery-policy,issue-lifecycle}.md`, `docs/implementation/{language-agnostic-audit,delivery-environment-program}.md`, `docs/evidence/nix-devenv-license.md`, index updates | Frozen interfaces; no code | Docs checks; contract review; no code/parity change |
| WP-E1 | Project conventions + environment compiler | WP0 | `native/src/planning/environment*` (new), `native/src/project_import.rs` conventions, `migrations/0071_environment_specs.sql` | Conventions snapshot, plan/spec, Lead review, license gate | Precedence/conflict, no default, freeze/hash, license tests |
| WP-E2 | Runtime/verifier/check environment binding | WP0, WP-E1 | `native/src/bin/verifier.rs`, `native/src/bin/runtime_broker.rs`, `native/src/execution.rs`, `native/src/lead.rs` (`SpecCheck`), `deploy/Containerfile`, `compose.yaml` | Checks run in the frozen spec; no global Rust image | Environment-specific verifier; no-secret/no-network/resource limits; real Podman |
| WP-E3 | Dependency/supply-chain generalization | WP0, WP-E1 | `native/src/dependencies.rs`, `native/src/dependency_broker.rs`, `native/src/bin/dependency_acquire.rs`, broker/compose config | Ecosystem adapters, SBOM/license/audit | Non-Rust lockfile flow; broker allowlist; atomic evidence |
| WP-E4 | Planning/stack/quality generalization | WP0, WP-E1, WP-E2 | `native/src/planning/stack.rs`, `native/src/planning/*`, `native/src/agentic.rs`, Core planning handlers | Environment artifact, no hardcoded language default | Golden language fixtures; plan/freeze integration |
| WP-D1 | Delivery policy and authorization | WP0 | `native/src/db/delivery*` (new), `migrations/0073_delivery_policies.sql`, Core routes | Versioned project policy; R0/R1/R2/R3 eligibility | Disabled/R0/R1/R2/R3 matrix; charter expiry/revocation |
| WP-D2 | Delivery execution, incidents, final source | WP0, WP-D1 | `native/src/bin/core.rs` delivery handlers, `native/src/db/delivery*`, `migrations/0074_delivery_attempts.sql`, `0075_candidate_final_source.sql` | Exact-head merge, idempotency, post-merge check, incident, manual revert | Fake-GitHub matrix; restart/replay; merge identity |
| WP-I1 | Issue links and lifecycle | WP0 | `native/src/issue_intake.rs`, `native/src/db/issue_intake.rs`, `native/src/github.rs`, `migrations/0072_issue_links.sql` | Immutable links, six states, comment/close receipts | Freeze/immutability; transitions; partial close; idempotency |
| WP-O1 | Office surfaces | WP-E1, WP-D1, WP-D2, WP-I1 | `office/src/views/*`, `office/src/coreApi.ts`, `docs/product/office-parity.json` | Environment review, delivery panel, issue lifecycle | Per-option browser/a11y; parity matrix |
| WP-T1 | Tests, chaos, evidence | all | `native/tests/*`, `office/e2e-browser/*`, fixtures | Deterministic + real evidence | Fault matrix; browser matrix; exact-candidate manifest |

## 3. Waves and merge order

```text
WP0
 ├─> WP-E1 ─> WP-E2 ─> WP-E4
 │             └─────> WP-D2
 ├─> WP-E3
 ├─> WP-D1 ─> WP-D2
 └─> WP-I1
             WP-O1 after E1/D1/D2/I1
             WP-T1 continuously, final after all
```

- **Wave 0 — sequential:** WP0 only. Freeze the ADRs/contracts/audit and merge
  before feature work.
- **Wave 1 — parallel:** WP-E1, WP-D1, WP-I1, WP-E3 where the environment
  interface is stable. WP-E2 waits for WP-E1's plan/spec types.
- **Wave 2 — parallel:** WP-E2, WP-E4, WP-D2. WP-O1 starts once Core API types
  are frozen; it may use contract fixtures but must not invent state.
- **Wave 3 — integration:** WP-O1 live wiring, WP-T1 full deterministic/chaos and
  real Podman/browser evidence, independent acceptance.

Merge order: WP0 → (E1, D1, I1) → (E2, E3, E4, D2) → O1 → T1. An integration
owner resolves shared-file registrations in one small commit per wave.

## 4. Shared-file ownership and anti-conflict rules

Parallel agents must not edit these shared files directly:

| Shared file/area | Owner | Rule |
| --- | --- | --- |
| `native/src/bin/core.rs` | Core integration owner | WPs add functions in their own modules; the integration owner applies the small route/handler registration diff. |
| `native/src/db/mod.rs` | Core integration owner | One export/registration line per WP; no WP edits another WP's module. |
| `migrations/` | Migration owner per wave | WPs submit SQL requirements; only the migration owner assigns/applies numbers. WP0 reserves numbers, it does not create files. |
| `native/src/planning/mod.rs` | Planning integration owner | Additive types only; interface changes go through WP0 contract-change note. |
| `native/src/bin/worker.rs`, `supervisor.rs`, `verifier.rs` | Execution integration owner | One WP owns a file at a time; no cross-WP drive-by edits. |
| `compose.yaml`, `deploy/Containerfile`, `.devcontainer/` | Build/infra owner | Image/network/volume changes require container-native impact review and real Podman evidence. |
| `office/src/coreApi.ts` | Office integration owner | Backend WPs provide exact TypeScript types; only the integration owner merges them. |
| `office/src/views/*`, `office/src/navigation.ts` | WP-O1 | WP-O1 owns UI files; routes/navigation changes require the navigation owner. |
| `docs/README.md`, ADR index, contracts README | Docs owner | One docs owner for index edits; WPs add their own pages only. |
| `docs/product/office-parity.json` | Office parity owner | Only after a real implementation slice; WP0 does not change states/capabilities. |
| `docs/product/capabilities.yaml`, `docs/roadmap/ROADMAP.md` | Product/governance owner | Generated/registry changes follow their own review; no WP0 change. |
| `native/src/bin/architecture_check.rs`, `repo_hygiene.rs` | Governance owner | New docs must pass existing checks; do not weaken them. |

Additional rules:

1. Work in your own branch/worktree (`factory/delivery-env-wpN-*`). Do not edit
   another WP's worktree or `main`.
2. Add new files where possible; keep a WP's writes inside its own module/view.
3. If an interface change is required, write a contract-change note and stop;
   do not modify another WP or silently widen the contract.
4. Do not change a frozen ADR/contract unilaterally. A material change needs a
   new ADR or an explicit contract revision with all callers updated atomically.
5. Do not change `TRUSTED_LOCAL`, authorization, credentials, sandbox, merge or
   quality policy outside the protected approval path.
6. Do not claim `ACCEPTED`, `MERGED`, `DONE` or `VERIFIED` from a mock, a route
   smoke test or a screenshot.

## 5. Migration reservations

The next free migration is `0070`. WP0 reserves the following numbers and
**does not create the SQL files**. The migration owner creates them only when the
owning WP is authorized:

| Migration | Owner | Purpose | Notes |
| --- | --- | --- | --- |
| `0071_environment_specs.sql` | WP-E1 | `environment_specs`, plan/review binding, build receipts, license inventory references | Immutable spec rows; append-only revisions; exact hashes |
| `0072_issue_links.sql` | WP-I1 | `request_issue_links`, lifecycle revisions, comment/close receipts | Link set immutable; receipts unique by idempotency key |
| `0073_delivery_policies.sql` | WP-D1 | project delivery policy revisions/heads, charter grant binding | Append-only revisions; disabled default; R1 opt-in |
| `0074_delivery_attempts.sql` | WP-D2 | delivery attempts/decisions, post-merge checks, incidents, revert records | Unique idempotency key; immutable decisions |
| `0075_candidate_final_source.sql` | WP-D2 | exact final source/candidate binding used for delivery | Prevents salvage/race drift; merge commit/head identity |

Rules:

- No empty/placeholder migration is committed. A reserved number is not a
  migration until its owner supplies the schema, tests and migration check.
- Migrations are append-only, deterministic and transactional; startup applies
  them or fails with a diagnostic. Never recreate a damaged/incompatible
  database silently.
- A migration owner must not reuse a number assigned to another WP. If a number
  is reallocated, update this table and all references in the same change.
- The schema must preserve historical rows and fail closed for missing identity;
  no destructive rewrite of existing candidates/gates/issues.

## 6. Contract-change and evidence rules

A WP that needs a type/field change not in the frozen contract must:

1. write a short contract-change note (field, reason, compatibility, callers,
   tests, migration impact);
2. stop and request review; do not edit another WP's implementation;
3. on approval, update the contract, all in-repo callers, fixtures, tests, docs
   and the migration reservation atomically.

Each WP reports:

- Action and exact candidate SHA;
- changed files and interfaces;
- format/lint/typecheck/test results;
- real-use/Compose/browser evidence (or `NOT_TESTED`/`BLOCKED`);
- security/authorization impact (credential, network, sandbox, merge);
- migration number/schema and rollback/failure behavior;
- documentation/parity updates;
- remaining risks and integration work.

## 7. Definition of done for the program

A request is complete only when:

- the frozen `EnvironmentSpec` exists and is bound to checks/candidate/delivery;
- the License Gate/SBOM/attribution passed (or an exact owner override exists);
- the Gate is deterministic `ACCEPTED` for the exact candidate;
- delivery follows the risk-tier policy with exact head, fresh checks, branch
  protection and idempotency;
- the issue link set is frozen and the lifecycle is truthful (partial issues
  stay open);
- the Office surface exposes the same Core state with per-option browser/a11y
  evidence;
- no host-only dependency, secret leak, model-granted authority or unverified
  isolation claim exists;
- independent acceptance (separate from the author) is recorded for the exact
  candidate; otherwise the status is `NOT_TESTED`/`BLOCKED`.

## 8. Current status

**WP0:** this documentation slice. It creates contracts and reservations only.
No production code, migration, test, parity state or capability is changed.
Implementation, real Nix/devenv build, real GitHub auto-merge and real issue
comment/close remain `NOT_TESTED` until their WPs and authorized evidence exist.
