# Core tool gateway

**Status:** integrated Worker/Office slice. The domain/policy/transport
library, development CLI, authenticated Core endpoint, durable Core audit,
role-filtered Worker tool pack and Office diagnostics are implemented. No live
MCP server call or independent acceptance is claimed, and this is not an
isolation boundary.

## Why

Agents need MCP-backed capabilities (browser inspection, current library docs,
read-only GitHub context), but connecting MCP servers directly to a Worker,
Reviewer or Verifier creates tool sprawl, a second tool path and non-reproducible
evidence. The gateway keeps the useful capability and removes the bypass:

```text
Worker (Implementer)
        │  authenticated gateway_call
        ▼
Core Tool Gateway
  ├── curated catalog by role
  ├── argument bound / digest
  ├── per-role/tool rate limit
  ├── timeout and response bound
  ├── SQLite audit bound to Worker/Attempt/generation
  └── MCP transport
        ├── HTTP  (Context7, GitHub read-only)
        └── stdio (Playwright MCP)
```

MCP is an implementation detail behind the gateway. A Worker never connects to a
server directly. The current slice wires the Worker implementation path; the
Reviewer and Verifier still have no gateway client, and the catalog rejects any
Verifier grant. The Verifier's oracle remains the frozen candidate and checks.

## Catalog and roles

The example catalog is
[`native/fixtures/tool-gateway.example.json`](../../native/fixtures/tool-gateway.example.json).
The tool identity, server mapping, exact MCP tool name and read-only
classification are an **immutable Core catalog**, not a free-form config: a
config cannot rename a server, point `context7` at another endpoint, invent a
tool id, remap a known id to a different MCP tool name, or relabel a mutating
tool as read-only for Reviewer. Dispatch uses the immutable MCP name, not the
configured one.

GitHub tools are advertised only when the exact Project has a resolvable GitHub
`owner/repo` from a strict HTTPS or SSH GitHub URL (source URL for GitHub/git
projects, remote origin for local projects). Unrecognized hosts, credentials,
queries, fragments, subpaths and deceptive URLs fail closed. Core requires the
tool arguments' `owner` and `repo` to match that project repository; a missing
or different repository is denied. `github.search_code` is not exposed because
a free-form code search cannot be bound reliably to one repository; it returns
only with a server-side scoping contract.

| Tool | Roles | Read-only |
|---|---|---|
| `context7.resolve-library-id`, `context7.query-docs` | Implementer, Repairer, Planner, Reviewer | yes |
| `playwright.browser_snapshot`, `browser_take_screenshot` | Implementer, Repairer, Reviewer | yes |
| `playwright.browser_close` | Implementer, Repairer | no |
| `github.issue_read`, `pull_request_read`, `get_file_contents` | Implementer, Repairer, Reviewer | yes |

The gateway rejects:

- unknown tools;
- a role not on the tool;
- an unknown tool id, a tool mapped to a different server, or a trusted server
  id pointed at an untrusted endpoint;
- a Verifier grant or a Reviewer grant for a mutating tool, even if the
  configuration claims `readOnly: true`;
- arguments over 64 KiB;
- more than `maxCallsPerMinute` calls for the same role/tool;
- a response over the per-tool bound.

## Configuration

```sh
export FACTORY_TOOL_GATEWAY_CONFIG=$PWD/native/fixtures/tool-gateway.example.json
# Optional, for the read-only GitHub MCP server:
export GITHUB_MCP_TOKEN=...
```

The gateway is **disabled** when `FACTORY_TOOL_GATEWAY_CONFIG` is unset. This is
intentional: a missing MCP server must not silently become a different tool path.
Model-backed intake adds `gateway_call` to the Campaign tool grant only when the
gateway is configured; a direct Campaign authorization may request it
explicitly. If a Campaign authorizes `gateway_call` but Core has no gateway, the
claim fails closed instead of silently dropping the tool.

The stdio child receives only the configured `env`. A deployment that runs
Playwright MCP must supply its runtime `PATH`, `HOME` and
`PLAYWRIGHT_BROWSERS_PATH`; secrets do not belong in that map.

## Development CLI

```sh
cargo run -p factory-native --bin factory-tool-gateway -- \
  IMPLEMENTER context7.query-docs '{"libraryId":"/facebook/react","query":"useEffect"}'
```

