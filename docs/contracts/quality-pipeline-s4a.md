# Contract: Frozen candidate quality pipeline (F1-S4-A)

**Status:** Implemented for F1-S4-A; final candidate acceptance is recorded in
the PR. **Version:** `2` (S4-A adds the quality pipeline to the S1–S3-D
contracts). Experimental pre-release contract; breaking changes update all
in-repo callers, fixtures, docs and tests atomically
([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

S4-A does not replace the S1 Action, S2 Run/Attempt/Worker or S3 Paseo session
lifecycles. It binds them: an Implementer Run can now reference a frozen
`ActionSpec`, and completion requires an independently validated
`CandidateRevision`.

## 1. Identities

| Entity | Identity | Notes |
| --- | --- | --- |
| ActionSpec | 16 random bytes hex (32 chars) + `contentHash` SHA-256 | Immutable; content hash excludes ID/timestamps |
| CandidateRevision | 16 random bytes hex | One per Implementer Attempt; immutable content identity |
| Candidate commit | full git object id | Exact parent is the frozen base revision |
| Content digest | SHA-256 of `git ls-tree -r --full-tree <commit>` | Stable across clones |
| Verification evidence | 16 random bytes hex + `(candidateId, idempotencyKey)` | Append-only, verifier authority |
| Review evidence | 16 random bytes hex + `(candidateId, idempotencyKey)` | Append-only, reviewer authority |
| Gate decision | 16 random bytes hex | Exactly one final decision per candidate |
| Scope | repository-relative paths/globs | Exact files, directory prefixes, `*`, `dir/**` |

`ContentHash` covers project/action, version, policy revision, goal, repository,
base revision, allowed paths, checks, risk, profile, budget, execution mode and
pinned route. `OracleDigest` is the SHA-256 of the acceptance+negative checks and
binds verifier/reviewer evidence to the exact protected oracle.

## 2. ActionSpec schema

```json
{
  "id": "…", "projectId": "default", "actionId": "…", "version": 1,
  "contentHash": "sha256-hex", "policyRevision": "quality-baseline-s4a-v1",
  "goal": "…", "repository": "/absolute/authorized/repo.git",
  "baseRevision": "git-object-id",
  "allowedPaths": ["add.js", "app.js"],
  "acceptanceCriteria": [
    {"id":"format","kind":"format","description":"…","command":["node","--check","add.js"],"expectExit":0,"timeoutSecs":30},
    {"id":"test","kind":"test","description":"…","command":["node","--test","add.test.js"],"expectExit":0,"timeoutSecs":60},
    {"id":"functional","kind":"functional","description":"…","command":["node","-e","…"],"expectExit":0,"timeoutSecs":30},
    {"id":"browser","kind":"browser","description":"…","command":["node","browser-check.mjs"],"expectExit":0,"timeoutSecs":120}
  ],
  "negativeCases": [
    {"id":"negative","kind":"negative","description":"…","command":["node","-e","…"],"expectExit":0,"timeoutSecs":30}
  ],
  "riskClass": "R0", "qualityProfile": "node-browser",
  "budget": {"maxAttempts": 1, "timeoutSecs": 600},
  "executionMode": "trusted-local",
  "provider": "opencode", "model": "opencode-go/deepseek-v4.1-flash",
  "paseoMode": "build",
  "createdAt": "RFC3339Nano", "frozenAt": "RFC3339Nano"
}
```

Rules:

- `executionMode` is `trusted-local`; an `isolated` engineering mode does not
  exist. The pinned provider/model/mode must be an S3 allowlisted triple.
- Commands are argv vectors, not shell strings, execute under the acceptance
  allowlist, and are bounded (8 args, 1024 chars, 1..600s, exit 0..127).
- Profiles add required kinds: `node-basic` = format/test/functional/negative;
  `node-browser` additionally requires `browser`; `generic` =
  build/test/functional/negative.
- `browser` checks are mandatory when present. Tool absence is `NOT_TESTED`;
  it can never be treated as PASS.
- Unknown fields, duplicate check IDs, unsafe paths and non-frozen specs are
  rejected. There is no spec update route.

## 3. CandidateRevision schema

```json
{
  "id": "…", "projectId": "default", "actionId": "…", "specId": "…",
  "specHash": "sha256-hex", "policyRevision": "quality-baseline-s4a-v1",
  "baseRevision": "…", "commitSha": "…", "treeSha": "…",
  "contentDigest": "sha256-hex", "candidateRef": "refs/factory/candidates/…",
  "parentCandidateId": "…", "repository": "/absolute/repo.git",
  "implementerRunId": "…", "implementerAttemptId": "…",
  "implementerGeneration": 1, "implementerWorkerId": "local-1",
  "executionMode": "trusted-local", "provider": "…", "model": "…",
  "paseoMode": "…", "changedPaths": ["add.js", "app.js"],
  "riskClass": "R0", "state": "frozen | superseded | rejected",
  "createdAt": "…", "frozenAt": "…", "supersededAt": "…"
}
```

The Worker may only claim a commit id and a `refs/factory/candidates/…` ref.
The Core independently resolves the commit in the authorized repository,
requires exactly the frozen base as its single parent, recomputes tree/content
digests, rejects empty diffs and any path outside `allowedPaths`, reclassifies
risk from the actual diff, and only then freezes the row. Freezing supersedes
previous frozen candidates for the same Action (parent link retained) and emits
`CandidateFrozen`/`CandidateSuperseded` events.

A spec-bound Run cannot report `completed` until a frozen candidate exists for
its exact Attempt generation. A failed Attempt never becomes a candidate.

## 4. Evidence

### Verifier evidence

```json
{
  "candidateId": "…", "candidateSha": "…", "specId": "…", "specHash": "…",
  "policyRevision": "…", "authority": "verifier", "actor": "verifier-1",
  "independent": true, "cleanResource": "git-clone:…", "oracleDigest": "…",
  "status": "passed | failed | blocked | not_tested",
  "checks": [{"id":"browser","kind":"browser","status":"PASS","exitCode":0,"durationMs":123,"detail":"chromium …; consoleErrors=0 …"}],
  "findings": [], "idempotencyKey": "…", "createdAt": "…"
}
```

The Core overwrites `authority`, independence, command argv and oracle digest
from the frozen spec/credential; unknown check IDs/kinds, identity mismatches,
pre-candidate timestamps and implementer actor identity are rejected. Browser
check details retain only bounded tool/provenance metadata (browser version,
console/request counts, isolated profile), never page content or tokens.

### Reviewer evidence

```json
{
  "candidateId": "…", "candidateSha": "…", "specId": "…", "specHash": "…",
  "policyRevision": "…", "authority": "reviewer", "actor": "reviewer-1",
  "verdict": "APPROVE | REQUEST_CHANGES | BLOCKED",
  "findings": [{"id":"review-1","severity":"blocking","message":"…"}],
  "diffStat": "…", "diffBytes": 42, "idempotencyKey": "…", "createdAt": "…"
}
```

The reviewer route is read-only; there is no candidate/oracle mutation route.
The reviewer actor cannot equal the implementer worker.

## 5. Deterministic Quality Gate

Inputs: frozen candidate/spec, succeeded Implementer Run/Attempt, every durable
verification and review. The gate emits synthetic checks for identity, scope,
freshness, independence, security and review, plus one result per spec check.

Rules:

- Any identity/scope/freshness/independence failure → `BLOCKED`.
- Actual R2/R3 or an under-classified spec → `HUMAN_REQUIRED`.
- Any mandatory check `FAIL` or reviewer `REQUEST_CHANGES`/`BLOCKED` →
  `REPAIR_REQUIRED` (reviewer `BLOCKED` is `BLOCKED` when no fix is implied by
  the finding; the gate fails closed).
- Missing/`NOT_TESTED` required evidence → `NOT_TESTED`/`BLOCKED`; never PASS.
- All required evidence `PASS` and reviewer `APPROVE` → `ACCEPTED`.
- A decision is persisted only when verifier and reviewer evidence exist. It is
  keyed uniquely by candidate, immutable across replay/restart, and repeated
  evidence delivery is idempotent. A new decision requires a new candidate.

Individual checks retain `PASS`, `FAIL`, `SKIPPED`, `NOT_APPLICABLE`,
`NOT_TESTED`; no unchecked status is PASS.

## 6. HTTP routes

| Method | Route | Auth | Meaning |
| --- | --- | --- | --- |
| POST | `/api/v1/action-specs` | operator | Freeze a spec |
| GET | `/api/v1/action-specs/{id}` | operator | Read spec |
| GET | `/api/v1/actions/{id}/specs` | operator | List action specs |
| GET | `/api/v1/candidates` | operator | List candidates (`actionId`,`runId`,`limit`) |
| GET | `/api/v1/candidates/{id}` | operator | Read candidate |
| GET | `/api/v1/candidates/{id}/evidence` | operator | Candidate bundle (spec/evidence/decision) |
| GET | `/api/v1/runs/{id}/candidate` | operator | Latest candidate for a Run |
| POST | `/api/v1/candidates/{id}/gate` | operator | Evaluate/read the one decision |
| GET | `/api/v1/workers/specs/{id}` | worker | Frozen spec for Implementer |
| POST | `/api/v1/workers/candidates` | worker | Register a commit claim for Core validation |
| GET | `/api/v1/verifier/candidates/{id}` | verifier | Read-only candidate+spec |
| POST | `/api/v1/verifier/evidence` | verifier | Append verification evidence |
| GET | `/api/v1/reviewer/candidates/{id}` | reviewer | Read-only candidate+spec |
| POST | `/api/v1/reviewer/verdict` | reviewer | Append review verdict |

Credentials are scoped and never interchangeable: operator, Worker, verifier and
reviewer tokens are rejected on each other's routes. Empty configured tokens
fail closed.

## 7. Events and projections

`ActionSpecFrozen` is recorded on the spec aggregate; `CandidateFrozen`,
`CandidateSuperseded`, `VerificationRecorded`, `ReviewRecorded` and
`QualityGateDecided` are recorded on the candidate aggregate. They use the S1
event ledger and SSE replay; no second ledger exists. Office reads Core
snapshots and refreshes on candidate events; it never invents review,
verification or gate activity.

## 8. Browser acceptance (A13–A18)

- **A13:** `@playwright/cli` is pinned in `office/e2e-browser`; a supported
  Implementer invokes it with an explicit Chromium executable and a disposable
  profile. The capability is documented and exposed through
  `FACTORY_BROWSER_CLI` in the Implementer prompt.
- **A14:** the agent journey unlocks Office, creates an Action through the real
  form and verifies the resulting Action via the Core API.
- **A15:** the Independent Verifier runs the frozen `browser` check from a clean
  read-only checkout with real Chromium, recording candidate/spec/oracle-bound
  results.
- **A16:** a deterministic UI regression makes the browser check `FAIL`; the
  gate cannot accept even when other checks and the reviewer pass.
- **A17:** verifier browser checks receive no Factory credentials; each run uses
  a fresh checkout, temp HOME and isolated browser profile; evidence contains
  identity, tool version and bounded diagnostics only.
- **A18:** existing S1–S3-D Chromium/SSE/recovery suites remain unchanged and
  run in CI alongside `e2e-s4a`.

## 9. Non-goals

No delivery/merge, no S5/S6, no full registry, no remote Workers, no
ISOLATED/SEC-01 certification, and no general MCP management subsystem. The
Playwright MCP server is deferred; see
[browser automation](../engineering/browser-automation.md).
