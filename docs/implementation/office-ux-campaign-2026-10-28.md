# Factory Office UX campaign — execution ledger (2026-10-28)

**Campaign:** [issue #39](https://github.com/JamesMorales04/software-factory/issues/39)
**Status:** ACTIVE. This ledger records planned and observed state; it is not
acceptance evidence.
**Baseline:** `campaign/office-ux-baseline-20261028`.

## 1. Entry gate

| Item | Recorded state |
|---|---|
| Authoritative `main` | `d58725266d132fafb96a79970ff292acc876feb8` |
| PR #36 | `bda12f0`, draft, checks pass, no independent review, unmerged |
| PR #37 | `715349f`, draft, checks pass, no independent review, unmerged |
| PR #38 | `c8bf930`, draft, checks pass, no independent review, unmerged |
| Integration baseline | `c8bf930` on `campaign/office-ux-baseline-20261028` |
| Main contains access features? | **No.** The campaign does not claim otherwise. |
| Protected-branch changes | None. The three PR branches are unchanged. |
| Merge decision | Pending Product Owner authorization and independent review |

New PRs target the integration baseline, not `main`, until the access stack is
integrated. Retargeting requires rebase/revalidation against the then-current
`main`.

## 2. Runtime and coordination

| Item | Observed |
|---|---|
| OpenCode | 2.2.0; `opencode-go/deepseek-v4.1-flash` verified with live prompt `MODEL_OK` |
| Codex | 0.165.0, ChatGPT auth; `gpt-6.1-sol`, `gpt-6-luna` present |
| GitHub | `gh` authenticated as `JamesMorales04` with `repo`/`workflow` scopes |
| Host | 16 CPU, 31 GiB RAM, rootless Podman 6.1.4 |
| Worktree root | `/home/james/Escritorio/Development/sf-office-ux` |
| Resource rule | At most 3 concurrent Rust builds / 2 browser suites; heavy Compose gates serialized |

## 3. Workstream DAG

```
profiles (C) ──> intake (B) ──> IA request workspace (A)
     │                 │
     └──> assessment routing (E)
files (K) ─────────────────────────────> docs workspace (K2)
i18n infra (J) ──> full translation sweep (J2)
projects/GitHub (F/G) ──> issues sync (L) ──> autonomous intake (M)
campaign diff (I) ──> PR operations (I2)
decisions (H) ──> delivery authorization (H2)
QA (N) runs alongside every lane
```

`A` may implement the Home/request UI against the contract in
[office-ux-contracts-2026-10-28.md](office-ux-contracts-2026-10-28.md) while `B`
implements Core. Integration is gated on the contract, not on a verbal summary.

## 4. Lane map and initial assignments

| Lane | Branch | Worktree | Owner runtime | Issue | State |
|---|---|---|---|---|---|
| Baseline/integration | `campaign/office-ux-baseline-20261028` | `sf-office-ux/baseline` | Director | #39 | Active |
| C profiles | `campaign/office-ux-profiles-20261028` | `sf-office-ux/profiles` | OpenCode | #40 | Assigned |
| A IA/Home | `campaign/office-ux-ia-20261028` | `sf-office-ux/ia` | OpenCode | #41 | Assigned |
| B intake | `campaign/office-ux-intake-20261028` | `sf-office-ux/intake` | OpenCode | #42 | Blocked on core-intake |
| J i18n | `campaign/office-ux-i18n-20261028` | `sf-office-ux/i18n` | OpenCode | #43 | Assigned |
| K files | `campaign/office-ux-files-20261028` | `sf-office-ux/files` | OpenCode | #44 | Assigned |
| Core intake foundation | `campaign/office-ux-core-intake-20261028` | `sf-office-ux/core-intake` | OpenCode | #42 | Assigned |
| L issues | `campaign/office-ux-issues-20261028` | `sf-office-ux/issues` | OpenCode | #50 | Assigned |
| QA | continuous | `sf-office-ux/qa` | OpenCode/Codex | #39 | Prepared |

## 5. Shared-file ownership

The integration owner (Campaign Director) owns final resolution of:

- `native/src/bin/core.rs` route registration and App state;
- `native/src/lib.rs` and `native/src/db/mod.rs` module registration;
- migration ordering and numbering;
- `office/src/coreApi.ts` client surface, `office/src/App.tsx`,
  `office/src/navigation.ts`, shared CSS entrypoints;
- `docs/product/office-parity.json`, capability registry and roadmap updates.

Lane writers must not rewrite another lane's worktree or force-push a shared
branch. Each lane commits small, reviewable changes and reports exact tests.

## 6. Architecture decisions frozen for this wave

1. **No second lifecycle.** Intake derives state from Lead request, Campaign and
   Quality Gate records. It stores only operator intent, messages and a
   cancellation flag.
2. **No second authorization path.** Approval composes the existing Lead start /
   Campaign authorization operations. Profile values are frozen into the
   authorization; the model never supplies them.
3. **Profiles are configuration, not grants.** Role routes must be executable
   against configured providers; unsupported combinations fail closed.
4. **Deterministic Verifier stays outside profiles.** Only model-backed roles
   receive model assignments.
5. **No silent fallback.** A missing profile route, provider credential or model
   gateway is an explicit typed unavailable state.
6. **Empty Action spec is allowed at intake.** The Lead proposal supplies the
   concrete plan and checks; the existing Lead/Campaign validation remains the
   authority.
7. **No compatible shims.** New endpoints and types are used directly by the new
   UI. Existing legacy views remain reachable but are not duplicated.

## 7. Acceptance matrix

Result vocabulary: `NOT_TESTED`, `FAIL`, `BLOCKED`, `UNKNOWN`, `DISCONNECTED`,
`PASS` (for the exact method/candidate), `ACCEPTED` only for a Core Quality Gate
decision.

| # | Journey | Method planned | Candidate | Result |
|---|---|---|---|---|
| 1 | First use: owner bootstrap, provider config, project registration | Compose + Chromium (candidate-bound) | — | NOT_TESTED |
| 2 | Private GitHub import | Authorized connector + Chromium | — | NOT_TESTED |
| 3 | Simple request without manual Actions/delegations | Core integration + Chromium, deterministic fake model | `040e6be` | PARTIAL — request/policy via Home UI PASS; provider/profile setup via API; not independent |
| 4 | Assessment, plan, authorization, execution | Core integration + deterministic fake model | `040e6be` | PARTIAL — assessment/freeze/operator approval/Campaign start PASS; execution/verification NOT_TESTED |
| 5 | Monitoring of tasks/agents/candidate/evidence | Browser against real Core | — | NOT_TESTED |
| 6 | Decision Inbox intervention | Browser + Core operation | — | NOT_TESTED |
| 7 | Review diff, PR, checks, authorized delivery | Browser + GitHub sandbox | — | NOT_TESTED |
| 8 | Documentation browse/render; governed edit | Browser + repository projection | — | NOT_TESTED |
| 9 | GitHub issue sync and campaign link | Mock server + authorized repo | — | NOT_TESTED |
| 10 | Bounded autonomous intake; adversarial rejection | Integration/negative tests | — | NOT_TESTED |
| 11 | EN/ES switch preserving context | Browser + component tests | — | NOT_TESTED |
| 12 | Mobile 390/320 essential journeys incl. QR pairing | Chromium viewports | — | NOT_TESTED |
| 13 | Restart/recreate persistence | Compose restart | — | NOT_TESTED |

Rows may only move to `PASS` with exact method, candidate SHA and observed
output recorded in an evidence file. A component test is not journey acceptance.

### Author-run regression evidence (not journey acceptance)

At implementation `260851a` the author ran the updated Chromium journey against
host Rust binaries + Vite with fixture assessment/provider. It exited 0 and
recorded candidate `18faf441…` with gate `ACCEPTED`. This supports the integrated
shell/navigation/localisation/legacy governed path, not the unified-intake
journeys. Full limitations (no Compose, no live provider, no independent
verifier, intentional pending-intake 404) are recorded in
[office-ux-integration-2026-10-28.md](../evidence/office-ux-integration-2026-10-28.md).
No acceptance-matrix row is moved to `PASS` by this author-run evidence.

## 8. Verification queue

1. Lane-local: `cargo fmt --check`, targeted `cargo test`, `cargo clippy -D warnings`
   where the lane touches Rust; `npm --prefix office run typecheck|lint|test|build`.
2. Integration candidate: full `cargo test --workspace`, architecture checker,
   repository hygiene, Office build/tests, migration from a clean database.
3. Browser/Compose: `cargo xtask e2e office`, then the campaign journeys at
   1440/834/390/320. Compose E2E is serialized.
4. Independent review: Codex `gpt-6.1-sol` for architecture/authorization and
   `gpt-6-luna` for the final diff. Reviewer must inspect the actual diff.
5. Protected merge: Product Owner authorization; never self-merge the campaign.

## 9. Resume state

If a lane stops, preserve its worktree and branch. Record the last commit, the
exact failing command and the next executable task here before reassigning.
Do not overwrite uncommitted work. Do not claim that a stopped agent is still
running.

## 10. Change log

- 2026-10-28: Preflight recorded; baseline created at `c8bf930`; contracts
  frozen; profiles, IA, i18n, files and GitHub Issues lanes assigned; intake
  blocked on the profiles foundation. No product feature is claimed complete.
- 2026-10-28: Independent Codex `gpt-6.1-sol` review at `9f341db` found eight
  contract/authority blockers. Intake launch paused; a second bounded Codex
  design task covers explicit-approval start, draft-to-frozen spec lineage,
  profile-binding, budget intersection, cancellation fencing and atomic replay.
  The review is evidence of review, not implementation approval. Findings are
  recorded in `office-ux-contracts-2026-10-28.md` §3.5.
- 2026-10-28: Design recorded as **ADR-0029 (Proposed)**. Core foundation lane
  `campaign/office-ux-core-intake-20261028` implements draft-to-executable spec,
  request-scoped operator start, start evidence and withdrawal fencing. Intake
  remains blocked until that foundation is reviewed and merged into the campaign
  baseline.
- 2026-10-28: Profiles, project files, EN/ES localization, operator-intent Home
  and GitHub Issues were integrated on `campaign/office-ux-integration-20261028`
  with the protected Core foundation. `cargo fmt`, workspace clippy, Office
  typecheck/lint/121 tests/build and fresh migrations 1–28 pass. The updated real
  Chromium legacy governed journey passed at implementation `260851a` (candidate
  `18faf441…`, gate `ACCEPTED`). A Codex `gpt-6.1-sol` review then returned
  **NOT_READY** on two Core authority gaps and one GitHub credential-generation
  race; repairs are in progress and intake remains blocked on them.
- 2026-10-28: Core withdrawal/model-call fencing and Lead WorkGraph lineage
  repairs merged; targeted tests and the Chromium journey rerun passed at
  implementation `ae95af1` (candidate `7987095…`, gate `ACCEPTED`). GitHub
  credential-generation, token-lifetime, ETag and rate-limit repairs remain in
  progress; intake remains blocked on the final review.
- 2026-10-28: Real Chromium at implementation `7360c05` additionally verified
  Home reflow/composer at 1440/834/390/320 px and an EN→ES→EN Settings language
  switch (candidate `a7b689d…`, gate `ACCEPTED`). This is partial journey 11/12
  evidence only: the new intake is absent from that run, the shell/Home are not
  fully translated and QR pairing was not exercised.
- 2026-10-28: Unified intake browser journey at integration implementation
  `040e6be` passed with a deterministic local fake model gateway: Home policy
  setup, durable request, assessment/freeze, exact plan-hash operator approval
  with `FACTORY_ENABLE_AUTONOMOUS_START=0`, and Campaign authorization
  (`EXECUTING`; profile revision and plan hash frozen). Request
  `6731e4e8…`, Campaign `5b0d4ed4…`, candidate/evidence in
  [office-ux-intake-2026-10-28.md](../evidence/office-ux-intake-2026-10-28.md).
  This is not live model inference, execution completion or independent
  acceptance; journeys 3 and 4 are recorded as PARTIAL. The journey is now
  `cargo xtask e2e intake` and runs in the containerized Chromium CI step; a
  second author run at `228db45` passed (request `1d3aaf1e…`, Campaign
  `40412266…`).
