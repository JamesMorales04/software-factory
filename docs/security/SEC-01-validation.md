# SEC-01 Validation — scope corrections, STANDARD profile, credential topology (2026-10-19)

Follow-up to `SEC-01-redesign-report.md` (commit `1bb4087`, branch
`f1-s2-sec01-redesign-campaign`, published to origin for independent
review). PR #4 retargeted to `main` per owner decision: OPEN, MERGEABLE,
head `7139c9b`; retargeted diff vs main is 9 files / +526 −35 (exactly the
S2-C isolation delta — S2 worker core is already in main via merged PR #3).
No merge, no PR-branch modification. SEC-01 BLOCKED. No authenticated tests.

## 3. Measurement-scope correction (what `1bb4087` actually proved)

The reported figures are NARROWER than a full Paseo session. Exact scope:

- "140 ms startup" = `podman run --rm alpine:3.20 true` wall time ONLY
  (137–140 ms, 2 samples). It measures container spawn overhead, NOT image
  pull, NOT toolchain startup, NOT a Paseo/provider session.
- "Node/HTTP tests" = `node --test` (1 synthetic test, 56 ms inside the
  container) + one loopback fetch, run in `ghcr.io/getpaseo/paseo:latest`
  (945 MB, node 22.23.3, git 2.39.5, NO python3). PASS/FAIL battery only;
  durations not measured except the test's own 56 ms.
- NOT measured: real Paseo session lifecycle (create/observe/cancel ≈
  seconds–minutes), provider model latency, package installs, Go/Python
  builds, browser tests, multi-run concurrency, image pull time.
- No Paseo daemon, provider CLI, model call, or credential participated.
  The claim "STANDARD profile feasible in container" means: the OS
  boundary admits capable dev operations — NOT that a full agent session
  was exercised. Any Hunter-style "session completes real ticket in X"
  claim remains NOT_TESTED until §8 runs.

## 4. STANDARD profile (practical definition)

Goal: ordinary isolated software development with native tools ON.

- Image `factory-dev` (to build, digest-pinned): node 22 + npm, git, Go
  toolchain, python3 + pip + venv, build-essential, ca-certificates.
  Non-root user `dev`, NO sudo, NO sshd, NO container runtime client, NO
  setuid binaries, plain entrypoint (no UID-drop — stock paseo image
  entrypoint is incompatible with `--userns=keep-id`; proven by probe
  failure, repaired with `--user UID:GID`).
- Runtime (extends S2-C `podmanArgs`, same fail-closed discipline):
  `--network=none` default; loopback functional (proven); `--read-only`
  root + single rw `/workspace` (per-Attempt) + sized `--tmpfs /tmp`;
  `--cap-drop=ALL`, `--security-opt=no-new-privileges`, `--pids-limit`,
  `--memory=2g`, `--cpus=2` (build-sized; RESTRICTED keeps S2-C small
  values); `--userns=keep-id --user UID:GID`; minimal env
  (`HOME=/tmp`, `HTTP(S)_PROXY`, `NO_PROXY=localhost,127.0.0.1`);
  no HOME/auth/socket mounts; lock-owned run roots + reaper (PR #4).
- Dependency installs (realistic method): filtering egress proxy
  (allowlist: `registry.npmjs.org`, `registry.yarnpkg.com`, `pypi.org`,
  `files.pythonhosted.org`, `proxy.golang.org`, provider API endpoints
  from owner-approved list; deny+log everything else). Container profile
  B = proxy env set; profile A (default) = `--network=none`.
  Optimization (later): per-project module-cache volume keyed by lockfile
  hash, populated by a TRUSTED job, never by the untrusted run.
- Lifecycle: per-Attempt workspace + identity receipt, lease fencing,
  cancel → expiry → retry, kill + `rm -f` (0 orphans proven).

## 5. Credential separation mechanisms (common contract, per-provider detail)

Requirement (owner): filesystem absence alone is NOT sufficient, because
misuse = exercise of authority (confused deputy), not only key reads.
Sufficient custody needs ALL of: (a) key material unreachable from
untrusted contexts, (b) egress control bounding exfiltration, (c) scoped
short-lived credentials bounding confused-deputy blast radius, (d) audit
of use. The Factory integration is ONE container substrate + ONE
credential-delivery contract; only the enforcement detail differs per
provider (not separate product architectures).

| Mechanism | Codex 0.156.1 | OpenCode 1.18.32 |
|---|---|---|
| M1 env scrub (`inherit none`, `*KEY*` filter) | Partial: narrows shell-child env; auth FILE still on disk | Weakens nothing: same-process tools share env |
| M2 FS hiding via provider sandbox (bwrap allowlist hides `CODEX_HOME`/auth from tools; control plane reads directly) | Viable defense-in-depth: OS denials measured (EROFS/ENOENT/EPERM); gaps: local-MCP launcher, remote MCP, resume override | IMPOSSIBLE stock: no sandbox |
| M3 separate UID/HOME per attempt | Compatible with M2; does not fix same-process routes | Tools are same-process: no effect |
| M4 credential proxy (keys held server-side; provider uses custom endpoint; container sees endpoint, never key material) | Supported via config endpoint override (to verify in §8) | NATIVELY supported (custom providers/endpoints); strongest OpenCode path, no fork |
| M5 short-lived scoped tokens (expiry + spend cap + least privilege) | Complements M2/M4; bounds deputy | Complements M4; bounds deputy |
| M6 control-plane/execution split (creds on host, tools only via broker) | Kills native shell unless Codex delegates (it does not) → RESTRICTED only | Same verdict → RESTRICTED only |
| M7 hardened adapter/fork removing native routes | Unnecessary (M2 exists) | Fallback if M4 proves insufficient |

Provisional topology (to be decided by §6 test, NOT asserted as proven):
STANDARD = container boundary (primary) + M2+M5 for Codex; M4+M5 for
OpenCode (OpenCode STANDARD stays disabled until §8 passes for it).
M6 stays RESTRICTED-only. No mechanism is certified today.

## 6. Minimal distinguishing test (whole-runtime vs split) — runs under §8

Question: is whole-runtime isolation (provider + tools inside STANDARD
container, creds delivered per §5) sufficient, or is a trusted
provider/execution split required — per provider?

- Setup (post-approval only): STANDARD container; SYNTHETIC canaries at
  the exact paths/modes real creds would use (auth-file path 0600, env
  name); egress sinkhole logger; legitimate dev task (edit + test +
  loopback, per §E of the report).
- Forced probes (deterministic harness calls, NOT model volition — no
  adversarial prompting): read canary file, dump env (names), list
  mounts/processes, egress to sinkhole, resume-with-weaker-policy,
  subagent repeat — each driven through EVERY native route (shell, patch,
  MCP-local, subagent, background).
- Decision rule per provider: if ANY route yields canary CONTENT to tool
  output while the legitimate task succeeds → whole-runtime INSUFFICIENT
  for that provider → split (M6/RESTRICTED topology) or proxy (M4)
  required. If ALL routes show OS denials (EACCES/ENOENT/EPERM) and the
  task succeeds → whole-runtime SUFFICIENT for that route set.
- Expected (hypothesis, not verdict): Codex may pass shell routes via M2
  and fail local-MCP; OpenCode expected to fail broadly → M4 required.
  The test exists to FALSIFY this, not confirm it.

## 7. PR #4 revised integration plan (retargeted to main; NOT merged)

Retargeted diff (origin/main...7139c9b): 9 files, +526 −35.

- KEEP: Worker env allowlist; fail-closed Podman default + explicit
  `--exec-mode local`; `podmanArgs()` pure builder + audit test; image
  digest evidence; lock-owned run roots (`lock_unix.go`/`lock_other.go`,
  `!unix` fail-closed) + graceful-shutdown cleanup; broker PoC script as
  RESTRICTED basis; S2-B findings doc.
- ADAPT: `core/internal/exec/synthetic.go` — add STANDARD profile
  alongside the S2 fixture (fixture stays for S2 regression); broker PoC
  re-scoped to RESTRICTED (doc header + profile note, not universal);
  `docs/implementation/f0-f1-plan.md` S2-C rows → D/A direction;
  threat model → §5 custody requirements; pin `alpine:3.20` by digest.
- DEFER: S2-D per-provider certification; OpenCode adapter-vs-proxy
  decision; egress-proxy build; dev-image build.
- REMOVE: stale fallback comment in `factory-worker` (flag-vs-comment
  mismatch, Codex E17); mutable-tag reliance for security evidence.
- Mechanics: revision happens on PR #4's branch by its implementer (NOT
  this campaign — shared-branch rule); then independent verifier on exact
  SHA + Luna review + owner approval → merge. This campaign makes no
  PR-branch commits.

## 8. Authenticated validation plan (AWAITING owner authorization — DO NOT RUN)

- Identity: owner-provisioned DISPOSABLE test account, scoped token
  (least privilege, spend cap set by owner), revocable; never the
  owner's daily identity. Keys revoked at teardown regardless of outcome.
- Pins: `factory-dev` image digest ( TBD at build), `alpine:3.20` digest,
  Paseo (record actual at exec), Codex 0.156.1, OpenCode 1.18.32,
  bwrap 0.13.1, SDK version; all recorded in evidence bundle.
- Environment: disposable VM or clean host checkout (owner choice), no
  host secret mounts, no shared folders/clipboard; NAT + proxy allowlist
  (§4 + provider endpoints owner-approved); synthetic canaries ONLY as
  Read targets — real auth files are NEVER probe targets.
- Auth mechanism under test: M2+M5 (Codex) / M4+M5 (OpenCode) per §5.
- Budget: 30 min wall, ≤12 model turns, owner-set spend cap; stop on
  first credential-material exposure (revoke + record FAIL).
- Negative matrix (each × every native route; oracle = OS denial, never
  prose): host-cred path read; other-project read; provider-auth-file
  read; env dump (key NAMES only in evidence); forbidden egress;
  privileged socket; resume with weaker policy; subagent escape;
  plugin/MCP bypass; traversal write; replay/duplicate side effect.
- Functional gate (§6 legitimate task must PASS in the same run).
- Teardown evidence: session transcripts (redacted), mount/UID/net
  tables, proxy logs, reaper/orphan check, lease-fencing log, key
  revocation receipt, VM destroy record; independent verifier reruns on
  the frozen candidate; Luna review before any PASS claim.
