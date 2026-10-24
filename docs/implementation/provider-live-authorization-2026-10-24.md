# Bounded live-provider smoke authorization — 2026-10-24

**Status:** authorization record for one read-only smoke turn per provider. This is not sandbox, isolation, or complete provider certification.

## Source and scope

Derived from the Product Owner directive of 2026-10-24: "Begin Bounded Live-Provider Certification Now" with **at most one small real model turn per provider initially**, a defined usage ceiling, no automatic retries, no write/tool authority, and explicit evidence. Authorization id: `po-directive-2026-10-24-readonly-smoke`.

## Exact envelope

| Item | Codex | OpenCode |
|---|---|---|
| Provider | `codex` CLI/App Server 0.156.1 | `opencode` 1.18.32 |
| Model | `gpt-6-luna` (discovered in `model/list`; fast/affordable) | `opencode-go/deepseek-v4.1-flash` (corrected OpenCode Go route; `opencode-go` and `opencode` remain distinct) |
| Requests | 1 read-only turn | 1 read-only prompt |
| Tools/write | Denied; read-only sandbox policy | `agent: plan`, write/bash/edit/patch tools false |
| Output | `turn/start.outputSchema` + strict Rust/jsonschema validation | `/session/{id}/message` `format: json_schema` + strict validation |
| Usage ceiling | 1024 output tokens; interrupt on exceed | 1024 total tokens; measured after response |
| Retries | 0 | 0 |
| Timeout | 240 s | 240 s |
| Cleanup | archive thread, then process exit | delete session and verify absence |
| Candidate | exact clean Git HEAD | exact clean Git HEAD |

No credential, prompt body, or raw environment value is written to evidence. Evidence is written by `factory-provider-smoke` to a caller-specified path.

## Non-claims

A successful smoke establishes bounded live protocol/structured-output compatibility for the exact provider/model/version. It does **not** certify sandbox isolation, credential confinement, coding-work quality, candidate acceptance, or complete campaign E2E. No retry or second paid turn is authorized. Any protocol incompatibility must be fixed and deterministic fixtures updated before another authorization request.


## Execution result (2026-10-24)

- **Codex `gpt-6-luna`:** one read-only structured turn completed. 29 output tokens (18,537 total reported), schema validation passed, thread archive response recorded. Evidence: [provider-live-smoke-2026-10-24-codex.json](../evidence/provider-live-smoke-2026-10-24-codex.json). Classification: `LIVE_STRUCTURED_OUTPUT_VERIFIED`.
- **OpenCode `opencode-go/deepseek-v4.1-flash`:** one read-only prompt request was attempted; the provider endpoint returned HTTP 401 `Invalid credential` before any model output. No tokens or cost were reported, no retry was made, and the session was deleted and verified absent. Evidence: [provider-live-smoke-2026-10-24-opencode.json](../evidence/provider-live-smoke-2026-10-24-opencode.json). Classification: `LIVE_PROVIDER_CREDENTIAL_BLOCKED`.

The OpenCode credential must be renewed by the operator before a new bounded authorization. Do not silently substitute another provider/model or retry within this envelope.


## Provider identity correction (2026-10-24)

`opencode models opencode-go` confirms `opencode-go/deepseek-v4.1-flash`. The
earlier smoke used the distinct ordinary `opencode` provider and failed with
HTTP 401. The native smoke now parses and preserves the exact provider id
(`opencode-go`), rejects unknown/malformed routes and classifies 401/403 as
`LIVE_PROVIDER_AUTH_BLOCKED`. Deterministic tests pass.

A new live OpenCode turn requires fresh explicit authorization beyond this
record; no paid retry was made after the provider-identity fix.


## Fresh OpenCode Go authorization (2026-10-24)

Product Owner authorizes one additional read-only OpenCode Go turn:
`opencode-go/deepseek-v4.1-flash`, maximum 1 model request, 256 requested
output tokens if enforceable, smallest practical input, no tools/writes, no
retries, native structured output, strict validation, deletion and sanitized
evidence. Authorization id: `po-directive-2026-10-24-opencode-go-256`.

Provider metadata confirms `opencode-go` is connected and exposes
`deepseek-v4.1-flash`. The OpenCode message API does not expose a request-level
output-token cap; the 256-output-token limit is therefore reconciled after the
response and is **not** a hard total-token cap. No total-token ceiling is
asserted. The smoke tool resolves provider/model against `/provider` before the
prompt and refuses unknown/unconnected routes.


## OpenCode Go live result (2026-10-24)

`LIVE_STRUCTURED_OUTPUT_VERIFIED`.

- Candidate: `0601a6d8af59314b62ed8e9a92d8c7958f8bc7f3`.
- Provider: `opencode-go` (`OpenCode` 1.18.32); model `deepseek-v4.1-flash`.
- One request: `msg_16becba690022DVSPG0M81bBkL`, session `ses_e941345e9ffe32yqIb9BacUrZ7`.
- Requested output limit 256; actual output 67 tokens. Total usage 7,372 (input 5,513, cached read 1,792); cost 0.000872526. The 256 limit is an output reconciliation, not a total cap.
- Native `format: json_schema` accepted; strict Rust/jsonschema validation passed.
- Session deleted; subsequent GET returned 404. No retry was made.
- Evidence: [provider-live-smoke-2026-10-24-opencode-go.json](../evidence/provider-live-smoke-2026-10-24-opencode-go.json).

This is bounded read-only protocol/structured-output evidence, not coding or sandbox certification.
