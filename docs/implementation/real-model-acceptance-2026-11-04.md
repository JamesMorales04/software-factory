# Real-model acceptance readiness — 2026-11-04

**Status:** admission contract implemented; live execution **BLOCKED** on
credentials and the Codex review integration. No real inference has been
performed for this document.

This document supersedes the provider-issued no-overage requirements in the
older real-provider runbook. [ADR-0031](../architecture/decisions/ADR-0031-owner-confirmed-subscription-admission.md)
is the decision authority.

## Approved automated-test routes

| Role | Provider | Model | Transport |
|---|---|---|---|
| Director / assessment / planning | `opencode-go` | `deepseek-v4.1-flash` | OpenCode Go subscription inference endpoint |
| Implementation | `opencode-go` | `deepseek-v4.1-flash` | OpenCode provider session |
| Implementation | `codex` | `gpt-6-luna` | Codex App Server |
| Independent review | `codex` | `gpt-6-luna` | **not implemented**; Codex App Server has no one-shot review adapter |

The test allowlist is in `native/src/subscription_admission.rs`. It is not a
product catalog restriction. `opencode-go` API-key authentication is accepted
for the subscription route; plain `opencode` and every OpenAI pay-as-you-go
route are rejected.

## Admission flow

1. Authenticated operator creates the intake request with an executable
   execution profile.
2. `POST /intake/requests/{id}/subscription-admission` records the exact
   request, Lead request, profile revision/hash, assessment route, implementer
   route, provider settings revision, Owner Extra Usage assumption, validity
   window and idempotency key.
3. Lead assessment proceeds only while that request/Lead admission is active.
4. `POST /intake/requests/{id}/approve` consumes the admission in the same
   transaction that creates the Campaign authorization. The Campaign is
   classified `SUBSCRIPTION`, has no monetary envelope, and receives an
   `ENABLED` Owner-confirmed funding policy.
5. Worker provider turns use non-billable token bounds. Exhausted or
   zero-remaining quota pauses; unknown quota is not treated as unlimited.
6. Semantic review through OpenCode Go is allowed by the test allowlist; the
   independent Codex review path is still missing.

## Remaining concrete blockers

- **No Factory-held OpenCode Go credential.** The key is stored by the host
  OpenCode CLI; it was not read or copied into Factory. The Owner or an
  authorized operator must enter it through Office/provider settings.
- **No native Codex ChatGPT credential custody.** Codex is ChatGPT-authenticated
  on the host, but Factory's Codex provider session uses a private empty
  `CODEX_HOME` and an egress token. There is no reviewed path to mount or
  delegate the ChatGPT login into a Factory reviewer.
- **No Codex review adapter.** `ProviderAdapter::CodexAppServer` implements a
  Worker provider session only. A one-shot review through Codex must be built as
  a native read-only reviewer transport before the requested independent Luna
  review can be claimed.
- **No bounded live authorization envelope has been executed.** The corrected
  policy permits an explicit bounded test admission, but no live run has been
  started or charged.

## NOT_TESTED

- Real DeepSeek assessment, planning or implementation through Factory.
- Real GPT-6 Luna review through Codex.
- Provider quota exhaustion/pause against a real account.
- Full natural-language objective to candidate to independent review to Quality
  Gate journey with two real models.
