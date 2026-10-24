# Factory Office

React + TypeScript Office backed by the Rust Core API. The only API client is
the Rust Core client; former provider-specific views were removed. The app
renders server-authoritative snapshots and polls Core; no live SSE claim is made.

Current views: Dashboard, Projects, Actions, Lead, Campaigns/Campaign detail,
Runs and Workers. Campaigns show WorkGraph tasks, candidates and deterministic
Quality Gate outcomes; Runs queues Attempts. Recovery/cleanup projection and
detailed provider-session evidence remain future work.

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

The Rust-backed views typecheck, lint and build. `cargo xtask e2e office` runs
the real Chromium journey against the Rust Core, Supervisor and Worker; see
`e2e-browser/README.md`. That journey is author-run and CI evidence, not
independent acceptance or isolation certification.
