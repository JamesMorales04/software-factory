# Managed live acceptance — OpenCode / DeepSeek (2026-10-26)

**Status:** **PASS** for the managed external-project journey through the normal
`factory start` path: real OpenCode/DeepSeek Lead, frozen plan, authorized
`TRUSTED_LOCAL_PROVIDER` Implementer, immutable candidate, independent
rootless-Podman verifier, semantic Reviewer and deterministic `ACCEPTED` gate.
The controlled self-development test is **PASS** and recorded separately in
[managed-self-development-2026-10-26.md](managed-self-development-2026-10-26.md).
Codex native ChatGPT-authenticated live inference remains **BLOCKED**.

This is a Linux `TRUSTED_LOCAL` developer-preview result. It is **not**
ISOLATED/SEC-01 certification, production deployment, Windows support, human
security review or an exact-invoice cost measurement.

## 1. Implementation and environment

| Item | Value |
|---|---|
| Authoritative main at entry | `9f888ac74512b392c8bf86f856c5a152db5b3566` |
| Implementation commit exercised | `ea2d92724d57d4f96838d84216f39db04b7c5511` |
| Follow-up hardening | `6ae3241bc7f9bb849d36ddbbff71eb90ef95d0c8` makes the first-write instruction conditional for `run_command`-only authorizations and strengthens the revocation/lost-connection tests; the write_file prompt text used by this run is unchanged. |
| Core image | `localhost/software-factory-core:dev`, image ID `7c7a886521920cbd89d15bd8d9bfc56a93bfd9f23b8dc78bd18267f7ca0b74bd` |
| Host Worker binary SHA-256 | `521235554819d241e4f7251f96a167044c05328913deb7fd9956bc6134ad0240` |
| Toolchain / runtime | Rust 1.90.0, rootless Podman 6.1.4 |
| Provider | OpenCode `1.18.32`, route `opencode-go/deepseek-v4.1-flash`, mode `build` |
| Core model route | `opencode-go/deepseek-v4.1-flash` for MODEL Lead and semantic Reviewer |
| Verifier image | `docker.io/library/rust:1.90.0-bookworm` |
| Execution mode | `TRUSTED_LOCAL_PROVIDER`, not isolated |

`factory start` created the real Core container, host Supervisor and host
Worker, published Core/Office only on `127.0.0.1:8893`, and reported the
configured Worker provider profile. The verifier and reviewer are documented
one-shot independent roles, not hidden auxiliary services. No host Core or
Worker was started manually for the accepted journey.

## 2. Authorization

The Product Owner approved a bounded paid envelope before inference and later
approved one replacement provider session after a verifier-harness error. The
approved limits were:

- route `opencode-go/deepseek-v4.1-flash`, no fallback model;
- disposable Rust discount repository, allowed paths `src/` and `tests/`;
- model-backed Lead, one Implementer session per accepted attempt, semantic
  Reviewer;
- max 18 upstream/model requests, 250,000 total tokens and USD 2.00 equivalent;
- 120-minute deadline, no silent retry, credentials only in private `0600`
  files, cleanup of the installation and provider sessions.

The accepted replacement session was separately bounded to one provider
session, four turns, 60,000 input / 12,000 output tokens, USD 0.50 and a
30-minute envelope; the Core per-Attempt deadline was 600 seconds. The total
measured spend stayed below both envelopes.

## 3. Journey identities

