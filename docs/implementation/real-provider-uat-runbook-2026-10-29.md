# Real-provider acceptance runbook — subscription-only OpenCode Go

**Date prepared:** 2026-10-29
**Policy baseline:** Product Owner funding policy — **subscription-included only, no additional charges**
**Prepared candidate:** `3290799` plus this subscription-policy amendment
**Status:** **PREPARED — NOT RUN; LIVE EXECUTION BLOCKED.** No provider inference
has been performed for this runbook. `#60` live-provider acceptance remains
`NOT_TESTED`. The current Core lacks subscription admission; Owner Console
verification and the separate go-ahead cannot authorize a run until Core
subscription admission is implemented and the read-only preflight passes.

This runbook prepares one small, bounded real-model Factory Campaign under the
current subscription-only policy. It does not authorize merge, deployment, a
GitHub pull request, ISOLATED execution, SEC-01 certification or any security
claim. It is UAT author-run evidence, not independent acceptance.

## 1. What the run exercises

The run drives the real Compose product path through one disposable Project:

```text
Project import
  -> operator Request (REVIEW_PLAN)
  -> MODEL Lead assessment (real subscription-included OpenCode Go call)
  -> Owner review of the exact frozen plan hash
  -> plan approval and Campaign start
  -> TRUSTED_LOCAL_PROVIDER Implementer session (subscription-included)
  -> independent verifier (rootless nested Podman, frozen checks)
  -> independent semantic Reviewer (subscription-included)
  -> deterministic Quality Gate
  -> candidate diff + PR-preparation inspection (no GitHub write)
```

The disposable stack uses `compose.yaml` plus the test-only
`compose.real-provider-uat.yaml`. It does not start the deterministic model
fixture. The deterministic fixture-provider acceptance is already recorded in
[`compose-browser-uat-2026-10-29`](../evidence/compose-browser-uat-2026-10-29/README.md)
and is separate evidence.

## 2. Funding policy and accounting

### 2.1 Authoritative policy

Current Product Owner policy permits **only** `GO_SUBSCRIPTION_INCLUDED` usage.
The following are refused before any request:

- `GO_EXTRA_USAGE_BILLABLE`;
- `PAY_AS_YOU_GO_API`;
- any other paid/balance route;
- any required or supplied positive USD spend cap.

The harness reports `PAID_MODE_DISABLED` for paid modes and
`SPEND_CAP_NOT_APPLICABLE` for a supplied cap. The #73 monetary ledger and its
security remain intact for a future explicitly authorized billable path, but no
billable path is enabled here.

### 2.2 Authoritative accounting sources

