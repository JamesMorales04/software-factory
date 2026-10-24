# Engineering standard: clean architecture and maintainable code

**Decision:** agreed quality direction; technical details subject to evidence. Applies to all Factory implementation languages and generated changes.

**Rust implementation:** [rust-standards.md](rust-standards.md) is the normative Rust-specific standard for Core, Worker, provider adapters, persistence, governance tools and tests. It specializes these principles with concrete ownership, error, async, security and transaction rules.

## Core design principles

- **Make the smallest correct design.** Favor explicit, readable code over abstraction for its own sake. Complexity must have a real requirement, measured benefit or documented invariant.
- **Cohesion and separation of concerns:** put business rules and state transitions in the domain/application layer, not UI handlers, persistence adapters, provider SDK callbacks or HTTP routes.
- **Dependency direction:** domain policy must not depend on infrastructure details. Use dependency injection/composition and narrow interfaces at actual process, persistence, tool and external API boundaries.
- **SOLID pragmatically:** one reason to change per cohesive module; stable interfaces; substitutability; avoid forcing unrelated clients into giant interfaces; depend on capabilities rather than implementations.
- **DRY = one source of truth for knowledge.** Centralize duplicated invariants, validation rules, policy decisions, protocol schemas and canonical identity logic. Do not abstract accidental similarity between two unrelated operations. Prefer modest duplication to the wrong abstraction.
- **KISS/YAGNI:** do not add extension points, plugin frameworks, compatibility layers, generic repositories or speculative services before a real consumer requires them.
- **Composition over inheritance:** prefer small composable functions, modules and explicit object composition. Introduce strategy/state/adapter patterns only when the concrete variation warrants them.
- **Clear contracts:** stable types, validation at trust boundaries, domain-specific errors, deterministic state transitions, idempotency for external effects, cancellation propagation and bounded retries.
- **Testability without artificial layers:** inject clocks, provider clients or storage interfaces when tests benefit; do not build a mock-heavy parallel architecture that never exercises reality.
- **Concurrency:** make ownership, lifecycle, leases, locks and cancellation explicit; avoid shared mutable state, implicit background workers and unbounded fan-out.
- **Security:** minimum capabilities, sandbox-aware execution, explicit egress, no host secret access, observable authorization boundaries.
- **Performance:** measure before optimizing. Avoid premature cleverness, unbounded caches, excessive requests, unnecessary model calls and large context payloads.
- **Decision mechanism:** document whether a meaningful inference or decision uses `DETERMINISTIC`, `MODEL` or `HYBRID` control and why its reliability, accuracy, latency, token cost, complexity and maintenance fit the task. Do not grow fragile keyword, regex or heuristic expert systems merely to avoid bounded semantic reasoning. Do not call a model for simple verifiable facts or stable policy checks. Reuse sufficient agent context for small structured judgments before creating another model session; independently validate model-informed effects under Core authority.

## Architecture boundaries, not ceremonial Clean Architecture

The Rust Core is a pragmatic modular monolith with hexagonal boundaries. Domain/application, persistence, provider adapters and interface/transport are *logical responsibilities*, not a required number of folders or framework-generated layers. A small feature may use a single module. Introduce a port/adapter when an actual trust, persistence, runtime or test boundary requires it; otherwise prefer concrete types and static dispatch. See [rust-standards.md](rust-standards.md).

Provider adapters translate Codex App Server and OpenCode protocol interactions into a small, versioned Core–Worker contract; no provider-specific types should leak into Core domain policy. React components must render server-authoritative state, not reimplement action or delivery rules.

## Code review checklist

- Are there duplicated business rules, validation, queries, state machines or permissions that need a single source of truth?
- Is each abstraction justified by more than one genuine use or a required boundary?
- Are state and error transitions explicit and independently testable?
- Can a reader trace an Action from input through persistence and evidence to delivery?
- Did an internal contract change leave dead code, deprecation paths or unsupported compatibility glue?
- Were relevant formatters, linters, analyzers, tests and real-user journeys run on the exact candidate?
- Did any lint suppression or dependency bypass reduce the quality baseline without a governed decision?
- Is the chosen decision mechanism explicit and supported by representative evidence, including a deterministic baseline where semantic judgment is proposed?

**Avoid:** god objects, duplicate validation/authorization logic, cyclic module dependencies, clever generics, unnecessary interfaces, premature plugin abstractions, long parameter lists with implicit behavior, silent retries, undocumented magic values, broad `any`/type assertions, ignored errors, and unused fallbacks.

See [Rust standards](rust-standards.md), [linting](linting.md), [quality baseline](../quality/baseline.md), and [early-development compatibility ADR](../architecture/decisions/ADR-0006-early-development-compatibility.md).
