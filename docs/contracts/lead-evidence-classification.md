# Lead evidence classification (contract, HYBRID)

Status: proposed contract, unactivated. No runtime, no egress, no policy change.

## Decision mechanism: HYBRID

The Lead reasons about a Nix identity and may request evidence through the
existing Librarian role; **Core** decides deterministically. Model output never
grants authority, never selects the source and never accepts an identity.

## Admission by class, not by identity list

Admission is a rule over a class of artifacts and provenance, plus the explicit
prohibitions that already exist (denied identifiers, unfree). There is no
allowlist of identities and no "permit anything except" default. An unverified
identity triggers **investigation**, not authorization and not an Owner
escalation.

## Evidence

- Primary sources must bind the **same instance**: the pinned revision, the exact
  derivation path, or the exact output path.
- Evidence about a different derivation or output is refused as a different
  instance and is never transferable.
- Every source carries a digest; an empty digest is refused.
- Upstream documentation alone is not a primary source: it can support a review
  but cannot establish the instance.
- Evidence is typed (`EvidenceSourceKind`, `EvidenceBundle`) and deterministic;
  the current implementation is offline and unit-tested with clearly labelled
  simulated sources.

## Decision

`classify_identity` accepts only when the identity is complete, the mandate is
active and the Lead actor is in scope **as verified by Core**, the Lead decision
is ACCEPT, and the licence passes the current catalog/profile policy in the
Closure context. Missing evidence, unknown or denied licences, unfree identities,
revoked mandates and out-of-scope actors all fail closed. `NEEDS_EVIDENCE` is a
hold/replan signal, not an escalation.

## Escalation boundary

Only a genuine policy exception or reserved risk decision escalates to the Owner,
with the reason and alternatives. A lack of evidence never escalates.

## Concrete blocker

The Librarian exists as a planning role and prompt
(`native/src/planning/prompts.rs:94`, `native/src/knowledge.rs`), but there is no
authorized evidence-fetch/egress channel in Core for it. Until such a channel
exists and is authenticated, evidence collection stays offline and simulated, and
the classification path is **not activated**. No new service, secret or egress is
introduced here.
