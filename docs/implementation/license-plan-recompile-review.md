# New plan revision: recompile/review invalidation and call count

Status: code inspection only; no runtime/model call.

## Code finding

`recompile_agentic_environment` (`core.rs:16742`) calls
`compile_environment_for_request` with `revision = previous + 1`. The produced
`EnvironmentPlan` has a new `plan_sha256`, new `license_inventory_sha256`,
current catalogue/profile hashes and `lead_review: None`. The endpoint itself
does **not** call a model; it records `environment.plan.recompiled` and returns
the projection.

The later `environment/review` path calls `run_lead_environment_review`
(`core.rs:13200`), which loads
`latest_environment_plan_review(&stored.plan_sha256)`. A new plan hash has no
review, so it calls `lead_model::review_environment` and then
`record_lead_review` binds the review to the new plan hash/revision. The old
review remains bound to the old plan hash and is not reused. Therefore:

- a new plan revision with current policy **requires a new Lead environment
  review model call**; there is no code path that rebinds the old review to a
  different plan hash;
- the review purpose is the existing `lead_environment_review` purpose; it is a
  new call/count, not a reuse of the two settled calls;
- a spec cannot freeze from the new plan without that review.

## Corrected order for the same intake (Reviewer70-confirmed)

Preconditions before any live call: backend final reviewed, integrated candidate
checks green, the relevant serial build completed, and the own stack started on
`18893` with the browser/API authorization lane. No live call is made against the
old plan or an unbuilt candidate.

The original licence-review-before-environment-review order was circular: the
decision requires an identity-facts receipt for the new plan, and that receipt
is produced by the first environment build. The supported order is:

1. Recompile a new environment plan revision with the current catalogue/profile
   and real inventory (no model call).
2. Run the new **environment review** model call (purpose
   `lead_environment_review`). It records the approved review and attempts the
   build without a licence decision; the builder refuses at the licence gate but
   still persists the exact new plan/spec identity facts (spec-cache
   observation, not a fabricated build receipt).
3. Run the new **licence evidence review** model call (purpose
   `license_evidence_review`) now that the receipt exists; the source proof is
   separate exact cache/pathinfo evidence, not a model call and not a
   fabricated manifest fact.
4. Record the exact Core licence decision against the new plan/inventory/evidence
   stage (no model call).
5. Retry the environment review. It reuses the already-recorded approved review
   and does not call the model again; the builder receives the decision overlay
   and the new plan/spec can freeze.
6. Build/spec/receipts; the old plan/review remains historical and is never
   forced onto the new plan.

Honest call count: two new Lead-route model calls after the two already settled
(`lead_environment_review` first, then `license_evidence_review`); the
environment-review retry is not a model call. The source metadata
itself needs no provider call. The exact count/purpose and costs must be recorded
in the independent evidence; if both execute, the real total is four, but do not
claim four before they actually run. No deterministic rebind is implemented to
save calls; Reviewer70 evaluates correctness, not authority or a bypass.

The old review, old plan hash, intake, admission and Owner declaration are
preserved. No SQL write, no old-plan grant and no static-asset acceptance.

## Plan STALE

The retained plan `c6cb4499…` has catalogue `725ba589…`; current policy is
`edef68e1…`. `PolicyBinding::from_plan…` rejects it with `PolicyMismatch`. The
new plan revision is mandatory; the old plan/static asset cannot be accepted.

No call is made in this document. The calls require the backend final SHA,
review, fresh Core authority/admission and the normal runtime checks.

## Receipt sequencing blocker (reported, unresolved)

`record_license_decision` requires `latest_identity_facts_receipt` for the
current plan hash. A new plan revision has a new hash and therefore no receipt.
The receipt is produced by `build_environment_before_freeze`, which runs inside
`freeze_reviewed_environment` after the environment review; the builder's
licence gate is exactly what needs the decision overlay. The naive
recompile -> licence review -> decision -> environment review order is
therefore circular.

Reviewer70 resolved it as option 1 (above). The first environment review builds
without a decision, the builder refuses at the licence gate but persists exact
new plan/spec identity facts, then the licence review and decision follow, and
the environment-review retry reuses the recorded approved review without a third
model call. Options 2 (pre-decision rebind) and 3 (bind to realised spec) are not
introduced. No option bypasses or fabricates a receipt.
