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
2. The settings **revision** is not part of the declaration key. The
   admission separately records a route binding (role, model and settings
   revision) for freshness and audit; that binding is not the Owner
   declaration identity. Model, timeout,
   pricing or display changes do not change the upstream Extra Usage setting
   and therefore do not ask the Owner again.
3. A changed credential fingerprint or base URL, a cleared credential, or an
   explicit revocation invalidates the declaration **irreversibly**: switching
   the settings back cannot reactivate the old row. A later admission then
   requires a new declaration (or the legacy per-request confirmation),
   fail-closed. Model, timeout, pricing and display changes do not invalidate
   it because they do not change the upstream Extra Usage setting.
4. `PUT/DELETE /settings/providers/{provider}/extra-usage` are authenticated
   operations. In `TRUSTED_LOCAL` the authenticated operator token or paired
   device session **is** the Owner principal; there is no separate product
   Owner role to invent. `GET` returns the declaration state (provider, active,
   base URL, credential-bound flag, declaration id, dates, author, statement,
   revoked timestamp) and never credential material. Only `opencode-go`, with a
   canonical (or explicitly enabled test) endpoint, uses this declaration.
5. `POST /intake/requests/{id}/subscription-admission` reuses active
   declarations that cover every OpenCode Go route in the frozen profile. The
   admission still creates its own time-scoped `OwnerBillingAssumption`
   (24 hours) with the Owner declaration as its factual basis, and still
   validates routes, settings revisions, quota/funding state and profile
   identity. Every declaration id used is persisted on the admission and is
   re-checked as active **inside the admission write transaction**, so a
   concurrent revocation cannot be bypassed by a read-then-write race.
   Existing admissions are not extended or renewed by reuse. A stable
   `requestFingerprint` (the full client payload plus the exact request/Lead/
   profile identities and profile revision/hash) is persisted and returned for
   audit; an exact original replay survives revocation or unrelated settings
   changes, while any changed payload field under the same idempotency key is
   refused with `409`.
6. Revocation targets the provider's **current** configuration
   (`base_url` + credential fingerprint), never an arbitrary historical row. It
   stops future reuse. It does not retroactively cancel a Campaign or rewrite an
   already-recorded assumption, which expires within its bounded window and
   remains independent of Campaign/policy/lease authority.
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
- **Enable toggle.** Disabling and re-enabling the same provider account with
  the same endpoint and credential fingerprint does not re-prompt: the upstream
  Extra Usage setting is unchanged. A changed credential or endpoint does.
- **Limitation.** The declaration is an Owner statement; Core does not
  re-observe the upstream console and cannot prove the setting is still off. A
  remote change must be communicated by the Owner, who revokes the declaration
  or changes the configuration. The declaration itself has no expiry, while
  every admission assumption remains bounded to 24 hours.
