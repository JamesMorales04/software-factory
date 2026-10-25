# Bounded live provider coding authorization request

**Status:** REQUEST ONLY — no paid provider call is authorized by this document.
The deterministic fake-provider path passes; a live coding turn requires the
Product Owner's explicit approval of this envelope.

## Requested envelope

| Field | Requested value |
|---|---|
| Provider / model / mode | `opencode-go` / `deepseek-v4.1-flash` / `build` |
| Provider server | OpenCode `1.18.32`, loopback HTTP, credential held only by the trusted Worker |
| Project | one disposable external Git fixture repository approved for this run |
| Action | one R0 Action with a frozen ActionSpec, exact allowed path `src`, functional and negative checks |
| Campaign / Run / Attempt | exactly one each |
| Requests / sessions / turns | 1 / 1 / 1 |
| Input / output / total token ceiling | 8,000 / 2,000 / 10,000 |
| Cost ceiling | 0.02 USD equivalent (2,000,000 micros) |
| Deadline | 120 seconds |
| Provider tools | none; `allowedTools=[]`; any tool part fails the Attempt |
| Allowed filesystem path | `src` only |
| Retries | none |
| Cleanup | session DELETE followed by GET 404; otherwise the session is unresolved and blocks replacement |
| Candidate path | Worker → durable intent → provider patch → Supervisor validated apply → CandidateRevision → sandboxed verifier → independent reviewer → deterministic gate |

## Preconditions

- The credential is renewed and the operator confirms the `opencode-go` route.
- The fake-provider E2E and exact-head CI remain green.
- The Worker holds the credential; the Supervisor/sandbox never receives it.
- The provider receives only the bounded objective/allowed-path prompt and an
  empty session directory in this tool-free slice. A live run therefore proves
  the execution and evidence path, not full repository-context coding.
- No retry, alternate model or silent provider substitution is permitted.

## Required evidence after a run

- exact Factory candidate SHA and frozen ActionSpec hash;
- provider session ID, turn/message ID, provider/model/mode and usage/cost;
- cleanup receipt and confirmed absence;
- exact CandidateRevision parent/tree/content identity;
- verifier and reviewer receipts bound to the candidate;
- deterministic gate outcome;
- truthful classification of the result, including failure or unresolved
  cleanup.

This request does not authorize tool mediation, source-context delivery,
ISOLATED execution, merge or deployment. Any expansion requires a new bounded
authorization.
