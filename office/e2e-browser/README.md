# Factory Office browser journeys (S1, S2, S3-C)

Real-Chromium user journey against a running Core + Office build. This is
the independent browser oracle for F1-S1 acceptance: it drives the actual
UI (token gate, Dashboard, Actions, Action Detail) and asserts ledger
state survives a Core restart.

## Prerequisites

- A Chromium binary. Locally this reuses the pre-existing Playwright
  browser cache via `CHROMIUM_EXE`; in CI install one with
  `npx playwright install chromium` (downloads the browser).
- `npm ci` in this directory (installs only `playwright-core`, pinned;
  no browser is downloaded by the package itself).

## Run

```sh
# 1. Build the UI and start Core against a scratch DB:
(cd .. && npm run build)
FACTORY_API_TOKEN=<token> factory-core \
  --addr 127.0.0.1:8471 --db /tmp/s1-browse.db --web-dir ../dist

# 2. Fresh DB journey (creates the action, transitions it):
BASE_URL=http://127.0.0.1:8471 FACTORY_API_TOKEN=<token> \
  CHROMIUM_EXE=$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome \
  PHASE=1 npm run journey
# writes office/e2e-browser/.journey-action.json on success

# 3. Restart Core on the SAME db, then verify persistence in a fresh browser:
PHASE=2 npm run journey   # same env as above
```

## S3-C agent observability

`office/e2e-browser/s3c-journey.mjs` runs in two phases against a real Core
and Worker with a deterministic `paseo` CLI stub (no daemon, no provider
calls). Phase `success` asserts the empty Agents state, provider/model/mode/Worker/
Attempt identity, Run-detail session evidence, an ordered duplicate-free
timeline, reload across a malformed SSE frame, and no source/prompt
rendering. Phase `multi` asserts two Attempts/generations stay distinct.
Phase `live` asserts a pending/active binding appears while the Run is still
executing, with no fabricated final evidence. Phase `orphan` asserts
historical state after a Core restart, an honest orphan warning, and
cancellation wording that never claims an external stop. Run the whole deterministic lane from the repo root:

```sh
bash scripts/e2e-s3c.sh
```

Set `FACTORY_S3C_STRICT=1` in CI so a missing browser or install becomes a
failure instead of `BLOCKED`.

## Core outage recovery

`npm run reconnect` keeps a real authenticated browser open while it stops
and restarts Core through the compiled lifecycle CLI. It checks the visible
reconnecting state, replay of the restart event, retained Action state, and a
contiguous duplicate-free event ledger. Example after `scripts/build-factory.sh`:

```sh
FACTORY_HOME=/tmp/factory-reconnect \
FACTORY_ADDR=127.0.0.1:8471 \
FACTORY_BIN="$PWD/../../build/factory/factory" \
BASE_URL=http://127.0.0.1:8471 \
CHROMIUM_EXE="$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome" \
npm run reconnect
```

## Parser regression tests (no browser needed)

`npm run test:parser` compiles the real `../src/sse.ts` with esbuild and
runs `sse-parser.test.mjs` under `node --test` (stdlib only, no new deps).
Covers heartbeat-only frames (the F1 freeze regression), partial-frame
carry, mixed heartbeat/event/malformed input, and unterminated garbage.

`npm run test:close` unit-tests the shared shutdown helper
(`harness.mjs`, also stdlib only): a hanging close times out, SIGKILLs
and reports failure; a clean close returns fast with no lingering timer.

`npm run test:launch` fault-injects a hung browser executable with a child
process and proves the harness returns on time and kills the entire owned
process group. The shared harness bounds locator operations to 15 seconds
and page navigation to 30 seconds.

## Long-running soak (>=95s, real Chromium)

`BROWSER_HEADLESS=false npm run soak` (same env as journey) keeps the Office
open across many ~20s server heartbeats, a second tab selected with a native
Ctrl+Tab key event, foreground recovery with Ctrl+Shift+Tab, and stream
mount/unmount cycles. Headless mode exits early because it cannot establish
that a real tab lost focus. On Linux, the soak launches Chromium through
Xwayland and requires `python3-xlib` plus a display server. KDE may show a
one-time input-control prompt for `python3`; approve it before the soak. The
“always allow” option is not required. Playwright's default focus emulation
must be disabled for both tabs so `document.hasFocus()` follows real browser
selection; the soak requires Office to lose focus while the second tab gains
it, then checks that both states reverse after Ctrl+Shift+Tab. In the headed
Chromium 156 CDP run, `document.visibilityState` remained `visible` for an
unselected tab, so it is not used as the tab-selection oracle. The soak fails
on any main-thread stall (>8s probe),
page error, duplicate or missing ledger event, or growth in established
connections to the Core port (checked with `ss`). Evidence lands in
`.soak-evidence/` (gitignored).

The native tab switch helper uses the pinned dependency in `requirements.txt`;
use an active Python 3 environment that has that requirement installed. The
soak launches Chromium over Xwayland; KWin may show a one-time `python3`
input prompt before tab switching works.

Screenshots land in `.journey-evidence/` (gitignored). Phase 1 asserts:
gate renders, unlock works, Dashboard is live, an Action is created
through the form, appears in the list, its detail shows the
`ActionCreated` event, and Start moves it to `in_progress`. Phase 2
asserts the Dashboard stream replays `CoreStarted`/`ActionCreated`/
`ActionTransitioned` after restart and the persisted detail timeline is
intact.
