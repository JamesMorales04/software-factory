# F1-S3 implementation brief — real Paseo sessions in Factory Runs (DRAFT, NOT STARTED)

Status: planning brief only. No code, no branch, no schedule. S3 starts only
after explicit owner authorization. TRUSTED_LOCAL-only scope; S3-ISOLATED
keeps its separate certification path (ADR-0009).

## Objective

Execute real coding work inside Factory Runs through actual Paseo sessions,
preserving every S2 guarantee (identity, leases, fencing, recovery,
evidence) while adding only the minimal Paseo session binding the Core
does not already own. Do NOT duplicate Paseo's agent runtime, tool
execution, session persistence, or model routing.

## Supported provider/model set (initial)

- Codex GPT-6 Luna (`codex/gpt-6-luna`).
- OpenCode Go Muse Spark 1.3 Contributor (`opencode/opencode-go/muse-spark-1.3-contributor`).
- OpenCode Go DeepSeek V4.1 Flash (`opencode/opencode-go/deepseek-v4.1-flash`).

All three completed the TRUSTED_LOCAL smoke loop (read → edit →
format/tests → diff → stop/archive) with asserted session Cwd, provider
and model identity (see `docs/security/paseo-readiness.md`). No other
provider/model is in S3 scope until it passes the same smoke.

## Architecture (minimal binding, not a runtime fork)

- Core owns: Run/Attempt identity, lease generations, fencing, budgets,
  authorization policy, evidence bundle, delivery decisions.
- Worker owns: lease polling/heartbeat/reporting, per-Attempt workspace
  lifecycle (existing S2 machinery, reused unchanged).
- Paseo owns: agent session lifecycle, native tools, timeline, model
  routing. The Factory NEVER re-implements these.
- New (and only new): a Paseo session binding per Attempt —
  create a real Paseo workspace rooted at the Attempt workspace, start
  the session with the pinned provider/model/mode, record
  (runId, attemptId, generation, paseoSessionId, workspaceId) as one
  atomic identity row, stream timeline observations into Run events, and
  map stop/cancel/reconnect onto the existing fencing protocol.

## Functional requirements

1. Actual Paseo workspaces per Attempt (no `--cwd` pretense; Cwd asserted
   equal to the Attempt workspace, hard FAIL otherwise — proven pattern
   in `scripts/paseo-trusted-local-smoke.sh`).
2. Effective cwd/provider/model validated from session evidence on every
   start AND resume (no silent substitution, pinned-model gate).
3. Durable Run/Attempt/session identity surviving daemon/Worker restarts
   (reconnect by session id; stale generations fenced as today).
4. Real activity streamed to Factory Office (timeline projection over
   Core events; reconnect-safe catch-up; never "isolated/certified"
   labeling — TRUSTED_LOCAL labeling per `docs/security/trusted-local.md`).
5. Cancellation (maps to `paseo stop`), reconnect (re-attach by session
   id), recovery (lease expiry → retry with new generation; stale
   sessions fenced, never double-reported).
6. Provider-specific modes as proven: codex `full-access` (fresh
   workspaces are untrusted), opencode `build`. No other modes without
   their own smoke evidence.

## Non-goals (explicitly out)

- Unattended multi-hour campaigns; autonomous merge/deploy; production
  delivery (S5 governs that separately).
- Credential proxies, universal brokers, provider forks, custom tool
  frameworks (rejected direction; TRUSTED_LOCAL risk accepted by owner).
- S3-ISOLATED execution (separate certification; unchanged requirements).
- New model routes beyond the three listed (each needs its own smoke).

## Suggested work slices (each independently verifiable)

1. Identity row + workspace binding (Core/Worker fields, contract
   amendment under ADR-0006 atomic-update rule).
2. Session lifecycle adapter (start/observe/stop/reconnect; reuse smoke
   script patterns as the executable oracle).
3. Timeline → Run events projection + Office view (read-only, catch-up).
4. Cancel/reconnect/recovery matrix (kill daemon mid-turn, resume,
   fencing probes — mirrors S2 negative discipline).
5. Three-model acceptance run (one bounded session each, same assertions
   as the smoke, independent verifier + Luna review).

## Entry checklist (all must hold before slice 1)

- PR #5 merged (TRUSTED_LOCAL policy + smoke tooling on main).
- S2 gates green on main at the S3 base SHA.
- Owner authorization to begin S3 recorded with scope (models, budget,
  no unattended campaigns, no auto-merge).

## Key risks

- Paseo API drift across versions (pin + adapter tests).
- Session persistence semantics across daemon restarts (must be measured,
  not assumed — resume may rebind policy).
- Approval-gated tools in fresh workspaces (mode strategy above; any new
  gate needs its own evidence).
- Timeline volume vs event-store growth (bounded projections, redaction).
