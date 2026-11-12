# Core-mediated MCP tool gateway — Worker and Office slice

**Candidate:** `0465c446da7069d043be9cadade0e4b46c48d3b0`  
**Branch:** `feat/tool-gateway`  
**Date:** 2026-11-12  
**Evidence level:** implementer self-test. Not independent verification,
Quality Gate acceptance or an isolation claim.

## Change boundary

The Worker can request an MCP-backed capability only through Core's
`gateway_call`. Core authenticates the Worker session, requires a live
TRUSTED_LOCAL_PROVIDER Attempt lease and generation, applies the immutable
Implementer-filtered catalog, records a durable audit row and returns the MCP
content. Office Runtime → Diagnostics exposes the enabled/disabled state, the
catalog, read-only policy and recent durable decisions. No MCP endpoint,
credential or raw argument is returned to the browser.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p factory-native --lib --locked
cargo test -p factory-native --bin factory-core --locked
cargo test -p factory-native --bin factory-worker --locked
cargo test -p factory-native --tests --locked
cargo deny check licenses bans sources
semgrep --config .semgrep.yml --error --quiet
cd office && npm run typecheck && npm run lint:typed && npm run lint && npm test -- --run && npm run build
FACTORY_EVIDENCE_DIR="$PWD/docs/evidence/tool-gateway-worker-office-2026-11-12" \
  node office/e2e-browser/rust-journey.mjs
```

## UI acceptance matrix

| ID | Surface | Control or state | Expected result | Evidence | Result |
|---|---|---|---|---|---|
| UI-001 | `/runtime?tab=diagnostics` | MCP tool gateway panel | Heading, enabled badge and Core-backed status are visible | `runtime-gateway-enabled-1440.png`; journey assertions | PASS |
| UI-002 | same | enabled catalog summary | Server `context7 (http)`, one authorized tool and the per-role rate limit render | `runtime-gateway-enabled-1440.png` | PASS |
| UI-003 | same | catalog row | `context7.query-docs` renders with its argument hint and `Read-only` policy | `runtime-gateway-enabled-1440.png` | PASS |
| UI-004 | same | empty recent calls | Explicit “No mediated MCP call has been recorded yet.” rather than invented history | `runtime-gateway-enabled-1440.png` | PASS |
| UI-005 | same | responsive layout | No document overflow at 834 or 390 px; the catalog stacks into labelled rows at 390 px | `runtime-gateway-enabled-390.png`; journey width assertions | PASS |
| UI-006 | same | accessibility floor | No critical or serious axe violations inside the gateway panel | journey scoped axe scan | PASS |
| UI-007 | same | disabled state | Explicit “not configured” empty state; no fake catalog or calls | `Runtime.test.tsx` | PASS (component) |
| UI-008 | same | populated recent calls | ALLOW/DENY, role, latency, bytes and bounded error render; endpoints and args are absent | `Runtime.test.tsx` | PASS (component); real-browser populated state `NOT_TESTED` |
| UI-009 | same | Spanish labels | Panel title, states and table labels have Spanish catalog entries | `catalog.es.ts`; existing i18n tests | PASS (component) |
| UI-010 | same | keyboard/tab navigation | The Diagnostics tab can be selected and exposes `aria-selected="true"` | journey assertion | PASS |

Axe is scoped to the new panel so the pre-existing global command-trigger
`label-content-name-mismatch` does not mask the changed surface. The journey
still checks document overflow and page-level rendering.

## Core and Worker evidence

| Check | Expected result | Evidence | Result |
|---|---|---|---|
| Operator status authentication | Missing/wrong bearer is rejected; operator sees disabled or enabled status | `factory-core` route test | PASS |
| Worker session authentication | Missing/wrong bearer is rejected | `factory-core` route test | PASS |
| Live Attempt binding | A registered Worker without the exact live lease/generation receives `403` | `factory-core` route test | PASS |
| Durable audit | ALLOW/DENY row carries Worker/Attempt/generation and argument digest; UPDATE/DELETE are rejected by triggers | `db::tool_gateway` test | PASS |
| Immutable identity | Unknown ids, server remaps, MCP-name remaps and role escalation are rejected | `tool_gateway` tests | PASS |
| Transport bounds | Args ≤ 64 KiB; response ≤ 256 KiB; timeout ≤ 300 s; rate limit per role/tool | `tool_gateway` tests | PASS |
| Worker tool pack | `gateway_call` is accepted, requires `write_file`, and is hidden when the assignment has no catalog | `factory-worker` tests | PASS |
| Provider paths | OpenCode and Codex both route `gateway_call` through the same Core endpoint | Code inspection + Worker tests | PASS (static) |

## Browser and network observations

The real Chromium journey completed with `{"ok":true,"journey":"office"}`.
It started disposable Core/Supervisor state, enrolled a real device session,
opened Runtime → Diagnostics, observed the configured `context7` catalog,
checked 834/390 px widths and ran the scoped axe scan. No unexpected console
errors or page errors were recorded by the journey.

## Limitations

- No live Context7, GitHub read-only or Playwright MCP call was exercised. The
  browser journey used a configured catalog but deliberately did not invoke a
  remote MCP server.
- The populated recent-call table is component-tested, not exercised in the real
  browser. A durable row requires a real Worker/Attempt gateway call.
- The running Core had no live Worker/Attempt, so the authenticated
  `/worker/tool_gateway` success path was not exercised end-to-end here. Its
  library, database, route-auth and binding components are covered separately.
- No independent verifier, Quality Gate acceptance, live provider turn or
  security review was performed for this slice.
- `TRUSTED_LOCAL` is not isolated or certified. Browser navigation/click/type
  remain disabled until a controlled network boundary exists.
