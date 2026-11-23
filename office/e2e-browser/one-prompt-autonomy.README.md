# One-prompt autonomy lane (harness)

Status: **HARNESS_IMPLEMENTED / NOT_RUN / NOT_ACCEPTANCE**.

This lane is owned by the harness implementer. It is not product code, not an
authority grant, not independent acceptance and not certified isolation. It
drives the real Office and real Core in the canonical rootless Compose
topology with the deterministic model fixture only (`paidInference:false`).

## What it proves

Positive lane:

1. One real Office prompt (`#home-objective`) with a discovered autonomous
   project/profile.
2. Exactly one real UI policy confirmation (`Enable autonomous R0 execution`).
3. No driver workflow POST/PUT: the page creates the intake and enables the
   policy; the driver reads only, except explicit configuration setup.
4. Core prepares the plan, participants, environment, spec and Campaign
   (`startReady` projection observed or Campaign created).
5. The deterministic Worker reaches a candidate and Quality Gate `ACCEPTED`
   with exact verifier/reviewer receipts and `src/provider.txt` lineage.
6. Core is restarted **only** in the dedicated project. The same request,
   action, lead, Campaign, run, attempt, candidate, receipts, plan/spec hashes,
   fixture counters and model reservations are observed after restart.
7. No new model-call reservation and no counter increment after restart.

Negative/edge lanes:

- `FACTORY_UAT_NEGATIVE=no-policy`: submit shows the policy dialog; dismiss
  leaves zero intakes, zero Campaigns and zero fixture counters.
- `FACTORY_UAT_RELOAD_PREPARATION=1`: reload after request navigation keeps the
  same request, no duplicate phase/model, and no routine manual controls.
- `FACTORY_UAT_NEGATIVE=license-header`: expects the Core98 fixture variant
  that chooses the header asset (`ed.h`) and must expose a typed blocker,
  no Campaign, no retry/fuzzy map. **Requires the fixture hook owned by Core98;
  do not claim this lane until it is integrated.**
- `FACTORY_UAT_NEGATIVE=funding-false`: reads
  `/funding/subscription/opencode-go` and asserts the full truthful projection:
  `coreVerifiable:false`, `admissible:false`, `entitlement`, `quota` and
  `missingEvidence`; it also requires the Office display. Owned by b786. If the
  projection or UI is missing it is `BLOCKED`, never PASS.
- `FACTORY_UAT_NEGATIVE=lease-vs-request`: reads
  `/office/participants?campaign_id=...` while the request is executing and
  asserts `lease_state` and request `status` are separate facts. A
  `RUNNING` participant cannot silently show `RELEASED`/`EXPIRED`; a
  `COMPLETED` request cannot still show a `LIVE` lease. It does not mutate
  workflow state.
- `FACTORY_UAT_NEGATIVE=license-wait-owner`: one prompt reaches the licence
  gate without a pre-existing Owner licence delegation. It asserts zero
  `license_evidence_review` model calls, zero licence decisions, no environment
  freeze, no spec freeze and no Campaign. It requires the b786 `licenseAuthority`
  projection (`required`, `state` `MISSING/REVOKED/EXPIRED/MISMATCH`,
  `source:"OWNER_DECISION"`, delegation/pin/useContext/criterion/deadline) and
  UI selectors `home-licence-blocked`/`home-licence-authority-state`; if the
  product does not expose them yet, the lane records `BLOCKED`, never PASS.

**Positive busy-gate:** the positive and `license-header` lanes are `BLOCKED`
with `LICENSE_FIXTURE_PHASE_MISSING` until Core98 adds
`license_evidence_review` to the fixture `/counters` (and the v4 response). The
lane never downgrades that to PARTIAL/PASS. They are also `BLOCKED` with
`ENVIRONMENT_LICENSE_TRIGGER_MISSING` until the authorized builder/environment
ED-helper trigger exists.

