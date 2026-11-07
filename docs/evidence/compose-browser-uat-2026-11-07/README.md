# Compose browser UAT — telemetry/autonomy cleanup

**Result:** `PASS` — deterministic-fixture UAT, **not** live-model or independent
acceptance.
**Candidate:** `70ddab2bb3f432b633fc66eff52dfab0ad3e0e5f`
**Date:** 2026-11-07
**Record:** [`compose-browser-uat.json`](compose-browser-uat.json)

This run exercised the real rootless Compose stack, the real Rust Core HTTP API
and real Chromium without intercepting `/core-api`. The Worker used the
deterministic provider/model fixture; no paid inference was requested and no
GitHub pull request was created or merged.

## Command

```sh
FACTORY_UAT_EVIDENCE_DIR="$PWD/docs/evidence/compose-browser-uat-2026-11-07" \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
  ./scripts/compose-browser-uat.sh
```

The harness admits only its disposable test scopes in the debug-only `core-e2e`
image. The production image does not contain the test-support endpoint.

## Passed journey

- first-run Owner enrollment, Project import/configuration and provider/profile setup;
- browser-created intake request, frozen plan and explicit UI approval;
- Worker claim, deterministic fixture candidate, verifier/reviewer receipts and
  `ACCEPTED` Quality Gate;
- candidate lineage/diff and Agent Office participant projection;
- Home, Request workspace, Campaign overview, Budget & usage, Candidates,
  Evidence and Gate at 1440 and 390 px;
- no unexpected console errors, page errors or failed requests.

The JSON records expected `404` polls for a candidate Gate before the decision
exists; these are explicit pending-state probes, not failures.

## What this run validates

- The active subscription/legacy execution path reaches the deterministic Gate
  after fixed model token/time/request/retry/provider-cost caps and their active
  Office controls were removed (ADR-0034).
- The top-bar Project/Quality context is populated from real Core projections.
- The Office Campaign Budget view shows observed usage/progress rather than
  fixed model productivity limits.

## Boundaries

- `live_provider_inference`: `NOT_TESTED`
- `independent_acceptance`: `NOT_TESTED`
- `isolation`: `NOT_CLAIMED`
- GitHub PR projection: `UNBOUND_NOT_TESTED` (no remote write)
- Product Owner visual sign-off remains separate recording.

This is author-run UAT evidence for the deterministic Compose path. It does not
certify live inference, security isolation, deployment or independent
acceptance.