| Source | Observation (2026-10-29) |
|---|---|
| `opencode --version` | `2.2.0` |
| `opencode models` | `opencode-go/deepseek-v4.1-flash` is listed; 30 `opencode-go/*` routes |
| `opencode auth list` | `OpenCode Go  API key  stored` (presence only; the value was never read) |
| `~/.cache/opencode/models.json` | provider `opencode-go`, API `https://opencode.ai/inference/go/openai/v1`, doc `https://opencode.ai/v2/docs/console/go/`; `deepseek-v4.1-flash` list rates: input `$0.15/M`, output `$0.60/M`, cached read `$0.003/M` |
| `opencode.db` read-only message projection | recent `opencode-go/deepseek-v4.1-flash` messages carry a **nonzero** local `cost` (for example `0.00267897` for 2,591 input / 265 output / 349 reasoning / 640,640 cached-read tokens). The arithmetic matches the list rates, so this is OpenCode's local list-rate estimate, not the provider's `usage.cost` and not a balance charge |
| `opencode stats --days 1 --cost --json` | today's `opencode-go/deepseek-v4.1-flash` model row reports `cost: 0` while the same messages carry nonzero list-rate estimates. This is a local statistics view, not the Console ledger, and does not expose remaining entitlement |
| [OpenCode Go documentation](https://opencode.ai/v2/docs/console/go/) | Go/Go Plus/Go Teams subscription plans; usage limits are monthly **dollar-denominated allowances**; DeepSeek V4.1 Flash monthly allowance is `$80` (Go), `$160` (Go Plus), `$85` (Go Teams); 5-hour = 20%, weekly = 50%, monthly = 100%; peak hours 01:00–04:00 and 06:00–10:00 UTC Mon–Fri (2× list rates) |
| [OpenCode Go — usage beyond limits](https://opencode.ai/v2/docs/console/go/#usage-beyond-limits) | Extra Usage is an opt-in fallback to the Console balance after the Go allowance. **Go Teams starts with Extra Usage on.** Current policy requires it OFF. |
| [OpenCode inference API cost](https://opencode.ai/v2/docs/console/api/inference#cost) | `usage.cost` is USD **charged to the balance**; `0` for Go usage within its limits. `opencode.billing_source` is `free`, `balance`, `go`, `go-plus` or `byok`; `opencode.allowance` counts toward the Go monthly limit |
| [OpenCode inference API authentication](https://opencode.ai/v2/docs/console/api/inference#authentication) | Free-tier chat models are only usable from inside OpenCode; Go models are usable from other clients |

The local message-cost probe used only the non-credential `message` table:

```sh
sqlite3 "file:$HOME/.local/share/opencode/opencode.db?mode=ro" \
  "SELECT json_extract(data,'$.providerID'), json_extract(data,'$.modelID'), json_extract(data,'$.cost'), json_extract(data,'$.tokens.input'), json_extract(data,'$.tokens.output') FROM message WHERE json_extract(data,'$.providerID')='opencode-go' ORDER BY time_created DESC LIMIT 5;"
```

### 2.3 The four evidence layers must remain separate

1. **Subscription entitlement/quota.** The Owner Console observation records the
   plan, exact model entitlement, Extra Usage OFF, balance fallback OFF,
   no-charge route VERIFIED and quota/reset state. The current Core and local
   preflight **cannot verify the Console**; the evidence records that
   limitation. Missing, stale or unverifiable evidence fails closed.
2. **Measured usage.** Tokens reported by Core/provider records: input, output,
   total (including reasoning/cache as reported), per role and provider.
3. **List-rate equivalent.** A calculation from the Core-configured rates plus
   OpenCode's local message-cost estimate. It carries a rate revision and
   provenance. It is **not** an actual charge and **not** a spending cap.
4. **Provider-reported actual charge.** `VERIFIED_ZERO`, `VERIFIED_POSITIVE` or
   `UNKNOWN`, with the evidence source. A positive charge or a paid
   `billing_source` on a subscription run is an incident and blocks further
   dispatch. Absence of charge evidence is `UNKNOWN`, never zero.

The authoritative remaining allowance, quota reset and invoice are in the
OpenCode Console. No local CLI or Factory endpoint exposes them. The preparer did
not sign in to the Console and did not read `~/.local/share/opencode/auth.json` or
any credential store. No current balance or allowance is claimed here.

**Unknowns that must remain explicit:** plan/workspace entitlement, current
remaining allowance, 5-hour/weekly usage, quota reset, Extra Usage state, actual
provider charge when Core has no charge evidence, taxes/currency conversion and
the final invoice.

### 2.4 Peak/off-peak

At preparation time (2026-10-29 14:54 UTC) DeepSeek V4.1 Flash was Off-Peak, so
the list rates were `$0.15/M` input and `$0.60/M` output. Peak rates are double.
The run records actual UTC start/finish and a peak/off-peak schedule; never
assume Off-Peak.

## 3. Preconditions

1. The Owner has an OpenCode Go plan covering `deepseek-v4.1-flash`.
2. The Owner signs in to <https://opencode.ai/console> and verifies:
   - the plan/workspace and exact model entitlement;
   - **Extra Usage is OFF**;
   - **balance fallback is OFF**;
   - the current quota state, remaining amount and reset time, or explicitly
     records them as unknown.
3. The Owner creates a `0600` subscription verification JSON file with the
   fields in §4. This is Owner-verified Console evidence, not a Core
   verification receipt. It must not contain a credential, API key, cookie or
   raw account identifier.
4. The Owner enters the OpenCode Go API key **directly into Office Settings**.
   The key is never passed to the harness, printed or committed.
5. A disposable Compose project whose name contains `uat` is used. The permanent
   `software-factory` stack is not touched.
6. The disposable Project is not bound to a GitHub repository; no PR is created,
   merged or commented on.
7. Core V3 subscription admission must be implemented and independently verified
   before any live run. The harness requires
   `GET /funding/subscription/opencode-go` to return a matching, fresh
   subscription binding (plan, account, exact model, Extra Usage OFF, balance
   fallback OFF, no-charge route VERIFIED and quota). This repository candidate's
   Core does not expose this endpoint, so the current live path is blocked and
   fails closed as `CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED` before creating a
   Project or Request. Owner Console evidence and the separate go-ahead do not
   bypass this Core admission requirement. **Do not add a cap, select billable
   mode or bypass this check.**

## 4. Owner verification and authorization required after Core is ready

After Core V3 is implemented and its read-only preflight passes, the Owner must
record all of the following in writing before the first provider call. Providing
this evidence now does not authorize live execution while Core admission is
missing.

| Decision | Required value |
|---|---|
| Funding route | `GO_SUBSCRIPTION_INCLUDED` only |
| Paid fallback | None; `GO_EXTRA_USAGE_BILLABLE` and `PAY_AS_YOU_GO_API` are disabled |
| Extra Usage | **OFF** in the Console; `FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED` plus fresh Console evidence |
| Balance fallback | **OFF** in the Console |
| Plan / workspace | Go, Go Plus or Go Teams; workspace name |
| Entitlement | Exact `opencode-go/deepseek-v4.1-flash` entitlement VERIFIED |
| Quota | AVAILABLE, UNKNOWN or EXHAUSTED; remaining/reset if observed |
| Spend cap | **None. No positive USD authorization is required or accepted.** |
| Authorization id | A unique id, for example `owner-rp-uat-2026-10-30` |
| Route | `opencode-go` / `deepseek-v4.1-flash`; no fallback provider or model |
| Sessions / turns | At most 1 Implementer provider session and 4 turns |
| Requests / tokens | At most one assessment Request; at most 60,000 input and 12,000 output tokens reported by Core |
| Deadline | At most 90 minutes from the start of the authorized run |
| Retries | None; an infrastructure failure is repaired before any new authorization |
| Quota exhaustion | Durable `PAUSED / SUBSCRIPTION_QUOTA_EXHAUSTED`; resume only after refreshed evidence, no fallback or redispatch |
| GitHub | No PR creation, merge, comment or review |
| Plan approval | The Owner personally reviews the exact frozen plan hash before `execute` |
| Final go-ahead | A **separate explicit Owner go-ahead** after verification and before the first real request |

Recommended authorization text:

> I authorize one bounded subscription-only real-provider UAT, id
> `owner-rp-uat-2026-10-30`, on `opencode-go/deepseek-v4.1-flash`. I verified in
> the OpenCode Console that Extra Usage and balance fallback are OFF, that my
> plan entitles this exact model, and that quota is
> [AVAILABLE/UNKNOWN/EXHAUSTED] with [remaining/reset or unknown]. There is no
> USD spend cap and no paid fallback. I authorize at most one Implementer
> session, four turns, 60,000 input and 12,000 output tokens within 90 minutes.
> No retry, no provider/model/funding fallback, and no GitHub write. I will
> review the frozen plan hash before execution. This is my separate explicit
> go-ahead for the first real request.

### Subscription verification file

Create it `0600` with only non-secret values:

```json
{
  "schema": "factory.subscription-verification.v1",
  "provider": "opencode-go",
  "model": "deepseek-v4.1-flash",
  "route": "GO_SUBSCRIPTION_INCLUDED",
  "plan": "Go",
  "account_fingerprint": "sha256:<non-secret account label hash>",
  "source": "OWNER_CONSOLE",
  "verification_method": "OWNER_CONSOLE_INSPECTION",
  "console_url": "https://opencode.ai/console",
  "observed_at": "2026-10-30T14:00:00Z",
  "valid_until": "2026-10-31T02:00:00Z",
  "entitlement": "VERIFIED",
  "extra_usage": "OFF",
  "balance_fallback": "OFF",
  "no_charge_route": "VERIFIED",
  "quota": {
    "state": "AVAILABLE",
    "remaining_usd": null,
    "resets_at": null
  },
  "core_verifiable": false,
  "limitation": "CORE_AND_LOCAL_PREFLIGHT_CANNOT_VERIFY_CONSOLE_EXTRA_USAGE",
  "rate_revision": "opencode-go-doc-2026-10-29",
  "notes": "Owner read the OpenCode Console at the observed time; no credential stored."
}
```

Rules:

- `source` is `OWNER_CONSOLE` for the current build; `core_verifiable` must be
  `false`. A future Core V3 export may use `source: "CORE"`,
  `verification_method: "CORE_EXPORT"` and `core_verifiable: true`.
- `entitlement` must be `VERIFIED`; `extra_usage` and `balance_fallback` must be
  `OFF`; `no_charge_route` must be `VERIFIED`.
- `observed_at`/`valid_until` must be fresh (default maximum age 24 hours;
  tighten with `FACTORY_REAL_PROVIDER_VERIFICATION_MAX_AGE_SECS`).
- `quota.state` may be `UNKNOWN`; unknown quota is not unlimited and is recorded
  as unknown. `EXHAUSTED` or zero remaining pauses before dispatch.
- No credential, API key, cookie, email or raw account identifier belongs in
  this file.

## 5. Harness and gates

Artifacts:

- `scripts/real-provider-uat.sh` — disposable stack, credential readiness and
  driver invocation.
- `scripts/real-provider-uat.mjs` — subscription-only gate, verification,
  assessment, approval/execution, quota pause/resume and redacted evidence.
- `compose.real-provider-uat.yaml` — test-only Git fixture and real-provider
  opt-in; no model fixture.
- `scripts/real-provider-uat.test.mjs` — contract tests for refusal, evidence
  separation, quota pause/recovery and secret redaction.

Run the contract tests with:

```sh
node --test scripts/real-provider-uat.test.mjs
```

They use a local fake Core and perform no inference.

The driver refuses to make a paid call unless **all** of the following are true:

- `FACTORY_REAL_PROVIDER_UAT=1` (live mode);
- `FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD=1` (the separate Owner go-ahead);
- `FACTORY_REAL_PROVIDER_AUTHORIZATION` is a bounded non-empty id;
- `FACTORY_REAL_PROVIDER_ACCOUNTING=GO_SUBSCRIPTION_INCLUDED`;
- `FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED`;
- no `FACTORY_REAL_PROVIDER_SPEND_CAP_USD` is supplied;
- `FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE` is a fresh, valid `0600`
  Owner Console verification;
- the Core URL is HTTPS or loopback HTTP, with no embedded credentials;
- the operator token is supplied by a `0600` file and is at least 32 bytes, or
  by a non-empty environment variable;
- Core returns a matching V3 `GET /funding/subscription/opencode-go` verification;
- the provider connection is configured, enabled, default, and its catalog is
  available with the exact model;
- the provider connection's `maxTurns` does not exceed the authorized turn
  ceiling (default 4);
- semantic review is enabled and a model gateway route exists;
- at least one Worker is registered.

The harness authenticates to Core with the local operator token read from
`/config/operator.token` into a `0600` file and removed on exit. It never prints
the token and does not reuse the Owner's browser session. Approval is audited as
`operator-token` with the label `owner-real-provider-uat`.

Token ceilings default to 60,000 input and 12,000 output and can be tightened.
The provider turn ceiling defaults to 4. These are technical bounds, not USD
spending caps.

## 6. Commands

### 6.1 Prepare the disposable stack

```sh
./scripts/real-provider-uat.sh prepare
```

The command prints the Office URL, a one-time enrollment code and the default
subscription verification path. The Owner:

1. Opens the Office URL and enrolls the device.
2. Opens **Settings → OpenCode Go**, enters the API key directly, enables the
   connection, selects **Default route for Core model calls**, and saves.
   Set the input/output rates to the current DeepSeek V4.1 Flash window
   (Off-Peak `$0.15/$0.60` per million, Peak `$0.30/$1.20` per million). If the
   run may cross a peak boundary, use Peak so the list-rate equivalent is
   conservative. These rates are **not** a spending cap.
3. Clicks **Test connection**. This reads the public catalog only; it performs
   no inference. It must show `AVAILABLE` with `deepseek-v4.1-flash`.
4. Enables semantic review under **Settings → Runtime** if it is not already on.
5. Performs the Console verification in §4 and creates the `0600` evidence file.
6. Gives the separate explicit go-ahead for the first real request.

### 6.2 Read-only preflight

```sh
export FACTORY_REAL_PROVIDER_DRY_RUN=1
export FACTORY_REAL_PROVIDER_AUTHORIZATION=owner-rp-uat-2026-10-30
export FACTORY_REAL_PROVIDER_ACCOUNTING=GO_SUBSCRIPTION_INCLUDED
export FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED
export FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE=/secure/owner-console-evidence.json
./scripts/real-provider-uat.sh preflight
```

Preflight performs only Core and catalog `GET`s. It does not create a Project,
Request, Campaign or provider session, and it does not prove inference access.
If Core returns `CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED`, stop; the current
candidate is blocked. Do not set the live flags or Owner go-ahead. Only remove
`FACTORY_REAL_PROVIDER_DRY_RUN` after a future Core candidate implements the
binding, the read-only preflight passes, and the Owner separately authorizes the
specific run.

### 6.3 Phase 1 — real MODEL assessment

```sh
export FACTORY_REAL_PROVIDER_UAT=1
export FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD=1
export FACTORY_REAL_PROVIDER_AUTHORIZATION=owner-rp-uat-2026-10-30
export FACTORY_REAL_PROVIDER_ACCOUNTING=GO_SUBSCRIPTION_INCLUDED
export FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED
export FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE=/secure/owner-console-evidence.json
export FACTORY_REAL_PROVIDER_OBJECTIVE='Fix apply_discount in src/pricing.rs so the discount is applied exactly once, and add tests for zero, fractional, full and maximum-subtotal discounts. Do not change the public API.'
./scripts/real-provider-uat.sh assess
```

This creates the Project, execution profile and Request **without**
`authorized_micros`, then runs the real Lead assessment. It stops at
`AWAITING_APPROVAL`, writes `plan.json` and `state.json`, and prints the exact
plan hash. It does not approve or execute the plan. The Lead call is
subscription-included usage.

If Core does not expose the V3 subscription verification, the run fails closed
as `CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED` before creating a Project or
Request; do not supply a cap.

Do not delete the evidence directory between phases. The state file prevents a
second paid request under the same authorization.

### 6.4 Phase 2 — execute the exact approved plan

Only after reviewing `plan.json` and the frozen hash:

```sh
FACTORY_REAL_PROVIDER_PLAN_APPROVAL=OWNER_REVIEWED \
  ./scripts/real-provider-uat.sh execute --plan-hash <exact-plan-hash>
```

The driver refuses a mismatched hash, approves without a USD authorization,
starts the Campaign, waits for the real Implementer, independent verifier, MODEL
Reviewer and deterministic Gate, and writes `real-provider-uat-evidence.json`.
It never creates a GitHub PR; it records `UNBOUND_READY` when no repository is
bound.

### 6.5 Quota pause and resume

If a verified quota-exhaustion response occurs, the harness records
`PAUSED_SUBSCRIPTION_QUOTA_EXHAUSTED`, preserves the Campaign, candidate/session
references and frozen plan, and exits `4`. It does not retry, re-approve, change
provider/model/account/funding or create a replacement request.

After the Console shows quota available again:

1. Refresh the subscription verification file with a new `observed_at`,
   `valid_until` and quota observation; keep the same plan/account/model.
2. Run:

```sh
./scripts/real-provider-uat.sh resume
```

`resume` requires Core V3's authenticated subscription resume operation. The
current pre-V3 Core does not expose it; the harness then fails closed as
`CORE_V3_SUBSCRIPTION_RESUME_REQUIRED` and does not fall back. A generic HTTP
429 or a paused state without the `SUBSCRIPTION_QUOTA_EXHAUSTED` reason is not
treated as subscription exhaustion.

### 6.6 Evidence and cleanup

```sh
tar -C "$FACTORY_REAL_PROVIDER_EVIDENCE_DIR" -czf /tmp/real-provider-uat-evidence.tgz .
./scripts/real-provider-uat.sh down
```

`down` removes the disposable Compose project and volumes. It does not revoke
the OpenCode Go credential; revoke it separately in Office if the stack was
created only for this run. Confirm:

```sh
podman ps -a --filter 'label=com.docker.compose.project=software-factory-rp-uat'
podman volume ls | grep software-factory-rp-uat || true
```

## 7. Stop conditions and failure handling

Stop on any of the following; do not retry within the same authorization:

- `ACCEPTED` gate (success) or any non-`ACCEPTED` terminal gate;
- subscription entitlement unknown/false/expired, Extra Usage enabled/unknown,
  balance fallback enabled/unknown, or stale verification;
- paid mode or spend cap supplied;
- provider/model/account/plan/revision mismatch from the frozen binding;
- quota exhaustion (durable pause) or a generic pause without a verified quota
  reason;
- unknown/ambiguous model or provider usage, unresolved provider turn, or
  unconfirmed cleanup;
- timeout, transport ambiguity, or any request to create/merge a GitHub PR.

A quota pause is recoverable only through `resume` after refreshed evidence and
unchanged authority. An infrastructure failure is repaired before any new
authorization. Do not silently reclassify a subscription run as paid.

## 8. Evidence to record

- Candidate wrapper commit, exact Compose files/project and UTC timestamps.
- Authorization id, funding policy, provider/model, plan, account fingerprint,
  Console observation time/validity and explicit limitation.
- Core V3 `GET /funding/subscription/{provider}` binding, or the fail-closed
  `CORE_V3_SUBSCRIPTION_ADMISSION_REQUIRED` result.
- Subscription entitlement, Extra Usage OFF, balance fallback OFF,
  no-charge route and quota/reset (or unknown).
- Project, request, Action, Campaign, Run, Attempt and candidate identities.
- Frozen plan hash, budget observation (not authorization) and Owner rationale.
- Implementer provider session record ID, OpenCode session ID, model, turns,
  terminal state, measured tokens, list-rate estimate, actual-charge observation
  and cleanup receipt. Preserve `billing_source` as a separate route-source
  observation; subscription or free route labels alone do not verify an actual
  zero charge.
- Lead and Reviewer model-call reservations, roles, measured tokens,
  configured-rate micros and actual-charge observation.
- Independent verifier receipt and MODEL Reviewer receipt.
- Quality Gate outcome and reasons.
- Candidate commit/tree, changed paths and diff summary.
- PR preparation: GitHub binding state and `PR_WRITE: NONE`.
- `PAUSED / SUBSCRIPTION_QUOTA_EXHAUSTED` evidence and resume outcome, if any.
- Cleanup receipts and surviving container/volume/provider session.

The evidence must keep **subscription entitlement/quota**, **measured usage**,
**list-rate equivalent**, **provider-reported actual charge** and **unknown**
fields separate. Configured-rate micros and the plan budget are never a balance
debit, invoice or spending cap.

## 9. Core V3 handoff (not implemented here)

The harness is aligned with the subscription-only funding contract but does not
implement Core V3. Required Core work, tracked separately:

- `FundingPolicy::{SubscriptionIncluded{...}, Billable{envelope_id}}` and
  `SubscriptionBinding`/`Observation<T>`/`QuotaState` types in
  `native/src/execution_policy.rs`;
- `ExecutionPolicyV3` with subscription admission and no positive USD envelope;
- migration `0041_subscription_funding_policy.sql` (if still unallocated) with
  immutable funding/binding evidence, append-only observations and explicit
  unknown actual-charge fields;
- `GET /funding/subscription/{provider}` returning the typed subscription binding
  consumed by the harness;
- funding-aware request/assessment/approval APIs and authenticated quota resume;
- durable `PAUSED / SUBSCRIPTION_QUOTA_EXHAUSTED` transitions preserving work;
- Office subscription/quota/usage projections with no mandatory spend form;
- no-charge enforcement and fail-closed paid-route selection.

Until that exists, the real Core may reject this harness at subscription
admission or resume. That is the correct fail-closed result; do not add a cap,
select billable mode, export a provider credential or bypass Core.

## 10. Non-claims

- No live result is claimed by this preparation. The contract tests use a local
  fake Core and perform no inference.
- No ISOLATED, SEC-01, credential-confinement, production, Windows, human
  security-review or GitHub delivery claim.
- A subscription allowance is not "free" or unlimited; it consumes a monthly
  allowance.
- List-rate equivalents and plan budgets are not actual charges or spending
  caps.
- Provider-reported actual charge remains `UNKNOWN` unless Core supplies
  explicit charge evidence.
- Author-run UAT is not independent acceptance. The independent verifier and
  Reviewer are independent roles, not independent acceptance of this runbook.
- The provider credential remains in Core/Worker trust; `TRUSTED_LOCAL` is not
  isolated.

### Office impact: none

This change adds operator tooling and documentation only. It adds no Office
route, control, projection or user-facing capability. The live run uses existing
Office Settings, Runtime, Request and Campaign surfaces already mapped in
[the Office parity matrix](../product/office-parity.json). The harness calls only
existing authenticated Core operations. The parity matrix is unchanged and the
impact is explicitly **none**; the required capability remains #60, which stays
`NOT_TESTED` until the Owner verifies Console evidence, gives the separate
go-ahead and executes the run.

## 11. Open accounting follow-ups

1. Persist raw `usage.cost`, `opencode.billing_source` and
   `opencode.allowance` so actual charge and allowance consumption are
   measurable instead of unknown.
2. Add a Core subscription entitlement/quota verification export and consume it
   in the harness instead of Owner Console evidence.
3. Verify the actual peak/off-peak window at request time and record it.
4. Reconcile OpenCode's local message `info.cost` list-rate arithmetic against
   the raw inference API `usage.cost` and billing source for both subscription
   and future billable routes.
5. Implement the Core V3 quota pause/resume and Office subscription/quota
   projections listed in §9.
