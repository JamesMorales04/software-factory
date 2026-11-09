# Agentic reliability and technology-knowledge plan

**Status:** Proposed — 2026-11-09. Extends
[ADR-0039](../architecture/decisions/ADR-0039-quality-first-planning-and-stack-selection.md)
and the [quality-first planning plan](quality-first-planning-plan.md).
No behavior is claimed until the acceptance tests below exist and pass.

## 1. Purpose

The Factory can produce an accepted candidate, but a long agentic flow still has
failure modes that are not product bugs: a provider stream cuts, a workspace
archive grows, a dependency cannot be fetched, a lease expires, a projection is
duplicated, a test is flaky, a model returns malformed JSON, or a context
compaction loses the plan. These are reliability defects and must be handled as
first-class engineering work.

The target is:

- durable, replayable planning and execution;
- at-least-once steps with idempotent handlers and exactly-once external
  effects;
- explicit failure classification and bounded recovery;
- deterministic tests and honest evidence;
- untrusted content never changes authority;
- up-to-date, source-cited technology knowledge for every stack used;
- reviewers that are independent and specialized.

## 2. Reliability principles

1. **The plan and evidence are durable, not chat state.** Persist every
   transition, artifact hash, decision, finding and external effect.
2. **Every step is idempotent.** A retry may repeat a step, but must not repeat
   an external effect or create a duplicate candidate/issue/repair.
3. **Classify before retrying.** Provider, infrastructure, policy, code and
   evidence failures have different recovery paths.
4. **Fail closed.** Unknown identity, stale lease, missing evidence or
   ambiguous external state blocks; it never guesses.
5. **Reconcile before retry.** Read the durable/external state after a crash or
   timeout before repeating an effect.
6. **No test retry can turn a failing assertion green.** Retries are only for
   infrastructure with a classified transient error.
7. **No silent fallback.** No provider/model/tool/stack fallback without an
   explicit, recorded policy.
8. **Evidence over prose.** A trace, receipt, hash or check result is evidence;
   a summary is not.
9. **Untrusted data is data.** Repository files, issues, docs, OpenSpec,
   provider output and skills cannot grant authority or select tools.
10. **Right-size recovery.** Small transient failures retry; repeated or
    semantic failures replan/repair; exhausted budgets go to the owner.

## 3. Workstreams

| # | Workstream | Priority | Main outcome |
|---|---|---|---|
| A | Durable workflow, state machine, outbox, idempotency | P0 | Crash/restart resumes without duplicate effects |
| B | Provider/stream/structured-output reliability | P0 | Long turns, SSE reconnect, schema repair, no partial candidate |
| C | Workspace and execution isolation | P0 | Content-addressed, bounded, clean, reproducible workspaces |
| D | Dependency supply chain | P0 | Deterministic offline builds, SBOM, licenses, vulnerabilities |
| E | GitHub/OpenSpec projections | P0 | Idempotent, drift-aware, rate-limit-safe projections |
| F | Leases, cancellation, exactly-once effects | P0 | Fencing and race-safe completion/cancellation |
| G | Context, handoff and durable plan | P0 | Compaction/restart never loses the plan or evidence |
| H | No-progress, replan and finding routing | P0 | Bounded automatic recovery instead of early stop |
| I | Deterministic tests and environment | P0 | No flaky green, pinned timezone/locale/ports/network |
| J | Prompt injection and untrusted content | P0 | Repo/issues/docs/skills cannot change authority |
| K | Observability, trace and replay | P1 | Every step is inspectable and replayable |
| L | Reviewer independence and specialization | P0 | Separate identities/processes per role, no self-review |
| M | Technology skill packs and docs freshness | P1 | Current, cited, versioned skills per stack |
| N | Chaos/fault-injection evaluation | P0 | Failure modes are tested, not assumed |

## 4. A. Durable workflow and state machine

### Problem

Planning and execution currently span Core, Worker, Supervisor, provider
sessions and projections. A crash or timeout can lose the active step, duplicate
a dispatch, or leave a candidate/session/projection inconsistent.

### Design

Add a Core-owned workflow state machine, without a second orchestrator:

