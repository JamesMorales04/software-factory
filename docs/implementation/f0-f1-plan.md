# F0 and F1 — implementation and independent validation plan

**Status:** F1-S1 implementation authorized (2026-10-18); source repository is permanently `JamesMorales04/software-factory`. SEC-01 disposition is mode-specific per [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md): SEC-01-STRICT not-required-for-TRUSTED_LOCAL; SEC-01-ISOLATED paused / BLOCKED, available-only-where-verified. This documentation does **not** certify a working product or authorize unbounded campaigns.

## Incremental implementation authorization (2026-10-18)

- **May start:** F1-S1 actual Go Core, durable Action/API/event model, minimum authenticated Factory Office UI and local-first telemetry, after defining the small contracts and negative acceptance cases that S1 requires. Do not mark S1 PASS on documentation or mocks.
- **May prototype:** F1-S2 disposable credential-free Worker protocol, lease/recovery and resource controls with rootless Podman and synthetic repositories, without attaching real agent accounts or untrusted host execution.
- **Still blocked:** F1-S3-ISOLATED integration of autonomous real Paseo sessions with untrusted commands, full factory self-development and delivery privileges, until SEC-01-ISOLATED mandatory tool isolation is certified. Security findings remain valid despite successful MCP and SDK integration.
- **S3-TRUSTED_LOCAL entry conditions (per [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md), replacing the absolute gate for that mode only):** implemented local-trust policy ([trusted-local](../security/trusted-local.md)); S2 green on its frozen synthetic scope; provider operational (diagnostic Ready); a bounded readiness test proving a real local session can start, act, and be observed/ended; observable mode labeling (every run labeled TRUSTED_LOCAL, never isolated/certified); no unauthorized delivery or privileged autonomous effects; explicit owner authorization recorded.
- **Deferred security work:** the product owner paused D3-D2 and Stage E tests. Record their states as NOT TESTED/BLOCKED, not SKIPPED-as-PASS. See [SEC-01 results](../security/SEC-01-results-2026-10-18.md).

## F0 — specification and blocked-risk experiments

- **F0-01:** review the MASTER, enforce English documentation, label agreed/proposed/unverified facts, and reconcile ADR and contract contradictions.
- **F0-02:** specify six minimal contracts: Project–Action, Campaign–Run, Core–Worker, Agent Session, Quality–Delivery and Factory Events. Each needs owners, identity, schema, lifecycle, idempotency, error rules, authority, evidence and positive/negative tests.
- **F0-03:** [SEC-01](../security/paseo-sandbox-spike.md) produced [partial evidence](../security/SEC-01-results-2026-10-18.md) on disposable fixtures: successful rootless Podman and MCP/SDK integration, but failed host read isolation and unproven credential confinement. Further strict-isolation experiments paused; F1-S3-ISOLATED remains blocked. S3-TRUSTED_LOCAL proceeds under its own entry conditions per [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md).
- **F0-04:** apply accepted [ADR-0005](../architecture/decisions/ADR-0005-go-core-worker-language.md): Go for Core/Workers, TypeScript for the Paseo adapter and Factory Office. Pin the Go formatter, analysis, build and test toolchain. TECH-01 is closed by product-owner decision; a comparative Rust prototype is not a prerequisite.
- **F0-05:** implement or configure a lightweight documentation-quality CI path when authorized and test it against this repo; ensure links, secret scanning and formatting checks are real rather than decorative.
- **F0 exit:** approved slice-relevant contracts, recorded mode-specific SEC-01 disposition, stack decision and objective F1 acceptance criteria. Security certification is **not** an all-development stop: S1 and noncredential S2 work are allowed, but no affected slice may be accepted or deployed by weakening isolation — except via the explicit owner-accepted TRUSTED_LOCAL policy ([ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md)), which is a recorded risk acceptance, not a silent weakening.

## F1 — verified vertical slices

