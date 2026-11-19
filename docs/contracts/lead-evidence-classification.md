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

## Owner criterion: commercial use with no payment obligation

The Owner's standing criterion is explicit: a licence may be used without asking
again when it **requires no payment** and **permits developing a commercial
product**. This is an interpretation of the current policy, not a new authority
and not a catalogue change.

The Lead may therefore authorize an identity without an Owner decision only when
all of the following hold:

1. **Commercial permission is evidenced from the licence itself** — the SPDX
   identifier plus the licence text obtained from a primary source with a digest.
   It is never inferred from the Nix `free` flag, from a missing price field, from
   the absence of a "non-commercial" string, or from a `meta` attribute.
2. **No payment obligation applies to the intended use** (development of the
   product), as stated by that licence text and recorded in the evidence scope.
3. **Applicable obligations are identified and can be met** (notice, attribution,
   source availability, relinking, patent terms, and similar), and their
   fulfilment is recorded with the decision.
4. **The scope is recorded**: the exact identity, the intended use, the licence
   identifier and the evidence digests.
5. **The decision is re-evaluated** when the form of use or distribution changes
   (for example shipping binaries, offering a hosted service, or redistributing a
   modified work), because the commercial/no-payment conclusion is scoped to the
   recorded use.

### Mapping to the existing policy

- The licence identifier must evaluate to `Allowed` in the current catalogue and
  profile for the closure context; the Owner criterion never bypasses that
  evaluation.
- `DENIED` remains a refusal: a denied identifier is not rescued by a claim of
  commercial permission.
- `UNKNOWN` triggers **investigation** (Librarian evidence), never acceptance and
  never an Owner escalation by itself. It becomes an escalation only if resolving
  it requires a policy exception or accepting a reserved risk.
- The existing `free`/`unfree` flags stay as they are; `unfree` is still refused.
  No profile, catalogue or SPDX list is modified by this contract.

### Evidence shape

`EvidenceBundle` for this criterion uses a primary `Output` or `Derivation` source
for the instance plus one or more `UpstreamDocumentation` sources whose
`reference` is the licence text location and whose `digest` covers that text, plus
the SPDX identifier carried in the observation. A decision without a primary
instance source or without a digested licence text is `MissingPrimarySource` /
`NeedsEvidence`, and the row stays blocked.

## Status

Documentation only: no code, catalogue, profile or activation change. The
Librarian's automated web retrieval is still not connected, so evidence remains
offline and simulated until an authenticated channel exists.

## Non-authorizing dictum, and what is still missing

`classify_with_evidence` no longer emits an approval. It returns a
`ClassificationDictum` with `authorized: false` that records the licence, the
digested licence text, the intended use, the obligation review, the mandate
revision and the canonical bundle digest, all bound to the same candidate. There
is no constructor, flag or caller-supplied handle that turns it into an approval:
until Core can verify the Lead actor and receive authenticated evidence, the
dictum is advisory only.

### Evidence channel: what does not qualify

The existing knowledge channel (`POST` handler at `native/src/bin/core.rs:17810`)
requires the operator token and triggers `detect_knowledge_gap` from the
objective's library/skill needs. It is **not** pinned to a derivation, is **not**
licence evidence, and must not be reused by pretending a gap or an egress exists.

### Next realizable stretch and the primary gap

- Already available: the deterministic classifier, the class mandate shape, the
  typed evidence validation, and the policy evaluation (all offline and tested).
- Primary gap for `stdenv-linux` and `update-autotools-gnu-config-scripts-hook`:
  a digested, primary licence-text source for those exact derivations. Neither has
  published licence metadata, and no current channel can obtain a primary source.
- Next step: wire a Core-verified actor handle plus an authenticated evidence
  channel (no new egress, no new secret invented here). Until then every result
  stays a non-authorizing dictum and the closure remains fail-closed.
