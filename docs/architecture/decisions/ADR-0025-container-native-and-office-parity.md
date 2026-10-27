---
id: ADR-0025
status: Accepted for developer-preview implementation
date: 2026-10-27
owner: product-owner
supersedes: null
amends: ADR-0024
---
# ADR-0025: Container-native engineering and Office-first product parity

- **Status:** Accepted for developer-preview implementation by the Product Owner
  campaign directive of 2026-10-27. The container-native baseline (PR #28) is
  merged; its deterministic and owner-authorized live container evidence is
  recorded in [container-native status](../../implementation/container-native-status.md).
  The Office redesign and its design system remain in progress.
- **Amends:** [ADR-0024](ADR-0024-container-native-compose-runtime.md) by making
  container-native engineering and Office-first parity permanent repository
  rules. It does not weaken ADR-0018, ADR-0019, ADR-0009, the service-placement
  policy, the isolation policy or the Quality Baseline.
- **Security status:** `TRUSTED_LOCAL`. Containers, Compose, the nested engine
  and the development container are not an isolation certification.

## Context

PR #28 established the permanent rootless Compose topology: Core/Office,
containerized Supervisor/Worker, independent verifier/reviewer, a dedicated
nested Podman engine and a narrow runtime broker. Its deterministic E2E passed
and the owner-authorized live OpenCode container UAT passed at
`1225baeb123f7a652bb3a927f53562e28c573e26`.

Two governance problems remain:

1. Development and CI still contain host-toolchain paths that can drift from the
   container-native product. Instructions alone do not prevent a future change
   from adding a host-only runtime dependency or bypassing Compose.
2. Office has many operational projections but is still a prototype-like
   collection of pages. A backend capability can ship without a discoverable,
   usable, accessible Office workflow. A button without a real Core operation,
   or a status without evidence, is also possible.

## Decision

### 1. Container-native is a permanent engineering invariant

- `compose.yaml` is the installed-product lifecycle. The host provides rootless
  Podman, a supported Compose provider, a browser and normal kernel facilities;
  it does not provide Rust, Cargo, Node, npm, OpenCode, Codex or Playwright for
  normal operation.
- New services, provider adapters, dependencies, executables, browser-testing
  tools and infrastructure changes must work within the Compose topology and the
  development-container workflow. A new host-only runtime dependency or an
  unsupported Compose bypass is a review failure.
- Trusted development tooling, permanent application services and untrusted
  execution sandboxes remain separate trust classes. Do not combine them to
  reduce container count.
- The dedicated engine and narrow broker remain the only execution API. No host
  Podman socket, privileged fallback, host networking, arbitrary host mount or
  unapproved image is allowed.
- The canonical workflow is
  [container-native engineering](../../engineering/container-native-engineering.md)
  and the [container-native engineering skill](../../../.agents/skills/container-native-engineering/SKILL.md).
- Changes affecting container execution require a real Podman test and negative
  policy tests; mock-only or host-only evidence is insufficient.

### 2. Office-first is a permanent product invariant

Every user-facing capability ships with a coherent Office experience in the same
delivery: discoverability, configuration or authorized action, execution or
observation, error/empty/offline/recovery states, observability and real browser
acceptance. Backend-only implementation does not complete a user-facing feature.

Every capability maintains a traceable row:

```text
domain capability → Core API → Office route → user action
→ error/empty/offline states → real browser acceptance
```

The machine-readable matrix is
[`docs/product/office-parity.json`](../../product/office-parity.json). The
workflow is defined by the
[office-feature-parity skill](../../../.agents/skills/office-feature-parity/SKILL.md)
and the design rules by the
[factory-office-design skill](../../../.agents/skills/factory-office-design/SKILL.md).
Purely internal changes may be exempt only with a documented rationale;
material operational effects still need the appropriate health/diagnostics
projection.

### 3. One design system and one application shell

Office uses one Factory-specific visual language, one accessible primitives
library, one icon family and shared components. The design direction,
tokens, states and accessibility conventions are documented in
[Office design system](../../product/office-design-system.md). The application
shell provides project context, stable routes, navigation, command search,
runtime health and Decision Inbox/account entrypoints.

Office is a truthful projection over Core-owned state. It never invents agent
thoughts, tool calls, activity, metrics or acceptance, and never displays
`ACCEPTED` without the Core Quality Gate for the exact candidate.

### 4. Governance enforcement

- `AGENTS.md` requires the container-native and Office-first skills and rules.
- The PR template requires architecture, container-native and Office impact
  declarations for matching changes. The impact checker binds declarations to
  the exact base/head and validates shape and referenced files; an independent
  reviewer checks semantics.
- A capability-parity checker validates the matrix against the capability
  registry and referenced artifacts.
- CI runs the container-native development checks and the existing exact-head
  Rust, browser, Compose-policy and governance gates. Runtime changes require a
  real Podman acceptance declaration; the real Compose E2E remains a separate
  required lane for the affected change.
- The capability registry, roadmap, design documentation and evidence are
  updated in the same change.

## Consequences

- A clean checkout remains usable with only a rootless container host, but
  building the full development image (including Chromium) is larger and slower
  than a host-only workspace.
- CI gains containerized development and parity checks; a failure is evidence
  that the workflow, not just the code, must be repaired.
- The Office redesign is incremental and capability-driven. It must not become
  a monolithic rewrite or a collection of decorative pages.
- Real browser and real Podman acceptance may be slower than unit tests. They
  are required for affected paths and remain separate from independent
  acceptance and owner approval.
- No claim of SEC-01 isolation, credential confinement, deployment, Windows or
  production readiness follows from this ADR.

## Alternatives considered

1. **Document container-native guidance only, without enforcement.** Rejected:
   future changes can drift back to host dependencies and bypasses.
2. **Add a second orchestration layer for development.** Rejected: duplicate
   lifecycle authority and drift. Use Compose services/one-shot jobs.
3. **Put development tooling into the runtime image.** Rejected: larger
   production images and weaker separation of trusted development from runtime.
4. **Build Office as independent admin pages.** Rejected: no cohesive operator
   workflow, inconsistent status, duplicate forms and inaccessible interactions.
5. **Require Office UI for every internal change.** Rejected: ceremonial UI and
   scope inflation. Internal changes with material operational effects still
   expose health/diagnostics; exemptions require rationale.

## Verification obligations

- The container-native development workflow and CI lane must build and run the
  Rust and Office gates without host toolchains.
- The Office parity matrix must reference real capabilities, APIs, routes and
  browser tests; the checker must reject missing or placeholder declarations.
- Each Office slice needs real Chromium journeys, responsive and keyboard/
  accessibility checks, and exact-head evidence.
- The installed Compose image build and deterministic E2E must continue to pass.
- Runtime changes require real Podman evidence and an independent review.
