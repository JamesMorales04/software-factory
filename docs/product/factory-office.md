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
| Home / Requests | A conversation-first intake composer plus a Project-filtered request list with human objectives and translated states; every request opens a workspace that shows the assessment, frozen plan, exact approval, execution and failure state |
| Decision Inbox | Core-backed work that needs an operator decision or awareness; Project filter, actionable items separated from notifications, exact confirmation for every mutation and no invented action |
| Campaigns / Campaign Detail | Read-only human campaign list with Project filter; detail keeps lifecycle, WorkGraph, execution, candidates, evidence, Quality Gate, usage telemetry and events distinct, with explicit limitations instead of invented state |
| Projects / Project Detail / Files | Import an authorized GitHub repository, select a folder from the operator's computer through the OS picker, or register a public Git URL; show human identity, base revision, Campaigns, decisions, discovery evidence and a bounded file browser |
| Issues | Automatic GitHub issue synchronization, a Project-filtered kanban by real work state, click-to-start governed intake and an explicit auto-start mode while Office is open; issue text remains untrusted plain text |
| Agent Office | Recorded participants, roles, provider/model, assigned Campaign, observable state, Worker/session linkage and bounded tool events; no simulated activity or hidden reasoning |
| Runtime & Settings | Core health, Workers, provider sessions, model calls, unresolved reservations, cleanup, telemetry, provider/model connections, execution profiles, runtime policy and device access |
| Application shell | Persistent collapsible sidebar, command/search navigation, page context, language/theme/density controls, Core health, Decision Inbox entrypoint and responsive layout |

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
navigation, route context in the top bar, command navigation (`Ctrl/Cmd+K`),
light/dark themes, comfortable/compact density, Core health visibility and a
shared token/component foundation. The canonical routes are `/`, `/requests`,
`/requests/:id`, `/inbox`, `/campaigns`, `/campaigns/:id`, `/projects`,
`/projects/:id`, `/projects/:id/files`, `/issues`, `/agents`,
`/agents/:id`, `/runtime`, `/settings` and `/pair`. Removed pages redirect to
the closest current workspace; unknown routes return Home. Path-style URLs
copied without the hash are redirected to the hash route for browser
navigation requests.

Home is a conversation-first unified intake composer: one Project, one
natural-language objective and one execution profile create a durable intake
request. It does not ask for monetary, token, turn, attempt, session or
Campaign-deadline controls. A profile whose `auto_start` policy enables R0 is an
autopilot profile: selecting it and sending the objective is enough. The Lead
assesses, freezes and starts the R0 work under the owner-granted autonomous
policy; the owner watches and can cancel. The first use of a Project/profile
shows one exact policy grant covering autonomous R0 execution and the
subscription Extra Usage confirmation. If the profile is not autonomous, the
Request workspace keeps the manual assessment and exact plan-approval flow.
Every request remains bounded by its frozen profile, delegation, allowed paths,
capabilities, operations, progress policy and expiry.

A completed or cancelled request can receive one additional operator message to
start a follow-up request in the same Project and exact profile. The Request
workspace explains that sending starts a new request, shows the parent on the
child, and shows the created child and its state on the parent. The follow-up is
new evidence: it runs its own assessment, plan/environment/spec freeze and
Campaign and never reuses the parent plan, candidate or Campaign. When a Factory
delivery merges the protected default branch, Core advances the Project base to
the exact merge commit so the follow-up plans from merged main.

Autonomous intake is a Core-owned background advance, not a browser macro.
Creating an `AUTONOMOUS` request with a valid autonomous grant makes Core run
the assessment, freeze the plan and call the autonomous start boundary. The
runner is idempotent for one request and resumes unfinished autonomous requests
after a Core restart. It still fails closed: R1+ and protected decisions remain
human, the policy/profile/route/path/expiry binding is revalidated, and a
failed assessment or start leaves an explicit Blocked/Failed state rather than
a false success.

