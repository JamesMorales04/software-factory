# Office UX campaign — Wave 2 (2026-10-28)

**Parent:** #39. **Baseline:** `campaign/office-ux-baseline-20261028` at
`f96aff5` (code candidate `e6c2184`/`aad25f3`). **`main`:** `d587252`,
unchanged.

Wave 1 delivered a verified developer-preview integration through Campaign
authorization with a deterministic model substitute. Wave 2 closes the
remaining P0/P1/P2 gaps. This ledger is a plan and status record, not acceptance.

## 1. Highest-priority product journey

**Project → natural-language request → execution profile → assessment → Lead
planning → authorization → Worker execution → candidate → independent
Verifier → semantic Reviewer → Quality Gate → changes/PR inspection**

Wave 1 stops at Campaign creation. Wave 2 must exercise the complete lifecycle
and, where an authorized provider route exists, real inference. Deterministic
fixtures remain valid for lifecycle regression but are not live-provider proof.

## 2. Lanes and ownership

| Lane | Branch | Scope | Owner | Depends on |
|---|---|---|---|---|
| R — Role routing | `campaign/office-ux-routing-20261028` | Per-role provider/model gateways; model catalog/discovery; profile executability; Lead/Reviewer/Implementer routes | OpenCode | Baseline |
| L — Full lifecycle | `campaign/office-ux-lifecycle-e2e-20261028` | Extend intake browser journey through Worker, candidate, verifier, reviewer, gate; fix discovered product defects | OpenCode | Baseline |
| P — Projects/GitHub | `campaign/office-ux-projects-20261028` | GitHub App/fine-grained token onboarding, private/public repository listing, local import through approved roots, project summary, extensible capability discovery | OpenCode | Baseline |
| D — Decisions | `campaign/office-ux-decisions-20261028` | Actionable Decision Inbox items, stale-revision guards, confirmation, typed Core operations where missing | OpenCode | Baseline |
| F — Campaign diff/PR | `campaign/office-ux-diff-20261028` | Bounded file-body diff viewer, exact revisions, GitHub PR/checks/comments/review/authorized merge | OpenCode | Baseline; shared GitHub client with P |
| J — i18n sweep | `campaign/office-ux-i18n-sweep-20261028` | EN/ES for shell/nav/Home/Request/Actions/Lead/Campaigns/AgentOffice/Issues; 390/320 layouts | OpenCode | Baseline |
| A — Autonomous intake | later, based on R/P/intake | Eligibility policy, bounded issue queue, campaign proposals/execution under explicit preauthentication | OpenCode/Codex review | R/P + GitHub Issues |

Shared-file owners: the Campaign Director resolves `native/src/bin/core.rs`,
`native/src/lib.rs`, `native/src/db/mod.rs`, migrations, `office/src/coreApi.ts`,
`App.tsx`, `navigation.ts`, shared CSS and parity JSON. Lane writers commit
small coherent changes; no two writers share a worktree.

## 3. Compose acceptance

The repository's `Compose end-to-end (manual)` workflow was dispatched on the
baseline ref. It runs `scripts/compose-e2e.sh` on a GitHub-hosted rootless
Podman runner, including Core/Worker/verifier/reviewer/gate and restart
persistence. It is the supported substitute for the local host, where buildah's
context overlay cannot mount on btrfs. The exact run URL/result is recorded in
the parent issue and final evidence, never inferred from unit tests.

## 4. Role-routing acceptance

- A profile revision may assign distinct supported providers/models to Lead,
  Implementer and Semantic Reviewer; the deterministic verifier is not a model
  role.
- Assessment uses the frozen Lead route; semantic review uses the campaign's
  frozen Reviewer route; Campaign authorization uses the Implementer route.
- A profile is executable only when each declared model role can actually be
  routed with the configured credential/base URL and adapter. Unsupported
  adapters/models remain explicit, non-executable drafts.
- Provider model discovery is real where supported; no invented model IDs.
  Manual IDs are advanced and labelled unverified.
- Immutable profile revisions and exact frozen binding remain mandatory.
- Budget intersection includes role route caps, profile campaign caps, plan,
  delegation, provider policy and Core policy.

## 5. Lifecycle acceptance

For an intake-originated Campaign, the test must observe:

1. exact frozen profile/plan/action/spec identity;
2. Worker claim/lease and recorded attempts;
3. candidate commit/tree identity;
4. independent verifier receipt bound to that candidate;
5. semantic reviewer receipt/judgment with its own route;
6. deterministic Quality Gate outcome;
7. changed-file/diff projection and, where authorized, PR/check inspection;
8. restart/recovery persistence.

A fixture-provider run is `DETERMINISTIC_VERIFIED`, not live-provider
certification. Live inference requires an authorized provider credential and a
fresh bounded budget envelope; absence is `BLOCKED`, not fabricated.

## 6. P1/P2 acceptance

- Projects: authorized GitHub repository discovery (public/private), secure
  Core-governed clone, local import via approved roots, human project identity,
  health/summary and evidence-based toolchain discovery.
- Decisions: real actions, exact revisions, confirmation and typed Core ops.
- Diff/PR: bounded file bodies, exact revisions, branch protection respected,
  no auto-merge.
- i18n/mobile: stable catalogs, persisted locale, full owned surfaces,
  1440/834/390/320 browser checks; QR pairing preserved.
- Autonomous intake: explicit eligibility rules, durable queue, semantic
  classification, risk/dependency checks, bounded profile/default selection,
  approval policy and adversarial negative tests. No arbitrary external issue
  execution.

## 7. Non-negotiable boundaries

- Core owns state, authorization, budgets, leases, evidence, gate and delivery.
- No model/Office-created authority; no profile self-widening.
- No plaintext secrets; no host Podman socket; no privileged containers.
- No fabricated metrics, provider access, tests, acceptance or PR state.
- `main` stays unchanged until independent review, protection checks and Product
  Owner merge authorization.

