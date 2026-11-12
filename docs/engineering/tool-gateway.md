# Core tool gateway

**Status:** first implementation slice. Domain/policy/transport library and a
development CLI are implemented. Worker/Office wiring is the next slice and is
not yet claimed. This is not independent acceptance or an isolation boundary.

## Why

Agents need MCP-backed capabilities (browser inspection, current library docs,
read-only GitHub context), but connecting MCP servers directly to a Worker,
Reviewer or Verifier creates tool sprawl, a second tool path and non-reproducible
evidence. The gateway keeps the useful capability and removes the bypass:

```text
Worker / Reviewer / Verifier
        │  typed gateway call
        ▼
Core Tool Gateway
  ├── curated catalog by role
  ├── argument validation / host allowlist
  ├── per-role/tool rate limit
  ├── timeout and response bound
  ├── audit (args digest, decision, latency, response size)
  └── MCP transport
        ├── HTTP  (Context7, GitHub read-only)
        └── stdio (Playwright MCP)
```

MCP is an implementation detail behind the gateway. An agent never connects to a
server directly. The Verifier has no gateway tools; its oracle remains the
frozen candidate and checks.

## Catalog and roles

The example catalog is
[`native/fixtures/tool-gateway.example.json`](../../native/fixtures/tool-gateway.example.json):

| Tool | Roles | Read-only |
|---|---|---|
| `context7.resolve-library-id`, `context7.query-docs` | Implementer, Repairer, Planner, Reviewer | yes |
| `playwright.browser_navigate`, `browser_snapshot`, `browser_take_screenshot`, `browser_close` | Implementer, Repairer, Reviewer | yes |
| `playwright.browser_click`, `browser_type` | Implementer, Repairer | no |
| `github.issue_read`, `pull_request_read`, `get_file_contents`, `search_code` | Implementer, Repairer, Reviewer | yes |

The gateway rejects:

- unknown tools;
- a role not on the tool;
- a Reviewer using a mutating browser tool;
- a browser URL with credentials, a non-HTTP(S) scheme, or a host outside the
  configured allowlist;
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

## Development CLI

```sh
cargo run -p factory-native --bin factory-tool-gateway -- \
  IMPLEMENTER context7.query-docs '{"libraryId":"/facebook/react","query":"useEffect"}'
```

The CLI prints the MCP result plus the audit record. It is development tooling,
not the installed runtime and not authority.

## Policy and audit

- Tool list is filtered by role before the model sees it (next Worker slice).
- Arguments are validated and hashed; raw arguments are not written to audit.
- The audit record contains role, tool, argument digest, decision, latency,
  response bytes and error class.
- The current audit log is bounded and in-memory. Durable audit/Office projection
  is part of the next slice.
- A transport error is returned to the caller and recorded; it is never a silent
  `PASS`.

## Transport bounds

- HTTP MCP: HTTPS only, except explicit loopback HTTP for tests; no redirects;
  60-second transport timeout; response bounded; optional bearer from env.
- stdio MCP: explicit command/argv; no shell; stderr discarded to avoid
  deadlock; child killed on drop; initialize and call timeouts.
- Playwright MCP is a development/container capability. Do not install it into
  the Core image by default.

## Recommended pattern

This follows the current MCP gateway/registry guidance rather than connecting
servers directly to agents:

- [AWS Well-Architected Agentic AI Lens](https://docs.aws.amazon.com/wellarchitected/latest/agentic-ai-lens/agentsec02-bp03.html) — runtime-enforced registry and gateway.
- [Microsoft MCP Security Gateway](https://microsoft.github.io/agent-governance-toolkit/tutorials/07-mcp-security-gateway/) — deny lists, sensitive tools, approval routing, rate limits, audit.
- [BRACE MCP gateway](https://braceframework.org/guides/mcp-gateway/) — registry, role filtering, fingerprint, budget and fail-closed call path.
- [OWASP MCP Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html) — tool poisoning, rug pull, confused deputy, schema validation, isolation.

## Next slice

1. Persist the catalog/policy and audit in Core; expose health/diagnostics in
   Office.
2. Add an authenticated `/worker/tool-gateway` operation with worker session,
   attempt, candidate and role binding.
3. Add `gateway_call` to the compiled provider tool pack; keep `write_file`/
   `run_command` frozen-check semantics unchanged.
4. Filter the tool list per role before the prompt; do not expose the full
   catalog.
5. Add replay cassettes for gateway calls so a browser/docs defect can become a
   deterministic test.
