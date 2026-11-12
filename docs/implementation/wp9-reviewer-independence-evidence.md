# WP9 — Reviewer independence and Compose reviewer fleet evidence

**Status:** implementer self-test evidence; **not independent acceptance and not
certification**. Implementation candidate: `f4c8d9e51c9dc92943afd9a9c6a7524b215ecd60`
on branch `factory/quality-wp9-reviewer-fleet` (baseline `bcb8b5b`). This
supersedes `d027bf2`, `de48c3b` and `2514c2b` after independent Codex GPT-6
Luna reviews found P1 (Gate could count roles without a frozen principal
assignment), P2 (Director did not show individual receipts), a P2 where receipts
for roles outside the frozen plan were ignored instead of rejected, and a P2
where a legacy roleless receipt could remain in an accepted set.

## Policy implemented

Reviewer independence is **identity, process, token and authenticated
principal**, not provider/model diversity.

- `validate_reviewer_independence` derives the required count and role set from
  `ReviewerPlan`, requires one unique non-empty principal per required role, and
  rejects implementer/verifier/reviewer-principal collisions.
- Same provider/model with distinct principals passes. Model diversity remains
  only `model_diversity_observation`, which no plan or Gate path consumes.
- Core compiles required reviewer roles into the participant plan instead of
  collapsing to one Functional reviewer.
- Core fails closed with `REVIEWER_INDEPENDENCE_REQUIRED` when the configured
  identities cannot staff every required role. The message names independent
  identities/processes and roles; it does not mention model or provider
  diversity.
- A reviewer receipt records its role. The Gate requires the configured
  reviewer count, distinct principals, every required role covered exactly once,
  and the receipt principal matching the frozen role assignment.
- The Campaign reviewer requirement freezes the exact role-to-principal
  assignment set (`required_assignments_json`). The Gate blocks when a required
  role has no assignment, when the assignment set contains an unexpected role,
  or when the frozen plan is unreadable. A role name alone can never satisfy a
  requirement.
- A reviewer receipt must name a role required by the frozen plan. Core returns
  `QUALITY_ROLE_MISMATCH` for a missing or unrequired role at ingestion, and the
  Gate independently blocks on any historical/foreign roleless or extra-role
  receipt instead of silently ignoring it. A roleless or extra-role receipt can
  never be included in an `ACCEPTED` Gate when required roles exist.
- Office shows required roles, assigned principals, per-role receipt
  result/evidence, missing receipts, a frozen-plan mismatch alert and the
  independent PASS-receipt count from Core. No "model diversity required"
  wording exists.

## Independent-review repair

- **P1 — frozen principal assignments:** the Campaign requirement now stores
  `required_assignments_json`. The Gate rejects an assignment role set that is
  not exactly the required role set, including a missing assignment for any
  required role or an unexpected assignment role. It decides `BLOCKED` with
  “reviewer plan does not bind every required role to an authenticated
  principal”, and does not count receipts by role name alone. This also covers
  an older plan whose roles are readable but assignments are absent/unreadable.
- **P2 — receipts outside the frozen plan:** Core now rejects a missing or
  unrequired reviewer role at receipt ingestion (`QUALITY_ROLE_MISMATCH`), and
  the Gate fails closed if a historical/foreign receipt for an unrequired role
  exists. Added deterministic coverage where all required roles pass and one
  extra-role receipt is present; the Gate is `BLOCKED`, never `ACCEPTED`.
- **P2 — legacy roleless receipts:** when required roles exist, a historical
  receipt with no `reviewer_role` is now invalid rather than skipped. Added a
  direct-insert regression: one Functional receipt, one roleless legacy receipt,
  then the required Code Quality receipt; all required roles pass, but the Gate
  is `BLOCKED` with “reviewer receipt has no role while the frozen plan requires
  role coverage”.
