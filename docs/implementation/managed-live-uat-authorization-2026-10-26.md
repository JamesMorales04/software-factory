# Managed live UAT authorization record (2026-10-26)

**Status:** Approved and consumed. This record documents the Product Owner
envelopes for the managed live acceptance and controlled self-development
tests. It is not a recurring authorization; a new paid phase requires a new
explicit envelope.

## Primary external-project UAT

| Field | Approved value |
|---|---|
| Provider / model | `opencode-go` / `deepseek-v4.1-flash`, OpenCode `1.18.32`, mode `build` |
| Execution mode | `TRUSTED_LOCAL_PROVIDER` (not isolated) |
| Objective | bounded Rust discount repair in a disposable repository |
| Allowed paths | `src/`, `tests/` |
| Allowed tools | `write_file`, `run_command` |
| Lead / Reviewer | real MODEL Lead and real MODEL semantic Reviewer on the same Core route |
| Requests | at most 18 upstream/model requests |
| Provider sessions | at most 2, one repair only if deterministically justified |
| Tokens | at most 250,000 total |
| Cost | at most USD 2.00 equivalent |
| Deadline | 120 minutes from approval |
| Retries / fallback | no silent retries; no fallback model |
| Credentials | operator's local OpenCode Go credential copied to private `0600` files; never printed or committed; deleted after |
| Cleanup | `factory stop`, disposable container/volume and provider-session roots removed |

After a genuine candidate was produced but its verifier receipt became an
immutable `FAIL` because the harness mistakenly used the local verifier
backend, the Product Owner separately authorized **one replacement provider
session**: one session, four turns, 60,000 input / 12,000 output tokens, USD
0.50, 30 minutes, same route, paths and tools. The deterministic rootless-Podman
verifier was proven on a fixture before the replacement.

## Controlled self-development test

| Field | Approved value |
|---|---|
| Project | disposable clone of this repository at `ea2d92724d57d4f96838d84216f39db04b7c5511` |
| Objective | add focused unit tests for `factory_cli::text::one_line` |
| Allowed path | `factory-cli/src/text.rs` only |
| Provider / model | `opencode-go` / `deepseek-v4.1-flash`, OpenCode `1.18.32` |
| Sessions / turns | 1 session, at most 4 turns |
| Tokens | 60,000 input / 12,000 output |
| Cost | at most USD 0.75 equivalent |
| Deadline | 30 minutes |
| Tools | `write_file`, `run_command` |
| Verification | independent rootless-Podman verifier with an offline Rust image; semantic Reviewer; deterministic gate |
| Merge | forbidden; candidate remains in the disposable clone |

Both envelopes were proposed before the paid requests and were executed
without a fallback model or an automatic retry. The measured spend and
outcomes are recorded in
[managed-live-uat-2026-10-26.md](../evidence/managed-live-uat-2026-10-26.md)
and
[managed-self-development-2026-10-26.md](../evidence/managed-self-development-2026-10-26.md).
