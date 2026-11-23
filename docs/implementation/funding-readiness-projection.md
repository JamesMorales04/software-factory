# Funding readiness projection

**Status:** Core projection implemented in `native/src/funding_status.rs` with focused tests. Core route/loader wire-up is proposed below for Core98/Root integration; it is **NOT_TESTED** until integrated. Office view and component tests are implemented; browser/axe is **NOT_TESTED** until a real candidate-bound run.

**Decision mechanism:** `DETERMINISTIC`. No model participates in funding readiness, admission selection or verdicts.

## Defect

`GET /funding/subscription/{provider}` described one provider connection and hard-coded `admissible: false`, `core_verifiable: false` and a static missing-evidence list. It ignored the durable request-scoped `subscription_scope_admissions` row that the real dispatch guard (`lead_model_funding` → `subscription_scope_admitted` → `require_scope_admission_in_tx`) evaluates for the exact Lead route. The static endpoint is now explicitly `scope: "UNSCOPED_PROVIDER_STATUS"`, `request_scoped: false`, still `admissible: false`, and its note says it cannot decide a request or Campaign.

## Request-scoped contract

Proposed read-only endpoint:

```text
GET /intake/requests/{id}/funding-readiness
→ funding_status::RequestFundingStatus (camelCase)
```

The endpoint is operator-authenticated and scoped to one intake request. It never returns a credential, base URL, request fingerprint, profile hash or credential fingerprint. Only the admission `receiptSha256` is exposed as a digest.

Core DTO fields:

- `schemaVersion`
- `scope { intakeRequestId, leadRequestId, complete }`
- `provider`, `model`
- `requestScoped`
- `coreVerifiable` — always `false`
- `requestScopedAdmissible` — mirrors the existing Core scope-admission guard for the exact Lead route
- `verdict` — `ADMITTED_FOR_REQUEST`, `SCOPE_REQUIRED`, `ADMISSION_MISSING`, `ADMISSION_INACTIVE`, `ADMISSION_EXPIRED`, `OWNER_ASSUMPTION_INVALID`, `PROVIDER_NOT_CONFIGURED`, `PROVIDER_DISABLED`, `SCOPE_MISMATCH`, `ROUTE_MISMATCH`, `SETTINGS_REVISION_MISMATCH`, `NON_CANONICAL_ROUTE`, `ROUTE_NOT_ADMITTED`, `EXTRA_USAGE_NOT_DISABLED`, `BINDING_REQUIRED`
- `admission` — exact recorded admission identity/state/route/expiry/Owner assumption/declaration bindings or `null`
- `missingEvidence[]`
- `recovery`
- `provenance[]`
- `note`

`requestScopedAdmissible: true` is not provider-issued entitlement and is not dispatch authority. The model gateway/Lead guard re-evaluates the durable admission before every provider call.

## Core hook (already implemented in `funding_status.rs`)

`project_request_scoped(&RequestFundingFacts) -> RequestFundingStatus` consumes:

- exact intake and Lead request ids;
- provider/model from the frozen profile Lead route;
- `RequestFundingProviderFacts { configured, enabled, settings_revision: String, canonical_route: bool }`;
- the stored `ScopeAdmissionRecord`;
- `scope_admitted: bool` from the existing `subscription_scope_admitted`/`require_scope_admission_in_tx` guard;
- `now_ms`.

It reuses `OwnerBillingAssumption::validate_at`, `TestRouteBinding::validate`, the test-route allowlist and Lead-role check. A false authoritative guard always produces `request_scoped_admissible: false` (`BINDING_REQUIRED`), even if the display facts look complete.

## Core route/loader patch proposal for Core98

Do not edit these files from this slice. Proposed integration:

1. Add a DB read method in `native/src/db/subscription_admissions.rs`:

```rust
/// Latest admission for one intake/Lead scope. ACTIVE rows are preferred;
/// otherwise the newest row is returned so Office can show
/// ADMISSION_INACTIVE/ADMISSION_EXPIRED truthfully.
pub async fn subscription_scope_admission_for_request(
    &self,
    scope: &str,
) -> Result<Option<ScopeAdmissionRecord>, Error> {
    let mut connection = self.pool.acquire().await?;
    let id: Option<(String,)> = sqlx::query_as::query_as(
        "SELECT admission_id FROM subscription_scope_admissions
         WHERE intake_request_id=?1 OR lead_request_id=?1
         ORDER BY CASE WHEN state='ACTIVE' THEN 0 ELSE 1 END,
                  created_at_ms DESC, admission_id DESC
         LIMIT 1",
    )
    .bind(scope)
    .fetch_optional(&mut *connection)
    .await?;
    match id {
        Some((admission_id,)) => load_scope_admission(&mut connection, &admission_id).await,
        None => Ok(None),
    }
}
```

2. Add an operator-authenticated route in `native/src/bin/core.rs`:

```rust
.route(
    "/intake/requests/{id}/funding-readiness",
    get(intake_funding_readiness),
)
```

3. Handler dataflow:

```text
authorized operator
→ app.db.intake_request(id)
→ app.db.execution_profile_revision(request.profile_revision_id)
→ profile revision content roles.lead.provider/model
→ app.db.provider_settings(provider)
→ app.db.subscription_scope_admission_for_request(request.id)
→ app.db.subscription_scope_admitted(request.id, provider, model)
   || app.db.subscription_scope_admitted(request.lead_request_id, provider, model)
→ canonical_route =
     provider != "opencode-go"
     || provider_catalog::is_canonical_opencode_go_base_url(base_url)
     || provider_catalog::is_test_upstream_url(base_url)
→ funding_status::project_request_scoped(&RequestFundingFacts { ... })
```

`settings_revision` is the settings revision string (`revision.to_string()`), matching `ScopeAdmissionRecord.settings_revision` and `TestRouteBinding.settings_revision`. Missing provider settings yields `provider_facts: None` and a fail-closed verdict. Do not derive readiness from the static provider endpoint.

## Office

- `coreApi.getIntakeFundingReadiness(id)` calls the new route.
- `FundingReadinessPanel` renders load, unavailable (404), retryable error, unscoped warning, admission identity/state/validity, Extra Usage assumption, declarations, routes, missing evidence, provenance, recovery and Core note. It never labels the result provider-verified or authorized.
- `RequestWorkspace` places the panel on the request route and refreshes it from the request `updatedAt`.
- `coreApi.getSubscriptionFunding(provider)` exposes the existing static endpoint in the provider card as `UNSCOPED_PROVIDER_STATUS`, with `NOT VERIFIED`/`Not satisfied` and no request-dispatch claim.
- All new strings are in EN and ES.

## Verification in this slice

Implemented and run:

```text
cargo test -p factory-native --lib funding_status
  → 18 passed
npx vitest run --no-file-parallelism
  src/views/FundingReadiness.test.tsx
  src/views/ProviderFundingStatus.test.tsx
  src/views/Settings.test.tsx
  src/views/RequestWorkspace.test.tsx
  → 55 passed
npm run typecheck
  → pass
```

`npm run lint` at the exact base reports pre-existing diagnostics in unrelated files (`App.tsx`, `Director.tsx`, `LicenseDecisions.tsx`, catalogue/licence entries and e2e formatting). Changed files pass targeted Biome checks; full-repository lint was not made green by modifying unrelated ownership.

**NOT_TESTED:** Core route integration, real browser/axe, 1440/390 rendering, real provider-session/dispatch evidence, independent acceptance.

## Risks / API gaps

- Until the Core route is integrated, Office shows an explicit unavailable state; it does not fall back to the static endpoint or infer an admission.
- The authoritative acceptance is the existing guard. If the route handler bypasses or reimplements that guard, the projection must fail closed.
- The static endpoint remains intentionally non-admissible; any consumer treating it as a request verdict is defective.
- Only the receipt hash is exposed; credential fingerprints, base URLs and request/profile fingerprints remain server-side.
