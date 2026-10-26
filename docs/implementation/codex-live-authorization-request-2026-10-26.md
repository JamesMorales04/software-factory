# Codex GPT-6 Luna live authorization request (2026-10-26)

**Status:** REQUEST ONLY. No billable live inference is authorized by this
document. A prior approval, protocol probe, mock turn, or deterministic test
does not authorize these turns.

## Requested envelope

| Field | Value |
|---|---|
| Provider route | `codex/gpt-6-luna` |
| Provider executable | Codex CLI/App Server `0.156.1`, absolute path pinned by the operator |
| Execution mode | `TRUSTED_LOCAL_PROVIDER` (not ISOLATED; not certified isolation) |
| Project | disposable Git repository, created for this run only |
| Objective | small Rust pricing/discount fix with deterministic tests |
| Allowed paths | `src/`, `tests/` |
| Allowed tools | `write_file`, `run_command` |
| Frozen checks | exactly the checks in the frozen ActionSpec; no additional commands |
| Max sessions | 1 |
| Max turns | 4 |
| Max egress requests per turn | 8 |
| Max input tokens | 200000 |
| Max output tokens | 4000 |
| Max cost | 5,000,000 micros (USD 5.00) using operator-supplied Codex rates |
| Wall-clock deadline | 900 seconds |
| Retries | none without a new authorization |
| Model fallback | forbidden |
| Cancellation | operator cancellation must fence turns and preserve reservations |
| Cleanup | confirmed sandbox process absence and disposable session-directory removal |
| Credentials | Worker-held per-run egress credential only; no operator HOME/`~/.codex` |

The operator must provide the two `FACTORY_PROVIDER_CODEX_*_COST_MICROS_PER_MILLION`
rates and the Codex egress token. Factory must use the installed App Server
schema, declare only the two dynamic tools, replace the upstream tool list with
that allowlist, reject all other server/item surfaces, bind every actual
`/v1/responses` body to Core, and refuse a candidate when usage or cleanup is
not confirmed.

## Acceptance evidence required

- Exact candidate commit, parent and tree.
- Genuine GPT-6 Luna provider/model identity and App Server version.
- Core provider-session and per-request reservation records.
- Token usage, configured-rate cost, cleanup receipt and process absence.
- Independent verifier PASS and semantic Reviewer evidence; deterministic Gate outcome.
- Actual source diff and frozen-check output.
- No retries outside the envelope and no model fallback.

## Explicitly not requested

- Unlimited or recurring paid turns.
- Codex self-development against the authoritative Factory repository.
- ISOLATED/SEC-01 certification, deployment, Windows, human review, or final merge.
