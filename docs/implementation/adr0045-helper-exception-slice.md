# ADR-0045 slice — observed deriver-less facts, fail closed

Status: **PHASE 1 CANDIDATE / NOT_ACTIVATED**. This candidate contains **no
acceptance path and no consumer** for a helper exception. It only makes the
licence failure exhaustive, provenance-bearing and fail-closed.

- Base: `fc1375028b29483a1299ab891a743806a326f10b` (tree `4d5346d8…`).
- Risk class: **R2** (licence-gate observations only; no authority change).
- Action: `F1-S11-NIX-HELPER-EXCEPTION-DELEGATION`.
- ADR: `docs/architecture/decisions/ADR-0045-narrow-nix-helper-exception-delegation.md`.

## What this candidate does

1. **Closure facts carry provenance.** `closure_manifest` now retains the
   closure `references`, so the referrer set of a path is a real fact derived
   from `nix path-info --json --recursive`, not a guess.
2. **Exhaustive diagnosis, still fail-closed.** Every deriver-less path that is
   not a legacy reviewed helper is reported with `storePath`, `narHash`,
   `narSize`, `referrerSetSha256` and `regularFile`.
   - The complete set is written to `deriver-less-helpers.json` beside the build
     log, with `total` and a canonical `sha256`, so no identity is lost.
   - The blocker message carries only **whole** entries that fit the existing
     1 000-character contract and always states `total`, `sha256`, the capture
     file and `truncated=true|false`. Truncation is explicit, never silent.
   - No cap on the number of candidates; the message bound comes from the real
     contract, not from an invented constant.
3. **No legalisation by plan contents.** A plan that supplies
   `licenseInventory.helperExceptions`, a `planSha256`, an SBOM hash or any other
   crafted exact-looking field changes nothing: the builder never reads an
   exception and the run still fails. A negative test proves it.
4. **No authority change.** `ENVIRONMENT_LICENSE_UNKNOWN` is reused; there is no
   new blocker code, no plan-schema field, no table, endpoint or migration.

## Authority gap (why activation is absent)

`agentic_owner_decisions` is created and resolved behind
`authorized(&headers, &app.operator_token)` (`native/src/bin/core.rs:18323`,
`:18360`). The resolution principal comes from `operator_principal(...)`
(`core.rs:18363`), which yields `operator-token` or `device:<id>`; the database
accepts any non-empty actor and stores it verbatim (`agentic_charter.rs:454-492`).
`resolved_by` therefore derives from `operator_principal`, and that function does
**not** attest the Owner: there is no cryptographic proof of an Owner
resolution, so the authority needed to activate a rule is **absent**. Designing
that proof is a separate protected decision and out of this slice's scope.

## Acceptance

- Builder suite (canonical dev container): exhaustive-facts test with all 14
  deriver-less identities, explicit truncation indicator, capture file with
  `total`/`sha256`, crafted-plan rejection, and the existing 12/14 split covered
  by the message and capture assertions.
- No Rust change, so no Cargo evidence is claimed.
- No runtime image build before the candidate SHA is frozen.
