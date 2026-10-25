# Mandatory global Quality Baseline

**Decision:** accepted. **Implementation:** F1-S4-A quality pipeline implemented
and exercised by `scripts/e2e-s4a.sh`; final candidate-bound acceptance is
recorded in the campaign PR. Not certified beyond the recorded evidence.

## Universal acceptance invariants

For every change, enforce appropriate evidence of: exact Project/Action/frozen spec/candidate/policy identity; authorized sandbox and scope; functional behavior; relevant regression protection; security; maintainable implementation; required independent verification; trustworthy provenance; and exact correspondence between verified and delivered commit.

A Project Quality Profile adds stack- and risk-specific checks; it never weakens this baseline. Universal guarantees **do not** require running every tool for every change.

## Quality states

- `ACCEPTED`: every required check is satisfied with current evidence.
- `REPAIR_REQUIRED`: verified fixable defects exist.
- `BLOCKED`: required evidence, capability or environment is missing.
- `HUMAN_REQUIRED`: explicit policy or product-owner authority boundary.
- `NOT_TESTED`: insufficient evidence to decide.

Individual checks distinguish `PASS`, `FAIL`, `SKIPPED`, `NOT_APPLICABLE` and `NOT_TESTED`; no unchecked status means PASS.

## Coding quality and static gates

Apply [code-quality standards](../engineering/code-quality.md) and the active [formatter/lint/static-analysis toolchain](../engineering/linting.md). Review for duplicate policy knowledge, wrong abstraction, excessive coupling, obscured error handling, unreachable/deprecated code and missing tests. A linter pass does not imply maintainability or behavioral correctness.

While internal APIs are experimental, remove superseded code instead of preserving backward-compatibility shims. Respect actual stable external contracts and persisted user data; see [ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).

## Risk and delivery

- **R0:** bounded low-risk change, potentially eligible for autonomous merge **only** after all required independent verification and CI.
- **R1:** moderate ordinary change; additional policy-gated checks.
- **R2:** security, data/schema migrations or published contracts; protected approval.
- **R3:** authority, secrets, sandbox, policy/Quality Gate, merge logic or live Core upgrades; strictly protected process.

Recalculate risk from the **final diff**. A proposal labeled R0 does not grant merge authority. Immediately before delivery, recheck candidate SHA, policy, CI, review, GitHub branch protections and authorization. Only the Core Delivery Engine owns merge credentials; coding agents do not.

## Browser acceptance (additive)

When a frozen spec includes a `browser` check, real Chromium is a mandatory
independent check. Browser tooling absence is `NOT_TESTED`/`BLOCKED`; a saved
screenshot or CLI exit alone is never PASS. The browser cannot override identity,
scope, functional or reviewer requirements. See
[browser automation](../engineering/browser-automation.md).

## Independent real-user verification

For each accepted slice: relevant unit/contract/integration tests → Implementer runs the actual built product and repairs issues → freeze exact candidate → **Independent Verifier** starts it on clean resources, checks CLI/HTTP/Playwright/provider user journeys and negative paths, and records separate evidence → Reviewer checks design/security/results → deterministic Quality Gate accepts or blocks.

Author-written mocks and only-author-run tests cannot substitute for independent end-to-end acceptance. Verifier must not edit candidate or protected test oracles. Repairs require a new candidate and revalidation.

## F1 negative acceptance examples

- Failing/missing mandatory check prevents acceptance and merge.
- Reviewer/Verifier evidence belongs to exact candidate/policy.
- Pending CI, mismatched SHA or changed branch protection blocks delivery.
- Actual R2/R3 diff cannot travel through R0 auto-merge.
- At least one reproducible UI/CLI integration defect is detected and repaired during development, evidenced independently.
