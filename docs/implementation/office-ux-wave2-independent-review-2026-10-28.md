# Office UX Wave 2 — independent review record (2026-10-28)

**Code candidate:** `7063321` (branch
`campaign/office-ux-wave2-integration-20261028`; later commits are documentation
and one unreachable grant-lifetime cleanup).
**Reviewers:** Codex GPT-6.1-Sol (critical architecture) and Codex GPT-6-Luna
(focused claims/security), via the external agent runner, read-only checkouts.
**Not reviewers:** the campaign director or any implementation lane.

## Verdicts

- **Sol: BLOCK for campaign completion.** The only remaining implementation
  BLOCK is the absent shared cross-role budget ledger. All other findings were
  repaired and re-checked.
- **Luna: APPROVE WITH CONDITIONS.** Its Medium parity/overclaim condition was
  fixed; no remaining wording finding in the parity matrix or ledger.

Neither reviewer performed independent runtime acceptance, live inference,
GitHub write/merge, browser acceptance or a populated-database upgrade.

## Findings and disposition

| Severity | Finding | Disposition |
|---|---|---|
| High | PR merge and read projection did not require the Core Gate `ACCEPTED` | Fixed: merge rejects missing/non-`ACCEPTED`; projection exposes `gate_accepted` and includes it in `merge_ready` |
| High | Five-second operation grant cancelled valid six-second commands, leaving a finalize `409` | Fixed: execute/run grants cover the maximum bounded runtime with the 500 ms Core fence; write/apply/finalize stay five seconds |
| High | Frozen Lead route differing from Implementer route failed model-call authorization | Fixed: validates the immutable frozen Lead route; deterministic full journey proves distinct routes |
| High | No shared request/Campaign budget envelope across Lead, implementation and review | **OPEN / BLOCKING.** Documented; paid profile-bound execution claiming aggregate limits must stay disabled until implemented |
| Medium | Discovered Implementer model accepted but Worker rejected configured-model mismatch | Fixed: Worker profile returns the exact authorized assignment model |
| Medium | PR context rejected exact WorkUnit attempt specs | Fixed: exact attempt spec plus WorkGraph lineage validation |
| Medium | New profile head invalidated an existing frozen assessment | Fixed: exact frozen revision is loaded; regression test updated |
| Medium | Git top-level could escape the approved local-import root | Fixed: resolved top-level is revalidated; regression test added |
| Medium | Compose evidence misattributed the exit-1 child | Fixed: intake child exited 0; grant expiry/fence is the confirmed cause |
| Low | `integrate` mentioned in the long-grant path though unreachable | Corrected |
| Low | Revocation is not instantaneous during a short grant's validity | Documented boundary; not a new bypass. Do not claim instantaneous revocation |

## Verification used by the reviewers

- Sol inspected the candidate and repair diffs, fetched the exact Compose run
  logs, and confirmed `main` remains `d587252` remotely.
- Compose run `40763932432` passed at `7d0c67b`; run `40765699005` passed at
  `7063321` with request `10ebbe0f…`, Campaign `e3fae868…`, candidate
  `9ef6983e…` at `5fe84636…`, verifier `bcd30867…`, reviewer `a0493267…`,
  Gate `ACCEPTED` and restart persistence.
- Luna ran `cargo test -p factory-native --lib issue_intake_tests` (11 passed)
  and checked catalog parity (2,440 identical keys) and parity wording.
- The author separately ran the final host journeys at `7063321`: full intake
  request `7a140b74…`, Campaign `29c344d2…`, candidate `d59a31f0…` at
  `1d79f3e3…`, Gate `ACCEPTED`; legacy Office candidate `34449fbe…`, Gate
  `ACCEPTED`.

## Promotion state

`main` is untouched and GitHub reports **no branch protection** (HTTP 404).
This candidate is not accepted, certified, merged or deployed. Promotion
requires the shared-budget repair, owner authorization and the normal protected
review path.