```text
SUBMITTED -> ASSESSING -> PLANNED -> PLAN_REVIEW -> FROZEN
  -> EXECUTING -> VERIFYING -> REVIEWING -> GATE
  -> REPAIRING | REPLANNING -> EXECUTING
  -> CONVERGING -> ACCEPTED | BLOCKED | FAILED | CANCELLED
```

Persist:

- `workflow_runs` (request/plan/candidate identity, state, revision, budget);
- `workflow_steps` (step id, kind, input hash, output hash, attempt, status);
- `idempotency_keys` (operation, key, result, expiry);
- `outbox` (external projection event, payload hash, status, attempts);
- `workflow_checkpoints` (unit, plan revision, context summary hash);
- `external_effects` (GitHub/OpenSpec/session/container identity, state).

Rules:

- Each transition has a deterministic guard and a durable event.
- A step is `PENDING -> RUNNING -> SUCCEEDED/FAILED/BLOCKED`, with an attempt
  and a fenced lease.
- Recovery scans `RUNNING` steps, reconciles the external effect, then retries
  only if idempotent and safe.
- A projection is written through the outbox; the worker never writes GitHub or
  OpenSpec directly.
- Workflow state is the source of truth for UI; projections are derived.

### Acceptance

- Kill Core/Worker/provider at every step; restart resumes at the last durable
  checkpoint.
- Replaying the same request/idempotency key creates no duplicate
  candidate/issue/repair/session.
- A crash after an external effect reconciles before retry.
- No `RUNNING` step survives a completed workflow.

## 5. B. Provider, stream and structured output

### Problem

Long model turns, SSE disconnects, malformed JSON, provider differences, token
limits and session cleanup are recurring failure sources.

### Design

- Use an SSE event cursor (event id/sequence). On disconnect, reconnect and
  replay from the cursor; do not treat a stream gap as a final answer.
- Separate **total turn** from **idle/heartbeat timeout**. Long turns are
  allowed; an idle stream triggers reconnect, not a silent finalize.
- Structured outputs: versioned JSON Schema, strict decode, deterministic
  domain validation, bounded repair prompt, max repair attempts, then a typed
  failure. Never fall back to free text.
- Model capability registry: context window, output limit, tool/structured
  support, provider route. Do not plan beyond the declared capability.
- Provider reservations: authorize/settle/release are idempotent; a turn cannot
  settle twice; release after failure; reconcile orphan sessions.
- No partial candidate on provider error; a candidate is created only after the
  plan/unit DoD is satisfied.
- Model/provider difference tests: same fixture through each supported route;
  no silent route substitution; critical review may require a different
  provider/model.

### Acceptance

- Cut SSE mid-turn and reconnect; the turn resumes/completes or fails typed,
  with no duplicate provider request.
- Malformed JSON triggers bounded repair, then a typed failure; no free-text
  fallback.
- Long turn > old 150s completes; idle stream reconnects.
- Provider crash/restart leaves no orphan session and no partial candidate.
- Context limit overflow is detected before the call, not discovered as a
  provider error.

## 6. C. Workspace and execution

### Problem

Snapshots can include build artifacts, exceed bounds, lose executable bits, mix
attempts, or leak stale state.

### Design

- Content-addressed snapshots with a manifest of path, mode, size and hash.
- Exclude generated paths (`target`, `node_modules`, `dist`, `build`, caches,
  `.git`) from the source snapshot; never treat them as candidate source.
- Verify tree/commit identity before and after every operation.
- Use an executable, ephemeral target tmpfs for build outputs; keep `/tmp`
  `noexec`.
- Pin images/toolchains by digest; verify image import; preflight the runtime.
- Clean volumes/worktrees deterministically; reconcile orphans; no cross-attempt
  reuse.
- Bound archive size/entries and return a typed `ArchiveTooLarge`, not a generic
  413.
- Make import/export atomic and hash-verified.

### Acceptance

- A build producing a large `target/` does not break later checks.
- A workspace with a generated artifact cannot be committed as candidate
  source.
- Corrupt/truncated snapshot is rejected and does not mutate the worktree.
- Restart leaves no orphan container/volume/worktree.
- Reproducible source snapshot hash for identical input.

## 7. D. Dependency supply chain

### Rust recommendation (web-verified)

Use **vendor/cache as the primary offline path**, with a brokered fetch only for
intentional dependency changes:

