# Factory Office (F1-S1 … F1-S3-C)

React + TypeScript UI over the Core API and durable event ledger.
Renders server-authoritative state only: Dashboard, Actions, Runs (queue a
safe synthetic Run, watch its lifecycle, cancel), Run detail (Attempts,
per-Attempt Paseo session bindings, bounded evidence and lifecycle
lineage), Workers (observed liveness only — never fabricated activity),
and Agents (real recorded Paseo sessions, live and historical, with
`TRUSTED_LOCAL` labels and explicit disconnected/orphaned states).

Browser journeys: `office/e2e-browser/journey.mjs` (S1),
`office/e2e-browser/s2-journey.mjs` (S2 Runs/Workers), and
`office/e2e-browser/s3c-journey.mjs` (S3-C Agents/Run session
observability), runnable via their scripts from the repo root.

## Toolchain (pinned in `package.json` + `package-lock.json`)

Node `22.23.2` (`.nvmrc`), Vite `7.3.7`, React `19.3.1`,
`react-router-dom` `7.18.4`, TypeScript `5.9.3`, Biome `2.5.15`.

## Commands (run from `office/`)

```sh
npm ci            # reproducible install
npm run typecheck # tsc --noEmit
npm run lint      # biome check src e2e-browser
npm run build     # typecheck + production bundle into dist/
npm run dev       # vite dev server (proxies /api to 127.0.0.1:8471)
```

The token gate keeps the API token in `sessionStorage` only; the event
stream resumes from the last `globalSeq` cursor on reconnect (no EventSource:
`fetch` + `ReadableStream` so the token stays in the header).
Hash routing is used so the Core static fallback serves deep links.
