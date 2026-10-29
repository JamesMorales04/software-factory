# Real Compose + Chromium product UAT — 2026-10-29

**Result: `PASS_WITH_FINDINGS` — real product journey passed; UAT acceptance is
`NOT_CLAIMED`.** Two Office contract defects were found and are reported below.
This evidence is **author-run**, not independent acceptance, and the model
gateway plus the coding provider are deterministic fixtures. No paid inference
was requested and no GitHub pull request was created or merged.

## Candidate and environment

| Item | Value |
|---|---|
| Factory candidate | `086a63f730f0a3db25c178b7dbcac2951d2db7b4` |
| Branch | `factory/compose-browser-uat/20261029` |
| Runtime path | rootless `podman compose` (`compose.yaml` + `compose.e2e.yaml`) |
| Compose project | `software-factory-uat60` (disposable) |
| Published Office URL | `http://127.0.0.1:8898` |
| Podman / Compose provider | `podman 6.1.4` rootless / `podman-compose 1.6.0` |
| Chromium | Chrome for Testing `156.0.8078.4` (`chromium-1248`) |
| Browser driver | `playwright-core 1.64.1` |
| Application model fixture | `http://model-fixture:8080/v1` (deterministic) |
| Worker provider | `FACTORY_PROVIDER_KIND=fixture` (deterministic adapter) |
| Paid inference | **none** |
| `/core-api` interception | **none** |

The host build succeeded. The previously suspected buildah/btrfs limitation did
not reproduce: rootless Podman on this btrfs host built all product images and
started every Compose service, including the nested runtime engine. Therefore
the real run is reported, not a CI-only fallback.

### Base product start (no test override)

The unmodified product topology was also started independently on
`http://127.0.0.1:8899` with:

```sh
podman compose -p software-factory-uat60-base -f compose.yaml up -d --build
```

Core, Supervisor, the runtime broker and Office became healthy; the Worker
registered (`compose-worker`); `FACTORY_PROVIDER_KIND` resolved to the
fail-closed live route `opencode` in both Worker and runtime. No provider
credential was configured, no request was created and no inference or execution
was performed. See `base-product-start.json`. This proves product startup on
this host; it does **not** certify live inference.

## Exact command

```sh
PODMAN_COMPOSE_PROVIDER=/usr/bin/podman-compose \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
FACTORY_UAT_KEEP=1 \
./scripts/compose-browser-uat.sh
```

Exit code `0`. The script cleans disposable volumes unless `FACTORY_UAT_KEEP=1`.
It issues the one-time owner confirmation inside the Core container and never
prints it.

The equivalent split commands are:

```sh
podman compose -p software-factory-uat60 \
  -f compose.yaml -f compose.e2e.yaml up -d --build
podman exec software-factory-uat60_core_1 \
  /usr/local/bin/factory-core issue-bootstrap-code
FACTORY_OFFICE_URL=http://127.0.0.1:8898 \
FACTORY_BOOTSTRAP_CODE=<one-time-code> \
CHROMIUM_EXE=<chromium> \
FACTORY_EVIDENCE_DIR="$PWD/docs/evidence/compose-browser-uat-2026-10-29" \
FACTORY_MODEL_CONTEXT=<run-root>/model/context.json \
node scripts/compose-product-browser-uat.mjs
```

## What is real and what is fixture

| Layer | Evidence level |
|---|---|
| Rootless Compose topology, images, volumes, loopback publication | **REAL**, this host |
| Rust Core, SQLite, Office static application | **REAL**, this host |
| Supervisor, Worker, runtime broker, nested V2/VFS engine | **REAL**, container logs and health |
| Git fixture repository (`git-fixture`) imported through Office | **REAL fixture repository**, test service |
| First-run owner enrollment and HttpOnly device session | **REAL** browser → Core |
| Home request composer with explicit USD amount | **REAL** browser → Core |
| Lead assessment model calls | **DETERMINISTIC MODEL FIXTURE** (`opencode-go` → `model-fixture`) |
| Worker implementation | **DETERMINISTIC FIXTURE ADAPTER** (`FACTORY_PROVIDER_KIND=fixture`) |
| Independent verifier | **REAL verifier process** executing frozen checks in the nested engine |
| Independent semantic reviewer | **REAL reviewer process**, model call served by the deterministic fixture |
| Quality Gate | **REAL deterministic Core decision** |
| Chromium UI and screenshots | **REAL Chromium** against the real Office URL |
| GitHub PR open/merge | **NOT TESTED** — no binding, no write attempted |
| Live OpenCode/Codex inference | **NOT TESTED** — no spend authorization |
| Independent verification/reviewer acceptance | **NOT TESTED** |
| SEC-01 isolation | **NOT CLAIMED** (`isolation_certified: false`) |