**Licence authority is not the execution policy.** The Home policy
confirmation sends only `autonomous:true` and `subscription_confirmed:true`;
that is execution/funding, not licence authority. The exact
`LICENSE_IDENTITY_EVIDENCE_DELEGATION` (project, pin, use context,
`no-mandatory-fee-commercial-use`, revision, `exact-identity` scope) is a
separate `agentic_owner_decisions` row created/approved through the `/licenses`
UI. A fresh stack without a pre-existing approved delegation must produce a
truthful `WaitOwner`/`BLOCKED` around the licence gate and **no Campaign**. This
harness must never seed that owner decision, derive it from R0/subscription, or
fabricate a licence decision. A real positive one-prompt lane requires either
the product UI to include the explicit licence criterion in the same Owner
confirmation or a pre-existing authorized delegation; otherwise the positive
lane remains BLOCKED.

The proposed read contract is `licenseAuthority {required,state,delegationId,pin,
useContext,economicCriterion,deadline,revokedAt,source:"OWNER_DECISION"}` with
fail-closed `LICENSE_AUTHORITY_REQUIRED`; normal MISSING recovery uses the
existing `createAgenticOwnerDecision` + `resolveAgenticOwnerDecision` matter
`LICENSE_IDENTITY_EVIDENCE_DELEGATION`. The proposed UI selectors are
`home-policy-license-authority`, `home-licence-authority-state`,
`home-licence-pin`, `home-licence-use-context`, `home-licence-criterion`,
`home-licence-deadline`, `home-licence-confirm`, `home-licence-blocked`,
`home-licence-resume`, `licenses-delegation-create`,
`licenses-delegation-approve-{id}`, `licenses-delegation-approve-confirm`.
The harness does not invent a second confirmation; a dual execution+licence
record must be implemented by product and reviewed by 98/70 before this lane is
allowed to pass.

## Commands

From the isolated worktree at candidate `8d3fcf15` (no shared worktree):

```sh
# Constructive contract tests (no browser/Core/Compose)
node --test office/e2e-browser/one-prompt-autonomy.contract.test.mjs

# Static syntax checks (no build, no restart)
node --check office/e2e-browser/one-prompt-autonomy.mjs
node --check office/e2e-browser/one-prompt-autonomy-contract.mjs
bash -n scripts/compose-oneprompt-uat.sh
bash -n scripts/compose-browser-uat.sh

# Full isolated lane (Root window only; starts Compose and restarts Core)
FACTORY_UAT_LANE=oneprompt \
  FACTORY_UAT_PROJECT=software-factory-oneprompt-<stamp> \
  FACTORY_UAT_PORT=19091 \
  CHROMIUM_EXE=<pinned-chromium> \
  scripts/compose-browser-uat.sh
```

The same entrypoint is available directly:

```sh
FACTORY_UAT_PROJECT=software-factory-oneprompt-<stamp> \
FACTORY_UAT_PORT=19091 \
scripts/compose-oneprompt-uat.sh
```

`scripts/compose-browser-uat.sh` keeps its historical product lane; only
`FACTORY_UAT_LANE=oneprompt` dispatches here.

The final acceptance lane must use a **fresh** disposable stack. Do not use
`FACTORY_UAT_SKIP_START=1` or `FACTORY_UAT_ALLOW_ENROLLED=1` for the accepted
run; those exist only for diagnostics/resume and are not acceptance evidence.

## Isolation

- Project must contain `oneprompt`; `software-factory`,
  `software-factory-uat-9a2a6678`, `factory-acceptance` and the historical
  `software-factory-uat60` are refused.
- Ports `18893` and `8893` are refused; the script also refuses a port already
  answering `/health`.
- Own project, networks, volumes, Git fixture, model context, evidence and
  browser state. No shared stack, no `/tmp/opencode/repro-fix-91bf`, no host
  Podman socket, no host networking, no privileged mode.
- The browser runs in the pinned `software-factory-dev:local` image by default
  (`FACTORY_UAT_BROWSER_IMAGE` can select another pinned image). It uses
  `playwright-core` 1.64.1 and Chromium from the image, never host Node
  packages.
- The browser joins the dedicated project network and uses a verification-only
  loopback proxy to `http://core:8765` so Core sees the supported loopback
  Origin/Host shape. The proxy is not a second authority.
- Only Core is restarted (`podman restart <project>_core_1`); DB, volumes,
  model fixture, Worker and the rest of the stack stay up.
- Evidence defaults to `/tmp/opencode/oneprompt-uat-<stamp>`; browser state is
  kept under the run root, not evidence, and is deleted after verification.
  Do not publish cookies/storageState.

