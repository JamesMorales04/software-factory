# Licence decision integration interface (Core98 ↔ integrator)

Status: implemented integration contract for the v4 selected-fact/derived-text
decision and the minimal autonomous licence-gate reuse. Core98 owns
`planning/license_decision.rs`,
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
`-only`/`-or-later` shortcut is accepted.

### v4 selected-fact / derived-text contract

Stored v3 decisions keep their v3 digest and verification. New decisions use
`contractRevision: license-review-v4-selected-grant-derived-text`:

- The Lead selects one exact `role=licensing` fact per selected atom
  (`purpose=header` or `source-version`) in `softwareTexts`. It must not put a
  `COPYING`/`purpose=license-text` asset there.
- Core derives exactly one compatible
  `purpose=license-text, role=license-text` asset per atom. Zero or multiple
  candidates fail closed; there is no family, name or suffix fallback.
- The decision and overlay record both: `licensingFacts` (selected path/sha/
  licence/atom), `licenseTexts` (derived path/sha/licence/atom),
  `derivationRule: SELECTED_LICENSING_FACT_TO_UNIQUE_LICENSE_TEXT`,
  `fileEvidence` (derived text), `packageRights` (selected fact) and per-document
  `licensingFactPath/Sha256` plus derived `licenseTextPath/Sha256`.
- Documentation may select the exact licensing fact for the output grant (the
  man page selects `ed.h`, GPL-2.0-or-later) or an exact text asset (the info
  page selects `doc/fdl.texi`, GFDL-1.3-or-later). Core records the derived
  compatible text (`COPYING`, GPL-2.0-only) and the exact output grant; it never
  assumes `COPYING` in the raw review.
- The raw model output, reservation, request digest, source proof, plan,
  receipt and lease are never mutated or silently rebound. The new decision is
  a new typed content digest.
- The builder accepts v3 and v4. v4 requires the selected fact, the unique
  compatible text, the exact output grant and the exact documentation
  projection; any mismatch is `ENVIRONMENT_LICENSE_DECISION_INVALID`/`STALE`.

The Owner delegation is project/pin/use/criterion/revision/deadline authority.
A project-scoped delegation may have `request_id = NULL`; `planSha256` and
`identityFactsReceipt` are optional context, not required fields. Each Lead
decision still binds the exact plan/spec/receipt/identity through the review and
decision context. The autopilot selects an exact request-scoped delegation
first, then the latest project-scoped one, and fails closed on expiry,
revocation, pin/use/criterion/revision mismatch or ambiguity.

The autonomous runner uses the same transitions. A licence gate is a typed
recoverable/owner wait (`ENVIRONMENT_LICENSE_UNKNOWN`, `LICENSE_*`), never a
generic terminal failure. With an existing approved Owner delegation and the
signed receipt it may reuse the normal Lead review (gateway exact replay, no
duplicate dispatch) and deterministic decision, then retry the freeze once. A
successful decision re-enqueues the same autonomous request; `REVIEW_PLAN` never
auto-starts. The runner never creates or renews a delegation, never widens
scope and never retries an ambiguous dispatch. Its request-scoped admission is
created only from a current durable Owner Extra Usage declaration covering the
exact provider, endpoint and credential; without that declaration it returns
`AUTOPILOT_OWNER_DECLARATION_REQUIRED` as a truthful owner wait and never
synthesizes an Owner confirmation. The builder marks any mismatch `STALE`
or `INVALID` (fail closed) and never approves a licence by name, package family
or a static GFDL profile allow.

The overlay gate is scoped to the exact current plan identity (`planSha256` and
`planRevision`). A historical decision for another revision never gates a new
plan's build/identity step. A decision that does match the current plan but is
revoked, denied or stale is not treated as absent: it still requires the exact
identity receipt and fails closed through the full overlay verifier.

The licence-review GET projection is scoped to the same exact current
plan/spec/identity-receipt/delegation context. If no exact settled review exists,
it returns `available:false`, `stale:true`, a bounded reason and the current plan
identity; it never projects the historical rev5 review as current. The POST
review path has no Lead-only early return: the gateway's exact
`(role, objective, request_sha256)` identity performs a settled replay, while a
new context gets a new reservation. The decision consumer recomputes the same
digest and selects only that exact settled reservation. The operator
idempotency key is bound through the existing durable idempotency store to the
exact request/plan/spec/receipt/delegation/grant context, so a reused key with a
different context is a conflict. This is a read-only projection change; no new
Office route or decorative action is added. Real browser acceptance of the
stale/empty projection remains NOT_TESTED.

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
