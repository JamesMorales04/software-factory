# Real-provider acceptance runbook — OpenCode Go

**Date prepared:** 2026-10-29
**Prepared candidate:** `2907b17` plus this preparation commit
**Status:** **PREPARED — NOT RUN.** No paid inference has been performed for this
runbook. `#60` live-provider acceptance remains `NOT_TESTED` until the Product
Owner authorizes the envelope in §4 and the commands in §6 are executed.

This runbook prepares one small, bounded real-model Factory Campaign. It does not
authorize merge, deployment, a GitHub pull request, ISOLATED execution, SEC-01
certification or any security claim. It is UAT author-run evidence, not
independent acceptance.

## 1. What the run exercises

The run drives the real Compose product path through one disposable Project:

```text
Project import
  -> operator Request (REVIEW_PLAN)
  -> MODEL Lead assessment (real OpenCode Go call through Core)
  -> Owner review of the exact frozen plan hash
  -> plan approval and Campaign start
  -> TRUSTED_LOCAL_PROVIDER Implementer session (real OpenCode Go call)
  -> independent verifier (rootless nested Podman, frozen checks)
  -> independent semantic Reviewer (real OpenCode Go call through Core)
  -> deterministic Quality Gate
  -> candidate diff + PR-preparation inspection (no GitHub write)
```

The disposable stack uses `compose.yaml` plus the test-only
`compose.real-provider-uat.yaml`. It does not start the deterministic model
fixture. The deterministic fixture-provider acceptance is already recorded in
[`compose-browser-uat-2026-10-29`](../evidence/compose-browser-uat-2026-10-29/README.md)
and is separate evidence.

## 2. Accounting investigation

### 2.1 Authoritative sources

