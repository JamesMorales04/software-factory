# Licence decision integration interface (Core98 ↔ integrator)

Status: coordination contract for the split backend/integrator work. No runtime
change. Core98 owns `planning/license_decision.rs`,
`planning/lead_classification.rs`, `db/license_decisions.rs`, migration `0084`
and the model-call changes; the integrator owns the Core bin endpoints,
builder and Office. Both sides must use the exact names below before replacing
placeholders.

## Backend types/functions (Core98)

- `VerifiedLicenseDecision`: private fields, no `Deserialize`; includes exact
  identity, software/documentation licences, selected branch, evidence digest,
  provenance, use context, obligations, decision, revision and a typed binding.
- `BindingArtifact`: `Frozen { sha256 }` or `Base { base_commit }`. Never use
  `project_config_hash` as a spec or inventory digest and never hash the base
  commit as a candidate. A missing required artifact yields `UNKNOWN/blocked`.
- `PolicyBinding`: catalogue/profile/policy hashes, `spec`/`inventory`/`candidate`
  artifacts, `identity_facts_receipt_digest`.
- Core verifies the exact SETTLED `license_evidence_review` reservation:
  request SHA equals the canonical Core input, output SHA is recomputed from
  `response_text`, role/purpose/project/objective/lead/intake equal, lead
  nonterminal and not withdrawn, expiry valid, principal valid.
- Core verifies the Owner delegation:
  `agentic_owner_decisions` matter `LICENSE_IDENTITY_EVIDENCE_DELEGATION`,
  status `APPROVED`, selected `APPROVE_DELEGATION`, project/pin/use-context/
  criterion/revision equal, not expired and not in
  `license_delegation_revocations`.
- The DB transaction stores the full binding, software/documentation obligations,
  selected branch, identity-facts receipt digest and a canonical full-content
  hash; idempotency compares that hash and unique conflicts return 409; content
  is immutable except audited revocation.

## Core bin endpoint (integrator, after backend types)

`POST /agentic/requests/{id}/license-decision` accepts only
`{delegation_id, idempotency_key}`. It loads the exact reservation, model output,
delegation, **real** EnvironmentSpec/LicenseInventory/plan or a typed Base
context and the current candidate (or `Base`). It calls the agreed backend
verifier and writes the decision. No licence/identity/payment/obligation field
is accepted from the caller.

`GET /agentic/license-decisions?project_id=...` returns the exact records and
binding/status; `POST /agentic/license-decisions/{id}/revoke` audits a bounded
reason/principal. Delegation revoke uses
`POST /agentic/license-delegations/{id}/revoke`.

## Builder plan receipt / STALE (integrator)

The environment plan carries an exact `licenseDecisions` overlay emitted by
Core after re-verifying the persisted decision against the current plan, spec,
inventory, policy, identity-facts receipt, delegation lifetime and source
proof. The overlay includes the v3 file/package/document rights projection:

```
licenseDecision: {
  id, projectId, planSha256, planRevision, specSha256,
  inventorySha256, evidenceBundleSha256, candidate,
  identity { pin, name, version, output, source, licenseEvidence, outputFiles },
  licenseExpression, selectedBranch,
  softwareObligations, documentation, documentationObligations,
  softwareVerdict, evidenceSha256, identityFactsReceiptDigest,
  catalogSha256, profileSha256, policySha256, stage,
  useContext, commercialUse, paymentRequired, decision, revision,
  status, revokedAt, delegationDeadline,
  decisionContentSha256,                 // domain: stored decision payload
  contractRevision: "license-review-v3-file-package-rights",
  fileLabelSource: "ASSET",
  packageRightsSource: "LICENSING_FACT",
  documentationGrantSource: "OUTPUT_FILE_ASSET",
  modelTextLicenseIgnored: true, modelObligationsIgnored: true,
  fileEvidence, packageRights, documentationGrants,
  sourceEvidence, sourceEvidenceSha256,
  contentSha256                          // domain: emitted overlay bytes
}
```

`decisionContentSha256` and the overlay `contentSha256` are different digest
domains and are never compared for equality. Core rebuilds the stored decision
payload from the row/provenance and verifies `decisionContentSha256` before it
emits the overlay. The builder does not read the decision database and does not
recompute `decisionContentSha256`; it verifies the overlay digest and binds the
decision digest, plan/receipt, assets, source proof and v3 facts. A changed
decision digest without a matching overlay digest is rejected.

`fileEvidence`/`packageRights`/`documentationGrants` are exact projections of
the asset-owned licence facts, selected branch and output grants. The model's
text label is diagnostic only. A `GPL-2.0-only` text may support the exact
`GPL-2.0-or-later` grant only through the typed relation; no family, suffix or
`-only`/`-or-later` shortcut is accepted. The builder marks any mismatch `STALE`
or `INVALID` (fail closed) and never approves a licence by name, package family
or a static GFDL profile allow.

Office parity impact: **none**. This is an internal Core-to-builder receipt. No
Office route, control, label, state, projection or user action changes; the
existing licence-decision audit surface continues to read the Core decision
record and does not invent overlay state.

## Office (integrator)

- Delegation: create/read/resolve/revoke through the existing OwnerCharter/
  agentic-owner-decision primitives; the Owner-decision evidence carries the
  exact domain/pin/use-context/criterion/revision.
- Decision audit: list `licenseDecisions`, show `ACCEPT`, `UNKNOWN`, `STALE`,
  `DENIED`, evidence/binding digests, selected branch and obligations.
- Real browser matrix at `18893` covers create/read/revoke, UNKNOWN/STALE/DENIED,
  empty/error/loading and responsive/a11y states; no decorative action.

## Conflict rules

- Core98 owns the backend paths; the integrator does not edit them or cherry-pick
  while the backend worktree is dirty.
- The integrator owns `native/src/bin/core.rs`, `deploy/environment-builder/**`
  and `office/**`.
- Integrate only after Core98 reports its commit SHA; rerun focused tests and
  freeze the integrated SHA for 70/de9. No model call, image build or runtime
  activation before that freeze and review.
