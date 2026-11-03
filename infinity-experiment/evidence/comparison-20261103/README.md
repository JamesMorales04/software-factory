# Measured comparison — 2026-11-03

Host: Linux, 16 logical CPUs, 32 GiB RAM, Rust 1.90.0, debug profile.
Harness: `infinity-experiment/scripts/measure.py`; samples every 5 ms and sums
live descendant RSS, threads and CPU ticks. CPU is sampled across PIDs,
including PIDs that exited during the run. `direct_child_rusage_*` is a
cross-check, not the primary metric.

## Commands

```sh
PROFILE=debug bash infinity-experiment/scripts/compare.sh /tmp/sf-comparison
```

Factory baseline commands were the already-built integration-test binaries:

```sh
target/debug/deps/e2e_fixture_dispatch-d70a187ff64776d3 \
  parallel_workgraph_reaches_accepted_on_integrated_candidate --nocapture

target/debug/deps/e2e_core_recovery-55bf35475ea727d3 \
  core_restart_preserves_durable_project_action_and_events --nocapture
```

## Results

| Workload | Wall ms | Peak summed RSS KiB | Peak processes | Peak threads | Sampled CPU ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Factory parallel workgraph E2E | 753 | 113,332 | 7 | 61 | 390 |
| Factory Core restart E2E | 364 | 43,920 | 2 | 22 | 240 |
| Factory cancellation/restart fencing E2E | 398 | 55,196 | 2 | 23 | 290 |
| Infinity prototype (5 runs) | 350–408 | 47,424–52,528 | 3 | 10–11 | 290–370 |
| Infinity prototype restart (start + resume) | 360 | 49,500 | 4 | 12 | 300 |
| Infinity prototype with Podman fixture execution | 1,636 | 301,368 | 7 | 67 | 1,420 |

Prototype internal metrics (5 runs):

| Run | First child ms | Callback queue ms | Effect→callback ms | Duplicate effects | Events |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 46 | 0.67 | 1.33 | 0 | 83 |
| 2 | 40 | 0.67 | 1.00 | 0 | 81 |
| 3 | 37 | 1.00 | 1.33 | 0 | 84 |
| 4 | 36 | 0.67 | 1.33 | 0 | 84 |
| 5 | 36 | 0.33 | 1.33 | 0 | 84 |

Candidate digest was stable at
`f3037e35242b45c04ca46a0296852f0c305ba7ede3d07da28ece0696b30f3070` across the
prototype runs and restart recovery. The restart evidence shows one APPLIED
effect before crash (review), three total external applications before and
after, three delivered callbacks after recovery, zero duplicate applications,
zero ambiguous outcomes.

## Dimension coverage

| Dimension | Prototype | Factory baseline | Status |
| --- | --- | --- | --- |
| Resident memory | 47–52 MiB local; 301 MiB with container runtime | 113 MiB parallel E2E; 44 MiB Core recovery | Measured, not equivalent workloads |
| CPU overhead | 290–370 ms sampled | 390 ms parallel; 240 ms recovery | Measured |
| Process / task count | 3 processes local; 4 agent threads (Director + 3) | 7 peak processes; 2 work units + Core/Supervisor/Worker/Verifier/Reviewer | Measured processes; baseline agent task count from test definition |
| Subagent startup latency | 36–46 ms to first child | not instrumented | Prototype measured; baseline `NOT_MEASURED` |
| Idle resource consumption | workload exits; not sampled while idle | Core runs during test; idle not sampled | `NOT_MEASURED` |
| Callback-to-resume latency | 0.33–1.0 ms queue; 1.0–1.33 ms effect→callback | not instrumented | Prototype measured |
| Restart recovery | PASS; one APPLIED effect recovered, 3/3 callbacks, 0 duplicate applications | Core restart E2E PASS | Both measured, different scopes |
| Duplicate external effects | 0 | not directly instrumented in fixture E2E | Prototype measured |
| Task completion correctness | PASS candidate `f3037e…` | ACCEPTED fixture candidate | Both PASS |
| Cancellation | PASS (no unsettled effect delivered) | PASS (`campaign_cancellation_fences_a_leased_attempt_through_restart`) | Both measured, different scope |
| Operational complexity | 5,037 src + 934 test lines; 1 runtime process; 4 pinned Infinity crates | 83,901 native src lines; Compose/7-process E2E | Static + process count |

## Interpretation limits

- The Factory test includes Core/Supervisor/Worker/Verifier/Reviewer and the
  governance/Quality Gate path. The prototype does not. This is a system-level
  comparison, not an agent-loop-only benchmark.
- The Factory fixture uses a deterministic fixture provider and a local test
  backend; the prototype uses a deterministic model and real Python file writes
  and tests. Workloads are similar in shape, not identical in work.
- RSS is summed resident set size; shared pages can be counted more than once.
  It is useful for relative comparison, not an absolute memory budget.
- Subagent startup latency for the Factory baseline was not instrumented;
  `NOT_MEASURED`. Idle daemon resource use was not measured; both observed
  workloads exit or are torn down after completion.
- The Podman row starts a container per fixture command; most of its RSS/threads
  belong to the container runtime/storage layers, not the Infinity process.
- Cancellation behavior was tested only in the prototype. Real engineering
  quality, subscription consumption and live provider latency are `NOT_TESTED`
  because no live-inference authorization exists.
- These are single-host debug-profile measurements, not independent acceptance.
