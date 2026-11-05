# ADR-0032 — Dedicated Codex ChatGPT authentication and native review

**Status:** Accepted by the Product Owner on 2026-11-05.
**Relates to:** ADR-0010 (frozen candidate quality pipeline), ADR-0018
(native Rust), ADR-0024/0025 (container-native and Office parity), ADR-0031
(Owner-confirmed subscription admission).

## Context

The Wave 3 acceptance suite needs an independent model review, but Factory must
not copy or mount the host's complete `CODEX_HOME`, expose a provider credential
to generic Workers or project tools, or fabricate a review through a generic
chat-completions endpoint. Codex supports an official App Server with
`account/read`, `account/login/start`, `model/list`, `review/start` and
`thread/read`.

## Decision

1. **One dedicated credential boundary.** Factory uses a dedicated
   `factory-codex-home` volume mounted only into the reviewer and the one-shot
   `codex-auth`/`codex-status` services. The reviewer image contains the pinned
   Codex App Server. No host `CODEX_HOME`, host HOME, Worker, project workspace,
   browser storage or CI artifact receives the credential.
2. **Official authentication.** Login uses `account/login/start` with the
   official ChatGPT device-code or browser flow. Status uses `account/read`.
   API-key authentication and unsupported or ambiguous account states are
   rejected for the acceptance profile.
3. **Exact model identity.** `model/list` must contain exactly one visible
   `gpt-6-luna` entry. Review threads request that exact model and the `openai`
   App Server provider, with provider-model fallback disabled.
4. **Native review transport.** A fresh disposable snapshot is created from
   the frozen repository and exact candidate commit. The reviewer starts a new
   read-only App Server thread and calls `review/start` with the commit target.
   It requires terminal completion, exact thread/turn identity, unchanged
   candidate HEAD/tree and a bounded timeout/interrupt. It never edits the
   candidate and removes the snapshot afterwards.
5. **Fail-closed receipts.** Review output is converted conservatively into
   Factory findings. A clean result requires the canonical `No findings.`
   marker or an explicit correct-overall statement; recognised priority
   headings become findings; unknown text is `NOT_TESTED`. The quality receipt
   uses mechanism `CODEX_APP_SERVER`, provider `codex`, model `gpt-6-luna`, no
   model reservation and a request hash. Missing usage remains `UNKNOWN` and
   cannot produce `PASS`.
6. **Opt-in activation.** Codex review is enabled by
   `FACTORY_ENABLE_CODEX_REVIEW=1`; the default Compose stack keeps it off. The
   reviewer is the only permanent service granted `factory-egress` in addition
   to the control network, because it owns the ChatGPT credential and must
   reach the official endpoints.

### Trust boundary

Core cannot independently attest a Codex App Server turn without provider-issued
evidence. A `CODEX_APP_SERVER` PASS receipt is authored by the dedicated
reviewer role; Core validates candidate binding, mechanism, exact
`codex/gpt-6-luna` provenance, the no-reservation rule, known usage and the
request-hash shape. A compromised reviewer token is therefore equivalent to a
compromised reviewer process, exactly as for other reviewer-authored receipts.
The receipt is not described as provider-verified. A candidate or model output
never becomes authority.

## Consequences

- Native review requires the reviewer image and the dedicated Codex volume.
- The Owner must complete the official ChatGPT login once per Factory volume;
  this is a CLI/Compose bootstrap, not a browser-stored credential.
- Real Codex inference still requires a separate bounded execution
  authorization. Deterministic fixture tests and local protocol probing do not
  prove real-provider behavior.
- The OpenCode Go Extra Usage assumption remains governed by ADR-0031 and is
  not upgraded to provider-verified evidence by this ADR.
