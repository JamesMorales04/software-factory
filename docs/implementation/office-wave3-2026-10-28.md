# Factory Office programme — Wave 3 ledger (2026-10-28)

> Status record, not acceptance evidence. A feature listed as *implemented on an
> integration candidate* is **not** merged, certified or accepted.

## 1. Exact baseline

| Item | Value |
|---|---|
| Repository | `JamesMorales04/software-factory` |
| `main` (authoritative) | `d58725266d132fafb96a79970ff292acc876feb8` (merge of PR #35) |
| `main` branch protection | **none** (GitHub HTTP 404) — merging requires explicit owner authority |
| Wave 2 evidence tip | `4e0a43e310f32530162a6793288f6de7db2e2bcf` |
| Wave 2 code candidate reviewed by Sol/Luna | `7063321` (Compose CI run `40765699005` passed) |
| Wave 3 integration branch | `campaign/office-ux-wave3-20261028` at `4e0a43e` |
| Wave 3 integration worktree | `/home/james/Escritorio/Development/sf-wave3/integration` |
| Open access PRs (unchanged) | #36 `bda12f0`, #37 `715349f`, #38 `c8bf930` — draft, unmerged |

The UX work is **not on `main`**. It is rooted in the access-stack commits and
has been reviewed but promotion is blocked on #73 (see
`office-ux-wave2-independent-review-2026-10-28.md`).

## 2. Runtime and model verification (observed this session)

| Probe | Command | Result |
|---|---|---|
| OpenCode version | `opencode --version` | `2.2.0` |
| Codex version | `codex --version` | `codex-cli 0.165.0` |
| DeepSeek route | `opencode run --model opencode-go/deepseek-v4.1-flash "Reply with exactly: MODEL_OK"` | `MODEL_OK` |
| Sol route | `codex exec -m gpt-6.1-sol --sandbox read-only "…SOL_OK"` | `SOL_OK` |
| Luna route | `codex exec -m gpt-6-luna --sandbox read-only "…LUNA_OK"` | `LUNA_OK` |
| GitHub CLI | `gh auth status` | logged in as `JamesMorales04` with `repo`, `workflow` |
| Podman | `podman --version` | `6.1.4` |
| Host | 16 CPU, 31 GiB RAM, ~1.5 TiB free | |

No paid inference was performed. The probes are text-only and use the owner's
existing authenticated sessions.

## 3. Provider evidence for #59 (observed, no credential)