| Stage | Identity |
|---|---|
| Project | `8e4bcfb4-8e6e-4152-aa93-bb9c94b4dc07` |
| Project config hash | `a91226823f2645ddd87762bcd9dd740507923d98f7a438294a29e833671be5e0` |
| Action | `4983fb52-92b3-4617-9b51-8465173de461` |
| Delegation | `managed-live-grant-3` |
| Lead request | `cd80c1bd-ae8c-4d91-8372-338aefebe83b` |
| Lead assessment | `4d47a496-0aaa-44dc-a8cc-c17b5d350791`, mechanism **MODEL** |
| Lead proposal | `4af951ab-57e1-4746-831f-d93b8a9584a4` |
| Frozen plan | `89dfc69d-4d47-4875-b45d-8cf5e0f21693`, hash `f6ca7b3ab0d62ec398efd66d57231dc156ce7efd2862c7bd16a51a98ace1c13d` |
| Campaign | `07ba7902-c75b-46d5-a508-dd8bedfbbe6d`, `AUTHORIZED`, R0 |
| Run / Attempt | `62216512-61ce-4774-b94d-4db22a3ea1c1` / `469d781e-9fd4-4306-88b2-8603b059ae24` |
| Provider session | `1006ffab-5672-452d-bf15-837172500de0`, OpenCode `ses_e86c567c0ffeeu7YjNPnHfJI11` |
| Candidate | `177060b3-5271-4b0f-a20f-2c5649426484` |
| Candidate commit / tree | `067c8569e651e0deb9b3edf4bfd7430b99a243b1` / `1ff574f70fcb2968ba7345399dad1cba11ef890d` |
| Verifier receipt | `ec36f22a-c64a-4408-8458-7caf1453ee15`, **PASS**, `EXECUTED` |
| Reviewer receipt | `c4074d5d-fdaa-4e6f-8cb5-a92b9062d500`, **PASS**, `MODEL` |
| Reviewer reservation | `a7f6d9c8-d42c-44b3-bbd9-f1ff6907694d` |
| Quality Gate | **ACCEPTED** at `2026-10-26T17:22:02.444284771+00:00` |

The model Lead returned a schema-valid `change` / `small` assessment with a
1–2 person-hour estimate, one `unit-1` implementation WorkGraph unit, allowed
paths `src` and `tests`, and the frozen build/test/functional/negative checks.

## 4. Actual source change

The accepted Implementer wrote only `src/pricing.rs` and `tests/discounts.rs`:

```diff
-    let percent = u64::from(percent.min(100));
-    let discount_cents = subtotal_cents.saturating_mul(percent) / 100;
-    subtotal_cents
-        .saturating_sub(discount_cents)
-        .saturating_sub(discount_cents)
+    let percent = u128::from(percent.min(100));
+    let subtotal = u128::from(subtotal_cents);
+    let discount_cents = subtotal * percent / 100;
+    (subtotal - discount_cents) as u64
```

The model also added maximum-subtotal, fractional, zero-discount, full-discount
and cart-total tests. The public function signature and cart API were unchanged.
The complete diff is [candidate.diff](managed-live-uat-2026-10-26/candidate.diff).

The Implementer exhausted four turns without a final prose explanation and its
last `run_command` check returned an operator-visible error; the Worker
preserved the applied writes for independent verification. The model
explanation is therefore **PARTIAL**, while the generated code, independent
execution and semantic review are **PASS**.

## 5. Usage and accounting

Measured Core records, not invoice estimates:

| Call / session | Input | Output | Total | Cost (micros) |
|---|---:|---:|---:|---:|
| Lead, accepted plan (`f6b52869…`) | 3,393 | 1,558 | 4,951 | 1,444 |
| Semantic Reviewer (`a7f6d9c8…`) | 2,616 | 1,565 | 4,181 | 1,332 |
| Implementer session `1006ffab…` | 9,608 | 1,033 | 30,430 | 3,924 |
| Earlier accepted-candidate session `dd0fb0a5…` | 8,135 | 1,363 | 26,861 | 2,980 |
| Failed session `aaad7896…` | 2,784 | 415 | 5,721 | 1,110 |
| Three earlier settled Lead calls (including the blocked-budget proposal) | — | — | 14,268 | 3,984 |
| **Committed-export total** | | | **86,412** | **14,774** |
| Stale-image entry Lead call (reconstructed; not in committed export) | — | — | 5,457 | 1,742 |
| **Reconstructed total across both installs** | | | **91,869** | **16,516** |

The accepted provider session ended `COMPLETED`, `usage_known=true`, with
cleanup receipt `session_delete_verified_absent`; the Core model calls settled
with known usage. The failed session ended `FAILED` with known usage and
cleanup `sandbox_process_identity_absent`; one bound-but-unsettled turn
reservation (`2342f2e3…`) remains **RESERVED** with unknown usage rather than
being treated as zero cost. It is visible through
`GET /provider/turn_reservations` and was not silently resolved. The
committed-export total is **86,412 tokens / 14,774 micros**; the stale-image
entry Lead call was recorded in the first installation's database before that
disposable install was removed, so the combined 91,869 / 16,516 figure is
reconstructed from the report's extracted record and is a lower-bound
accounting total, not a complete exported ledger. The reserved turn's
potential cost is unknown; even at its 498,890-micro ceiling the primary
envelope is not exceeded. Costs are USD equivalents at configured rates, not
an external invoice.

