# Bounded live-provider smoke authorization — 2026-10-24

**Status:** authorization record for one read-only smoke turn per provider. This is not sandbox, isolation, or complete provider certification.

## Source and scope

Derived from the Product Owner directive of 2026-10-24: "Begin Bounded Live-Provider Certification Now" with **at most one small real model turn per provider initially**, a defined usage ceiling, no automatic retries, no write/tool authority, and explicit evidence. Authorization id: `po-directive-2026-10-24-readonly-smoke`.

## Exact envelope

| Item | Codex | OpenCode |
|---|---|---|
| Provider | `codex` CLI/App Server 0.156.1 | `opencode` 1.18.32 |
| Model | `gpt-6-luna` (discovered in `model/list`; fast/affordable) | `opencode/deepseek-v4.1-flash` (discovered route) |
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
- **OpenCode `opencode/deepseek-v4.1-flash`:** one read-only prompt request was attempted; the provider endpoint returned HTTP 401 `Invalid credential` before any model output. No tokens or cost were reported, no retry was made, and the session was deleted and verified absent. Evidence: [provider-live-smoke-2026-10-24-opencode.json](../evidence/provider-live-smoke-2026-10-24-opencode.json). Classification: `LIVE_PROVIDER_CREDENTIAL_BLOCKED`.

The OpenCode credential must be renewed by the operator before a new bounded authorization. Do not silently substitute another provider/model or retry within this envelope.
