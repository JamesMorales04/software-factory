# ADR-0031 — Owner-confirmed subscription test admission

**Status:** Accepted by the Product Owner on 2026-11-04.
**Supersedes:** the provider-issued no-overage trust-anchor requirement in the
Wave 3 real-provider preparation.

## Context

OpenCode Go is a subscription product authenticated with an API key. The
provider does not expose a documented API that proves to Factory that upstream
Extra Usage is disabled or that a request cannot be billed as overage. The
Product Owner controls that setting in the provider Console and accepts
responsibility for keeping it disabled.

Requiring a provider-issued no-overage certificate, a cryptographic proof or a
machine-verifiable Extra Usage status therefore blocked all real-model testing
on evidence the provider does not offer. Treating an API key as proof of
pay-as-you-go billing was also wrong: the key is an authentication method, not
a billing classification.

## Decision

1. **Authentication, route, billing setting, model, authorization and quota are
   separate facts.** OpenCode Go uses `api_key` authentication on the exact
   `opencode-go` subscription endpoint. It is not classified as billable merely
   because the credential is an API key.
2. **The Owner assumption is recorded, not verified.** An authenticated
   operator creates a bounded admission containing the Owner's Extra Usage
   assumption, statement, validity window and the exact provider/model/account
   configuration. Core records it as `OWNER_CONFIRMED_CONFIGURATION` and never
   labels it provider-issued evidence.
3. **The automated acceptance suite has a separate allowlist.** Only
   `opencode-go/deepseek-v4.1-flash` and `codex/gpt-6-luna` are admitted. The
   general product catalog and model selection remain configurable.
4. **Production dispatch stays fail-closed.** A Campaign is admitted only by an
   explicit, request-scoped admission consumed atomically with Campaign
   authorization. A debug `test-support` wildcard is not used for genuine live
   inference.
5. **Routing guarantees remain absolute.** No plain `opencode` API-key route,
   no OpenAI pay-as-you-go endpoint, no balance/credit fallback, no silent
   model/account/provider switch. Exhausted or zero-remaining included quota
   stops or pauses the operation. Unknown quota is not treated as unlimited;
   the admission policy records the limitation and provider errors pause.
6. **Request admission remains atomic and idempotent.** The admission binds the
   intake request, Lead request, frozen profile revision/hash, assessment route,
   implementer route, provider settings revision, Owner assumption and an
   idempotency key. Approval consumes it into an `ENABLED` Campaign policy in
   the same transaction that creates the Campaign authorization.
7. **Codex review is not faked.** The current Codex adapter is a Worker App
   Server implementation transport; it has no one-shot review adapter. Codex is
   not advertised as a generic Chat Completions provider, and a review role is
   not admitted until a native Codex review integration exists.

## Consequences

- The outdated provider-issued no-overage requirement is removed from the test
  admission design.
- The Owner remains responsible for the external Extra Usage setting.
- Live-model acceptance still requires real credentials, an explicit bounded
  authorization and a working Codex review integration; this ADR does not grant
  those.
- General provider/model selection is unchanged.
