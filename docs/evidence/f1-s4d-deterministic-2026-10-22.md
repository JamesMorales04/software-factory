# F1-S4-D deterministic candidate evidence

**Recorded:** 2026-10-22. **Original code candidate:**
`be592770b77ca81c491e5021913e1d23d5a228b7`. **Lint-repaired product code
candidate:** `602d2352f90f66abd0819de78aa48e905c3689b0`. The original evidence
is bound to `be592`; the resume section records the later lint-repair commits.
This report is a docs-only follow-up and is not included in the code SHAs.

## Candidate checks

The release owner reports these checks passing on the code candidate:

| Check | Result |
| --- | --- |
| `go test ./...` | PASS |
| `go vet ./...` | PASS |
| `go test -race ./...` | PASS |
| `gofmt` | PASS |
| Office `npm run typecheck`, `npm run lint`, `npm run build` | PASS |
| `scripts/check-architecture-consistency` | PASS |
| `scripts/e2e-s4a.sh` | PASS |
| `scripts/e2e-s4b.sh` | PASS |
| `S4C_VLOG_REPO=/nonexistent S4C_CALC_REPO=/nonexistent bash scripts/e2e-s4c.sh` | PASS; 51 checks |

S4-A browser artifacts are preserved in
[Office journey evidence](f1-s4d-s4a-office-browser.json) and
[run evidence](f1-s4d-s4a-run-browser.json). S4-B browser evidence is in
[f1-s4d-s4b-browser.json](f1-s4d-s4b-browser.json). The complete S4-C
deterministic result for this candidate is
[f1-s4d-s4c-regression-pass-be592770.json](f1-s4d-s4c-regression-pass-be592770.json).
These used deterministic fixtures and Paseo simulation; they are not paid or
real-model evidence.

An independent reviewer examined the clean `be592` candidate and reported no
P1 blocker. An independent verifier passed targeted Go/race/vet checks, 10
race-enabled grant reservation attempts, the API no-spec guard, Office
typecheck/lint/build, and the Chromium legacy-null fixture. The verifier did
not independently rerun S4-A, S4-B or S4-C end-to-end suites.

## Resume lint remediation (2026-10-22)

PR head `5907a25` failed the pinned `golangci-lint` v2.5.0 with 32 findings:
gofumpt 5, errcheck 1, ineffassign 1, staticcheck 24 and unused 1. Repair
commits on the same branch fixed every finding at source without changing the
pinned tool, lint configuration, thresholds or tests:

- gofumpt formatting for the affected API and store files;
- the temporary assessment workspace removal error is now handled;
- an ineffectual assignment and an unused scanner were removed;
- unsupported WorkUnit dependencies are rejected directly instead of through
  an unreachable loop; test hash-stability and expiry-boundary assertions were
  corrected; De Morgan's law was applied; error strings were lowercased;
- review follow-up keeps structured assessment failure evidence parseable when
  workspace cleanup also fails and persists the cleanup status at the API
  boundary.

On lint-repaired product code candidate
`602d2352f90f66abd0819de78aa48e905c3689b0`:

| Check | Result |
| --- | --- |
| `go tool golangci-lint run ./...` | PASS; 0 issues |
| `gofmt` / gofumpt | PASS |
| `go vet ./...` | PASS |
| `go test ./...` | PASS |
| `go test -race ./...` | PASS |
| Office `npm run typecheck`, `npm run lint`, `npm run build` | PASS (Office code unchanged) |
| Mocked `office/e2e-browser/s4d-journey.mjs` | PASS, fully intercepted |
| `scripts/check-architecture-consistency` | PASS |
| `scripts/e2e-s4a.sh` | PASS |
| `scripts/e2e-s4b.sh` | PASS |
| `S4C_VLOG_REPO=/nonexistent S4C_CALC_REPO=/nonexistent bash scripts/e2e-s4c.sh` | PASS; 51/51 checks |

The candidate-bound S4-C result is
[f1-s4d-s4c-regression-pass-602d235.json](f1-s4d-s4c-regression-pass-602d235.json).
These reruns used deterministic fixtures and Paseo simulation only.

An independent reviewer found no P0/P1 on the lint repair. Its P2 findings
(cleanup error corrupting structured evidence, fixture prefix drift, evidence
size growth, dropped API cleanup status and missing compact-fallback coverage)
were fixed in follow-up commits; `602d235` adds the last test coverage. An
independent verifier independently reproduced 0 lint findings, full/race Go
tests, targeted S4-D tests, default-closed S4-D live preflight with zero Paseo
calls, and architecture consistency on `559a68d`; it did not rerun S4-A/B/C
end-to-end and its Office checks were not tested. The S4-D live-entrypoint
coverage gap it found is addressed by commit `5a14c24`, which adds S4-D cases
to `scripts/test-live-e2e-entrypoints.py` (24 cases, zero Paseo calls) and
changes no product code.

## Preserved S4-C Projects regression

The initial S4-C run at `3ad483631300f5041779fc039c8ba2c61b0665d1` failed C17 while
waiting for the Projects heading. The original report is preserved at
[f1-s4d-s4c-first-failure-3ad4836.json](f1-s4d-s4c-first-failure-3ad4836.json).
Diagnosis traced the crash to a legacy Default Project with
`browserEndpoints: null` and an unconditional Office `.join()`. The fix makes
the Office type nullable and renders that field safely; the final `be592` S4-C
suite passes.

A focused attempt after the UI fix reached Campaign start but could not find a
Worker supporting the browser capability because the ad hoc launch omitted
the browser environment variables. That incomplete attempt is preserved at
[f1-s4d-s4c-focused-incomplete-worker-env.json](f1-s4d-s4c-focused-incomplete-worker-env.json).
With the complete harness environment, focused C17 passed; the exact-candidate
full S4-C pass above supersedes it as regression evidence.

## Scope and limits

- CAP-013, CAP-015 and CAP-017 bounded deterministic contract behavior passed
  on this candidate.
- CAP-014 schema/context fixtures are partial evidence only. Its original
  deterministic acceptance also requires a scored model-versus-baseline
  comparison, so full CAP-014 deterministic verification is blocked.
- CAP-016's one-WorkUnit validation is partial evidence only. Its original
  acceptance requires the dependency-aware WorkGraph contract; that capability
  remains in progress and full deterministic verification is blocked.
- CAP-043's deterministic authorization/plan subset is verified; the broader
  autonomous product-development objective remains in progress.
- No real model result, real Paseo session, paid session, Product Owner UAT,
  deployment, or isolation certification is claimed. Live assessment remains
  blocked pending explicit authorization and the separately guarded runner.
- Independent review and scoped verifier checks are complete for clean
  `be592`; the resume review above covers the lint-repair commits. This does
  not establish independently rerun S4-A/B/C end-to-end suites or live
  acceptance. Earlier targeted review against `3ad4836` does not certify
  `be592`. Local S4-A/B/C reruns passed on `602d235`; the final head adds only
  the S4-D entrypoint guard and this documentation.
- TRUSTED_LOCAL is still broad host access; SEC-01 isolation remains blocked.
  Provider spend/quota is not established by the session ceiling. SQLite lock
  contention remains in the reliability register.
