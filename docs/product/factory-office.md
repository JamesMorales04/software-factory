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
| Projects / Project Detail | Authorized public/private GitHub repository search and import through the existing encrypted connection, approved-root local import, explicit human name and source identity, Core-backed summary with origin/base revision/last activity/connection health/Campaigns/decisions/capabilities, and bounded manifest discovery whose advisory toolchain and check evidence never grants authority |
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
navigation, route context in the top bar, command navigation (`Ctrl/Cmd+K`),
light/dark themes, comfortable/compact density, Core health visibility and a
shared token/component foundation. Existing views are Overview, Decision
Inbox, Projects, Actions/Action Detail, Lead, Delegations, Campaigns/Campaign
Detail, Runs, Workers, Runtime and Settings. It uses Core snapshots and
polling; it does not claim SSE/live streaming. Overview projects authoritative
Core counts, Quality Gate outcomes, measured and unmeasured spend, accepted
candidates, decisions, runtime health and recent events. Decision Inbox
separates action-required items from informational events and aggregates
blocked authorizations, frozen intake/Lead plans, Gate repair/human-review
states, unresolved model usage, unresolved provider sessions and failed work.
Its Core-backed actions are exact and confirmation-gated: approve an intake
plan only with the frozen plan hash and profile revision, release an ambiguous
model reservation, withdraw a frozen Lead request, or cancel a Campaign. Every
action re-reads `/office/decisions` and refuses a changed or vanished item.
Full-scope Campaign authorization and Gate repair are not posted from the
Inbox; it links to the exact Campaign or Gate evidence and records the
available limitation instead of inventing a mutation. Action Detail shows the frozen spec,
checks and related Core records. Campaign Detail is a tabbed operational
workspace with separate lifecycle, WorkGraph, execution/participant,
candidate-revision, evidence, Quality Gate, budget/usage and events/recovery
views. Candidate revisions expose a bounded Core file diff from the exact
frozen base to the exact candidate commit/tree, including per-file status,
additions/deletions and unified patches with explicit binary, large, non-file,
truncated and unavailable states. The same workspace can read a bound GitHub
pull request, its checks, comments and bounded PR diff, request review, open a
PR only when the remote head matches the candidate commit, and merge only with
explicit confirmation after a fresh PR/check read. GitHub credentials remain
server-side; real GitHub acceptance remains open. Runtime is an operational console
for Core health, registered Workers, filtered provider sessions, measured and
unmeasured usage, model-call reservations, audited release, cleanup, telemetry
and explicit unexposed services. Settings is a provider and policy workspace
for route configuration, credential lifecycle, connection diagnostics, advanced
limits, semantic-review policy and configuration audit. Agent Office projects
recorded Lead, Implementer, Verifier, Reviewer and Worker participants into a
visual office, table and timeline with a Core-backed inspector and bounded
provider tool events. Campaign Detail now includes an interactive,
dependency-aware WorkGraph with a keyboard-accessible node inspector while
retaining the authoritative table. Issues has a read-only GitHub snapshot
foundation: encrypted connection, Project repository binding, bounded sync and
plain-text issue detail are implemented in the lane view; governed intake,
board ordering, Action/Campaign relationships and issue writing remain
unimplemented. The view is wired at `/issues`; real browser acceptance is
pending. Projects now offers three Core-governed onboarding journeys: import an
authorized GitHub repository through the existing encrypted connection,
import a local repository only inside the operator-approved managed root, or
register a public Git URL with an explicit human name. Project detail shows a
Core summary, exact base revision, connection and issue-sync health, active
Campaigns, pending decisions and persisted discovery evidence. GitHub App/OAuth
installation onboarding is not implemented; fine-grained tokens do not report
OAuth scopes, and Office does not imply installation-wide access. Home is now a
unified intake composer: a Project-scoped objective and
exact execution profile revision create a durable intake request. Every request
stops for explicit operator approval; the composer does not offer or imply
automatic start. A 409 `EXECUTION_POLICY_REQUIRED` surfaces an explicit
"Enable governed execution" acknowledgement that shows the exact objective
scope, per-request limits and cumulative grant bounds, then creates a bounded,
non-autonomous R0 policy before retrying the same request. The Request
workspace derives its state from the authoritative Lead request, shows the
frozen profile revision and plan hash, runs the bounded assessment, approves
the exact frozen plan, appends pre-freeze operator messages and withdraws
through the authoritative Lead boundary. It does not expose an intake-only
terminal flag. Execution profiles resolve each model-backed role independently:
the frozen Lead route runs intake assessment, the frozen Semantic Reviewer
route runs candidate review, and the implementer route remains the Campaign
authorization route. The profile editor and Provider Settings expose
authenticated provider model discovery; an unavailable or unsupported catalog
is shown as a typed state, and manual model IDs are labelled unverified
non-executable drafts. A Codex App Server connection can implement a Campaign
but cannot execute one-shot assessment or semantic review in this build. Docs
and a stable `/office` deep-link base remain unimplemented.

