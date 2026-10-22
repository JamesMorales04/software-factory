# ADR-0017 — Bounded intelligent Lead planning

**Status:** Proposed; implementation contracts and standing authority semantics
require Product Owner approval. **Date:** 2026-10-21 UTC.

## Context

ADR-0016 accepts autonomous development and maintenance as product objectives,
while leaving implementation contracts and delegation limits open. The current
Core has Project-bound ActionSpecs, Campaigns, Worker Runs, independent
verification/review and a deterministic Quality Gate. A planning step must
connect an owner request to that path without allowing model output to create
authority or a second execution lifecycle.

Code candidate `be592770b77ca81c491e5021913e1d23d5a228b7` adds domain
validation for a typed Lead proposal, a structured advisory assessment,
durable request/grant records and a route through the existing ActionSpec and
Campaign machinery. The Office renders the Core projection and exposes an
explicit authenticated start action after the plan/spec are visible for owner
review. Candidate deterministic checks pass. Independent review of clean
`be592` reported no P1 blocker; Product Owner approval to use paid model
sessions remains outstanding.

## Decision drivers

- The Product Owner owns objectives, risk appetite and delegation limits.
- Core remains authoritative for project scope, authorization, budgets,
  persistence, execution, evidence and delivery effects.
- Semantic judgments can be model-assisted; observable bindings and budget
  enforcement remain deterministic.
- Planning freezes exact identities before Campaign execution.
- Ambiguity, stale authority, unsupported plan shape and resource-limit
  violations stop for owner input instead of widening the grant.

## Considered alternatives

1. **Human-authored ActionSpec only.** Smallest change and lowest model cost,
   but does not provide an intelligent Lead path for approved objectives.
2. **Model-generated general WorkGraph or workflow DSL.** More flexible, but
   expands execution surface and validation burden before evidence shows that
   arbitrary graphs are needed.
3. **One bounded implementation unit validated by Core.** Reuses current
   ActionSpec/Campaign/Run/evidence lifecycles and is the proposed S4-D scope.

## Proposed decision

Use one advisory Lead assessment to produce a typed plan proposal bound to one
active registered Project, one nonterminal Action, its current Project
configuration hash and an authorized base revision. The proposal must contain
exactly one executable `implementation` WorkUnit. Core validates objective
class, path and capability subsets, quality profile/check coverage, risk class,
exact model route and all attempt, timeout, repair, wall-clock, parallelism and
session bounds. Core then freezes the Project-bound ActionSpec and immutable
plan identity and creates the existing Campaign. Execution is started through
the Campaign path.

The assessment mechanism is `MODEL`; the plan acceptance path is `HYBRID`:
semantic interpretation is advisory model output, while Core deterministically
checks identities, authority, graph shape, scope, route, budgets and frozen
bindings. Model output does not approve evidence, change protected policy,
select ungranted capabilities, or authorize delivery. The Quality Gate remains
the only candidate acceptance authority.

Submitting an objective may create an advisory model plan only under an
explicit delegation. A `ready` plan does not start execution: an authenticated
owner must review the immutable proposal/spec projection and explicitly invoke
the existing Campaign start transition. No additional model prompts are
inserted among Implementer, Verifier, Reviewer or Quality Gate participants.
One delegation authorizes one distinct request; idempotent replay returns that
same reservation. Its `maxSessions` bound includes the one assessment session
and the bounded Implementer attempt horizon.

In the current implementation, creating a Lead Action, appending its creation
event and binding its ID to the reserved request are one store transaction.
The prepared live runner also reconciles assessment and Implementer session
identities against the successful Run, Attempt, candidate, verifier, reviewer,
Gate and observed provider routes; this runner remains unexecuted and provides
no live acceptance evidence.

All delegation limits must be explicit. This proposal defines no privilege
defaults and does not settle Product Owner identity/provenance, objective-class
eligibility, risk appetite, permitted paid-use budget, expiry/renewal policy,
revocation treatment for already-running work, or escalation response times.
Those semantics require an explicit Product Owner decision before real use.
The current API's local-operator credential and implementation do not prove
Product Owner approval. No paid session, delivery, merge, deployment, self-
development or isolation certification is authorized here.

### Protected paid-session gate (implementation; not an authorization)

The Core paid-assessment gate is closed by default. Opening it requires the
operator to supply all of `--lead-paid-authorization-id`,
`--lead-paid-project-id`, `--lead-paid-base-revision`,
`--lead-paid-provider`, `--lead-paid-model`,
`--lead-paid-paseo-mode`, and `--lead-paid-max-sessions`. Core checks a grant
against that exact Project, current base revision, provider/model/mode route,
and total session ceiling. The delegation-creation `Idempotency-Key` must equal
the configured authorization ID; this creates one durable grant, and replays
of that key resolve to that grant rather than creating another authorization.

The live S4-D runner separately requires `FACTORY_S4D_LIVE=1` and
`FACTORY_LIVE_AUTHORIZATION_ID` to match its fixed authorization identity. It
also requires the owner authorization envelope and an exact clean frozen
Factory SHA. These code-level gates do not supply Product Owner authorization
by themselves. No live or paid run is authorized by this ADR. Independent
review of clean `be592` reported no P1 blocker; this does not authorize paid
use or replace Product Owner approval.

## Consequences and limitations

- S4-D has one executable WorkUnit and cannot represent parallel independent
  implementation tasks. Additional roles or graph shapes require a later
  decision and acceptance evidence.
- The proposal's base revision is syntax-checked in the domain; Core/store must
  confirm it is a current authorized Project base tip before freezing the spec.
- `maxSessions` is checked against one assessment session plus the Implementer
  attempt horizon. It does not prove provider cost/quota or a monetary spend
  cap.
- TRUSTED_LOCAL remains broad host access. A temporary Paseo workspace and
  control-plane scope do not establish OS isolation or credential confinement.
- An interrupted `assessing` request is blocked on Core restart because the
  external session outcome may be ambiguous; it is not automatically retried.
- Intake performs assessment synchronously in the HTTP request. Code candidate
  `be592` sets Core `WriteTimeout` to 300 seconds and the live runner's
  Lead POST timeout to 360 seconds. This leaves response headroom beyond the
  bounded assessment/session work; the final docs-inclusive checkout still
  needs verification. Retries must use the same idempotency key.
- Revocation, expiry and project/spec drift prevent plan freeze or Campaign
  progress. The exact owner policy for work already in flight remains open.
- No live or paid model session was run as part of this ADR or contract work.

## Verification required before acceptance

The S4-D code candidate's deterministic negative cases, API/state lifecycle,
restart handling, idempotency and S1–S4-C regression suites pass at
`be592770b77ca81c491e5021913e1d23d5a228b7`. The docs-inclusive candidate still
requires independent review and verification. Semantic assessment quality
must be compared with a reasonable deterministic baseline on representative
requests, including ambiguous effort estimates. The scoped independent
review/verifier checks on `be592` passed; the verifier did not rerun S4-A/B/C
end-to-end or produce a real model result. Product Owner UAT and live
authorization are separate evidence. A green deterministic suite alone does
not authorize paid use or certify a live workflow.

## Supersession

This proposal does not supersede ADR-0016 or change its accepted authority
boundaries. If accepted, it will supply the S4-D implementation contract while
preserving ADR-0016's Core ownership, delivery separation and security limits.
