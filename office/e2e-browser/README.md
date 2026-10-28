# Factory Office browser tests

The authoritative end-to-end journey is `rust-journey.mjs`, run by
`cargo xtask e2e office` and the Native Rust CI `office-browser` job. It starts
the real Rust Core, Supervisor, Worker and Vite Office in disposable state and
drives Chromium through Project registration, Action creation, Campaign
authorization, Run/Attempt dispatch, the independent verifier/reviewer and an
`ACCEPTED` Quality Gate, plus the Workers and Lead views and reload paths. It
also checks the pending reviewer projection before reviewer evidence exists,
the decoded Lead assessment/proposal/plan, a Lead-started Campaign WorkGraph,
the authorization budget projection and an explicit operator Campaign
cancellation with its recorded recovery state.

`harness.mjs`, `browser-close.test.mjs` and `launch-timeout.test.mjs` retain
generic bounded browser shutdown and launch-timeout fault coverage. They do not
contact Core or a provider.

```sh
npm ci
npm run test:close
npm run test:launch
```

The legacy API journeys and provider-specific views were retired with the former
implementation. Historical records are archived under
`docs/evidence/history/`.

`personal-access.mjs` exercises the session and device-pairing journey against
an already running disposable Compose candidate. Start first-run onboarding
with `./factory open`, set `FACTORY_OFFICE_URL`, pass the one-time confirmation
as `FACTORY_ENROLLMENT_CODE`, and point `CHROMIUM_EXE` at the pinned browser.
It verifies the initial cookie session, reload persistence, independent phone
session, matching-code approval, rejection, single-device revocation, all-device
revocation, challenge replay rejection, proxy-header denial, PWA worker
registration, and 1440/834/390 px layouts. Its output marks mobile-network
Tailscale access `NOT_TESTED`; this local browser journey does not prove remote
connectivity, node restart persistence, HTTPS, or Funnel exposure.
