# ADR-0005: Core implementation language — Go versus Rust

- **Status:** Proposed / decision pending; do not treat Go as ratified.
- **Context:** Core and Worker orchestrate HTTP APIs, concurrent jobs, durable state and subprocesses. Paseo's client SDK is TypeScript; the most significant risk is isolation of provider credentials from untrusted agent tools, not raw CPU throughput.
- **Alternatives:** Go (simple server/concurrency ergonomics, likely faster F1 delivery) and Rust (ownership guarantees and tighter memory control at increased initial implementation complexity).
- **Preferred hypothesis:** Go for Core/Worker, TypeScript for the Paseo adapter. Neither language automatically guarantees OS sandboxing or secret separation.
- **Required experiment:** Compare a small vertical slice: API, durable job, subprocess launch, persistence/recovery and measurable engineering effort, CPU/RAM, latency, defect risk and integration complexity. Decide before the first actual Core implementation commit.
- **Consequences:** For Go, use race detection and explicit concurrency ownership. For Rust, adapt toolchain/CI without changing domain contracts. Do not maintain both production stacks for speculative compatibility.
