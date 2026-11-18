# Real generation-2 recovery scenario (P1–P3 live, P4 deterministic)

**Status:** implementation prepared; no live stack, build or paid inference has
been started. The Owner authorized real-model runs. The current production
policy funds the admitted `opencode-go` route through a request-scoped
Owner-confirmed **subscription admission**, not a paid monetary envelope, so a
run through that path is labeled subscription/no-charge; it is never described
as paid. This document is the runbook for the disposable real stack, the narrow
credential provisioning and the evidence contract.

## 1. Scope

| Dimension | Requirement |
| --- | --- |
| P1 | Generation 1 persists and verifies the first functional result with a real provider/model session (subscription-funded on the production path). |
| P2 | The second functional result is still pending at a real pre-intervention barrier and is completed after the Worker restart. |
| P3 | The generation-2 exact check receipt, post-claim candidate, Gate, Campaign and restart are observed. The ActionSpec has no restart field: the restart dimension is declared by this scenario/harness and is verified by restarting the disposable stack and comparing the exact Core state. |
| P4 | The frozen deterministic `ProgressSupervisor` regression (`replayed_candidate_after_generation_recovery_reaches_diagnose`) is the negative control. The live P4 dimension is `NOT_TESTED`; no paid end-to-end replay is required. |

A claim alone is never OK. A run where generation 1 already completed both
results is `NOT_TESTED` (invalid scenario), never PASS.

## 2. Real route

- Provider `opencode-go`, adapter `opencode`, model `deepseek-v4.1-flash`, mode
  `build`, base `https://opencode.ai/inference/go/openai/v1`.
- The route/credential already exist in the installed product's Core data and
  config volumes; the scenario never invents a route and never silently falls
  back to the fixture.
- The Lead/intake request-scoped approval is the only provider authorization
  path: a direct `POST /campaigns/{id}/authorize` with
  `TRUSTED_LOCAL_PROVIDER` is refused by Core, and the scenario records that
  negative response (no paid side effect) as evidence.
- Funding is read from Core, never assumed: a `policy.monetaryEnvelopeId` of
  `urn:factory:no-billable-inference` is the subscription-funded production
  path and a positive amount must not be claimed; any other policy envelope must
  match the intake `spending.envelope` (`envelopeId`/`authorizedMicros`) exactly.
  On the subscription path `owner_reference` is the Owner assumption identity
  recorded by Core (`confirmedBy#assumptionId`), never the sentinel envelope;
  on the monetary path it is the intake envelope id. The caller flag is only a
  `caller_reference`. Missing or contradictory values are `NOT_TESTED`.
- The scenario never fabricates a subscription admission, standing grant or
  environment plan. It records the request-scoped Owner-confirmed admission
  through the existing intake API (once the Owner's current metadata exists) and
  assumes the pre-run binding (project, Git fixture and the Owner-granted
  execution policy) already exists. If Core refuses with
  `SUBSCRIPTION_ADMISSION_REQUIRED`, `ENVIRONMENT_PLAN_REQUIRED`,
  `PLAN_HASH_MISMATCH`, `PROFILE_HASH_MISMATCH` or
  `MONETARY_AUTHORIZATION_NOT_APPLICABLE`, the run stops before any provider
  turn as `NOT_TESTED` with that code. No
  test-support admission or legacy-billable seam is enabled to bypass it.

## 3. Narrow credential provisioning

Script: `scripts/e2e/real-recovery-provision.sh` (`--check` first, `--apply`
after review). It is development/UAT tooling, not the product install path.

- Destination: a **new** disposable project (`*recovery*`/`*uat*`) with its own
  `factory-data` and `factory-config-core` volumes. No product DB or config is
  cloned; no product operator/reviewer/master identity is carried.
- Source access is read-only and in-container: the source data/config volumes are
  mounted `:ro` into a short-lived tool container; there is **no whole-volume
  export**, no host copy of the database or config and no source tar. `umask
  077`, `set +x`. The source project must have no running container: the
  snapshot is opened with SQLite `immutable=1`, so a live writer would make the
  read inconsistent; the script refuses instead of guessing.
- Copy allowlist: exactly one `provider_settings` row for `opencode-go` with the
  schema's exact column list (ciphertext, nonce, fingerprint; no plaintext, no
  other provider, no `provider_settings_audit`, no other table). The row is
  read with a read-only SQLite connection and inserted into the fresh database
  through the SQLite API; the destination is checkpointed with
  `PRAGMA wal_checkpoint(TRUNCATE)` instead of deleting WAL/SHM files.