The old-format provider session totals include OpenCode's cache/reasoning
accounting; the input/output columns are the reported message fields.

## 6. Independent verification, semantic review and gate

The verifier ran the frozen checks for the exact candidate in a separate
rootless-Podman named-volume sandbox with the Rust image and `--network=none`.
It submitted `PASS`/`EXECUTED`, bound to commit `067c8569` and tree
`1ff574f7`. The receipt does not persist the backend, image, network or
per-check output; the rootless-Podman mechanism is author-attested by the
invocation and the separately verified fixture. The exact-candidate executed
receipt is the authoritative verifier evidence. The verifier log is
[verifier.log](managed-live-uat-2026-10-26/verifier.log).

The semantic Reviewer was a distinct principal (`managed-live-reviewer-2`),
ran its mandatory deterministic pre-review first, then a real MODEL call. It
submitted `PASS` with no findings, real provider/model provenance, request
SHA-256 `0fbb36e0…`, measured usage and the exact candidate tree. The
deterministic gate then returned **ACCEPTED** only because both independent
receipts existed for the same frozen candidate.

## 7. Managed lifecycle, Office and restart

`factory doctor` passed for the managed Core model and Worker provider env
files. `factory status` showed Core healthy, rootless Podman, Supervisor and
Worker running, the OpenCode profile active, and the Core model fingerprint.
The Rust Office at `http://127.0.0.1:8893` was unlocked with the operator token
and showed the real campaign/quality evidence:

- Dashboard: 2 projects, 3 candidates, 4 runs, the accepted run;
- Campaign detail: candidate `177060b3`, verifier and reviewer receipts, gate
  `ACCEPTED`;
- Runtime: settled Lead/reviewer calls and the completed Implementer session.

Screenshots and the captured page text are in this directory
([dashboard](managed-live-uat-2026-10-26/office-dashboard.png),
[campaign detail](managed-live-uat-2026-10-26/office-campaign-detail.png),
[runtime](managed-live-uat-2026-10-26/office-runtime.png),
[observations](managed-live-uat-2026-10-26/office-observations.json)).

`factory stop` removed the Core container, retained the data volume by design,
and `factory start` recreated the Core with the same data. After restart the
exact gate remained `ACCEPTED`, the projects remained registered, and Core
health returned 200. The disposable container, data volumes, provider session
roots, project clone and credential files were removed at the end. Evidence
was copied out before deletion.

The Office page produced two unexplained 404 console entries; the observed
views and API-loaded values were correct. The campaign record remains
`AUTHORIZED` and the Lead request remains `RUNNING` after the run is
`ACCEPTED`; the authoritative gate and Run states are correct. These are
projection/UX observations, not acceptance failures.

## 8. Reliability and accounting evidence

Deterministic checks passed at `ea2d927` and follow-up `6ae3241` (full
workspace tests, strict Clippy and format). The specific matrix is:

| Requirement | Evidence | Result |
|---|---|---|
| Running-turn cancellation | `campaign_cancellation_fences_a_leased_attempt_through_restart`; worker cancellation projection test | PASS (deterministic) |
| Revocation during execution | `delegation_revocation_fences_the_active_provider_session_and_lease` (new) | PASS (deterministic) |
| Worker interruption / restart | `core_restart_preserves_durable_project_action_and_events`; fan-in restart recovery | PASS (deterministic) |
| Lost provider connection | `broker_does_not_retry_an_upstream_that_drops_the_connection` (new) | PASS (deterministic, no automatic retry) |
| Ambiguous upstream completion | `model_call_budget_unknown_usage_and_ambiguity_fail_closed`; `unknown_turn_usage_leaves_the_bound_reservation_reserved` | PASS (deterministic) |
| Budget exhaustion | model-call scope/global budgets; CAP-014 budget exhaustion; provider session budget | PASS (deterministic) |
| Unknown usage | provider-session unknown-usage and operator-resolution tests | PASS (deterministic) |
| Stale lease | `stale_lease_generation_is_rejected_by_worker_api`; post-restart stale lease | PASS (deterministic) |
| Stale/duplicate tool request | provider tool-event uniqueness/budget tests | PASS (deterministic) |
| Cleanup after failure | failed-terminal cleanup receipt tests; recovery orphan cleanup | PASS (deterministic) |
| Paid in-flight provider cancellation/revocation | not run under this envelope | **NOT_TESTED (paid)** |

