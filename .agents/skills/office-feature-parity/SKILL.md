---
name: office-feature-parity
description: Enforce the permanent Software Factory rule that every user-facing capability ships with a coherent Factory Office experience, a traceable Core-to-Office impact matrix and real browser acceptance.
---

# Office feature parity

Use this skill when adding or changing any user-facing capability, Core API,
state projection, operator action, provider/runtime setting, project workflow or
Office route. Backend-only implementation is not completion of a user-facing
feature.

## 1. Permanent rule

Every user-facing capability must include a coherent Factory Office experience
in the same feature delivery:

- discoverability;
- configuration or authorized action;
- execution or observation;
- error, empty and offline/recovery states;
- observability and provenance;
- real browser acceptance.

A route with a button is not parity. The operator must be able to complete the
actual workflow without auxiliary manual API calls, filesystem inspection or a
terminal for ordinary operation.

Purely internal changes may be exempt only with a documented rationale. If the
change has observable operational effects, expose them through health,
diagnostics or telemetry projections even when no Office screen is needed.

## 2. Required matrix

For every capability, maintain a traceable row:

```text
Domain capability
  → Core API / projection
  → Office view / route
  → user action
  → loading / empty / error / offline / permission states
  → real browser E2E acceptance
  → capability registry and roadmap status
```

The machine-readable matrix is
[`docs/product/office-parity.json`](../../../docs/product/office-parity.json).
Reference existing capability IDs from
[`docs/product/capabilities.yaml`](../../../docs/product/capabilities.yaml).
Do not invent IDs, routes, APIs, tests or statuses.

A row must identify:

- the capability ID and the user job;
- the authenticated Core endpoint or durable projection that supplies the data;
- the exact Office route;
- the operator actions and the Core operation each invokes;
- all relevant states and the recovery path;
- the browser test that exercises the real Core-backed workflow;
- any explicitly unavailable or deferred part, with a reason.

## 3. Delivery workflow

1. Define the observable user job and the authoritative Core data/action.
2. Identify the Office route and the shared components needed.
3. Implement the Core operation and projection; keep authority in Core.
4. Implement the Office experience, including search/filter/deep links,
   validation, confirmations, cancellation, empty/error/offline states and
   redaction.
5. Add the parity matrix row and update registry/roadmap statuses.
6. Exercise the journey in a real browser at desktop, tablet and narrow widths;
   check keyboard navigation, focus, console errors, failed requests and 404s.
7. Add or update independent acceptance evidence. Mocks/Storybook may support
   component development but not product-readiness claims.
8. Add a PR `office-impact` declaration. Internal-only changes use `impact:
   none` with a real rationale; they do not bypass health/diagnostics exposure.

## 4. PR declaration

Fill the Office impact section in the PR template:

```text
<!-- office-impact:start -->
impact: changed|none
capabilities: CAP-...
routes: /...
core_api: GET /...
user_actions: ...
states: loading, empty, error, offline, unauthorized
acceptance: path/to/browser-test
rationale: ...
<!-- office-impact:end -->
```

A valid changed declaration must be specific and non-placeholder. It is bound to
the same PR base/head as the architecture assessment. The CI gate checks
presence and shape; an independent reviewer checks semantic accuracy and real
user behavior.

## 5. Review checklist

- Can the operator discover and complete the workflow without raw API calls?
- Does every action invoke an existing authenticated Core operation?
- Are approval, spending, revocation, merge and risk decisions still Core-owned?
- Are `UNKNOWN`, `NOT_TESTED`, `DISCONNECTED`, `STALE`, `BLOCKED` and `ACCEPTED`
  represented truthfully?
- Are loading, empty, error, permission, cancellation, reconnection and
  destructive states deliberate?
- Are credentials, tokens, prompts and sensitive values redacted?
- Are routes stable, deep-linkable and reconstructable?
- Is the workflow keyboard accessible, with visible focus and non-drag
  alternatives?
- Does it work at desktop, tablet and narrow widths?
- Is there a real Playwright journey against the real Core/Compose stack?
- Are the capability registry, roadmap, design documentation and evidence
  updated in the same change?

## 6. Stop conditions

Stop and report when:

- the required Core API does not exist and cannot be added under Core authority;
- an apparent action would have no real effect, would bypass authorization or
  would fabricate state;
- the feature would require a credential or secret in browser storage;
- the only interaction is drag-only, hover-only or color-only;
- a status would claim acceptance or success without the corresponding Core
  evidence;
- the change would leave an ordinary workflow dependent on a terminal, raw API
  calls or filesystem editing;
- the capability mapping would be a placeholder merely to satisfy CI.

## 7. References

- [Factory Office contract](../../../docs/product/factory-office.md)
- [Office-first configuration](../../../docs/product/office-configuration.md)
- [Office parity matrix](../../../docs/product/office-parity.json)
- [Factory Office design](../factory-office-design/SKILL.md)
- [Container-native engineering](../container-native-engineering/SKILL.md)
- [ADR-0025](../../../docs/architecture/decisions/ADR-0025-container-native-and-office-parity.md)
- [Factory browser](../factory-browser/SKILL.md)
