---
id: ADR-0009
status: Accepted
date: 2026-10-19
owner: product-owner
supersedes: null
---

# ADR-0009: TRUSTED_LOCAL execution mode with accepted local-access risk

## Context and problem

SEC-01 observed facts remain valid: a host Paseo agent read a synthetic canary outside its workspace (D1 FAIL), a same-UID process read a colocated synthetic credential (D2-C FAIL), and mandatory mediation of every native tool path was never proven (D3-D2 NOT TESTED). See [SEC-01 results](../../evidence/history/security/SEC-01-results-2026-10-18.md).

The product owner has issued a binding sequencing decision for the TRUSTED_LOCAL migration campaign (branch `f1-s2-trusted-local`): broad local access by trusted local tooling is explicitly accepted with documented risks. SEC-01-STRICT (mandatory credential isolation from every native tool) is NOT REQUIRED for TRUSTED_LOCAL. This supersession rests on informed risk acceptance, not on proof of isolation.

## Decision drivers and invariants

- Preserve truthful security reporting: historical FAIL/NOT TESTED findings stay valid observations and are never rewritten to PASS.
- Never present TRUSTED_LOCAL as a sandbox, an isolation certification, or evidence that credential confinement holds.
- Keep ISOLATED guarantees narrow and evidence-bound to actually tested boundaries.
- Forbid silent mode downgrade: ISOLATED to LOCAL requires explicit owner authorization.
- Retain S2 synthetic-fixture evidence and frozen S2 contract semantics unchanged.
- Define explicit, observable entry conditions for S3-TRUSTED_LOCAL without weakening ISOLATED/S3-ISOLATED gates.

## Considered alternatives

- **Strict-everywhere:** require SEC-01-STRICT certification before any real local Paseo execution. Rejected by the product owner for TRUSTED_LOCAL because it would block all owner-authorized local development on the owner's own machine despite accepted risk.
- **Broker-only:** permit real execution solely through a certified mandatory-mediation broker (MCP/SDK path with all native tools denied at the OS boundary). Retained as the required topology for ISOLATED/RESTRICTED modes, but not imposed as a gate on TRUSTED_LOCAL because mandatory mediation was never certified (D3-D2 NOT TESTED) and the owner accepts the residual bypass risk locally.
- **TRUSTED_LOCAL with documented risk acceptance (chosen):** authorize broad local access for trusted local runs under an operational policy ([trusted-local policy](../../security/trusted-local.md)), with transparency wording and host-operation limits, while keeping ISOLATED certification available-only-where-verified.

## Decision

The Factory recognizes three execution modes, defined in [isolation policy](../../security/isolation.md):

- **TRUSTED_LOCAL:** owner-authorized local execution with broad host access. SEC-01-STRICT is not required. Residual risks (canary readability, same-UID credential access, native-tool bypasses) are accepted by the owner for this mode only.
- **ISOLATED:** execution claiming an OS-enforced boundary. Requires evidence-bound certification per actually tested paths. SEC-01-ISOLATED is available-only-where-verified.
- **RESTRICTED:** credential-free synthetic or disposable-fixture execution (current F1-S2 scope). Broker-certification rows (S2-C/S2-D class: mandatory mediation of all native paths) apply to ISOLATED/RESTRICTED only.

What this decision does **not** authorize:

- It does not certify SEC-01-STRICT, SEC-01-ISOLATED, or any sandbox claim for TRUSTED_LOCAL.
- It does not rewrite historical FAIL/NOT TESTED results to PASS.
- It does not authorize S3-ISOLATED, unattended real agents, privileged autonomous effects, or production delivery without their own gates.
- It does not amend frozen S2 contract semantics (see below).

Approver: product owner, 2026-10-19, TRUSTED_LOCAL migration campaign.

## Consequences

Positive:

- Owner-authorized local development (S3-TRUSTED_LOCAL) can proceed under explicit entry conditions without waiting for an uncertified strict-isolation proof.
- Security reporting stays honest: mode labels distinguish accepted risk from certified boundary.

