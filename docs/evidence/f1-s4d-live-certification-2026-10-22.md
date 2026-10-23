# F1-S4-D authorized live certification results

**Date:** 2026-10-22/23 UTC. **Product code candidate for the completed
runner and CAP-014 collection:** `9e12dcf0871105652160e328980bdec53d4fabd5`.
Attempt 1 ran on the preceding code `e94ab07`; attempt 2 and CAP-014 ran on
`9e12dcf`. **Route:**
`opencode/opencode-go/deepseek-v4.1-flash/build`. No provider fallback.
No merge or deployment occurred.

## Budget accounting

| Envelope | Used | Ceiling | Result |
| --- | --- | --- | --- |
| S4-D graph E2E | 2 provider sessions | 4 | **INCOMPLETE** |
| CAP-014 semantic | 11 assessment sessions | 11 | **INCOMPLETE** |
| Combined | 13 | 15 | Not certified |

The E2E envelope has 2 sessions remaining, below the 3 sessions required for a
complete assessment + two-unit journey. Paid E2E dispatch stopped and the
budget was not expanded. CAP-014 exhausted its separate 11-session envelope.

## S4-D graph E2E attempt 1

- Factory head: `4640e30d5ca58077c6a8c08a77b41996f6423d23`.
- Session: `e3224b47-cc31-489c-a80e-9fdd1a799a0b`.
- The model proposed an invalid per-unit check kind (`syntax`); Core blocked
  the proposal before any WorkUnit dispatch.
- The session was stopped and is archived. No unit or integration session was
  started. The original evidence JSON was written by the pre-fix runner and
  therefore records a historical `unboundSessions`/cleanup-failure
  classification; independent `paseo inspect` confirms the session is closed
  and archived, and `orphanedSessions` is empty. The failure work root was
  intentionally retained under `/tmp` for diagnosis.
  Evidence: [f1-s4d-graph-live-4640e30d5ca.json](f1-s4d-graph-live-4640e30d5ca.json).

## S4-D graph E2E attempt 2

- Factory head: `278f6ca6abf110c2f921d60a1b34c107cd8cadad`.
- Session: `a90e1a0a-3a82-44a0-895f-97656132fbcb`.
- The assessment provider session was reconciled with the exact requested
  route, but the Paseo run failed (`observedStatus: other`). Core blocked the
  request before plan freeze or any WorkUnit dispatch.
- The session was stopped and is archived. The failure work root was
  intentionally retained under `/tmp` for diagnosis.
  Evidence: [f1-s4d-graph-live-278f6ca6abf.json](f1-s4d-graph-live-278f6ca6abf.json).

No real multi-unit implementation, integration, browser/API acceptance,
independent Verifier/Reviewer result or Quality Gate acceptance was produced
by either E2E attempt. The deterministic quota/dry-run evidence remains
separate and is not a substitute.

## CAP-014 semantic collection

- Factory head: `310add3634a02da9559e8c1448d2ef0e4cbdd690` (code
  `9e12dcf`).
- Exactly 11 authorized assessment sessions were attempted, one per frozen
  fixture.
- **6 captured:** S4D-02, S4D-03, S4D-06, S4D-07, S4D-08, S4D-11.
- **5 failed:** S4D-01 and S4D-04 (`runCause: failed`); S4D-05, S4D-09 and
  S4D-10 (`runCause: timeout`).
- Captured sessions: `fe001f4d-b38e-4c5d-a1b8-a6b4f16df366`,
  `358c43df-b42b-41ce-be1c-2f380d07badf`,
  `a4d189ed-1620-44d7-a26c-30f69ea07b6a`,
  `7d1dd6b1-695f-425a-bae6-541b6bdb9df8`,
  `6a1b5001-814b-44c9-a629-7ed14181702c`,
  `19f9d306-270b-49f9-a26e-14750b03db42`.
- Failed-run reconciled sessions: `d2209aed-ab39-4baa-a214-0f4b91d655fa`,
  `1c90e6d9-c8b6-411e-a2f9-5c6d64e1aaa1`,
  `7eac6e0a-5555-4cd2-b658-211335405378`,
  `f5a181d6-66fb-432a-8535-c5f40261d389`,
  `8c571e74-9dbe-4c19-9338-79b8fcc9427a`.
- All 11 sessions are closed and archived. Raw/assessment/observation outputs
  are preserved in
  [f1-s4d-semantic-live-310add3634a0/](f1-s4d-semantic-live-310add3634a0/).
- Because only 6 of 11 fixtures produced decodable outputs and the envelope is
  exhausted, no independent blinded scoring or model-versus-baseline result
  exists. CAP-014 remains **blocked**.

## Capability and certification status

- CAP-013/015/016/017: deterministic scope verified; live-agent verification
  remains blocked.
- CAP-014: deterministic and live verification remain blocked.
- CAP-043: in progress.
- S4-D full-product real certification is **NOT COMPLETE**.
- No UAT, deployment, SEC-01 isolation, S5 PR creation, autonomous merge or
  autonomous deployment is certified.

## Required Product Owner decision

To continue real certification, authorize a new bounded envelope (at least
three fresh E2E sessions for one complete journey, and a separate revised
CAP-014 envelope). The CAP-014 timeout failures also require a reviewed
assessment-timeout adjustment or smaller per-fixture evidence before retry.
No additional paid session is authorized by this evidence.
