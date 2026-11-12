# Stage-addressed quality harness

**Status:** implemented development/test infrastructure. It is not part of the
installed Factory runtime, not independent acceptance and not a security
certification. `TRUSTED_LOCAL` remains broad-access and is never described as
isolated.

## Purpose

Make the full-flow Compose journey cheap to re-run, resumable after a late
failure, explicit about non-deterministic boundaries and useful as evidence
without a paid model call.

## Commands

```sh
python3 scripts/e2e/scenario.py --list
python3 scripts/e2e/scenario.py --doctor
python3 scripts/e2e/scenario.py --from prepare --to gate --keep
python3 scripts/e2e/scenario.py --from gate --to summary \
  --run-dir /var/tmp/factory-e2e/<run-id>
python3 scripts/e2e/scenario.py --provider cassette \
  --cassette scripts/fixtures/<recorded>.json
```

The compatibility entrypoint is `scripts/compose-e2e.sh`. `cargo xtask e2e
scenario -- <args>` forwards the same arguments.

`--provider fixture` is the default and uses the deterministic local model
fixture. `--provider cassette` validates a provider cassette and mounts it as
the model fixture's replay input; a request whose canonical body digest does not
match a recorded turn fails closed. `toolCalls` in a cassette is advisory
trajectory metadata; it is not executed and is not proof that a tool ran.
`--provider real` is rejected: a real provider run requires a fresh owner
authorization envelope and is not part of this harness. `--github fake` is the
only supported GitHub mode; Core-owned real delivery is a separate authorized
lane.

The runner does not execute the real-Chromium Office journey. Run
`cargo xtask e2e office` separately in the canonical containerized development
environment; the scenario runner must not start candidate build/browser code
with the host environment. A run directory contains a `run.json` ownership
marker; cleanup/`--fresh` refuse an unowned directory. `--allow-dirty` permits a
diagnostic run but the manifest is `PARTIAL`, never `PASS`.

## Stages

| Stage | Responsibility |
|---|---|
| `prepare` | disposable Git fixture, model context/cassette, run layout |
| `start` | Compose build/start and all readiness boundaries |
| `enroll` | first-run owner enrollment and Worker registration |
| `import` | import the Git project and freeze project config |
| `freeze` | freeze the Action and functional/negative checks |
| `delegate` | bounded owner delegation |
| `settings` | deterministic gateway, execution profile, project policy |
| `intake` | create the unified intake request and model context |
| `assess` | Lead assessment and frozen plan hash |
| `approve` | approve the exact plan and bind the Campaign |
| `gate` | candidate, independent receipts and deterministic Gate |
| `restart` | restart without volumes and verify persistence |
| `summary` | candidate-bound summary |

A selection that stops before `summary` is `PARTIAL`, never `PASS`. A missing
tool, unknown provider outcome or missing authorization is `BLOCKED`/
`NOT_TESTED`, never a silent pass.

## State and artifacts

- Run directory: `--run-dir` under `FACTORY_E2E_ROOT` (default
  `/var/tmp/factory-e2e`) or `/var/tmp/factory-e2e/<run-id>`.
- State: `<run-dir>/state.json`; resuming requires the prior identities.
- Artifacts: `--artifacts` or `target/qh/<run-id>`.
- `manifest.json` records candidate/base SHA, stages, results, artifacts,
  `paidInference:false`, `TRUSTED_LOCAL`, `isolated:false`, `certified:false`
  and limitations.
- `logs/commands.log` contains every Compose command and its exit status.
- `diagnostics/` contains Compose/Podman state, service log tails and the latest
  Core projections. Secrets and raw prompts are not intentionally written.
- `--snapshot` exports Compose volumes and an index; `--restore` imports them
  before `start`. The index is validated for project, duplicate/unexpected
  volume names and archive confinement before any destructive step; foreign
  volume ownership is preflighted before `down -v`. Snapshot/restore is best-effort and
  must be verified; it is not a backup certification. Snapshots contain
  disposable E2E configuration/credentials and are written with private
  permissions; do not publish them.

## Multi-agent execution

The reusable OpenCode role definitions live in `.opencode/agents/qh-*.md`:
implementers use DeepSeek V4.1 Flash, reviewers/verifiers use GPT-6 Luna and
editors are restricted to disjoint paths. Reviewers have edit denied and shell
limited to `git diff/show/status`; the verifier may run the exact test commands
but cannot edit. These are development-time definitions, not Factory runtime
agents and not authority grants. If a needed role is not discovered by the
`subagent` tool, stop and fix discovery; do not fall back to an unrestricted
`general` agent for a read-only review.

## Determinism rules

1. Project, port, volumes, temp dirs, Git fixture and browser profile are unique
   per run. The shared `software-factory` project is refused.
2. Fixed model fixture; no paid inference; no GitHub contact.
3. Typed JSON assertions, not `grep` over a projection.
4. Every wait has a deadline. A hang is a failure.
5. New/changed Playwright specs run 20 times and fail on flaky results.
6. A test that passes only on retry is a failure, not a pass.
7. Cleanup is owner-scoped; a failed run is not deleted before diagnostics.
8. The exact candidate SHA and the exact commands are recorded.

## MCP boundary

The harness and the development agent may use Playwright MCP, Chrome DevTools
MCP or Context7 as authoring/diagnostic aids. The Factory runtime does not expose
raw MCP servers to Worker, Reviewer or Verifier. A capability that a runtime
agent genuinely needs is added as a typed, Core-authorized tool in the compiled
tool pack; MCP may be an implementation detail behind that tool, never a second
authority path. Reviewers and verifiers use the frozen candidate, checks and
evidence, not live exploratory tools.

## Local/CI lanes

```sh
bash scripts/ci/quality-harness.sh       # Python runner + Office checks
QUALITY_HARNESS_SKIP_BROWSER=1 bash scripts/ci/quality-harness.sh
bash scripts/ci/dependency-policy.sh     # licenses/bans/sources, no advisories
bash scripts/ci/semgrep.sh               # narrow security rules
bash scripts/ci/mutation-sample.sh       # PR-scoped mutation sample
```

`cargo audit` remains unsuppressed. `RUSTSEC-2026-0459` is a known dependency
blocker owned by the Product Owner; `cargo-deny` advisories are not enabled
until it is resolved.

## Evidence template

```text
Candidate:  <SHA>
Scenario:   <name and selected stages>
Result:     PASS / PARTIAL / FAIL / BLOCKED
Provider:   fixture / cassette (never "real" without an envelope)
Browser:    none / chromium
Commands:   <exact commands>
Artifacts:  <manifest, logs, diagnostics, snapshots>
Negatives:  <invalid resume, bad cassette, missing tool, timeout>
Limitations:<NOT_TESTED / TRUSTED_LOCAL / no isolation claim>
```
