---
name: service-placement
description: Evaluate where a Software Factory component should run by choosing among an in-process module, an independent process, a persistent container, an ephemeral execution container or sandbox, and stronger isolation, while documenting trust, authority, credentials, state, lifecycle, resources, remote Worker impact, and required tests.
---

# Service placement

Use this skill when adding, moving, containerizing or reviewing a Factory component. It is an assessment aid, not an authority: it cannot approve a placement, grant credentials, waive isolation or certify a boundary. The canonical rules are in the policy; the required fields are in the template.

## Procedure

1. **Load the governing inputs.** Read the placement policy, ADR-0018, the isolation policy and the relevant component or Action. Identify the authorized scope and risk class.
2. **Define the component.** State its responsibility, proposed owner, trust classification, authority, and what it explicitly does not own.
3. **Inventory exposure.** List credentials, external effects, persistent state, filesystem and network access, untrusted code execution, lifecycle, resource bounds and remote Worker implications.
4. **Choose a baseline level.** Select LEVEL A–E using the eight decision rules. Record the simpler and stronger alternatives that were rejected, with reasons.
5. **Test the boundary.** Apply the shared-container/Pod rules and security invariants. Stop if untrusted code would share a credential-bearing boundary, host HOME, Core database, GitHub token, provider credential or privileged runtime socket.
6. **Specify evidence.** Name the negative tests, cleanup and reconciliation checks, restart behavior and independent review needed. State exactly what remains `NOT_CERTIFIED`, `BLOCKED` or `NOT_TESTED`.
7. **Complete the assessment.** Fill the template and set the decision status. Do not mark `ACCEPTED` without the governing authority and candidate-bound evidence.
8. **Reassess on change.** Re-run the assessment when trust, credentials, state, provider or runtime, topology, network, lifecycle or remote scope changes. Update the policy, ADR, tests and assessment atomically.

## Stop conditions

Stop and report rather than guessing when:

- authority, trust classification, allowed paths, risk class or budget is missing;
- untrusted code would run in an in-process module, persistent container or shared Pod with credentials or authoritative state;
- required isolation or credential confinement is unavailable, unverified, or would require host fallback;
- requested access includes provider credentials, GitHub tokens, Core authority or database, host HOME, or a privileged Podman/Docker socket in an untrusted workload;
- persistent state ownership or the single writer is unclear;
- lifecycle, cleanup, resource bounds or remote Worker behavior has no owner;
- an isolation claim lacks candidate-bound independent evidence;
- a decision would relax a security, quality or budget rule without the required approval.

## References

- [Service placement policy](../../../docs/architecture/service-placement-policy.md) — levels, eight decision rules, shared-container/Pod rules, mandatory questions, security invariants and the initial baseline table.
- [Assessment template](../../../docs/templates/service-placement-assessment.md) — required fields and decision statuses.
- [Isolation and trust boundaries](../../../docs/security/isolation.md) — current isolation disposition and negative acceptance tests.
- [ADR-0018](../../../docs/architecture/decisions/ADR-0018-native-rust-replacement.md) — Core, Worker and provider boundaries.
- [Rust engineering standards](../../../docs/engineering/rust-standards.md) — process, security, lifecycle and review rules.

## Evidence boundary

Placements are hypotheses until approved. Rootless Podman, containers, Pods, remote Workers, protocol probes and TRUSTED_LOCAL runs are not certified isolation. Record `NOT_CERTIFIED` or `BLOCKED` honestly; never turn an untested boundary into a `PASS`.