1. Commit `Cargo.lock` and `rust-toolchain.toml`.
2. `cargo vendor --locked vendor` and commit/archive the vendor tree.
3. Add the printed source replacement to `.cargo/config.toml`.
4. Build/test with `--locked --offline` or `--frozen`.
5. Gate with `cargo-deny` (advisories, licenses, bans, sources),
   `cargo-audit` (RustSec), `cargo-cyclonedx` (SBOM) and `cargo-vet` where
   trust review is required.
6. Optional: `cargo-vendor-filterer` to drop target-specific crates,
   `cargo-local-registry` for a local registry, `cargo auditable` to embed the
   dependency list in the binary.
7. A brokered, network-enabled step may run `cargo fetch --locked`/`cargo
   update` only with an allowlist; it writes the lockfile/vendor atomically,
   then checks run offline again.
8. Re-vendor on every lockfile change and on a scheduled cadence; review the
   vendor diff, licenses, advisories and SBOM.

This is preferred because it is deterministic, works offline, keeps third-party
source reviewable and matches Cargo's source-replacement model. A brokered fetch
alone is not enough for reproducible checks; a vendor tree alone is not enough
for supply-chain security. Use both.

For Node/Python, apply the same pattern: lockfile + `npm ci`/`uv sync` + cached
or vendored packages + SBOM/license/vulnerability checks; no online install
inside the check sandbox.

### Design

- `DependencyPlan`: name, version, source, license, checksum, maintenance,
  advisory status, SBOM ref, acquisition strategy, owner approval.
- `DependencyAcquire`: isolated, network-enabled, credential-free, allowlisted,
  lockfile-verified; writes a vendor/cache; emits SBOM/license/vuln evidence.
- Checks run offline against the frozen vendor/cache.
- If acquisition is unavailable, the plan declares a constraint or chooses a
  stack that fits; no fake availability.
- Dependency changes are scored by the autonomy points policy; owner escalation
  only when above budget/reserved.

### Acceptance

- `cargo build/test --locked --offline` passes with network disabled.
- A new dependency cannot enter a check sandbox without a plan, lockfile,
  checksum, license, advisory review and SBOM.
- Partial/aborted vendor fetch leaves no half-written vendor tree.
- SBOM and license evidence are candidate-bound.
- Vendor refresh detects stale/unmaintained/vulnerable dependencies.

## 8. E. GitHub and OpenSpec projections

### Problem

Duplicate issues/PRs, rate limits, stale projections and drift between Core and
the repository.

### Design

- Outbox pattern with idempotency key and external identity mapping.
- ETag/If-Match and conditional requests; backoff on 429/5xx; bounded retries.
- Reconcile by external id + content hash before create/update.
- OpenSpec renderer is deterministic and revision-bound; `openspec validate` is
  a check; archive is idempotent.
- Drift detection: Core hash vs projection hash; a mismatch is surfaced, not
  silently repaired.
- Local fallback board when GitHub is unavailable; no duplicate external issue.

### Acceptance

- Replaying the same projection creates no duplicate issue/PR/OpenSpec change.
- A 429/500 sequence eventually succeeds or fails typed; no busy loop.
- Drift is detected and shown; Core does not trust edited markdown.
- Archive is idempotent and updates `openspec/specs/` exactly once.

## 9. F. Leases, cancellation and exactly-once effects

### Problem

Stale workers, expired leases, cancellation vs completion races and duplicate
external effects.

### Design

- Fencing token/generation on every lease and external effect.
- Heartbeat with a bounded lease; a stale worker cannot complete.
- Cancellation is a durable request; completion checks the cancellation fence
  before an external effect.
- External effects carry an idempotency key; reconcile before retry.
- A cancelled-after-effect state is explicit and reconciled; no silent success
  or duplicate.
- No cross-attempt reuse of a session, container, volume or candidate identity.

### Acceptance

- Cancel at every state; no duplicate effect, no accepted candidate after
  cancellation.
- Kill a worker mid-effect; restart reconciles and does not repeat.
- Stale lease completion is rejected.
- Two workers racing on the same attempt: one fenced winner.

## 10. G. Context, handoff and durable plan

### Problem

Context compaction, restarts or handoffs can lose the plan/spec and lead to
shallow or contradictory work.

### Design