- `master.key` is transferred volume-to-volume, 0600, 64 lowercase hex, never
  printed or logged.
- Destination operator/reviewer/verifier tokens and remaining keys are generated
  by the canonical `factory-compose-init` service; none are copied. The
  pre-installed `master.key` lets Core decrypt the provider credential.
- Preflight: exactly one configured/default `opencode-go` row, ciphertext and
  fingerprint present, no plaintext credential column, `PRAGMA integrity_check`
  ok, `FACTORY_LEAD_FIXTURE_ASSESSOR=0`, no test-support grants or seeds.
- Source volumes are never modified. The disposable copy is removed with
  `down -v` plus volume removal; only copies are destroyed.
- `.containerignore` excludes `*.db`, `master.key`, `*.token` and `evidence/` so
  no database, key or token enters a build context.

## 4. Scenario flow

Driver: `scripts/e2e/real_recovery.py` (`--plan`, `--dry-run`, `--execute`).
Required run inputs include `--project-id`, `--profile-id`, `--core-api`,
`--operator-token-file`, `--core-database`, `--compose-project`, `--git-dir`
and `--authorization`; the assumption file is required only immediately before
the admission.

1. Consume the pre-run binding: project, Git fixture, V2 execution profile and
   its execution-policy grant are created once by the operator lane through the
   existing Core API. The scenario requires `--profile-id`, reads
   `GET /execution-profiles/{id}`, requires `executable=true` and the canonical
   BALANCED roles/operations/progress/disabled-auto-start from the shared
   `real_recovery_profile.profile_mismatches` helper, records
   `revisionId`/`contentHash`, and never creates or marks a profile default.
   Then it posts the Lead intake (`REVIEW_PLAN`; an explicit positive
   `authorized_micros` only for the test-support billable-envelope path). It
   never creates the grant.
2. Record the request-scoped subscription admission **before any model call**.
   The metadata is read from a 0600 non-secret file supplied from the Owner's
   current answer, using only the existing `OwnerBillingAssumption` fields:
   `assumptionId`, `confirmedBy`, integer `confirmedAtMs`/`validUntilMs`,
   `extraUsage` (`DISABLED`) and a bounded `statement`. There is no
   `authorizationClaim` field in Core and none is invented. The generic paid-run
   permission and elapsed time are never used as confirmation; without the file the driver refuses to create an
   admission and makes no provider call. Core remains the validator of the
   window and of the exact V2 profile/canonical route. The admission id is
   request-scoped and is never reused.
3. Run `POST /intake/requests/{id}/assess`, require `AWAITING_APPROVAL` and the
   frozen `plan_hash`/profile identity.
4. Approve with the exact plan hash, profile revision/hash and
   `subscription_admission_id` — no `authorized_micros` on the production path.
   Before that real approval, probe the same request with a positive
   `authorized_micros`; Core must refuse it with
   `409 MONETARY_AUTHORIZATION_NOT_APPLICABLE`. That negative response is
   evidence and cannot consume the admission.
5. Record the Core authorization record
   (`/campaigns/{id}/authorization`: camelCase V2
   `schemaVersion`/`authorizationId`/`campaignId`/`riskClass`/`executionMode`/
   `delegationId`/`authorizedPaths`/route/`policy.monetaryEnvelopeId`) and the
   Core-projected funding reference. On the subscription path `owner_reference`
   is the recorded `assumption.confirmedBy#assumptionId`, not the sentinel
   envelope. The admission binding (`admission_id`, `campaign_id`, state,
   provider/model, profile revision/hash, `extraUsage`, `receipt_sha256`,
   route transport) is read from the durable admission row and checked against
   the approved Campaign; a mismatch is `NOT_TESTED`. There is no snake_case
   alias: the real serialized schema is enforced. `funding_mode` is
   `REAL_SUBSCRIPTION` or the test-support monetary path; it is never reported
   as `PAID_METERED`. Read the approved ActionSpec from the
   authenticated `/actions` projection (the intake projection does not carry
   it) and verify the exact checks, frozen `base_commit` and `allowed_paths`.
