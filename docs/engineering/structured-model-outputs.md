# Structured model outputs

**Status:** normative for machine-readable model results. Read with [Lead contracts](../../docs/evidence/history/implementation/s4d-rust-recovery-matrix.md) and [Rust standards](rust-standards.md).

## When required

Use structured output for model-generated assessments, proposals, routing or
recommendations that Factory will validate or persist. Ordinary prose and
conversational replies do not require a schema.

## Pipeline

```text
Rust model-output DTO
  -> generated JSON Schema (schemars)
  -> provider compatibility check
  -> provider structured-output request
  -> terminal completed response
  -> Serde decode
  -> structural JSON Schema validation (jsonschema)
  -> domain invariant validation
  -> advisory result
  -> Core-owned authorization / transition
```

A schema-valid result remains advisory. It never grants authority, budget,
tools, risk approval, delivery rights or Quality Gate acceptance.

## Rules

- Define the DTO once in Rust. Do not maintain separate handwritten schemas for
  Codex and OpenCode when they represent the same logical contract.
- Generate schemas from the DTO (`schemars`). Record the schema version and
  digest with the provider result.
- Validate provider compatibility before dispatch. A provider may support only
  a JSON Schema subset; fail closed on an unsupported keyword.
- Use provider-native structured output:
  - Codex App Server `turn/start.outputSchema`.
  - OpenCode's supported JSON Schema request field.
  Verify the exact installed provider version; do not assume.
- Do not silently parse free-form prose when structured output was required.
- Require terminal completion evidence. A stream opening, partial output or
  disconnect is not a validated result.
- Distinguish unsupported schema, schema rejection, provider refusal, missing
  result, interrupted turn, invalid JSON, deserialization failure, domain
  failure, timeout, cancellation and retry exhaustion.
- Retries are explicit, bounded, correlated, budgeted and idempotent. Count
  provider-managed and Factory-managed retries.
- Persist schema version/digest, provider/model identity, validation outcome and
  available usage metadata. Do not invent usage or cost.
- Structural validation is not domain validation. Core still checks risk,
  WorkGraph, paths/capabilities, delegation scope, budgets, evidence references,
  route eligibility and candidate identity.
- A structurally valid model result cannot authorize its own execution.
