# ADR-0008: Language-appropriate linting and static quality gates

- **Status:** Accepted (product-owner decision, tools conditional on TECH-01)
- **Context:** Autonomous implementation needs objective, repeatable checks in addition to semantic review. A one-size-fits-all toolchain would waste resources; an entirely manual style policy is unenforceable.
- **Decision:** Adopt the checks in `docs/engineering/linting.md`. Markdown is governed from the documentation stage. Once the implementation language is selected, pin formatter, linter/static analyzer, compiler/type check and tests for every active language. Fail mandatory gates closed. Prefer Biome + `tsc` for TypeScript; use `gofmt`, `go vet` and a pinned `golangci-lint` if Go is selected, or `cargo fmt`/Clippy if Rust is selected. Scope special tools to actual needs.
- **Consequences:** More consistent code and reproducible review, with some CI overhead. Formatters and linter versions must be measured, pinned and updated deliberately. A lint tool does not replace end-to-end user verification.
