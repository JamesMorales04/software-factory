# ADR-0012 — Multi-project registry and scoped capabilities

**Status:** Accepted (F1-S4-C implementation; final candidate evidence is recorded in the PR). **Date:** 2026-10-21.

## Context

S1–S4-B operate on one seeded `default` Project. ActionSpecs take an arbitrary
absolute repository path, Runs do not carry capability requirements, Worker
selection has no project or capability filter, and Factory Office cannot
register a Project or freeze an ActionSpec. Browser automation exists for the
Independent Verifier and an optional Implementer helper, but it is not a
Core-governed capability bound to a Project, Run and participant.

The next minimum capability is a genuinely reusable local-first multi-project
platform, without claiming hostile-code isolation: TRUSTED_LOCAL still grants
broad host access to the Factory process.

## Decision

1. **Extend the S1 Project row; do not create a second project subsystem.**
   Registered Projects carry a canonical local Git repository identity, base-ref
   policy, Quality Profile, workspace scope, capability grants, loopback browser
   endpoints, execution mode, security label, immutable configuration revision
   history and audit events. The seeded `default` row migrates to `legacy`: it
   retains its data and evidence but gains no repository grant.
2. **Repository identity is Core-computed.** Identity is a SHA-256 over the
   symlink-resolved canonical path and the repository's root commit set. A
   symlink alias resolves to the same identity; a different repository does
   not. Base revisions must be exactly the tip of an authorized base ref.
   Registration, spec freeze, dispatch and candidate freeze revalidate the
   relevant facts; there is no path or revision fallback.
3. **ActionSpecs are Project-bound snapshots.** A registered Project derives
   the repository, exact base tip, workspace scope, browser endpoints and
   capability requirements. The spec stores the Project configuration revision
   and a separate `bindingHash` over the project/config/repository/capability
   snapshot; the legacy S4-A `contentHash` remains the normative content hash
   for persisted compatibility. CandidateRevision records the binding hash and
   the Quality Gate rejects a mismatch.
4. **Capabilities are a closed vocabulary.** `repository-edit`,
   `node-toolchain`, `generic-toolchain` and `browser` are declared by the
   Project, frozen into the spec/Run and matched against the Worker's observed
   runtime. A missing capability is `BLOCKED`/`NOT_TESTED`; there is no
   privileged fallback. The Implementer cannot acquire Verifier, Reviewer,
   operator or delivery authority by requesting more tools.
5. **Playwright is a Factory-managed Implementer capability.** For an eligible
   browser Run the Worker writes a run-scoped skill and a wrapper around the
   pinned `@playwright/cli`, launches Chromium through a generated
   `playwright-cli` config, restricts URL arguments to authorized loopback
   endpoints, records bounded invocation metadata, and closes/removes the
   disposable profile. The wrapper is a control-plane guard, not an OS
   isolation boundary. The Independent Verifier continues to use its own clean
   checkout and protected `browser` oracle.
6. **Dispatch is project- and capability-aware and fair.** Queued Runs are
   eligible only when the Project is active, the Worker advertises the required
   capabilities/execution mode and its heartbeat is fresh. Scheduling rotates
   by Project last-dispatch time so one Project cannot indefinitely starve
   another. Cross-project recovery retains exact Run/Attempt/generation
   ownership.
7. **Office provisions the whole vertical slice.** Projects, Actions and frozen
   ActionSpecs are created through Core-authoritative forms backed by Core
   APIs and projections; Campaigns are started and inspected in the same UI.
   No custom Python bootstrap is required for the operator journey.
8. **Security truthfulness.** This is control-plane authorization, not OS
   confinement. TRUSTED_LOCAL remains broad host access under ADR-0009;
   cross-project confidentiality and hostile-code containment are not claimed.
   SEC-01-ISOLATED retains its separate certification gate.

## Consequences

- New Project columns/config-revision table, capability/binding columns on
  specs/runs/candidates/workers, and new project events.
- Legacy `default` Actions/Specs remain readable and executable only where their
  existing explicit repository snapshot is valid; new Project-bound specs
  require a registered Project.
- Candidate staging ignores known runtime artifacts (logs, browser profiles,
  dependency/build directories) while still rejecting any changed source path
  outside the frozen allowed scope.
- Real-model certification is intentionally separate from deterministic
  verification; see the live-certification policy.

## Alternatives rejected

- **A separate project service/database:** duplicates authority and breaks the
  single Core ledger.
- **Path-only identity:** rejects no aliases but changes identity on relocation;
  root commits alone can collide for identical independent baselines.
- **Letting ActionSpec choose any repository:** breaks Project binding and
  cross-project evidence isolation.
- **Global tool grants:** would give every Participant browser or generic host
  tools regardless of the Project/Run requirement.
- **Mandatory Playwright MCP:** adds a persistent browser-server/profile and
  credential surface without a demonstrated requirement; CLI + harness suffice.
- **Claiming TRUSTED_LOCAL is isolated:** false; rejected explicitly.
