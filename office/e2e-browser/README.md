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

`intake-journey.mjs` is the unified Home → request workspace → approval →
Campaign journey. It starts a disposable real Core with a deterministic
OpenAI-compatible fake gateway and a fixture project that has engineering docs
and a committed Nix environment. The fake model returns only test data; Core
authorization, subscription admission, planning review, environment freeze,
Campaign start, the Worker, independent verifier and the Quality Gate remain
real. `cargo xtask e2e intake` covers assessment through operator approval and
Campaign authorization; `cargo xtask e2e intake-full` also runs the fixture
Worker, verifier, one reviewer process per frozen role, the Gate, and a Core
restart/browser reload persistence check.

```sh
cargo xtask e2e intake
cargo xtask e2e intake-full
```

The journey builds Core with the debug-only `test-support` feature. Three
fixtures are explicitly test-only and are not reachable in a production build:

- `FACTORY_ENVIRONMENT_TEST_BUILDER=1` selects a deterministic environment
  builder. Its fixed `fixture/environment-builder` image/digest and receipts are
  labelled fixture evidence; no Nix, network or real build occurs.
- `FACTORY_VERIFIER_LOCAL_ENVIRONMENT_CHECKS=1` lets the debug+`test-support`
  verifier run the frozen check argv against the already-imported local
  candidate volume when the local test backend is selected. Production
  verification still requires the runtime broker and a frozen builder image.
- `POST /test-support/intake/{id}/clear-fixture-environment` removes the
  ActionSpec environment placeholder that `test-support` installs at Action
  creation, so the journey can bind the real compiled/frozen plan and spec. The
  route does not exist without `test-support` and production never installs the
  placeholder.

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

## Accessibility scan

`npm run test:a11y` runs `axe-check.mjs` against a running Office URL and writes
a bounded JSON report. Serious/critical violations fail the command. This is an
automated floor, not a WCAG conformance claim; keyboard, focus visibility,
320 px/400 % reflow, screen-reader output and content quality remain manual.

```sh
FACTORY_OFFICE_URL=http://127.0.0.1:8471 \
CHROMIUM_EXE=/path/to/chrome \
npm run test:a11y
```

## WP10 quality-first Office acceptance

`wave12-acceptance.mjs` starts an isolated real Rust Core on a dedicated
loopback port with a disposable SQLite database, starts the real Office Vite
server, and drives Chromium through the Director plan review, reviewer
independence, finding disposition, workflow trace and Skills technology-pack
surfaces. Core is built with the debug-only `test-support` feature and seeds
labelled fixture rows; the browser always uses the real Core HTTP API and no
paid model inference runs. The script writes screenshots and `manifest.json`
under `docs/evidence/wave12-office-acceptance-2026-11-10/`.

```sh
# From the repository root.
(cd native && cargo build -p factory-native --bin factory-core --features test-support)
(cd office && npm ci)
(cd office/e2e-browser && npm ci)
node office/e2e-browser/wave12-acceptance.mjs
```

The run covers EN and ES at 1440, 834, 390 and 320 px, keyboard focus order,
disabled and error states, negative redaction, unauthorized Core reads, long
lists, and browser console/network cleanliness. It is implementer self-test
evidence, not independent acceptance or certification. Seeded state is recorded
in the manifest; the duplicate-sequence projection is injected by the
test-support endpoint because the durable event primary key cannot hold a
duplicate sequence.

`personal-access.mjs` exercises the session and device-pairing journey against
an already running disposable Compose candidate. Start first-run onboarding or
local recovery, set `FACTORY_OFFICE_URL`, pass the one-time confirmation as
`FACTORY_BOOTSTRAP_CODE`, and point `CHROMIUM_EXE` at the pinned browser.
It verifies the initial cookie session, reload persistence, current-device
marking, independent phone session, matching-code approval, rejection,
cancellation, single-device revocation, all-device revocation, challenge replay
rejection, proxy-header denial, PWA worker registration without cache entries,
security activity rendering, and 1440/834/390 px layouts. Its output marks
mobile-network Tailscale access `NOT_TESTED`; this local browser journey does not
prove remote connectivity, node restart persistence, HTTPS, or Funnel exposure.
The harness passed at implementation `9afbb6f`; the evidence record is
`docs/evidence/personal-access-pairing-2026-10-28.json`.
