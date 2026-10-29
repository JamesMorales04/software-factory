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