| Request | Observed |
|---|---|
| `GET https://opencode.ai/inference/go/v1/models` | **200**, 2,518 bytes, OpenAI list with **30** real model IDs (`deepseek-v4.1-flash`, `deepseek-v4-pro`, `gpt-6-luna`, …) |
| `GET https://opencode.ai/inference/go/openai/v1/models` (Factory's current composition) | **404**, empty body |
| `GET https://models.dev/api.json` | 200, 5.9 MiB (not needed for the primary fix) |

Root cause: the configured inference base URL ends in `/openai/v1`, while the
documented catalog lives at the sibling `/inference/go/v1/models`. Factory
composes `{inference_base}/models` unconditionally, so the catalog 404 proves a
wrong path, not an invalid credential. The fix must model **catalog URL and
inference base URL separately per provider** and keep 404 neutral about
credential validity. A models.dev integration is not the primary fix.

## 4. Issue reconciliation (campaign view)

Statuses are the campaign's working view from the recorded evidence and actual
source inspection; each issue still needs its own closing evidence.

| Issue | State | Where |
|---|---|---|
| #39 umbrella | ACTIVE | this ledger |
| #40 profiles | Implemented on integration candidate (PR #56 merged to baseline); needs Policy V2 rework | baseline → wave2 |
| #41 IA / Home composer | Implemented on integration candidate (PR #54 closed as superseded, code in baseline) | baseline |
| #42 unified request workflow | Implemented to Campaign authorization with deterministic model; full lifecycle construction in wave2 | wave2 |
| #43 EN/ES + mobile | Partial: shell/nav/Home/Request/Actions/Lead/Campaigns/Agent Office/Issues; remaining surfaces open | wave2 |
| #44 file browser | Implemented bounded browser (PR #55) | baseline |
| #45 provider connectivity | Partial; OpenCode Go discovery defect remains (#59) | wave2 |
| #46 semantic assessment/quality plan | Implemented with model gateway; live certification blocked | wave2 |
| #47 project onboarding / GitHub / local import | Implemented on integration candidate | wave2 |
| #48 Decision Inbox | Implemented on integration candidate | wave2 |
| #49 diff / PR / delivery | Partial; merge gate repaired, GitHub write operations remain limited | wave2 |
| #50 GitHub Issues | Implemented bounded sync on integration candidate | baseline/wave2 |
| #51 autonomous issue intake | Contract + commits 1–2; **blocked** by Sol/Luna HIGH findings | wave2 |
| #52 independent QA | Continuous; live acceptance not performed | — |
| #59 OpenCode Go discovery | **Open defect** — `/models` 404 misdiagnosed | **Wave 3 lane P** |
| #60 live agentic UAT | **BLOCKED** on live provider route + spend authorization | after P + ledger |
| #61 provider/profile simplification | Partial (selectors exist; caps still exposed) | Wave 3 P + C |
| #62 Home/campaign/nav rethink | Partial | Wave 3 |
| #63 GitHub repo selector | Implemented on integration candidate | wave2 |
| #64 Issues Kanban + advanced policy separation | Implemented on integration candidate | wave2 |
| #65 Agent Office visual | Slice 4 table/timeline/floor cards on `main`; isometric absent | **Wave 3 lane I** |
| #66 Markdown links/anchors | Not implemented | later |
| #67 >512 KiB pagination/streaming | Not implemented | later |
| #68 budget UX simplification | Partial; artificial caps still surface | Wave 3 C |
| #69 per-project merge policy | Partial; manual default enforced, project config not complete | later |
| #70 isometric Office V0 | **Not implemented** | **Wave 3 lane I** |
| #71 execution control simplification | Partial; Core caps still bind | Wave 3 C |
| #72 Execution Policy V2 | **Not implemented**; caps confirmed in code | **Wave 3 lane C** |
| #73 shared campaign ledger | **Not implemented**; Wave 2 blocking finding | **Wave 3 lane L** |

## 5. Dependency graph

```
P  #59 OpenCode Go discovery ──────────────┐
I  #70 isometric Office V0 ────────────────┤
                                           ├─> integration candidate
Sol contract (#72 + #73 interface)         │
   ├─> C #72/#71/#68 execution policy ─────┤
   └─> L #73 shared campaign ledger ───────┘
                                           │
                          Luna independent review
                                           │
                          owner visual / promotion decision
```

P and I are independent and start immediately. C and L require the Sol contract
and must not share Rust modules or migration numbers; the ledger lane owns the
new spend migration, the policy lane owns policy fields in existing tables.

## 6. Lane matrix

| Lane | Branch | Worktree | Owner | Issue | Depends | Reviewer |
|---|---|---|---|---|---|---|
| P provider | `factory/provider-opencode-go/20261028` | `sf-wave3/provider` | DeepSeek V4.1 Flash | #59 | — | Luna |
| I isometric | `factory/office-isometric/20261028` | `sf-wave3/isometric` | DeepSeek V4.1 Flash | #70/#65 | — | Luna (frontend arch) |
| C policy | `factory/core-execution-v2/20261028` | `sf-wave3/policy` | DeepSeek V4.1 Flash | #72/#71/#68 | Sol contract | Sol + Luna |
| L ledger | `factory/shared-budget-ledger/20261028` | `sf-wave3/ledger` | DeepSeek V4.1 Flash | #73 | Sol contract | Sol + Luna |
| Integration | `campaign/office-ux-wave3-20261028` | `sf-wave3/integration` | Director | #39 | all | — |

Director owns shared files: `native/src/bin/core.rs`, `native/src/lib.rs`,
`native/src/db/mod.rs`, migrations, `office/src/coreApi.ts`, `App.tsx`,
`navigation.ts`, shared CSS, parity JSON, capability registry.

## 7. First checkpoint acceptance

1. Lanes P and I have a committed implementation on their branch with focused
   tests, or a truthful blocker report.
2. Sol has produced a bounded contract for #72/#73 with exact enforce points and
   migration/ownership boundaries.
3. No change to `main`; no fabricated provider or acceptance claim.
4. This ledger and the campaign state are committed on the integration branch.

## 8. Checkpoint 1 — lane evidence (2026-10-28)

| Lane | Candidate | State | Evidence |
|---|---|---|---|
| P #59 | `0ae58f4`, docs delta `278688f` | **Draft PR #74** | Rust: 11 provider-catalog tests + Core discovery test pass; clippy clean; Office typecheck + 18 focused tests pass. Luna: **APPROVE WITH CONDITIONS**; read-only check condition resolved by a writable Director rerun; parity-state Low fixed in `278688f`. |
| L #73 | `e9a3acd` | **Draft PR #75** | 19 ledger tests (real SQLite) pass; 417 lib tests pass; clippy clean. Covers concurrency, stale revision/scope, replay, fencing, overdraw, revocation, ambiguous recovery, five-role aggregation, populated-0033 migration. Luna security review running. |
| I #70 | `6bb5cde` + docs | Amendment in progress | V0 SVG/DOM isometric scene, 0/1/5/12/30, stable identity, filters, selection, pan/zoom; 229 Office tests, typecheck/lint/build pass. Owner reference pack now hosted at `docs/product/reference/factory-office-visual-reference-pack-20261028/` (commit `9f6a8c0`). Agent re-reading updated #70/#65 and amending visual fidelity. |
| C #72 | no commit yet | Active | Migration `0034`, `progress.rs`, authorization/worker/progress changes in progress; clippy repairs running. |

### Independent architecture review

Sol reviewed the frozen shared interface (`execution_policy.rs`) and returned
**BLOCK** with two HIGH findings:

1. derived `Deserialize` for `UsdMicros` accepted values above SQLite `INTEGER`;
2. the frozen reserve/settlement boundary lacked durable authorization-revision,
   scope and Worker-fencing context.

Disposition: fixed in `b16ee5e` (checked deserialization, `authorization_revision`,
`scope_sha256`, `worker_id`/`generation`, full reservation provenance, role/
provider spending breakdown) and adapted by Lane L in `a1ea4c5`/`e9a3acd`.
Sol's remaining MEDIUM items (validated identity newtypes, `JsonSchema`) are
recorded as follow-ups, not silently dropped.

### Contested files and duplicate writers

- The provider worktree briefly had two writers (a duplicate agent session).
  `0ae58f4` is the accepted sibling-catalog implementation; the later
  models.dev variant `ef79f7f` is preserved only as local branch
  `factory/provider-opencode-go-modelsdev/20261028` and is **not** the PR.
- Lane L cherry-picked the interface fix; its branch contains `a1ea4c5`, while
  the integration branch has `b16ee5e`. The Director resolves this at merge.
- Migrations are disjoint: Lane C owns `0034`, Lane L owns `0035`. No lane may
  renumber the other's migration.

### Not yet done

- Lane C commit and its independent reviews.
- Luna ledger review and Sol ledger implementation review.
- Model-gateway `mark_inference_dispatched` wiring; Office spending surfaces.
- Real Core/Compose/Chromium acceptance and live inference (requires owner spend
  authorization).
- No `main` merge; access PRs #36–#38 remain unmerged.

## 9. Checkpoint 2 — synchronization (2026-10-28)

### Lane status

| Lane | Candidate | PR | Review | Blockers |
|---|---|---|---|---|
| P #59 | `278688f` | #74 draft | **Luna: APPROVE WITH CONDITIONS**; condition (writable Rust tests) resolved by Director rerun; parity Low fixed | Live inference NOT_RUN_PENDING_OWNER_AUTHORIZATION; no Compose/browser acceptance |
| I #70 | `cd0c1ef` (repairs `c06432d`) | #76 draft | Luna: **BLOCK** on `489bc48`; repair addresses overlap, duplicate determinism, candidate-bound evidence; re-review pending | Real Core/Compose/Chromium acceptance; Owner visual approval |
| C #72 | `de31135` | not opened | Sol + Luna reviews running | `bin/core.rs` 36 Director-integration errors; one nondeterministic cross-lane test in Lane L-owned `db/model_calls.rs`; #72 OPEN |
| L #73 | `e9a3acd` + repair in progress | #75 draft | **Sol: BLOCK; Luna: BLOCK** | Ledger optional in production; gateway releases after send; binding ownership; mutable dispatch fields; per-egress holds; estimate-as-bound; settlement helpers; ordered 0034→0035 migration |

### Verified review evidence

- Sol produced the V2/ledger contract (`5da9e5a`) and reviewed the shared
  interface: **BLOCK** with two HIGH findings, fixed in `b16ee5e` (checked money
  deserialization, authorization revision/scope/fencing context, full receipt
  provenance, role/provider breakdowns). Sol's remaining MEDIUM items (validated
  identity newtypes, `JsonSchema`) are explicit follow-ups.
