# Environment check workspace staging and failed-check diagnostics — 2026-11-11

**Status:** Implementer evidence under `TRUSTED_LOCAL`. This is not independent
acceptance and does not claim a full Nix/devenv check. No paid Factory inference
was used.

**Branch:** `fix/live-check-reliability`, base `fdb8ab7`. The exact candidate is
the commit containing this record.

## 1. Observed failure

A live campaign's frozen `npm test` check returned exit 254 while a manual
builder check with the exact v16 spec/receipt and the plain base project passed.
A separate frozen `format-delivery-acceptance` check exited 1 with no stderr,
so the Implementer had no diagnostic beyond the check id.

Real npm produces exit 254 for a specific, reproducible condition:

```text
npm error code ENOENT
npm error syscall open
npm error path /cache/check-workspace/package.json
npm error enoent Could not read package.json: Error: ENOENT: no such file or directory
```

## 2. Root cause

The Supervisor's `POST /v1/workspaces/{workspace_id}/environment-check` route
resolved the durable workspace volume and forwarded it to the broker without
materializing the current Attempt worktree. Allocation creates an **empty**
volume; only generic `run` and dependency-acquisition operations refresh it.
When a frozen check was the first operation on the workspace (or followed
`write_files`, which only changes the host worktree), the builder copied an
empty `/inputs/workspace`, wrote the generated environment files, and ran
`npm test` with no `package.json`. npm exited 254. The same empty/stale
workspace explains the silent `format-delivery-acceptance` exit 1: its command
had no project file to inspect.

The Git worktree itself was not the cause. The workspace archive already
excludes `.git`, and a staged worktree check passes exactly like the plain
directory.

## 3. Reproduction in the real builder image

`scripts/environment-check-reproduce.sh` uses the real
`localhost/software-factory-environment-builder:dev` image, the real
`prepare_workspace` copy, the real bounded `run_check` runner and real npm. It
creates a real Git worktree and compares:

- an allocated but **never staged** empty workspace volume;
- the Git worktree candidate;
- the plain-directory base project.

Nix environment realization is bypassed so the harness is offline and
deterministic; the live campaign had already realized its environment before
npm failed. This harness is therefore not evidence for a full Nix/devenv check.

Command:

```bash
TMPDIR=/home/james/.cache/software-factory/repro-tmp \
  bash scripts/environment-check-reproduce.sh
```

Result (full output in [`reproduction.log`](reproduction.log)):

| Case | Exit | `package.json` |
|---|---|---|
| empty, never staged workspace volume | 254 | absent |
| Git worktree candidate | 0 | present |
| plain-directory base project | 0 | present |

The empty case's stderr contains `Could not read package.json` and the exact
`/cache/check-workspace/package.json` path. The worktree and plain copies
contain no `.git`, matching the archive contract.

## 4. Product fix

- `WorkspaceManager::stage_workspace_for_check` snapshots the exact current
  worktree, **removes and recreates the recorded broker volume**, and imports
  the archive under the manager lock, verifying the worktree device/inode
  identity. Podman `volume import` merges into an existing volume, so import
  alone leaves files that are no longer in the worktree; removal/recreation
  prevents that. The backend `create_volume` re-applies the owner/workspace
  labels. The Supervisor calls it before calling the broker.
- `EnvironmentCheckSpec`, the builder `check-result.json`, and
  `EnvironmentCheckResult` now carry the frozen `SpecCheck` description.
- The builder emits one redacted, bounded `stderrTail`; the broker builds a
  `diagnostic` of the form
  `"<description> (exit N, expected M): <last redacted stderr line>"` bounded to
  1,400 characters. The Worker tool result, the automatic-final summary and the
  Verifier failure surface it even when the command writes nothing.
- Source bodies and full stderr are never copied into the diagnostic.
- Environment checks now run under the Supervisor operation fence. The manager
  registers an active check by workspace/volume; `cancel_active` calls the
  broker's `POST /v1/environments/check/cancel`, which resolves the broker-owned
  `factory-env-*` check container, removes it, confirms absence, and clears the
  active registration. A second concurrent check is refused, and the handler
  returns `ENVIRONMENT_CHECK_CANCELLED` when the grant is revoked. The bounded
  timeout and no-host/security rules are unchanged.

## 5. Deterministic evidence

- `cargo test -p factory-native --bin factory-supervisor environment_check` —
  4 passed, including
  `environment_check_stages_the_git_worktree_before_the_broker` (real Git
  worktree, real archive/import, stale-file removal, fake broker returning
  npm's 254 when `package.json` is absent and PASS after staging) and
  `environment_check_fence_cancels_an_active_check_on_revocation`.
- `cargo test -p factory-native --bin factory-runtime-broker environment_check`
  — 9 passed, including `environment_check_cancel_targets_only_an_active_owned_check`;
  `check_diagnostic_is_description_first_and_bounded` — 1 passed.
- `FACTORY_REAL_PODMAN_TESTS=1 cargo test -p factory-native --test
  real_podman_environment_check_staging -- --nocapture` — 1 passed. A real
  rootless Podman volume is staged, `old.txt` is deleted from the worktree and
  `modified.txt` changes; after restaging `old.txt` is gone and the modified
  content is present.
- `cargo test -p factory-native --bin factory-worker environment_check` — 1
  passed; frozen-check diagnostic unit test passed.
- `cargo test -p factory-native --bin factory-verifier builder_check` — 1
  passed.
- `python3 -W error::ResourceWarning -m unittest discover -s deploy/environment-builder/tests`
  — 69 passed, including description bounds, cwd, timeout, offline npm cache
  and bounded redacted stderr tail.
- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p factory-native --all-targets --all-features -- -D warnings`
  — clean.
- `podman compose -f compose.yaml config --quiet` — exit 0.
- `factory-architecture-check --root .` — PASS.
- Office parity check — PASS; `impact: none` because no Office route, control or
  projection changed.

## 6. NOT_TESTED / residual

- A full live Compose environment check with a real Nix/devenv environment and
  the exact v16 spec/receipt was not rerun; no paid inference was used.
- The live `software-factory` stack, `/data` and the acceptance DB were not
  touched.
- This evidence is implementer self-test, not independent acceptance or a
  Quality Gate decision.
