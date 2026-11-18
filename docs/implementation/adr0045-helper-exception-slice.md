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
   - Evidence status: the broker capture is **code-verified, NOT_CAPTURED at
     runtime** in this slice — the builder side writes the file, but no broker run
     was executed to observe the read. Nothing here claims runtime capture.
   - The complete set is written to the **existing broker capture file**
     `closure-license-inventory.json` in the run directory, with `total`,
     canonical `sha256`, `captureComplete`/`captureReason` under the same 60 KiB
     accounting used by the licence inventory. The broker reads that file from
     the cache volume before cleanup (`read_environment_output` with
     `"closure-license-inventory.json"`, `runtime_broker.rs:5939`)
     and retains the bounded JSON in its log line; the cache volume itself is
     disposable, so this is **not** a durable file claim.
   - The blocker message carries only **whole** entries that fit the existing
     message contract (`errors.py:47` truncates at 1 000 characters; the helper
     budget is 900) and always states `total`, `sha256`, the capture file,
     `captureComplete` and `truncated=true|false`. Truncation is explicit, never
     silent.
   - No cap on the number of candidates; the message bound comes from the real
     contract, not from an invented constant.
3. **No legalisation by plan contents.** A plan that supplies
   `licenseInventory.helperExceptions`, a `planSha256`, an SBOM hash or any other
   crafted exact-looking field changes nothing: the builder never reads an
   exception and the run still fails. A negative test proves it.
4. **No authority change.** `ENVIRONMENT_LICENSE_UNKNOWN` is reused; there is no
   new blocker code, no plan-schema field, no table, endpoint or migration.

## Authority gap (why activation is absent)

`agentic_owner_decisions` is created by `create_agentic_owner_decision`
(`native/src/bin/core.rs:18323`) and resolved by `resolve_agentic_owner_decision`
(`core.rs:18360`); both gate on `authorized(&headers, &app.operator_token)`. The
resolution principal comes from `operator_principal`
(`core.rs:18094`), which returns exactly `operator` or `device:<id>`; the database
entrypoint `resolve_agentic_owner_decision`
(`native/src/db/agentic_charter.rs:447`) accepts any non-empty actor and stores it
verbatim. `resolved_by` therefore derives
from `operator_principal`, and that function does **not** attest the Owner:
there is no cryptographic proof of an Owner resolution, so the authority needed
to activate a rule is **absent**. Designing that proof is a separate protected
decision and out of this slice's scope.

## Acceptance

- Builder suite (canonical dev container): exhaustive-facts test built from the
  **real 14 identities** of the Run8 inventory (`6a355fcc…97c50`), of which the
  two legacy helpers are accepted and exactly **12** are reported; explicit
  truncation indicator, capture payload with `total`/`sha256`/`captureComplete`,
  coverage equality on the 12 paths, and crafted-plan rejection.
- No Rust change, so no Cargo evidence is claimed.
- No runtime image build before the candidate SHA is frozen.
