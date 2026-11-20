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

The environment plan carries an exact `licenseDecision` receipt per affected
inventory entry:

```
licenseDecision: {
  decisionId, decisionSha256, decisionStatus,       // ACCEPT / DENIED / STALE
  identity { pin, name, version, outputStorePath, outputNarHash },
  licenseExpression, selectedBranch, documentationLicense,
  obligations, documentationObligations,
  evidenceSha256, catalogSha256, profileSha256, policySha256,
  specArtifact, inventoryArtifact, candidateArtifact, identityFactsReceiptDigest
}
```

The builder recomputes canonical sorted/compact/LF hashes for the evidence,
output files, raw `.drv`, cache and receipt, checks the current
catalogue/profile/policy/spec/inventory/candidate binding and marks a mismatch
`STALE` (fail closed). It never approves a licence by name, package family or a
static GFDL profile allow. GFDL is classified in the catalogue but allowed only
by the exact decision.

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