- Luna reviewed provider candidate `0ae58f4`: no blocking code defect; condition
  on test execution satisfied; parity Low fixed in `278688f`.
- Sol and Luna independently reviewed ledger `e9a3acd` and both blocked it. The
  repair is on the same lane; no acceptance claim.
- Luna reviewed isometric `489bc48` and blocked on overlapping avatar bounds;
  `c06432d` fixes the geometry and duplicate determinism, with 235 Office tests
  and refreshed 0-overlap evidence.

### Contested files / duplicate writers

- The provider worktree briefly had two writer sessions. The accepted candidate
  is `0ae58f4` (sibling catalog URL). The models.dev variant `ef79f7f` is
  preserved only as local branch `factory/provider-opencode-go-modelsdev/20261028`
  and is not the PR.
- The isometric worktree also had a duplicate amendment writer. The reconciled
  branch is coherent (`cd0c1ef`); earlier variants remain under
  `/tmp/opencode/lane-i-collision-20261028`.
- Shared `bin/core.rs` is Director-only. Lane C's `WorkerAssignment` DTO edit in
  `db/mod.rs` is a declared deviation that the Director must review.
- Migrations remain disjoint: C `0034`, L `0035`; ordered chain test is a
  Director gate.

### Owner visual reference status

The owner's generated pack is now repository-accessible at
`docs/product/reference/factory-office-visual-reference-pack-20261028/` with
`README-visual-contract.md`. The images are direction, not runtime assets. The
isometric renderer composes original SVG/DOM geometry; no Game Dev Tycoon assets
are used. Owner visual approval remains outstanding.

