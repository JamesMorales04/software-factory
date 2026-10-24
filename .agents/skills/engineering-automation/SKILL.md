---
name: engineering-automation
description: Decide where repository engineering automation belongs, extend cargo-xtask only for justified cross-tool orchestration, preserve independent Rust/pytest/Playwright test ownership, and keep the installed Factory runtime independent from development tooling.
---

# Engineering automation

Use this skill before adding or changing repository automation, test orchestration, CI entrypoints, image-build workflows or scripts. The canonical rules are in `docs/engineering/automation-standards.md`; this skill is the procedure.

## Procedure

1. **Name the responsibility.** Is this a product command, a test scenario/assertion, a single-tool invocation, or genuine cross-tool orchestration?
2. **Check for an existing tool.** Cargo, pytest, Playwright, Podman or an existing xtask command may already do the work.
3. **Choose the location.** Product lifecycle → `factory` CLI. Single build/test command → Cargo/tool. Cross-tool workflow → an existing or new `cargo xtask` command. Test assertions → the test suite. Container topology → the service-placement policy.
4. **Extend, do not duplicate.** Reuse an existing xtask module/command when the responsibility matches. Do not create a second path for CI or developers.
5. **Design the command.** Explicit argv, no shell interpolation, correct exit propagation, bounded output, disposable unique resources, timeouts and cleanup.
6. **Protect boundaries.** No paid provider session from default commands; no privileged Podman socket, host HOME or provider credential exposure; no bypass of Core authorization.
7. **Keep tests independent.** Xtask may select and launch suites but must not become their only oracle. Preserve negative, recovery, security and cleanup assertions.
8. **Update discoverability.** Add help text, documentation and the E2E migration matrix where applicable. Remove superseded wrappers only after verified replacement.
9. **Verify in the container.** Run the command from the canonical development container on the exact candidate. Record what ran and what remains `NOT_TESTED`/`BLOCKED`.
10. **Commit and hand off.** Add candidate-bound results to the handoff. Do not claim live certification or Windows verification without real evidence.

## Stop conditions

Stop and report when the automation would need paid provider access, privileged host/Podman access, credentials or a network service without explicit authorization; when it duplicates an existing workflow; when it weakens an independent test; or when the installed product would depend on development tooling.

## References

- [Automation standards](../../../docs/engineering/automation-standards.md)
- [Rust standards](../../../docs/engineering/rust-standards.md)
- [Service placement policy](../../../docs/architecture/service-placement-policy.md)
- [E2E migration matrix](../../../docs/implementation/e2e-migration-matrix.md)
- [Container-first development](../../../docs/engineering/container-first-development.md)
