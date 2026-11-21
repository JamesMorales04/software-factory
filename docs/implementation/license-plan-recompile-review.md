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

## Required order for the same intake

1. Recompile a new environment plan revision with the current catalogue/profile
   and real inventory (no model call).
2. Run the new **license evidence review** (separate purpose
   `license_evidence_review`) to produce the typed `LicenseDecisionReview`; the
   source proof is supplied from the exact cache/pathinfo evidence, not a model
   call and not a fabricated manifest fact.
3. Record the exact Core license decision against the new plan/inventory/evidence
   stage.
4. Run the new **environment review** model call (purpose
   `lead_environment_review`) so the builder receives the decision overlay and
   the new plan can freeze.
5. Build/spec/receipts; the old plan/review remains historical and is never
   forced onto the new plan.

Honest call count: two new Lead-route model calls after the two already settled
(`license_evidence_review` and `lead_environment_review`), unless the backend
later provides a deterministic rebind that Reviewer70 accepts. The source
metadata itself needs no provider call. The exact count/purpose must be recorded
in the independent evidence; no claim of "still 2".

## Plan STALE

The retained plan `c6cb4499…` has catalogue `725ba589…`; current policy is
`edef68e1…`. `PolicyBinding::from_plan…` rejects it with `PolicyMismatch`. The
new plan revision is mandatory; the old plan/static asset cannot be accepted.

No call is made in this document. The calls require the backend final SHA,
review, fresh Core authority/admission and the normal runtime checks.
