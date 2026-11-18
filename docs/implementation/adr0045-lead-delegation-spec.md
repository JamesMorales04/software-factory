# ADR-0045 — Lead authority inside an Owner-approved perimeter (spec freeze, v2)

- Base: `fc1375028b29483a1299ab891a743806a326f10b` (tree `4d5346d8…`), separate
  worktree, clean.
- Risk: **R2** (authority-adjacent, additive, fail-closed).
- Status: **SPEC v3 / AUTHORITY GATE ONLY**. A tested Core gate exists in this
  candidate; it has no caller and nothing is activated.
- Pausable at any point to act as the single Run9 executor.
- Supersedes the per-identity framing: the Owner approves the **perimeter class
  once**, not a list of helpers case by case.

## Contract (corrected)

The Owner approves **what kind of thing may be excepted**, once. The Lead then
makes the **operational decision for a concrete observed candidate** inside that
perimeter. The Lead is not a ratifier of Owner-approved identities, and a new
eligible helper must not need another Owner act.

### 1. Perimeter: trusted roots per pin, reviewed once

A versioned deployment artifact (protected change, outside the plan and outside
model output) records, for one pinned nixpkgs revision:

- `pin`
- the exact `stdenv` output path, its `.drv` and its `narHash`
- the derived shell wrapper (`nix-shell-env`) path, `.drv` and `narHash`
- the allowed class: regular files whose store name ends in `.sh`
- a revision and canonical `catalogSha256`

No helper identity appears in this artifact. It is a trust-root declaration, so
adding a new stdenv-generated helper does not change it.

### 2. Eligibility is derived, never listed

A candidate is inside the perimeter only if all hold, evaluated from observed
closure facts:

1. `deriver` is absent and the path is a regular file;
2. the observed referrer set contains the trusted `stdenv` output for this pin;
3. **every** referrer is in the trusted root set (`stdenv` and its
   `nix-shell-env` wrapper) — an arbitrary devShell referrer, a label or an SBOM
   assertion does not qualify;
4. the store name matches the generated-helper class (`.sh`);
5. the `narHash` is exact and the plan pin equals the trust-root pin;
6. the number of candidates is bounded by the observed closure size and the
   evidence count.

The trusted `stdenv` is anchored by its own `.drv`, so membership is a positive
derivation fact, not a naming convention.

### 3. Lead decision (operational, advisory, one call)

The existing `lead_environment_review` is reused: the Lead receives the bounded
candidate set with its facts and returns a typed decision set
`{candidateSha256, ACCEPT|REJECT, rationale}` (`structured-model-outputs`, strict
decode, no free strings, no paths/hashes supplied by the model). One review per
plan covers the set; there is **no extra paid call per identity**.

The Lead's role, project and lease are the authenticated actor Core already
derives (`lead:<lead_request_id>`, `core.rs:12991`), plus the durable delegation
domain. No `policy_autonomy`, no sandbox change, no Owner login.

### 4. Core is the authority

Core applies an exception only when every check passes: trust-root revision is
current (not superseded or revoked), plan pin matches, the candidate is eligible
by §2 recomputed from the frozen facts, the Lead delegation is active for this
project/principal/pin, the Lead decision is ACCEPT for that
`candidateSha256`, and the model call is settled. It then writes the identity into
the frozen plan and records `environment.helper_exception_applied` with
trust-root revision, delegation id, Lead decision id, identity and plan sha.

Anything outside the perimeter — arbitrary helper, other pin, wrong referrer,
non-regular file, deriver present, invented or revoked decision, Lead REJECT —
stays blocked with `ENVIRONMENT_LICENSE_UNKNOWN`.

### 5. Revocation

Revoking the trust-root revision, revoking the delegation, or a newer trust-root
revision supersedes prior applications. No per-identity revocation is needed
because identities are not the approved object.

## Tests (fixture, canonical container)

