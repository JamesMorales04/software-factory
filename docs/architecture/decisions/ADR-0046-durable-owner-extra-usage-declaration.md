# ADR-0046 — Durable Owner Extra Usage declaration

Status: accepted (implementation slice; no licence or delivery authority).

## Context

The Owner confirms that the upstream OpenCode Go **Extra Usage** setting is
disabled before a subscription admission. That confirmation was requested again
for every manual admission, even though it is a statement about one provider
account configuration and does not change between requests. The autonomous path
already derived a 24-hour funding assumption from a durable policy grant, but a
manual request had no durable provider-level declaration.

An Owner declaration must not become a policy, budget, routing, lease or
delivery authorization. It is evidence about one provider configuration; every
admission still passes the route, settings, profile and campaign checks.

## Decision

1. Persist one durable Owner declaration per exact provider configuration in
   `provider_extra_usage_declarations`, bound to the real configuration
   identity already stored by Core:
   - `provider`,
   - canonical `base_url`,
   - the existing `credential_fingerprint` (never the credential itself).
2. The settings **revision** is not part of the binding. Model, timeout,
   pricing or display changes do not change the upstream Extra Usage setting
   and therefore do not ask the Owner again.
3. A changed credential fingerprint or base URL, or an explicit revocation,
   invalidates the declaration. A later admission then requires a new
   declaration (or the legacy per-request confirmation), fail-closed.
4. `PUT/DELETE /settings/providers/{provider}/extra-usage` are authenticated
   operator operations; `GET` returns only the declaration state (dates, author,
   statement, active flag), never credential material. Only `opencode-go` uses
   this declaration.
5. `POST /intake/requests/{id}/subscription-admission` reuses active
   declarations that cover every OpenCode Go route in the frozen profile. The
   admission still creates its own time-scoped `OwnerBillingAssumption`
   (24 hours) with the Owner declaration as its factual basis, and still
   validates routes, settings revisions, quota/funding state and profile
   identity. Existing admissions are not extended or renewed by reuse.
6. Revocation stops future reuse. It does not retroactively cancel a Campaign or
   rewrite an already-recorded assumption, which expires within its bounded
   window and remains independent of Campaign/policy/lease authority.
7. The legacy explicit per-request confirmation remains accepted when no
   declaration covers the route; it does not create a durable declaration
   implicitly.

## Consequences

- The Owner is asked once per real provider configuration; Office shows the
  persisted state and a revocation control instead of a repeated checkbox.
- The declaration is factual evidence only. It grants no model call, budget,
  route, retry, Campaign or delivery authority, and it never claims provider
  verification.
- A stale configuration cannot silently reuse a declaration: the exact base URL
  and credential fingerprint are required, and revocation is immediate for new
  admissions.
- No new provider/account identifier is invented. If a provider does not expose
  an account identifier, the existing configuration fingerprint remains the
  binding.
