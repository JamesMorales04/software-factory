# Environment identity facts — authenticated broker channel

**Status:** implemented for the environment freeze path (migration `0081`).
**Mechanism:** `DETERMINISTIC` capture, binding, HMAC and replay control.
Semantic licence classification is a later `HYBRID` slice and is not part of
this contract.

This document records the frozen v6 contract that lived outside the repository
(`IDENTITY-FACTS-CONTRACT.md`, consolidated hash prefix `485e4b43`). The
external document is provenance; this file is the normative in-repo text.
Earlier appendices contradicted each other and are not resurrected.

## Purpose and limits

The broker proves **which artifacts it observed** in the pinned runtime for an
exact Core-bound environment operation. It does not prove licence text,
commercial permission, obligation compliance or an accepted build. Identity
facts are advisory input to the existing licence gate and never grant
authority. A licence rejection keeps the identity receipt and leaves the
environment blocked.

## Challenge (Core-issued)

`IdentityFactsChallenge { operationId, runtimeId, nonce, pin, planSha256, mode,
root: OperationRoot, questions }` with
`OperationRoot { Operation, Plan, Runtime }`.

Core never hashes a root before realisation. It issues an **operation selector**
and the broker returns the real root it resolved together with the facts. In
this cut Core always issues `OPERATION`, the resolved environment of the exact
operation. Other selectors are typed but not used by the producer.

`runtimeId` is the digest-pinned environment-builder image digest. Core and the
broker both know it; the broker refuses a challenge bound to another runtime.

`mode`: `ENUMERATE` discovers the closure of the broker's realised root and
requires `questions` to be empty; `QUESTIONS` requires 1..=64 complete
Core-owned identities. Empty `questions` therefore means discovery, never an
empty manifest.

## Manifest (complete, never paginated)

`IdentityFactsManifest { observedRoot, facts, factCount, closureManifestSha256,
truncated }` and each `IdentityFact { storePath, deriver, narHash, outputs }`.

- The manifest is complete by contract: `truncated` is false, `factCount`
  equals the number of facts, store paths are unique, and
  `closureManifestSha256` is the canonical digest over the ordered facts.
- The technical bound is 4096 facts, comfortably above the 73-path Run10
  closure. Exceeding it, truncating, duplicating or emitting an incomplete fact
  is a refusal with no receipt, never a partial success.
- A deriver-less path is represented explicitly as `deriver: null`. Its own
  store path is its only output; no `.drv` is invented and no fact is omitted.
- Derivation-carrying paths obtain `outputs` only from
  `nix derivation show`; the broker never copies a builder- or model-supplied
  manifest or asserted hash.
- Facts come from the broker's own fixed queries over its persistent store
  volume: `nix path-info --json --recursive --offline <root>` followed by
  `nix derivation show --offline <derivers>`. The root is resolved from the
  realised profile inside the exact spec-keyed cache volume.

## Envelope and authentication

`IdentityFactsEnvelope { schemaVersion, context, challenge, manifest }` is
signed by the broker with an HMAC-SHA256 key derived from the existing scoped
environment bearer and the context
`factory.environment.identity-facts.v1`. The receipt is
`IdentityFactsReceipt { envelope, signature }`; the signature is outside the
signed bytes. Core re-verifies the signature, context, exact challenge echo,
manifest rules and observed-root binding.

The builder image never sees the key and never signs anything. A manifest
supplied by the builder, a model or an operator is rejected by construction
because only the broker-owned fixed query produces the envelope.

## Ordering, persistence and atomicity

1. Issue the challenge and persist it as `PENDING` **before** external
   observation dispatch.
2. Realise the environment, then observe the broker-owned realised store and
   sign the manifest. The pinned builder image realises the environment and
   evaluates its internal licence metadata in one container; the broker reads
   only the store that container actually wrote, immediately after it exits and
   before operation cleanup. The receipt therefore describes the realised
   store, never a builder self-report.
3. Core verifies the receipt, then consumes the challenge and inserts the
   immutable receipt **in one transaction**:
   `UPDATE ... WHERE nonce = ? AND state = 'PENDING'` and the receipt insert
   commit together. A replayed, concurrent or foreign nonce matches no row,
   changes nothing and deletes nothing.
4. Core persists the receipt **before** applying the builder's licence outcome.
   If the existing licence gate refuses, the identity receipt is preserved and
   the environment remains blocked; it is never converted into an acceptance.
5. A missing or late envelope is a typed `EvidenceAbsent`: no receipt, an
   explicit blocker and an unchanged build status.

The tables are separate from `environment_build_receipts`, whose
READY/SUCCEEDED build semantics and unique key are different:
`identity_facts_challenges` and `environment_identity_facts_receipts`
(migration `0081`).

## Placement

- Pure challenge/manifest/envelope rules and the MAC live in
  `native/src/planning/identity_facts.rs`.
- The broker producer and its fixed Nix queries live in
  `native/src/bin/runtime_broker.rs`.
- Signature verification and challenge echo live in the builder client path
  (`RuntimeBrokerEnvironmentBuilder`), which holds the bearer and no database.
- Durable state transitions live in `native/src/db/environment.rs`; the
  planning domain never imports a database handle.

## Coverage

The freeze path (`build_environment_before_freeze`) is the common producer and
consumer for `PROJECT_COMMITTED` and `GENERATED` plans. This is the exact path
Run10 took with a committed lock and `packages = []`; a hook only in
`resolve_generated_environment` would not cover it. The committed plan keeps
its source, plan and lock; identity observation never converts it into a
generated plan or accepts an environment in order to obtain evidence.

## Office impact

`impact: none` for this internal observation preparation. There is no new
operator flow, control or authority. Failure is surfaced by the existing
truthful environment projection as the typed blockers
`ENVIRONMENT_IDENTITY_EVIDENCE_ABSENT` and
`ENVIRONMENT_IDENTITY_FACTS_REFUSED`; no decorative UI or manual raw-API
workflow is introduced. The parity matrix does not gain a capability row until
an operator-visible capability exists.

## Acceptance matrix

| Case | Expected |
|---|---|
| Enumeration with ≥1 complete fact and valid signature | accepted, one receipt |
| Empty / truncated / overflowing manifest | refused, no receipt |
| Duplicate path, incomplete fact, wrong count or digest | refused |
| Questions mode with extra, missing, reordered or duplicate question | refused |
| Wrong pin/plan/runtime/operation or wrong bearer/context | refused |
| Builder- or model-supplied manifest | refused by construction (broker re-queries) |
| Replayed or concurrent nonce | refused by the pending-state transaction |
| Expired, revoked or foreign challenge | refused without deleting state |
| Envelope absent after cleanup | `EvidenceAbsent`, build status unchanged |
| Licence gate rejects after the receipt | environment blocked, receipt kept |
| Any accepted facts | advisory only; no licence or policy effect |
