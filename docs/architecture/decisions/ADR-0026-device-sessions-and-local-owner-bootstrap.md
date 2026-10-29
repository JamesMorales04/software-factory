# ADR-0026: Device sessions and protected local Owner bootstrap

**Status:** Accepted  
**Date:** 2026-10-27  
**Decision owners:** Product Owner and Core  
**Scope:** Factory Office browser authentication and local Owner enrollment

## Context

Office previously required the operator to retrieve a permanent Core bearer
credential and keep it in browser session storage. That credential is also a
protected recovery/API mechanism and must not become a browser session secret.
The Factory remains local-first and may be reached through an explicitly
configured private HTTPS origin. A loopback HTTP request alone cannot establish
Owner identity.

## Decision

- Office uses random opaque device sessions in an HttpOnly, SameSite=Strict
  cookie. Core stores only SHA-256 session-token digests. Sessions have a fixed
  30-day lifetime, can be explicitly rotated, and can be revoked individually
  or together.
- Core issues a distinct CSRF token, stores only its digest, and sends it in a
  SameSite=Strict readable cookie. Browser state changes require that token in
  `x-factory-csrf` and an exact allowed Origin. Session requests reject
  ambiguous Authorization plus cookie credentials.
- Session middleware maps a validated device session to the existing operator
  authorization path. Verifier, Reviewer, Worker, Supervisor and broker
  credentials keep their existing route scopes and are never accepted as
  operator sessions.
- Browser HTTP is allowed only for loopback Host/Origin pairs. Remote browser
  access requires a full exact HTTPS origin from `FACTORY_TRUSTED_ORIGINS`; no
  wildcard Tailscale domain or forwarded identity header is trusted.
- A protected local Core container operation issues a random single-use Owner
  confirmation code with a five-minute expiry. The Owner enters that code once
  in Office. Core persists only its digest and expiry, and a unique database
  claim makes initial enrollment single-use across concurrent requests.
- Local recovery requires an explicit container operation. It revokes and
  audits every active device session, cancels outstanding pairing authority,
  clears the Owner bootstrap claim, and issues a new confirmation code. The
  permanent operator credential remains available only through its protected
  internal recovery/API paths.
- Office removes the retired `sessionStorage["factory-api-token"]` key during
  frontend initialization without reading, transmitting or logging its value.
  The upgrade path is covered by a test that seeds the legacy key, enrolls,
  reads the session and logs out.
- Versioned SQLite migrations add session and audit tables without deleting or
  rewriting existing Projects, Campaigns, credentials or evidence.

## Consequences

- Browser authentication survives reloads and Core restarts without exposing a
  permanent bearer credential to JavaScript.
- Mobile pairing can issue a separate device session after authenticated Owner
  approval; Factory authorization and spending policy remain Core-owned.
- First-run enrollment requires one explicit confirmation. Zero-touch Owner
  enrollment is not claimed because network location and Tailscale membership
  do not prove ownership.
- The host launcher depends on Podman Compose and a browser, not a host Rust or
  Node runtime. Local HTTP is restricted to loopback; configured private
  remote access must use HTTPS.

## Verification

Deterministic database and Core route tests cover digest-only lookup, rotation,
expiry/revocation behavior, owner bootstrap uniqueness, CSRF checks and local
recovery. Exact-head independent acceptance and real remote HTTPS evidence are
tracked separately; this ADR does not certify Tailscale or SEC-01 isolation.
