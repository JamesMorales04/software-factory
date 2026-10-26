# Codex GPT-6 Luna live UAT — 2026-10-26

**Result:** PASS for a single bounded Codex coding turn ending in a Git candidate
with independent verifier PASS, deterministic Reviewer PASS and Quality Gate
`ACCEPTED`.

| Item | Value |
|---|---|
| Provider/model | `codex` / `gpt-6-luna` |
| Codex App Server | `0.156.1` |
| Execution mode | `TRUSTED_LOCAL_PROVIDER` (not isolated) |
| Candidate record | `1d79d4e8-46da-4120-a13d-db6c152ae90b` |
| Candidate commit | `635336d3d525a20ab4079ae04586c4e2f7f1ab74` |
| Source change | `src/lib.rs`: `discount` now returns `cents - cents / 10` |
| Verifier | deterministic executed `cargo test` PASS |
| Reviewer | deterministic PASS (semantic model review not run) |
| Gate | `ACCEPTED` |
| Session state | `COMPLETED` |
| Cleanup | `sandbox_process_identity_absent` |
| Provider usage | 6,342 input + 186 output = 6,528 tokens; known |
| Configured-rate cost | 7,086 micros (USD 0.007086 at the recorded configured rates) |
| Egress requests | 3, all `gpt-6-luna`, all bound to Core |
| Retries | none after the accepted candidate |
| Disposable repo | one Rust repository; allowed paths `src/` |

The model used only `write_file` and `run_command`; the command was the frozen
`cargo test` check. The Factory egress broker injected the real upstream
credential; the sandbox saw only a per-run token. Cleanup was confirmed by
sandbox process absence.

**Limitations.** Live inference used a test-only local proxy that mapped the
Factory `/v1/responses` egress to the authenticated ChatGPT Codex backend. That
proxy is not part of the merged Factory runtime, so native ChatGPT-backend
configuration remains a follow-up. The Reviewer was deterministic, not a live
semantic model. Only one session/three requests were run; cancellation,
revocation, ISOLATED execution, human security certification and deployment
were not tested. The cost is a configured-rate estimate, not an external invoice.