## 10. Integration-blocker status (2026-10-28, post-review)

**Migration numbering resolved.** Policy owns `0034`; ledger owns `0035`;
policy progress holds are `0036`; the ledger repair is `0037` (renamed from a
duplicate `0036`). The full `0001…0037` chain applies cleanly with
`PRAGMA foreign_key_check` empty and `integrity_check = ok`.

**Integrated candidate** `4912ef0` (P + I + C + L merged):
`cargo test -p factory-native --lib` → **467 passed, 0 failed, 2 ignored**.
One browser test was flaky on an earlier run and passed on rerun; it is not
claimed as stable evidence.

**Remaining independent-review blocks (exact reviewed SHAs):**

| Lane | Reviewed SHA | Verdict | Remaining finding |
|---|---|---|---|
| C #72 | `f93a5cb` (repairs after `de31135`) | Sol/Luna re-review running | policy JSON decoder mismatch; progress not durable across attempts; fake progress from unchanged checks; V2 egress; `max_turns*8`; Core wiring |
| L #73 | `1a6d7a4` (repairs after `8cba5ad`) | Sol/Luna re-review running | existing bound campaigns default to V1 on upgrade; quote token envelope does not bound the dispatched request; stale pricing remains eligible; V2 cumulative token caps |
| I #70 | `0455b39` (repairs after `cd0c1ef`) | Luna re-review running | inspector still uses the raw first duplicate instead of the layout winner |
| P #59 | `0ae58f4`/`278688f` | Luna APPROVE WITH CONDITIONS | resolved; live inference not authorized |

Repairs are committed on the same lanes and re-reviewed at the exact heads.
A separate Core-wiring worktree
(`factory/core-wiring/20261028`, base `a12aa38`) is implementing the
Director-owned `bin/core.rs` envelope/authorization wiring.

**Not accepted, not merged to `main`.** The ordered `0034→0035` populated
upgrade, Core binary tests, Compose E2E and live-provider proof remain gates.

## 11. Development candidate — 2026-10-29

**Code candidate:** `66a71bf99453ea507514d12cae63bdbe59758b96` on
`campaign/office-ux-wave3-20261028`. `main` is unchanged.

