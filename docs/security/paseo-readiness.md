# Paseo TRUSTED_LOCAL readiness (Agent C) — 2026-10-19

**Disposition: SMOKE PASS (functional trusted-local workflow only). NOT CERTIFIED** for isolation, S3/unattended execution, or delivery. This record covers the owner-accepted TRUSTED_LOCAL topology: existing local Paseo daemon, supported SDK/CLI, native provider integrations, existing authentication, normal network. No proxies, brokers, or forks were used.

**Evidence provenance:** `paseo --version`, `paseo status`, `paseo provider diagnostic <codex|opencode> --json` (names/presence only — no secret values read), plus one bounded real-use run of `scripts/paseo-trusted-local-smoke.sh` with `PROVIDER=codex` on 2026-10-19. A first attempt failed honestly (agent ran in the caller workspace, no diff); the script was fixed to pin the disposable absolute path in the prompt, and the rerun passed. Raw timelines and auth files are **not** committed; observations below are redacted summaries.

## Versions observed

| Component | Observed |
|---|---|
| Paseo CLI / local daemon | 0.9.1 (daemon `running` on 127.0.0.1:6767) |
| codex CLI | `codex-cli 0.156.1`, diagnostic `Ready`, 6 models |
| opencode CLI | 1.18.32, diagnostic `Ready`, 151 models, credentials present (count only) |
| Node.js / npm | v22.23.2 / 12.1.0 |
| Model that ran the smoke | `codex/gpt-6-astra` (reported by `paseo inspect` as model `gpt-6-astra`, thinking `low`) |

## Provider availability

| Provider | Available | Notes |
|---|---|---|
| codex | **true** | Ran the smoke session (agent `7632c120-…`, status `idle` → stopped/archived) |
| opencode | **true** (diagnostic) but **session start FAILS** — see below | Diagnostic Ready; two session attempts failed before any model turn (no substitution made) |
| claude / copilot / pi | false | Unavailable in this daemon; untouched |

## What the smoke test proved

Functional trusted-local workflow with the default provider (`codex`), all in a disposable dir (`/tmp/paseo-trusted-local-*/proj`, `git init`, `add.js` + `add.test.js` + `package.json`):

- **Session created:** `paseo run --provider codex --wait-timeout 300s --json` returned agent `7632c120-167c-4459-a394-e1ed0f7cc524`.
- **Session observed:** `paseo inspect --json` reported `Provider: codex`, `Status: idle`; `paseo logs` showed 11 redacted timeline lines (`[User]` ×1, `[Shell]` ×3, `[Edit]` ×1) ending in a ≤10-line summary.
- **Source change made by the agent:** `git diff --stat` → `add.js | 8 +++++++-` (289 bytes), containing the expected `multiply` addition; `add` left unchanged.
- **Formatter + tests pass:** `npm run format` exit 0; `npm test` exit 0 (`# pass 1 # fail 0`).
- **Session ended:** `paseo stop` exit 0, `paseo archive` exit 0; trap cleanup removed the TMP dir (verified absent).
- **Containment evidence (narrowed — not a proof of no-outside-writes):**
  the session runs in a real Paseo workspace rooted at the disposable
  target with session Cwd asserted equal to it (hard FAIL otherwise);
  additionally the agent's file-write timeline steps referenced only paths
  under the disposable dir, and `git diff`/`status` showed the change
  confined to the disposable repo. Shell-output paths, relative paths, and
  timeline-invisible writes are NOT covered by these checks.
  (`paseo run --cwd` alone does not set the session Cwd — observed.)

- **OpenCode Go routes pass the same loop:** `opencode-go/muse-spark-1.3-contributor`
  (agent `a6e54a0f`) and `opencode-go/deepseek-v4.1-flash` (agent `30c28243`),
  each with effective provider/model asserted from session evidence, 0 nudges,
  full edit → format/tests → diff → stop/archive/workspace-deregister.

Two honest behavioral notes: (1) the agent-scoped `paseo run --cwd <tmp>` did **not** change the agent's `Cwd` (inspect showed the caller workspace); the script now creates a real workspace rooted at the target and asserts Cwd instead. (2) Attempt 1 (agent `ca05f95e-…`) correctly reported "Files changed: none" and the script failed as designed; no failure was masked.

## What it did NOT prove

- **opencode routes: session launch works; full edit/test loop NOT completed.**
  (a) GitHub-Copilot-backed models are session-start BLOCKED in this
  environment: `github-copilot/gemini-4-argon` → 400 `model_not_supported`;
  pinned `github-copilot/gpt-5-mini` → errored in <1s with `LastUsage: null`.
  Diagnostic still reports available:true — failure is at session/model
  entitlement, not installation.
  (b) OpenCode Go subscription models: FULL LOOP PASS after the workspace
  fix. Root cause of the earlier read-only stalls: sessions ran with Cwd
  in the caller workspace (`paseo run --cwd` does not set session Cwd —
  verified), so target files fell under OpenCode's `external_directory =
  ask` and the models read but never completed edits. Mode selection was
  ruled out (unset/default mode maps to OpenCode's own default agent;
  the owner config sets no custom default) and tool exposure was ruled
  out (a direct `opencode run` in the target dir edited immediately).
  The smoke script now creates a real Paseo workspace rooted at the
  disposable target and asserts session Cwd equals it. Passing runs,
  0 nudges, effective provider/model asserted from session evidence:
  `opencode-go/muse-spark-1.3-contributor` (agent `a6e54a0f`),
  `opencode-go/deepseek-v4.1-flash` (agent `30c28243`). Earlier partial
  sessions (`b8ff78c9`, `b663df06`, `4028ca17`, `0dcdeffb`) are superseded
  by these passes, retained here as diagnosis history.
  Related codex finding fixed in the same repair: fresh disposable
  workspaces are untrusted, so codex mode `auto` gates shell/edit behind
  a pending permission (observed `CodexBash` request, run status
  `permission`); the smoke defaults codex to `full-access`
  (TRUSTED_LOCAL-appropriate; target stays disposable and post-hoc
  verified). Codex re-passed (`871e2251`, 0 nudges).

- **Isolation:** no sandbox boundary, read boundary, or credential-confinement claim — SEC-01 remains paused, not approved.
- **S3 adapter / unattended campaigns:** one short attended run only; no scheduling, long campaigns, or production actions.
- **Delivery:** no merging, no production writes, no certification language (`PASS` here means the script's own assertions only).

## Rerun command

```bash
PROVIDER=codex ./scripts/paseo-trusted-local-smoke.sh
# with explicit model pin (asserted against session evidence, no substitution):
PROVIDER=codex MODEL=gpt-6-luna ./scripts/paseo-trusted-local-smoke.sh
# OpenCode Go subscription routes (workspace-selected, MODE=build default):
PROVIDER=opencode MODEL=opencode-go/muse-spark-1.3-contributor ./scripts/paseo-trusted-local-smoke.sh
PROVIDER=opencode MODEL=opencode-go/deepseek-v4.1-flash ./scripts/paseo-trusted-local-smoke.sh
# GitHub-Copilot-backed models are NOT runnable here (session-start failure,
# see above); do not substitute them for the commands above.
```

Script: `scripts/paseo-trusted-local-smoke.sh` (`bash`, `set -euo pipefail`, disposable `mktemp -d` + `trap` cleanup, `PROVIDER` override `codex|opencode`). Test target is always a fresh disposable repo under `/tmp`; the factory repo itself is only read for the script path. No secret values are inspected or logged.