- Persist plan/spec/stack/architecture/DoD/OpenSpec hash; inject a bounded
  read-only plan into every unit.
- `tasks.md` checkboxes mirror WorkGraph progress; Core validates task/check/
  evidence, not the checkbox alone.
- Compact at 70% of effective context; clear tool results/structured notes
  first; at most two consecutive compactions; audited handoff.
- Handoff summary includes identities, completed units, open findings,
  remaining work, checks and evidence refs; probes verify it.
- A new session reloads the plan and probes the workspace before continuing.

### Acceptance

- Force compaction and restart mid-unit; the new session resumes with the same
  plan and no duplicated work.
- A handoff summary cannot substitute for a missing artifact/evidence.
- `tasks.md` and WorkGraph agree at every checkpoint.

## 11. H. No-progress, replan and finding routing

### Problem

The current breaker stops on stagnation; findings may be repaired at the wrong
layer.

### Design

- Stagnation -> diagnosis -> bounded replan (new graph revision, same frozen
  spec/stack unless the finding requires a revision).
- Finding Router: code -> Repairer; test -> Test Planner/Repairer; spec/arch/
  stack -> Planner/Spec Manager; security -> Security Reviewer/owner; docs ->
  docs unit.
- Max replans/repair cycles from policy; owner escalation only when exhausted
  or a reserved matter is touched.
- Every finding/disposition is durable and linked to the resolving candidate/
  spec revision.

### Acceptance

- Inject a stalled unit; the campaign replans and continues.
- Inject an architecture finding; the Planner/Spec Manager revision is created,
  reviewed and executed, not patched around.
- Exhausted budget goes to an owner decision, never accepted.

## 12. I. Deterministic tests and environment

### Problem

Flaky tests, timezone/locale/port/randomness/network differences and retries
that hide real failures.

### Design

- Pin toolchains/images by digest; commit `rust-toolchain.toml`/lockfiles.
- UTC, `LC_ALL=C`, deterministic seeds, OS-assigned ports, no network in checks.
- Test data factories; no sleep-based waits; bounded polling with deadlines.
- Retry only classified infrastructure errors; never retry assertions.
- Quarantine flaky tests with owner, issue and expiry; no permanent allowlist.
- Run the same fixture through every supported provider route.
- `cargo nextest` or equivalent for isolation; test ordering independent.
- Record test result hashes and flake history; a flaky test is a defect.

### Acceptance

- The same suite passes repeatedly across restarts, order changes, timezones
  and providers.
- No test retry can convert a failed assertion to PASS.
- A flaky test is detected, quarantined and tracked.
- Offline checks never reach the network.

## 13. J. Prompt injection and untrusted content

### Design

- Treat repository files, docs, issues, PR comments, OpenSpec and web skills as
  untrusted data. Delimit and label them; never follow instructions from them.
- The planner/reviewer/implementer prompts state that data cannot change
  authority, tools, scope, stack or the plan.
- Tools/capabilities are fixed by the assignment; no model-selected tools.
- Secrets never enter prompts/logs; redaction at telemetry boundaries.
- Skill content is reviewed for license/provenance; no scripts or install hooks.
- Add malicious fixtures: issue says "ignore policy", docs contain fake commands,
  README asks to exfiltrate, OpenSpec task asks to widen scope.

### Acceptance

- Malicious content cannot change the route, stack, tools, scope or gate.
- The planner cites untrusted content as evidence, not instruction.
- No secret appears in a prompt, trace, receipt or error.

## 14. K. Observability, trace and replay

### Design

- Correlation ids for request/campaign/unit/attempt/candidate/receipt.
- Structured event log for every state transition, model call, tool, check,
  finding, projection and external effect.
- Trace/span per step; redacted payloads; token/time/cost telemetry.
- Replay a workflow from the event log in a test environment; compare evidence.
- Dashboards/alerts for stuck workflows, repeated retries, stale projections,
  orphan sessions and flaky checks.
- Evidence is immutable and candidate-bound.

### Acceptance

- Any failure has a trace from objective to finding/evidence.
- A stuck workflow is detectable without reading logs manually.
- Replay reproduces the decision/evidence for the same frozen inputs.
- No secret or raw prompt in telemetry.

## 15. L. Reviewer independence and specialization

