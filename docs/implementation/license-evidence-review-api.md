# Licence evidence review API

Status: implementation contract for the Core producer and Office control. No
runtime/model call is made by this document.

## Endpoints

```text
GET  /agentic/requests/{id}/license-evidence-review
POST /agentic/requests/{id}/license-evidence-review
body: { "delegation_id": "...", "idempotency_key": "..." }
```

Both require the operator authentication used by the other `/agentic` writes.
The caller supplies only the delegation id and idempotency key. Core resolves
the intake, Lead request, Project config, grant, delegation, current plan/spec,
inventory, identity-facts receipt and exact source proof from its own state.

## GET

Returns the exact settled review if one exists:

```json
{
  "available": true,
  "reservationId": "...",
  "requestSha256": "...",
  "review": {
    "decision": "ACCEPT",
    "pin": "github:NixOS/nixpkgs/...",
    "name": "ed",
    "version": "1.21.1",
    "licenseExpression": "GPL-2.0-or-later",
    "selectedBranch": "GPL-2.0-or-later",
    "useContext": "commercial-development",
    "paymentRequired": false
  }
}
```

With no settled reservation it returns `{ "available": false }`. It never
creates a reservation or calls a model.

## POST

1. Serialize per intake request.
2. Load and validate the active approved Owner delegation for the exact Project,
   matter `LICENSE_IDENTITY_EVIDENCE_DELEGATION`, option `APPROVE_DELEGATION`,
   unexpired and not revoked.
3. Require the current `EnvironmentPlan`; if a frozen spec exists, use the spec
   stage. Build the typed `InventoryEvidenceBundle` from the exact plan
   inventory, the `CandidateBinding::Base` from the Project base commit and the
   `PolicyBinding` from the real plan/spec, evidence bundle and HMAC-verified
   identity-facts receipt. A stale catalogue/profile or missing receipt fails
   before any model call.
4. Load the pinned exact `SourceEvidenceProof` (Ed25519 cache signature,
   flat/NAR/CA/size and source-to-output drv binding). Missing/invalid proof
   fails before any model call.
5. Require the same Lead-route gateway, frozen profile, grant and funding as
   `lead_assessment`/`lead_environment_review`: active subscription admission
   or explicit request-scoped monetary envelope, with the pricing quote only
   when the envelope is the sole source.
6. If a settled `license_evidence_review` reservation already exists, return it
   without a second model call.
7. Call `lead_model::review_license_evidence` with the Core-built prompt/schema
   and purpose `license_evidence_review`; the model has no tools and its output
   is advisory.
8. Return the reservation identity and the decoded review. The decision is
   persisted only by `POST /agentic/requests/{id}/license-decision`.

## Failure codes

`LICENSE_PLAN_REQUIRED`, `LICENSE_PLAN_STALE`, `LICENSE_IDENTITY_FACTS_REQUIRED`,
`LICENSE_IDENTITY_FACTS_INVALID`, `LICENSE_SOURCE_EVIDENCE_INVALID`,
`LICENSE_DELEGATION_REQUIRED`, `INFERENCE_AUTHORIZATION_REQUIRED`,
`LICENSE_EVIDENCE_MODEL_UNAVAILABLE`, `LICENSE_EVIDENCE_REVIEW_INVALID`.
A failed/unknown model call never settles a review and grants nothing.

## Office

`/licenses` lists Project intake requests, loads the GET status, runs the POST
only with an approved Owner delegation and records the exact decision using the
reservation id as the idempotency key. No name/version/family approval is
offered. The Office never creates a reservation by chat/SQL.
