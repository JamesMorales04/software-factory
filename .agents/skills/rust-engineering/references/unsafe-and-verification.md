# Unsafe and verification reference

## Policy

- First-party production code should avoid `unsafe` unless there is a real
  boundary that safe Rust cannot express.
- Every `unsafe` block needs a `// SAFETY:` comment naming the invariant, the
  caller obligations and why the current context satisfies them.
- Do not use `unsafe` to silence a borrow error, avoid a clone or bypass a
  validated constructor.
- Do not expose a public safe API whose soundness depends on an undocumented
  caller obligation. Seal it behind a validated constructor or make it `unsafe`.

## Miri

Use Miri selectively for modules with unsafe/FFI/concurrency:

```sh
rustup +nightly component add miri
MIRIFLAGS="-Zmiri-strict-provenance" cargo +nightly miri test
```

Miri finds undefined behavior in executed paths; it is not a proof for all
inputs and is much slower than normal tests. Do not run it on the whole
workspace on every PR unless the cost is justified.

## Kani

Use Kani only for a small, critical, pure invariant where exhaustive bounded
checking is worth the cost (budget/lease arithmetic, canonical ordering,
protocol state). A Kani harness is code: review its assumptions.

## Other tools

- `cargo-geiger` for an inventory of unsafe usage; not a correctness proof.
- `cargo-auditable`/SBOM for dependency provenance, not vulnerability prevention.
- `cargo-vet` for human-audit provenance, not runtime safety.

## Review checklist

- Is the unsafe block necessary?
- Is the safety argument complete and local?
- Are aliasing, provenance, alignment, initialization and thread-safety
  obligations stated?
- Is there a negative test for the invalid input?
- Is the FFI boundary checked for ownership, lifetime, panic and null?
- Is Miri/Kani evidence candidate-bound and recorded?
- Does a safe wrapper preserve the invariant for all callers?

## Stop conditions

Stop and report if an unsafe change cannot state its safety argument, if the
only evidence is “it passed once”, or if the change would require a stronger
isolation/security claim than the product currently supports.