## Driver no-seeding contract

- Driver Core calls are recorded. Reads are unlimited.
- Driver setup mutations are allow-listed only:
  `/settings/providers/*`, `/settings/runtime`, `/execution-profiles`,
  `/execution-profiles/*/default`, `/projects/*/config`.
  These are configuration setup, not workflow state; they do not create the
  intake/Campaign. **Root must explicitly accept this allowlist or request the
  setup move to Office UI before freeze.**
- Driver workflow mutations are a hard failure: `/intake/requests`, `/assess`,
  `/plan`, `/participants`, `/environment*`, `/license*`, `/decision`,
  `/spec*`, `/approve`, `/start`, `/campaigns*`, `/runs*`,
  `/test-support/subscription-dispatch-admissions`.
- The page must own exactly one `POST /intake/requests` and exactly one
  `POST /projects/*/execution-policy`. No request ID is accepted from
  environment/deep link; it is discovered from the post-submit URL.
- One bootstrap enrollment at most. After Core restart the lane reuses the
  saved device storageState; if it is lost, the lane fails rather than
  enrolling a second time.

## Fixture hooks (Core98)

The lane expects but does not implement:

- `license_evidence_review` phase in `scripts/fixtures/compose-model-fixture.py`
  and its `/counters` entry.
- A deterministic positive response that accepts the exact licensing fact
  chosen by Core (`softwareTexts` = the exact selected fact, e.g. `ed.h` with
  its sha); Core derives the `COPYING` license-text asset.
- A negative variant for the `license-header` lane that produces a typed STOP
  (wrong role/sha), no Campaign and no retry. The harness writes
  `fixtureVariant:"license-header"` (positive: `"positive"`) in
  `/context/context.json`; the integrated fixture reads that key (or the
  `FACTORY_UAT_NEGATIVE` fallback).
- The fixture project/environment that actually exercises the licence review
  for the one prompt; no hard-coded family/suffix shortcut. The harness does
  not yet have the **environment trigger** that makes the real builder return
  `ENVIRONMENT_LICENSE_UNKNOWN` for the exact `ed` identity. Until Core98
  provides a bounded test-builder hook or exact flake, the positive lane stays
  `BLOCKED` even though the fixture phase exists.
- The harness requires `FACTORY_UAT_ED_HELPER=1` before the positive/licence
  lanes may proceed. Without that marker it returns
  `BLOCKED/ENVIRONMENT_LICENSE_TRIGGER_MISSING`; the marker must be wired to the
  actual Core98 builder hook, not only set in the browser container.

Expected counter contract:

- required exactly once: `assessment`, `planner`, `environment`, `spec`,
  `license_evidence_review`;
- `semantic_review` at least once (one per required reviewer role is allowed);
- `lead_stack_selection` at most once;
- `other` zero and `/errors` empty.

If the fixture emits different phase names, update this contract **with Core98**
and record the exact hook SHA. Do not rename product phases to fit the harness.

## Evidence labels

Every result file is labelled:

- `paidInference:false`
- `deterministic_fixture_model:true`
- `live_provider_inference:NOT_TESTED`
- `independent_acceptance:NOT_TESTED`
- `isolation:NOT_CLAIMED`
- `certified:false`
- `simulated:true`

A successful run is `PASS` only for this harness lane; it is **not** independent
acceptance. Until the Root window runs the integrated candidate, the browser
lane remains `NOT_TESTED`.

## Files owned by this lane

- `office/e2e-browser/one-prompt-autonomy.mjs` (new)
- `office/e2e-browser/one-prompt-autonomy-contract.mjs` (new)
- `office/e2e-browser/one-prompt-autonomy.contract.test.mjs` (new)
- `office/e2e-browser/one-prompt-autonomy.README.md` (new)
- `scripts/compose-oneprompt-uat.sh` (new)
- `scripts/compose-browser-uat.sh` (one dispatch hook only)

No edits to Core, `scripts/fixtures`, Office UI/`coreApi`, global parity matrix,
shared stacks or the historical UAT driver.

## Handoff

Publish the candidate SHA, changed files, `node --check`/contract-test logs,
the exact run command and any fixture-hook requirements to Core98/Root/70/de9.
No main merge, push or acceptance claim from this lane.