### Verified gates

| Gate | Result |
|---|---|
| Rust library | **487 passed, 0 failed, 2 ignored** |
| Core binary | **31 passed** |
| Fixture dispatch E2E | **12 passed** |
| Office | typecheck, lint, **239 tests**, build pass |
| Clippy all targets | clean |
| Compose E2E | run **40825390549 SUCCESS** on `66a71bf` (intake → assessment → approval → envelope → Worker → verifier/reviewer → ACCEPTED gate → restart) |
| Migration chain | `0001…0040` applies; FK/integrity checks clean; populated 0033→0040 fixture passes |
| Provider sandbox | 2 host-environment failures; both reproduce on the clean base and require the containerized OpenCode runtime |

### Independent reviews on the exact candidate

- **Sol:** APPROVE WITH CONDITIONS.
- **Luna:** APPROVE WITH CONDITIONS.
- Prior #72/#73 HIGHs are closed: assessment charged to the request envelope;
  receipt-bound test evidence; binding quarantine; durable dispatch intent;
  crash/restart ambiguity; per-egress holds; fence/ownership; no artificial
  productivity cap found.

### Conditions carried forward

1. Add explicit Home tests for zero, negative, unsafe/over-limit and
   policy-retry edited amounts (code inspection currently covers them).
2. Live paid inference remains **NOT_RUN_PENDING_OWNER_SPEND_AUTHORIZATION**.
3. Real-browser Office journey, OS-process-kill injection, upstream pricing
   correctness and independent verifier/Gate acceptance remain **NOT_VERIFIED**.
4. Access PRs #36–#38 remain draft and unmerged; promotion requires owner
   authorization and the normal protected review path.
5. `main` is not protected; no merge is authorized by this ledger.

### Draft PRs

- #74 provider — draft, Luna approved with conditions resolved.
- #75 shared ledger — draft; repaired code is in the integrated candidate.
- #76 isometric Office — draft; Luna approved with conditions.

No duplicate issues were opened. No `main` merge was performed.

## 12. Final review disposition (2026-10-29)

**Code candidate:** `66a71bf` (Compose run `40825390549` SUCCESS).
**Final branch head:** `1c7621d` — adds only review-condition tests and the
evidence record; no runtime behavior changed after `66a71bf`.

- **Sol:** APPROVE WITH CONDITIONS on `66a71bf`.
- **Luna:** APPROVE WITH CONDITIONS on `66a71bf`; its only condition (run
  formatter/tests/clippy on a writable checkout) was satisfied by the Director
  (`cargo fmt`, 487 lib tests, 31 Core-bin tests, 12 fixture E2E, clippy, Office
  241 tests).
- Prior #72/#73 HIGH findings are closed: assessment envelope, receipt-bound
  progress, binding quarantine, crash/restart ambiguity, dispatch intent,
  ownership/fencing and no artificial productivity ceiling.
- Remaining conditions are explicit, not waived: live paid inference (owner
  spend authorization), real-browser Office journey, OS-process-kill injection,
  upstream pricing correctness, access-stack promotion and `main` protection.
- Wave 3 remains a development branch. No `main` merge and no product
  acceptance are claimed.

## 8. Change log

- 2026-10-28: Wave 3 opened at `4e0a43e`. Model routes verified
  (`MODEL_OK`, `SOL_OK`, `LUNA_OK`). Five worktrees created. P/I started; Sol
  contract requested for C/L.
- 2026-10-28: Verified the OpenCode Go catalog root cause: the catalog is at
  `/inference/go/v1/models` (200, 30 models), while Factory composed
  `{inference_base}/models` (`/openai/v1/models`, 404). Lane P corrected toward
  a per-provider catalog URL.
- 2026-10-28: Sol delivered the Execution Policy V2 and shared-ledger contract
  (`docs/implementation/office-execution-policy-v2-contract-2026-10-28.md`,
  commit `5da9e5a`).
- 2026-10-28: Director froze the shared interface
  (`native/src/execution_policy.rs`, commit `5937d6b`) with checked `UsdMicros`,
  `FrozenExecutionPolicy`, V2 operation/progress types, continuation decisions,
  inference roles and ledger DTOs; five interface tests pass. Lanes C and L
  started from that commit.
