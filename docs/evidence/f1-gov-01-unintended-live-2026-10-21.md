# GOV-01 verification deviation: unintended S3-B live lane

**Observed:** 2026-10-21 America/Bogota. **Candidate:** `4e92051f983d0f3b31d3f102b0e09c6d92965dc8` (the repository changes were documentation and governance tooling). **Authorization:** GOV-01 allowed zero new paid Paseo/Implementer sessions.

A regression runner invoked `bash scripts/e2e-s3b.sh` without `FACTORY_S3B_LIVE=0`. The script defaults to `LIVE=1` and entered its paid provider lane after deterministic cases s1–s6 passed. I interrupted the runner as soon as the live lane was noticed. This invocation was an authorization error and supplies no GOV-01 certification claim.

- The log recorded one completed `codex/gpt-6-luna` Paseo session: Run `ca8501a944648bb07e12eafbc86858b3`, workspace `wks_640a55e09357d5ce`, session `9285bc08-0f93-49c3-bbcc-f0a520c0421f`. The script reported resource cleanup for it. Cost was not measured.
- A second `opencode/opencode-go/muse-spark-1.3-contributor` Run `20cb71cbfc660a116953d90b3dc25a1f` was queued before interruption. A read-only SQLite inspection after stopping found Run state `running` and binding state `intended`, with empty workspace/session IDs. No completed second session was observed; whether any provider charge occurred is unknown. No Factory Core/Worker process from this test remained running when checked.
- Later S3-B regression was rerun with `FACTORY_S3B_LIVE=0` and passed the deterministic lane. No further paid tests are authorized by this deviation.

The script default is tracked as [REL-005](../quality/reliability-debt.md). The first live session is not tied to GOV-01's certification manifest, frozen GOV-01 ActionSpec, independent verifier or Quality Gate and must not be counted toward GOV-01 acceptance.
