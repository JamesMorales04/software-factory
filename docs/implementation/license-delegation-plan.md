# License, identity and evidence delegation — minimal implementation plan

Status: candidate plan under the Owner's explicit 2026-11-20 delegation. No
runtime activation, rebuild, model call or Campaign is started by this document.
Reviewer70 review is required before build/calls.

## Authority

The Owner delegates licence, identity and evidence decisions in this domain to
the Lead under the Owner charter. The sole economic/commercial limit is:

- no mandatory licence fee, royalty or paid activation for commercial
  development/use;
- the licence permits building, using and distributing commercial products (or
  a free branch applies to the intended use);
- copyleft, attribution and source obligations are allowed and must be met and
  recorded; they are not payment and are not a permissive/closed-source-only
  requirement;
- optional paid support, paid distribution or dual licensing do not disqualify;
- non-commercial/research-only terms, mandatory royalties or a payment
  obligation for the intended use are denied;
- unknown terms require exact evidence and fail closed; the Lead investigates
  instead of a routine Owner re-ask.

This does **not** extend to provider payment, merges, sandbox, credentials or
the Quality Gate. Those remain separate authorities. No `8893`, retention or pin
change is part of this plan.

## Existing components to reuse

- `OwnerCharter` and the `/agentic/charters` API: the durable Owner domain and
  expiry frame. `policy_autonomy` stays false; this plan does not turn a charter
  into a generic policy grant.
- `agentic_owner_decisions` and its authenticated operator boundary: the audit
  pattern for typed Owner decisions, including revocation.
- `docs/contracts/lead-evidence-classification.md` and
  `native/src/planning/lead_classification.rs`: the typed `EvidenceBundle`,
  `NEEDS_EVIDENCE`/`ACCEPT`/`DENY` vocabulary and the non-authorizing dictum.
  The dictum is activated only through the new Core record, never as a grant.
- `VersionedSourceEvidence` and the shared
  `deploy/environment-builder/environment_builder/versioned_source_evidence.json`
  candidate: exact pin/output/source/NAR/licence-evidence/output evidence for
  the bootstrap `ed` identity.
- The existing catalogue/profile evaluator and the minimal
  `GFDL-1.3-or-later` closure diff in this candidate.
- `IdentityFactsReceipt`/`VerifiedIdentityFacts`: the broker HMAC receipt and
  exact facts anchor already used by `SourceLicenseOrigin`.
- Office `EnvironmentPanel` (licence blockers/inventory) and `Charter` view;
  the new controls must be Core-backed and browser-tested.

## New durable records

Add migration `00xx_license_delegation.sql`:

`license_delegations`

- `id`, `project_id`, `owner_charter_id` (nullable), `pin`, `domain`
  (`licenses-identity-evidence`), `scope_json` (exact identities/use contexts),
  `economic_criterion`, `created_by`, `revision`, `created_at`, `expires_at`,
  `revoked_at`, `revocation_reason`.

`license_decisions`

- `id`, `delegation_id`, `project_id`, `lead_request_id`,
  `lead_model_call_reservation_id`, `lead_model_output_sha256`,
  `identity_json` (pin/name/version/output drv/path/NAR/source FO drv/path/URL/
  outputHash/licence evidence/output files), `license_expression`,
  `documentation_license`, `evidence_sha256`, `provenance_json`,
  `use_context`, `obligations_json`, `decision` (`ACCEPT`/`NEEDS_EVIDENCE`/
  `DENY`), `revision`, `created_at`, `revoked_at`.

The decision is immutable per evidence/use-context revision. A changed use,
licence expression, source hash or pin requires a new decision; no update in
place.

## API and actor fence

- `POST /agentic/license-delegations` — authenticated operator/Owner boundary;
  creates one durable delegation. `GET` lists; `POST /{id}/revoke` revokes with a
  reason. These are user-facing Office actions.
- `GET /agentic/license-decisions?project_id=...` — authenticated read-only
  audit projection for Office.
- `POST /agentic/requests/{id}/license-decision` — operator-triggered bounded
  Lead evidence review. The operator can trigger the review but cannot select
  the licence, evidence or decision. The Lead model output is schema-bound; Core
  validates it.
- `LeadActor` is a private Core type constructed only by the Core decision
  handler from the exact `lead_request_id`, the SETTLED
  `model_call_reservations` row for purpose `license_evidence_review`, the
  request/grant/profile binding and the output digest. A model, chat message,
  operator token or external caller cannot construct it. The Lead actor is not
  the operator; no synthetic approval from chat is accepted.
- Core validates the active delegation (project, pin, exact identity, use
  context, expiry, revision, revocation), the exact evidence, the existing
  catalogue/profile verdict and the economic/commercial criterion, then records
  and applies the decision. A log entry or model dictum is never a grant.

## Lead evidence call and budget

- One bounded Lead-route structured call, purpose `license_evidence_review`, with
  the exact `VersionedSourceEvidence` and licence-text digests; no tools, no
  egress, no free-form authority.
- The fetched source is a separate fixed-output proof, not an invented identity
  fact. `VerifiedSourceEvidence` is a private Core type produced only by
  verifying the pinned `cache.nixos.org-1` Ed25519 signature, the exact
  SRI/Nix-base32/fingerprint/CA/size derivation and the source→output
  `inputDrvs` membership; no caller boolean or deserialised struct can stand in
  for it.
