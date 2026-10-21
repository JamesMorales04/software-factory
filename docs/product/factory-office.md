# Factory Office — required minimum F1 UI

**Status:** The own web UI is agreed for F1; animated/pixel office is a later view, not an F1 gate.

## Required views

| View | Responsibilities |
|---|---|
| Dashboard | Core/Worker health, queues, campaigns and maintenance |
| Projects | Register/inspect/pause authorized local Git Projects; show canonical identity, base policy, profile, scope and capability grants |
| Actions | Create/query/filter Project-bound Actions; show their specs, risk and status |
| Action Detail | Freeze a Project-bound ActionSpec through an explicit form, then inspect candidates, evidence and history |
| Agents | Actual role, runtime/model, state and Paseo session for each participant |
| Action Detail | Events, Runs, tool activity, quality evidence, review and PR/merge status |
| Campaigns / Campaign Detail | Start an authorized campaign, follow the Core-owned task graph, participant identities, candidate/evidence lineage, repair/recovery budget, Implementer recovery history and Quality Gate outcome |

The browser consumes Core snapshots plus persisted event projections. It cannot write Core storage or control Workers directly. Reconnect must use cursor/sequence and fetch missing data; stale/lost provider status must be displayed as `UNKNOWN`/`DISCONNECTED`, never as fabricated activity. Observed tool calls are not access to hidden reasoning. Read permissions do not imply control permissions.

## Paseo integration

Persist stable correlation of `runId`, `agentId`, `paseoSessionId` and `workerId`. Expose an authenticated inspector link/route only if tested. Validate SDK timeline/replay and redaction rather than assuming capabilities.

S4-C implements Projects, Project-bound Action creation and ActionSpec
freezing through real forms backed only by Core APIs/projections. The main
operator journey no longer needs a Python bootstrap or direct spec/campaign
API calls. S4-B implements Campaign list/detail views from Core projections and
the existing SSE/replay ledger. The browser can create/start/cancel a campaign
through authenticated operator intents; it never schedules tasks itself.
S3-C implements the read-only Agents and Run-detail session projections from
durable Core bindings. The contract is
[`agent-observability-s3c.md`](../contracts/agent-observability-s3c.md).
Lifecycle events and the bounded final S3-B summary are shown; live
per-tool streaming is not implemented and is documented as a Level-3
limitation rather than simulated.

## F1 verification

- Real browser creates an Action, reloads and finds the same persisted record.
- Agents view shows a real active Run and its session without fake data.
- Independent Playwright verifier navigates Actions → Agent → evidence/PR.
- Disconnect/reconnect does not lose or duplicate visible transitions.
- Read-only roles cannot initiate management effects.
- Browser traces/screenshots are attached to candidate evidence with redaction and retention controls.

Animated Factory Office must eventually reuse the same authoritative events and not establish separate agent state.
