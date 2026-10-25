# F1-S4-A acceptance matrix

**Status:** Implemented and exercised by deterministic and real paths; final
candidate-bound results are recorded in the campaign PR. TRUSTED_LOCAL only;
never isolated or SEC-01 certified.

The executable oracle is [`scripts/e2e-s4a.sh`](../../scripts/e2e-s4a.sh)
(`scripts/e2e_s4a_oracle.py`), the live single-session lane is
[`scripts/e2e-s4a-live.sh`](../../scripts/e2e-s4a-live.sh), and the Office
browser journey is [`office/e2e-browser/s4a-journey.mjs`](../../office/e2e-browser/s4a-journey.mjs).
The deterministic lane uses a real fixture git repository, a real Core/Worker,
real verifier/reviewer binaries and real Chromium; only the Paseo CLI is
simulated.

| ID | Requirement | Oracle | Status |
| --- | --- | --- | --- |
| A1 | Real Implementer produces a nonempty candidate with frozen source identity | One real Paseo session through Factory → Worker → Paseo; Core revalidates commit/tree/content/parent/scope | PASS (live 2026-10-20, see [evidence](../evidence/f1-s4a-live-2026-10-20.json)) |
| A2 | Independent verifier checks the exact candidate on clean resources | `factory-verifier` + clean read-only clone + frozen checks | PASS (deterministic) |
| A3 | Read-only reviewer returns a candidate-bound verdict | `factory-reviewer` + dedicated reviewer credential | PASS (deterministic) |
| A4 | Core gate accepts exactly the verified candidate | `quality.Evaluate` + durable decision; Office projection | PASS (deterministic) |
| A5 | Failing mandatory check never accepted, even with reviewer APPROVE | `failing` stub mode → functional FAIL → `REPAIR_REQUIRED` | PASS |
| A6 | Different SHA/spec/policy evidence rejected | Forged verifier submissions | PASS (403) |
| A7 | Forged/self/unauthorized verifier or reviewer evidence cannot accept | Worker/operator tokens rejected; verifier actor = implementer rejected | PASS (401/403) |
| A8 | Repaired candidate gets a new identity; old evidence invalidated | New Run → new candidate; prior candidate superseded; fresh verifier/reviewer required | PASS |
| A9 | Restart/replay yields exactly one decision, no fabricated events | Core restart + evidence replay + gate replay + event count | PASS |
| A10 | Unauthorized repository/scope/delivery rejected | Bad repository 400; out-of-scope candidate fails closed; delivery route 404 | PASS |
| A11 | Real Chromium shows candidate/verifier/reviewer/gate, including rejection | `s4a-journey.mjs` accepted + `REPAIR_REQUIRED` runs, reload and evidence | PASS |
| A12 | S1–S3-D invariants retained | Existing `go test -race ./...` and S1/S2/S3 workflows unchanged | Run in CI; see workflow results |
| A13 | Agent can launch/operate real Chromium through integrated tooling | Pinned `@playwright/cli` journey (`scripts/agent-browser-office.mjs`) | PASS |
| A14 | Agent navigates Office, interacts with a control and verifies authoritative state | UI Action creation + Core API check | PASS |
| A15 | Independent verifier repeats protected browser journey on exact candidate | Frozen `browser` check in verifier clean checkout, real Chromium | PASS |
| A16 | Negative browser acceptance detected and gate refuses acceptance | `browser-failing` stub mode → browser FAIL → `REPAIR_REQUIRED` | PASS |
| A17 | Isolation/provenance for browser evidence | Separate verifier credential, fresh clone, temp HOME/profile, no Factory token in browser env/evidence | PASS |
| A18 | Existing browser/SSE/recovery regression compatibility | S2 browser, S3-C, S3-D workflows plus s4a CI | CI |

## Recorded live Implementer evidence (2026-10-20)

One bounded real session on `opencode/opencode-go/deepseek-v4.1-flash` mode
`build` produced candidate
`f120900cb651e574925c78f4a26a093b` at commit
`7d6d894bf0f561afc995e46018a94a2e84ca603b` (changed `add.js`, `app.js`).
The independent verifier passed format/test/functional/negative and the
protected Chromium `browser` check; the reviewer returned `APPROVE`; the gate
returned `ACCEPTED`. Session `e5349124-64d7-43c6-aa96-ba878ebf9ef9` was archived
and its workspace was absent from the active listing. No retry was attempted.
See the sanitized [live evidence](../evidence/f1-s4a-live-2026-10-20.json).
This is TRUSTED_LOCAL evidence, not isolation certification.

## Commands

```sh
# deterministic full S4-A lane (stub Paseo, real Chromium)
FACTORY_S4A_STRICT=1 bash scripts/e2e-s4a.sh

# one bounded real Paseo session (paid; run only when authorized)
FACTORY_S4A_LIVE=1 bash scripts/e2e-s4a-live.sh

# core/office gates
(cd core && gofmt -l . && go vet ./... && go tool golangci-lint run ./... && go test -race ./...)
(cd office && npm ci && npm run typecheck && npm run lint && npm run build)
```

## Status semantics

`PASS` above refers to the deterministic fixture lane unless the row says
otherwise. The live Implementer lane is recorded separately in the PR with the
exact provider/model, session id, run id and candidate SHA. A completed Paseo
session alone is not A4 acceptance. Chromium/tooling absence is
`NOT_TESTED`/`BLOCKED`; it never becomes PASS.
