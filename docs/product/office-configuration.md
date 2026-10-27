# Office-first configuration

**Status:** implemented developer-preview UX, author-run. Model/provider
inference requires a configured provider and explicit work authorization.

Office is served by Core at the single published loopback endpoint. It is a
projection/client of typed Core APIs; it is never an authorization bypass.

## Onboarding

- First-run status is public but read-only.
- Enrollment requires the one-time code from the private data volume.
- The operator token is returned once to the tab and kept in session storage;
  it is not written to a provider setting or URL.
- After enrollment, no unauthenticated administrative mutation endpoint exists.

## Settings → Providers & Models

The provider catalog is typed:

| Provider | Adapter | Authentication | Status |
|---|---|---|---|
| OpenCode Go | OpenCode | API key | supported `TRUSTED_LOCAL`; model discovery provider-dependent |
| Codex App Server | Codex | API key | deterministic adapter only; native ChatGPT OAuth blocked |

Each provider card exposes:

- installed/configured status and masked credential fingerprint;
- model, endpoint, enabled/default route;
- provider-route default turn/token/cost limits and per-million rates; these
  cap Core model calls but never widen or replace the explicit Campaign
  authorization that bounds provider sessions;
- test connection (model list only), configuration revision and audit;
- credential replacement and revocation.

Secrets are sent only on save, encrypted with AES-256-GCM in Core, and never
returned. Saving or testing does not start inference.

## Review & runtime policy

Semantic Reviewer model calls are **off by default**. Enabling the policy
allows bounded Core-owned model calls for each candidate review, and Core
rejects semantic model calls or MODEL receipts while it is disabled. The
verifier's frozen-check execution is always independent and deterministic. The
reviewer binary runs its deterministic pre-review before model review; Core
enforces the policy and budget but cannot prove that internal ordering against
a compromised reviewer token. A model `approve` alone does not create the
verifier receipt required by the gate.

## Projects

Register HTTP(S) Git URLs; Core imports them into managed storage. Office shows
the managed repository, base commit, identity digest, config revision/hash.
Configure:

- allowed repository paths;
- capabilities (`repository-edit`, `generic-toolchain`, `node-toolchain`,
  `browser`);
- generic/node-basic/node-browser quality profile;
- active state.

Arbitrary host folders are not mounted.

## Actions

An Action freezes the objective, route, allowed paths and checks. The editor
supports format, lint, build, test, functional, negative and browser kinds, with
command previews, expected exit codes and timeouts. At least one executable
check is required for an independent PASS.

## Delegations

Create/revoke scoped grants with:

- objective classes, allowed paths and capabilities;
- risk classes and route;
- attempts, sessions, requests, timeouts and cumulative ceilings;
- expiration and concurrency;
- explicit autonomous-start off by default;
- cumulative usage display.

## Lead, Campaigns and evidence

- Lead requests use the configured Core model gateway. Assessment, proposal
  and plan are visible and hash-bound.
- Starting a Campaign requires an explicit Office authorization under a valid
  Core-owned delegation and budget.
- Campaign detail shows Runs/Attempts, provider sessions, candidate diff,
  verifier/reviewer receipts, gate, cost and cleanup.
- Cancel/revocation remains Core-audited.

## Known UX gaps

- The delegation editor exposes a conservative fixed budget envelope rather
  than arbitrary per-field editing.
- Project source URL is displayed as its managed repository path; a separate
  source-label revision is planned.
- Semantic review is controlled by a global policy toggle, not per-project.
- Live Codex configuration is visible but container inference remains blocked.
- Browser onboarding and the deterministic full journey are author-run; an
  independent browser acceptance remains open.
