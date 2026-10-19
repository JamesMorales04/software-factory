# F0 and F1 — implementation and independent validation plan

**Status:** F1-S1 implementation authorized (2026-10-18); source repository is permanently `JamesMorales04/software-factory`. SEC-01 is paused / BLOCKED for credential-bearing workloads. Documentation and feasibility prototypes exist, but no Factory Core, Worker or Office implementation is certified. **Owner decision (S2 completion):** SEC-01 isolation certification is part of F1-S2 completion criteria (S2-B research, S2-C mandatory isolation architecture, S2-D provider-specific certification). F1-S3 begins only when its required provider execution boundary is independently certified and the owner authorizes progression.

## Incremental implementation authorization (2026-10-18)

- **May start:** F1-S1 actual Go Core, durable Action/API/event model, minimum authenticated Factory Office UI and local-first telemetry, after defining the small contracts and negative acceptance cases that S1 requires. Do not mark S1 PASS on documentation or mocks.
- **May prototype:** F1-S2 disposable credential-free Worker protocol, lease/recovery and resource controls with rootless Podman and synthetic repositories, without attaching real agent accounts or untrusted host execution.
- **May research (S2-B, owner-authorized S2 completion):** bounded security investigation with synthetic credentials and canaries only — provider sandbox mechanisms, process topology, tool-mediation exclusivity, adversarial OS-denial oracles. No privileged execution, no real host credentials, no uncontrolled authenticated-provider experiments; any real-auth test plan requires explicit owner approval before running.
- **May implement (S2-C, proven infrastructure only):** the smallest mandatory isolation controls validated by S2-B proof-of-concept evidence (per-Attempt disposable workspaces, scoped broker capabilities, fail-closed policy, orphan cleanup). No S3 Paseo adapter, autonomous campaigns, arbitrary command execution or delivery workflows.
- **Still blocked:** F1-S3 integration of autonomous real Paseo sessions with untrusted commands, full factory self-development and delivery privileges, until SEC-01 mandatory tool isolation is certified. Security findings remain valid despite successful MCP and SDK integration.
- **Deferred security work:** the product owner paused D3-D2 and Stage E tests. Record their states as NOT TESTED/BLOCKED, not SKIPPED-as-PASS. See [SEC-01 results](../security/SEC-01-results-2026-10-18.md).

## F0 — specification and blocked-risk experiments

- **F0-01:** review the MASTER, enforce English documentation, label agreed/proposed/unverified facts, and reconcile ADR and contract contradictions.
- **F0-02:** specify six minimal contracts: Project–Action, Campaign–Run, Core–Worker, Agent Session, Quality–Delivery and Factory Events. Each needs owners, identity, schema, lifecycle, idempotency, error rules, authority, evidence and positive/negative tests.
- **F0-03:** [SEC-01](../security/paseo-sandbox-spike.md) produced [partial evidence](../security/SEC-01-results-2026-10-18.md) on disposable fixtures: successful rootless Podman and MCP/SDK integration, but failed host read isolation and unproven credential confinement. Further experiments paused; F1-S3 remains blocked.
- **F0-04:** apply accepted [ADR-0005](../architecture/decisions/ADR-0005-go-core-worker-language.md): Go for Core/Workers, TypeScript for the Paseo adapter and Factory Office. Pin the Go formatter, analysis, build and test toolchain. TECH-01 is closed by product-owner decision; a comparative Rust prototype is not a prerequisite.
- **F0-05:** implement or configure a lightweight documentation-quality CI path when authorized and test it against this repo; ensure links, secret scanning and formatting checks are real rather than decorative.
- **F0 exit:** approved slice-relevant contracts, recorded SEC-01 disposition, stack decision and objective F1 acceptance criteria. Security certification is **not** an all-development stop: S1 and noncredential S2 work are allowed, but no affected slice may be accepted or deployed by weakening isolation.

## F1 — verified vertical slices

| Slice | Deliverable | Independent real-user journey | Required negative path |
| --- | --- | --- | --- |
| S1 | Durable Core, API, domain events, telemetry and minimal Factory UI | Start the server, create/view Action in browser, restart Core and observe persisted state | Unauthorized/invalid operation rejected; duplicate event ignored; OTel outage nonblocking |
| S2 | Local Worker and Core–Worker dispatch/lease/recovery **plus SEC-01 isolation foundation** | Submit Run; see real Worker and result in UI | Worker crash/reconnect, stale lease and replay cannot duplicate execution |
| S2-B | Threat model, provider architecture investigation, executable acceptance criteria | Adversarial oracles defined per provider route | Unverifiable route stays BLOCKED, never PASS |
| S2-C | Mandatory isolation architecture for untrusted execution | OS-denied canary probes; broker-only mediation; fail-closed policy | Model refusal or missing tool is not an OS boundary |
| S2-D | Independent security validation + provider certification decision | Per-provider verdicts with evidence | Uncertified provider stays disabled for S3 |
| S3 | Real Paseo integrated inside certified sandbox | Start/inspect a real coding session and correlate its timeline | Cannot read host/other-project/credential canaries; bounded egress |
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

## Non-negotiable F1 blockers

No F1 PASS with fabricated Paseo agents, credential leakage, unsafe sandbox fallback, UI-only mock data, missing independent review, unaudited delivery, missing telemetry, unresolved duplicate effects or unrecoverable Runs. The verifier cannot be the candidate's sole author.

## Development hygiene

Follow [engineering standards](../engineering/code-quality.md), [lint policy](../engineering/linting.md) and [ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md). Do not build a compatibility subsystem for pre-release internal schemas; simplify and update callers/tests/docs together. Project/quality boundaries remain protected.
