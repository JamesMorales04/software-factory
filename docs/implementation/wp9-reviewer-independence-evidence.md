# WP9 — Reviewer independence and Compose reviewer fleet evidence

**Status:** implementer self-test evidence; **not independent acceptance and not
certification**. Implementation candidate: `d027bf2ea30d11ace72bbb962c70112dfaa81b3a`
on branch `factory/quality-wp9-reviewer-fleet` (baseline `bcb8b5b`).

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
- Office shows required roles, assigned principals and independent PASS-receipt
  count from Core. No "model diversity required" wording exists.

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
15 passed; 0 failed
  includes gate_accepts_distinct_principals_with_the_same_model_and_required_roles,
  gate_rejects_a_second_receipt_from_the_same_principal and
  gate_waits_for_role_coverage_even_when_the_count_is_met

cargo test -p factory-native --bin factory-core
36 passed; 0 failed
  includes quality_receipt_rejects_caller_selected_principal_or_role

cargo test -p factory-native --lib
741 passed; 0 failed; 2 ignored

cargo clippy -p factory-native --all-targets --all-features -- -D warnings
clean
```

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
45 files passed; 299 tests passed

npm run typecheck
clean

npm run lint
clean
```

`docs/product/office-parity.json` parsed as JSON and
`docs/product/capabilities.yaml` parsed as YAML (66 capability entries).

## Changed files

High-level: `native/src/review_roles.rs`, `native/src/reviewer_identity.rs`
(new), `native/src/planning/{mod,route}.rs`, `native/src/agentic.rs`,
`native/src/db/{mod,quality,lead,tests,intake}.rs`,
`native/src/bin/{core,reviewer,compose_init}.rs`,
`migrations/0065_reviewer_role_coverage.sql`, `compose.yaml`,
`office/src/{coreApi.ts,views/Director.tsx,views/Director.test.tsx,i18n/*}`,
`docs/architecture/decisions/ADR-0037-*`, the quality-first status/plan
documents, Office parity, capability registry, roadmap, Compose/Codex runbooks
and `native/README.md`.

## Security and authorization impact

- Core remains the only authority. Reviewer model output still cannot accept
  work, widen scope, grant authority or bypass the Gate.
- Reviewer identity is authenticated from trusted deployment configuration; the
  receipt principal and role are checked, not trusted.
- One token is scoped to one role and one service. Tokens are distinct across
  roles, mounted read-only and redacted from telemetry.
- Independence rejects implementer/verifier collusion and duplicate reviewer
  principals. The same principal cannot count twice.
- The existing Gate policy, candidate binding, finding disposition and
  authorization rules were not relaxed.

## Limitations and remaining work

- `NOT_TESTED`: a real Compose Campaign with two or more reviewer role receipts
  and Gate aggregation. This slice ran deterministic DB/plan/Gate tests, Compose
  config validation and Compose-init provisioning; it did not start the full
  Compose stack.
- `NOT_TESTED`: real browser acceptance of the new Director roles/principals
  display. Component tests and the containerized Office suite passed.
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
