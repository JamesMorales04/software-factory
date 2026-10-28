---
id: ADR-0028
status: Proposed for Product Owner review
date: 2026-10-27
owner: product-owner
supersedes: null
amends: ADR-0026
---
# ADR-0028: QR device pairing and approval boundary

- **Status:** Proposed for Product Owner review. Implementation is not accepted or certified by this proposal.
- **Amends:** [ADR-0026](ADR-0026-personal-access-and-device-sessions.md) with the device enrollment and approval boundary.

## Context

Factory Office needs a safe way to enroll a phone without copying a permanent operator credential. QR content is retained by camera/browser applications and can appear in browser history, so it must not contain a bearer secret. Connectivity through a private network does not establish Factory identity or grant application authority.

## Decision

- Core creates a short-lived random challenge and persists its lifecycle and audit records. A QR carries only the challenge identifier, which grants no Factory access by itself.
- A single phone claim changes the challenge from `AVAILABLE` to `PENDING`. Core returns a separate random poll capability to the claimant and persists only its hash. The claimant presents it in a POST body, never in a URL.
- The phone and authenticated Office display the same short verification code. Core approves only when the authenticated operator submits the matching code. Rejection, expiry, replay and a second claim fail closed.
- Core consumes approval atomically before issuing a new independent device session. The raw session and CSRF values are returned only through the session cookies and the CSRF response contract; session storage contains hashes. A failed session issue requires a new pairing challenge.
- Per-device and all-device revocation are Core operations and are audited. Tailscale supplies private transport only; it does not provide a Factory identity or session.
- The claim and polling routes are capability-scoped public operations under strict Core Host/Origin checks. Device-management operations require an authenticated Factory operator session and CSRF validation.

## Consequences

The QR can be copied without transferring a reusable secret. An unauthorized claimant can at most occupy one short-lived pending challenge; the owner sees the claimant's code and must reject a mismatch. Pairing does not bypass initial Owner enrollment, Core budgets, campaign authorization or Quality Gate authority.

## Verification obligations

Test one-time claim, expiry, replay, matching and mismatching verification codes, approval, rejection, concurrent poll consumption, independent session creation, session revocation, unauthenticated management denial, CSRF/Origin/Host negatives and audit persistence. Real HTTPS remote access and real mobile browser behavior remain separate acceptance obligations.
