# ADR-0047 — Versioned source evidence for bootstrap closure identities

**Status:** Owner-delegated implementation candidate (2026-11-20). The Owner has
explicitly delegated licence, identity and evidence decisions in this domain to
the Lead under the Owner charter and the sole economic/commercial criterion
recorded below. This ADR and its data are a candidate for Reviewer70 review
before any build or runtime activation; they do not create a new reserved-matter
authority outside that delegation.

**Supersedes:** none.

**Amends:** the exact-source-identity rules in ADR-0029 and the environment
licence inventory rules without changing the Quality Gate, trust boundaries or
delivery authority.

## Context

The recovered `patch-2.7.6` output embeds the exact bootstrap `ed-1.21.1`
output at `/nix/store/36qzll…-ed-1.21.1` (deriver `frkd…`). The plan pin's
normal top-level `ed` is a different output (`g4qyr…` / `czkshm…`), so package
set metadata cannot be substituted by name or version. The existing
`SourceLicenseOrigin` mechanism is limited to exact unversioned MIT substrate
identities: Core's validator requires `versionState=Unversioned`, `license=MIT`,
`licenseFile=COPYING` with the exact MIT `COPYING` SHA, non-empty input sources
and a composition NAR equal to the identity NAR. The bootstrap `ed` is a
versioned identity with a GPL-2.0-or-later source and no input sources, so that
format is not reusable.

The exact output contains `share/info/ed.info`, which is the GFDL
1.3-or-later manual. `share/man/man1/ed.1.gz` is not asserted as GFDL: the man
page itself declares GPLv2+ and is recorded as GPL-2.0-or-later. The current
licence catalogue/profile does not classify GFDL, so the `ed.info`/`fdl.texi`
documentation verdict is missing. The software licence (GPL-2.0-or-later) is
established by the `ed.h`/`main.c` headers and is already allowed in the
closure context; `COPYING` is the GPL-2.0 license text and is not by itself
claimed to grant "or later".

The Owner's standing delegation for this domain is: the Lead may decide licence,
identity and evidence questions without per-licence Owner approval, provided the
licence requires no payment (licence fee, royalty or paid activation) for
commercial development/use, permits building and using commercial products, and
the applicable copyleft, attribution and source obligations can be met and are
recorded. Copyleft, attribution and source obligations are not payment and are
not a closed-source/permissive-only requirement. Optional paid support, paid
distribution or dual licensing do not disqualify a licence when a free branch
applies to the intended use. Non-commercial/research-only terms, mandatory
royalties or a payment obligation for the intended commercial use are denied.
Unknown terms require exact evidence and fail closed; the Lead investigates
rather than asking the Owner by routine. This delegation does not extend to
provider payment, merges, sandbox, credentials or the Quality Gate.

## Decision

Add a bounded, versioned source-evidence record for exact bootstrap/closure
identities. It is evidence, not authority:

- A shared data asset `versioned_source_evidence.json` is loaded by both Core
  and the environment builder (Core compiles it with `include_str!`; the builder
  reads the same packaged file). It contains entries keyed by the exact pin,
  output derivation and output store path.
- A `VersionedSourceEvidence` value may be attached to a closure entry as
  `versionedOrigin`, analogous to but separate from `source_origin`.
- The record contains the exact pin, name, version and `versionState: Known`,
  software licence, documentation licence, output derivation/path, output NAR
  hash and size, output cache signature, source fixed-output derivation/path,
  URL, output hash, source NAR hash/size and signature, licence evidence with
  exact purpose/role and SHA-256 (`COPYING` license-text GPL-2.0-only;
  `doc/fdl.texi` documentation-license-text GFDL-1.3-or-later; `ed.h` header
  licensing GPL-2.0-or-later; `main.c` source-version licensing
  GPL-2.0-or-later), output files with purpose and SHA-256 (`ed.info`
  GFDL-1.3-or-later; man page GPL-2.0-or-later; `bin/ed`/`bin/red`
  GPL-2.0-or-later), and evidence hashes for the raw `.drv` ATerm bytes, cache
  narinfo, source flat hash and canonical licence-evidence/output file lists.
- The raw `.drv` ATerm bytes are the byte observation. `nix derivation show`
  JSON is derived provenance and MUST NOT be represented as a byte observation
  (`drvJsonIsByteObservation: false`).
- Core and the builder enforce exact equality of every field. A mismatch,
  missing evidence, unknown identity, non-`Known` version, missing or tampered
  licence file, unknown/denied licence atom, or an identity-facts mismatch fails
  closed.
- The record is anchored to the broker-signed identity-facts receipt. Core
  verifies the HMAC receipt and requires the signed challenge pin, plan/spec
  binding, manifest fact `{storePath, deriver, narHash, outputs}` and the exact
  source fact. The receipt HMAC is verified at runtime and is never stored in
  the shared data file.
- Licence verdicts use the existing `license_catalog.json` and
  `license_profile.json` evaluator. No allowlist, grant, model output, name
  equivalence or generic framework is introduced. The documentation-licence
  verdict is exercised by the delegated Lead decision and recorded in Core; it
  is not a static per-licence Owner approval.

## Delegation and actor fence

The Owner delegation is a durable Core record created once through the
authenticated operator boundary on the project's own stack, bound to the
project, domain, exact pin/identity scope, use context, the economic/commercial
criterion and an expiry/revocation revision. The Lead decision is a separate
Core record bound to the same scope, exact licence expression, source/evidence
hashes, provenance, use context and revision. The Lead actor is a private
Core-verified binding to the exact Lead request and its model-call reservation;
a model, chat message, operator token or external caller cannot construct it or
grant itself a licence. Core validates the delegation, the exact evidence, the
existing catalogue/profile verdict and the economic/commercial criterion, then
persists and applies the decision. Revocation, expiry, restart and Office audit
are part of the record. A log entry or a model dictum is never a Core grant.

## Policy dependency

The exact documentation licence is `GFDL-1.3-or-later`. The catalogue
**classifies** that identifier so evidence can refer to it, but the global
profile does not allow it: `closure`/`build`/`devTest` remain UNKNOWN for GFDL
and the profile hash is unchanged. The allow is the exact Core
`license_decision` for the exact identity, evidence, use context and revision,
within the active Owner delegation; no global allow and no name/version
substitution. Catalogue hash `edef68e1…`, profile hash `233c0a44…`, binding
`157242c0…`. The catalogue entry uses the existing `strong-copyleft` family (no
new family enum) and the obligations `notice` + `source`; it does not reduce the
licence to notice-only. The data is part of the delegated candidate and is
activated only after Reviewer70 review and the normal build/restart; it is not
an inline per-licence Owner approval and not a static allowlist.

## Consequences

- The catalogue/profile content hashes change, so the `LicenseInventory`
  binding and SBOM hash for any newly compiled environment change. Existing
  frozen inventories/plans are immutable; a new environment plan/revision and
  review are required. No historical row is rewritten and no SQL bypass is
  used.
- The environment review must consume the exact `ed` evidence and the new
  documentation verdict; the existing freeze/ActionSpec and Campaign
  authorization remain unchanged.
- The implementation requires Core, builder, shared-asset and test changes. It
  does not grant model or Worker authority and does not change the Quality Gate.

## Acceptance

The implementation is accepted only with deterministic tests for exact match,
pin/derivation/path/NAR/source-hash mismatch, missing licence file, tampered
licence bytes, unknown and denied licence atoms, missing/tampered identity-facts
HMAC, no MIT-origin reuse, builder/Core shared-hash agreement, and the
environment-plan/inventory rebind. Tests must prove fail-closed behaviour, not
only the happy path.