The CLI prints the MCP result plus the audit record. It is development tooling,
not the installed runtime and not authority.

## Policy and audit

- The Worker receives only the Implementer-filtered catalog in
  `WorkerAssignment.gatewayTools`; the full catalog is never exposed to the
  model or the Worker.
- `gateway_call` is an authorizable provider tool. Core resolves the Worker
  role as Implementer and binds the request to the Worker session, live
  Attempt lease and generation.
- Arguments are bounded and hashed; raw arguments are not written to audit.
- Audit errors are stable classes (`role_denied`, `transport`, `rate_limited`,
  etc.), never raw transport URLs, commands or MCP response bodies.
- Every ALLOW/DENY is persisted in `tool_gateway_calls` with the argument
  digest, decision, latency, response bytes, error class and exact
  Worker/Attempt/generation binding. The in-memory log remains a bounded local
  diagnostic view.
- `GET /tool_gateway/status` is operator-authenticated and returns the catalog
  plus recent durable calls. It never returns MCP endpoints, credentials or raw
  arguments.
- A transport error is returned to the caller and recorded; it is never a
  silent `PASS`.
- Audit persistence happens after the gateway decision and before the Worker
  response. A persistence failure returns `500`, so a transport retry may repeat
  a tool call. The current catalog contains read-only inspection and
  browser-close operations; it has no arbitrary effectful MCP tool.

## Transport bounds

- HTTP MCP: HTTPS only, except explicit loopback HTTP for tests; no redirects;
  one tool deadline covers initialization and the call; optional bearer from
  env.
- stdio MCP: explicit command/argv; no shell; stderr discarded to avoid
  deadlock; child killed on drop; one deadline covers initialize, write and
  read.
- Per-tool bounds: response at most 256 KiB and timeout at most 300 s.
  Validation rejects larger values, and the readers clamp to the same hard
  ceiling while streaming.
- Playwright MCP is a development/container capability. Do not install it into
  the Core image by default.
- The example catalog intentionally exposes browser **inspection**
  (`snapshot`/`screenshot`), not arbitrary navigation or click/type. Playwright
  MCP's `--allowed-hosts` guards the MCP server host, not every redirect or page
  request; a navigation-capable browser tool must add a controlled browser
  network boundary before it is enabled.

## Recommended pattern

This follows the current MCP gateway/registry guidance rather than connecting
servers directly to agents:

- [AWS Well-Architected Agentic AI Lens](https://docs.aws.amazon.com/wellarchitected/latest/agentic-ai-lens/agentsec02-bp03.html) — runtime-enforced registry and gateway.
- [Microsoft MCP Security Gateway](https://microsoft.github.io/agent-governance-toolkit/tutorials/07-mcp-security-gateway/) — deny lists, sensitive tools, approval routing, rate limits, audit.
- [BRACE MCP gateway](https://braceframework.org/guides/mcp-gateway/) — registry, role filtering, fingerprint, budget and fail-closed call path.
- [OWASP MCP Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html) — tool poisoning, rug pull, confused deputy, schema validation, isolation.

## Worker and Office integration

- `POST /worker/tool_gateway` accepts `workerId`, `attemptId`, `generation`,
  `tool` and `arguments`; it requires the Worker session token, a live
  TRUSTED_LOCAL_PROVIDER lease bound to the exact Attempt, and a Campaign grant
  that explicitly includes `gateway_call`.
- `gateway_call` is part of the compiled provider tool pack. The OpenCode tool
  loop and the Codex dynamic-tool path both route it through the same Core
  endpoint; `write_file`/`run_command` frozen-check semantics are unchanged.
- Office Runtime → Diagnostics shows the enabled/disabled state, the
  role-filtered catalog, read-only policy and recent durable call decisions.

## Next slice

1. Wire the read-only Reviewer path to the same gateway with Reviewer-only
   catalog filtering; keep the Verifier without gateway tools.
2. Add replay cassettes and deterministic MCP fixtures for docs, GitHub and
   browser inspection so a provider defect can be reproduced without a live
   server.
3. Add a controlled browser navigation/network boundary before exposing
   navigation, click or type.
4. Exercise live Context7, repository-scoped GitHub read-only and Playwright MCP
   calls in the canonical container topology, including the stdio environment
   contract.
5. Obtain independent browser and security review for the Worker/Office path.
