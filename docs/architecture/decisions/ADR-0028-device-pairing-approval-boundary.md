---
id: ADR-0028
status: Proposed for Product Owner review
date: 2026-10-27
owner: product-owner
supersedes: null
amends: ADR-0026
---
# ADR-0028: QR device pairing and approval boundary

- **Status:** Proposed for Product Owner review. The 2026-10-29 runtime repair `7bca751` has author-run Rust and real Chromium pairing/revocation evidence; static socket-volume policy hardening is committed at `d2fa767`, alias resolution at `d0d4040`, and implicit external-volume protection at `d2783e4`. Independent review, remote Tailscale HTTPS, mobile-data UAT and Product Owner promotion approval remain pending.
- **Amends:** [ADR-0026](ADR-0026-device-sessions-and-local-owner-bootstrap.md) with the device enrollment and approval boundary.

## Context

Factory Office needs a safe way to enroll a phone without copying a permanent operator credential. QR content is retained by camera/browser applications and can appear in browser history, so it must not contain a bearer secret. Connectivity through a private network does not establish Factory identity or grant application authority.

## Decision

- Core creates a short-lived random challenge and persists its lifecycle and audit records. A QR carries only the challenge identifier, which grants no Factory access by itself.
- A single phone claim changes the challenge from `AVAILABLE` to `PENDING`. Core returns a separate random poll capability to the claimant and persists only its hash. The claimant presents it in a POST body, never in a URL.
- The phone and authenticated Office display the same short verification code. The code is generated from `ring::rand::SystemRandom` with rejection sampling, so all one million values are equally likely. Core approves only when the authenticated operator submits the matching code. A per-challenge persisted failed-attempt counter cancels the challenge after five invalid approvals. Rejection, expiry, replay, throttling and a second claim fail closed.
- Core consumes approval and creates the independent device session in one SQLite transaction. The transaction requires the challenge's approval epoch to equal the current global access epoch. Logout-all, revoke-all and local recovery advance that epoch and cancel outstanding `AVAILABLE`, `PENDING` and `APPROVED` challenges, so an approval delayed across a global revocation cannot issue a session. A revocation that commits after the exchange revokes the session the exchange created. The raw session and CSRF values are returned only through the session cookies and the CSRF response contract; session storage contains hashes. A failed session issue requires a new pairing challenge.
- Per-device and all-device revocation are Core operations and are audited. Tailscale supplies private transport only; it does not provide a Factory identity or session.
- The claim and polling routes are capability-scoped public operations under strict Core Host/Origin checks. Device-management operations require an authenticated Factory operator session and CSRF validation.

## Consequences

The QR can be copied without transferring a reusable secret. An unauthorized claimant can at most occupy one short-lived pending challenge; the owner sees the claimant's code and must reject a mismatch. Pairing does not bypass initial Owner enrollment, Core budgets, campaign authorization or Quality Gate authority.

## Verification obligations

Test one-time claim, expiry, replay, matching and mismatching verification codes, approval, rejection, cancellation, concurrent exchange, failed-attempt throttling under concurrency, delayed polling after logout-all/revoke-all/recovery, concurrent exchange versus global revocation, independent session creation, session revocation, unauthenticated management denial, CSRF/Origin/Host negatives and audit persistence. Real HTTPS remote access and real mobile browser behavior remain separate acceptance obligations.

## Evidence

The 2026-10-29 author-run evidence records the runtime implementation at `7bca751` and socket-volume policy hardening at `d2fa767`/`d0d4040`/`d2783e4`: access-epoch fencing, atomic exchange, CSPRNG code generation, persisted per-challenge throttling, delayed-poll and concurrent-revocation tests, and a real Compose/Chromium desktop-and-phone journey. That journey confirms matching-code approval, an independent phone session, replay rejection, single-device revocation and responsive layouts; the Rust tests cover failure throttling, cancellation, global revocation and concurrent exchange. The full package test run had three provider-sandbox fixture failures in this host environment; they are recorded separately and are not represented as pairing failures. See [personal-access-access-repairs-2026-10-29.json](../evidence/personal-access-access-repairs-2026-10-29.json) and the earlier [personal-access-pairing-2026-10-28.json](../evidence/personal-access-pairing-2026-10-28.json). Mobile-data Tailscale access, independent review and Product Owner approval remain pending.
