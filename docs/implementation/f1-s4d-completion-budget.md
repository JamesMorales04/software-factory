# F1-S4-D completion certification envelope

**Status:** prepared, not authorized, not executed. **Date:** 2026-10-22.
This document requests a numeric Product Owner decision; it is not an
authorization and it starts no provider session.

## Candidate and route

- Product code candidate: `9e12dcf0871105652160e328980bdec53d4fabd5`. Execution must pin the exact clean
  `git rev-parse HEAD` at that time; the runner must refuse a dirty worktree or
  a SHA mismatch. A later docs-only commit is behaviorally identical but must
  still be the exact pinned clean HEAD.
- Core/Worker/Verifier/Reviewer/Office: built from that candidate.
- Provider/model/mode: `opencode` / `opencode-go/deepseek-v4.1-flash` / `build` (the authorization envelope binds the full triple).
- No fallback provider or route substitution is allowed.
- Execution mode: TRUSTED_LOCAL. This is not SEC-01 isolation.

## Fixture

Disposable external Project `factory-fixture-s4d-graph` with two dependent
engineering units and browser-testable behavior:

1. backend unit: implement `src/greeting.js` with validation and unit checks;
2. frontend unit: wire the greeting into `src/app.js` with unit checks;
3. Core-generated deterministic integration: merge both candidate commits
   without a provider session; a conflict fails closed without paid recovery;
4. independent Verifier runs the global format/test/functional/browser
   acceptance on the integrated candidate; Reviewer and deterministic Quality
   Gate decide acceptance.

The fixture must fail if the agent only implements one unit, if dependency
order is ignored, or if a prebuilt candidate bypasses the WorkGraph.

## Numeric ceilings requested

| Item | Ceiling |
| --- | --- |
| Lead assessment sessions | 1 |
| Unit implementer sessions | 2 |
| Deterministic integration sessions | 0 provider sessions |
| Expected actual provider sessions | **3** |
| Authorized maximum Factory sessions | **4** |
| First attempt used | **1 assessment** (preserved failure) |
| Retry attempt used | **1 assessment** (preserved failure) |
| Total E2E envelope used | **2 of 4**; remaining 2 cannot fund a complete journey |
| Model implementer attempt budget | 2 (one per independent unit); deterministic integration consumes none |
| Repair cycles | 0 |
| Maximum parallel implementation tasks | 2 |
| Maximum wall clock | 1800 seconds |
| Runner wall-clock deadline | 2400 seconds |
| Concurrency | one Core, one Worker, one current Campaign |
| Retry policy | no automatic retry beyond the frozen attempt budget |
| Monetary/token ceiling | not observable in current Core; unknown, not claimed |

CAP-014 semantic evaluation is a **separate** envelope: 11 assessment sessions,
no implementer sessions, no more than 11 provider sessions, same route. It must
not be combined with this E2E envelope without a new numeric decision.

## Stop and cleanup conditions

Stop and preserve evidence if any of the following occurs:

- route/session count exceeds the ceiling or an unbound session is observed;
- a session cannot be inspected or its provider/model/mode differs;
- wall-clock deadline or attempt budget is exhausted;
- integration conflict, unresolved external session, or Gate non-acceptance;
- the Factory worktree is dirty or the candidate SHA changes.

On stop or completion, inspect, stop and archive every discovered session;
terminate Core/Worker/participants; remove the disposable fixture repository;
record external-session cleanup results and any unresolved session truthfully.
Never fabricate termination or acceptance.

## Independent oracles

- deterministic API/browser tests against the integrated candidate;
- independent Verifier evidence bound to the exact integrated candidate SHA;
- independent Reviewer verdict;
- deterministic Quality Gate decision;
- Office projection/event reload and exact-SHA evidence;
- a bounded negative path proving expired authority, invalid graph or
  exhausted budget prevents dispatch.

## Required authorization decision

The Product Owner must explicitly authorize:

1. the exact candidate SHA;
2. the exact route above;
3. the 4-session E2E ceiling and the 11-session CAP-014 ceiling (separately or
   together);
4. the fixture Project and authorized paths/capabilities;
5. the stop/cleanup conditions above.

Until that decision is recorded, `scripts/e2e_s4d_graph_live.py` remains
default-closed and cannot execute live work. No authorization is inferred from
a successful deterministic suite.
