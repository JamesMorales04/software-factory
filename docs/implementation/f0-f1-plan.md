# F0 and F1 — implementation and independent validation plan

**Status:** implementation planning; source repository is permanently `JamesMorales04/software-factory`. This documentation does **not** certify a working product or authorize unbounded campaigns.

## F0 — specification and blocked-risk experiments

- **F0-01:** review the MASTER, enforce English documentation, label agreed/proposed/unverified facts, and reconcile ADR and contract contradictions.
- **F0-02:** specify six minimal contracts: Project–Action, Campaign–Run, Core–Worker, Agent Session, Quality–Delivery and Factory Events. Each needs owners, identity, schema, lifecycle, idempotency, error rules, authority, evidence and positive/negative tests.
- **F0-03:** execute the blocking [SEC-01](../security/paseo-sandbox-spike.md) Paseo/Podman/credential feasibility experiment on disposable fixtures and fake canaries. Avoid writing the full Worker until the boundary is demonstrated.
- **F0-04:** evaluate Go versus Rust (TECH-01) using an actual bounded API/job/persistence/recovery sample. Approve the implementation stack and select matching lint/type toolchain.
- **F0-05:** implement or configure a lightweight documentation-quality CI path when authorized and test it against this repo; ensure links, secret scanning and formatting checks are real rather than decorative.
- **F0 exit:** approved contracts, SEC-01 outcome, stack decision and objective F1 acceptance criteria. If an isolation blocker remains, document `BLOCKED` rather than weakening policy.

## F1 — verified vertical slices

| Slice | Deliverable | Independent real-user journey | Required negative path |
|---|---|---|---|
| S1 | Durable Core, API, domain events, telemetry and minimal Factory UI | Start the server, create/view Action in browser, restart Core and observe persisted state | Unauthorized/invalid operation rejected; duplicate event ignored; OTel outage nonblocking |
| S2 | Local Worker and Core–Worker dispatch/lease/recovery | Submit Run; see real Worker and result in UI | Worker crash/reconnect, stale lease and replay cannot duplicate execution |
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
