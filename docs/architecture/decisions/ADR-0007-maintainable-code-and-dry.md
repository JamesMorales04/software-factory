# ADR-0007: Clean, cohesive, non-duplicative implementation

- **Status:** Accepted (product-owner decision)
- **Context:** A multiproject autonomous system can accumulate repetitive state machines, authorization logic, wrappers and generated scaffolding. Unnecessary abstraction is itself maintenance debt.
- **Decision:** Use the engineering standard in `docs/engineering/code-quality.md`: cohesive modules, explicit dependencies, clean boundaries around policy and infrastructure, compositional design, pragmatic SOLID, DRY for duplicated knowledge, and KISS/YAGNI for avoiding speculative layers. Keep authoritative state and policy in the Core, never in agent prompts or UI projections. Every significant change undergoes a duplication/complexity check during Review.
- **Not a mandate:** Do not require a design-pattern catalog, an interface for every function, or rigid concentric layer directories. Patterns serve an existing problem, not the reverse. Incidental repetition may be simpler than an incorrect shared abstraction.
- **Consequences:** Reviewers must check maintainability and removal of obsolete code, not merely tests. Small refactors and deduplication should accompany feature changes when in scope; large unrelated refactors require separate Actions.
