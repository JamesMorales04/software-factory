# ADR-0033 — Provider-agnostic reviewer selection and bounded autonomy

**Status:** Proposed; requires Product Owner approval and independent review
before promotion to `main`.

**Relates to:** ADR-0010 (frozen candidate quality pipeline), ADR-0013
(bounded autonomous recovery), ADR-0018 (native Rust), ADR-0025
(container-native and Office parity), ADR-0032 (Codex ChatGPT native review).

## Context

ADR-0032 introduced a dedicated Codex reviewer boundary, but the reviewer
transport was selected by a global `FACTORY_ENABLE_CODEX_REVIEW` flag and the
quality receipt validator hardcoded `codex/gpt-6-luna`. That made the reviewer
impossible to select from Office, prevented other configured providers from
serving the semantic-reviewer role, and left the global flag able to override a
frozen Execution Profile.

Reviewer findings were also prose with a severity label. Core could not
deterministically score them, enforce a risk budget, escalate critical findings,
or authorize a bounded repair without an operator hand-writing the next attempt.
Progress stagnation produced an operator-facing `DIAGNOSE` hold, but the
diagnosis/replan path had no production caller and the no-progress breaker was
not reachable.

## Decision

1. **The frozen Execution Profile is the only reviewer selection authority.**
   The `semanticReviewer` route from the candidate's bound Execution Profile
   revision selects provider and model. `GET
   /quality/candidates/{id}/review-config` resolves that route against the
   configured provider adapter. A provider capability, not a global flag,
   chooses the transport:
   - OpenAI-compatible chat completions → `MODEL` receipt;
   - Codex App Server → `CODEX_APP_SERVER` native review receipt.
   `FACTORY_ENABLE_CODEX_REVIEW` is only a deployment availability signal for
   the dedicated native reviewer; it never overrides a selected chat route.
   A missing or disabled selected route fails closed instead of silently
   falling back to another provider. Legacy Campaigns without a frozen profile
   retain the existing deterministic/default-provider fallback.

2. **Every model-backed role uses the same route mechanism.** Lead,
   Implementer and Semantic Reviewer already consume the frozen profile roles;
   Reviewer transport is derived from `ProviderAdapter::review_transport()`.
   Adding a future adapter requires a catalog entry, an adapter variant, a
   review transport and a receipt validation path, not a new global switch.

3. **Findings carry canonical, Core-validated points.** Severity weights are
   Critical 8, Blocking 5, Warning 2, Info 1. Reviewers must provide a concrete
   rationale and evidence references. Core rejects any points that differ from
   the canonical mapping, an approval containing Blocking/Critical findings,
   and a passing receipt with a Blocking/Critical finding. Historical receipts
   decode with defaulted points/rationale for display but fail closed for
   autonomous repair. Informational findings on a passing, executed review are
   non-blocking observations: they stay visible and retain a durable
   disposition but do not block acceptance.

4. **Repair authorization is deterministic.** Core filters informational
   observations, computes the finding score from Warning/Blocking/Critical
   findings, requires reviewer independence from the implementer and verifier,
   requires executed non-passing review evidence, applies the frozen
   `AutonomyPolicy` point and cycle budgets by risk class, and records an
   append-only `AUTHORIZED`, `DENIED` or `HUMAN_REQUIRED` decision. An
   info-only review never creates a repair authorization. Any Critical finding,
   R2/R3 risk, unknown risk, disabled policy or exhausted budget is a human
   stop. Model output never grants repair or delivery authority.

5. **Autonomy is default-off and hash-preserving.** `AutonomyPolicy` is an
   optional field of `ProgressPolicy`; a disabled policy is omitted from
   serialization so historical profile, delegation and authorization hashes
   remain stable. Enabled policies must have positive R0 budgets, at most one
   R1 repair cycle and zero R2/R3 budgets.

6. **Progress diagnosis/replan is deterministic.** When autonomy is enabled, a
   stagnation trigger records a bounded diagnosis, resets the observation
   window and lets the Worker continue instead of creating an immediate
   operator hold. Each diagnosis counts as an unsuccessful replan; the frozen
   `escalateAfterUnsuccessfulReplans` threshold remains the no-progress circuit
   breaker and produces a human escalation hold. No model call is required for
   this mechanical decision.

7. **Office parity.** The Execution Profiles workspace exposes the reviewer
   provider/model selector, the review transport hint, and the autonomy
   enable/budget controls. Candidate evidence renders severity points,
   rationale and repair authorization from Core. These are read/configure
   surfaces over existing authenticated Core operations, not new authority.

## Consequences

- A Codex reviewer is selected explicitly by the frozen profile; the global
  environment flag is only availability. Operators can select another
  configured chat provider without changing Core code.
- Critical review findings now stop at `HUMAN_REQUIRED`; they cannot be
  converted into an autonomous repair even if a policy accidentally allows
  points.
- The repair authorization is evidence, not execution: the current slice
  records and projects the deterministic decision. Automatically scheduling a
  new repair attempt remains a separate, bounded change.
- Autonomy defaults off; existing profiles and acceptance Campaigns retain
  their current behavior unless an operator opts in.
- The dedicated Codex home remains the only native-review credential boundary;
  no Codex credential is copied into `provider_settings` or Office.

## Verification

- Unit tests cover canonical points, critical severity, autonomy policy
  validation and hash stability, repair authorization branches, first-time PASS
  progress, critical gate escalation and frozen reviewer routing.
- Compose/browser acceptance must select the Codex reviewer through the profile
  and preserve exact candidate binding, measured usage, critical-zero and the
  no-progress breaker. Real acceptance evidence is recorded separately and is
  not implied by unit tests.
- Author-run final acceptance on 2026-11-06 (exact code head `051a781`, corrected
  Netavark `isolate=true` topology): fresh project
  `f7d2d87d-3263-4f88-a576-14f1108d8190`, profile revision
  `9ce954b0-8892-4c1f-8fa7-199ac29b102c` selecting Codex `gpt-6-luna`, candidate
  `a2e45562-f55e-48db-85bd-be6ffe715c9c` at commit
  `767d12d07e11e24e1aba7e1c8028964e3b65d5e5`, verifier PASS and reviewer PASS
  with 9,123 measured tokens; deterministic Quality Gate `ACCEPTED`. Evidence:
  `docs/evidence/wave3-final-real-model-acceptance-2026-11-06.json`. This is
  author-run evidence, not independent certification or SEC-01 isolation.