1. A **new** eligible helper (as observed fact) is accepted by the Lead inside
   the perimeter with no Owner action.
2. An arbitrary helper (referrer outside the trusted roots, or a forged
   devShell/SBOM label) is denied.
3. Out-of-scope: other pin, other project/principal, expired or revoked
   delegation are denied.
4. Invented decision (`candidateSha256` mismatch or path supplied by the model)
   is denied.
5. Lead REJECT keeps the block.
6. The 14/12 inventory is the fixture for the eligible set, with the trust roots
   `stdenv`/`nix-shell-env` from the same inventory.

## a44 bridge (separate, not a dependency)

If only an approved identity list exists today, it is a **separate bridge** and
must not become the contract. This design does not depend on that catalogue to
develop or to prove the minimum non-per-item Owner behaviour.

## Trust channel: real gap (corrected by the independent preflight)

The plan/spec hash and the OCI pin do **not** prove that the caller is Core: the
broker receives an unsigned request and the builder reads JSON input. The builder
also only **self-reports** closure facts; it does not capture the `.drv` narHash.
Therefore no membership claim can be trusted for acceptance today, and no
consumer may be activated from that channel.

What this candidate therefore is: a **tested Core authority gate** (trust roots +
membership + typed Lead decision + size bound), with **no caller**. It is not a
feature and nothing is activated.

### Exact protected requirement to unblock acceptance

One of the following must exist, and it is outside this slice:

1. **Verifiable facts**: a builder receipt signed with a Core-held key (verified
   at both ends) carrying `path`, `narHash`, `deriver`, `.drv` narHash and the
   referrer set; **or**
2. **Core-side verification**: Core queries the pinned closure itself (Nix in a
   Core-owned, non-untrusted path).

Either one, plus Core-held trust roots for the pin, makes the gate in this
candidate usable. Guessing new auth or secrets is explicitly out of scope.

### Simpler fallback (recorded, not implemented)

Keep the candidate blocked in the builder and make acceptance exclusively Core
with a typed Lead review and a receipt, distributing the environment only after
Core accepts it. This still needs (1) or (2) to make the facts trustworthy; it
only removes the need to feed an accepted list back into the builder.

## Allowed paths

`deploy/environment-helper-trust/**`, `native/src/planning/helper_exceptions.rs`,
`native/src/planning/{mod,environment}.rs`, `native/src/bin/core.rs`,
`native/src/lead_model.rs` (typed decision), `deploy/environment-builder/**`
(frozen-list consumption + tests), `docs/**`, parity JSON.

## Broker receipt (implemented primitives, not yet wired)

`planning::helper_facts` implements the authenticated envelope and both ends:

- derivation context `factory.environment.helper-facts.v1`; the MAC key is
  HMAC(bearer, context), so the shared environment bearer is never used raw;
- envelope bound to `operationId`, `nonce`, `pin`, `planSha256`, `runtimeId`,
  the observed `stdenv`/`shellWrapper` (each anchored by `.drv` + `narHash`) and
  the observations;
- `sign` (broker) and `verify` (Core) with constant-time HMAC verification;
- durable replay identity `helper-facts:<nonce>` for
  `environment_build_receipts.id` (primary key), so a replayed receipt fails the
  insert instead of relying on an arbitrary freshness window;
- negative tests: tampered observations, another operation/pin/plan/runtime,
  wrong context, another bearer, forged/malformed signature, replayed nonce.

### Service placement (no new topology)

The observer is **broker-owned**: it reuses the broker's existing execution of
the pinned builder image/rootfs with the store mounted read-only, no workspace,
no network and a fixed argv, and only reads `nix path-info` bytes. It adds no
process, container, privilege, host mount or credential; the builder never holds
the bearer and never signs. The builder's self-report is not signed and is not
used for membership.

Still not wired: the broker call site that builds and signs the envelope and the
Core call site that verifies it before `helper_exceptions::authorize`. Nothing
accepts an exception plan from untrusted JSON.
