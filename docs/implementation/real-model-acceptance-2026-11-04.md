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
| Independent review | `codex` | `gpt-6-luna` | Native Codex App Server `review/start` transport |

The test allowlist is in `native/src/subscription_admission.rs`. It is not a
product catalog restriction. This admission contract currently executes only
the `opencode-go` route. Codex is not admitted as subscription-funded
implementation because Core cannot verify the credential class; the Codex
pair is reserved for the independent review transport. Plain `opencode` and
every OpenAI pay-as-you-go route are rejected.

Native Codex review is implemented behind `FACTORY_ENABLE_CODEX_REVIEW=1`.
See [Codex ChatGPT authentication and native review](../operations/codex-review-auth.md)
for credential placement, login, status and review evidence.

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

## Implemented review integration

- Dedicated `factory-codex-home` volume, mounted only into the reviewer and the
  one-shot `codex-auth`/`codex-status` services.
- Official App Server `account/login/start` device-code or browser login;
  `account/read` must return a ChatGPT account.
- Exact `gpt-6-luna` check through `model/list`; API-key and ambiguous states
  are rejected.
- Fresh disposable candidate snapshot, read-only App Server sandbox,
  `review/start` targeting the exact commit, bounded timeout/interrupt and
  cleanup.
- Candidate-bound `CODEX_APP_SERVER` quality receipt with exact provenance;
  missing usage is `UNKNOWN` and cannot produce a PASS.

## Remaining concrete blockers

- **No Factory-held OpenCode Go credential.** The key is stored by the host
  OpenCode CLI; it was not read or copied into Factory. The Owner or an
  authorized operator must enter it through Office/provider settings.
- **No Factory Codex login has been completed yet.** The dedicated volume is
  ready; an operator must run the login command and complete the official
  ChatGPT flow.
- **No bounded live authorization envelope has been executed.** The corrected
  policy permits an explicit bounded test admission, but no live run has been
  started or charged.
- **No real inference evidence.** Authentication and review were tested only
  with the deterministic fixture App Server and local non-inference protocol
  probing.

## NOT_TESTED

- Real DeepSeek assessment, planning or implementation through Factory.
- Real GPT-6 Luna review through Codex ChatGPT. The deterministic fixture
  covers the protocol, auth rejection paths, review parsing, cancellation and
  receipt conversion.
- Provider quota exhaustion/pause against a real account.
- Full natural-language objective to candidate to independent review to Quality
  Gate journey with two real models.
