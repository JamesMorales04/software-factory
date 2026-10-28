# Office UX Wave 2 — per-role model routing (lane R)

**Status:** implemented on `campaign/office-ux-routing-20261028` (developer
preview, not accepted). **Owner:** lane R. **Contract:** shared Office UX
contracts 2026-10-28. **Evidence:** module and API tests below; no live provider
call was authorized or made, and no browser acceptance run was recorded.

## 1. What changed

Before this lane, one deployment default gateway served the Lead assessment and
semantic review. A profile was executable only when every role named that exact
provider/model. This lane replaces that restriction with bounded, exact role
resolution.

| Role | Route source | Transport | Executable in this build |
|---|---|---|---|
| Lead | frozen intake profile revision `lead`; legacy non-intake Lead keeps the deployment default | OpenAI-compatible `POST /chat/completions` (`provider_settings.kind = opencode`) | `opencode-go` or configured `opencode` |
| Semantic Reviewer | candidate → Attempt → Run → Campaign → immutable start evidence → frozen profile revision `semanticReviewer`; a Campaign without a frozen profile keeps the deployment default | OpenAI-compatible chat | `opencode-go` or configured `opencode` |
| Implementer | Campaign authorization built from the frozen profile `implementer` route and validated against the exact provider connection | OpenCode HTTP or Codex App Server provider session | both adapters |
| Evaluation (CAP-014) | explicit Evaluation route via the same factory; the `/model/calls` Evaluation endpoint still uses the deployment default | OpenAI-compatible chat | default only |

A Codex App Server connection is **not** an OpenAI-compatible gateway. It can
implement a Campaign, but a `codex` Lead or semantic-reviewer route is persisted
as a non-executable draft with a warning. The implementer Codex path is
unchanged.

## 2. Gateway construction

`RoleGatewayFactory` resolves a declared role route before any request:

1. load `provider_settings` for the exact provider (missing ⇒ typed error);
2. require `enabled`;
3. map `kind` through the explicit `ProviderAdapter` boundary;
4. require the adapter that implements the role transport;
5. validate the model ID shape and require Core to substantiate the model
   (configured model, baseline tested registry route or real provider
   discovery);
6. decrypt that provider’s credential inside Core (no credential is ever
   returned to Office);
7. intersect the profile role caps, the provider settings caps and the
   deployment caps; build a gateway bound to the role.

A role-bound gateway rejects a request for another role. There is no silent
fallback: an unavailable route returns a typed error and the caller returns an
explicit unavailable response.

Profile executability is derived from stored provider facts (configured,
enabled, credential present). Gateway construction performs the actual
decryption; a wrong master key, corrupt ciphertext or rotated credential fails
closed with `MODEL_CREDENTIAL_UNAVAILABLE` instead of using another provider.

## 3. Provider model discovery

`GET /settings/providers/{provider}/models` is operator-authenticated and:

- uses the stored credential with no proxy and no redirect following;
- calls the real OpenAI-compatible `GET {base_url}/models` with a bounded
  timeout and body;
- returns at most 200 validated model IDs;
- returns a typed `UNAVAILABLE` outcome with detail for denial, an unsupported
  adapter, a malformed body or a transport failure;
- never returns credential material and never invents model IDs.

The catalog is an in-memory cache on Core. An unavailable refresh replaces the
entry with an empty catalog, so a stale discovery cannot keep a route
executable. The cache is not persisted; after a Core restart, an
operator-triggered discovery or the provider’s configured baseline model is
required again for discovered-only IDs.

`codex` discovery is `UNAVAILABLE` by design: the App Server has no supported
model-catalog transport.

## 4. Profile executability

A profile revision is executable only when every declared role:

- names a configured, enabled provider connection with a stored credential;
- has an adapter that implements that role’s transport;
- names a model Core can substantiate for that connection.

Manual model IDs are accepted as **unverified drafts**; they are not rejected at
profile creation, are never bound as a default, and never reach a request. The
editor labels them explicitly.

`GET /execution-profiles` still returns `supportedRoutes` as the baseline
tested-route registry; it is no longer the complete set of routable models.

## 5. Authority and budgets

- Core still owns start authority, delegation capacity, risk, paths, Campaign
  authorization and the Quality Gate.
- Profile content never grants authority; a default binding still requires an
  executable revision.
- Each role call reserves its own model-call envelope using the intersection of
  route, provider and Core caps. This is a **per-call** intersection, not one
  durable request/Campaign ledger shared by assessment, review and
  implementation. Aggregate cross-role spending is therefore not yet bounded by
  a single profile Campaign ceiling; this is a blocking limitation for the
  "complete budget intersection" claim.
- Campaign authorization continues to carry the implementer route and its caps.
  A frozen Lead route may differ from the implementer route; Lead model-call
  authorization now validates against the immutable frozen Lead route rather
  than the Action/delegation implementer route. The deterministic full journey
  proves distinct Lead/Implementer routes end to end.
- The semantic-review receipt path validates the model-call route against the
  frozen reviewer route (a legacy Campaign without a frozen profile falls back
  to its recorded authorization route).

## 6. Tests

Rust:

- `context`: `native/src/model_routing.rs`, `provider_catalog.rs`,
  `provider_adapter.rs`, `execution_profile.rs`, `model_gateway.rs`,
  `db/execution_profiles.rs`, `db/model_calls.rs`, `bin/core.rs`.
- Covered: per-role gateway construction; credential selection per provider;
  Lead vs Reviewer vs Implementer role matrices; Codex one-shot honesty;
  unconfigured/disabled/credential-less/unadvertised failures; budget
  intersection; discovery against a fake HTTP server; typed `UNAVAILABLE`;
  candidate → frozen reviewer route and an actual `/model/calls` dispatch to
  the reviewer provider only.

Office:

- `ExecutionProfiles.test.tsx`: role provider matrix, searchable discovered
  models, unavailable discovery, unverified manual ID, persisted reload.
- `Settings.test.tsx`: discovery status, catalog display and staging a model.

## 7. Honest limits

- No live provider inference was performed in this lane. The fake upstreams
  prove routing, not provider compatibility.
- No Compose or Playwright acceptance run is claimed here.
- Discovered-only model IDs depend on the current Core process discovery cache.
- Codex one-shot assessment/review remains unsupported by design.
- The request composer already selected a saved profile and required no
  per-request provider/model input; it is unchanged.
