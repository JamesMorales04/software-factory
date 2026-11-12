# ADR-0044 — Core-mediated MCP tool gateway

**Status:** Accepted — 2026-11-12. Owner-directed developer-preview decision.
It grants no new merge, delivery, sandbox or acceptance authority, and it does
not claim isolation.

**Supersedes:** none.

**Related:** [ADR-0009](ADR-0009-trusted-local-execution.md),
[ADR-0022](ADR-0022-source-aware-provider-plan-mediation.md),
[ADR-0034](ADR-0034-model-capability-telemetry-and-agent-scope.md),
[ADR-0035](ADR-0035-sec01-accepted-risk-scope.md),
[ADR-0037](ADR-0037-agent-topology-context-and-composition.md) and
[tool gateway engineering](../../engineering/tool-gateway.md).

## Context and problem

Factory agents need MCP-backed capabilities: current library documentation,
read-only GitHub context and browser inspection. Connecting the same MCP server
to a Worker, Reviewer and Verifier creates several problems:

- each agent becomes an independent MCP client with its own tool list and
  bypass path;
- role restrictions (for example, no Verifier tools and read-only Reviewer
  tools) are enforced only by prompt text;
- credentials, endpoint configuration and retry behaviour spread across
  agents;
- tool identity, arguments, decisions and failures are not durably auditable in
  one place.

The Factory already has a Core-owned model gateway and a compiled provider tool
pack (`write_file`, `run_command`). The new capability must preserve that
authority model instead of creating a second one.

## Decision drivers and invariants

- Core owns the catalog, role policy, bounds, rate limit and durable audit.
- A model or agent cannot add a tool, widen a role or change the downstream MCP
  name through configuration or prompt content.
- The Verifier has no MCP tools; the Reviewer may receive only read-only tools.
- The Worker path is bound to a live Worker session, Attempt lease and
  generation.
- MCP is an implementation detail; no agent receives a raw MCP connection.
- Network and response resources are bounded; a failed call never becomes a
  silent `PASS`.
- The first browser slice exposes inspection only until a controlled page
  navigation/network boundary is implemented.

## Considered alternatives

1. **Connect MCP servers directly to each agent.** Rejected: no single policy
   point, role restrictions are prompt-only, credentials spread and audit is
   fragmented.
2. **Run a separate gateway process or container.** Not selected for this
   slice. The gateway is an in-process Core module, matching the existing model
   gateway. A later placement review may separate it if isolation, scaling or
   independent lifecycle justifies a new trust boundary.
3. **Allowlist only MCP server URLs.** Rejected: it does not constrain tool
   identity, role grants, argument bounds, response size or audit.
4. **Expose a generic HTTP proxy.** Rejected: it becomes a second unmediated
   tool path and cannot enforce the immutable catalog.

## Decision

1. Add an in-process Core `tool_gateway` module with an immutable catalog that
   binds each gateway tool id to its MCP server id, exact downstream MCP tool
   name and read-only classification. Configuration may select roles, bounds and
   a trusted endpoint, but cannot invent an identity, remap a server, rename the
   MCP tool or relabel a mutating tool as read-only. GitHub tools are further
   bound to the exact Project repository in Core; free-form code search is not
   exposed.
2. Support bounded HTTP MCP (Context7 and GitHub read-only) and stdio MCP
   (Playwright). The stdio child receives only its configured environment.
3. Enforce role policy in Core: Verifier grants are rejected; Reviewer grants
   require a read-only identity; every call is rate-limited and bounded.
4. Add an authenticated `POST /worker/tool_gateway` operation bound to the
   Worker session, a live TRUSTED_LOCAL_PROVIDER Attempt lease and a Campaign
   grant that explicitly includes `gateway_call`. The Worker receives only the
   Implementer-filtered catalog.
5. Add `gateway_call` as an authorizable provider tool. The OpenCode tool loop
   and Codex dynamic-tool path both route it through Core. `write_file` and
   `run_command` retain their frozen-check semantics.
6. Persist every decision in Core's `tool_gateway_calls` audit with the argument
   digest, decision, latency, response bytes and Worker/Attempt/generation
   binding. Expose a sanitized operator status projection in Office Runtime →
   Diagnostics.
7. Do not expose `browser_navigate`, `browser_click` or `browser_type` until a
   controlled browser network boundary is implemented and tested. Playwright
   `--allowed-hosts` guards the MCP server host, not page redirects or requests.

## Consequences

- One mediated egress path for MCP capabilities, with Core-owned role policy and
  durable evidence.
- Core becomes a runtime dependency for MCP-backed Worker turns. A missing or
  invalid gateway configuration fails closed for authorizations that request
  `gateway_call`.
- The gateway configuration is a trusted deployment input. It is not a
  sandbox and does not confine a compromised Core process.
- The Worker and Office surfaces must be included in future role changes;
  adding a role to the catalog without a client path is not completion.
- Browser navigation remains deferred, so Playwright inspection is useful only
  for pages already open in the persistent MCP browser session.
- GitHub issue, pull-request and file reads require the exact Project
  repository; `search_code` remains deferred until the MCP server exposes a
  scoping contract Core can enforce.
- Live Context7, GitHub and Playwright calls, replay fixtures and independent
  acceptance remain follow-up work.

## Verification

- Gateway unit tests cover immutable identity, role denial, Verifier exclusion,
  Reviewer read-only enforcement, response/timeout ceilings, child environment
  isolation, JSON-RPC identity and oversized responses.
- Core route tests cover operator authentication for the status projection and
  Worker session plus live-assignment binding for the call endpoint.
- Database tests cover durable audit insertion and bounded error text.
- Worker tests cover the `gateway_call` argument shape, provider tool grants and
  hiding the tool when the assignment has no catalog.
- Core tests cover GitHub repository-scope extraction and exact owner/repo
  matching; a mismatched or absent repository fails closed.
- Office component tests cover enabled, disabled and recent-call states; the
  real Chromium journey exercises the enabled diagnostics panel against a real
  Core.
- Exact candidate-bound commands and results are recorded in the handoff and
  the tool-gateway engineering document.

## Supersession

None. Revisit this decision if the gateway needs an independent trust boundary,
if MCP is exposed outside TRUSTED_LOCAL, or if a controlled browser navigation
boundary is added.
