# F1-S4-D standing autonomy and WorkGraph deterministic evidence

**Recorded:** 2026-10-22. **Code candidate:** `PENDING_COMMIT`. This evidence is
bound to the exact code commit resolved below after the completion commit.
No paid or live model/Paseo session was started.

## Scope verified

- Reusable standing delegation with cumulative request, session and attempt
  accounting, atomic concurrency-safe reservation, and migration from the
  legacy one-request constraint.
- Autonomous Campaign start only for delegated R0/R1 risk classes, with a
  durable `LeadRequestAutoStarted` event and `startMode` projection.
- Immediate revocation containment: open requests blocked, pending/running
  Campaigns terminated, active implementer Runs cancelled through the existing
  fencing path, open tasks cancelled or preserved as terminal evidence.
- Bounded multi-WorkUnit planning: 1–8 units, stable IDs, explicit single
  predecessor dependencies, DAG/cycle/scope/capability/path-conflict/budget
  validation, per-unit frozen ActionSpecs, Core-generated integration node,
  and immutable plan identity.
- Dependency-aware execution: sequential units start from their prerequisite
  candidate; independent units start from the campaign base; fan-in integration
  merges terminal dependency candidates; the final integrated candidate alone
  receives Verifier, Reviewer and Quality Gate acceptance.
- Worker Git workspace chaining and conflict failure using real Git refs.
- Restart/idempotent dispatch and dependency failure containment.
- CAP-014 frozen machine corpus, blinded score-sheet validator, threshold
  comparison and non-provider `s4d-evaluate` command. No model outputs exist.

## Tests executed

| Check | Result |
| --- | --- |
| `go tool golangci-lint run ./...` | PASS; 0 issues |
| `go vet ./...` | PASS |
| `go test ./...` | PASS |
| `go test -race ./...` | PASS |
| `TestLeadPlanAcceptsSequentialParallelAndFanInGraphs` | PASS |
| `TestLeadPlanRejectsInvalidGraphShapesAndParallelOverlap` | PASS |
| `TestStandingDelegationRequiresExplicitAutonomousPolicy` | PASS |
| `TestReusableDelegationAccountsRequestsAndSessionsAcrossReservations` | PASS |
| `TestReusableDelegationConcurrentRequestsRespectCumulativeCeiling` | PASS |
| `TestReusableDelegationMigrationRemovesUniqueConstraint` | PASS |
| `TestLeadAutonomousStartBuildsMultiUnitGraph` | PASS |
| `TestLeadAutonomousRevocationContainsRunningCampaign` | PASS |
| `TestWorkGraphSequentialBaseChaining` | PASS |
| `TestWorkGraphParallelFanInIntegration` | PASS |
| `TestWorkGraphDependencyFailureStopsDownstreamDispatch` | PASS |
| `TestWorkGraphRestartPreservesDependencyAndDispatchIdempotency` | PASS |
| `TestPrepareSpecWorkspaceSequentialAndIntegration` | PASS |
| `TestPrepareSpecWorkspaceRejectsIntegrationConflict` | PASS |
| `TestSemanticEvaluationScoresAndComparesBlindedSheets` | PASS |
| `TestSemanticEvaluationFailsClosedOnMissingScoresAndThresholds` | PASS |
| `TestSemanticEvaluationCorpusIsFrozen` | PASS |
| Office `npm run typecheck`, `npm run lint`, `npm run build` | PASS |
| Mocked `office/e2e-browser/s4d-journey.mjs` | PASS; 12/12 checks |
| `scripts/check-architecture-consistency` | PASS |
| `scripts/test-live-e2e-entrypoints.py` | PASS; 27 cases, zero Paseo calls |
| `scripts/e2e_s4d_graph_live.py` default preflight | PREFLIGHT_SAFE; zero provider calls |

## Capability status

- CAP-013/015/017 deterministic scope: verified for the reusable/autonomous and
  multi-unit planning path.
- CAP-016: implementation implemented; deterministic verification verified for
  the bounded graph contract. Real model/Paseo execution remains blocked.
- CAP-014: harness prepared, but deterministic verification remains blocked
  until independently scored real model-versus-baseline outputs exist.
- CAP-043: remains in progress; a deterministic planning/execution subset is
  not the complete autonomous product-development objective.

## Limits

- No real model output, provider session, monetary/token measurement, Product
  Owner UAT, deployment or isolation certification is claimed.
- The multi-unit live runner is preflight-only; a reviewed execution runner and
  explicit numeric authorization are still required.
- TRUSTED_LOCAL remains broad host access and is not SEC-01 isolation.
- CAP-014 has no model score and must not be marked verified.
