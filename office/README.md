# Factory Office

React + TypeScript Office backed by the Rust Core API. The only API client is
the Rust Core client; former provider-specific views were removed. The app
renders server-authoritative snapshots and polls Core; no live SSE claim is made.
Runtime is a tabbed console for health, registered Workers, filtered sessions,
model-call reservations, audited release, cleanup and telemetry; unsupported
Supervisor and nested-runtime state is shown as not exposed. Settings provides
provider route and credential lifecycle, connection diagnostics, advanced
limits, runtime review policy and configuration audit without returning
credential values.

Current views: Overview, Decision Inbox, Projects, Actions/Action detail, Lead,
Delegations, Campaigns/Campaign detail, Agent Office, Runs, Workers, Runtime and
Settings, inside a responsive shell with grouped navigation, command search,
themes and density. Agent Office projects recorded Lead, Implementer, Verifier,
Reviewer and Worker participants into URL-backed visual, table and timeline
views with a shared inspector and bounded provider tool events. Campaign detail projects the
Project → Action → Lead → Assessment → Plan → Campaign → WorkGraph → Attempt →
Candidate → Verifier → Reviewer → Gate workflow from Core data in URL-backed
tabs for lifecycle, WorkGraph, execution, candidates, evidence, Gate, budget and
events/recovery. The WorkGraph includes a deterministic dependency layout with
keyboard node selection and an inspector while retaining the authoritative
table. It shows per-unit dependencies/status, a bounded candidate diff
summary, receipts, honest pending states for missing evidence, deterministic
Gate outcome and reasons, provider-session usage/cleanup and a compact
authorization budget projection. Operators can
request Campaign cancellation with an explicit confirmation; cancelled and
unconfirmed provider termination are shown from the API rather than inferred.
Lead detail decodes the recorded assessment, proposal and frozen plan, shows
Lead model-call provider/model/reservation/usage provenance, and links a started
Lead request to its Campaign. A READY FIXTURE plan has a Start control. A READY
MODEL plan requires the operator to submit the explicit
`TRUSTED_LOCAL_PROVIDER` authorization form; the Office never invents or
silently downgrades that authorization. Campaign detail labels the execution
mode and route, records Lead FAILED/BLOCKED state, and renders Reviewer
provider/model/reservation/request-digest/usage provenance even when a receipt
has no findings. Candidate Campaign cells and Run Campaign cells link to the
Campaign detail route, which reloads from Core directly.

## Toolchain

Node `22.23.2` (`.nvmrc`), Vite `7.3.7`, React `19.3.1`,
`react-router-dom` `7.18.4`, TypeScript `5.9.3`, Biome `2.5.15`.
Dependencies are pinned by `package.json` and `package-lock.json`.

## Commands (from `office/`)

```sh
npm ci
npm run typecheck
npm run lint
npm run test
npm run build
npm run dev
```

The Vite dev server proxies `/core-api` to `http://127.0.0.1:8765` and strips
the prefix. The operator token stays in `sessionStorage`.

## Renderer fixture preview (development only)

The Agent Office visual renderer has a test-only preview at
`visual-harness.html`, served by the Vite dev server and excluded from the
production build. It uses deterministic fixtures, carries a permanent
`TEST FIXTURE — NOT LIVE CORE DATA` banner and is never a product route or a
substitute for Core data. Example:
`http://localhost:5173/visual-harness.html?count=5&mode=implementers&allActive=1`.

## Status

The Rust-backed views typecheck, lint, pass component tests and build. `cargo xtask e2e office` runs
the real Chromium journey against the Rust Core, Supervisor and Worker; see
`e2e-browser/README.md`. That journey is author-run and CI evidence, not
independent acceptance or isolation certification.
