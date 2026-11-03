# Wave 3 resumption checkpoint — 2026-11-02

**Campaign branch:** `campaign/office-ux-wave3-20261028`
**Remote campaign HEAD at resumption:** `f7479c0c5f936d1097426dd1293ef43ea6e72fb6`
**Final campaign HEAD (stable baseline):** `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e`
**Remote `main` (unchanged):** `d58725266d132fafb96a79970ff292acc876feb8`
**Integration worktree:** `/home/james/Escritorio/Development/sf-wave3/integration`
**No `main` merge, branch-protection change, paid inference, or access-stack
promotion occurred.**

## Final stable Wave 3 baseline (2026-11-02)

**Exact code baseline:** `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e`
(the campaign head after merging PR #83). `main` remains
`d58725266d132fafb96a79970ff292acc876feb8`.

- **PR #80** merged at `4f732ffb2173561945194508b2f796bad56a1e5f` from head
  `01cc72b78a61b98cccff229afdf4f2a84ca4f2eb`; GPT-6 Luna exact-head
  **APPROVE**. Required CI run `41537700176`: architecture-consistency,
  compose-policy, rust and container-dev all PASS.
- **PR #83** merged at `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e` from head
  `5b07e6215cf66a1c9f0427b1d7152b7beda8d3ae`; GPT-6 Luna exact-head
  **APPROVE**. Required CI run `41571994723`: architecture-consistency,
  compose-policy, rust and container-dev all PASS. The container lane ran the
  canonical Office and unified-intake Chromium journeys at this exact head:
  Office candidate `a19f51bda492f374c81f7987ec51da9d2fe6abd4` with gate
  `ACCEPTED`; intake request `77b355fe-2dda-4b52-b99d-960f99ec22a5`, Campaign
  `9e82d45f-163c-4508-a86b-a57ca2e98a72`, mode `DETERMINISTIC_FAKE_MODEL`.
- A post-merge rerun of PR #83 at `5b07e62` (run `41573710950`) also passed
  all jobs. One post-merge PR #80 rerun at `01cc72b` hit a SQLite `Busy` flake
  in `db::provider_sessions::tests::execution_check_receipts_bind_the_frozen_command_and_candidate`;
  the required run on that head passed, and the final head passed twice. This
  remains test-reliability debt, not a product assertion failure.
- **PR #81** was closed as explicitly superseded; its CAP-050/CAP-052
  correction is carried by PR #80.
- **Local evidence at `fe0eeba`** (parent of the formatting-only `5b07e62`):
  fmt PASS; Clippy all features PASS; library 512 passed / 2 ignored; Core API
  33 with `test-support`, 30 without; Worker 13; CAP-014 2; fixture-dispatch
  12; production build without `test-support` PASS; strict repository hygiene
  PASS. `cargo xtask e2e office` PASS (candidate `3820eb82…`, gate
  `ACCEPTED`); `cargo xtask e2e intake` PASS; `cargo xtask e2e intake-full`
  PASS (candidate `48881581…`, gate `ACCEPTED`, MODEL reviewer).
- **Funding posture:** production dispatch remains fail-closed. The
  debug-only `test-support` feature carries exact-scope admissions for
  deterministic tests and the intake E2E; it is rejected in non-debug builds,
  is absent from production binaries, and cannot override a persisted
  subscription policy or quota pause. No provider entitlement, paid fallback,
  Extra Usage, automatic top-up or live inference was enabled.
- **NOT_TESTED:** live provider inference, verified subscription
  entitlement/no-overage proof, real quota recovery, atomic pre-Project request
  admission, ISOLATED execution, deployment, and Owner visual sign-off. No paid
  inference occurred.
- **Deferred, not part of this baseline:** PR #82 visual V3, PR #79 provider
  preparation and the access stack. The Infinity Runtime comparison must start
  from the reviewed `25e3478` integration commit in a separate worktree.

## Recovered prior Director work

While resuming, the campaign branch advanced from the previously observed
`f7479c0` to `bb22524` through recovered prior work:

- `ec246df` — deterministic duplicate-participant winner shared by the Agent
  Office floor and inspector, with unit/integration coverage.
- `17aa3e9` — V2 evidence rebound to that repair.
- `f2322fc` — merge of the V2 inspector repair into the campaign branch.
- `bb22524` — earlier checkpoint and proposed funding contract.

These were preserved rather than discarded. The recovered inspector fix passes
Office typecheck, Biome, 265 Vitest tests and production build on the integration
worktree. The earlier checkpoint is now marked superseded because it predates the
PR #83 foundation. Remote `main` is unchanged.

A separate recovered access-repair branch is at
`factory/access-repairs/20261029` = `603f823`, with two prior commits
(`56aeb58`, `4387040`) and a docs-only evidence addendum. This is newer than the
Luna-reviewed `ad04aca`; it has **not** been exact-head re-reviewed and was not
promoted. It remains local/unpushed.

## Subscription routes actually observed

- OpenCode Go: available. The account usage report showed the Go rolling window at 93 % remaining,
  weekly at 97 %, monthly at 29 % remaining. This proves quota availability, not
  that Extra Usage is disabled.
- Codex: ChatGPT-authenticated (`codex login status`: logged in using ChatGPT),
  Plus plan; 66 % weekly remaining. Credits balance is zero; no API-key route was
  used.
- No separate positive-USD test authorization was created. Real provider UAT
  remains blocked pending no-overage proof and atomic request admission.

## Historical open-PR snapshot at resumption (superseded)

| PR | Branch | GitHub head | State |
|---|---|---|---|
| #79 | `factory/provider-acceptance/20261029` | `73664882316acfc18928ab697ec04da435187dd8` | harness prepared; Core subscription admission still missing |
| #80 | `factory/office-conversation/20261029` | `f310192c33a02d9191fd9cfbcaf90264c2c6e8cf` | Home implementation; local diagnostic commit `1a2c0ab` not yet pushed |
| #81 | `factory/capability-evidence-shape/20261029` | `13adff74a8de434fd46c527ea92608b695a98d03` | CAP-050/CAP-052 correction; local fixture-Worker fix `3b0c983` not yet pushed |
| #82 | `factory/office-isometric-v3/20261029` | `95346734a4c3d99384388f84c15c7cb2996e2efa` | V3 visual; exact-head review approved; browser acceptance blocked by fixture Worker and Office harness foundations |
| #83 | `factory/subscription-funding/20261029` | `c0eb767d84d774362c7bed27fc5a7834fbbc2436` | fail-closed funding gate; full Rust suite red |

## Historical implementation ownership (superseded)

| Lane | Worktree | Writer | State |
|---|---|---|---|
| Funding/Rust | `sf-wave3/funding-repair` | DeepSeek implementer A | repairing test-only non-billable funding fixture and RECOVERABLE schema contradiction |
| Home/Office | `sf-wave3/home-repair` | DeepSeek implementer B | `cargo xtask e2e office` passes at `1a2c0ab`; repairing `cargo xtask e2e intake` |
| PR81 evidence/Worker | `sf-wave3/capability-evidence-shape` | Director | full Rust suite PASS at `3b0c983`; Compose Office journey blocked by base Office contract |
| PR38 security review | `sf-wave3/access-luna-ad04aca` | GPT-6 Luna | independent exact-SHA review running at `ad04aca` |
| Integration | `sf-wave3/integration` | Director | local checkpoint commits only |

## Verified regressions and root causes

1. **CAP-050/CAP-052 governance failure.** Existing README evidence entries are
   invalid for `deterministic: verified`; PR #81 removes those two entries and
   passes `factory-architecture-check` locally.
2. **Fixture Worker exits 1 on PR #82/#83-style branches.** The pre-fix
   `Database::load_assignment` rejected empty provider route identity for
   `FIXTURE` assignments. The fix, present in PR #80 as `3d39465` and locally in
   PR #81 as `3b0c983`, scopes route identity validation to
   `TRUSTED_LOCAL_PROVIDER` and adds positive/negative coverage.
3. **Office/Lead base harness mismatch.** The campaign base UI expects direct
   `StandingDelegation[]` while Core returns Delegation projections, and the old
   Lead journey creates a V1 delegation that cannot authorize the current request
   path. PR #80 repairs this and its Office journey passes locally.
4. **PR #80 Office-impact parser failure.** The declaration used
   `/actions/{id}` and the non-path `Office Vitest suite`. The checker is correct;
   the declaration was corrected to `/actions/:actionId`, `/requests/:requestId`
   and real test paths.
5. **PR #83 funding regression.** Nine ledger/gateway/routing tests are blocked
   by `SubscriptionFunding(BindingRequired)`. Production must stay fail-closed;
   the repair uses a `#[cfg(test)]` per-scope non-billable admission fixture and
   keeps persisted proposals/quota pauses authoritative.
6. **RECOVERABLE schema contradiction.** Migration 0041 cannot represent the
   decoder's required reason for `RECOVERABLE`. Repair is in progress with a
   forward migration/test; quota recovery remains unavailable.

## Historical candidate reviews before final merge (superseded)

- PR #80 final candidate `d2dd02ccff6a4566e1178d992ab42fb89e1a85c4`
  (`base 36079cf`): GPT-6 Luna exact-head review **APPROVE**. The
  candidate adds Home/Lead V2 behavior, the FIXTURE Worker admission fix, the
  current intake journey contract and the CAP evidence correction. Final local
  canonical rootless Compose run passed both `cargo xtask e2e office`
  (candidate `d148cbb3…`, Gate ACCEPTED) and `cargo xtask e2e intake`
  (request `13b3d5a4…`, Campaign `cb7f912e…`, Gate ACCEPTED). Office
  typecheck, Biome, 276 Vitest tests and build pass.
- PR #83 final candidate `75d3894b30d5cca8d39bc749cc841454adfcd120`
  (`base 36079cf`): GPT-6 Luna approved the funding implementation at
  `da462d2` and is reviewing the delta. It adds the fail-closed V3 funding
  gate, migration 0042, the debug-only `test-support` admission seam and
  explicit test admissions for the positive CAP-014/provider fixtures.
  `cargo test --workspace --locked --features test-support` passes on the host
  excluding only the two environment-dependent OpenCode sandbox tests;
  509 library tests, 32 Core API tests, 12 fixture-dispatch tests and the
  CAP-014 tests pass. Production `cargo build --bins` without the feature
  passes. Real provider entitlement, no-overage proof, request admission and
  quota recovery remain NOT_IMPLEMENTED/NOT_TESTED.

Both candidates remain unmerged pending fresh required-check results on their
exact heads. PR #81 is superseded by the CAP correction carried in both
candidates.

## Tests run at this checkpoint

- PR #81 candidate `3b0c983`:
  - `cargo test -p factory-native --lib --locked`: PASS, 490 passed, 2 ignored.
  - `cargo clippy --locked -p factory-native --lib --bins --all-features -- -D warnings`: PASS.
  - `cargo fmt --all -- --check`: PASS.
  - `factory-architecture-check --base-ref f7479c0`: PASS.
  - Host/Compose `cargo xtask e2e office`: FAIL before Agent Office because the
    campaign base Office/Lead contract is older than PR #80. Not a PR #81 code
    regression.
- PR #82 candidate `9534673`:
  - typecheck, Biome, 268 Vitest tests, production build: PASS.
  - Exact-head independent review was previously approved; browser acceptance
    remains blocked by the shared fixture/Office harness foundation.
- PR #80 candidate `1a2c0ab` (implementer self-test):
  - Office typecheck/lint/272 tests/build: PASS.
  - `cargo xtask e2e office`: PASS.
  - `cargo xtask e2e intake`: FAIL; local repair in progress for temp-directory
    ownership and current profile/campaign wire contracts.

## Additional exact-candidate checks

- `factory/wave3-foundation/20261102` = PR #80 head `1a2c0ab` + CAP evidence
  correction `13adff7` = `79b73e9`.
  - `factory-architecture-check --base-ref f7479c0`: PASS.
  - Rust lib: 490 passed, 2 ignored.
  - `cargo fmt --all -- --check`: PASS.
  - strict Clippy for lib/bins: PASS.
  - Real Chromium `cargo xtask e2e office` in the canonical environment:
    PASS, candidate `b4ac518ceb9c976a484c5caf15c42729be4ee897`, gate
    `ACCEPTED`. The intake journey is still being repaired.
- PR #38 exact head `ad04aca`: GPT-6 Luna independent review returned
  **APPROVE WITH CONDITIONS**, no HIGH/BLOCKER finding. Conditions are:
  authenticate Serve over the new Unix-socket backend at this exact candidate;
  align deployment docs that cite older TCP-Serve evidence; add a close/reopen
  restart test for revocation/pairing authority. Director-run Rust lib suite:
  271 passed, 2 ignored; focused pairing/revocation suite: 11 passed.
  Compose/browser, Serve and mobile-data remain NOT_TESTED.

## Sol architecture decision — subscription admission

GPT-6.1 Sol reviewed `c0eb767` plus the in-progress repair and PR #79's harness.
Decision: keep real dispatch **BLOCKED** until a legitimate provider-issued
no-overage trust anchor exists. An Owner-signed Console observation remains
`OWNER_ATTESTED` with `core_verifiable: false`; hashing, importing,
countersigning or re-exporting it cannot promote its trust. Real admission
requires one atomic claim binding principal/authority, objective,
repository/base, config/hash, exact profile/policy acknowledgement, assessment
authority, technical limits, provider/model/account/authentication/credential/
settings bindings, funding-evidence IDs/digests/trust-anchor revisions, and
idempotency, with rollback of admission-created resources and no inference in
the transaction.

Minimum contracts are a read-only authenticated
`GET /funding/subscription/{provider}` (truthfully `core_verifiable:false`,
`admissible:false` today), a side-effect-free `POST /intake/admissions/check`,
and an atomic `POST /intake/admissions`; PR #79 must consume the admission
receipt rather than sequentially creating resources. Unknown quota is not
admissible. Ambiguous provider errors are `PAUSED_PROVIDER_UNKNOWN`, never
verified exhaustion. A reset time or manual observation never resumes work.
Recovery requires a future reviewed contract with fresh authority and no replay
of ambiguous requests. No route may be enabled until the negative invariants
(evidence provenance, exact bindings, one-winner claims, crash/restart,
revocation and dispatch rechecks) pass.

## Historical pre-merge checkpoint (2026-11-02 22:30 UTC-05, superseded)

- **PR #80 final candidate:** `d2dd02ccff6a4566e1178d992ab42fb89e1a85c4`
  against base `36079cf`. GPT-6 Luna exact-head review: **APPROVE** after two
  MEDIUM findings were repaired (expired delegation filtering/display and
  recent-request poll fencing). Office typecheck, Biome, 276 Vitest tests and
  production build pass; canonical rootless Compose `e2e office` and `e2e
  intake` pass at this head, including 14 responsive Home captures and the
  deterministic intake lifecycle. Local/Fixture evidence only; live provider
  and Owner visual acceptance remain NOT_TESTED.
- **PR #83 final candidate:** `75d3894b30d5cca8d39bc749cc841454adfcd120`
  against base `36079cf`. GPT-6 Luna approved `da462d2` and the earlier
  funding implementation; the remaining delta is test-only scoped admissions
  for three positive provider integration tests. Host `cargo test --workspace
  --locked --features test-support` passed excluding only the two
  environment-dependent OpenCode sandbox tests; all 12 fixture-dispatch tests
  pass. CI is pending. Production remains fail-closed.
- **PR #80/#83 CI:** PR #80 `architecture-consistency` and `compose-policy`
  PASS; `rust` and `container-dev` in progress. PR #83 architecture and
  compose-policy pass; `rust` and `container-dev` in progress. No merge has
  occurred yet.

## Post-baseline next tasks

1. Use `25e3478` as the exact reviewed Factory integration commit for the
   separately authorized Infinity Runtime comparison. Do not migrate inside the
   Wave 3 worktrees.
2. Keep real provider dispatch blocked until a legitimate verified subscription
   entitlement/no-overage trust anchor and atomic request admission exist.
3. Keep PR #82 visual V3, PR #79 provider preparation and the access stack
   deferred unless separately authorized.
4. Do not claim deployment, ISOLATED execution or Owner visual acceptance from
   this baseline.