Negative and follow-up work:

- TRUSTED_LOCAL runs operate with broad local access; a compromised or misdirected tool can read local secrets and mutate host state within the user's privileges.
- Office/CLI surfaces must label TRUSTED_LOCAL runs as trusted-local, never as isolated or certified.
- S3-ISOLATED still requires the full SEC-01 resumption criteria (mandatory mediation, per-provider evidence, egress/restart/fencing tests).

Compatibility impact (per [ADR-0006](ADR-0006-early-development-compatibility.md)): no compatibility shims, adapters, or dual code paths are introduced by this decision. Mode handling is a clean policy distinction, not a versioned interface negotiation.

## Verification

- Docs change set: this ADR, [isolation policy](../../security/isolation.md), [MASTER](../MASTER.md) sections 8/15/18, [F0/F1 plan](../../evidence/history/implementation/f0-f1-plan.md), and new [trusted-local policy](../../security/trusted-local.md).
- Acceptance: markdown formatting matches repo style; cross-links resolve; no frozen S2 contract file is rewritten; no historical result is claimed PASS; S3-TRUSTED_LOCAL entry conditions are stated explicitly.
- Evidence: this ADR and linked policy docs on branch `f1-s2-trusted-local`; independent review of the documentation diff.

## Contract relationship (amendment note, non-rewriting)

Checked 2026-10-19: [contract inventory](../../contracts/README.md), [Run–Worker S2](../../contracts/run-worker-s2.md), [Factory Events S1](../../contracts/factory-events-s1.md).

- `run-worker-s2.md` v1 states "SEC-01 remains BLOCKED: only the fixed credential-free synthetic workload below is authorized" and forbids real provider credentials. That statement remains true **for the frozen S2 synthetic fixture contract**. It does not gain a TRUSTED_LOCAL workload kind by this ADR, and this ADR does not edit its semantics.
- TRUSTED_LOCAL execution is authorized by owner policy (this ADR plus the trusted-local operational policy), not by the S2 contract. Any future TRUSTED_LOCAL workload kinds, routes, or credential rules require their own contract revision with explicit versioning and approval.
- `factory-events-s1.md` redaction and durability rules align with the trusted-local secret-handling policy; no contradiction found. `README.md` inventory describes S1/S2 frozen scope only and does not forbid a policy-authorized local mode.

## Accepted risks (TRUSTED_LOCAL only)

1. Host canary readability: local tools can read files outside the task workspace within user privileges.
2. Same-UID credential access: colocated secrets are readable by same-user processes; no OS separation is claimed.
3. Native-tool bypasses: unmediated shells, file edits, code modes, plugins, or subprocesses may bypass any advisory tool policy.
4. Broad host mutation: destructive or persistent out-of-task operations are possible if attempted; controlled only by operational limits and separate authorization, not by a sandbox.
5. Credential-bearing provider CLIs run alongside untrusted repository commands in this mode.

## What remains mandatory for ISOLATED / S3-ISOLATED

- OS-enforced topology separating credential custody from every untrusted tool path, with explicit threat model and process/mount/UID/network evidence.
- Real Paseo SDK agent runs through that topology with useful scoped work while all alternative paths fail closed, repeated per enabled provider/runtime.
- Synthetic canaries at host/project/credential locations verified via transcripts, syscalls, or equivalent OS-enforced denials.
- Egress policy, cancellation, restart, orphan cleanup, lease fencing, and duplicate/replay tests.
- Pinned image digests and binary versions with immutable redacted evidence and independent review before any SEC-01-ISOLATED PASS.
- No silent ISOLATED-to-LOCAL downgrade: any mode change requires explicit owner authorization.

## Supersession

This ADR partially supersedes the absolute S3 gate stated in prior SEC-01 and F0/F1-plan text **for TRUSTED_LOCAL only**, by owner risk acceptance. It does not supersede or weaken any ISOLATED/S3-ISOLATED requirement. Prior FAIL/NOT TESTED findings are preserved as observations.