## 8. Status — 2026-10-28

Wave 2 starts from `924e026` (design doc later preserved at baseline `de7e2b7`).
No lane result is accepted yet.

### Active implementation lanes
- R `campaign/office-ux-routing-20261028`
- L `campaign/office-ux-lifecycle-e2e-20261028`
- P `campaign/office-ux-projects-20261028`
- D `campaign/office-ux-decisions-20261028`
- F `campaign/office-ux-diff-20261028`
- J `campaign/office-ux-i18n-sweep-20261028`
- A `campaign/office-ux-autonomy-20261028` (commits 1–2 only; automatic start disabled)

### Compose acceptance fix
The first supported CI run failed because `compose.yaml` hard-codes
`name: software-factory`, overriding the E2E project guard and defeating the
dedicated-project check. `scripts/compose-e2e.sh` now passes an explicit
`-p "$PROJECT"` on every invocation and prints container diagnostics on failure.
Branch `campaign/office-ux-compose-fix-20261028` at `770a200`; a fresh CI run is
pending. This was a real isolation defect, not merely a test failure.

### Live-provider local diagnostic
`factory-provider-probe opencode /tmp/opencode/provider-probe-workspace` against
the host OpenCode 2.2.0 failed with `provider protocol incompatibility: OpenCode
startup exceeded 20 output lines`. The installed host OpenCode is not the pinned
provider adapter version. Live local inference is therefore **BLOCKED** on a
compatible containerized provider runtime plus an owner-supplied credential and
fresh bounded spend authorization. The deterministic fake-model and fixture
journeys are not substitutes for live certification.

### Autonomy design
Codex Sol produced `office-ux-autonomy-contract-2026-10-28.md` (commit
`271c821`, preserved on the baseline at `de7e2b7`). It keeps automatic
execution blocked behind a protected ADR and finite preauthorization. GPT-6
Luna independently reviewed the contract and found HIGH gaps in start-authority
ownership, immutable issue/author/repository binding, enforced confinement,
reservation accounting and revocation-to-provider fencing. Commits 1–2 remain
safe only as default-off policy/acknowledgement/queue/advisory work. Commit 3
(automatic start) is **blocked** pending a protected ADR amendment, owner
approval, Core-owned envelope validation, enforced confinement, durable
accounting and independent review. See
[office-ux-autonomy-review-2026-10-28.md](office-ux-autonomy-review-2026-10-28.md).
Wave 2 starts 2026-10-28. No lane result is accepted yet.

### Lane L — actual result (2026-10-28)

Candidate `5e7f8657a3a4e6c63b37876a1ae0817a4cc9162f` on
`campaign/office-ux-lifecycle-e2e-20261028` extends
`office/e2e-browser/intake-journey.mjs` and exposes
`cargo xtask e2e intake-full`. The deterministic fixture journey passes:

- request `e1157165-1e6f-4dbe-880f-837b7b446d69` → Campaign
  `4fd4de11-7462-489f-9089-69b2a100fd6e`;
- frozen profile revision `d5cdf397-35f9-4de7-b4da-12467cefab67`, plan
  `9e10dc489ebcc987d3159d8d002b55288fe92f2d81d46773e07a8aa6b904d48a`,
  Action `dccd91d6-588d-4e7e-8df5-6f3672a08ee2`;
- Worker claim generation 1 on Lead Attempt
  `257b2a8d-a82d-484e-8902-a75b602d4b97`, candidate
  `d6a8d992-c1eb-4f4c-9e55-7ecc60122468` at commit
  `b12a38bbb46aed13d28d295b1d3a4149a4c986ca`;
- verifier and `MODEL` semantic-reviewer receipts `PASS`, deterministic Gate
  `ACCEPTED`, changed path `src/provider.txt`;
- Core restart and browser reload preserved the exact request, Campaign,
  candidate, receipts and Gate.

The lifecycle was re-run after merging into `campaign/office-ux-wave2-integration-20261028`
at implementation `bb2ce6b` and passed again (request `51d5d5e2…`, Campaign
`92db9d55…`, candidate `d23f6db0…`, gate `ACCEPTED`, restart/reload
persistence). This is still deterministic fixture evidence, not live inference
or independent acceptance.

The journey exposed and repaired one projection defect: intake **detail** now
exposes authoritative `campaignStatus` and `gateOutcome` instead of only the list
projection. Live provider inference, independent verification and delivery
remain `NOT_TESTED`; see
[the candidate-bound evidence](../evidence/office-ux-intake-lifecycle-2026-10-28.md).

### Lane L follow-up — Compose intake (2026-10-28)

Candidate `056c50248e0b74a84be2070e6e148df5348c9e36` extends the supported
Compose acceptance with a deterministic E2E-only model fixture and the full
intake lifecycle. The `Compose end-to-end (manual)` workflow run
[`40742928074`](https://github.com/JamesMorales04/software-factory/actions/runs/40742928074)
passed for that exact SHA. It exercised request
`bfafdc1b-8d2a-4e7c-833a-dfa8b761d72a`, Campaign
`7464a25e-ecae-4688-85da-8d003c8f58e2`, candidate
`be2c44cd-eb84-4089-b235-b0d0603231ec` at commit
`777de0cb5df42219686dba6574500e417468916b`, independent verifier/reviewer
receipts and deterministic Gate `ACCEPTED`, followed by `compose down`/`up`
persistence. The model and coding provider are deterministic fixtures; live
inference and Compose Office/Chromium acceptance remain `NOT_TESTED`. See
[the Compose evidence](../evidence/office-ux-compose-intake-2026-10-28.md).
