# ADR-0010 — Frozen candidate, independent evidence and a deterministic Quality Gate

**Status:** Accepted (F1-S4-A implementation; final candidate evidence is recorded in the PR). **Date:** 2026-10-20.

## Context

S1–S3-D established durable Actions/Runs/Attempts, fenced Paseo sessions and
recovery. A completed Run still did not imply a validated change: there was no
frozen acceptance specification, no immutable candidate identity, no independent
verifier or reviewer path, and no Core-owned decision that could distinguish a
successful agent turn from accepted software.

The F1-S4-A amendment additionally requires agent-native browser automation so
an Implementer can exercise the real Office UI and an Independent Verifier can
replay a protected browser journey against the exact candidate.

## Decision

1. An `ActionSpec` is an immutable, versioned engineering contract. It binds the
   Action, authorized repository/base revision, goal, allowed scope, executable
   acceptance and negative checks, risk class, Quality Profile, budget and pinned
   Paseo route. Its content hash is the evidence identity; changes create a new
   spec and never retroactively validate old evidence.
2. The Core independently validates an Implementer commit and freezes a
   `CandidateRevision` with commit/tree/content digests, changed paths, derived
   risk and Run/Attempt/generation provenance. Candidate rows are immutable
   except the explicit superseded state. A repair creates a new identity; prior
   candidates are superseded.
3. The Independent Verifier is a separate process with a dedicated credential.
   It checks out the exact commit into a clean read-only worktree, executes the
   frozen oracle (including real Chromium browser checks through the pinned
   `playwright-core`/Chromium pair), and submits concrete results. The
   Implementer cannot submit verifier authority with the Worker credential.
4. The Independent Reviewer is a separate read-only process with its own
   credential. It revalidates the exact commit/scope and reviews the final diff,
   returning `APPROVE`, `REQUEST_CHANGES` or `BLOCKED`.
5. The Core Quality Gate is a deterministic, side-effect-free policy function.
   It consumes only durable identity and evidence; no model confidence, saved
   screenshot or agent assertion can override a mandatory failure. Exactly one
   final decision per candidate is persisted and replayed idempotently.
6. Browser checks are ordinary mandatory spec checks of kind `browser`. Tool
   absence is `NOT_TESTED`, not PASS. The `node-browser` profile adds the browser
   requirement to the global baseline; profiles can only add requirements.
7. The official Playwright CLI (`@playwright/cli`, pinned) plus the existing
   `playwright-core` harness is the browser tooling. Playwright MCP is deferred:
   the CLI and harness already cover agent interaction, independent verification,
   diagnostics and CI, while MCP would add a persistent server/profile surface
   without a demonstrated requirement.
8. No GitHub delivery, merge, registry expansion or self-development promotion is
   authorized by this ADR. TRUSTED_LOCAL remains broad local access and is never
   labeled isolated or certified.

## Consequences

- New tables (`action_specs`, `candidates`, `candidate_verifications`,
  `candidate_reviews`, `gate_decisions`), candidate events and scoped
  verifier/reviewer routes; the run ledger remains the replay source.
- Candidate evidence is append-only and candidate-bound. Superseding a candidate
  invalidates acceptance of its evidence but never rewrites history.
- Browser checks require Chromium and `playwright-core`; missing tooling yields a
  truthful `NOT_TESTED`/`BLOCKED` outcome, never a green skip.
- The protected Quality Gate and credential scopes are R3 surfaces. Material
  changes require Product Owner approval.

## Alternatives rejected

- **Paseo success as acceptance:** a completed session is execution evidence, not
  correctness, scope or independent review.
- **LLM-only gate:** non-deterministic and unable to fail closed on forged or
  missing identity.
- **Verifier role as a JSON field:** independence requires separate credentials,
  processes and clean resources.
- **Playwright MCP as mandatory:** no requirement beyond the CLI/harness, and it
  would add a persistent authenticated browser surface. It can be evaluated later
  as an additive capability.
