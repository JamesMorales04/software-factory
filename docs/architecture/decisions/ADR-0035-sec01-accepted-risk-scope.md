# ADR-0035 — SEC-01-ISOLATED scoped out for the TRUSTED_LOCAL product

**Status:** Accepted by the Product Owner on 2026-11-08.
**Relates to:** ADR-0009, ADR-0018, ADR-0024, ADR-0025.

## Context

SEC-01 established real failures and gaps: a host provider read a synthetic
canary outside its workspace, a same-UID process could read a colocated
synthetic credential, and mandatory mediation of every native tool path was
never proven. The later Rust stack improved the tested surface with per-Attempt
named volumes, Core-signed grants, lease fencing, an authenticated egress broker,
bounded cleanup and provider-session evidence. Those controls are useful, but
none of them is an independent SEC-01-ISOLATED certification.

ADR-0009 accepted the broad local-access risk for `TRUSTED_LOCAL` and kept
`SEC-01-ISOLATED` available-only-where-verified. The current developer-preview
product remains `TRUSTED_LOCAL` only. Continuing to treat SEC-01-ISOLATED as an
open product blocker would imply a security claim the product does not make and
a certification program the Owner has chosen not to fund now.

## Decision

1. The current product is **TRUSTED_LOCAL only**. `SEC-01-ISOLATED` is out of
   scope for this product and is not a blocker for the current developer-preview
   scope.
2. No surface may claim `ISOLATED`, `sandboxed`, `certified`, `SEC-01 PASS`,
   `credential-isolated` or equivalent. Office and CLI must keep the existing
   `TRUSTED_LOCAL — broad local access, not isolated` wording.
3. `ISOLATED` and `S3-ISOLATED` dispatch remain **fail-closed**. This decision
   does not authorize untrusted third-party code, hostile multi-tenant work,
   credential-bearing untrusted execution, remote isolated Workers or production
   deployment.
4. The existing controls remain in force because they reduce accident and
   attack surface even without certification: provider credentials stay in the
   trusted Core/Worker boundary, the provider sandbox receives only a per-run
   token, egress is broker-mediated, tool operations are mediated, workspace
   operations require grants, leases and fencing, and cleanup receipts remain
   mandatory. They are safety controls, not an isolation certificate.
5. The open SEC-001 reliability-debt item is recorded as **accepted residual
   risk for the current product**, not as a pending SEC-01 certification task.
6. This decision can be reopened only by a new Owner decision and a new ADR if
   the product scope adds isolated execution, untrusted third-party code,
   multi-tenant operation, a remote isolated Worker, a production deployment
   claim or any customer-facing isolation claim. Reopening requires the full
   SEC-01-ISOLATED program in [isolation.md](../../security/isolation.md):
   threat model, credential custody, OS-enforced topology, tool-mediation
   negatives, egress, resource limits, lifecycle/fencing tests, pinned image and
   binary digests, candidate-bound evidence and independent review.

## Consequences

- The product no longer presents SEC-01-ISOLATED as an unfinished feature. It
  presents the absence of that claim truthfully.
- Owner-authorized local development remains available with documented broad
  host access. This is not a sandbox, isolation, confinement or certification.
- A future isolated mode is a separate security campaign, not an incremental
  cleanup of the current `TRUSTED_LOCAL` path.
- Historical SEC-01 FAIL/NOT TESTED observations remain valid and are not
  rewritten as PASS.

## Verification

- Documentation/status update only; no runtime behavior changes.
- Architecture, parity and repository-hygiene checks must pass.
- A future change that adds isolation claims or untrusted execution must
  supersede this ADR and provide the full evidence in
  [isolation.md](../../security/isolation.md).