Campaigns is a read-only list with human objectives, translated states and a
Project filter. Campaign Detail keeps lifecycle, WorkGraph, execution,
candidates, evidence, Quality Gate and usage telemetry separate. Authorization
is read from the V2 record; historical V1 caps are provenance, not current
authority. The current product does not enforce model token, turn, attempt,
session, wall-clock or subscription-cost productivity caps. Token, time, retry
and subscription usage is telemetry, and the no-progress breaker is the
automatic stop.

Issues automatically synchronizes a bound GitHub repository, renders a
five-column board (Ready, Queued, Working, Needs attention, Done) with a
Project filter, and starts one governed work request from a ready issue. An
explicit auto-start toggle starts one ready issue at a time while Office is
open; it is a browser-side convenience, not a Core scheduler. Repository
binding verifies and records the immutable GitHub repository ID on the server.
Policy setup exposes labels, issue capacity and expiry; monetary, attempt,
session, turn and wall-clock policy fields are not part of the active surface.

Projects supports GitHub import through the encrypted connection, public Git
URL registration and a computer-folder picker. Chromium's File System Access
API opens the operating system picker, copies the selected folder into managed
storage through the Core ZIP import boundary (with traversal, symlink and size
protections), initializes Git when needed and registers the Project. This is a
copy, not a direct host-path mount.

Decision Inbox has a Project filter and separates actionable from
informational items. Its mutations remain exact and confirmation-gated:
intake-plan approval with the frozen hash/profile revision, model-call
resolution, Lead withdrawal and Campaign cancellation. Runtime, Settings and
Agent Office remain truthful Core projections; no view synthesizes agent
activity or acceptance.

Known gaps:

- Issues auto-start is browser-side and stops when Office closes. A Core-owned
  autonomous issue scheduler remains unimplemented; the unified-intake
  autopilot is Core-owned.
- Autonomous intake currently requires OpenCode Go subscription routes for both
  the Lead and the implementer. A profile that declares R0 auto-start on another
  route shows a warning and uses manual plan review instead of starting.
- The computer-folder picker copies the folder into managed storage; there is
  no native desktop bridge or direct host-path mount into the container.
- GitHub App/OAuth installation onboarding is not implemented. The supported
  flow uses the existing encrypted token connection.
- Codex one-shot assessment and semantic review remain unsupported; a Codex
  route for those roles stays non-executable.
- Autonomous Gate repair is disabled. A Gate that requests changes remains
  `REPAIR_REQUIRED` until an operator cancels the Campaign or supplies a Project
  based on the candidate to repair; a new request starts from the Project base
  commit and is not an in-place repair. The Inbox does not post a repair
  authorization.
- Provider-session termination and full-scope Campaign authorization remain
  outside the Inbox because no exact operator mutation is exposed there.
- A true independent security review, Windows support, native ChatGPT OAuth and
  deployment remain open. `TRUSTED_LOCAL` is never described as isolated or
  certified.
- Polling only; no cursor/replay stream in the browser. A disconnected view
  preserves and labels the last authoritative snapshot.
- Storybook, full visual-regression coverage and complete automated
  accessibility certification are not implemented.

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

The Agent Office visual view is a deterministic isometric SVG/DOM floor over
the same authoritative records; it is a 2D operational visualization, not a
simulation. An animated or sprite-based office may come later only if it reuses
the same records, never creates a parallel agent state and passes a measured
bundle/performance and accessibility decision. See
[Agent Office isometric floor](agent-office-isometric.md).

## Design and parity

- [Office design system](office-design-system.md)
- [Office parity matrix](office-parity.json)
- [Office-first configuration](office-configuration.md)
- [Factory Office design skill](../../.agents/skills/factory-office-design/SKILL.md)
- [Office feature parity skill](../../.agents/skills/office-feature-parity/SKILL.md)
- [ADR-0025](../architecture/decisions/ADR-0025-container-native-and-office-parity.md)
