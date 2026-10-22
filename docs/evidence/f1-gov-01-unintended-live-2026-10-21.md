# GOV-01 verification deviation: unintended S3-B live lane

**Observed:** 2026-10-21 America/Bogota. **Candidate:** `4e92051f983d0f3b31d3f102b0e09c6d92965dc8` (the repository changes were documentation and governance tooling). **Authorization:** GOV-01 allowed zero new paid Paseo/Implementer sessions.

A regression runner invoked `bash scripts/e2e-s3b.sh` without `FACTORY_S3B_LIVE=0`. The script defaults to `LIVE=1` and entered its paid provider lane after deterministic cases s1–s6 passed. I interrupted the runner as soon as the live lane was noticed. This invocation was an authorization error and supplies no GOV-01 certification claim.

- The log recorded one completed `codex/gpt-6-luna` Paseo session: Run `ca8501a944648bb07e12eafbc86858b3`, workspace `wks_640a55e09357d5ce`, session `9285bc08-0f93-49c3-bbcc-f0a520c0421f`. The script reported resource cleanup for it. Cost was not measured.
- A second `opencode/opencode-go/muse-spark-1.3-contributor` Run `20cb71cbfc660a116953d90b3dc25a1f` was queued before interruption. A read-only SQLite inspection after stopping found Run state `running` and binding state `intended`, with empty workspace/session IDs. No completed second session was observed; whether any provider charge occurred is unknown. No Factory Core/Worker process from this test remained running when checked.
- Later S3-B regression was rerun with `FACTORY_S3B_LIVE=0` and passed the deterministic lane. No further paid tests are authorized by this deviation.

The script default is tracked as [REL-005](../quality/reliability-debt.md). The first live session is not tied to GOV-01's certification manifest, frozen GOV-01 ActionSpec, independent verifier or Quality Gate and must not be counted toward GOV-01 acceptance.

## Read-only reconciliation on 2026-10-21 (America/Bogota)

The persisted SQLite database at `/tmp/factory-s3b-81g1HH/factory.db` was opened read-only with `PRAGMA query_only=ON`. Run `20cb71cbfc660a116953d90b3dc25a1f` still reports `running` with one Attempt. Its Paseo binding remains `intended`, with no workspace or session ID; the final recorded event is `PaseoSessionIntended` at `2026-10-22T00:04:24.983Z`.

Read-only `paseo ls -g -a --json` and `paseo inspect` found external session `1df6d6b7-db9e-40b9-bce4-2dac6ac5f84a`, titled `factory-s3d:20cb71cbfc660a116953d90b3dc25a1f:g1`. Paseo reports it **closed and archived** at `2026-10-22T00:04:46.065Z`, with `LastUsage: null`. No active external resource for that Run was found. This later observation resolves the orphaned external-session concern; it does not establish whether a provider charge occurred. Factory's persisted Run and binding remain unreconciled.

No cleanup, state mutation, new session or retroactive authorization was performed. An operator still needs to inspect provider billing, decide how to reconcile the stale Factory Run through an authorized procedure, and acknowledge the campaign authorization deviation. This record is incident evidence, not GOV-01 certification.
