# Governed issue intake — independent review record (2026-10-28)

**Reviewer:** GPT-6 Luna via the external agent runner (independent of the
Codex Sol design and the OpenCode implementation lane).
**Design:** `docs/implementation/office-ux-autonomy-contract-2026-10-28.md`
(commit `271c821`).
**Verdict:** commits 1–2 safe only as default-off policy, acknowledgement,
queue and advisory assessment with **no execution path**; commit 3 must remain
blocked.

## Findings

1. **HIGH — start authority must be Core-owned, not HTTP-selected.** The design
   must not let `/lead/requests/{id}/start` choose autonomous mode from the
   deployment flag. A new issue-intake start path must load and validate the
   persisted grant/envelope inside Core; ordinary start must reject
   issue-intake requests unless they are operator-approved or pass the
   Core-owned envelope check.
2. **HIGH — identity binding is underspecified.** The canonical envelope must
   bind immutable author ID/association, exact labels/state and snapshot
   revision, canonical remote, repository ID and reopened-issue identity, not
   only issue number/content digest/observed revision. A closed-then-reopened
   issue cannot reuse an old grant.
3. **HIGH — issue text still reaches model/worker decision surfaces.** Prompt
   delimiting and typed output are insufficient. Assessment and execution must
   use an enforced tool/path/egress boundary with no credentials or unrelated
   project data; if the boundary is unavailable, block before any provider
   call. Workers receive only the exact authorized repository allowlist.
4. **MEDIUM — reservation/recovery invariants need a single durable ledger.**
   Define atomic idempotent assessment/dispatch/replay/restart/cancellation
   reservations, settlement keys, fail-closed unknown usage and shared
   project/provider ceilings so concurrent grants cannot overspend.
5. **MEDIUM — revocation must fence immediately before provider start.** A
   Core/broker check immediately before each provider start, durable
   cancellation-pending state across restart, and no replacement dispatch are
   required to close the revocation race.

## Sound invariants

Finite acknowledged batches, digest checks, direct freshness reads, no implicit
link fetching, default-off policy, immutable grant binding, shared transactional
start, replay returning original IDs, and existing verifier/reviewer/Quality Gate
remain sound. The design does not add a second scheduler or acceptance authority.

## Gate decision

- Implement commits 1–2 only: default-off policy, explicit acknowledgement,
  finite batch grants, durable queue, deterministic eligibility and advisory
  assessment/proposals. No automatic start, no Worker dispatch from intake, no
  PR write/merge.
- Commit 3 remains blocked pending: a protected ADR that explicitly amends
  ADR-0029, Product Owner approval, Core-owned envelope validation, enforced
  confinement, durable accounting and independent review.
- No runtime, live-provider or automatic-execution evidence is claimed.