- Every evidence, source, plan/spec, policy, inventory, delegation, grant,
  receipt and root-binding guard runs before `gateway.call`. A refusal therefore
  spends zero model calls and creates zero reservations; the producer's tests
  assert the 0/1 call boundary with a fake dispatcher.
- The source proof has one canonical representation shared with the builder:
  `deploy/environment-builder/environment_builder/source_evidence_proof_vector.json`.
  Core and the builder hash the same sorted compact JSON plus LF to the same
  digest; there is no `signatureValid` or caller membership boolean, and a Rust
  test plus a builder test consume the same vector. A drift in either
  representation fails both suites.
- Execution receives the existing builder `licenseDecisions` overlay, never a
  second approval path. Core re-verifies the persisted decision with
  `verify_license_decision_overlay` against the exact current project, plan,
  spec, inventory, evidence bundle, candidate base, catalogue/profile/policy,
  delegation deadline and revocation, identity and pinned source proof before
  emitting the overlay. The same verified overlay is delivered to the resolve
  and build operations; a stale, revoked or expired decision refuses and the
  environment stays `FAILED`. A failed realisation is observed as a
  `RESOLUTION`-stage identity receipt and never as an accepted build.
- The call reuses the frozen Lead route and the request-scoped subscription
  admission already active for `fc7e7311`; no new provider configuration,
  credential or monetary envelope. Token/cost telemetry is recorded, not used
  as a productivity cap.
- If the call is unavailable, the state is `NEEDS_EVIDENCE`/blocked, not an
  Owner escalation and not a fake acceptance. Reviewer70 must approve the call
 /budget before it is made.
- No Planner or Campaign start until the environment review passes with the
  accepted decision.

## Policy candidate

The catalogue classifies only `GFDL-1.3-or-later`; the global profile does not
allow it and its hash is unchanged. The allow is the exact Core
`license_decision` for the exact identity/evidence/use context/revision under
the active Owner delegation; `build`/`devTest` do not inherit a GFDL allow,
`selected`/`runtimeDistributed` are unchanged, and there is no global allow.
The exact hashes and evidence are in `ADR-0047`,
`versioned_source_evidence.json`, and the evidence bundle
`uat-ed-source-0fa4990e` (`OWNER-DECISION.md`, `CORRECTIONS.md`,
`PROPOSED.patch`, policy patches, raw `.drv` ATerm bytes, source/cache hashes).
This is a delegated exact decision, not a static manual per-licence approval.

## Core/builder changes

- `native/src/planning/environment.rs`: `VersionedSourceEvidence` struct,
  `ClosureLicense.versionedOrigin`, exact validation, identity-facts bind,
  catalogue/profile evaluation of software and documentation licences.
- `native/src/db/`: delegation/decision records, migration, audit/revocation.
- `native/src/bin/core.rs`: the delegation/decision/audit endpoints and the
  private Lead actor handler.
- `deploy/environment-builder/environment_builder/policy.py`,
  `cli.py`: load the shared asset, attach the exact evidence for matching
  closure paths, reject mismatch/missing/tamper, include the SBOM/closure
  entries.
- `deploy/environment-builder/environment_builder/versioned_source_evidence.json`:
  the exact candidate data.
- `native/src/planning/lead_classification.rs`: activate the dictum only through
  the Core record; keep model output non-authorizing.
- Tests in `native/src/planning/environment.rs`, DB/API tests, builder tests,
  Office component/browser tests.

## Office parity

- Charter view: Owner creates/reads/revokes the `licenses-identity-evidence`
  delegation with exact scope/expiry; no decorative control.
- EnvironmentPanel: for a blocked/unknown licence, show the exact identity,
  licence expression, source/evidence digests, use context, delegation state,
  Lead decision and audit/revocation; expose only existing Core actions.
- States: `DELEGATION_REQUIRED`, `EVIDENCE_REQUIRED`, `DECISION_RECORDED`,
  `DENIED`, `REVOKED`, `STALE`; responsive and accessible; real browser
  acceptance before any claim.
- Update `docs/product/office-parity.json` and the capability registry.

## Acceptance matrix

1. Delegation create/read/revoke/expiry/restart; operator only; no model/chat
   grant.
2. Lead actor binding: exact request + model reservation + output digest;
   forged/missing actor rejected.
3. Exact identity: pin/output drv/path/NAR/source FO hash/licence evidence;
   any mismatch/unknown/tamper fails closed.
4. Economic criterion: no-payment/commercial-use accepted; NC/research-only/
   mandatory royalty/paid activation denied; optional paid support/dual licence
   with a free branch accepted only when the free branch applies.
5. Policy: GFDL closure/build/devTest allowed after the decision; selected
   UNKNOWN; no name/version or `meta` inference.
6. Builder/Core shared-asset equality and SBOM/inventory rebind; no historical
   rewrite.
7. Office component/browser: delegation form, blocked/evidence/recorded/denied/
   revoked states, audit, responsive/a11y.

## Worktree / next SHA

This candidate is in the single integration worktree; the next commit contains
the policy data, shared asset, ADR and this plan. Reviewer70 reviews the SHA and
the evidence bundle before any Core/builder/Office build, model call, Planner or
Campaign. No `8893`, retention, pin or SQL-bypass change.
