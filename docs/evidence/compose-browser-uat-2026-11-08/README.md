# Compose browser UAT — truthful Office status and simplified operator surface

**Result:** `PASS` — deterministic-fixture UAT, **not** live-model or independent
acceptance.
**Candidate:** `f5e99da820b04efbd4768eac2baf4d49fba7736d`
**Date:** 2026-11-08
**Record:** [`compose-browser-uat.json`](compose-browser-uat.json)

This run exercised the real rootless Compose stack, the real Rust Core HTTP API
and real Chromium without intercepting `/core-api`. The Worker used the
deterministic provider/model fixture; no paid inference was requested and no
GitHub pull request was created or merged.

## Command

```sh
FACTORY_UAT_EVIDENCE_DIR="$PWD/docs/evidence/compose-browser-uat-2026-11-08" \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
  ./scripts/compose-browser-uat.sh
```

The harness admits only its disposable test scopes in the debug-only `core-e2e`
image. The production image does not contain the test-support endpoint.

## Passed journey

- first-run Owner enrollment, Project import/configuration and provider/profile setup;
- the Projects managed-folder browser against the real `/projects` volume;
- a subscription Owner admission created through the real Core endpoint, followed
  by a browser-created intake request, frozen plan and explicit UI approval;
- Worker claim, deterministic fixture candidate, verifier/reviewer receipts and
  `ACCEPTED` Quality Gate;
- candidate lineage/diff and Agent Office participant projection;
- Home, Request workspace, Campaign overview, Budget & usage, Candidates,
  Evidence and Gate at 1440 and 390 px;
- no unexpected console errors, page errors or failed requests.

The JSON records expected `404` polls for a candidate Gate before the decision
exists; these are explicit pending-state probes, not failures.

## What this run validates

- Home creates a request without a per-request USD inference authorization on the
  subscription route.
- The approval path consumes an exact Owner-confirmed subscription admission that
  is already bound to the request.
- The previously broken Home status and Decisions projection no longer fail on
  historical Actions without frozen specifications.
- Activity & telemetry renders tokens and session state rather than a monetary
  reserved ceiling.
- The Projects folder browser lists only directories inside the managed root and
  fills the selected local import path.

## What this run does not validate

- Live provider inference, independent acceptance, deployment, Windows, GitHub
  write operations or SEC-01 isolation.
- The owner-facing creation of the subscription admission is API-assisted in this
  candidate; the audit records the missing UI as a follow-up.
- `TRUSTED_LOCAL` remains broad local access and is never described as isolated
  or sandboxed.