| Slice | Deliverable | Independent real-user journey | Required negative path |
|---|---|---|---|
| S1 | Durable Core, API, domain events, telemetry and minimal Factory UI | Start the server, create/view Action in browser, restart Core and observe persisted state | Unauthorized/invalid operation rejected; duplicate event ignored; OTel outage nonblocking |
| S2 | Local Worker and Core–Worker dispatch/lease/recovery | Submit Run; see real Worker and result in UI | Worker crash/reconnect, stale lease and replay cannot duplicate execution |
| S3-TRUSTED_LOCAL | Real Paseo session under owner-accepted local trust (ADR-0009) | Start/inspect a real coding session in a real workspace, verify effective cwd/provider/model, correlate its timeline | Unauthorized delivery/admin action blocked; session not labeled isolated/certified |
| S3-ISOLATED | Real Paseo integrated inside certified sandbox | Start/inspect a real coding session and correlate its timeline | Cannot read host/other-project/credential canaries; bounded egress |
| S4 | Implementer, Independent Verifier, Reviewer, baseline | Produce a real diff; independent tester starts and exercises exact commit | Mandatory failure blocks; Reviewer cannot mutate candidate |
| S5 | GitHub Delivery and R0 gate | Open real PR with CI/review and accept or reject exact SHA | Ineligible R0 blocked; repeated webhook cannot double merge |
| S6 | First self-development campaign | Stable Core A supervises a functional Factory UI improvement in candidate B | B cannot modify or replace A; incorrect evidence blocks delivery |

Real autonomous R0 merge requires a separate eligible case and rechecked protection rules; the first self-development PR must not be force-merged if its final risk disallows it.

## Mandatory slice loop

1. Define scope, exact acceptance criteria, independent oracles and negative cases **before** implementation.
2. Implementer makes the smallest coherent change in the assigned workspace, removing obsolete internal interfaces and updating all callers atomically when needed.
3. Run formatter/linter/static checks, compilation, targeted unit/contract/integration tests and applicable security checks. Never disable quality tooling to pass.
4. Implementer **uses the built product** via real executable, HTTP, Playwright browser UI or real Paseo session as applicable. Diagnose from actual logs/traces, repair boundedly, and rerun affected checks.
5. Freeze candidate commit and artifact digests. An **Independent Verifier** (not its author) uses a clean immutable checkout and protected oracle to rerun the journeys and negative paths.
6. Independent Reviewer assesses architecture boundaries, maintainability, DRY, security, test adequacy and evidence. Deterministic Quality Gate decides acceptance.
7. Record failures as repair Tasks with bounded budget; repairs create new candidates and invalidate affected evidence. Never use a mock-only PASS as final acceptance.
8. Only after acceptance advance the slice or authorize governed delivery. Regressions discovered later become Incident/Action and eval fixtures.

## Evidence per slice

Attach exact candidate SHA, Action/spec/policy revision, Run/Worker/Paseo session identities where applicable, actual commands and browser journeys, formatter/linter/build/test results, redacted logs and OTel traces, user-journey evidence, Reviewer verdict, repairs and PR/merge receipts. Artifact storage follows retention/security policy, not indiscriminate Git commits.

## Mode-specific S2/S3 disposition (per [ADR-0009](../architecture/decisions/ADR-0009-trusted-local-execution.md))

- S2 rows keep their evidence: Podman fixture checks, SDK smoke/reconnect, Worker boundary, and broker functional integration stand as observed; D1/D2-C FAILs and D3-D2/E NOT TESTED remain valid and are never rewritten to PASS.
- S2-C/S2-D class broker-certification rows (mandatory mediation of every native shell, file-edit, code mode, plugin, subprocess, and agent path) are re-scoped to ISOLATED/RESTRICTED only. They explicitly do **not** gate TRUSTED_LOCAL.
- S3-TRUSTED_LOCAL entry conditions: implemented local-trust policy, S2 green, provider operational, bounded readiness test, observable mode, no unauthorized delivery, owner authorization (see authorization section above).
- S3-ISOLATED keeps the full SEC-01 resumption criteria: OS-enforced credential/tool separation per provider/runtime, canary denials, egress/restart/fencing/replay tests, pinned digests, independent review.

## Non-negotiable F1 blockers

No F1 PASS with fabricated Paseo agents, credential leakage, unsafe sandbox fallback, UI-only mock data, missing independent review, unaudited delivery, missing telemetry, unresolved duplicate effects or unrecoverable Runs. The verifier cannot be the candidate's sole author. TRUSTED_LOCAL runs must be labeled as such and never claimed isolated or certified; any silent ISOLATED-to-LOCAL downgrade blocks acceptance.

## Development hygiene

Follow [engineering standards](../engineering/code-quality.md), [lint policy](../engineering/linting.md) and [ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md). Do not build a compatibility subsystem for pre-release internal schemas; simplify and update callers/tests/docs together. Project/quality boundaries remain protected.