6. Wait for the generation-1 `status` PASS receipt and no `version` PASS
   receipt. Build the pre-intervention snapshot: attempt status, generation-1
   sessions, tool events, last turn event, `active_turns=0`, lease state, every
   tool call's terminal decision (`RESULT`/`ERROR`/`DENY`), `gen1_quiescent`,
   backend-confirmed quiescence, P2 check/artifact absence, `candidate_count=0`,
   `observed_at`. A live lease is recorded as a fact but is not by itself
   quiescence: the turn balance and terminal tool decisions must also be
   settled. Any missing fact is `NOT_TESTED`.
7. Revalidate the whole barrier immediately before the kill. Events after the
   kill are classified: generation-1 tool events, provider turns, receipts,
   candidates and progress before the claim are dangerous; the gen2 claim, run
   lifecycle, session terminal and cleanup are legitimate; unknown is ambiguous.
   Dangerous or ambiguous ⇒ `NOT_TESTED`, never PASS.
8. Kill the Worker, await the generation-1 lease `EXPIRED` from the participant
   projection (the provider-session row does not expose `lease_state`), start it
   again, and observe `worker.claimed generation=2` for the same attempt with
   `observed_at < kill_started_at < claim.recorded_at`. A lease that does not
   expire, or a claim before the kill, is `NOT_TESTED`.
9. Verify the generation-1 `status` and generation-2 `version` receipts with
   their exact command hashes, work item, session, generation and sequence; the
   single post-claim candidate with SHA/tree and the frozen ActionSpec
   `base_commit`; the Gate outcome and `candidate_id`; the Campaign id, action
   and a satisfied status; then restart `core`/`supervisor`/`worker`, wait for
   health and prove the exact candidate/Gate/Campaign/attempt state survived.
10. Result: `ACCEPTED` when P1–P3, Gate/Campaign and the observed restart
    persistence are complete; `OK_SCOPED` when P1–P3 are proven but that
    envelope is incomplete; otherwise `NOT_TESTED`/`FAIL` with the causal reason.

## 5. Evidence and truthfulness

- Capture before cleanup: authorization record, approved ActionSpec, pre-
  intervention snapshot, events, receipts (exact columns), provider sessions,
  candidate, Gate, Campaign, the restart before/after fingerprint, the
  Owner-assumption metadata hash, the subscription admission and both negative
  responses (direct authorization and production monetary approval).
- No secrets, ciphertext, plaintext credentials, raw prompts or provider bodies.
- `api_key_route_disabled` is recorded as `NOT_TESTED_NO_VISIBLE_SURFACE`
  because no current projection exposes it; no flag or field is invented.
  External no-extra-metered use is `NOT_TESTED_OWNER_CONFIRMATION_PENDING`
  until the Owner confirms the current state. The subscription admission is an
  Owner-confirmed configuration assumption, not provider-issued attestation.
  R2 pending blocks only the live admission/model call, not the deterministic
  tests, review or integration of this harness.
- The independent package (`real-e2e-authorized/independent`) belongs to the
  independent lane; this author lane does not write it. The author's test suite
  is author evidence. The Verifier/Reviewer and the Core Quality Gate remain the
  acceptance authority.
- P4 live is `NOT_TESTED`; the deterministic regression is the negative control.

## 6. Stop conditions

- Missing real route/credential, a non-disposable DB path, or a Core refusal
  code that shows the ingestion/approval configuration is not the production
  one.
- Any pre-intervention fact missing, P2 already done, dangerous/ambiguous late
  effects, or a claim before the kill.
- Direct provider authorization not refused, or the approved ActionSpec differs
  from the contract.
- Gate/Campaign/restart persistence incomplete (recorded as `OK_SCOPED`, not
  accepted), or the caller-claimed amount/owner disagrees with Core.
