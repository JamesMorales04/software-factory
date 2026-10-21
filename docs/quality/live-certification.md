# Live certification policy for Factory engineering campaigns

**Status:** Adopted 2026-10-21 UTC (F1-S4-C-01). Applies to campaigns that change
Core orchestration, Worker/Paseo execution, capability provisioning, agent
workflows, quality integration or external-project execution.

## Evidence levels

### DETERMINISTIC_VERIFIED

Real functional, negative, regression, security and concurrency oracles pass
against the exact candidate. Real Core, Worker, verifier, reviewer, Git and
Chromium processes may be used; a deterministic Paseo/model simulation is
allowed only for the agent-decision step. The claim must name the exact
implementation SHA and the executable oracle.

### REAL_PASEO_E2E_VERIFIED

At least one bounded, genuine Factory-managed Paseo/model Run has exercised the
relevant affected execution capability. Record:

- exact Factory implementation SHA;
- provider, model and mode;
- Factory Project, Action, ActionSpec and Campaign IDs;
- Paseo workspace/session IDs and Run/Attempt/gate generation;
- candidate SHA and independent verifier/reviewer/gate evidence;
- cleanup status and any unresolved external lifecycle state;
- whether the live test was invalidated by later runtime changes.

A failed real session is a valid result. It is never replaced with a simulator
or hidden by a retry. Bounded automatic recovery under an approved budget is
not a hidden retry: each Run, failure class, recovery authorization and new
candidate is recorded. Additional paid sessions beyond the approved budget
require explicit Product Owner approval.

### PRODUCT_OWNER_UAT_VERIFIED

The Product Owner personally exercised the intended workflow and accepted the
result. Use the instructions in the campaign handoff; do not silently execute
the owner's manual acceptance on their behalf.

## Rules

- Deterministic green is necessary but not sufficient for a complete
  engineering acceptance claim on the affected execution paths.
- Do not require repeated paid sessions for every intermediate deterministic
  repair. The final live evidence must apply to the final runtime being
  proposed; documentation-only changes do not invalidate it.
- Missing provider availability, quota or authorization is recorded as
  `LIVE_NOT_TESTED` or `LIVE_BLOCKED`. The campaign may be reported
  `MERGE-READY` only with an explicit, recorded Product Owner exception.
- Simulated records and real records remain distinguishable in evidence.
- This policy does not certify SEC-01 isolation, authorize unsafe sandbox
  modes, or grant delivery/merge authority.

## F1-S4-C-03 recovery certification budget

- Vlog: up to 4 additional Implementer sessions, max 3 corrective cycles.
- Calculator: up to 4 additional Implementer sessions, max 3 corrective cycles.
- One active paid Implementer per Project; no model fallback.
- Stop on independent acceptance; do not consume unused sessions.
- Infrastructure failures must be repaired before another paid session.
- Unknown provider cost/quota remains unknown.

## Browser capability vs product acceptance (proposed)

- **Product browser acceptance:** the Independent Verifier executes the
  protected browser oracle against the exact candidate. This is UI quality evidence.
- **Agent browser-capability certification:** the Factory-managed Implementer
  records successful model-mediated `open` + observation/interaction commands
  in the authorized Paseo session. This is capability-use evidence.

F1-S4-C/C-04 requires both. A future ADR may let certain projects require only
product acceptance, but cannot retroactively alter the frozen Vlog contract or
historical gate decisions.

## Current F1-S4-C disposition (2026-10-21 UTC)

- `DETERMINISTIC_VERIFIED`: the S4-C/C-03 lane passed with two external
  Projects, real Chromium, deterministic Paseo simulation and forced R1–R8
  recovery scenarios. Independent clean-checkout verification and GPT-6 Luna
  read-only review were performed.
- Historical C-05 Vlog result: **not achieved for that implementation.** Vlog
  used all 4 C-03 sessions; the final session produced a candidate and
  independent browser PASS but recorded no Playwright invocation, so the gate
  returned `REPAIR_REQUIRED`. Calculator accepted on the non-browser path at
  `5fa3c1c`; the final `cfa250e` delta is browser-only.
- **C-07 final live result: `REAL_PASEO_E2E_VERIFIED` for the bounded Vlog
  path on tested Factory head `9567bc5` / runtime `14133cb7`.** One paid
  `opencode/opencode-go/deepseek-v4.1-flash` build session produced candidate
  `e49f62d`; the Implementer ran model-mediated `open,snapshot,click,snapshot,
  click,snapshot` on the initial turn against the Worker-managed server.
  Independent verifier PASS, reviewer `APPROVE`, gate `ACCEPTED`; session,
  workspace and server cleanup confirmed. One of two authorized sessions was
  conserved; no repair cycle was used.
- `REAL_PASEO_E2E_VERIFIED` is candidate-bound to that tested head and does not
  prove later runtime changes, Product Owner UAT, isolation or delivery.
- C-04 same-session completion remains deterministic-verified; the live C-07
  run did not need the continuation because initial-turn evidence was complete.
- `PRODUCT_OWNER_UAT_VERIFIED`: not claimed; PR #12 remains unmerged.
- Further live sessions require explicit Product Owner authorization. C-07's
  unused second session is not authority for a speculative retry.