Known gaps:

- Project, Lead, Delegations, Campaign list, Runs and Workers still use the
  legacy compatibility surface; they are migrated by module.
- The WorkGraph visualizes recorded dependencies; it does not infer scheduling
  progress, and malformed graph data is surfaced rather than repaired.
- Candidate diff metadata does not include file bodies; there is no code-diff
  viewer.
- Discovered-only model IDs are executable only while the current Core process
  holds that provider discovery result; discovery is not persisted.
- Codex one-shot assessment and semantic review remain unsupported by design;
  a Codex route for those roles stays a non-executable draft.
- Campaign authorization labels can remain `AUTHORIZED` after a terminal Run;
  the Run and gate are authoritative and must be displayed separately.
- Decision Inbox actions are limited to Core operations with an exact decision revision: intake-plan approval, model-call resolution, Lead withdrawal and Campaign cancellation. Provider-session termination, Gate repair and full-scope Campaign authorization remain outside it; the Inbox links to the exact object and does not claim those mutations.
- Unified intake assessment requires an authorized model gateway. Without one
  Core returns a typed 503; it does not fabricate plan checks. A FIXTURE
  assessment cannot derive executable checks for a draft Action by itself.
- The execution-policy grant is conservative: R0 only, non-autonomous, no
  repair cycles, one parallel unit, four requests and a 30-day expiry.
  Aggregate token/cost/turn accounting across assessment plus implementation is
  not implemented; approval intersects per-task profile/plan/delegation/Core
  ceilings and records the limitation. Role-specific routing beyond the single
  active default gateway is not implemented, so cross-provider profiles remain
  non-executable.
- A real browser acceptance run against Core/Compose for the unified intake
  journey is pending; the current evidence is component tests plus Rust
  DB/API tests.
- Runtime shows Supervisor, nested Podman and broker state as not exposed; it
  does not infer their health.
- Agent Office is a read-only observability experience; no stop, model switch,
  budget change or tool invocation is exposed.
- Provider sessions remain bounded to the latest projection; very old sessions
  are reached by Campaign scope or exact identity rather than a global sweep.
- Provider connection tests read model discovery only; no live coding inference
  is authorized by this slice.
- No issue board, issue-to-Action/Campaign relationship, issue writing or
  repository docs/wiki. The GitHub issue slice is read-only and requires a
  Project repository binding plus an explicit sync.
- Hash-only routes, no stable `/office` deep-link base.
- The design foundation is implemented; Storybook, visual regression, full
  automated accessibility and the remaining graph/board/diff components are not.
- Polling only; no cursor/replay stream in the browser. A disconnected Agent
  Office preserves and labels the last authoritative snapshot.
- Limited filters and keyboard alternatives for the remaining legacy
  operational views.

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
