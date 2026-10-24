# ADR-0021: Rust runtime implementation status

- **Status:** Accepted
- **Date:** 2026-10-24
- **Relationship:** Updates implementation-status statements in ADR-0019; does not change the container-first direction, trust model or security policy.

## Context

ADR-0019 recorded the container-first direction before the Rust lifecycle,
container image and managed host execution existed. Those statements are now
stale. The architecture decision itself remains valid, but current
implementation status must be recordable without pretending the original
decision text was never written.

## Decision

1. The Rust `factory` CLI implements `doctor`, `start`, `status`, `logs` and
   `stop`. `install`, `restart`, `update` and `dev` remain unimplemented.
2. `deploy/Containerfile.core` builds the Core/Office runtime image. The Core
   serves the built Office and supports explicit `FACTORY_CONTAINER_MODE=1`
   loopback-published container deployment.
3. `factory start` manages a host Supervisor and Worker when explicit project
   roots are configured. The Supervisor uses rootless Podman named volumes;
   no Podman socket enters a workload.
4. These are author-verified implementation statuses, not independent
   certification, production deployment, Windows acceptance or isolation.
5. Provider-backed coding, Quadlet/installer automation, strict zero-Go
   hygiene and independent review remain open.

## Consequences

- ADR-0019's direction and security constraints remain in force.
- The implementation-status sentences in ADR-0019 are superseded by this
  record; the original decision rationale is preserved in Git history.
