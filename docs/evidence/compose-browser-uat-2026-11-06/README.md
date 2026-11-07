# Compose browser UAT — Office command-center standard

**Result:** `PASS` — deterministic-fixture UAT, **not** live-model or independent
acceptance.
**Candidate:** `bd90e990d31f7110c368b1253da813f729a729e6`
**Date:** 2026-11-07
**Record:** [`compose-browser-uat.json`](compose-browser-uat.json)

This run exercised the real rootless Compose stack, the real Rust Core HTTP API
and real Chromium without intercepting `/core-api`. The Worker used the
deterministic provider fixture and the bounded model fixture; no paid inference
was requested and no GitHub pull request was created or merged.

## Command

```sh
FACTORY_UAT_EVIDENCE_DIR="$PWD/docs/evidence/compose-browser-uat-2026-11-06" \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
  ./scripts/compose-browser-uat.sh
```

The harness admits only its disposable test scopes in the debug-only `core-e2e`
image. The production image does not contain the test-support endpoint.

## Passed journey

- first-run Owner enrollment and Project import/configuration;
- provider/profile setup and browser-created intake request;
- frozen Lead plan display and explicit UI approval;
- Worker claim, fixture candidate, verifier/reviewer receipts and
  `ACCEPTED` Quality Gate;
- candidate lineage/diff and Agent Office participant projection;
- Home, Request workspace, Campaign overview, Budget & usage, Candidates,
  Evidence and Gate at 1440 and 390 px;
- campaign pulse and top-bar Project/Quality context populated from real Core
  projections after navigation;
- no unexpected console errors, page errors or failed requests.

The JSON records expected `404` polls for the candidate Gate before the decision
exists; those are explicit pending-state probes, not failures.

## Boundaries

- `live_provider_inference`: `NOT_TESTED`
- `independent_acceptance`: `NOT_TESTED`
- `isolation`: `NOT_CLAIMED`
- GitHub PR projection: `UNBOUND_NOT_TESTED` (no remote write)
- Product Owner visual sign-off remains separate recording.

This is author-run UAT evidence for the UI standard and the deterministic
Compose path. It does not certify live inference, security isolation, deployment
or independent acceptance.
