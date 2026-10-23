# Native Rust Lead domain and quality-evidence checkpoint

**Date:** 2026-10-23 UTC. **Candidate:** `b964cc6beff62bc37db4c6b850f7e8acbbe6606a`.
**Predecessor:** `05828db394e842d05e93eedc09ef158493b24ef4` (typed Lead domain),
`706399d422415033ffb586483e067821f3a88479` (candidate/quality ingestion).
**Branch:** `native-rust-replacement-20261023`. **PR:** #17, draft.

All results below are author-run on the exact candidate tree. They are not
independent acceptance and do not certify provider compatibility, isolation,
dispatch, live model quality or the completed replacement.

## Implemented scope

### Typed Lead domain (`native/src/lead.rs`)

- Strict advisory `LeadAssessment` decoding with `deny_unknown_fields`, typed
  effort states, identity binding, unique bounded evidence references, bounded
  notes and a coarse deterministic estimate-request trigger.
- Immutable `StandingDelegation` validation: explicit project/principal/audit
  identities, revision and content hash, objective classes, paths,
  capabilities, risk classes, exact route, per-Campaign and cumulative limits,
  expiry, one-way revocation and R0/R1-only autonomous risk subsets.
- `LeadPlanProposal` validation: base/config binding, objective class, quality
  profile, closed check kinds, risk/route/budget containment, 1–8 WorkUnits,
  DAG/cycle/self/unknown/duplicate rejection, safe/covered paths, capability
  containment, parallel path-overlap rejection and session/attempt ceilings.
- Deterministic proposal hash and plan hash binding proposal, delegation and
  Project configuration revision.
- Explicit `LeadRequestState` transition table.
- No database, API, model call or execution authority is added by this module.

### Candidate and quality ingestion

- Core independently verifies a Worker candidate claim against the registered
  Git repository: full commit/tree identity, claimed tree equality, non-empty
  change, and a single parent equal to the frozen base.
- Candidate rows are immutable, bound one-to-one to an Attempt, and require the
  current unexpired lease generation plus an `AUTHORIZED` Campaign. No public
  API can set either, so dispatch remains disabled.
- Separate verifier and reviewer bearer tokens are required and must be
  distinct from operator and Worker bootstrap tokens.
- Quality receipt requests authenticate before parsing JSON.
- Receipts are immutable, exact-candidate/`ActionSpec` bound, unique per
  independent role, and rejected when the principal is the implementer or when
  verifier and reviewer share a principal.
- The deterministic gate is persisted once both receipts exist. R0 maps to
  within-policy; R1 asks for human approval; R2/R3 exceed policy; unassessed
  risk is unknown. Mocked evidence blocks; unknown evidence is `NOT_TESTED`.
- Operator-read routes expose the candidate, receipts and gate. No UI journey
  exists yet.

## Verification run at the candidate

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked --bins
cargo doc --workspace --no-deps --locked
```

Results: formatter PASS; strict Clippy PASS; test suite PASS with **83 library,
4 architecture-check, 3 Core binary, 7 hygiene and 1 Worker tests** (98 passed,
1 ignored Podman fixture); binaries PASS; docs PASS. The ignored rootless
Podman Alpine fixture was not rerun in this slice; its last local author run
passed in 12.55 s on candidate `f79f04f` and does not certify isolation.

New negative coverage includes strict assessment JSON (unknown/duplicate/
trailing), non-change executable units, expired/revoked/tampered delegations,
unsafe paths, unsupported routes, invalid graph shapes, parallel overlap,
capability expansion, missing required check kinds, session escalation, forged
candidate generation, wrong spec/tree binding, implementer self-review,
verifier/reviewer collusion, conflicting receipt replay, human-required and
unknown risk, candidate/gate immutability, and missing/cross-role credentials
on the HTTP route.

## Limits and non-claims

- **No provider session or paid turn** was started. No Codex/OpenCode coding
  compatibility is claimed.
- **No public dispatch**: Campaign authorization and Attempt `sandbox_ready`
  remain test-only setup. The candidate/quality tests use direct SQL in the
  `#[cfg(test)]` module solely to reach the guarded paths.
- **No independent verifier/reviewer process**: receipts are authenticated
  ingestion only; separate clean-checkout executables, oracle execution and
  browser evidence are absent.
- **No isolation certification**: the same-UID workspace pathname race remains
  unresolved; TRUSTED_LOCAL is not isolation.
- **No S4-D activation**: ADR-0017 remains Proposed; no grant was issued and
  autonomous start is not implemented.
- **No Office journey**: Office still targets the legacy API.
- **No exact-SHA CI result** is recorded yet; PR #17 remains draft.
- Independent review of this candidate is pending. CI and review must be
  recorded before any stronger claim.
