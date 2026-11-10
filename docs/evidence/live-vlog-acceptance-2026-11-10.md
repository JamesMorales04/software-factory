# Live vlog CLI acceptance — 2026-11-10

**Status:** Implementer/orchestrator live evidence. **This is not independent
verification or certification of the Factory.**

**Scope:** One real R0 `TRUSTED_LOCAL_PROVIDER` Campaign on an external GitHub
project, exercised through Factory Office, Core, the containerized Supervisor and
Worker, the OpenCode Go provider route, independent verifier/reviewer processes
and the deterministic Quality Gate.

**Candidate:** `be5eb14f-1742-4c86-922e-11c9dda5bb3c` at commit
`ff1c0c8ced8a67d38cf286b342fd14432c7b6002`, tree
`c4afcab70427889402081f584700dfa1b6c41367`.

**Gate:** `ACCEPTED` at `2026-11-10T16:51:03Z` (11:51:03 America/Bogota,
UTC−5).

The final banner verification ran against code candidate `7960123`
(`merge: hide stale repair authorization on accepted gate`), parent `543bb50`;
`fc2b957` is included. The original Campaign ran before the banner fix, and its
exact Factory runtime image digest/commit was not recorded. The docs merge is a
later commit on `docs/live-vlog-evidence`, not a code candidate. No code,
Compose, migration, test or Gate-policy change is included in this docs
delivery. The live run used the operator's running Compose stack and the merged
fixes listed in [§9](#9-live-fixes-recorded). No paid inference was used to
produce this document.

## 1. Non-claims

This record does **not** establish any of the following:

- independent acceptance of the Factory itself, its evidence or its Gate;
- isolation or certification of `TRUSTED_LOCAL` execution;
- production readiness, deployment, Windows support or a security review;
- correctness of any provider/model beyond the recorded run;
- acceptance of a future candidate or a different base commit;
- that the target project's GitHub issues were synchronized, linked to the
  Campaign, or closed;
- that the candidate was pushed, merged or opened as a pull request;
- that every Office control, state or locale was exercised.

The target candidate passed the frozen checks, one verifier and five required
reviewers on this run. That is candidate-level evidence, not a Factory
certification.

## 2. Environment and authority

| Item | Recorded value |
|---|---|
| Product/code baseline for this document | `7960123` (`merge: hide stale repair authorization on accepted gate`), parent `543bb50`, includes `fc2b957` |
| Docs merge | later commit on `docs/live-vlog-evidence` (not a code candidate) |
| Runtime | Rootless Podman Compose (`podman compose`) |
| Core / Office | `http://127.0.0.1:8893`, Core health `{"healthy":true,"version":"0.1.0"}` |
| Execution mode | `TRUSTED_LOCAL_PROVIDER` (not isolated) |
| Risk class | `R0` |
| Provider / model | `opencode-go` / `deepseek-v4.1-flash` |
| Execution profile | `Wave 3 Minimal Acceptance`, revision 28, hash `36b4b112…` |
| Subscription admission | `07ca5ee4-0369-4399-b7b6-dedbb0daf171`; Extra Usage `DISABLED`; owner-confirmed |
| Operator principal | `device:296a22f3-a5e0-4f13-b2eb-cc8d349d5ee7` (`Live Playwright owner`) |
| Timezone | `America/Bogota` (UTC−5); durable timestamps below are UTC |
| Paid inference for this document | none |

## 3. Exact identities

| Object | Identifier |
|---|---|
| Project | `7222b42f-db02-4705-9c5f-2824c7602a5b` (`vlog-cli-factory-live-test`) |
| Repository | `https://github.com/JamesMorales04/vlog-cli-factory-live-test` |
| Project base commit | `4c23cbebd6ce48c4babfed7137aebf68e68cb8a0` (`4c23cbebd6ce`) |
| Intake request | `037165ad-bc3f-4b5e-b8bc-db9f59338f83` |
| Lead request | `5a3865a0-070e-4f26-8ca1-f907490ba50d` |
| Campaign | `d61a09f9-1564-4625-bb74-8e4a0ceded19` |
| Run | `09c365c5-9df6-4135-b8b6-c9146bd748ca` |
| Attempt | `373d7ad4-1ad7-4ea7-9843-1b0abc00b621` |
| Candidate | `be5eb14f-1742-4c86-922e-11c9dda5bb3c` |
| Candidate commit / tree | `ff1c0c8ced8a67d38cf286b342fd14432c7b6002` / `c4afcab70427889402081f584700dfa1b6c41367` |
| Frozen Lead plan hash | `1d172b8452e861f6fb9d57a05d083e8de452f007e443780b2df320a520d29168` |
| Compiled plan content hash | `8191910204a3085622f344729738e96dbf0a77242dfb401b5d88438ab3e58aa5` |
| Frozen specification hash | `c9c0861e969af2f7a08d388c6bf89bddc82b76d572fe6cff4bb33efcb25edd83` |
| WorkGraph / participant graph hash | `f62a0393c44fcf18baaacf70f6b27805c1db8a386202e2ea326da889337d690e` |
| Campaign status | `AUTHORIZED` |
| Run / Attempt status | `ACCEPTED` / `CANDIDATE_READY` |
| Initial Gate row | `REPAIR_REQUIRED` at `2026-11-10T15:07:08.399941101Z` |
| Latest Gate revision | `ACCEPTED` at `2026-11-10T16:51:03.655161705Z` |

The target candidate changed three files (`README.md`, `src/main.rs`,
`tests/cli.rs`; 1,108 insertions) from the base commit.

## 4. Flow and timeline

The run followed the ordinary Office intake path. The plan review was
policy-auto-approved; the later operator action was the exact intake approval.

| Local time (UTC−5) | UTC | Event |
|---|---|---|
| 09:56:59 | 14:56:59 | Intake request submitted; Lead assessment reserved |
| 09:57:04 | 14:57:04 | Subscription admission confirmed (`07ca5ee4…`, Extra Usage disabled) |
| 09:57:06 | 14:57:06 | Lead assessment starts |
| 09:58:03 | 14:58:03 | Assessment settled; Lead proposal and frozen plan recorded |
| 09:58:09–09:58:45 | 14:58:09–14:58:45 | Planner compiles the WorkGraph (one Planner call) |
| 09:58:45 | 14:58:45 | Participant plan recorded (9 assignments, 13 FormalSpec units, 5 reviewers); plan review `AUTO_APPROVED` |
| 09:58:51–09:59:51 | 14:58:51–14:59:51 | Spec Manager authors the formal specification |
| 09:59:54 | 14:59:54 | Specification frozen; workflow `FROZEN` |
| 10:00:01 | 15:00:01 | Operator approves the exact frozen plan; Campaign authorized and started |
| 10:00:01 | 15:00:01 | Run starts; attempt `373d7ad4…` claimed |
| 10:00:29 | 15:00:29 | First provider `final` rejected as invalid JSON (`provider.tool_event`) |
| 10:02:25–10:04:28 | 15:02:25–15:04:28 | Implementer writes the CLI, tests and README; frozen checks pass |
| 10:04:31 | 15:04:31 | Provider returns a valid `final` explanation |
| 10:04:32 | 15:04:32 | Candidate recorded with exact commit and tree |
| 10:04:46 | 15:04:46 | Verifier `PASS` (`EXECUTED`) |
| 10:05:47–10:07:08 | 15:05:47–15:07:08 | Five reviewer receipts, all `PASS` (`EXECUTED`) |
| 10:07:08 | 15:07:08 | Gate `REPAIR_REQUIRED`: 5 reviewer findings remain open |
| 11:51:03 | 16:51:03 | Gate re-evaluated and `ACCEPTED`; findings recorded as observations |
| 11:51:03–11:51:11 | 16:51:03–16:51:11 | Four unique finding dispositions recorded `RESOLVED` by the owner |
| 11:51:21 | 16:51:21 | Campaign detail screenshot shows `Accepted` |

The frozen Lead proposal contained **one** implementation work unit (`unit-1`).
The Planner's FormalSpec WorkGraph contained **13** design units used for the
specification and participant plan. The Campaign executed the single
implementation unit; this is not a multi-unit execution test.

## 5. Checks, verifier, reviewers and Gate

### 5.1 Frozen project checks

The Worker executed the same four frozen checks twice, before and at final.
All eight `execution_check_receipts` are `PASS`.

| Check | Command | Expected / actual |
|---|---|---|
| Build | `cargo build --all-targets` | 0 / 0 |
| Tests | `cargo test --all-targets` | 0 / 0 |
| CLI help | `cargo run --quiet -- --help` | 0 / 0 |
| Negative CLI | `cargo run --quiet -- unknown-command-for-vlog-test` | 2 / 2 |

### 5.2 Local candidate verification

I exported the exact candidate commit from the Core project repository into a
temporary directory and re-ran the requested commands. No target code was
modified.

```text
cargo build --locked
  Finished `dev` profile ... in 0.77s

cargo test --locked
  running 0 tests (unit tests)
  running 5 tests (tests/cli.rs)
  test result: ok. 5 passed; 0 failed; 0 ignored
```

The accepted commit contains **five** integration tests, not eleven. The
"11 tests" count in the handoff is not reproducible from candidate
`ff1c0c8`; it is not used as evidence. See [§10](#10-evidence-reconciliation).

### 5.3 Verifier and reviewer receipts

All receipts below are `PASS` with evidence `EXECUTED` and are bound to the
exact candidate commit and tree.

| Role | Principal | Receipt | Received (UTC) |
|---|---|---|---|
| Verifier | `verifier-process` | `7c0fd56e-8433-4168-886d-5e19de85b8a9` | 15:04:46 |
| Code quality | `reviewer-code-quality-process` | `823b8b98-583f-424f-8a6c-9ad0b11fd7f8` | 15:05:47 |
| Architecture | `reviewer-architecture-process` | `a3357a4c-217d-47d7-90cd-b4bea116bfce` | 15:05:58 |
| Testing | `reviewer-testing-process` | `314424e2-8397-418c-8d3a-6c8f71e968a1` | 15:06:14 |
| Functional | `reviewer-functional-process` | `fe1e2158-ae4c-4087-85b6-61f1fcfe753d` | 15:06:30 |
| Security | `reviewer-security-process` | `c99952c2-008f-4045-aeff-c65a6dda1705` | 15:07:08 |

`operability` was not required at assurance `NONE`. The operability reviewer
service existed but produced no receipt for this Campaign.

### 5.4 Gate outcome and findings

The append-only `gate_decisions` row retains the first immutable outcome:

```text
REPAIR_REQUIRED  2026-11-10T15:07:08.399941101Z
reasons: ["5 reviewer finding(s) remain open"]
```

The latest `gate_decision_revisions` row and the Office Gate projection are:

```text
ACCEPTED  2026-11-10T16:51:03.655161705Z
reasons: ["independent executed evidence accepts exact candidate",
          "5 informational reviewer finding(s) recorded as observations"]
```

The five findings are informational and comprise four unique IDs; one ID was
reported by both the code-quality and functional reviewers:

| Finding | Route | Reviewer(s) | Disposition |
|---|---|---|---|
| `profile-lookup-case-sensitive` | `REPAIRER` | code-quality, functional | `RESOLVED` |
| `escape-roundtrip-untested` | `TEST_PLANNER` | code-quality | `RESOLVED` |
| `storage-corruption-resilience` | `REPAIRER` | testing | `RESOLVED` |
| `no-interprocess-write-lock` | `REPAIRER` | functional | `RESOLVED` |

The four durable dispositions were resolved by the authenticated owner with
evidence references `candidate:be5eb14f…`, `src/main.rs`, `tests/cli.rs` and
`README.md`. The Gate recorded the findings as observations rather than
blockers; the verifier and five reviewer receipts are the independent executed
evidence used for acceptance.

## 6. Browser, responsive and accessibility evidence

The browser run used the real Office against the running Core at
`http://127.0.0.1:8893`. It covered four surfaces at three viewports:

| Surface | URL | Controls (1440/768/390) | h1 | Overflow | Unnamed controls | Images without alt |
|---|---|---|---|---|---|---|
| Campaign detail | `#/campaigns/d61a09f9…?tab=gate` | 34 / 19 / 17 | 1 | 0 | 0 | 0 |
| Director | `#/director?request=037165ad…` | 83 / 68 / 66 | 1 | 0 | 0 | 0 |
| Request workspace | `#/requests/037165ad…` | 27 / 12 / 10 | 1 | 0 | 0 | 0 |
| Issues | `#/issues` | 39 / 24 / 22 | 1 | 0 | 0 | 0 |

All twelve surface/viewport combinations reported `docOverflow: 0`,
`bodyOverflow: 0`, no unnamed controls, no images without alt text and exactly
one `h1`. Raw records:
`/tmp/opencode/live-vlog/a11y-responsive.log`
(SHA-256 `dee0291b74b70f1e41e3fb83f356b3e281a2c9d518093b79f27dbcbcdb478395`).

Screenshots and logs are **local artifacts, not committed**:

- `evidence/campaign-gate-accepted.png` — the 11:51 pre-fix Campaign detail at
  1440. It shows `Accepted`, candidate `be5eb14f`, the two Gate reasons, the
  five-finding observation note and the old `Repair authorization: denied`
  banner. It is retained as historical pre-fix evidence.
- `evidence/campaign-gate-accepted-final.png` — the 12:47 final 1440 px
  Campaign detail against code candidate `7960123`. It shows `Accepted`,
  candidate `be5eb14f` and the two Gate reasons with **no repair-authorization
  banner** (`has repair banner: false`). SHA-256:
  `aa0dbb72de0eb18ebbff42edd519cc2fa509db2828b8f8ad7b65914b46d5ea55`.
- `evidence/findings-resolved.png` — Director showing the four distinct finding
  IDs resolved, five reviewer receipts and seven evidence items.
- `evidence/project-detail-accepted.png` — Project detail showing the accepted
  Campaign and `Issue sync: OK — No issue synchronization recorded.` The
  Project summary shows the Gate-effective `Accepted` status even though Core
  stores Campaign status `AUTHORIZED`, consistent with the `934eabf` repair.
- `evidence/issues-board.png` and `evidence/a11y-issues-*.png` — Issues board.
- `evidence/a11y-{campaign,director,request,issues}-{desktop,tablet,mobile}.png`
  — the twelve responsive captures.

The final 1440 px banner check at 12:47 America/Bogota confirmed that the
Accepted Gate no longer renders the repair-authorization banner
(`has repair banner: false`) while still showing `Accepted`, candidate
`be5eb14f` and both Gate reasons.

This is implementer visual review only. Full WCAG 2.2 AA, keyboard focus order,
contrast, screen-reader traversal, the Spanish locale and 320 px layouts were
**not tested** in this browser run. The run checked the four surfaces above,
not every Office route or every state of those surfaces.

## 7. Issues and kanban findings

GitHub has five open issues on the target repository:

| Issue | Title | Label |
|---|---|---|
| #5 | Vlog CLI: pruebas y documentacion | `docs` |
| #4 | Vlog CLI: busqueda | `feature` |
| #3 | Vlog CLI: comentarios | `feature` |
| #2 | Vlog CLI: perfiles de autor | `feature` |
| #1 | Vlog CLI: publicar y listar entradas | `feature` |

The Office Issues board rendered those five under **Needs attention**, each
with **“Automatic intake is off for this Project.”** The board also contained
six older `vlog-factory-live-test` issues, so the screenshot's total Needs
Attention count is 11; only the five `vlog-cli-factory-live-test` issues belong
to this run.

The Project detail reported:

```text
Issue sync  OK
No issue synchronization recorded.
```

This is a real gap, not a success:

- the accepted candidate was not linked to any of the five issues;
- no issue was moved to Done or closed;
- no spec issue publication or issue-intake batch exists for this Project;
- the board's “Sync is automatic” copy conflicts with the Project detail's
  “No issue synchronization recorded” state;
- there is no automatic issue-to-Campaign synchronization.

The five issues remain `OPEN` on GitHub. The run therefore demonstrates a
read-only issue projection and a Needs Attention explanation, not an
issue-to-Campaign delivery loop.

## 8. Security, authorization and delivery impact

- The Campaign ran at `R0` in `TRUSTED_LOCAL_PROVIDER`; this is **not
  isolation** and is not certified under `SEC-01-ISOLATED` (out of scope under
  ADR-0035).
- The intake approval bound the exact frozen plan hash, profile revision and
  authorized paths (`Cargo.lock`, `Cargo.toml`, `README.md`, `src`, `tests`).
- The subscription admission was owner-confirmed with OpenCode Go Extra Usage
  `DISABLED`; it is a funding precondition, not an isolation or security claim.
- The Worker and reviewers operated through their authenticated principals and
  separate processes. Reviewer independence is identity/process based, not
  provider/model diversity.
- The candidate was **not** pushed, merged or opened as a pull request. GitHub
  has no such branch or PR, and `gh api` returns no commit for `ff1c0c8`.
- Issue reads used the encrypted server-side GitHub connection; no token is
  present in this document or in browser evidence.
- The Gate accepted only the exact candidate commit/tree after executed
  verifier and reviewer receipts. Model output did not grant authority or
  select tools.
- Old Campaigns and a pre-existing unresolved model-call decision remain
  visible in the running Office; they are historical state, not part of this
  acceptance.

## 9. Live fixes recorded

The following repairs were found during the live test sequence and are merged
into code candidate `7960123`; the docs merge is a separate commit on top of
that candidate.

| Area | Fix commits | State |
|---|---|---|
| Runtime/workspace bootstrap | `0b0547c` | merged |
| Informational Lead assessment | `1d7053b` | merged |
| Worker runs missing frozen checks on final | `dc57840` | merged |
| Model accounting/reservation bounds | `61fea3d`, `9e4cdc9`, `4ff4090` | merged |
| Plan readiness, approval and re-evaluation | `86ee70c`, `ef77ca8`, `3958610`, `8b36501` | merged |
| Planner truncation retry | `6ccb9c4`, `11f3a08` | merged |
| Planner WorkGraph repair | `b0db72a` | merged |
| Core-control URL for control plane | `513f948`, `949f33f` | merged |
| Worker stagnation salvage | `8d11a05`, `3e8fc87` | merged |
| Gate info observations and re-evaluation | `59e7b3f`, `44455cb` | merged |
| Project summary uses Gate-effective status | `934eabf` | merged |
| Hide repair authorization when Gate is not repairable | `fc2b957` | merged in `7960123` |

`fc2b957` is included in code candidate `7960123` (parent `543bb50`). The final
1440 px browser check at 12:47 America/Bogota confirmed the Accepted Gate no
longer renders the repair-authorization banner (`has repair banner: false`)
while still showing `Accepted`, candidate `be5eb14f` and both Gate reasons. The
earlier 11:51 screenshot still shows the old banner and is retained as pre-fix
evidence.

## 10. Evidence reconciliation

The handoff narrative contained three claims that the durable record does not
support, plus one status that changed after the first draft. They are corrected
here rather than repeated:

1. **Planner validation repair.** Request `037165ad` compiled its WorkGraph in
   one Planner call (`14:58:09–14:58:45Z`) and auto-approved revision 1. No
   Planner validation repair is recorded for this request. The Planner repair
   commits in [§9](#9-live-fixes-recorded) are merged from the broader live test
   sequence; they are not part of this request's timeline.
2. **Stagnation salvage produced the candidate.** The durable provider events
   show an initial invalid `final` at `15:00:29Z`, continued writes and frozen
   checks, and a later valid `final-explanation` at `15:04:31Z`; the candidate
   was recorded 0.6 seconds later. No `worker.stagnation_finalized` event is
   recorded in durable events; the Worker emits that event to stderr and the
   Worker logs for that attempt were no longer available. The durable
   `final-explanation` event is the normal final path, and the campaign progress
   state reports one stagnant checkpoint, below the three-checkpoint `Diagnose`
   threshold. The earlier `f8ea5e85` Campaign was blocked at
   `IMPLEMENTER_PROGRESS_Diagnose` and produced no candidate. The salvage repair
   is deployed and deterministically tested, but it is not proven to have
   finalized `be5eb14f`. The record does not claim that it did.
3. **The candidate has 11 tests.** `ff1c0c8` has five `#[test]` functions in
   `tests/cli.rs`; `cargo test --locked` passes 5/5 with zero unit tests. The
   eleven-test count is not reproducible and is not used.
4. **Banner fix status.** The first draft placed the banner fix outside the
   delivered baseline. It is now merged in code candidate `7960123` (parent
   `543bb50`), and the final 12:47 browser check confirmed the banner is no
   longer rendered. The other measurements above are unchanged.

The Gate's append-only `gate_decisions` row still says `REPAIR_REQUIRED`. That
is the initial immutable decision; the latest `gate_decision_revisions` row and
the Office Gate projection are `ACCEPTED`. The screenshot and the revision row
agree with the accepted outcome.

## 11. Residual gaps and explicit non-results

| Item | Status | Note |
|---|---|---|
| Independent verifier/reviewer acceptance of the Factory | `NOT_TESTED` | This is implementer/orchestrator evidence |
| `TRUSTED_LOCAL` isolation/certification | `NOT_APPLICABLE` | Not isolated; ADR-0035 scope |
| Factory runtime image digest/commit | `NOT_RECORDED` | The final banner check used code candidate `7960123`; the original Campaign runtime digest was not recorded |
| Node/browser check for the target project | `NOT_TESTED` | The project has Rust checks only; the Rust check image does not provide a Node/browser toolchain, so no Node/Playwright project check was run |
| Full WCAG 2.2 AA, keyboard, contrast, screen reader | `NOT_TESTED` | Only the four-surface a11y assertions above |
| Spanish locale and 320 px | `NOT_TESTED` | English at 1440/768/390 only |
| Plan-review blocking states (missing artifacts, mismatch, changes requested) | `NOT_TESTED` | The ready/auto-approved path was exercised |
| Finding resolve/refute controls in the browser | `NOT_TESTED` | Dispositions were recorded; the browser actions were not exercised |
| Multi-unit execution | `NOT_TESTED` | One implementation unit executed |
| Model-call resolution/release | `NOT_TESTED` | A pre-existing unresolved call remains visible; no resolution action was exercised |
| Automatic issue-to-Campaign sync | `BLOCKED` | Automatic intake is off; no sync recorded; issues remain open |
| Candidate push, PR or merge | `NOT_DONE` | No branch, PR or commit on GitHub |
| Plan Compile before Lead assessment | `NOT_TESTED` | Compile was run after assessment; no pre-assessment path was tested |
| Stagnation salvage attribution for `be5eb14f` | `NOT_PROVEN` | See [§10](#10-evidence-reconciliation) |
| `fc2b957` accepted-Gate banner fix | `DELIVERED` | Merged in `7960123`; final 12:47 1440 px browser check reported `has repair banner: false` |
| Old requests/campaigns remain | `OBSERVED` | Old Campaigns and a pending model-call decision remain visible |

The target project candidate is real and was accepted by the deterministic
Gate for this exact run. The Factory itself remains an incomplete
developer-preview product and `TRUSTED_LOCAL` remains uncertified.

## 12. Reproduction and local artifacts

Read-only reproduction used a copy of the Core SQLite database and the Office
API; no live state was mutated:

```text
sqlite3 <read-only copy> "select ... from campaigns/candidates/gate_decisions/
  gate_decision_revisions/quality_receipts/finding_dispositions/..."
gh issue list -R JamesMorales04/vlog-cli-factory-live-test --state open
podman exec software-factory_core_1 git -C /projects/project-169b8e55acac4c967cd2a6e5 \
  show --stat ff1c0c8ced8a67d38cf286b342fd14432c7b6002
```

The local candidate was exported with `git archive` and verified with:

```text
cargo build --locked
cargo test --locked
```

Local browser artifacts (not committed):

```text
/tmp/opencode/live-vlog/a11y-responsive.log
/tmp/opencode/live-vlog/a11y-responsive.mjs
/tmp/opencode/live-vlog/full-flow-8.log
/tmp/opencode/live-vlog/evidence/
/tmp/opencode/live-vlog/evidence/live-flow.log
```

`evidence/live-flow.log` SHA-256:
`3b6e034a0c7b1c6ac230cd3a41b2f996ff4bba4a6485336f48dbeb283089fdd9`.
`full-flow-8.log` SHA-256:
`0d9f39284a4450a1f0bd653e888be32c6f02714093e2caf1508e224a40dc9fb5`.

The logs and screenshots are evidence for this environment and candidate only.
They are not committed binaries and they are not independent acceptance.
