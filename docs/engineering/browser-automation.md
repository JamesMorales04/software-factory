# Browser automation for Implementers and independent verification

**Status:** F1-S4-A implementation; extended by F1-S4-C as a Core-governed
Project/Run capability. Research and disposition: **CLI adopted, MCP deferred**.
TRUSTED_LOCAL only; no isolation/certification claim.

## Primary tool: pinned Playwright CLI + existing Chromium harness

- Package: `@playwright/cli` **0.1.22**, pinned in
  `office/e2e-browser/package.json` (installed by `npm ci`).
- Runtime: Node 22.23.2 (`.nvmrc`), Chromium supplied explicitly through
  `CHROMIUM_EXE`; CI installs Chrome for Testing via
  `browser-actions/setup-chrome`.
- The existing `playwright-core` 1.64.1 harness in `office/e2e-browser` remains
  the assertion/launch library for deterministic journeys and independent
  browser checks. The CLI is the token-efficient agent surface; the harness is
  the deterministic test surface.
- The official skill can be installed with
  `office/e2e-browser/node_modules/.bin/playwright-cli install --skills=agents`.
  Skill text is external, untrusted input; it cannot grant capabilities, bypass
  Run scope, or replace Factory policy. The project-owned
  `.agents/skills/factory-browser/SKILL.md` documents the authoritative Factory
  workflow and trust boundary.

## Agent workflow

`scripts/agent-browser-office.mjs` drives the pinned CLI from a disposable
workspace/profile. It:

1. launches real Chromium with `--config` pointing at `CHROMIUM_EXE`;
2. unlocks the actual Factory Office token gate;
3. creates an Action through the real form;
4. verifies the Action against the Core API;
5. opens the authoritative Action detail;
6. collects console errors and failed requests;
7. captures a bounded screenshot and writes `agent-browser-evidence.json`;
8. closes the browser and removes the temporary profile/workspace.

```sh
BASE_URL=http://127.0.0.1:8471 \
FACTORY_API_TOKEN=... \
CHROMIUM_EXE=/path/to/chrome \
S4A_BROWSER_EVIDENCE_DIR=/tmp/agent-browser \
node scripts/agent-browser-office.mjs
```

The Worker exposes `FACTORY_BROWSER_CLI` to the Implementer prompt when
configured, so an eligible agent can discover and invoke the same CLI against an
authorized local URL. This is an Implementer self-test capability; it is **not**
independent evidence.

## Independent verifier boundary

Browser acceptance is a frozen spec check of kind `browser` (for example
`node browser-check.mjs`). The Independent Verifier:

- checks out the exact candidate into a clean read-only worktree;
- keeps protected browser-check scripts outside the Implementer's allowed scope;
- runs real Chromium with an isolated profile and a temporary `HOME`;
- receives only `PATH`, `HOME`, `TMPDIR`, `CHROMIUM_EXE`,
  `FACTORY_PLAYWRIGHT_CORE` and `PLAYWRIGHT_BROWSERS_PATH` — never Factory or
  provider credentials;
- re-verifies the checkout identity and rejects tracked modifications after the
  checks;
- records bounded browser provenance (browser/version, console error count,
  failed-request count, isolated-profile marker) and concrete PASS/FAIL.

Missing Chromium or `playwright-core` yields `NOT_TESTED`; a test failure yields
`FAIL`; neither is accepted by the Quality Gate.

## Playwright MCP evaluation

`@playwright/mcp` (`0.0.83` available at evaluation time) was evaluated and
**deferred**. The CLI + `playwright-core` harness already cover navigation,
interaction, assertions, screenshots, diagnostics, clean profiles and CI. MCP
would add a persistent browser-server surface, profile/session lifecycle and
another credential boundary without a concrete requirement. It is not
mandatory and no MCP management subsystem is built in F1-S4-A.

## Security and limitations

- TRUSTED_LOCAL processes have broad local access. The browser is not an
  isolation boundary and this integration does not claim SEC-01-ISOLATED.
- Temporary profiles and workspaces are disposable and removed after the run;
  no shared authenticated profile is persisted.
- Evidence must not contain bearer tokens, cookies, raw page content or
  unbounded request bodies. Screenshots are bounded artifacts, not acceptance
  proof by themselves.
- Browser tooling/Chromium absence is an explicit `BLOCKED`/`NOT_TESTED`
  condition for mandatory checks, never a silent skip.