The Campaign authorization records `executionMode: TRUSTED_LOCAL_PROVIDER` from
the frozen V2 execution profile. The Worker was deliberately started with the
test-only `FACTORY_PROVIDER_KIND=fixture` override, and the recorded provider
session `ses_fixture` proves the deterministic adapter actually executed. This
difference between the authorization route and the adapters used is a test
override boundary, not a live-provider claim.

## Journey result and exact identities

| Fact | Value |
|---|---|
| Project | `459ee709-8c56-448e-a350-41742bc8634c` |
| Intake request | `9ddd7b88-e9a7-4b4f-bd16-e3a31e15952f` |
| Action | `be6bb2dc-fcfd-4d76-be5f-7127fbd5be9d` |
| Lead request | `9f80ad6d-5c67-4b50-bbb6-be044ad85f31` |
| Execution profile | `0b8510de-964e-4962-9c61-d33be2130458` |
| Frozen plan hash | `2bc112b6ccc260d1f8550bcde8c043b0f583385fab86e4553585cdb3c552f937` |
| Campaign | `3f5f9abe-bcbe-4e0c-8ac9-6a0227e6ca3f` |
| Run | `692e45b5-c802-401f-b809-4b14fac2b7a7` |
| Attempt | `7a9c6699-5cb1-4db4-9734-4b96cefb60ca` |
| Candidate | `0ee276a1-0a65-46d6-ae10-a8fa4a311cc4` |
| Candidate commit | `42c4b47aee07a37b48649334ff7eb15afe0ccec4` |
| Changed path | `src/provider.txt` (`ADDED`, +1) |
| Frozen base | `5b1e23e193ea8652e9005c143626baf3dd2759f2` |
| Implementer | `compose-worker`, generation `1`, session `ses_fixture`, `COMPLETED` |
| Cleanup receipt | `session_delete_verified_absent` |
| Verifier receipt | `2322b463-9c23-48aa-9c8f-8ee1df4242c4` (`PASS`, `EXECUTED`, `DETERMINISTIC`) |
| Reviewer receipt | `def99d99-3c5a-415d-be0f-2c6cc0445129` (`PASS`, `EXECUTED`, `MODEL`) |
| Quality Gate | `ACCEPTED` — “independent executed evidence accepts exact candidate” |
| Operator spend authorization | `5,000,000` micros (USD 5.00) at request creation |
| Agent Office participants | 4: Lead, Implementer, Verifier, Reviewer |
| PR projection | `UNBOUND_NO_GITHUB_WRITE` |
| Restart persistence | **NOT TESTED in this run** |

The Lead model fixture settled 500 input / 500 output tokens (2 micros) and the
semantic reviewer 300 input / 100 output (2 micros). Both are fake-gateway
measurements inside the 5,000,000-micros request envelope; they are not paid
provider spend. The implementer session usage (40 input / 80 output, 400
micros) is deterministic fixture usage, not live inference.

The Core V2 spending ledger confirms the boundary: authorized `5,000,000`
micros, settled `4` micros total, available `4,999,996`, with `IMPLEMENTER`
settled at `0`. The only billable-route reservations are the Lead assessment
and the semantic reviewer, both served by the deterministic model fixture.

## Screenshots (1440 and 390)

All files are in this directory. Every capture is the real Office served by the
real Compose Core; none is an intercepted fixture response.

