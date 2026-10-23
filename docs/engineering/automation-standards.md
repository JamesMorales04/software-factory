# Engineering automation standards

**Status:** normative for repository engineering automation. **Applies to:** developers, CI and autonomous agents.

This doctrine defines where build, test, container and governance automation belongs. It complements [Rust standards](rust-standards.md), [lint policy](linting.md), [container-first development](container-first-development.md) and the [service placement policy](../architecture/service-placement-policy.md).

## 1. Ownership

| Responsibility | Canonical owner |
|---|---|
| Rust build, check, format, dependency resolution and native tests | Cargo |
| Cross-tool engineering workflows, environment preparation, test selection and aggregation | `cargo xtask` |
| Actual test scenarios and independent assertions | Rust tests, pytest/Python where justified, Playwright for browser journeys |
| Product installation, runtime lifecycle and operator commands | `factory` CLI |
| Container execution and persistent deployment | Podman/Quadlet |
| CI invocation and enforcement | GitHub Actions calling the same canonical commands |
| Small environment-specific wrappers | Shell only when no existing tool or xtask command fits |

Xtask coordinates; tests verify; Podman executes; Factory governs. Xtask is not a second Core, policy engine, scheduler, sandbox authority, test framework or product runtime dependency.

## 2. Decision rules

**A — Prefer built-in tools.** If `cargo test`, `cargo fmt`, `cargo clippy`, `cargo audit`, pytest, Playwright or Podman already do the job, invoke them rather than reimplementing them.

**B — Use xtask for meaningful orchestration.** Add an xtask command when a workflow combines multiple tools, prepares disposable prerequisites, aggregates multiple suites or must be identical in developer and CI use. A single cargo command does not need an xtask wrapper.

**C — Preserve independent tests.** Xtask launches suites and aggregates status; assertions, fixtures, oracles, evidence and independent acceptance remain in the test suite.

**D — Avoid script proliferation.** A new general-purpose Bash or Python entrypoint needs a documented reason the existing automation interface is insufficient. Do not add a wrapper solely to preserve an obsolete interface.

**E — No duplication.** One engineering workflow has one authoritative implementation. CI, developers and agents use the same command or the same underlying suite.

**F — Protect security boundaries.** Automation must not silently grant privileged Podman access, mount host credentials, expose provider tokens, start paid sessions, bypass Core authorization or mark skipped/blocked work as PASS.

**G — Keep product and engineering separate.** The installed Factory must operate without Cargo, xtask, Python, pytest, Node or the development container. `factory` commands must not call xtask.

**H — Maintain portability.** Automation runs in the canonical Linux development container on Linux and on Windows through Podman Machine/WSL2. Do not depend on personal paths, host-installed Go/Paseo/mise, hidden services or fixed ports.

## 3. Command design

- Use explicit argument parsing and argument-based process spawning. Never build shell strings from untrusted values.
- Propagate child exit status. Do not report success after a failed subprocess.
- Print the command, working directory where relevant, and a bounded diagnostic on failure.
- Use disposable, unique resources with bounded timeouts and cleanup. Never delete user repositories, databases or unrelated containers.
- Do not launch a paid provider session from `check`, `test`, default `e2e` or `security`. Live certification requires a separate explicit authorization envelope.
- Keep command modules cohesive: checks, tests/E2E, images, governance and migrations are different change reasons.
- Update help output and documentation when adding or changing a command.
- Implement a command only when it performs real, tested work. Do not create placeholder commands that imply capability.

## 4. Test ownership and E2E

- Unit tests cover pure domain logic and focused algorithms.
- Rust integration tests cover persistence, transactions and internal contracts.
- System E2E exercises the real product interface and external outcomes: Git revisions, process restart, durable state, browser-visible behavior, authorization, evidence and cleanup.
- Independent acceptance must not reuse the implementation's own validation as its only oracle.
- Deterministic suites use protocol fixtures and never require a paid model. Provider-backed certification is separate, exact-SHA, budget-bound, explicitly authorized and recorded as such.
- A test is `MIGRATED_VERIFIED` only when the relevant assertions execute and pass on an exact candidate. A file with the same name is not migration evidence.
- Preserve historical E2E results as historical evidence; never present them as verification of the Rust implementation.

See the [E2E migration matrix](../implementation/e2e-migration-matrix.md) for the current inventory and status.

## 5. Scripts

Classify each script as: replaced by Cargo/xtask, moved into a test suite or fixture, deployment infrastructure, a small justified wrapper, retired after verified replacement, or retained historical evidence. Do not blindly translate Bash/Python to Rust. Preserve meaningful assertions, security checks and certification guards.

## 6. CI

- CI invokes the same canonical commands used locally; do not maintain a CI-only test implementation.
- Keep lanes independently understandable: format/lint, Rust tests, Office, migrations, deterministic E2E, security, governance, image checks and separately authorized live certification.
- Pin tool versions and use lockfiles. Do not make success depend on an undeclared cache or a developer database.
- Report `SKIPPED`, `BLOCKED`, `NOT_TESTED` and `NOT_APPLICABLE` truthfully.

## 7. Review checklist

- Is the responsibility actually automation, a product command or a test?
- Does an existing tool or xtask command already cover it?
- Is the new entrypoint justified and documented?
- Does it duplicate another workflow?
- Are failures, timeouts and cleanup handled?
- Can it start a paid provider or gain privileged host access? If yes, is that explicitly authorized and isolated?
- Does it work in the development container and preserve Windows-through-Podman portability?
- Are independent assertions preserved?
- Is help output and documentation updated?
- Was the command exercised on the exact candidate?