- **P2 — individual receipts in Director:** each required role now renders its
  assigned principal, the matching receipt result and evidence, or an explicit
  “No receipt” state. A failing/changes-requested receipt gets a danger badge.
  A role/assignment set mismatch renders an alert explaining that acceptance is
  blocked until Core records an exact assignment set. Core remains the authority;
  the UI displays only its projection.

## Compose identities and provisioning

`factory-compose-init` now generates one token per reviewer identity and writes
it into both Core's `/config` and that role's own configuration volume:

| Service | Token file | Principal | Role |
|---|---|---|---|
| `reviewer-functional` | `reviewer-functional.token` | `reviewer-functional-process` | `FUNCTIONAL` |
| `reviewer-architecture` | `reviewer-architecture.token` | `reviewer-architecture-process` | `ARCHITECTURE` |
| `reviewer-code-quality` | `reviewer-code-quality.token` | `reviewer-code-quality-process` | `CODE_QUALITY` |
| `reviewer-security` | `reviewer-security.token` | `reviewer-security-process` | `SECURITY` |
| `reviewer-testing` | `reviewer-testing.token` | `reviewer-testing-process` | `TESTING` |
| `reviewer-operability` | `reviewer-operability.token` | `reviewer-operability-process` | `OPERABILITY` |

Each service has its own state volume, `FACTORY_REVIEWER_PRINCIPAL`,
`FACTORY_REVIEWER_ROLE` and read-only token mount. Core reads
`/config/reviewer-identities.json` through
`FACTORY_REVIEWER_IDENTITIES_FILE`; the JSON contains principals, roles and
token-file paths, never token values. A caller cannot select its principal: Core
authenticates the bearer token, derives the principal/role, and rejects a
receipt whose `principal_id` or `reviewerRole` differs
(`QUALITY_PRINCIPAL_MISMATCH` / `QUALITY_ROLE_MISMATCH`). Tokens are registered
with telemetry redaction and are not printed by init, logs, UI or telemetry.

## Verification run

From `native/`:

```text
cargo fmt --all -- --check
FMT_OK

cargo test -p factory-native --lib review_roles
9 passed; 0 failed

cargo test -p factory-native --lib planning
45 passed; 0 failed

cargo test -p factory-native --lib db::tests::gate_
8 passed; 0 failed
  includes gate_accepts_distinct_principals_with_same_model_and_exact_role_assignments,
  gate_blocks_required_roles_without_frozen_principal_assignments,
  gate_blocks_an_extra_role_receipt_even_when_required_roles_pass,
  gate_blocks_a_roleless_legacy_receipt_when_required_roles_exist,
  gate_rejects_an_assignment_set_with_an_unexpected_role and
  gate_rejects_a_second_receipt_from_the_same_principal

cargo test -p factory-native --lib gate_
18 passed; 0 failed
  (broader substring filter; not the exact db::tests::gate_ command above)

cargo test -p factory-native --bin factory-core
36 passed; 0 failed
  includes quality_receipt_rejects_caller_selected_principal_or_role

cargo test -p factory-native --lib
745 passed; 0 failed; 2 ignored

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

The earlier evidence reported 17 for `db::tests::gate_`; that was actually the
broader `gate_` substring filter. This version records the exact
`db::tests::gate_` command and count (8) plus the broader `gate_` count (18), so
the filter and result are unambiguous.

From the repository root:

```text
podman compose config
exit 0; reviewer-functional/architecture/code-quality/security/testing/operability
and factory-reviewer-*-state/config volumes rendered.

factory-compose-init against temporary per-role config directories
"Factory role configuration material is ready; no secrets were printed."
6 distinct reviewer tokens generated; each service copy matched Core's copy;
reviewer-identities.json wrote 6 principal/role/tokenFile entries;
token and identity files mode 0640; no token-shaped value in stdout.
```

Office checks ran in the pinned Node 22.23.2 container image with the repository
mounted:

```text
npm test -- --run
45 files passed; 301 tests passed

npm run typecheck
clean

