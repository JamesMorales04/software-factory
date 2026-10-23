# Factory Office

React + TypeScript Office backed by the Rust Core API. The legacy Go/Paseo API
client and Paseo-specific views have been removed. The app renders
server-authoritative snapshots and polls Core; no live SSE claim is made.

Migrated views: Dashboard, Projects, Actions, Campaigns, and Runs. Campaigns
show candidates and deterministic Quality Gate outcomes; Runs queues Attempts.
Delegations/Lead and detailed session/evidence views are not yet exposed by the
Rust API and are not rendered.

## Toolchain

Node `22.23.2` (`.nvmrc`), Vite `7.3.7`, React `19.3.1`,
`react-router-dom` `7.18.4`, TypeScript `5.9.3`, Biome `2.5.15`.
Dependencies are pinned by `package.json` and `package-lock.json`.

## Commands (from `office/`)

```sh
npm ci
npm run typecheck
npm run lint
npm run build
npm run dev
```

The Vite dev server proxies `/core-api` to `http://127.0.0.1:8765` and strips
the prefix. The operator token stays in `sessionStorage`.

## Status

The Rust-backed views typecheck, lint and build. No Playwright browser journey
has been migrated to the Rust API yet; the retained `e2e-browser/` scripts
describe the former API and are historical until replaced. Do not use them as
evidence for the Rust Office.
