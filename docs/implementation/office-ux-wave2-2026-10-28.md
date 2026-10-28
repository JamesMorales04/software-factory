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

## 8. Status

Wave 2 starts 2026-10-28. No lane result is accepted yet.

### Lane R — role routing (implementation result, 2026-10-28)

Exact per-role routing is implemented on
`campaign/office-ux-routing-20261028`. The frozen Lead route serves intake
assessment, the frozen Semantic Reviewer route is resolved from candidate →
Campaign → immutable start evidence, and the implementer route remains the
Campaign authorization route. Profiles are executable only when every role’s
provider, credential, adapter and substantiated model resolve. Codex remains an
implementer-only adapter; a Codex Lead/reviewer route is a truthful
non-executable draft. Provider model discovery is a real authenticated
`GET /settings/providers/{provider}/models` call with typed `UNAVAILABLE`
outcomes and a process-local cache.

Recorded evidence is limited to Rust module/API tests and Office component
tests. No live provider call, Compose run or browser acceptance was performed
for this lane; see
[role routing](office-ux-role-routing-2026-10-28.md). Independent acceptance
remains pending.