npm run lint
clean
```

The first full Office run after the P2 change reported one failure in
`Home.test.tsx` (`Keep existing clients working.`); an immediate rerun of the
same suite passed 301/301. The failure was not reproduced and is not claimed as
fixed. The Director suite itself passed 12/12 on both runs. The new component
cases cover all roles covered, a required role with no receipt, a
changes-requested receipt with evidence, and unassigned/frozen-plan mismatch.

`docs/product/office-parity.json` parsed as JSON and
`docs/product/capabilities.yaml` parsed as YAML (66 capability entries).

## Changed files

High-level: `native/src/review_roles.rs`, `native/src/reviewer_identity.rs`
(new), `native/src/planning/{mod,route}.rs`, `native/src/agentic.rs`,
`native/src/db/{mod,quality,lead,tests,intake}.rs`,
`native/src/bin/{core,reviewer,compose_init}.rs`,
`migrations/0065_reviewer_role_coverage.sql`,
`migrations/0066_reviewer_role_assignments.sql`, `compose.yaml`,
`office/src/{coreApi.ts,views/Director.tsx,views/Director.test.tsx,i18n/*}`,
`docs/architecture/decisions/ADR-0037-*`, the quality-first status/plan
documents, Office parity, capability registry, roadmap, Compose/Codex runbooks
and `native/README.md`.

The first repair commit changed `native/src/db/{quality,lead,tests}.rs`,
`migrations/0066_reviewer_role_assignments.sql`,
`office/src/views/{Director.tsx,Director.test.tsx}`,
`office/src/i18n/catalog.{en,es}.ts`, the quality-first status document,
capability registry and Office parity. The next repair commit added the
extra-role ingestion/Gate rejection in `native/src/{bin/core.rs,db/quality.rs}`
with tests in `native/src/db/tests.rs`. The current repair commit makes legacy
roleless receipts invalid in `native/src/db/quality.rs` and adds the
direct-insert regression in `native/src/db/tests.rs`.

## Security and authorization impact

- Core remains the only authority. Reviewer model output still cannot accept
  work, widen scope, grant authority or bypass the Gate.
- Reviewer identity is authenticated from trusted deployment configuration; the
  receipt principal and role are checked, not trusted.
- One token is scoped to one role and one service. Tokens are distinct across
  roles, mounted read-only and redacted from telemetry.
- Independence rejects implementer/verifier collusion and duplicate reviewer
  principals. The same principal cannot count twice.
- A required role without an exact frozen principal assignment cannot satisfy
  the Gate, so a missing or unreadable assignment set fails closed rather than
  degrading to role-name counting.
- The existing Gate policy, candidate binding, finding disposition and
  authorization rules were not relaxed.

## Limitations and remaining work

- `NOT_TESTED`: a real Compose Campaign with two or more reviewer role receipts
  and Gate aggregation. This slice ran deterministic DB/plan/Gate tests, Compose
  config validation and Compose-init provisioning; it did not start the full
  Compose stack.
- `NOT_TESTED`: real browser acceptance of the Director roles/principals and
  per-role receipt display. Component tests and the containerized Office suite
  passed.
- `NOT_TESTED`: live paid model inference or Codex App Server review; none was
  run.
- `BLOCKED` for independent acceptance: an independent verifier/reviewer and
  Product Owner visual acceptance have not run on this candidate.
- Integration note: `compose.yaml` no longer defines a single `reviewer`
  service. Any external override or operator command that targets `reviewer`
  must target the six role services (or all services). Reviewer HTTP receipts
  for a planned candidate must include `reviewerRole`; Core rejects a
  principal/role mismatch.
- The legacy single-token host harness (`FACTORY_REVIEWER_TOKEN` plus
  `FACTORY_REVIEWER_PRINCIPAL`) still exists for local diagnosis but maps to one
  Functional identity only; it cannot satisfy a multi-role plan. The Compose
  fleet is the supported multi-reviewer deployment.
- No isolation, deployment certification or `TRUSTED_LOCAL` certification is
  claimed.
