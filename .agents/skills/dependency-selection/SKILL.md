---
name: dependency-selection
description: Evaluate whether a maintained Rust crate should replace custom infrastructure for Git, GitHub, Podman, filesystems, protocols, validation or async lifecycle work, while keeping Factory authorization and policy in Factory code.
---

# Dependency selection

Use before adding a significant Rust dependency, replacing subprocess
integration, or adopting a crate for Git/GitHub/Podman/filesystem/protocol
infrastructure. The normative rules are in `docs/engineering/dependency-selection.md`.

## Procedure

1. Name the capability and the Factory invariant it must not decide.
2. Inspect existing code and dependencies; avoid duplicate capability.
3. Identify realistic maintained candidates and current versions.
4. Check RustSec, license, MSRV (Rust 1.90.0), Linux/Windows support and native
   build dependencies.
5. Compare transitive impact, failure behavior, security boundaries and test
   effort.
6. Build a minimal disposable spike when a security/lifecycle invariant depends
   on the library.
7. Select or reject with a recorded rationale and status.
8. Integrate behind a narrow adapter; keep Core authorization outside it.
9. Run equivalence and negative tests, including cleanup and stale-state cases.
10. Remove superseded code only after verification; update Cargo.lock and docs.
11. Record candidate-bound evidence and continue the functional slice.

## Stop conditions

Stop when a crate hides or weakens authorization, isolation, credential or
delivery policy; when no candidate supports a required security control; when
the MSRV/toolchain is incompatible; or when adoption would create two active
implementations without a verified cutover.

## References

- [Dependency selection](../../../docs/engineering/dependency-selection.md)
- [Rust standards](../../../docs/engineering/rust-standards.md)
- [Service placement policy](../../../docs/architecture/service-placement-policy.md)
- [Dependency evaluation record](../../../docs/engineering/dependency-evaluation-2026-10-23.md)
