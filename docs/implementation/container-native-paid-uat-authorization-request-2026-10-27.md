# Container-native live UAT authorization request (2026-10-27)

**Status:** Proposed. **Not authorized. No paid inference has been run.** This
request is the separate Product Owner envelope required before the hardened
PR #28 candidate can receive a live container-provider acceptance. It does not
authorize merge, deployment, ISOLATED execution or any security certification.

## Candidate and purpose

| Field | Proposed value |
|---|---|
| Candidate | Exact PR #28 hardening commit (recorded after the final evidence commit) |
| Deployment | Rootless Compose from the candidate, fresh disposable project and volumes |
| Purpose | Exercise the live OpenCode-in-container route end to end and verify the candidate-bound gate and cleanup |
| Execution mode | `TRUSTED_LOCAL_PROVIDER` (not isolated, not SEC-01 certified) |
| Provider / model | `opencode-go` / `deepseek-v4.1-flash`, OpenCode 1.18.32, mode `build` |
| Project | Disposable Git repository created by the UAT harness; no host project or secret mounted |
| Allowed paths | `src/`, `tests/` |
| Frozen checks | At least one executable functional check and one negative check |
| Allowed tools | `write_file`, `run_command` (frozen checks only) |
| Lead / Reviewer | Core model gateway configured through Office; semantic reviewer remains optional and disabled unless included below |

## Budget and stop conditions

| Field | Proposed ceiling |
|---|---|
| Provider sessions | At most 2: one primary and at most one corrective repair only after a deterministic verifier/reviewer failure |
| Provider turns / requests | At most 8 turns and 16 upstream requests total |
| Tokens | At most 120,000 input and 12,000 output total |
| Cost | At most USD 2.00 equivalent at the configured rates |
| Deadline | 90 minutes from the start of the authorized run |
| Retries / fallback | No silent retry and no fallback provider/model; an infrastructure failure must be repaired before any new paid session |
| Stop | Stop on `ACCEPTED`, exhausted budget, expiry, unresolved session, or any unconfirmed cleanup |

## Credentials and cleanup

- The operator enters the OpenCode Go API key directly into Office Settings.
  It must not be printed, committed, placed in browser storage, or copied into
  UAT logs.
- The UAT uses its own Compose project name containing `uat`, a disposable port
  and fresh volumes; it must not touch the permanent `software-factory` stack.
- After the run, revoke the provider credential in Office, remove the
  disposable project and volumes, remove any UAT token files, and record the
  cleanup receipt.

## Evidence to record

- Exact candidate commit and Compose project name.
- Project, Action, ActionSpec, Campaign, Run and Attempt identities.
- Provider session ID, route, generation, observed request count and terminal
  state.
- Candidate commit/tree and independent verifier/reviewer receipts.
- Quality Gate outcome and reasons.
- Measured input/output tokens and configured-rate cost; unknown usage remains
  `UNKNOWN`.
- Cleanup receipts for provider and execution containers.
- Whether live OpenCode container inference actually ran; a fixture result does
  not satisfy this authorization.

## Explicit non-claims

- No ISOLATED or SEC-01 certification.
- No deployment, Windows, SELinux/AppArmor or production claim.
- No human security review by this UAT.
- The UAT does not itself authorize merge; merge still requires the Product
  Owner's separate decision and exact-head evidence.

## Authorization

The Product Owner must approve this envelope in writing, including any change
to the provider/model, budget, deadline or session ceiling. Until then the live
route remains `NOT_TESTED` and no paid request may be issued.
