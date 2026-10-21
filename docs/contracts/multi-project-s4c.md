# Contract: Multi-project registry, scoped capabilities and browser capability (F1-S4-C)

**Status:** Implemented for F1-S4-C; final candidate acceptance is recorded in
the PR. **Version:** `1`. Experimental pre-release contract; breaking changes
update all in-repo callers, fixtures, docs and tests atomically (ADR-0006).
TRUSTED_LOCAL only; this is not isolation certification.

S4-C extends the S1 Project, S4-A ActionSpec/Candidate and S4-B Campaign
lifecycles. It does not introduce a second project store, scheduler or
candidate lifecycle.

## 1. Project identity and configuration

| Field | Rule |
| --- | --- |
| `id` | Stable slug `[a-z0-9_-]{1,64}`; the seeded `default` remains `legacy` |
| `registrationStatus` | `registered` or `legacy`; legacy rows carry no repository/capability grant |
| `status` | `active` or `paused`; paused Projects receive no new work |
| `repositoryPath` | Symlink-resolved absolute local Git working tree or bare repository |
| `repositoryIdentity` | SHA-256 over `factory-git-identity-v2`, canonical path and sorted root commits |
| `baseRefs` | Non-empty safe Git refs; every base revision must equal a current tip |
| `qualityProfile` | `node-basic`, `node-browser` or `generic`; required capabilities derive from it |
| `workspaceScope` | `.` or a safe repository-relative prefix; spec allowed paths must stay inside |
| `capabilities` | Closed set: `repository-edit`, `node-toolchain`, `generic-toolchain`, `browser` |
| `browserEndpoints` | Plain `http://` loopback endpoints; required when `browser` is granted |
| `executionMode` | Exactly `trusted-local`; no isolated-to-local fallback |
| `securityLabel` | `TRUSTED_LOCAL` |
| `configRevision` / `configHash` | Monotonic revision and deterministic hash of the configuration |

Registration and updates are Core-authorized operator operations. Repository
identity is immutable. A repository path cannot change while frozen specs
exist. A capability required by any frozen spec cannot be revoked. Each
revision is stored as an immutable audit snapshot.

## 2. Project-bound ActionSpec

The Core derives `repository`, `baseRevision` validation, `workspaceScope`,
`browserEndpoints`, required capabilities, `projectConfigRevision` and
`repositoryIdentity` from the registered Project. A caller-supplied repository
that differs from the Project is rejected.

`ActionSpec.bindingHash` is SHA-256 over project/action/config revision,
repository identity/path/base, capabilities, browser endpoints, Quality Profile
and allowed paths. The legacy S4-A `contentHash` remains the content identity.
Candidates record `specBindingHash`; the Quality Gate requires equality.

A later Project configuration change never rewrites a frozen spec or accepted
evidence. New specs use the current revision. A frozen spec can dispatch only
while the Project is active, the repository identity matches and the Project
still grants its capabilities.

## 3. Capability and Worker matching

- Capabilities are snapshotted into the Run by Core from the frozen spec; the
  Worker cannot widen them.
- Workers register observed capabilities and execution modes. A Run is
  eligible only when the Worker's fresh heartbeat, capabilities and execution
  mode are a superset of the Run requirements.
- A Project that requires unavailable browser tooling yields a truthful
  `BLOCKED` campaign after a bounded grace; there is no privileged fallback.
- Project rotation uses last-dispatch time so eligible work in one Project is
  not indefinitely starved by another.

## 4. Factory-managed browser capability

For a Run requiring `browser`, the Worker:

1. verifies the pinned CLI, Chromium and `playwright-core` are available;
2. writes a run-scoped `.agents/skills/factory-browser/SKILL.md` and a
   `.factory/bin/factory-playwright-cli.mjs` wrapper;
3. writes a generated `playwright-cli` config with the explicit Chromium path;
4. rejects non-loopback or unauthorized HTTP arguments in the wrapper;
5. records bounded invocation metadata (`browserInvoked`, count, command names,
   CLI version, skill digest) in the Run result;
6. closes the run-scoped browser and removes the disposable profile.

The wrapper and skill are excluded from candidate staging. This is
instruction/control-plane scoping, not OS enforcement. The Quality Gate
requires `browserInvoked=true` when the frozen spec requires `browser`.

The Independent Verifier does not use this wrapper. It executes the frozen
protected `browser` check from a clean read-only candidate checkout with its own
temporary Home/profile and records Chromium provenance. Missing tooling is
`NOT_TESTED`; a failure is `FAIL`; neither is accepted.

## 5. Project binding propagation

`Project → Action → ActionSpec → Campaign → Task → Run → Attempt → Paseo
session → CandidateRevision → Evidence → GateDecision` carries project identity.
Core rejects:

- Actions pointing at unknown/unregistered Projects where a registered binding
  is required;
- ActionSpecs with a different repository, identity, base-policy violation or
  out-of-scope path;
- Runs whose Project/Action/Spec relationship conflicts;
- candidate/evidence relationships whose Project, spec, binding hash, candidate
  SHA or oracle digest differ;
- cross-project verifier/reviewer evidence.

Legacy `default` data is preserved. It does not silently acquire a newly
registered repository.

## 6. HTTP surface

| Method | Route | Auth | Meaning |
| --- | --- | --- | --- |
| GET | `/api/v1/projects` | operator | List Projects with configuration state |
| POST | `/api/v1/projects` | operator | Register an authorized local Git repository |
| GET | `/api/v1/projects/{id}` | operator | Project configuration |
| PATCH | `/api/v1/projects/{id}` | operator | Update mutable configuration/status |
| GET | `/api/v1/projects/{id}/refs` | operator | Revalidated base-ref tips |
| GET | `/api/v1/projects/{id}/events` | operator | Project audit replay |
| GET | `/api/v1/projects/{id}/config?revision=N` | operator | Immutable configuration snapshot |
| POST | `/api/v1/action-specs` | operator | Freeze a Project-bound spec |
| GET | `/api/v1/candidates?projectId=…` | operator | Project-scoped candidate history |
| GET | `/api/v1/campaigns?projectId=…` | operator | Project-scoped campaign history |

Existing worker/verifier/reviewer routes keep their scoped credentials.

## 7. Non-goals

No remote Git authentication, GitHub App/credential storage, S5 delivery,
S6 self-development, remote Workers, general MCP orchestration, dynamic global
skill marketplace, full workflow DSL or SEC-01-ISOLATED certification.