| Screen | 1440 | 390 |
|---|---|---|
| Home, immediately after first-run enrollment | `home-first-run-1440.png` | — |
| Home, request composer with USD 5.00 | `home-1440.png` | `home-390.png` |
| Request workspace, created | `request-created-1440.png` | — |
| Request workspace, assessment + frozen plan | `request-assessment-1440.png` | `request-assessment-390.png` |
| Request workspace, completed + Gate | `request-completed-1440.png` | `request-completed-390.png` |
| Campaign overview | `campaign-overview-1440.png` | `campaign-overview-390.png` |
| Candidate diff + unbound PR panel | `campaign-candidates-1440.png` | `campaign-candidates-390.png` |
| Verifier/reviewer evidence | `campaign-evidence-1440.png` | `campaign-evidence-390.png` |
| Quality Gate | `campaign-gate-1440.png` | `campaign-gate-390.png` |
| Agent Office, campaign-filtered | `agent-office-1440.png` | `agent-office-390.png` |
| Home after completion | `home-completed-1440.png` | `home-completed-390.png` |
| Budget probe (finding) | `campaign-budget-1440.png` (blank) | `campaign-budget-390.png` (blank) |

The 390 px captures have no document-level horizontal overflow. The Campaign
tab strip itself scrolls horizontally, as designed.

## Findings — reported, not fixed here

Both findings are outside this UAT workstream’s allowed scope. The
execution-policy/Office budget lane owns the V2 projection contract.

### F1 — Request workspace prints `Authorization undefined · undefined`

`request-completed-1440.png` and `request-completed-390.png` show the Campaign
panel with `Authorization undefined · undefined`. The Core projection returns
`riskClass` and `executionMode` (camelCase V2), while
`office/src/coreApi.ts` declares the legacy snake_case fields
`risk_class`/`execution_mode`. The operator cannot read the authorization class
or route from the completed request.

### F2 — Campaign “Budget & usage” tab crashes the Office view

Navigating to `?tab=budget` raises:

```text
TypeError: Cannot read properties of undefined (reading 'join')
```

and unmounts the application (`campaign-budget-*.png` are blank). The tab reads
legacy fields such as `authorization.authorized_paths`, `authorization.max_attempts`
and `authorization.max_sessions`; Core V2 returns `authorizedPaths` and a
`policy.operations`/`policy.progress` object. It also means the authoritative V2
limits are not visible to the operator.

The required journey screens (request, candidate diff, evidence, Gate, Agent
Office) were captured before the Budget probe, so the findings do not invalidate
those captures. They do invalidate any claim of complete Office completeness.

## Console and network classification

- 6 `404` responses are the harness’s pending Gate polls
  (`/quality/candidates/<id>/gate`) before Core records the Gate. They are the
  truthful pending state and are classified as `expected_gate_polls`.
- 6 generic browser `404` console messages correspond exactly to those polls.
- No unexpected `404`, no failed requests, no unexpected console errors.
- One page error comes from the Budget probe (`reading 'join'`), recorded as F2.
- `stack-health.json` records Core, Supervisor, runtime broker, Worker and model
  fixture health from the same run.
- `runtime-broker-excerpt.txt` records the real nested provider container and
  five child executions; the final `exit_code=Some(1)` is the expected negative
  check result that the verifier interprets as a PASS.

## Machine-readable result

[`compose-browser-uat.json`](compose-browser-uat.json) contains the request,
action, plan, campaign, attempt, candidate, receipt and Gate identities, the
assertion map, the two findings, the screenshot list, the browser console
classification, the exact commands and the honest evidence boundary.

Supporting records from the same run:

- [`core-projections.json`](core-projections.json) — exact Core projections for
  the request (including the V2 spending ledger), Campaign, Gate, receipts,
  participants, model calls and candidate diff.
- [`stack-health.json`](stack-health.json) — Core, Supervisor, runtime broker,
  Worker registration and model-fixture health.
- [`runtime-broker-excerpt.txt`](runtime-broker-excerpt.txt) — real nested
  provider container and child executions, including the expected negative
  check exit 1.
- [`base-product-start.json`](base-product-start.json) — the separately started
  unmodified product topology, health checks and fail-closed provider kinds.

## Unverified / explicitly not claimed

- Live OpenCode/Codex inference, live model quality or pricing.
- Live GitHub pull-request open, comment, review, merge or branch protection.
- A GitHub-bound PR projection; this Project has no binding and no write was
  attempted.
- Independent verifier/reviewer acceptance outside the deterministic fixtures.
- SEC-01 isolation, credential confinement or production hardening.
- Core restart persistence for this exact run (covered by the separate
  deterministic Compose E2E, not rerun here).
- Windows/macOS hosts and the Docker-Compose-through-rootless-socket provider.
