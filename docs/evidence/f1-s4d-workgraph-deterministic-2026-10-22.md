# F1-S4-D standing autonomy and WorkGraph deterministic evidence

**Recorded:** 2026-10-22. **Code candidate:** `62df72002cc12fdfbfab2e709c54b6b6ff5e3d7c`. This evidence is
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
- Bounded multi-WorkUnit planning: 1–8 units, stable IDs, up to eight
  predecessors per unit, DAG/cycle/scope/capability/path-conflict/budget
  validation, per-unit frozen ActionSpecs and per-unit checks, Core-generated
  integration only for multiple terminal units, and immutable plan identity.
- Dependency-aware execution: sequential units start from their prerequisite
  candidate; independent units start from the campaign base; ordinary
  multi-predecessor units and terminal integration consume dependency
  candidates through deterministic Git merges; conflict-free integration
  creates no provider session; the final integrated candidate alone receives
  Verifier, Reviewer and Quality Gate acceptance.
- Worker Git workspace chaining and conflict failure using real Git refs.
- Restart/idempotent dispatch, dependency failure containment, and a
  crash-consistency failure-injection test for atomic Campaign start + Lead
  projection/audit.
- A real multi-unit runner (`scripts/e2e_s4d_graph_live.py`) and Office UI
  journey, exercised end-to-end against a deterministic Paseo stub; live
  execution remains a separate authorized step.
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
| `TestWorkGraphParallelUnitCompletionDoesNotInvalidateSiblingCandidate` | PASS |
| `TestWorkGraphOrdinaryUnitFanInConsumesBothPrerequisites` | PASS |
| `TestStartLeadCampaignIsAtomicAcrossCrashBoundary` | PASS |
| `scripts/e2e_s4d_graph_live.py` dry run | PASS; ACCEPTED_AND_VERIFIED, 3 stub sessions, deterministic integration, restart/reload, cleanup |
| `TestWorkGraphDependencyFailureStopsDownstreamDispatch` | PASS |
| `TestWorkGraphRestartPreservesDependencyAndDispatchIdempotency` | PASS |
| `TestPrepareSpecWorkspaceSequentialAndIntegration` | PASS |
| `TestPrepareSpecWorkspaceRejectsIntegrationConflict` | PASS |
| Integrated candidate `candidate.InspectCommit` lineage | PASS; single parent on frozen base, both unit paths accepted |
| `TestSemanticEvaluationScoresAndComparesBlindedSheets` | PASS |
| `TestSemanticEvaluationFailsClosedOnMissingScoresAndThresholds` | PASS |
| `TestSemanticEvaluationCorpusIsFrozen` | PASS |
| `TestSemanticEvaluationRejectsUnknownFixtureIDs` | PASS |
| Office `npm run typecheck`, `npm run lint`, `npm run build` | PASS |
| Mocked `office/e2e-browser/s4d-journey.mjs` | PASS; 12/12 checks |
| `scripts/check-architecture-consistency` | PASS |
| `scripts/test-live-e2e-entrypoints.py` | PASS; 30 cases, zero Paseo calls |
| `scripts/e2e_s4d_graph_live.py` default preflight | PREFLIGHT_SAFE; zero provider calls |
| `scripts/e2e_s4d_semantic_live.py` default preflight | PREFLIGHT_SAFE; zero provider calls |

## Review findings repaired

Independent review found and the candidate now fixes: integration candidates
being rejected for multi-parent Git lineage; sibling unit candidate
supersession under parallel completion; assessment-session refund after the
model boundary; non-atomic autonomous-start audit recording; campaign attempt
cap inconsistent with the 1–8 unit bound; missing coordinator process cleanup
on revocation; CAP-014 silently accepting unknown fixture IDs; unbound
provider-session discovery and cleanup; semantic collector direct-invocation
authority gaps; corpus digest pinning; raw model-output capture; and exact
route-mode binding.

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
- The multi-unit live runner is implemented and dry-run verified with a
  deterministic stub; real provider execution is not yet recorded here. The
  Product Owner's numeric authorization and exact-SHA live execution remain
  separate evidence.
- TRUSTED_LOCAL remains broad host access and is not SEC-01 isolation.
- CAP-014 has no model score and must not be marked verified.
