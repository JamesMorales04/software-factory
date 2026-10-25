---
name: structured-model-outputs
description: Implement typed schema-constrained Codex/OpenCode model outputs using Rust-derived JSON Schema, provider compatibility checks, strict decoding and deterministic domain validation, without granting model output any Core authority.
---

# Structured model outputs

Use for assessments, WorkGraph proposals, semantic judgments, routing
recommendations or any machine-readable model result. The normative rules are in
`docs/engineering/structured-model-outputs.md`.

## Procedure

1. Define or reuse a Rust model-output DTO; keep Core domain state separate.
2. Derive JSON Schema (`schemars`) and record its version/digest.
3. Run the conservative provider-compatibility check before dispatch.
4. Send the schema through the provider's native structured-output field
   (Codex `turn/start.outputSchema`, OpenCode's supported field).
5. Require terminal completed evidence; reject partial/stream-only output.
6. Decode with Serde and validate against the generated JSON Schema.
7. Apply domain invariants (identity, evidence, effort, scope, graph, budgets).
8. Classify failure (`UNSUPPORTED_SCHEMA`, `SCHEMA_REJECTED`, `PROVIDER_REFUSAL`,
   `MISSING_RESULT`, `INTERRUPTED`, `INVALID_JSON`, `DOMAIN_FAILURE`, `TIMEOUT`,
   `CANCELLED`, `RETRY_EXHAUSTED`).
9. Retry only explicitly, boundedly and idempotently; persist usage where known.
10. Hand the advisory result to Core; never let it grant authority or bypass a
    protected decision.

## Stop conditions

Stop when the provider cannot produce a supported structured result, the schema
uses an unverified keyword, the result is not terminal, domain validation
fails, or a retry would create duplicate paid effects without authorization.

## References

- [Structured output doctrine](../../../docs/engineering/structured-model-outputs.md)
- [Rust standards](../../../docs/engineering/rust-standards.md)
- [Dependency selection](../../../docs/engineering/dependency-selection.md)
