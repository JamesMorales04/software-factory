# SEC-01 Redesign Campaign — checkpoint (2026-10-19)

Branch: `f1-s2-sec01-redesign-campaign` (base `7139c9b`, PR #4 candidate).
PR state: PR #3 MERGED (2026-10-19). PR #4 OPEN, MERGEABLE, NOT merged, NOT certified.
SEC-01: BLOCKED / PAUSED for credential-bearing execution. No S3. No auth tests run.

## Phase status

- [x] Phase 1 — reconnaissance (git, PRs, agents, providers, docs, S2 source, PoC).
- [~] Phase 2 — parallel investigation. Reused S2-B subagent evidence
      (Codex e6f33a1, OpenCode 5a11254, Paseo-surface ede1358) instead of
      redoing; added lead direct probes 2026-10-19. One gap-fill subagent
      active (see below).
- [ ] Phase 3 — synthesis (draft in this file §3; final after surface file).
- [ ] Phase 4 — PoC (lead probes already give core evidence; formalize §4).
- [ ] Phase 5/6 — recommendation + owner checkpoint (this file §5–§7).

## Active subagents

- `4140142a` Stream A completion (codex/gpt-6-sol, auto-review): Paseo
  plugin/relay/adapter gaps → `/tmp/sec01-paseo-surface.md`. Check with
  `paseo logs 4140142a`.
- Prior S2-B agents (finished, evidence on disk): `e6f33a1` Codex,
  `5a11254` OpenCode, `ede1358` Paseo surface (partial, no file).

## Completed investigations (evidence)

1. Codex 0.156.1 (`/tmp/sec01-codex-findings.md`, 1892 lines, CODEX-NEEDS-AUTH-PLAN):
   bwrap 0.13.1 Linux sandbox real; custom allowlist gives OS denials
   (EROFS/ENOENT/EPERM, NoNewPrivs, seccomp); legacy `read-only` leaks;
   gaps: local-MCP launcher w/o sandbox, remote MCP `sandbox: None`,
   resume accepts weaker policy, hooks/plugins/cloud untested vs Paseo.
2. OpenCode 1.18.32 (`/tmp/sec01-opencode-findings.md`, OPENCODE-BLOCKED):
   permissions/modes/MCP in-process only (pinned tag `545f51d`);
   `plan` ≠ read boundary; hidden `--yolo`; plugins = arbitrary host code;
   subagents exceed parent; no OS boundary via any config.
3. Lead probes 2026-10-19 (this campaign, credential-free):
   - Rootless Podman 6.1.4, startup 137 ms (`true` container).
   - Dev workflow in `ghcr.io/getpaseo/paseo:latest` (node 22.23.3, git):
     `node --test` PASS, loopback HTTP `LOOPBACK:hi` with `--network=none`,
     outside canary INACCESSIBLE (unmounted), root ROFS enforced, workspace
     writable. = STANDARD profile feasible in container.
   - Egress default-deny verified (`registry.npmjs.org` UNREACHABLE).
   - Stock paseo image entrypoint incompatible with `--userns=keep-id`
     (needs `--user` or custom image without UID-drop entrypoint).
   - Kill + `rm -f` cleanup: 0 orphans.
   - Daemon topology: 0.9.1 daemon → worker → codex `app-server` +
     `opencode serve` as same-UID children. No inter-process OS boundary.
   - SDK 0.11.4: `AgentSessionConfig` has NO sandbox/env/UID/net options;
     `toolPolicy.preapproved` + `mcpServers` are advisory routing;
     `disallowedTools` hides tools from model only. All confirmed in
     `protocol/dist/agent-types.d.ts:430-480`, `provider-config.d.ts`.
   - Providers available: codex + opencode (both `available:true`).

## Technical comparison (draft for Phase 3)

| Option | Capability | Security | Cost/complexity |
|---|---|---|---|
| A. Full dev env in scoped container, native tools ON, creds OUTSIDE (proxy/scoped delivery) | Full (node/go/py, git, tests, loopback) — PROVEN above | OS boundary (mount/net/UID ns, ro-root, caps, no-new-privs); same-UID-in-container reads only see non-secret mounts; creds never enter | New dev image + egress proxy + credential delivery; moderate one-time, low per-run (137 ms) |
| B. Provider-native sandbox only (Codex bwrap profiles) | Good for shell routes; MCP/resume gaps | Real for covered routes; NOT full (see Codex gaps); OpenCode: none | Low build, HIGH version-fragility (flag/profile drift per release) |
| C. Trusted control plane + isolated exec (broker-only tools) | POOR: fixed ops only; kills normal dev (no shell/git/tests) | Strong for covered ops; mediation ≠ OS boundary for bypass routes | Sunk (PoC exists) but wrong as universal default |
| D. Hybrid risk-adaptive (RECOMMENDED): A as STANDARD, C-pattern as RESTRICTED, audited host-assist as TRUSTED LOCAL | Full where needed, minimal where risky | Boundary matched to risk; each profile independently testable | Smallest set: 3 profiles, 1 common container substrate |

Common-substrate note: one rootless-Podman per-Run boundary serves all
profiles and all providers (no per-provider security architecture);
provider differences remain in *capability certification* (Codex
NEEDS-AUTH-PLAN, OpenCode BLOCKED), not in topology.

## Open technical questions

1. Credential delivery into containerized agent without same-UID exfil:
   short-lived scoped token via env + Codex `inherit none` + allowlist
   hiding auth paths, vs loopback credential-proxy holding real keys?
   Needs owner-approved auth test to decide (cannot test creds here).
2. Egress allowlist mechanism for registries (registry.npmjs.org, pypi,
   proxy.golang.org, model APIs): NAT + squid-like filtering proxy vs
   per-run `--network` profiles. Design + measure, no decision yet.
3. Custom dev image contents (node 22 + npm + git + go + python + build
   tools) and digest pinning; browser-test story (playwright deps vs
   external browser service).
4. Paseo change surface: exact narrow hook (if any) to bind session cwd
   into container + freeze policy across resume. Await surface file.
5. OpenCode path: hardened adapter (remove native routes at code level)
   vs credential-proxy topology vs continued BLOCKED. No code yet.

## Blocked tests (need owner approval)

- Any authenticated provider session probing credentials or bypasses.
- Pulling large new images is deferred (bandwidth), not blocked.
- VM-gated plans held by investigator records (Codex plan in findings
  tail; OpenCode plan §"Exact isolated authenticated test plan").

## Follow-up (2026-10-19, post-checkpoint)

- Surface report collected: `docs/security/SEC-01-streamA-paseo-surface.md`
  (provider launch = plain `child_process.spawn`, no isolation; plugins
  trusted-side; relay = transport; ACP file/terminal callbacks need
  redirect-or-disable).
- Validation spec: `docs/security/SEC-01-validation.md` (§3 scope
  correction, §4 STANDARD profile, §5 custody comparison + M4 deputy
  requirement, §6 repaired decision rule, §7 PR #4 plan, §8 auth plan).
- PR #4 retargeted to `main` (OPEN, MERGEABLE, head `7139c9b`; diff vs
  main = 9 files / +526 −35). No merge.
- Fresh Luna challenge: **CHALLENGE-FAIL**
  (`docs/security/SEC-01-luna-challenge.md`) — M4 deputy undesigned,
  §6 misuse-class gap, Codex inconsistency, Paseo prerequisite
  understated. Repairs integrated; auth test must target repaired design.
- Branch published: `origin/f1-s2-sec01-redesign-campaign`.
- SEC-01 BLOCKED. No authenticated tests. S3 not authorized.

## Original action list (superseded — all items completed above)

1. Collect `/tmp/sec01-paseo-surface.md` from `4140142a`.
2. Finalize §§3–4, write benchmark + acceptance spec.
3. Formalize `/tmp/streamB-dev` probe as `scripts/sec01-dev-container-probe.sh`.
4. Owner checkpoint report (§§A–J of mission): recommend D/A, PR #4
   disposition (KEEP/ADAPT/DEFER/SUPERSEDE), costs, risks, smallest next step.
5. NO product-code changes, NO PR modifications, NO Luna review needed
   (docs-only branch). Luna reserved for future implementation candidate.
