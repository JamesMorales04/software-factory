# Maintainability and module organization reference

Use with `docs/engineering/rust-standards.md` §3. These are review heuristics,
not arbitrary CI thresholds.

## Module boundaries

Organize by cohesive change reason and real architectural boundary:

```text
domain/          authorization, campaign, execution, quality policy
application/     scheduling, recovery, dispatch orchestration
persistence/     SQLite schema, transactions, projections
providers/       codex, opencode, shared session lifecycle
workspace/       allocation, candidate verification, integration
sandbox/         confinement preflight and ownership ledger
api/             HTTP routes and durable event projection
worker/          claim, execute, report, reconcile
tools/           governance and hygiene binaries
telemetry/       redacted structured events
```

This is a responsibility map, not a mandate to create every folder or crate.
A small cohesive module may own several adjacent responsibilities; a large
`db.rs`, `runtime.rs` or `core.rs` that mixes unrelated change reasons is the
problem, not its filename.

- Split when a file contains multiple independently changing policies or when a
  reviewer must page through unrelated setup to understand one behavior.
- Keep shared invariants and error types in the parent module and expose only
  the narrow `pub(super)` helpers submodules need.
- Do not create a crate for every component. A crate needs a dependency,
  compilation, ownership or security boundary.
- Keep binary entry points thin. Move testable behavior into library modules.

## File size review

| Implementation lines | Action |
|---|---|
| < 400 | No action unless responsibility is unclear |
| 400–600 | Acceptable if cohesive |
| 600–800 | Look for an independently changing responsibility |
| 800–1,000 | Strongly consider a split; document why if kept |
| > 1,000 | Required cohesion justification and refactoring assessment |

- Exclude `#[cfg(test)]` code from the implementation count; assess test
  organization separately.
- A dense protocol table, generated binding or single algorithm may justify a
  larger file. Document the reason and keep navigation aids (`//!` module doc,
  clear section boundaries).
- Do not create `part1.rs`/`part2.rs` splits that leave a shared mutable
  invariant spread across files. Split by ownership and policy.

## Function review

Inspect functions over roughly 100 lines. Common genuine problems:

- a transaction plus network/provider work plus projection mapping;
- several independent validation policies in one match/loop;
- deeply nested `if let`/match combinations that obscure failure precedence;
- large test fixtures embedded in production modules;
- manual row mapping mixed with policy decisions.

Prefer a named helper for one coherent sub-policy. Avoid extracting a one-line
wrapper that only forwards arguments and hides nothing.

## Test organization

- Keep pure-domain tests inline when they are short and close to the policy.
- Move large fixture builders, process/repository setup and long scenario tests
  into a dedicated `tests` submodule or `tests/` integration file.
- Keep negative cases beside the positive behavior they constrain.
- Do not move tests merely to lower a line count; move them when navigation and
  responsibility boundaries improve.

## Refactoring method

1. Record the behavior and tests before moving code.
2. Move one cohesive responsibility at a time; keep public behavior identical.
3. Re-export stable public items from the parent module so callers do not churn.
4. Keep error mapping, transaction boundaries and authorization checks in the
   same layer as before unless the move explicitly improves them.
5. Run the full relevant suite after each structural move, not only at the end.
6. Review the resulting module tree for cycles and leaked internals.

## Review questions

- Can a reviewer name the responsibility of this file in one sentence?
- Does each function have one reason to change?
- Are transaction, network and projection concerns separated?
- Is the large file cohesive, or does it hide three different policies?
- Are tests easy to locate and run for the behavior they cover?
- Did the refactor preserve exact identity, authorization, fencing and cleanup
  semantics?
