# SEC-01 Architecture Redesign — decision-ready report (2026-10-19)

Campaign branch: `f1-s2-sec01-redesign-campaign` (base `7139c9b` = PR #4
candidate). PR #3 MERGED. PR #4 OPEN/MERGEABLE, NOT merged, NOT certified.
SEC-01 remains BLOCKED for credential-bearing execution. No S3. No
authenticated adversarial tests were run. Evidence: credential-free only,
except version-pinned source reading.

Prior S2-B evidence reused (not repeated): `/tmp/sec01-codex-findings.md`
(CODEX-NEEDS-AUTH-PLAN), `/tmp/sec01-opencode-findings.md`
(OPENCODE-BLOCKED). New 2026-10-19 evidence: `scripts/sec01-dev-container-probe.sh`
(ALL CHECKS PASSED), SDK/daemon probes in §C, checkpoint
`docs/security/SEC-01-redesign-campaign.md`.

## A. Recommendation

Adopt **D — hybrid, risk-adaptive execution on one common rootless-Podman
substrate**, with **A — fully isolated dev environment — as the STANDARD
default**:

- STANDARD (default): per-Run rootless Podman dev container. Native
  provider tools stay ENABLED inside. No host HOME/auth/sockets mounted,
  read-only root, dropped caps, no-new-privs, bounded CPU/RAM/PIDs,
  default-deny egress with allowlisted registry/proxy profile, per-Attempt
  workspace, lock-owned run roots + reaper (keep PR #4). Provider
  credentials NEVER enter the container as files/accessible env: Codex via
  scoped short-lived delivery + `inherit none` + allowlist hiding auth
  paths (to be proven in gated auth test); OpenCode EXCLUDED from STANDARD
  until its path is proven (still BLOCKED).
- RESTRICTED (high-risk/unknown code): existing S2-C broker pattern —
  fixed typed ops, workspace-bound, no general shell (keep PoC, narrow it).
- TRUSTED LOCAL (explicit owner-authorized host work): scoped, expiring,
  audited host exception. Never silent, never the default, never presented
  as sandbox-equivalent.

Why D/A wins: it is the only evaluated option that passes the container-level
engineering probes (§E: node tests, loopback, git, writable workspace —
all PASS in container) AND the isolation probes (canary invisible, ro-root,
no egress, no orphans) simultaneously on one substrate for all providers. B (provider-native) is
kept as defense-in-depth (Codex allowlist profile) but is version-fragile
and absent for OpenCode. C (broker-only) as universal default fails the
owner's capability test by construction.

## B. Technical comparison (evidence-backed)

| | A isolated dev env | B provider-native | C broker-only | D hybrid (recommended) |
|---|---|---|---|---|
| Agent capability | Container ops FULL (§E); session/toolchains/proxy/auth NOT tested | Good shell; MCP/resume gaps; OpenCode none | Fixed ops only — normal dev impossible | Full where needed |
| OS enforcement | mount/net/UID ns, ro-root, caps, seccomp, no-new-privs (measured) | Codex bwrap real but partial (gaps §F); OpenCode none | Container for fixed op; bypass routes unproven | Boundary per profile, one substrate |
| Paseo compat | External; no Paseo change needed (SDK has no sandbox opts — §C) | Depends on Paseo forwarding flags (unverified) | Needs Paseo mediation exclusivity (does not exist — §C) | PREREQUISITE: trusted launcher owning provider processes + resume freeze; without it, prototype only (challenge finding 4) |
| Multi-provider | Common topology; per-provider capability certification only | Per-provider sandbox engineering (fragile) | Per-provider route audit (large) | Common topology + per-provider cert matrix (§H) |
| Credential protection | Creds outside container (design; auth test pending) | Codex: filtered env + hidden paths (partial, pending); OpenCode: impossible stock | Broker holds no creds; but native routes bypass it | Per-profile custody; STANDARD = outside-container |
| Perf/ops cost | ~140 ms trivial-spawn; excludes pull/session (see validation §3) | ~0 infra; high upgrade-churn | PoC exists; high capability cost | spawn only; +1 image +1 proxy +3 profiles |
| Maintenance | Image + proxy + argv audit (extends PR #4) | Track every provider release's sandbox drift | Custom tool framework (owner rejected this direction) | Smallest capability/complexity ratio |

## C. Real process topology (measured 2026-10-19)

```text
Paseo daemon 0.9.1 (pid 4026, 127.0.0.1:6767)
 └─ daemon worker (pid 4037, SAME UID)
     ├─ codex app-server ×N (same UID; e.g. pids 4464, 25205…)
     ├─ opencode serve (same UID; e.g. pid 5550, --port 43827)
     └─ terminal-worker-process (same UID)
Factory Core (trusted) → Worker (trusted, scoped token, env allowlist)
 → per-Attempt rootless Podman (UNTRUSTED; no HOME/auth/socket/net,
    ro-root, caps dropped, no-new-privs, bounded, digest-pinned)
```

- Paseo SDK 0.11.4 `AgentSessionConfig`: provider/cwd/systemPrompt/
  modeId/model/thinkingOptionId/featureValues/providerOptions/
  `toolPolicy.preapproved` (MCP pre-approval only) / `mcpServers`.
  NO sandbox/env/UID/network/executor options
  (`protocol/dist/agent-types.d.ts:430-480`). `ProviderRuntimeSettings`
  `command/env/disallowedTools` and `AgentSessionConfig` tool hiding are
  advisory routing (hide from model), NOT OS enforcement.
- Server-side adapter source NOT shipped locally (0.9.1 ships `cli` only;
  spike SDK ships client/protocol/relay only) — unauditable internals are
  another reason the boundary must be EXTERNAL.
- Consequence: Paseo cannot impose the required boundary today; the
  Factory's Worker + Podman layer must. No Paseo fork needed for D/A;
  narrow Paseo cooperation wanted later (bind session cwd to container,
  freeze policy across resume) — pending surface agent confirmation.

## D. Execution profiles

STANDARD (default, untrusted ordinary dev): §A + per-Run identity,
digest-pinned dev image (node 22/npm/git/go/python/build tools),
loopback allowed, egress allowlist (registries + provider endpoints via
filtering proxy), resource caps, cancel/reap. AuthN: scoped short-lived
credential delivery; exact mechanism decided by gated auth test (§H).
OpenCode excluded until certified.

RESTRICTED (adversarial/unknown): S2-C broker pattern, fixed typed ops,
workspace-bound, traversal-rejected, no shell/tools, disposable identity,
tighter caps, no network except via broker. For code the model must not
touch directly.

TRUSTED LOCAL (owner-authorized only): explicit scope + expiry + audit
trail; separate approval; never silent escalation; documented as
non-equivalent to sandbox.

Profiles ≠ R0–R3 delivery risk: execution authority and merge/delivery
authority stay separate (MASTER §9). A STANDARD run producing an R3 diff
still goes through the R3 gate.

## E. Functional benchmark (disposable synthetic project)

`scripts/sec01-dev-container-probe.sh` — ALL CHECKS PASSED (2026-10-19):

- [PASS] node --test (1 pass) in container, read-only root, rw workspace
- [PASS] loopback HTTP `LOOPBACK:hi` with `--network=none`
- [PASS] unmounted outside canary INACCESSIBLE from container
- [PASS] root filesystem read-only; workspace writable, artifact on host
- [PASS] default egress denied (`registry.npmjs.org` UNREACHABLE)
- [PASS] kill + `rm -f`: 0 orphans; startup overhead 140 ms

Supported/tested: repo inspection, multi-file edit (workspace rw),
lint/test compile (node present), local servers + loopback, git (binary
present; commit/diff inside workspace — exercised in PoC fixture),
cancel/reap. NOT YET verified: package install (needs allowlist proxy),
Go/Python toolchains (need dev image), browser tests (needs playwright
story), crash recovery end-to-end, multi-provider real sessions. Synthetic
canaries: host creds n/a (none mounted — by construction), provider cred
files NOT YET (needs gated auth test), other-project paths (unmounted —
same mechanism as canary PASS), privileged sockets absent (S2-C battery),
env inheritance (S2-C CLEAN), subagent bypass (NOT YET — provider cert).

## F. Security guarantees (OS/runtime control per boundary)

1. Filesystem: unmounted paths are unreachable by construction (not by
   policy); ro-root + single rw `/workspace`; `--userns=keep-id` +
   `--user UID:GID` (stock paseo image entrypoint UID-drop incompatible —
   custom image must not UID-drop). Same-UID-in-container reads can only
   reach container mounts (no host secrets mounted).
2. Network: `--network=none` default-deny (measured UNREACHABLE);
   loopback works (measured). Allowlist = filtering proxy (to build).
3. Privilege: `--cap-drop=ALL`, `--security-opt=no-new-privileges`,
   `--pids-limit`, `--memory`, `--cpus` (all in S2-C argv audit).
4. Identity/lifecycle: per-Attempt workspace + receipt (run/attempt/
   generation); PR #4 lock-owned roots (kernel-released on SIGKILL) +
   concurrent-safe reaper; cancel → expiry → retry with fencing.
5. Provider defense-in-depth (Codex only): custom read allowlist
   (`:minimal` + workspace, no writes, no net, approval never, env
   inherit none) — OS denials measured (EROFS/ENOENT/EPERM). Legacy
   `read-only` FORBIDDEN as policy (leaks). OpenCode: no native control
   available — excluded from STANDARD.
6. What is NOT claimed: credential-file isolation with creds inside the
   same container (D2-C FAIL stands); broker-only exclusivity (no Paseo
   mechanism — §C); model refusal / hidden tools as boundaries (never).

## G. PR #4 disposition (independent of old assumptions)

- KEEP: Worker env allowlist + canary regression; fail-closed Podman
  default + explicit `--exec-mode local`; `podmanArgs()` pure builder +
  audit test; image digest evidence; lock-owned run roots + `!unix`
  fail-closed + graceful cleanup; S2-C PoC as RESTRICTED-profile basis;
  threat model + S2-D inputs (update to D/A).
- ADAPT: Podman defaults extended for STANDARD (dev image, `--user`
  UID:GID with keep-id, loopback documented, egress-proxy profile,
  toolchain mounts); broker PoC narrowed to RESTRICTED (not universal);
  `alpine:3.20` mutable tag → digest pin for fixture AND dev image.
- DEFER: per-provider certification (S2-D) to gated auth tests (§H);
  OpenCode hardened-adapter decision.
- REMOVE/SUPERSEDE: the "broker-only tools for all execution" assumption
  (superseded by A/D evidence); stale fallback comment in
  `core/cmd/factory-worker` (flag vs comment mismatch noted in Codex E17);
  nothing else — no functioning safeguard is weakened.

Smallest implementation delta on top of PR #4: (1) dev image
Dockerfile + digest pin; (2) STANDARD argv profile + audit test;
(3) egress-proxy (allowlist) design; (4) credential-delivery mechanism
(decided by §H test); (5) benchmark/acceptance spec; (6) docs update.
S2 infra vs S3 orchestration stays separated: all of the above is S2
isolation; no Paseo session wiring until §H passes.

## H. SEC-01 acceptance criteria (finite, per provider/runtime)

Matrix rows: {Codex 0.156.1+bwrap 0.13.1, OpenCode 1.18.32} ×
{STANDARD container, RESTRICTED broker}. Overall PASS requires per-row
PASS for every ENABLED row; uncertified rows stay disabled (OpenCode
STANDARD starts disabled).

Per enabled row: (1) real Paseo SDK session does scoped edit + test
through the profile; (2) every enabled route probed with synthetic
canaries (host cred path, other-project path, provider cred file,
forbidden egress, privileged socket, env-name leak, resume-with-weaker-
policy, subagent-escape, tool-bypass) with OS-denial oracles
(EACCES/ENOENT/EPERM), never model prose; (3) cancel/restart/reap +
lease-fencing + no duplicate effects; (4) evidence bound to exact
candidate SHA + image digests + policy digest; (5) independent verifier
+ Luna review. Unknown/unverified capability = restricted until
validated. Full gated plans already held: Codex (findings tail),
OpenCode ("Exact isolated authenticated test plan").

## I. Implementation plan (small tasks, S2 infra only)

1. Dev image + pin (Dockerfile: node 22, npm, git, go, python, build
   tools; no UID-drop entrypoint; digest in code + docs). Deps: none.
   Complexity S. Acceptance: probe §E passes on dev image; digest test.
2. STANDARD argv profile + audit test (extend `podmanArgs`: dev image,
   `--user`, proxy env, keep S2-C restrictions; unit-lock). Deps: 1. S.
3. Egress allowlist proxy (design + disposable test: registry install
   works, metadata endpoint blocked). Deps: none (parallel). M.
4. Credential-delivery decision + gated auth test execution (owner
   approves identity/budget/image/egress/matrix first). Deps: 1–3. M.
5. Provider certification runs per §H (Codex STANDARD first; OpenCode
   STANDARD only via hardened-adapter or proxy topology). Deps: 4. M.
6. RESTRICTED profile extraction from S2-C PoC + threat-model update to
   D/A. Deps: none. S.
7. Benchmark spec (`scripts/sec01-benchmark.sh`: §E items 1–11 +
   canary matrix) + docs. Deps: 1–2. S.

## J. Independent review and decision

- Blockers: (a) credential-delivery mechanism unproven (needs §8 test,
  now with M4 deputy requirements); (b) egress proxy undesigned (domain
  allowlist alone insufficient — operation scoping required);
  (c) OpenCode STANDARD has no viable stock path (BLOCKED stands; M4
  needs full bypass-surface disables at the Core boundary);
  (d) Paseo session binding + resume freeze are PREREQUISITES for any
  Paseo-path claim (trusted launcher owning provider processes; without
  it, prototype only); (e) Luna independent challenge returned
  CHALLENGE-FAIL (`docs/security/SEC-01-luna-challenge.md`): M4 without
  deputy semantics, §6 insufficiency for misuse-class, Codex topology
  inconsistency — repairs integrated into `SEC-01-validation.md`;
  auth test must target the repaired design.
- PR #4: REVISE (ADAPT per §G) then merge AFTER PR #3 — do NOT merge
  as-is as universal-broker architecture; do NOT discard (KEEP items are
  the foundation). Retarget to main after #3 (already merged — rebase
  needed since base is `f1-s2-worker-dispatch`).
- S3 evidence still required: §H Codex STANDARD row PASS + Luna review +
  owner authorization. Nothing here certifies SEC-01.
- Review status of THIS report: implementer self-probe (§E script,
  rerunnable); Luna review RESERVED for the implementation candidate
  (this branch is docs + 1 PoC script, no product change — no Luna
  needed yet); independent verifier required for §H, not for this paper.
