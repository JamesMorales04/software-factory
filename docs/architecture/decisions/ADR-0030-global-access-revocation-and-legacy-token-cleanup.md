---
id: ADR-0030
status: Proposed for Product Owner review
date: 2026-10-29
owner: product-owner
supersedes: null
amends: ADR-0026, ADR-0028
---
# ADR-0030: Global access revocation and legacy token cleanup

- **Status:** Proposed for Product Owner review. The implementation has author-run Rust, Office and browser evidence; independent review of the code and policy candidate is recorded separately. Product Owner approval and promotion remain pending.
- **Amends:** [ADR-0026](ADR-0026-device-sessions-and-local-owner-bootstrap.md) for global session revocation and recovery, and [ADR-0028](ADR-0028-device-pairing-approval-boundary.md) for fencing pairing authority across global revocation.

## Context

Factory supports independent device sessions and a QR pairing flow. A global logout or recovery operation must not leave an in-flight challenge able to create a new session afterward. Office also had a retired operator token in `sessionStorage`; upgrades must remove that obsolete browser state without treating it as current authority.

## Decision

- Core stores a monotonic global access epoch. A pairing challenge records the epoch at issuance. Device revoke-all, logout-all and protected local recovery advance the epoch in the same authoritative database transaction that revokes active sessions.
- Those global revocation operations cancel outstanding `AVAILABLE`, `PENDING` and `APPROVED` challenges. Pairing approval consumes the challenge and creates the independent device session in one SQLite transaction only when the challenge epoch matches the current global access epoch. This fences delayed approval and polling across revocation. If pairing commits first, a later global revocation revokes the newly created session; if revocation commits first, the old challenge cannot issue one.
- Local recovery revokes and audits active device sessions, cancels outstanding challenges, clears the Owner bootstrap claim and issues a new confirmation code. The permanent operator credential remains available only through its protected internal recovery/API paths.
- Office deletes the retired `sessionStorage["factory-api-token"]` key during frontend initialization without reading, transmitting or logging its value. New authentication continues to use Core-issued sessions and CSRF protection.
- Session revocation, pairing cancellation and recovery remain Core-authoritative and produce durable audit records. The Tailscale transport does not grant Factory identity or authority.

## Consequences

A challenge created before a global revocation cannot later create a device session. A revocation racing with approval has a durable SQLite ordering and cannot leave a newly issued session active. Browser upgrades remove the retired token without copying it into another storage location or request. The change does not grant remote access, expand operator privileges or certify container isolation.

## Verification obligations

Test pairing approval delayed across logout-all, revoke-all and local recovery; concurrent approval and global revocation in both commit orders; cancellation of `AVAILABLE`, `PENDING` and `APPROVED` challenges; audit persistence; and upgrade behavior when the retired browser key is present. Keep browser cleanup tests separate from Core session authorization tests.

## Evidence

Author-run implementation and regression evidence is recorded in [personal-access-access-repairs-2026-10-29.json](../evidence/personal-access-access-repairs-2026-10-29.json). It records focused Rust access tests, Office legacy-token cleanup tests and a real Chromium pairing/revocation journey. The full Rust suite had three provider-sandbox fixture failures in that host environment; those failures are recorded and are not represented as access-test failures. Independent exact-SHA review outcomes and Compose policy evidence are documented in the corresponding review and policy records. Remote Serve over the current Unix-socket path remains `NOT_TESTED`; no SEC-01 claim is made.
