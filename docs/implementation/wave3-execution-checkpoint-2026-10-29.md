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
| Subscription funding V3 | `sf-wave3/subscription-funding` / `factory/subscription-funding/20261029` | Commit `c0eb767d84d774362c7bed27fc5a7834fbbc2436`, clean | Exact-SHA independent review active; 45 focused Core/Worker tests, fmt and strict Clippy reported PASS by author | Resolve any review finding; preserve typed refusal, Worker hold and auth/lease/fencing; keep dispatch disabled |
| Conversational Home (#62) | `sf-wave3/ux-conversation` / `factory/office-conversation/20261029` | PR #80 at `f310192c33a02d9191fd9cfbcaf90264c2c6e8cf` | Canonical Compose Office/browser gates passed; exact review and hosted CI pending | Obtain exact-SHA review and resolve CI; keep visual sign-off pending |
| Agent Office V3 (#65/#70) | `sf-wave3/isometric-v3` / `factory/office-isometric-v3/20261029` | PR #82 at `82b9ee4703326947a9a3c78153d9bf1994a3a960`; source `e31e46298b42f13e80701913b43878336587ccb8`, captures `782b5d9dbe484f41afa12120667a07f248703bb0` | Frontend lane reassigned to `home_conversation`; V3 visual spike active; prior exact candidate review PASS | Compare reference and implement bounded visual refinements; preserve Owner visual sign-off |
| Access repairs (#36–#38) | `sf-wave3/access-repairs` / `factory/access-repairs/20261029` | #38 cumulative head `ad04aca35d0538c614f0f957ea42929895fd63a0`; #37 policy-only candidate `da2f07a58d0dfe638786dedf12af7c9cb598c4e9` | Exact #38 review PASS; hosted architecture/Compose pass, Rust/container-dev pending; #37 Rust/container-dev pending | Collect remaining hosted checks; keep stack Draft, no promotion; remote Serve/SEC-01 NOT_TESTED |
| Provider acceptance / PR #79 | `sf-wave3/provider-acceptance` / `factory/provider-acceptance/20261029` | Candidate `6818b30aa7066f366c3ec2496e599dc6e475a60a`, pushed | Exact review BLOCK HIGH: exported fake runner trusts forgeable `simulationTransport` flag; wrapper could delegate to real Core; repair active | Remove fake-runner authority from production module, block transport delegation, defer evidence-dir creation, add wrapper/zero-side-effect regressions; re-review exact SHA |
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
- #79 — subscription-only provider UAT preparation: open draft; current commit
  `6818b30` is pushed. It requires explicit valid charge amounts before showing
  zero, separates subscription billing-source observations, omits arbitrary
  Core error bodies, rejects paid modes/spend caps, and blocks live execution
  before all resource creation when request admission is unverified. Fake
  downstream flows are marked `SIMULATION`. Harness tests pass 28/28; Node and
  Bash syntax, diff, impact declarations for event base `f0f9a24` / head
  `6818b30`, and Podman config/quiet/service listing passed without starting
  services. The HIGH in `f6af062` was partly repaired: live `main` ignores the
  old simulation option, but independent re-review found the exported fake
  runner trusts a caller-set transport flag, allowing a real-Core wrapper to
  emit simulated evidence. The candidate is blocked again; a stronger module
  boundary and no-evidence-directory regression are being implemented. No
  live inference/billing.
- #80 — conversational Home: open draft against Wave 3 at
  `f310192c33a02d9191fd9cfbcaf90264c2c6e8cf`. Typecheck, Biome (four existing
  CSS `!important` warnings), Vitest (12 files / 51 tests), production build
  (existing large-chunk warning), and canonical Compose `cargo xtask e2e office`
  passed. The deterministic candidate `c5d55619261738bdb8c65f506ee64f1c438801b2`
  returned ACCEPTED after authenticated request-scoped `/approve`; the browser
  reached an approved/pending Campaign with no provider/session/model/tool
  calls. V1 grants are shown as historical and excluded from Lead authority.
  Responsive screenshots and parity are updated. Exact review and hosted CI
  remain pending; architecture CI reports inherited #81 CAP-050/CAP-052 registry
  entries. Visual sign-off and real subscription assessment are not accepted or
  tested.
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
- #36 → #37 → #38 — existing draft stack preserved. PR #38 repair is pushed at
  `ad04aca` with unchanged base. Accepted ADR-0026 is restored byte-for-byte;
  proposed ADR-0030 now records access-epoch, challenge-cancellation and legacy
  token cleanup decisions. Normal merge `9ef246f` includes reviewed #37 guard
  `da2f07a` while preserving `0c3fd44` ancestry. Exact review PASS; hosted Rust
  and container-dev remain pending. Review of
  `2851345` found a Major Compose policy gap, addressed in `d2fa767`/`a03dd55`;
  re-review found HIGH named-volume and unnamed-external alias bypasses.
  Commits `d0d4040` and `d2783e4` resolve canonical, project-derived, explicit,
  external-name and implicit-external identities. Commit `48a1346` tests both
  canonical and physical-key external source spellings plus named aliases in
  Worker; `0c3fd44` records the evidence. Author reports base and overlay policy
  suites pass (27 each; five overlay-only cases skipped on base). Exact review
  PASS at `0c3fd44`; reviewer verified both source spellings resolve as
  protected but could not rerun suites because Podman Compose lacks CI's JSON
  config format and Tailscale origin was unset. Current Tailscale
  authentication, Serve and remote HTTPS remain NOT_TESTED; Owner authorization
  is pending.

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
  HIGH alias bypasses in the checker. `d0d4040`/`d2783e4` resolve explicit,
  named, implicit-external and physical-key external aliases; `0c3fd44` records
  27-test base/overlay runs and marks current Unix-socket Serve/HTTPS NOT_TESTED.
  Exact review PASS; its full suite rerun was unavailable in the reviewer
  environment. Remote Tailscale access and SEC-01 remain NOT_TESTED.
- No live provider inference ran in this campaign session.
- PR #79 candidate `a8d3488` was independently BLOCKED for null/source-only
  verified-zero accounting and arbitrary Core error-body secret leakage. Both
  HIGH findings are repaired in `c5c90ce`; 27/27 harness tests and syntax/
  whitespace checks pass. Independent re-review BLOCKED another HIGH: if
  provider verification exists but request-level Lead admission fails, the
  harness may create Project/profile before `/intake/requests` refuses. A
  bounded Sol review found no existing read-only Core operation can adjudicate
  the exact request before Project creation; a preview alone cannot fence stale
  or revoked authority. Real UAT must remain blocked until Core implements a
  transactional create-and-admit boundary binding principal, objective,
  repository/base, config, exact profile, explicit approval, assessment
  authority, funding evidence and idempotency. Harness repair will block before
  resources and label fake downstream flows `SIMULATION`. Podman config and six
  negative mutations passed without starting services; fixture image is
  digest-pinned. No live inference or billing occurred.

## Earlier sequence (superseded by Director update below)

This sequence describes the campaign before the Director update and is kept as
provenance. Current executable tasks are listed below.

**Independent review:** access cumulative candidate `0c3fd44` PASS (reviewer
suite rerun limitation recorded); #37 policy-only candidate `da2f07a` APPROVE;
funding `f4e757a` APPROVE; current funding `c0eb767` review pending; Agent Office V3 `82b9ee4` PASS;
capability PR #81 `13adff7` APPROVE (container-dev rerun pending on corrected
integration candidate); PR #79 `c5c90ce` BLOCK HIGH (Core atomic create-and-admit
is missing; fake/live separation repair active); PR #79 `f6af062` BLOCK HIGH
(exported simulation mode can use real Core); PR #79 `6818b30` BLOCK HIGH (fake
transport marker can be forged by a real-Core wrapper); new repair review
pending. PR #80 `f310192` exact review pending; PR #38 `ad04aca` PASS.
**Campaign status:** in progress; Wave 3 is not a certified product release.

