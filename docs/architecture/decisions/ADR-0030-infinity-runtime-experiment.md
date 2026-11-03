---
id: ADR-0030
status: Proposed — experiment only, not accepted for production
date: 2026-11-03
owner: Engineering Director (experiment)
supersedes: null
amends: null
---
# ADR-0030: Infinity Runtime experiment and migration recommendation

- **Status:** Proposed by the authorized Infinity-first experiment. This ADR
  does **not** authorize a production migration, a merge into `main`/Wave 3, or
  real paid inference.
- **Base:** `25e34783a3195dba3827ae2e2b730bc5b0c6eb9e` (exact reviewed code
  baseline), branch `experiment/infinity-runtime-20261102`.
- **Infinity pin:** `1abc6fcff1f186124f924b56aa92c3a3684c90c5`.
- **Evidence:** `infinity-experiment/evidence/`, executable tests in
  `infinity-experiment/tests/`, and measured comparison in
  `evidence/comparison-20261103/summary.md`.

## Context

Factory already owns a governed Rust execution architecture: Core
authorization/budgets/delegations, Supervisor/Worker dispatch, SQLx state,
nested-Podman execution and broker, independent verification/review, and a
deterministic Quality Gate. The experiment asked whether Infinity should
replace the custom agent-execution/orchestration layer, whether Infinity Code
should become the primary coding system, whether selected components should be
integrated, or whether Factory should retain its architecture.

The experiment was required to start from the reviewed code baseline, not the
documentation checkpoint, and to ignore sunk development cost.

## What was actually executed

A separate crate (`infinity-experiment`) depends on the pinned Infinity crates
`infinity-agent-core`, `infinity-provider-protocol`, `rap-protocol` and
`rap-client`. It implements:

- a Director plus two fresh-context implementers and one independent reviewer;
- SQLx/SQLite `ConversationStore` and `StateStore` implementations;
- durable effect intent/apply/complete ledger, callback inbox, attempt fencing
  and ambiguous-outcome settlement;
- process-exit restart recovery with marker verification and no effect replay;
- a RAP command server, callback bridge and Infinity resume path;
- deterministic subscription-route guards and fake providers;
- a process-tree measurement harness for prototype and Factory baseline.

## Options evaluated

| Option | Evidence | Verdict |
| --- | --- | --- |
| **A. Existing Factory Rust execution** | 84k LOC native tree; full governance, funding, candidate and quality path; deterministic E2E passes; container-native Compose path exists. | Retain as production authority. |
| **B. Infinity Agent Runtime inside Factory** | The prototype completes the deterministic journey, restart, cancellation, callback fencing and RAP tests. Durable stores, effect journal, funding guards and confinement remain Factory-owned code, not Infinity features. | Viable only as a bounded, adapter-based pilot. |
| **C. Infinity Code as primary coding foundation** | Infinity Code adds daemon/session/UI/sandbox/RAP, but its pinned daemon persistence rewrites whole JSON snapshots and ignores write errors; PR #178 crash-safe persistence is not in the pin. It has no Factory authorization, budget, delegation, Quality Gate, Office or funding boundary. | Reject as primary on current evidence. |
| **D. Retain current Factory architecture** | Existing architecture already covers the critical reliability/security/funding controls. The prototype did not demonstrate a reliability advantage that compensates for a new fast-moving upstream dependency. | Retain; no wholesale replacement. |

## Measured comparison

Debug-profile, same host, deterministic fixtures, 2026-11-03. Full data:
`infinity-experiment/evidence/comparison-20261103/`.

| Workload | Wall ms | Peak summed RSS KiB | Peak processes | Peak threads | Sampled CPU ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| Factory parallel workgraph E2E (2 units, full gate) | 753 | 113,332 | 7 | 61 | 390 |
| Infinity prototype, 5 runs | 350–408 | 47,424–52,528 | 3 | 10–11 | 290–370 |
| Factory Core restart E2E | 364 | 43,920 | 2 | 22 | 240 |
| Infinity prototype restart (start crash + resume) | 360 | 49,500 | 4 | 12 | 300 |