| Source | Observation (2026-10-29) |
|---|---|
| `opencode --version` | `2.2.0` |
| `opencode models` | `opencode-go/deepseek-v4.1-flash` is listed; 30 `opencode-go/*` routes are available |
| `opencode auth list` | `OpenCode Go  API key  stored` (presence only; the value was never read) |
| `~/.cache/opencode/models.json` | provider `opencode-go`, API `https://opencode.ai/inference/go/openai/v1`, doc `https://opencode.ai/v2/docs/console/go/`; `deepseek-v4.1-flash` list rates: input `$0.15/M`, output `$0.60/M`, cached read `$0.003/M` |
| `opencode.db` read-only message projection | recent `opencode-go/deepseek-v4.1-flash` messages carry a **nonzero** local `cost` (for example `0.00267897` for 2,591 input / 265 output / 349 reasoning / 640,640 cached-read tokens). The arithmetic matches the list rates, so this is OpenCode's local message-cost estimate, not the provider's `usage.cost` and not a balance charge |
| `opencode stats --days 1 --cost --json` | today's `opencode-go/deepseek-v4.1-flash` model row reports `cost: 0` while the same messages carry nonzero message cost. This is a local statistics view, not the Console account ledger; it does not expose the remaining Go allowance |
| [OpenCode Go documentation](https://opencode.ai/v2/docs/console/go/) | Go `$10/month`, Go Plus `$40/month`, Go Teams `$20/member/month`; usage limits are monthly **dollar amounts**; DeepSeek V4.1 Flash monthly limit is `$80` (Go), `$160` (Go Plus), `$85` (Go Teams); 5-hour = 20%, weekly = 50%, monthly = 100% of the limit; peak hours are 01:00–04:00 and 06:00–10:00 UTC Monday–Friday (2× list rates); Off-Peak otherwise |
| [OpenCode Go documentation — usage beyond limits](https://opencode.ai/v2/docs/console/go/#usage-beyond-limits) | **Extra Usage** is an opt-in fallback to the Console pay-as-you-go balance after the Go allowance is exhausted. **Go Teams starts with Extra Usage on.** |
| [OpenCode inference API cost](https://opencode.ai/v2/docs/console/api/inference#cost) | `usage.cost` is USD **charged to the balance**; `0` for free models and for Go usage **within its limits**. `opencode.billing_source` is `free`, `balance`, `go`, `go-plus` or `byok`; `opencode.list_cost` is the list price; `opencode.allowance` is how much the request counted toward the Go monthly limit |
| [OpenCode inference API authentication](https://opencode.ai/v2/docs/console/api/inference#authentication) | Free-tier chat models are only usable from inside OpenCode; other clients receive HTTP `403`. Go models are explicitly usable from other clients. |

The local message-cost probe used only the non-credential `message` table:

```sh
sqlite3 "file:$HOME/.local/share/opencode/opencode.db?mode=ro" \
  "SELECT json_extract(data,'$.providerID'), json_extract(data,'$.modelID'), json_extract(data,'$.cost'), json_extract(data,'$.tokens.input'), json_extract(data,'$.tokens.output') FROM message WHERE json_extract(data,'$.providerID')='opencode-go' ORDER BY time_created DESC LIMIT 5;"
```

### 2.2 The three funding modes are not interchangeable

1. **Subscription-included usage (Go / Go Plus / Go Teams).**
   Requests that stay inside the plan report `cost = 0` and
   `billing_source = go`/`go-plus`; they still consume a dollar-denominated
   monthly allowance, tracked by `opencode.allowance`. This is **not free and not
   unlimited**. The subscription fee is fixed, but exceeding the allowance blocks
   requests unless Extra Usage is on.
2. **Additional paid usage (Extra Usage / balance).**
   After the allowance is exhausted, Extra Usage can fall back to the Console
   balance. Those requests report a positive `cost` and
   `billing_source = balance`. This is billable. Go Teams enables Extra Usage by
   default; the Owner must verify the actual setting before this run.
3. **Separately billable API requests.**
   A personal API key against the general Console models endpoint is
   pay-as-you-go balance usage independent of the Go allowance. Factory is
   configured for the **Go route** `https://opencode.ai/inference/go/openai/v1`,
   so this run is intended to consume Go allowance, not the general API balance.
   The per-model USD rates in the catalog and documentation are list prices used
   to compute `list_cost`/`allowance`; they are not, by themselves, an invoice.

### 2.3 What Factory measures — and what it cannot

- The Lead and semantic Reviewer calls use Core's `model_gateway`. Core reserves
  a configured-rate estimate, then settles the measured token counts at the
  configured input/output rates (`inputMicrosPerMillion` /
  `outputMicrosPerMillion`). This is a deterministic **configured-rate
  equivalent**, not the provider invoice.
- Core's gateway parses `usage.prompt_tokens`/`completion_tokens`/`total_tokens`
  and a `usage.cost_micros` field, but it does **not** persist
  `usage.cost`, `opencode.billing_source` or `opencode.allowance`.
- The Implementer provider session records the OpenCode message `info.cost`.
  Locally observed Go messages have a nonzero `cost` computed from list rates
  (including cached reads), so this is OpenCode's local estimate, **not** the
  raw API `usage.cost`, not the Go allowance consumed and not a balance charge.
  The Worker/Core do not capture `opencode.allowance` or
  `opencode.billing_source` for that session.
- Consequently the run can record:
  - measured tokens and the Core configured-rate equivalent;
  - the OpenCode message-cost estimate (nonzero even inside the Go allowance);
  - provider/model/session IDs and cleanup;
  - but **not** an independently verified Go allowance consumption, balance
    charge or invoice.
- The authoritative remaining allowance/balance is the OpenCode Console. No
  local CLI or Factory endpoint exposes it. `opencode stats` reports a local
  usage view (today's Go model row shows `cost: 0` while its messages carry a
  list-rate estimate) and is not the Console account ledger.

**Unknown until the Owner reads the Console:** current plan, remaining monthly
allowance for DeepSeek V4.1 Flash, current 5-hour/weekly usage, Extra Usage state,
Console balance, taxes/currency conversion and the final invoice. These are
recorded as `UNKNOWN`, never as `$0`.

The preparer did not sign in to the Console and did not read
`~/.local/share/opencode/auth.json` or any credential store. No current balance
or allowance is claimed here.

### 2.4 Off-peak observation

At preparation time (2026-10-29 14:54 UTC) DeepSeek V4.1 Flash was Off-Peak, so
the list rates were `$0.15/M` input and `$0.60/M` output. Peak rates are double.
The run must record the actual UTC start/finish time and whether the requests
fell in a peak window. Do not assume Off-Peak.

Core's envelope uses the **configured** input/output rates, not a live price
lookup. Configure the Peak rates when a run may cross a peak boundary; otherwise
the USD cap understates the allowance consumed at the provider.

### 2.5 Factory real-provider paths inspected

| Path | Role in this runbook |
|---|---|
| `native/src/bin/provider_probe.rs` + `runtime::probe_opencode` | Read-only provider/version discovery. Not used as acceptance evidence and not a paid call. |
| `native/src/bin/provider_smoke.rs` | Separate default-closed one-turn smoke (`FACTORY_LIVE_PROVIDER_SMOKE=1`). It proves bounded protocol compatibility only; it does not run a Campaign. |
| `native/src/bin/provider_bridge.rs` | Unix-socket to loopback-TCP bridge used by the confined provider namespace. |
| `native/src/model_gateway.rs` | Core-owned Lead, semantic Reviewer and evaluation calls. Reserves a configured-rate envelope before dispatch and settles measured usage after the response. The harness relies on this for the Lead and Reviewer. |
| `native/src/provider_sandbox.rs` | Confined provider process/container and loopback credential broker. The Worker uses the container form; the long-lived credential stays in the Worker egress broker. |
| `compose.yaml` worker wiring | `FACTORY_PROVIDER_KIND=opencode`, `FACTORY_PROVIDER_SANDBOX=container`, `/provider-sessions`, runtime broker. The Worker fetches `/worker/provider_profile` for the exact leased attempt and does not receive a fixture fallback. |
| `scripts/compose-e2e.sh`, `scripts/compose-browser-uat.sh`, `scripts/compose-provider-smoke.mjs` | Existing deterministic fixture and browser harnesses. They do not exercise a real model. The provider smoke is a separate credential/catalog test. |
| `scripts/real-provider-uat.{sh,mjs}` | The gated real-provider Campaign harness in this runbook. |

The credential flow is: Owner → Office Settings → encrypted Core storage →
Worker `provider_profile` request for the exact leased attempt → Worker egress
broker → provider container via a per-run capability. The harness never receives
the credential and cannot read it from the browser or evidence.

## 3. Preconditions

1. The Owner has an OpenCode Go plan (or explicitly chooses a billable API
   balance route), a valid credential, and Console access.
2. The Owner confirms the actual funding mode and Extra Usage state in
   <https://opencode.ai/console> (Go page and usage page).
3. The Owner either confirms Extra Usage is **off**, or accepts balance fallback
   and sets a cap below.
4. The Owner enters the OpenCode Go API key **directly into Office Settings**.
   The key is never passed to the runbook scripts, never printed and never
   committed. The scripts only use the local Core operator token.
5. A disposable Compose project whose name contains `uat` is used. The permanent
   `software-factory` stack is not touched.
6. No GitHub repository is bound to the disposable Project, and no GitHub PR is
   created, merged or commented on.

## 4. Exact Owner decision required

The Owner must record all of the following in writing before the first paid call:

| Decision | Required value |
|---|---|
| Billing route | `GO_SUBSCRIPTION_INCLUDED` (Go/Go Plus/Go Teams allowance) **or** `GO_EXTRA_USAGE_BILLABLE` (allowance plus balance fallback) **or** `PAY_AS_YOU_GO_API` (separate Console balance) |
| Extra Usage | `GO_SUBSCRIPTION_INCLUDED` requires `DISABLED`; `GO_EXTRA_USAGE_BILLABLE` requires `ENABLED_WITHIN_CAP`; `PAY_AS_YOU_GO_API` requires `NOT_APPLICABLE` |
| Plan / workspace | Go, Go Plus or Go Teams; workspace name; member or personal workspace |
| Remaining allowance or balance | Owner-read Console value, with timestamp |
| Spend cap | A USD cap for this run, `> 0` and `<= 5.00`; the runbook recommends `2.00` for Go allowance and `1.00` for balance fallback |
| Authorization id | A unique id, for example `owner-rp-uat-2026-10-30` |
| Route | `opencode-go` / `deepseek-v4.1-flash`; no fallback provider or model |
| Sessions / turns | At most 1 Implementer provider session and 4 turns |
| Requests / tokens | At most 1 request envelope; at most 60,000 input and 12,000 output tokens reported by Core |
| Deadline | At most 90 minutes from the start of the authorized run |
| Retries | None; an infrastructure failure is repaired before any new authorization |
| GitHub | No PR creation, merge, comment or review |
| Plan approval | The Owner personally reviews the exact frozen plan hash before `execute` |

The **cap is a single monetary envelope** for the request. Core reserves it
before the first billable assessment and refuses to exceed it. It is not a
per-role or per-call allowance.

Decision tree:

1. Console shows a Go/Go Plus/Go Teams plan and **Extra Usage off** →
   `GO_SUBSCRIPTION_INCLUDED` + `DISABLED`.
2. Console shows a Go plan and **Extra Usage on** → either turn Extra Usage off
   and use case 1, or accept balance fallback and use
   `GO_EXTRA_USAGE_BILLABLE` + `ENABLED_WITHIN_CAP`.
3. No Go plan, or the Owner intends to spend the Console pay-as-you-go balance
   directly → `PAY_AS_YOU_GO_API` + `NOT_APPLICABLE`.

There is no "free" or "unlimited" option in this harness. A Go allowance is a
prepaid monthly dollar limit, not an absence of cost.

Recommended authorization text:

> I authorize one bounded real-provider UAT, id `owner-rp-uat-2026-10-30`, on
> `opencode-go/deepseek-v4.1-flash` under my OpenCode Go subscription. I confirm
> Extra Usage is off. The run may consume up to USD 2.00 of my DeepSeek V4.1
> Flash monthly allowance, at most one Implementer session, four turns, 60,000
> input and 12,000 output tokens, within 90 minutes. No retry, no fallback
> model, and no GitHub write. I will review the frozen plan hash before
> execution.

If the Owner instead authorizes balance fallback, replace the Extra Usage sentence
with the exact balance and cap and state that fallback is accepted within the cap.

## 5. Harness and gates

Artifacts:

- `scripts/real-provider-uat.sh` — disposable stack, credential readiness and
  driver invocation.
- `scripts/real-provider-uat.mjs` — authorization gate, preflight, assessment,
  approval/execution and redacted evidence.
- `compose.real-provider-uat.yaml` — test-only Git fixture and real-provider
  opt-in; no model fixture.
- `scripts/real-provider-uat.test.mjs` — contract tests that refuse live mode
  without the flag and prove secrets are not logged.

Run the contract tests with:

```sh
node --test scripts/real-provider-uat.test.mjs
```

They use a local fake Core. They do not contact OpenCode, a real Core or any
paid provider.

The driver refuses to make a paid call unless **all** of the following are true:

- `FACTORY_REAL_PROVIDER_UAT=1` (live mode);
- `FACTORY_REAL_PROVIDER_AUTHORIZATION` is a bounded non-empty id;
- `FACTORY_REAL_PROVIDER_ACCOUNTING` and `FACTORY_REAL_PROVIDER_EXTRA_USAGE` are
  explicit, consistent values;
- `FACTORY_REAL_PROVIDER_SPEND_CAP_USD` parses exactly to a positive amount no
  greater than `5.00`;
- the Core URL is HTTPS or loopback HTTP, with no embedded credentials;
- the operator token is supplied by a `0600` file and is at least 32 bytes, or
  by a non-empty environment variable;
- the provider connection is configured, enabled, default, and its catalog is
  available with the exact model;
- the provider connection's `maxTurns` does not exceed the authorized turn
  ceiling (default 4);
- semantic review is enabled and a model gateway route exists;
- at least one Worker is registered.

Token ceilings default to 60,000 input and 12,000 output for this runbook and can
be tightened with `FACTORY_REAL_PROVIDER_MAX_INPUT_TOKENS` /
`FACTORY_REAL_PROVIDER_MAX_OUTPUT_TOKENS`. The provider turn ceiling defaults to
4 and can be tightened with `FACTORY_REAL_PROVIDER_MAX_TURNS`. They are
reconciled from the measured Core/provider records after the run; the monetary
cap is the pre-request enforcement.

It also refuses to approve a plan unless
`FACTORY_REAL_PROVIDER_PLAN_APPROVAL=OWNER_REVIEWED` and the plan hash passed on
the command line exactly matches the frozen plan.

The harness authenticates to Core with the local operator token read from
`/config/operator.token` into a `0600` file and removed on exit. It never prints
the token and does not reuse the Owner's browser session. The approval is
audited as `operator-token` with the label `owner-real-provider-uat`; the frozen
plan hash and rationale are recorded in the evidence.

## 6. Commands

### 6.1 Prepare the disposable stack

```sh
./scripts/real-provider-uat.sh prepare
```

The command prints the Office URL and a one-time enrollment code. The Owner:

1. Opens the printed Office URL and enrolls the device.
2. Opens **Settings → OpenCode Go**, enters the API key directly, enables the
   connection, selects **Default route for Core model calls**, and saves.
   Set the input/output rates to the current DeepSeek V4.1 Flash window (Off-Peak
   `$0.15/$0.60` per million, Peak `$0.30/$1.20` per million). If the run may
   cross a peak boundary, use the **Peak** rates so Core's envelope is
   conservative. Record the configured rates in the evidence.
3. Clicks **Test connection**. This reads the public catalog only; it performs
   no inference. It must show `AVAILABLE` and include `deepseek-v4.1-flash`.
4. Enables semantic review under **Settings → Runtime** if it is not already on.
5. Confirms the Console accounting facts in §4.

The `prepare` command does not start a paid call. It may be run before the
authorization exists. The stack remains disposable until `down`.

After enrollment and provider configuration, a read-only preflight can validate
the gate without the live flag:

```sh
export FACTORY_REAL_PROVIDER_DRY_RUN=1
export FACTORY_REAL_PROVIDER_AUTHORIZATION=owner-rp-uat-2026-10-30
export FACTORY_REAL_PROVIDER_ACCOUNTING=GO_SUBSCRIPTION_INCLUDED
export FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED
export FACTORY_REAL_PROVIDER_SPEND_CAP_USD=2.00
./scripts/real-provider-uat.sh preflight
```

Preflight performs only Core and catalog `GET`s. It does not create a Project,
Request, Campaign or provider session, and it does not prove inference access.
Remove `FACTORY_REAL_PROVIDER_DRY_RUN` before a live phase.

### 6.2 Phase 1 — real MODEL assessment

Set the Owner-authorized values (example values are illustrative, not
authorization):

```sh
export FACTORY_REAL_PROVIDER_UAT=1
export FACTORY_REAL_PROVIDER_AUTHORIZATION=owner-rp-uat-2026-10-30
export FACTORY_REAL_PROVIDER_ACCOUNTING=GO_SUBSCRIPTION_INCLUDED
export FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED
export FACTORY_REAL_PROVIDER_SPEND_CAP_USD=2.00
export FACTORY_REAL_PROVIDER_OBJECTIVE='Fix apply_discount in src/pricing.rs so the discount is applied exactly once, and add tests for zero, fractional, full and maximum-subtotal discounts. Do not change the public API.'
```

Then:

```sh
./scripts/real-provider-uat.sh assess
```

This creates the Project, execution profile, request envelope and Request, then
runs the real Lead assessment. It stops at `AWAITING_APPROVAL`, writes
`plan.json` and `state.json` under the evidence directory, and prints the exact
plan hash. It does not approve or execute the plan. The Lead call is paid usage
and counts against the envelope.

Do not delete or move the evidence directory between phases. The state file is
the guard against a second paid request under the same authorization. If it is
lost, stop and request a new authorization id rather than re-running blindly.

The Owner must read `plan.json` and confirm:

- the objective and allowed paths are the intended small change;
- the frozen acceptance and negative checks are executable;
- the route is `opencode-go/deepseek-v4.1-flash` with no fallback;
- the budget and session/turn limits are within §4.

### 6.3 Phase 2 — execute the exact approved plan

Only after reviewing the plan:

```sh
FACTORY_REAL_PROVIDER_PLAN_APPROVAL=OWNER_REVIEWED \
  ./scripts/real-provider-uat.sh execute --plan-hash <exact-plan-hash>
```

The driver refuses a mismatched hash. It approves the frozen plan, starts the
Campaign, waits for the real Implementer, the independent verifier, the MODEL
Reviewer and the deterministic Gate, and writes
`real-provider-uat-evidence.json`. It never creates a GitHub PR; it inspects the
Project's GitHub binding and records `UNBOUND_READY` when no repository is bound.

The assessment phase also rejects a frozen plan that declares more than one
provider session or any repair cycle; it does not silently narrow a model plan.
The Owner must reject such a plan and request a new authorization rather than
editing the state file.

### 6.4 Evidence and cleanup

```sh
# Keep the evidence before destroying the stack.
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
- exhausted envelope, unknown or ambiguous model-call usage, or an unresolved
  provider turn;
- missing credential, rejected catalog, provider/model mismatch, disabled
  semantic review, or no Worker;
- timeout, transport ambiguity, or unconfirmed provider cleanup;
- any request to create or merge a GitHub PR.

If the run stops with an unresolved provider session, record it as unresolved;
do not silently treat it as zero cost. Repair the infrastructure failure, then
request a **new** authorization id. The state file refuses to reuse one
authorization id for a second request.

## 8. Evidence to record

- Candidate wrapper commit and exact stack Compose files/project name.
- Authorization id, funding mode, Extra Usage state, cap and UTC start/finish.
- Provider and model, Core model route, and evidence that the catalog was
  `AVAILABLE` (catalog availability does not prove inference).
- Project, request, Action, Campaign, Run, Attempt and candidate identities.
- Frozen plan hash and the Owner's approval rationale.
- Implementer provider session record ID, OpenCode session ID, model, turns,
  terminal state, measured tokens, OpenCode message-cost estimate (labelled as
  a local estimate, not a charge) and cleanup receipt.
- Lead and Reviewer model-call reservations, roles, measured tokens and
  configured-rate cost.
- Independent verifier receipt (result/evidence/commit/tree) and Reviewer
  receipt (result/mechanism/provider/model/usage).
- Quality Gate outcome and reasons.
- Campaign inference ledger status and any ambiguous/reserved/overrun amounts.
- Candidate commit/tree, changed paths and diff summary.
- GitHub binding state and `PR_WRITE: NONE`.
- Console-reported allowance/balance observed by the Owner, marked `UNKNOWN` if
  not read.
- Cleanup receipts and any surviving container, volume or provider session.

## 9. Non-claims

- No live result is claimed by this preparation. The dry-run contract test uses
  a local fake Core and performs no inference.
- No ISOLATED, SEC-01, credential-confinement, production, Windows, human
  security-review or GitHub delivery claim.
- A subscription allowance is not "free"; the run may consume paid allowance.
- Configured-rate micros and OpenCode message-cost estimates are not invoices.
- The run cannot prove the Go allowance consumed or a balance charge unless the
  Owner separately reads the Console.
- Author-run UAT is not independent acceptance. The independent verifier and
  Reviewer are independent *roles*, not independent acceptance of this runbook.
- The provider credential remains in Core/Worker trust; `TRUSTED_LOCAL` is not
  isolated.

### Office impact: none

This change adds operator tooling and documentation only. It adds no Office
route, control, projection or user-facing capability. The live run uses the
existing Office Settings, Runtime, Request and Campaign surfaces, which are
already mapped in [the Office parity matrix](../product/office-parity.json).
The harness calls only existing authenticated Core operations. Therefore the
parity matrix is unchanged and the impact is explicitly **none**; the required
capability remains #60's real-provider acceptance, which stays `NOT_TESTED`
until the Owner authorizes and executes it.

## 10. Open accounting follow-ups

1. Persist `opencode.billing_source`, `usage.cost` and
   `opencode.allowance` from provider responses so Go allowance is measurable
   rather than unknown.
2. Add a Console/account usage read path or an explicit owner-attested usage
   record to the evidence schema.
3. Verify the actual peak/off-peak window at request time and record it.
4. Reconcile OpenCode's local message `info.cost` (list-rate arithmetic
   including cache reads) against the raw inference API `usage.cost`,
   `billing_source` and `opencode.allowance` for both Go and pay-as-you-go
   routes; do not label the local estimate as a charge.