## 9. Codex disposition

The Codex App Server `0.156.1` adapter is merged and deterministic tests drive
the real executable to a candidate with exact upstream tool replacement and
bound requests. `factory doctor` and `factory start` accept a strict Codex
Worker profile with the real native binary, egress bridge, upstream URL and
explicit input/output cost rates; this was verified with a placeholder token
and **no paid inference**
([logs](codex-managed-config-2026-10-26/doctor.log)). A real managed Codex
coding interchange was **not** run.

The installed Codex is ChatGPT-authenticated. A secure native ChatGPT-backend
route is not available through the current Factory boundary: the egress broker
injects one long-lived bearer credential and deliberately forwards only a
bounded header subset, while the ChatGPT backend requires the Codex OAuth
access token and account header to be owned and refreshed by Codex outside the
model request. Exporting those tokens from `~/.codex/auth.json` would be
unsupported credential extraction, and allowing Codex to hold them inside the
sandbox would violate the credential boundary. The supported route remains an
operator-supplied API key with `https://api.openai.com/v1` as the pinned
upstream. The earlier test-only ChatGPT proxy is **not** part of the product.

## 10. Failures found and disposition

1. The first managed run used a stale Core image that did not parse the
   explicit Lead-start authorization; it created a FIXTURE Campaign. The image
   was rebuilt from the tested tree; the one Lead call is retained in the
   measured total. The failed Campaign produced no provider session.
2. The first real Implementer session ended after an empty final response and a
   broker per-turn budget exhaustion. It was recorded `FAILED` with known
   usage and process-absence cleanup. The Worker prompt now requires the first
   response to be a `write_file` request; one replacement session was
   explicitly authorized.
3. The first accepted-code candidate (`fc057c39`) was genuine, but the verifier
   invocation inherited `FACTORY_VERIFIER_TEST_BACKEND=local` and submitted an
   immutable FAIL receipt. The candidate remains immutable and is **not** the
   acceptance candidate. Rootless-Podman verification was independently
   demonstrated on a deterministic fixture before a fresh provider session.
4. A replacement Lead proposal exceeded a deliberately narrow delegation
   budget; Core rejected it before start and preserved the failure reason. A
   wider delegation and a new Lead request were used, staying within spend.
5. The model's own frozen `run_command` checks frequently returned errors while
   the independent verifier passed. The Worker correctly preserved writes;
   check-command reliability remains an open reliability item.
6. The failed session's external-session deletion was not verified
   (`sandbox_process_identity_absent`) and one bound turn remains `RESERVED`.
   This is truthful accounting, not zero cost.

## 11. Verdicts

| Dimension | Verdict |
|---|---|
| Managed live startup | **PASS** |
| Real OpenCode inference | **PASS** |
| Model-backed Lead / frozen plan | **PASS** |
| Authorized TRUSTED_LOCAL Campaign | **PASS** |
| Real model-generated candidate | **PASS** |
| Independent executed verifier | **PASS** (rootless Podman) |
| Semantic model Reviewer | **PASS** |
| Deterministic Quality Gate | **PASS / ACCEPTED** |
| Office presentation | **PASS with two unexplained 404 console entries** |
| Restart and persistence | **PASS** |
| Cleanup | **PASS**, with one unverified external deletion and one reserved turn |
| Reliability matrix | **PASS deterministic; paid failure injection NOT_TESTED** |
| Codex deterministic adapter | **PASS** |
| Codex managed configuration | **PASS preflight** |
| Codex native ChatGPT live route | **BLOCKED** |
| Self-development | **PASS** in a separate report |
| Isolation / production / Windows / human review | **NOT CLAIMED** |

## 12. Non-claims

This record does not certify SEC-01 isolation, credential confinement,
production readiness, Windows, GitHub delivery, human security review or final
merge. A TRUSTED_LOCAL process has broad host access. The provider credential
remained in the trusted Core/Worker; the sandbox received only per-run
capability tokens. The candidate was not merged. The cost is measured at
configured rates, not an external invoice.
