# Real generation-2 recovery scenario (P1–P3 live, P4 deterministic)

**Status:** implementation prepared; no live stack, build or paid inference has
been started. Owner authorization covers the real/paid runs and the P1–P4
acceptance change. This document is the runbook for the disposable real stack,
the narrow credential provisioning and the evidence contract.

## 1. Scope

| Dimension | Requirement |
| --- | --- |
| P1 | Generation 1 persists and verifies the first functional result with a real paid provider/model session. |
| P2 | The second functional result is still pending at a real pre-intervention barrier and is completed after the Worker restart. |
| P3 | The generation-2 exact check receipt, post-claim candidate, Gate, Campaign and restart (when the ActionSpec requires it) are observed. |
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

## 3. Narrow credential provisioning

Script: `scripts/e2e/real-recovery-provision.sh` (`--check` first, `--apply`
after review). It is development/UAT tooling, not the product install path.

- Destination: a **new** disposable project (`*recovery*`/`*uat*`) with its own
  `factory-data` and `factory-config-core` volumes. No product DB or config is
  cloned; no product operator/reviewer/master identity is carried.
- Source access is read-only: volume exports into a 0700 temporary directory,
  files 0600, `umask 077`, `set +x`, trap cleanup.
- Copy allowlist: exactly one `provider_settings` row for `opencode-go` with the
  schema's exact column list (ciphertext, nonce, fingerprint; no plaintext, no
  other provider, no `provider_settings_audit`, no other table).
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

1. Create the execution profile, project policy and the Lead intake with a
   positive per-request `authorized_micros`. Record the Core authorization
   record (`/campaigns/{id}/authorization`: authorization id, delegation,
   envelope, route) and the owner reference; verify the ActionSpec binding.
2. Verify the approved ActionSpec against the exact contract (`status()` and
   `version()` Python checks). A different plan is `INCONCLUSIVE`/`FAIL`; the
   plan is never rewritten to pass.
3. Wait for the generation-1 `status` PASS receipt and no `version` PASS
   receipt. Build the pre-intervention snapshot: attempt status, generation-1
   sessions, tool events, last turn event, `active_turns=0`, lease state,
   `gen1_quiescent`, backend-confirmed quiescence, P2 check/artifact absence,
   `candidate_count=0`, `observed_at`. Any missing fact is `NOT_TESTED`.
4. Revalidate P2 absence immediately before the kill. Events between the kill
   and the generation-2 claim are classified: tool `RESULT`, provider turns,
   receipts, candidates and progress are dangerous; session terminal/cleanup and
   worker terminal reporting are legitimate; unknown is ambiguous. Dangerous or
   ambiguous ⇒ `NOT_TESTED`, never PASS.
5. Kill the Worker, await the lease `EXPIRED`, start it again, observe
   `worker.claimed generation=2` for the same attempt, with
   `observed_at < kill_at < claim.recorded_at`.
6. Verify the generation-2 `version` PASS receipt with the exact command hash,
   session, generation and sequence; the candidate after the claim with
   SHA/tree/base; the Gate outcome; the Campaign status; and restart evidence
   when the ActionSpec requires it.
7. Result: `ACCEPTED` when P1–P3 and Gate/Campaign/restart are complete;
   `OK_SCOPED` when P1–P3 are proven but the acceptance envelope (Gate,
   Campaign, restart) is incomplete; otherwise `NOT_TESTED`/`FAIL` with the
   causal reason.

## 5. Evidence and truthfulness

- Capture before cleanup: authorization record, approved ActionSpec, pre-
  intervention snapshot, events, receipts (exact columns), provider sessions,
  candidate, Gate, Campaign, negative direct-authorization response.
- No secrets, ciphertext, plaintext credentials, raw prompts or provider bodies.
- The independent package (`real-e2e-authorized/independent`) validates the
  bundle; the Verifier/Reviewer and the Core Quality Gate remain the acceptance
  authority.
- P4 live is `NOT_TESTED`; the deterministic regression is the negative control.

## 6. Stop conditions

- Missing real route/credential or a non-disposable DB path.
- Any pre-intervention fact missing, P2 already done, dangerous/ambiguous late
  effects, or a claim before the kill.
- Direct provider authorization not refused, or the approved ActionSpec differs
  from the contract.
- Gate/Campaign/restart incomplete (recorded as `OK_SCOPED`, not accepted).
