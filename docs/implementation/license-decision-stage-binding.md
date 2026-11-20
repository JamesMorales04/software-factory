# Licence decision stage binding (pre-spec)

Status: minimal explicit contract for the current pre-spec state. No runtime
change; Core98 and Reviewer70 must confirm the exact type names before the bin
uses them.

## Current persisted stage (real, retained stack)

Request `fc7e7311-c49e-4c7a-abd9-49c501cb1c09`, lead `2504f484…`.

- **EnvironmentPlan**: `plan_sha256
  c6cb4499d86380bd53dfe1c89a33882213b3e36af7e58250dee9f5d12acc377a`,
  revision `1`, status `CORE_VALIDATED`, created
  `2026-11-20T19:24:23.258355086+00:00`.
  Load: `Database::latest_environment_plan(request_id)`.
- **LicenseInventory**: the plan carries
  `license_inventory_sha256
  16b2df71a8a7996ee5b433655b0c6c1773d23113a350e4b5b8e6074375df2773`
  and `sbom_sha256
  57bfc9052d4417c2a5ce92617936e5550fc461beba3e7e741a212aef5f0fae2e`.
  This is the real plan inventory, not a raw/unmaterialised empty inventory.
- **EnvironmentSpec**: absent. `environment_specs` has no row for this request
  after the failed review. Do **not** use the plan hash as a spec hash and do
  not invent a spec. A spec-required consumer is **BLOCKED** until the spec is
  frozen.
- **Identity-facts receipt**: nonce
  `identity-facts:475c1f95e7d8450ab5d83e374c8c1377`, plan `c6cb4499…`,
  observed candidate/spec hash `067f8c8948e570e3ac2c8b8c8a9c47a0120759916b19e3c420308ff509d1b901`,
  fact count `73`, signature length `64`, created
  `2026-11-20T19:42:40.691572824+00:00`.
  Load: `environment_identity_facts_receipts` by plan/nonce. Verify the
  HMAC-SHA256 receipt with the existing broker bearer at
  `/config/runtime-broker-core.token` (path only; never print the token). The
  observed `067f8c…` is evidence of a candidate spec, **not** a frozen
  EnvironmentSpec row.
- **Evidence snapshot**: the exact shared asset
  `deploy/environment-builder/environment_builder/versioned_source_evidence.json`
  (revision `versioned-source-evidence-2026-11-20-v2`) with the raw
  `frkd`/`j151` ATerm, source flat/NAR/cache and licence-evidence hashes from
  `uat-ed-source-0fa4990e`.
- **Candidate**: no executable Campaign candidate exists. Use
  `CandidateBinding::Base { base_commit: 4fdf1b15f862b2564642443a71ef3e25a2785013 }`.
  Never hash the base commit and call it a candidate.

## Proposed typed stage (Core98)

```rust
pub enum CandidateBinding {
    Base { base_commit: String },
    Candidate { candidate_sha256: String },
}

pub enum StageBinding {
    PlanInventoryEvidence {
        plan_sha256: String,
        plan_revision: u32,
        inventory_sha256: String,
        evidence_bundle_sha256: String,
        identity_facts_receipt_digest: String,
        candidate: CandidateBinding,
    },
    SpecInventoryEvidence {
        plan_sha256: String,
        plan_revision: u32,
        spec_sha256: String,
        inventory_sha256: String,
        evidence_bundle_sha256: String,
        identity_facts_receipt_digest: String,
        candidate: CandidateBinding,
    },
}

pub struct PolicyBinding {
    // catalog/profile/policy hashes + stage + derived getters; private fields.
}
```

`PolicyBinding::new` validates the stage-specific hashes and the identity-facts
receipt. It does not require a spec for `PlanInventoryEvidence`. A spec-required
consumer (builder/environment freeze) checks the decision stage against the
current plan/spec/inventory:

- plan/inventory/evidence/policy/receipt equal and only the spec materialised →
  **REBIND** the same verdict to the spec stage (a new exact binding record, no
  new licence approval);
- any plan/inventory/evidence/policy/receipt change → **STALE** until a new
  exact decision;
- missing required plan/inventory/evidence/receipt → **BLOCKED/UNKNOWN**, never
  a fabricated hash;
- no generic framework, name/family approval or static GFDL profile allow.

## Producer/consumer order (no approval circle)

1. The producer compiles the plan + real inventory + evidence snapshot.
2. The Lead evidence review is recorded; the decision is bound to the
   `PlanInventoryEvidence` stage and the signed identity-facts receipt.
3. The environment review consumes that exact decision to produce the candidate
   inventory/spec; if the inventory hash changes, the decision is STALE and a
   fresh exact decision is required (or a rebind only when the evidence/policy
   are unchanged).
4. The builder never runs on a raw or missing inventory; a spec-required stage
   stays BLOCKED until the spec is real.
5. Office displays the stage (`PLAN`, `SPEC`, `STALE`, `BLOCKED`, `UNKNOWN`,
   `DENIED`) and the exact plan/spec/inventory/evidence/receipt digests and
   dates; no fake spec hash.

## Oracle

The independent oracle can load the exact rows above from the retained
`factory.sqlite`, verify the receipt HMAC with the broker token path, recompute
the evidence canonical hashes from the committed asset/ATerm/cache evidence,
and check that the decision/plan/inventory stage matches. It must not treat the
candidate spec hash in the receipt as a frozen spec.
