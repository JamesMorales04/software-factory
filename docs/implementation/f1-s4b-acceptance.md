# F1-S4-B acceptance matrix

**Status:** Implemented and exercised by the deterministic native-orchestration
lane (2026-10-20; see [sanitized evidence](../evidence/f1-s4b-deterministic-2026-10-20.json));
final candidate-bound results are recorded in the campaign PR. The
bounded live orchestration lane is recorded separately and remains
`BLOCKED`/`NOT_RUN` unless an authorized provider session is available.
TRUSTED_LOCAL only; never isolated or SEC-01 certified.

The executable oracle is [`scripts/e2e-s4b.sh`](../../scripts/e2e-s4b.sh)
([`scripts/e2e_s4b_oracle.py`](../../scripts/e2e_s4b_oracle.py)); the Office
journey is [`office/e2e-browser/s4b-journey.mjs`](../../office/e2e-browser/s4b-journey.mjs).
The bounded live lane is [`scripts/e2e-s4b-live.sh`](../../scripts/e2e-s4b-live.sh)
(`FACTORY_S4B_LIVE=1`, one paid session, maxAttempts=1, maxRepairs=0).
The deterministic lane uses a real fixture git repository, a real Core (two
concurrent scheduler instances for B10), a real Worker, real verifier/reviewer
binaries spawned by Core, real Chromium, and a deterministic Paseo CLI
simulator. Only the Paseo CLI is simulated.

| ID | Requirement | Oracle | Status |
| --- | --- | --- | --- |
| B1 | Authorized ActionSpec creates a durable Campaign with immutable validated plan | `POST /campaigns`, plan hash/dependencies, duplicate replay, budget rejection | PASS (deterministic) |
| B2 | One authorized start causes native Implementer dispatch and progression | Campaign started once; no per-role commands; task graph advances | PASS (deterministic) |
| B3 | Verification cannot run before frozen candidate and succeeded Implementer Run | Early task inspection + candidate-bound promote | PASS (deterministic) |
| B4 | Factory schedules the existing verifier with scoped authority, exact-candidate evidence | Core-spawned `factory-verifier`, participant identity, clean clone, browser check | PASS (deterministic) |
| B5 | Factory schedules the separate Reviewer and records a candidate-bound verdict | Core-spawned `factory-reviewer`; APPROVE bound to exact SHA | PASS (deterministic) |
| B6 | Mandatory evidence yields one Core-authoritative ACCEPTED decision | Task graph completion + exactly one `CampaignFinished`/gate decision | PASS (deterministic) |
| B7 | Deliberate failure yields `REPAIR_REQUIRED`, bounded repair, new identity/revalidation | Stub sequence failing → good; two candidates, lineage, supersession | PASS (deterministic) |
| B8 | `BLOCKED`/`HUMAN_REQUIRED` prevent progress, retries and delivery | Protected-diff campaign terminates `human_required`; no repair task advances; Go tests cover blocked terminal | PASS (deterministic + Go negative) |
| B9 | Crash recovery preserves graph, fences and resumes/blocks safely | SIGKILL Core during Implementer; restart; S2 expiry/S3-D recovery; one candidate/decision | PASS (deterministic) |
| B10 | Concurrent schedulers, delayed reports, ineligible tasks, lease expiry do not duplicate/starve | Two Core instances on one DB; exactly one dispatch per task; Go test with 300 ineligible tasks | PASS (deterministic + Go) |
| B11 | Chromium starts a Campaign in Office, observes real tasks and survives reload | `s4b-journey.mjs` + Core API cross-check | PASS (deterministic, real Chromium) |
| B12 | Forged role results, mismatched identities, unauthorized transitions, budget escalation rejected | Scoped-token, forged-evidence, task-route and budget negatives | PASS (deterministic) |
| B13 | Existing S1–S4-A suites preserve contracts | `go test -race ./...` plus s1/s2/s3a/s3b/s3c/s3d/s4a CI workflows | CI (regression) |
| B14 | At least one bounded real Factory-managed workflow with actual Paseo Implementer | `e2e-s4b-live.sh`: one real `opencode/opencode-go/deepseek-v4.1-flash` mode `build` session; Core then scheduled verifier/reviewer/gate | PASS (live 2026-10-20; candidate `db2f3399c51a40e414c605b933d2fbb54e0487c5`, session `3e038bcc-8111-494c-bfe3-88ce13909599`) |

## Commands

```sh
# deterministic native-orchestration lane (stub Paseo, real Chromium)
FACTORY_S4B_STRICT=1 bash scripts/e2e-s4b.sh

# one bounded real Paseo session, Factory-managed through the campaign graph
FACTORY_S4B_LIVE=1 bash scripts/e2e-s4b-live.sh

# core/office gates
(cd core && gofmt -l . && go vet ./... && go tool golangci-lint run ./... && go test -race ./...)
(cd office && npm ci && npm run typecheck && npm run lint && npm run build)
```

## Status semantics

`PASS` refers to the deterministic fixture lane unless stated otherwise.
B14 is the separately recorded live session; simulator evidence alone is not
B14. A missing Chromium/tooling or an unavailable live provider budget remains
`NOT_TESTED`/`BLOCKED`, never PASS. B8 `BLOCKED` is
covered by deterministic Go tests; the live lane's `HUMAN_REQUIRED` case is not
rewritten as a blocked case.

## Evidence boundaries

- Implementer execution remains an S2 Run/Attempt with TRUSTED_LOCAL Paseo
  routing; no second attempt lifecycle exists.
- The verifier/reviewer are separate processes with dedicated credentials and
  candidate-bound evidence; process exit alone never completes a task.
- The Quality Gate remains deterministic and is the only acceptance authority.
- Campaign state, task claims, repair budget and terminal reasons are durable
  Core state; Office only renders projections and event replay.
- No GitHub delivery, S5/S6, remote Worker or SEC-01 isolation claim is made.
