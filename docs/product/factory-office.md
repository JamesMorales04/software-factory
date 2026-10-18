# Factory Office — required minimum F1 UI

**Status:** The own web UI is agreed for F1; animated/pixel office is a later view, not an F1 gate.

## Required views

| View | Responsibilities |
|---|---|
| Dashboard | Core/Worker health, queues, campaigns and maintenance |
| Actions | Create/query/filter Actions; show their specs, risk and status |
| Agents | Actual role, runtime/model, state and Paseo session for each participant |
| Action Detail | Events, Runs, tool activity, quality evidence, review and PR/merge status |

The browser consumes Core snapshots plus persisted event projections. It cannot write Core storage or control Workers directly. Reconnect must use cursor/sequence and fetch missing data; stale/lost provider status must be displayed as `UNKNOWN`/`DISCONNECTED`, never as fabricated activity. Observed tool calls are not access to hidden reasoning. Read permissions do not imply control permissions.

## Paseo integration

Persist stable correlation of `runId`, `agentId`, `paseoSessionId` and `workerId`. Expose an authenticated inspector link/route only if tested. Validate SDK timeline/replay and redaction rather than assuming capabilities.

## F1 verification

- Real browser creates an Action, reloads and finds the same persisted record.
- Agents view shows a real active Run and its session without fake data.
- Independent Playwright verifier navigates Actions → Agent → evidence/PR.
- Disconnect/reconnect does not lose or duplicate visible transitions.
- Read-only roles cannot initiate management effects.
- Browser traces/screenshots are attached to candidate evidence with redaction and retention controls.

Animated Factory Office must eventually reuse the same authoritative events and not establish separate agent state.
