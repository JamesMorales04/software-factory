# Factory Office (F1-S1)

React + TypeScript UI over the Core API and durable event ledger.
Renders server-authoritative state only; the Agents view honestly reports
"no agents yet" until F1-S2.

## Toolchain (pinned in `package.json` + `package-lock.json`)

Node `22.23.2` (`.nvmrc`), Vite `7.3.7`, React `19.3.1`,
`react-router-dom` `7.18.4`, TypeScript `5.9.3`, Biome `2.5.15`.

## Commands (run from `office/`)

```sh
npm ci            # reproducible install
npm run typecheck # tsc --noEmit
npm run lint      # biome check src
npm run build     # typecheck + production bundle into dist/
npm run dev       # vite dev server (proxies /api to 127.0.0.1:8471)
```

The token gate keeps the API token in `sessionStorage` only; the event
stream resumes from the last `globalSeq` cursor on reconnect (no EventSource:
`fetch` + `ReadableStream` so the token stays in the header).
Hash routing is used so the Core static fallback serves deep links.