### Design

- Separate reviewer processes/identities per role: Functional, Architecture,
  Code Quality, Security, Testing/Traceability, Operability/Performance.
- Separate read-only sessions; no implementer self-review; critical work uses a
  different provider/model where available.
- Reviewer bundle includes spec/OpenSpec/stack/architecture/docs/DoD/
  traceability/diff/source/verifier evidence.
- Gate requires every required role PASS with zero open findings.
- Reviewer findings are durable and route through the Finding Router.

### Acceptance

- One role cannot submit two receipts under one principal.
- A missing required role is `NOT_TESTED`, not accepted.
- A critical candidate with same-model review is flagged or blocked by policy.
- Reviewer prompts cannot edit code or accept work.

## 16. M. Technology skill packs and documentation freshness

This is the owner-requested capability: at assessment/stack detection, check
whether the project has current documentation and skills for its technologies;
if not, send the Librarian to fetch official sources and create/update a
versioned skill pack. Refresh it periodically.

### Skill pack contents

- technology/language/framework and supported versions;
- official source URLs and access date;
- current best practices, anti-patterns and deprecations;
- canonical commands (build/test/lint/format/security);
- framework-specific gotchas, security notes and migration guidance;
- applicability (paths/languages/versions), owner, license, provenance;
- `last_verified_at`, `review_after`, content hash.

### Source hierarchy

1. Official documentation.
2. Official changelog/blog.
3. Standards (MDN, RFC, W3C, OWASP, RustSec, etc.).
4. Runtime compatibility data.
Never use Stack Overflow, tutorials, AI summaries or training data as the
primary source. Cite exact deep links; flag unverified patterns explicitly.

### Lifecycle

- **Assessment:** detect stack and docs; check registry for a matching,
  non-stale skill pack.
- **Gap:** Librarian fetches official docs; Skill Author/Planner composes the
  pack; Core validates schema/size/license/provenance; owner reviews if
  required.
- **Use:** Planner uses the pack for stack/architecture/dependency decisions;
  implementer and reviewers receive the applicable pack as read-only context.
- **Refresh:** periodic `review_after` (e.g., 30 days for fast-moving
  frameworks, 90 days for stable libraries); major version or stack change
  invalidates immediately; stale packs trigger Librarian refresh and block
  `FORMAL_SPEC` until refreshed or explicitly waived by the owner.
- **Projection:** store in the Core skill registry and optionally project a
  concise `.factory/skills/<tech>/SKILL.md` into the repo for review.
- **Authority:** a skill is context, never a tool or authority grant. It cannot
  select itself or widen scope.

### Documentation baseline recommendation

Quality-first, tiered. Use C4 for architecture views, ADRs for decisions,
Diátaxis for reader intent and docs-as-code for delivery.

**Every project:**

- `README.md`: what it is, quickstart, commands, link map.
- `AGENTS.md`: concise constitution, commands, structure, conventions,
  constraints, docs/skills pointers.
- `docs/architecture/overview.md`: context and container/module view; 2–5
  sequence diagrams for critical flows.
- `docs/architecture/decisions/ADR-*.md`: one decision per record, alternatives
  and consequences.
- `docs/quality/definition-of-done.md`: checks, evidence, review roles.
- OpenSpec change folder for each change (mandatory for `FORMAL_SPEC`, compact
  for `DIRECT`).
- `docs/security/` when a trust boundary, credential or user data exists.

**Service/API:** add OpenAPI/contract, data model/migrations, runbook,
observability/SLO notes, threat model for trust boundaries.

**UI:** add design system/tokens, accessibility/WCAG 2.2 AA notes, browser test
matrix, responsive states.

**Library:** add public API reference/examples, semver/deprecation policy,
supported versions and migration notes.

Rules: docs live in the repo and change in the same PR; generated reference is
preferred; no duplicate README; diagrams as Mermaid/C4; freshness metadata;
stale required docs block acceptance. Do not create empty ceremony for a small
CLI; create the smallest set that lets a new engineer/agent change it safely.

## 17. N. Chaos and fault-injection evaluation

Run these as deterministic tests/fixtures, not one-off manual checks:

| Fault | Expected result |
|---|---|
| Kill Core/Worker/provider at each state | Durable resume, no duplicate effect |
| SSE cut/reconnect | Cursor replay, no partial final |
| Malformed/oversized model JSON | Bounded repair, typed failure |
| Disk full/archive too large | Typed error, no worktree mutation |
| Dependency fetch partial/failure | Atomic rollback, explicit blocker |
| GitHub 429/500/duplicate webhook | Backoff, idempotent projection |
| Stale lease/duplicate worker | Fenced winner, no double effect |
| Cancel at each state | No accepted candidate, no duplicate effect |
| Context compaction/restart | Plan reload, no duplicate work |
| Flaky/ordering/timezone/port test | Deterministic pass, no retry-to-green |
| Malicious issue/docs/OpenSpec | No authority/scope change |
| Reviewer identity collision | Rejected |
| OpenSpec drift/manual edit | Drift detected, Core remains authority |

## 18. Phases and priorities

| Phase | Workstreams | Acceptance |
|---|---|---|
| 0 | Contracts, state machine schema, reliability ADR/plan | No behavior claim; schemas/tests reviewed |
| 1 | A, F, K | Crash/restart, idempotency, fencing, trace |
| 2 | B, C, I | Provider reconnect, workspace bounds, deterministic tests |
| 3 | D, M | Rust vendor/SBOM/deny/audit, skill packs/refresh |
| 4 | E, H | Outbox projections, Finding Router, automatic replan/repair |
| 5 | G, J, L | Context handoff, prompt-injection tests, specialized reviewers |
| 6 | N | Chaos suite and live pilot |

P0 before more paid live campaigns: A, B, C, D, E, F, H, I, J, L.
P1: K, M. P2: broader provider matrix and cross-repo OpenSpec stores.

## 19. Metrics

- workflow recovery success rate;
- duplicate external effects (target zero);
- provider turn failure/reconnect rate;
- workspace/snapshot failure rate;
- dependency acquisition/blocker rate and stale-vendor age;
- projection drift/duplicate rate;
- lease/cancellation race findings;
- context handoff success;
- automatic finding resolution rate and owner escalations;
- flaky test rate;
- skill freshness coverage and stale-skill blocks;
- time to accepted candidate (observability only).

## 20. Open decisions

1. Which workflow engine primitive to use inside Core: hand-rolled SQLx state
   machine, or a Rust durable-execution crate? Core ownership and no second
   orchestrator remain constraints.
2. Exact reviewer fleet topology: separate processes per role vs one service
   with role-scoped principals.
3. Skill refresh cadence by technology class and who owns approval.
4. Vendor tree in git vs digest-pinned cache image per project.
5. Whether `cargo-vet` is mandatory for all Rust projects or only high-assurance
   ones.
6. OpenSpec toolchain packaging (Node image/cache vs Rust subset).
7. Owner escalation threshold for automatic repair/replan.

## References

- Cargo vendor: https://doc.rust-lang.org/cargo/commands/cargo-vendor.html
- Cargo source replacement: https://doc.rust-lang.org/stable/cargo/reference/source-replacement.html
- Cargo fetch: https://doc.rust-lang.org/cargo/commands/cargo-fetch.html
- Rust Supply Chain Security Guide:
  https://rust-secure-code.github.io/rust-supply-chain-security/
- cargo-deny / cargo-audit / cargo-vet / cargo-cyclonedx:
  https://microsoft.github.io/RustTraining/engineering-book/ch06-dependency-management-and-supply-chain-s.html
- CycloneDX Rust: https://github.com/CycloneDX/cyclonedx-rust-cargo
- C4 model / arc42 / ADRs:
  https://dev.to/simonbrown/a-minimal-approach-to-software-architecture-documentation-4k6k
  and https://www.highcircl.com/en/blog/software-architecture-documentation
- Diátaxis: https://diataxis.fr/
- Docs as Code: https://www.writethedocs.org/guide/docs-as-code
- Durable execution for LLM agents:
  https://vadim.blog/durable-execution-llm-agents
- Agentic workflows in production:
  https://www.coreweave.com/blog/agentic-workflows-in-production-mechanisms-trade-offs-costs-and-design-patterns
- Agent Skills examples (source-driven development, per-technology skills):
  https://github.com/addyosmani/agent-skills and
  https://github.com/mikkelkrogsholm/dev-skills
