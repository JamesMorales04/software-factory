# Core-mediated MCP tool gateway — Worker and Office slice

**Candidate:** `293f849da44ed815e36907e913c7b7925fd7853c`  
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
| UI-005 | same | responsive layout | No document overflow at 834 or 390 px; the catalog stacks into labelled rows at 390 px; the disabled state is checked too | `runtime-gateway-enabled-390.png`, `runtime-gateway-disabled-390.png`; journey width assertions | PASS |
| UI-006 | same | accessibility floor | No critical or serious axe violations inside the gateway panel | journey scoped axe scan | PASS |
| UI-007 | same | disabled state | Explicit “not configured” empty state; no fake catalog or calls | `runtime-gateway-disabled-1440.png`; journey assertions | PASS |
| UI-008 | same | populated recent calls | A durable ALLOW row renders role, latency and bytes; endpoints and raw args are absent | `runtime-gateway-populated-1440.png`; journey assertions | PASS (seeded durable row; no live MCP call) |
| UI-009 | same | Spanish labels | Panel title, states and table labels have Spanish catalog entries | `catalog.es.ts`; existing i18n tests | PASS (component) |
| UI-010 | same | keyboard/tab navigation | The Diagnostics tab can be selected and exposes `aria-selected="true"` | journey assertion | PASS |
| UI-011 | same | theme and density | Dark theme + compact density retain the panel layout and pass the scoped axe scan at 1440 and 390 px | `runtime-gateway-enabled-dark-compact-1440.png`, `...-dark-compact-390.png`, disabled variants; journey assertions | PASS |

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
| GitHub repository scope | GitHub tools require a strict HTTPS/SSH GitHub source and the exact Project owner/repo; deceptive hosts, subpaths and mismatches are denied; `search_code` is not exposed | Core scope/parser tests + code inspection | PASS (static/unit); live multi-repo token `NOT_TESTED` |
| Transport bounds | Args ≤ 64 KiB; response ≤ 256 KiB; timeout ≤ 300 s; rate limit per role/tool | `tool_gateway` tests | PASS |
| Worker tool pack | `gateway_call` is accepted, requires `write_file`, and is hidden when the assignment has no catalog | `factory-worker` tests | PASS |
| Provider paths | OpenCode and Codex both route `gateway_call` through the same Core endpoint | Code inspection + Worker tests | PASS (static) |

## Worker gateway E2E

`worker_gateway_call_is_bound_to_the_live_lease_and_audited` passes on host and
in `software-factory-dev:local`. It registers a Worker, claims a real Attempt
lease, receives the Implementer gateway catalog, performs a Context7 call and a
same-repository GitHub call against a deterministic fake multi-repository MCP
upstream, then proves a different-repository GitHub call is denied with a
durable `repository_scope` DENY audit and never reaches the upstream. See
`worker-gateway-e2e.json`.

## Model-driven Worker gateway E2E

`codex_fixture_runs_a_mediated_tool_turn_and_records_core_accounting` now emits
a model-driven `gateway_call` dynamic tool request from the Codex fixture. The
Worker executes it through Core, Core calls the fake Context7 upstream and
persists an ALLOW row, then the Worker writes the candidate and finishes the
turn. Host: PASS (13/13 target). Container: BLOCKED by the same three
provider-sandbox readiness failures; the direct Core/lease gateway E2E passes
in the container. See `worker-model-turn-e2e.json`.

## Canonical-container test evidence

| Test target | Environment | Result |
|---|---|---|
| `provider_sandbox` | `software-factory-dev:local`, pinned OpenCode 1.18.32 | 7 passed / 0 failed |
| `e2e_dispatch_negative --features test-support` | same dev container | 5 passed / 0 failed |
| `e2e_fixture_dispatch --features test-support` | same dev container | 10 passed / 3 failed in provider-sandbox readiness (the new worker gateway E2E passes); host run 13/13 |
| Host default `--lib` | host Rust 1.90 | 1000 passed / 0 failed / 4 ignored |
| Host `factory-core` / `factory-worker` | host Rust 1.90 | 70 / 36 passed |

A full container `--tests --features test-support` run is not a supported
combination: the test-support environment seam conflicts with 20 Core intake
fixtures that construct their own environment bindings. The gateway tests
themselves pass in that run; the default lib suite and the feature-gated E2E
targets are the supported lanes.

## Live MCP smoke

| Server | Call | Result | Evidence |
|---|---|---|---|
| Context7 | `context7.query-docs` with `/react/react` | PASS; bounded documentation content, ALLOW audit | `context7-live.json` |
| Context7 redirect | `/facebook/react` returns `isError` redirect content | PASS; content returned and audited as `tool_error` | `context7-live.json` note |
| Playwright | `playwright.browser_close` via stdio MCP 0.0.84 + pinned headless Chromium | PASS; no open tab, successful no-op | `playwright-live.json` |
| GitHub read-only | `github.pull_request_read` on `JamesMorales04/software-factory` + fake multi-repo cross-repository denial | PASS; real Project-repository read and scope E2E; real multi-repo token not exercised | `github-live.json`, `worker-gateway-e2e.json` |

## Browser and network observations

The real Chromium journey completed with `{"ok":true,"journey":"office"}` in
both modes:

- enabled: configured `context7` catalog, empty recent-call state, then a
  seeded durable ALLOW row after a reload; a real Worker-generated row is
  covered separately by the gateway E2E;
- disabled: no `FACTORY_TOOL_GATEWAY_CONFIG`, explicit not-configured state.

Both modes checked 834/390 px widths and ran the scoped axe scan. The journey
started disposable Core/Supervisor state and enrolled a real device session.
No unexpected console errors or page errors were recorded.

## Limitations

- Live Context7 and Playwright stdio calls pass. Real GitHub read-only is
  BLOCKED because no authorized `GITHUB_MCP_TOKEN` is available; the
  cross-repository denial is proven against a deterministic fake multi-repository
  upstream.
- The browser populated row is seeded directly in SQLite. A real
  Worker/Attempt-generated row is proven separately by
  `worker-gateway-e2e.json`, not rendered in the same browser run.
- The direct Worker gateway E2E exercises registration/claim/live lease and the
  authenticated Core endpoint. The separate Codex fixture E2E exercises a
  model-driven provider tool turn; it passes on host but is one of the three
  provider-sandbox readiness failures in the container.
- The canonical container `e2e_fixture_dispatch` target has 3 provider-sandbox
  readiness failures; host passes 13/13 and the new gateway E2E passes in both.
  A full `--tests --features test-support` run is not a supported combination
  because the test-support environment seam conflicts with Core intake fixtures.
- No independent verifier, Quality Gate acceptance or security review was
  performed for this slice.
- `TRUSTED_LOCAL` is not isolated or certified. Browser navigation/click/type
  remain disabled until a controlled network boundary exists.
