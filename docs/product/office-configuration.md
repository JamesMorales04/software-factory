# Office-first configuration

**Status:** implemented developer-preview UX, author-run. Model/provider
inference requires a configured provider and explicit work authorization.

Office is served by Core at the single published loopback endpoint. It is a
projection/client of typed Core APIs; it is never an authorization bypass.

## Onboarding

- First-run status is public but read-only.
- `./factory open` obtains a five-minute, single-use confirmation code through
  a protected Core container operation. The Owner enters it once in Office;
  it is not placed in a URL or request log.
- Office uses an opaque server-managed device session in an HttpOnly,
  SameSite=Strict cookie. Core persists only its SHA-256 digest. A separate
  readable CSRF cookie is sent in `x-factory-csrf` for browser mutations.
- Browser sessions use exact Host/Origin checks and require loopback HTTP for
  local development. Exact private HTTPS origins are configured explicitly.
- `./factory recover` revokes enrolled sessions and issues a new one-time local
  confirmation code. The permanent operator credential remains a protected
  internal API/recovery mechanism and is not returned to Office.

## Settings → Providers & Models

The provider catalog is typed:

| Provider | Adapter | Authentication | Status |
|---|---|---|---|
| OpenCode Go | OpenCode | API key | supported `TRUSTED_LOCAL`; model discovery provider-dependent |
| Codex App Server | Codex | API key | deterministic adapter only; native ChatGPT OAuth blocked |

Each provider card exposes:

- installed/configured status and masked credential fingerprint;
- model, endpoint, enabled/default route;
- provider-route token/cost limits and per-million rates; these cap Core model
  calls but never widen or replace the explicit Campaign authorization that
  bounds provider sessions. The turn limit is stored as an operator default but
  is not enforced in this iteration;
- test connection (model list only), configuration revision and audit;
- credential replacement and revocation.

Secrets are sent only on save, encrypted with AES-256-GCM in Core, and never
returned. Saving or testing does not start inference. The base Compose stack
defaults to the live OpenCode adapter and fails closed without matching provider
settings; the deterministic fixture is enabled only by the explicit E2E
override, never as a silent fallback.

## Review & runtime policy

Semantic Reviewer model calls are **off by default**. Enabling the policy
allows bounded Core-owned model calls for each candidate review, and Core
rejects semantic model calls or MODEL receipts while it is disabled. The
verifier's frozen-check execution is always independent and deterministic. The
reviewer binary runs its deterministic pre-review before model review; Core
enforces the policy and budget but cannot prove that internal ordering against
a compromised reviewer token. A model `approve` alone does not create the
verifier receipt required by the gate.

## Settings → Access & Devices

Access & Devices shows the current browser session, enrolled devices, last
activity, expiry and recent Core security activity. Each browser or phone has
its own revocable session; the permanent operator credential is never sent to
the browser.

To pair a phone:

1. Open **Settings → Access & Devices** on the enrolled desktop and select
   **Pair device**. The QR contains only a short-lived random challenge
   identifier, never a session, operator token or verification code.
2. Scan the QR with the phone while it can reach the Factory Office origin.
   The phone claims the challenge and displays a six-digit verification code.
3. Confirm the same code in Office and select **Approve device**. Core creates
   an independent HttpOnly phone session only after the matching code is
   submitted.
4. Use **Reject** for an unexpected claimant or **Cancel pairing code** to make
   an unclaimed or awaiting-pickup challenge terminal. Challenges expire after
   three minutes and cannot be replayed.

**Revoke** ends one device session; **Sign out** ends the current device; **Revoke
all devices** ends every session, including the current browser. Revocation is
immediate for protected API access. The Office shell still loads so local
recovery can be shown, but it grants no operator API authority.

The PWA service worker is network-only. It does not cache Core responses,
campaign data, credentials or authorization state and offers no offline
administration. Private Tailscale status is reported from Core's health probe;
tailnet membership alone is not a Factory session, and Funnel is not supported.

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