## Director update — 2026-10-29 19:20 UTC (revised 19:29 UTC)

- Remote integration remains exactly `f7479c0c5f936d1097426dd1293ef43ea6e72fb6`; local checkpoint branch contains documentation-only checkpoint commits. `main` is unchanged.
- PR #37 policy guard `da2f07a58d0dfe638786dedf12af7c9cb598c4e9` has an exact independent APPROVE and was pushed as a normal fast-forward. Hosted architecture-consistency and compose-policy now pass; Rust/container-dev remain in progress. No merge or promotion has occurred.
- PR #38 repair is pushed at `ad04aca35d0538c614f0f957ea42929895fd63a0`, with base unchanged and still OPEN/Draft. It restores accepted ADR-0026 byte-for-byte, records the new policy in proposed ADR-0030, and includes #37 candidate `da2f07a` through merge commit `9ef246f` while preserving `0c3fd44` ancestry. Exact-SHA review **PASS**; local architecture, format, strict Clippy, focused Rust, and Compose-policy checks passed; hosted architecture and Compose-policy pass, while Rust/container-dev remain pending. Current Serve/HTTPS behavior and SEC-01 are NOT_TESTED.
- PR #79 is pushed at `6818b30aa7066f366c3ec2496e599dc6e475a60a`. Exact review still BLOCK HIGH: the exported fake runner trusts forgeable `simulationTransport`, so a real-Core wrapper can produce simulated evidence. The ordinary live entrypoint ignores old simulation options, and 28 tests plus syntax/diff/impact/Podman config gates pass, but the runner boundary and empty evidence-directory side effect need repair. Hosted architecture flags inherited CAP-050/CAP-052; newer runs are queued. No live inference or billing occurred.
- PR #80 is pushed at `f310192c33a02d9191fd9cfbcaf90264c2c6e8cf`; its canonical deterministic browser journey, request-scoped approval, Office typecheck/Biome/Vitest/build and screenshots passed with no provider/session/model/tool calls. Exact review and hosted CI are pending; architecture CI reports the inherited #81 CAP-050/CAP-052 evidence entries. Visual acceptance is pending.
- Funding V3 Rust slice is committed at `c0eb767d84d774362c7bed27fc5a7834fbbc2436` on the existing subscription-funding branch. It shares typed refusal/error codes across Core/Worker, orders Worker funding before monetary prerequisites while preserving the existing subscription hold, and keeps dispatch disabled. Author reports 45 focused tests, fmt and strict Clippy passed; independent exact-SHA review is active. Quota pause/recovery remains blocked on trusted provider exhaustion evidence and durable Core recovery APIs.
- No active worktree was reset or discarded. Existing `office/node_modules`, unrelated root untracked files, and long-running services remain untouched.

### Next executable tasks

1. Collect the exact-SHA independent funding review; repair any issue without enabling dispatch.
2. Repair PR #79’s forgeable fake transport boundary and evidence-directory side effect; add negative coverage, rerun gates, and obtain fresh exact-SHA review without enabling live UAT.
3. Obtain exact-SHA review and hosted CI for PR #80; repair any finding while preserving the request-scoped approval flow and fixture boundary.
4. Collect PR #38 remaining hosted checks; repair any finding while preserving #37/#38 ancestry. Keep the stack Draft and unpromoted.
5. After the above complete or free capacity, resume the independent Agent Office V3 visual lane and #81 registry repair/integration checks. Preserve Owner visual acceptance as pending.
