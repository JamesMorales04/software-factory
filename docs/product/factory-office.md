# Factory Office — Engineering Command Center

**Status:** The container-native baseline (PR #28) is merged and Factory Office is
being redesigned as the Product Owner's primary engineering operations
workspace. The current implementation is a truthful prototype-like projection;
the command-center information architecture, design system and several modules
are in progress. This document is not evidence that a planned view exists.

**Authority:** Core owns state, authorization, budgets, evidence and acceptance.
Office is an authenticated projection/client. It never writes SQLite, controls
Workers directly, reads provider credentials or grants authority.

## Product job

Factory Office is where the Product Owner runs an autonomous engineering
organization:

- define and authorize engineering objectives;
- monitor Actions, Campaigns, agents, sessions and candidates;
- review evidence and make approvals/interventions;
- operate multiple registered Projects;
- inspect runtime, provider and quality state;
- recover from blocked, failed, stale or disconnected work.

Ordinary operation should not require raw API calls, terminal inspection or
filesystem editing.

## Target modules

| Module | Required experience |
|---|---|
| Overview | Active Campaigns, running agents, pending decisions, blocked work, recent accepted candidates, gate outcomes, measured budget/spend, runtime health and meaningful events; every signal deep-links to the affected object |
| Decision Inbox | Campaign authorization, budget/recovery requests where supported, blocked execution, manual review, unresolved usage/cleanup, conflicting requirements and delivery requests; actionable items separated from informational notifications; every button invokes an existing authenticated Core operation |
| Actions | List, search, filter, create and detail; objective, frozen spec, Project, risk, scope, route, Lead assessment, WorkGraph, budget, campaigns, runs/attempts, candidates, evidence and delivery; progressive disclosure with expert controls |
| Campaigns / Campaign Detail | Authorization, execution and quality states kept distinct; WorkGraph, timeline, participants, attempts, integrations, candidates, diffs, receipts, gate, budgets and cleanup |
| Agent Office | Actual recorded participants, roles, provider/model, assigned Action/Run, observable status, Worker/session linkage, authorized capabilities, bounded tool events, errors, usage and times; visual, table and timeline views over the same data |
| Issues | Governed issue intake/board, relationships to Actions and Campaigns, filters, priority, accessible drag-and-drop and keyboard moves; GitHub synchronization only when implemented and labeled |
| Docs | Repository-backed, versioned Markdown with project navigation, search, source provenance, history and governed editing; no parallel ungoverned architecture source |
| Runtime & Settings | Providers/models, authentication, credentials/revocation, budgets, active sessions, Worker/Supervisor/container health, unresolved usage, cleanup/recovery, runtime policy and changes |
| Application shell | Persistent collapsible sidebar, Project context, command/search navigation, page headers/breadcrumbs, Decision Inbox and runtime health entrypoints, account/lock and responsive layout |

## Truthfulness rules

- Every metric and status comes from a Core snapshot or durable event.
- Unknown, unavailable, stale, disconnected, blocked, not-tested and accepted
  are distinct states. A disconnected view never implies an agent stopped.
- Never invent agent thoughts, hidden reasoning, tool calls, activity, progress,
  cost or acceptance.
- Never display `ACCEPTED` unless the Core Quality Gate recorded it for the exact
  candidate. A model response or deterministic fixture is not automatic product
  acceptance.
- Campaign authorization, Run execution and Quality Gate outcome are separate
  facts, not one ambiguous label.
- Observed tool calls are bounded recorded events, not hidden reasoning.
- Read permission does not imply control permission. Every management action is
  an authenticated Core intent.
- Credentials, tokens, prompts, source bodies and sensitive values are redacted
  from UI, telemetry, screenshots and evidence.

## Current implementation (truthful baseline)

The current Office has a responsive command-center shell with grouped
navigation, project/work context in the header, command navigation (`Ctrl/Cmd+K`),
light/dark themes, comfortable/compact density, Core health visibility and a
shared token/component foundation. Existing views are Dashboard, Projects,
Actions, Lead, Delegations, Campaigns/Campaign Detail, Runs, Workers, Runtime
and Settings. It uses Core snapshots and polling; it does not claim SSE/live
streaming. Campaign Detail includes a table-based WorkGraph projection,
candidate receipts and the deterministic Gate. The browser journey and Compose
smokes pass on the baseline; Decision Inbox, Agent Office, Issues, Docs,
graph/timeline views and the command-center Overview are not yet implemented.

Known gaps:

- Existing views still use the legacy compatibility surface; they are migrated
  to the shared components module by module.
- Campaign authorization labels can remain `AUTHORIZED` after a terminal Run;
  the Run and gate are authoritative and must be displayed separately.
- No central decision aggregation or actionable inbox.
- No visual Agent Office or session inspector.
- No issue board or repository docs/wiki.
- Hash-only routes, no stable `/office` deep-link base.
- The design foundation is implemented; Storybook, visual regression, full
  automated accessibility and the remaining graph/board/diff components are not.
- Polling only; no cursor/replay stream in the browser.
- Limited responsive behavior, filters, command search and keyboard alternatives.

## Verification requirements

- Real browser creates an Action, reloads and finds the same persisted record.
- Real browser completes Project → Action → delegation → Lead/Campaign →
  candidate → evidence/gate paths without auxiliary API calls.
- Agent views show real recorded participants and sessions without fake data.
- Disconnect/reconnect does not lose or duplicate visible transitions.
- Read-only roles cannot initiate management effects.
- Every primary journey is exercised at desktop, tablet and narrow widths with
  keyboard navigation, visible focus, accessible labels/errors and no console
  errors, failed requests or unexplained 404s.
- Browser traces/screenshots are candidate-bound, redacted and retention-aware.
- A visual/aesthetic pass does not replace Core-backed functional acceptance.

An animated/pixel office may come later; it must reuse the same authoritative
records and must never create a parallel agent state. The first Agent Office is
a useful 2D operational visualization, not an expensive simulation.

## Design and parity

- [Office design system](office-design-system.md)
- [Office parity matrix](office-parity.json)
- [Office-first configuration](office-configuration.md)
- [Factory Office design skill](../../.agents/skills/factory-office-design/SKILL.md)
- [Office feature parity skill](../../.agents/skills/office-feature-parity/SKILL.md)
- [ADR-0025](../architecture/decisions/ADR-0025-container-native-and-office-parity.md)