Prototype internal metrics: first child 36–46 ms; callback queue latency
0.33–1.0 ms; effect-to-callback 1.0–1.33 ms; 81–84 lifecycle/display events;
0 duplicate external effects; candidate
`f3037e35242b45c04ca46a0296852f0c305ba7ede3d07da28ece0696b30f3070` stable
across restart.

The prototype is smaller at runtime, but the Factory baseline also runs the
full governance/quality pipeline. This is **not** evidence that the prototype is
a replacement for that pipeline. The prototype’s durability, effect and funding
code is largely code the experiment had to write; Infinity itself did not
supply those guarantees at the pinned revision.

## Decision

1. **Do not replace Factory’s production execution/orchestration path now.**
   Retain Core ownership of authorization, budgets, delegations, state,
   candidate identity, Quality Gate, provider funding and delivery.
2. **Do not adopt Infinity Code as the primary coding system.**
3. **Approve only a bounded follow-up pilot (Option 3)** if a concrete need
   arises: integrate `infinity-agent-core`’s step/thread/spawn/report semantics,
   `infinity-provider-protocol` and RAP behind Factory-owned ports, with
   Factory-owned SQLx stores and effect journal. The pilot must not expose
   Infinity’s sandbox as Factory isolation.
4. **Keep Infinity’s daemon JSON persistence out of production.** Use
   Factory SQLx stores or an upstream revision that includes PR #178 plus
   independent restart/fencing tests.
5. **No live provider inference** until the Owner authorizes a specific,
   subscription-only test with the attestations in
   `subscription::OwnerAttestation`.

## Component recommendation

| Component | Recommendation | Reason |
| --- | --- | --- |
| Core API, authorization, budgets, delegations, candidate/Quality Gate | **Retain** | Factory-owned authority; Infinity has no equivalent. |
| SQLx state/migrations and durable effect/callback journal | **Retain / Factory-owned** | Pinned Infinity daemon persistence is not crash-safe; prototype needed 1.4k+ LOC for stores/journal. |
| Supervisor/Worker, nested Podman, runtime broker | **Retain** | Container-native and security boundary; Infinity sandbox is not equivalent or certified. |
| Infinity agent loop, fresh-context spawn/report, step API | **Pilot adoption** | Executable prototype shows the semantics work and are compact. |
| `infinity-provider-protocol` | **Pilot adoption** | Clean provider abstraction; OpenCode Go still needs a guarded adapter. |
| RAP async tool invocation | **Pilot adoption** | End-to-end callback test passed; fits long-running commands. |
| Infinity daemon/Codex app-server as provider | **Do not wrap as completion** | App-server/SDK are complete coding agents; preserve their approvals/context or integrate as an external agent. |
| Infinity Code CLI/TUI/daemon | **Do not adopt as primary** | Would duplicate/abandon Factory governance and Office. |
| Duplicate custom orchestration | **Remove only after parity** | No parity evidence for production authorization, budgets or recovery. |

## Upstream persistence/recovery findings

- `1abc6fc` predates `e9dad5c` (PR #178) “crash-safe, versioned, append-only
  thread persistence”; the pinned daemon writes whole `{thread}.json` snapshots
  with `std::fs::write` and ignores errors.
- `01e889e` “settle tool calls orphaned by a restart” is also not in the pin;
  the prototype adds explicit ambiguous-effect settlement.
- `c2aab9e` (PR #24, persist in-flight tool results on stop) and `a7b01f1`
  (recover sessions after daemon restart) are ancestors of the pin, but do not
  make whole-file JSON writes crash-safe.
- Upstream main moves quickly with breaking changes (`!` commits). A production
  dependency must pin, vendor, and continuously test the exact revision.

## Consequences and conditions to revisit

- The experiment is reproducible but not independently reviewed; GPT-6 Luna
  review is blocked pending a separate subscription-only inference authorization.
- A production pilot must add: a real OpenCode Go provider adapter behind the
  funding guard, a Factory-owned SQLx store integration in Worker/Supervisor,
  per-session process isolation, durable input inbox, subtree cancellation,
  signed/fenced callbacks, and end-to-end Office/Core evidence.
- Revisit Option 1/2 only if the pilot demonstrates a measurable reliability or
  engineering-effectiveness advantage after those controls exist.
