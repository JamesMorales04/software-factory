# Bounded live provider coding authorization request

**Status:** REQUEST ONLY — no paid provider call is authorized by this document.
The deterministic multi-turn fake-provider path passes; this live multi-turn
coding envelope requires the Product Owner's explicit approval and is
**UNAPPROVED** until that approval is recorded.

## Requested envelope

| Field | Requested value |
|---|---|
| Provider / model / mode | `opencode-go` / `deepseek-v4.1-flash` / `build` |
| Provider server | OpenCode `1.18.32` in a bubblewrap-confined process with an empty HOME, private session directory and no external network; per-run bridge capability; long-lived credential held only by the trusted Worker |
| Project | one disposable external Git fixture repository approved for this run |
| Action | one R0 Action with a frozen ActionSpec, exact allowed path `src`, functional and negative checks |
| Campaign / Run / Attempt | exactly one each |
| Requests / sessions / turns | 1 / 1 / up to 4 |
| Input / output / total token ceiling | 8,000 / 2,000 / 10,000 |
| Cost ceiling | 0.02 USD equivalent (2,000,000 micros) |
| Deadline | 120 seconds |
| Provider tools | `write_file` and `run_command` only; every command must exactly match a frozen ActionSpec check; permission denials are recorded and returned to the provider |
| Allowed filesystem path | `src` only; bounded source context up to 16 files/64 KiB; credential-shaped names excluded |
| Retries | none |
| Cleanup | session DELETE followed by GET 404; otherwise the session is unresolved and blocks replacement. Sandboxed-session recovery after Worker crash is not yet implemented. |
| Candidate path | Worker → durable intent → provider tool turns with permission decisions → Supervisor mediated write/run/finalize → CandidateRevision → sandboxed verifier → independent reviewer → deterministic gate |

## Preconditions

- The credential is renewed and the operator confirms the `opencode-go` route.
- The fake-provider E2E and exact-head CI remain green.
- The Worker holds the long-lived credential; the Supervisor/sandbox never
  receives it. The provider process runs under bubblewrap and receives only a
  per-run bridge capability.
- Supervisor allocation uses a Core-signed Attempt grant; workspace operations
  use a per-Attempt capability.
- The provider receives bounded source context and may use up to four turns,
  requesting at most one mediated tool per turn. The Supervisor executes allowed
  writes/frozen-check commands in the sandbox; denied requests are recorded and
  returned to the provider. The provider never receives host workspace access or
  credentials. Live model egress through the sandbox is not implemented, so
  this envelope is not executable yet. It remains unapproved and must not be
  executed until the Product Owner records approval and sandboxed egress is
  implemented.
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

This request does not authorize ISOLATED execution, merge or deployment. It
explicitly requests trusted-local tool mediation and bounded source-context
delivery within the envelope above. Any expansion requires a new bounded
authorization.
