# Wave 3 execution checkpoint — 2026-10-29

**Integration branch:** `campaign/office-ux-wave3-20261028`
**Verified remote integration HEAD:** `f7479c0c5f936d1097426dd1293ef43ea6e72fb6`
**Integration worktree:** `/home/james/Escritorio/Development/sf-wave3/integration` (local checkpoint branch has documentation commits ahead of the remote)
**`main`:** unchanged. No automatic merge or release is authorized.

This checkpoint supersedes earlier phase-3 planning text wherever it conflicts
with the current Owner policy below. Historical candidate evidence remains
provenance and is not current campaign certification.

## Owner policy

- Inference is limited to verified subscription-included entitlements.
- OpenCode Go Extra Usage is OFF; no balance fallback, API-billed credits,
  automatic top-up or billable API-key fallback.
- Codex requires ChatGPT-plan authentication. Other providers must prove the
  included entitlement and no-charge route.
- No positive USD spend cap is required for subscription-only use. Provider
  quota, authorization and technical limits still apply. Unknown billing
  behavior fails closed.
- No live inference until the subscription route, no-extra-billing condition
  and specific test authorization are established.
- No automatic merge to `main`; no provider billing settings may be changed.

## Active implementation lanes

| Lane | Worktree / branch | Base / current state | Owner | Next deliverable |
|---|---|---|---|---|
| Subscription funding V3 | `sf-wave3/subscription-funding` / `factory/subscription-funding/20261029` | Latest `f4e757a755e43ded4f9cb3ac17117952466def49`; worktree clean | Luna subagent `funding_shared_admission`; independent review APPROVE for `f4e757a` | Continue request-level subscription admission only after defining its separate boundary; quota pause/re-admission still needs trusted provider exhaustion evidence and append-only observations |
| Conversational Home (#62) | `sf-wave3/ux-conversation` / `factory/office-conversation/20261029` | PR #80 at `3d394652fd3956676affbefb6c8109076ca33c77` | Luna subagent `home_conversation`; feature review PASS; fixture route fix committed; isolated Home Chromium run active | Run Home-specific fixture journey, rebind PR declarations to exact SHA; broader Lead journey requires V2 grant HTTP route, while existing fixture creates historical V1 grant; keep visual sign-off pending |
| Agent Office V3 (#65/#70) | `sf-wave3/isometric-v3` / `factory/office-isometric-v3/20261029` | PR #82 at `82b9ee4703326947a9a3c78153d9bf1994a3a960`; source `e31e46298b42f13e80701913b43878336587ccb8`, captures `782b5d9dbe484f41afa12120667a07f248703bb0` | Luna subagent `agent_office_v3`; exact independent review PASS; prior infinite loop corrected | Resolve inherited CAP-050/052 architecture check after #81; Owner visual acceptance pending |
| Access repairs (#36–#38) | `sf-wave3/access-repairs` / `factory/access-repairs/20261029` | Repair HEAD `a03dd55776fe20c15a8fac3565a40fa5b147a0b6`; clean | Luna subagent `access_security_repairs`; re-review BLOCK HIGH for named-volume alias bypass | Resolve actual Compose volume identity aliases and add exact Worker negative regression; do not promote while HIGH remains |
| Provider acceptance / PR #79 | `sf-wave3/provider-acceptance` / `factory/provider-acceptance/20261029` | `a8d3488abfbfc4cec1b60005ca7ae664008a5019`, pushed | Luna subagent `funding_shared_admission`; exact review and PR checks running | Wait for review/CI; UAT remains BLOCKED because current Core lacks V3 subscription admission |
| Capability evidence shape | `sf-wave3/capability-evidence-shape` / `factory/capability-evidence-shape/20261029` | `13adff74a8de434fd46c527ea92608b695a98d03`; draft PR #81 | Director | Collect CI and exact-SHA independent review; integrate only after gates pass |

No active Cargo or frontend writer was observed in the funding, Home, access,
or V3 worktrees before assignment. A long-running Vite server has its working
directory in `sf-wave3/isometric-v3`; no file-writing process was observed, and
the server plus untracked `office/node_modules` were left untouched. The current
Codex checkout also has untracked files in `software-factory-native-rust`; they
were left untouched.

## Pull requests and current delivery state

- #77 — Design System and Home visual foundation: integrated into Wave 3.
- #78 — Isometric Agent Office V2: integrated into Wave 3.
- #79 — subscription-only provider UAT preparation: open draft; Owner requires
  subscription-only inference with no positive USD cap, no-extra-billing
  evidence and fail-closed behavior. Commit `a8d3488` is pushed and updates the
  runbook/harness accordingly. Core V3 funding admission is absent, so the live
  preflight refuses before Project/Request creation; no UAT ran. Fake-Core
  harness tests passed 26/26. Exact review and architecture/Compose/container/
  Rust PR checks are running.
- #80 — conversational Home: open draft against Wave 3 at
  `3d394652fd3956676affbefb6c8109076ca33c77`. Feature review passed. The
  FIXTURE-only claim regression is repaired and focused DB, fmt and strict
  Clippy gates passed. A full journey has an existing V1/V2 contract mismatch:
  fixture setup creates a historical V1 delegation, but V2 Lead requires a V2
  grant and Core exposes no HTTP V2 grant-create route. The agent is exercising
  the isolated Home-only Chromium journey. PR architecture declaration needs
  rebinding to this exact SHA.
- #81 — capability evidence-reference correction: open draft against Wave 3 at
  `13adff74a8de434fd46c527ea92608b695a98d03`. Canonical container architecture
  governance and Compose policy passed after retrying with the worktree's common
  Git metadata mounted read-only; Rust passed. Independent exact-SHA review
  **APPROVED** the two evidence-reference deletions. Container-dev failed on the
  pre-existing FIXTURE Worker route-identity bug, repaired in Home candidate
  `3d39465`; rerun only after rebinding #81 to a reviewed integration base that
  contains that repair, without folding Home into the evidence-only PR.
- #82 — Agent Office V3: open draft at
  `82b9ee4703326947a9a3c78153d9bf1994a3a960`. The previous candidate was
  blocked for a non-terminating appearance allocation loop; `e31e462` replaces
  it with deterministic identity-sorted allocation and regressions for 200
  unique appearances. Fixture captures and parity mapping are committed.
  Compose policy passed; Rust/container-dev remain pending; architecture check
  fails on the inherited CAP-050/052 registry entries pending #81.
- #36 → #37 → #38 — no PRs opened from this repair branch yet. Exact review of
  `2851345` found a Major Compose policy gap, addressed in `d2fa767`/`a03dd55`;
  re-review then found a HIGH named-volume alias bypass. A `shared-bits` alias
  with `name: software-factory_factory-office-socket` mounted read-only in
  Worker passes policy. Current Compose does not use this alias, but the static
  policy must resolve top-level volume names and external aliases. A regression
  fix is active. Tailscale authentication, Serve and remote HTTPS remain
  NOT_TESTED; no promotion until exact re-review and Owner authorization.

Historical deterministic Compose/Chromium acceptance at `40d44ec` and prior
live OpenCode evidence at `1225bae` are candidate-bound history. They do not
authorize new paid inference, certify current changes or imply campaign
completion.

## Completed and pending evidence

- Funding Milestone 1: committed domain, migration 0041, persistence and quota
  pause representation at `5b902a4`; dispatch remains disabled.
- Funding Milestone 2: `fe1729a`, `5619ce4` and `4f1ab87` provide shared
  fail-closed Core/Worker admission and request binding; proposals and verified
  subscription routes remain disabled. Independent review of `4f1ab87` was
  **BLOCKED** for lost Worker-to-ledger lifecycle coverage. Commit `f4e757a`
  restores restart/replay, persisted egress links, idempotent settlement and
  mid-turn revocation tests. Format, diff, provider-session (32/32), ledger
  (23/23), funding-admission (3/3) and strict Clippy passed. Independent review
  **APPROVED** `f4e757a` for lifecycle coverage (focused review command 9/9).
  Quota recovery is not safely implementable with current provider
  evidence: migration 0041 stores immutable disabled proposals and has no
  append-only refreshed quota observations; Worker OpenCode errors are generic,
  Codex terminal status has no trusted quota classification, and egress does
  not report a signed/bound exhaustion receipt to Core. There is no Campaign /
  Attempt pause or authenticated re-admission API. The contract-aligned next
  surface is migration 0042 with authorization-bound append-only observations,
  plus reservation-fenced pause and authenticated re-admission after in-flight
  reconciliation. Generic HTTP 429 remains insufficient. No live inference or
  fabricated quota evidence was used; dispatch stays fail-closed.
- Home conversational slice: typecheck, lint, 270 Office tests, production
  build, parity check and fixture Chromium captures passed on `6482f0c`. The
  production build reports an existing >500 kB chunk warning. Screenshots at
  1440/834/390/320 px are in
  `docs/evidence/office-conversation-home-2026-10-29/`; fixture data was used,
  so real Core/Compose acceptance was initially `NOT_TESTED`; the deterministic
  Compose harness result below now supplies Core/Compose evidence. Independent
  review passed, and Owner visual sign-off is pending.
- Independent Home review of `523b288`: **BLOCK**, high correctness finding:
  overlapping or late `getIntakeRequest` responses could overwrite the selected
  current thread. Commit `6482f0c` adds a request-generation guard, serial
  timeout polling and deferred-response regressions for stale selection and
  overlapping polls. Exact-SHA re-review passed.
- Home fix candidate `6482f0c`: implementer reports focused Home tests 24/24,
  full Office suite 270/270, typecheck, Biome, build and fixture Chromium PASS.
  The >500 kB chunk warning persists. Deterministic Core/Compose evidence is
  recorded separately below; live-provider and Owner visual acceptance remain
  `NOT_TESTED`.
- Home candidate `6482f0c` ran the canonical deterministic Compose flow with
  `FACTORY_E2E_PROJECT=software-factory-home-e2e` and port 8895. It passed with
  fixture candidate `558756c0ce2959ebff97c89dc8579f5075d53bbf`, Gate
  `ACCEPTED`, restart persistence true, `DETERMINISTIC_FIXTURE`, and
  `paid_inference=false`. Isolated containers and volumes were removed. This is
  not live provider evidence or independent acceptance.
- Draft PR #80 is open against `campaign/office-ux-wave3-20261028`; feature
  review passed. The canonical Compose browser journey exposed a fixture claim
  route-identity bug (locally repaired) and then a missing Lead delegation
  selector option. It is not integrated; visual acceptance remains pending.
- The CAP-050/CAP-052 evidence registry check rejects their `README.md` fixture
  summaries in deterministic JSON evidence lists. A separate branch removes
  those invalid entries while retaining the referenced passing JSON records;
  canonical container architecture governance passed. Draft PR #81 is open;
  rust, Compose policy and container-dev checks are still running.
- Agent Office V3 commits `e31e462`, `782b5d9`, `82b9ee4`: typecheck, Biome
  (141 files), Vitest (268/268), production build, parity validation and fixture
  Chromium at desktop/tablet/mobile passed. The build retains the existing
  ~1.2 MB chunk warning. Captures are committed under
  `docs/evidence/agent-office-isometric-v3-2026-10-29/`. Visual inspection
  found better framing and identity variety, but the scene remains flatter and
  less painterly than the reference. Exact re-review and Owner visual acceptance
  remain pending.
- Independent review of exact Agent Office V3 SHA `82b9ee4`: **PASS**. Reviewer
  verified termination/uniqueness through 200 participants, Core participant
  identity binding, keyboard/accessibility behavior and candid fixture-only
  evidence. The palette rank assignment can change later participants' looks
  when an earlier-sorting identity is added or removed; this is nonblocking.
  Owner visual acceptance remains pending because the scene is materially less
  painterly than the approved reference.
- Independent review of capability-evidence PR #81 exact SHA `13adff7`:
  **APPROVE**. The two deleted README references were fixture-only; CAP-050 and
  CAP-052 retain their own deterministic JSON evidence, so no evidence list is
  emptied or overstated.
- Access exact-SHA review of `2851345` found no credential leak, auth bypass,
  pairing resurrection or current Tailscale path to Core's control listener,
  but returned BLOCK for the socket policy gap. Re-review of `a03dd55` found a
  HIGH alias bypass in the checker, now under repair. The changed evidence
  correctly marks current Unix-socket Serve/HTTPS NOT_TESTED. Remote Tailscale
  access and SEC-01 remain NOT_TESTED.
- No live provider inference ran in this campaign session.
- PR #79 candidate `a8d3488`: 26 UAT harness tests pass; JS syntax, shell syntax
  and diff checks pass. It refuses a positive cap, paid fallback and absent Core
  admission before project/request creation. This is fake-Core preparation
  evidence only; provider inference/billing remains NOT_TESTED.

## Immediate sequence

1. Collect exact funding review of `f4e757a`; continue the separate
   request-level subscription envelope boundary while keeping provider
   inference disabled. Quota recovery remains blocked until a trusted provider
   exhaustion evidence source and append-only observation model exist.
2. Resolve the Home Lead selector journey issue, rerun the canonical
   containerized Chromium journey, rebind PR #80 declarations to its exact
   current SHA, and clear CAP-050/CAP-052 through #81. Keep visual approval
   pending.
3. Reconcile PR #79 only after subscription admission and no-overage evidence
   are represented accurately. Keep the live run gated and draft until reviewed.
4. Re-review Agent Office V3 exact commit `82b9ee4`, clear inherited registry
   CI through #81, and preserve explicit Owner visual sign-off as pending.
5. Collect access repairs in dependency order #36 → #37 → #38. Do not merge the
   stack while high findings or required Owner authorization remain open.
6. Integrate only coherent, reviewed slices into this Wave 3 branch after their
   relevant deterministic gates pass. Do not merge to `main`.

**Independent review:** access `a03dd55` BLOCK HIGH (alias repair candidate
`d0d4040` pending); funding `f4e757a` APPROVE; Agent Office V3 `82b9ee4` PASS;
capability PR #81 `13adff7` APPROVE (container-dev rerun pending on corrected
integration candidate); PR #79 review pending.
**Campaign status:** in progress; Wave 3 is not a certified product release.
