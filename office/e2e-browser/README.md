# Factory Office browser journey (S1)

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

Screenshots land in `.journey-evidence/` (gitignored). Phase 1 asserts:
gate renders, unlock works, Dashboard is live, an Action is created
through the form, appears in the list, its detail shows the
`ActionCreated` event, and Start moves it to `in_progress`. Phase 2
asserts the Dashboard stream replays `CoreStarted`/`ActionCreated`/
`ActionTransitioned` after restart and the persisted detail timeline is
intact.
