# ADR-0045 — Narrow Nix helper exception delegation

- Status: proposed
- Date: 2026-11-18
- Related: ADR-0006 (early-development compatibility), ADR-0038 (Owner charter and
  delegated autonomy), ADR-0041 (agnostic environment compiler)

## Context

The environment compiler refuses any deriver-less closure path that is not a
reviewed helper. Generated stdenv helpers (`strip.sh`, `move-docs.sh`,
`prune-libtool-files.sh`, …) have no derivation of their own but are produced by
`stdenv`, which does. Approving them one path at a time in code is whack-a-mole:
Run6 blocked on `move-docs.sh`, Run8 on `prune-libtool-files.sh`, and the same
closure contains twelve more. The Owner has asked to delegate this narrow class
to the Lead inside a perimeter, while keeping every denial and every widening
outside it with the Owner.

## Decision

1. An **Owner rule** names one pinned nixpkgs revision and an exact, closed set of
   helper identities `(storePath, narHash)` with a canonical digest. The rule is
   recorded as an `agentic_owner_decisions` row (matter keyed by pin, typed
   evidence payload, `APPROVE_RULE:<digest>` option). Revocation is a later
   approved decision for the same digest.
2. **Core may apply a rule only for a cryptographically verified Owner
   resolution, and no such resolution exists today.** Decisions are resolved
   behind the operator token and their `resolved_by` derives from
   `operator_principal` (`core.rs:18094`), which returns `operator` or
   `device:<id>` and does not attest the Owner. The authority needed
   for activation is absent, so activation is blocked; no new authentication is
   designed here.
3. **The builder stays untrusted, observational and fail-closed.** It reports
   closure facts (`path`, `narHash`, `narSize`, `deriver`, `referrerSetSha256`,
   `regularFile`) exhaustively through the existing pre-cleanup capture channel,
   with explicit truncation when the message bound is reached, and it applies no
   exception at all. A plan cannot legalise a path by carrying
   exception-looking fields.
4. **The Lead may recommend** inside the approved candidate set through the
   existing bounded environment-review record. Its dictum is evidence, never
   authority, and no additional paid call is required for exact-identity facts.
5. Everything outside the perimeter — other paths, patterns, suffixes, packages,
   unfree, a different pin, a different narHash, a revoked rule — remains
   fail-closed with the Owner.

## Consequences

- The licence gate remains fail-closed by default: with no approved rule the
  behaviour is unchanged.
- A pin change invalidates every rule; a new Owner decision is required.
- No SBOM or label ever authorizes; only exact identity with an Owner rule.
- The model cannot grant itself an exception, a lease or a Quality Gate pass.
