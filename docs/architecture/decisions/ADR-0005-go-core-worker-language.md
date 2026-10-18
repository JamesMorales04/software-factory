# ADR-0005: Go as the Core and Worker implementation language

- **Status:** Accepted
- **Decision date:** 2026-10-18
- **Decision authority:** Product owner
- **Scope:** Factory Core, orchestration modules, local Workers, and future compatible Worker implementations.

## Context

The Factory Core and Workers coordinate HTTP APIs, concurrent tasks, durable workflows, process lifecycles, job recovery, scheduling, and policy enforcement. The primary F1 risk is reliably integrating the Paseo provider with isolated untrusted command execution, not maximizing raw CPU throughput.

Go and Rust were considered. Rust offers strong compile-time ownership and memory-safety guarantees; Go provides a comparatively straightforward standard-library ecosystem for networked services, concurrency, and operational tooling. Either language can implement the core behavior, and neither inherently isolates provider credentials or untrusted subprocesses.

The product owner ratified Go before running SEC-01. This choice is intentional and **does not claim an empirical Go-versus-Rust benchmark was performed**.

## Decision

- **Use Go for Factory Core and Workers.** Preserve a modular monolith for Core and explicit, authenticated Core–Worker contracts.
- **Use TypeScript for the Paseo SDK integration adapter**, subject to real compatibility testing; **React + TypeScript** for Factory Office.
- Adopt the Go quality toolchain: `gofmt`, `go vet`, `go test`, a pinned `golangci-lint` configuration, and `go test -race` for relevant concurrent code, as specified in [linting standards](../../engineering/linting.md).
- Use explicit goroutine lifetime ownership, `context.Context` cancellation, bounded concurrency, disciplined channel synchronization, well-defined shutdown, and idempotent recovery contracts. Use compiler/static checks and race testing, not assumptions.
- **Do not implement or maintain a parallel Rust Core/Worker as speculative fallback.** A future change to language would require a new architectural decision with migration evidence.
- **SEC-01 remains independently blocking** for production-safe Paseo agent execution. A failure of credential/sandbox isolation requires a topology/security redesign, not an automatic reversal of this language decision.

## Consequences

Positive: one implementation language/toolchain for Core and Workers, reduced initial cognitive and CI overhead, straightforward server/concurrency implementation, and a narrower initial F1 scope.

Trade-offs: runtime garbage collection and the need to prevent data races and manage goroutine leaks deliberately. Keep CPU, memory, latency, reliability, and cost measurements from the first slices; investigate measured bottlenecks rather than introducing speculative alternatives.

## Acceptance and follow-up

TECH-01 (language choice) is **closed by explicit product decision**. F0 must still demonstrate that the selected Go toolchain can build and test the Core/Worker skeleton and integrate with the TypeScript adapter. SEC-01 is separate and remains open until the security boundary has been tested with disposable fixtures and synthetic canaries.
