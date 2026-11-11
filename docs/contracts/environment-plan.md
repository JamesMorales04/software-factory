# Contract: environment plan and spec (agnostic Nix/devenv)

**Status:** Proposed WP0 contract — 2026-11-10. This freezes the interface for
the environment work packages; it is not implementation or acceptance evidence.
**Version:** `1`. Experimental pre-release contract; breaking changes update all
in-repo callers, fixtures, docs and tests atomically
([ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md)).

**Decision:** [ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md).
**License evidence:** [Nix/devenv license findings](../evidence/nix-devenv-license.md).
**Security:** `TRUSTED_LOCAL`; generated Nix/devenv files and the builder are
untrusted project input. A container is not an isolation certification.

## 1. Scope

This contract defines how Core turns a project's committed environment
conventions into one reviewed, licensed, pinned and frozen Nix/devenv
environment, and how that environment is bound to checks, candidates and
delivery.

It does not define a second scheduler, a host toolchain, a per-language image
selection or a deployment. It does not authorize a package, credential, paid
service or unfree license by itself.

## 2. Identities

| Entity | Identity | Notes |
| --- | --- | --- |
| `ProjectConventions` | `(projectId, baseCommit, conventionsSha256)` | Deterministic snapshot of committed evidence; no working-tree input |
| `EnvironmentPlan` | random 32-hex `planId` + `planSha256` | Canonical plan content; excludes ID/timestamps |
| `EnvironmentPlanReview` | random 32-hex `reviewId` | Lead planning decision bound to one plan revision |
| `EnvironmentSpec` | random 32-hex `specId` + `specSha256` | Immutable frozen executable environment |
| `EnvironmentBuildReceipt` | random 32-hex `receiptId` | One build/evaluation attempt for one spec hash |
| `LicenseInventory` | `(specSha256, inventorySha256)` | SBOM, license expressions, attribution and owner overrides |
| `EnvironmentBlocker` | stable code + bounded detail | Fail-closed result; no partial execution |

All hashes are lowercase SHA-256 over canonical JSON or canonical file bytes as
specified by the implementation. IDs, timestamps and display labels are excluded
from content hashes.

## 3. ProjectConventions

```json
{
  "schemaVersion": 1,
  "projectId": "…",
  "baseCommit": "git-object-id",
  "conventionsSha256": "sha256-hex",
  "scannedAt": "RFC3339",
  "files": [
    {
      "path": "devenv.lock",
      "kind": "DEVENV_LOCK",
      "sha256": "sha256-hex",
      "precedence": "COMMITTED_ENVIRONMENT",
      "pins": [{"name": "nixpkgs", "value": "github:NixOS/nixpkgs/…", "source": "devenv.lock"}]
    }
  ],
  "commands": [
    {"name": "test", "argv": ["cargo", "test"], "cwd": ".", "source": "CI"}
  ],
  "conflicts": [],
  "truncated": false,
  "skippedEntries": 0,
  "provenance": "git-object-store"
}
```

Recognised evidence includes `flake.nix`/`flake.lock`, `devenv.nix`/
`devenv.yaml`/`devenv.lock`, `.devcontainer/devcontainer.json` and its referenced
files, `mise.toml`/`.mise.toml`/`.tool-versions`, `.nvmrc`, `.python-version`,
`rust-toolchain.toml`/`rust-toolchain`, `global.json`, package-manager
declarations and lockfiles for the project's actual ecosystem, CI workflow
commands and documented `AGENTS.md`/README/ADR commands.

Rules:

1. The snapshot reads only the exact `baseCommit` from the Git object store. The
   working tree, host paths, symlink targets and uncommitted files never
   contribute.
2. Precedence is: explicit authenticated owner constraint; committed
   environment pins/lock data; version files/CI commands; manifest inference;
   model proposal. Equally-specific contradictions fail closed.
3. There is no hardcoded language default. A Rust/Node/Python repository does
   not select an image or toolchain for another project. Factory's own
   Rust/Node development pins are not user-project defaults.
4. A missing or stale snapshot yields a new revision; it is never reused across
   base commits.
5. Issue text, prompts, remote documentation and model output are not
   conventions; they are untrusted inputs.

## 4. EnvironmentPlan

```json
{
  "schemaVersion": 1,
  "planId": "…",
  "projectId": "…",
  "requestId": "…",
  "baseCommit": "…",
  "conventionsSha256": "…",
  "status": "DRAFT | LEAD_APPROVED | CORE_VALIDATED | FROZEN | REJECTED | BLOCKED",
  "revision": 1,
  "toolchains": [
    {"id": "python", "version": "3.12.7", "source": ".python-version", "evidence": ["…"]}
  ],
  "packages": [
    {"name": "ripgrep", "version": "14.1.1", "license": "MIT", "source": "nixpkgs", "sha256": "…", "unfree": false, "ownerOverrideId": null}
  ],
  "services": [
    {"name": "postgresql", "version": "17", "ephemeral": true, "network": "LOOPBACK"}
  ],
  "commands": {
    "build": {"argv": ["…"], "cwd": ".", "timeoutSecs": 600},
    "test": {"argv": ["…"], "cwd": ".", "timeoutSecs": 600},
    "lint": {"argv": ["…"], "cwd": ".", "timeoutSecs": 300},
    "format": {"argv": ["…"], "cwd": ".", "timeoutSecs": 300},
    "typecheck": {"argv": ["…"], "cwd": ".", "timeoutSecs": 300},
    "run": {"argv": ["…"], "cwd": ".", "timeoutSecs": 300}
  },
  "files": {
    "flakeNix": {"path": "flake.nix", "sha256": "…"},
    "flakeLock": {"path": "flake.lock", "sha256": "…"},
    "devenvNix": {"path": "devenv.nix", "sha256": "…"},
    "devenvYaml": {"path": "devenv.yaml", "sha256": "…"},
    "devenvLock": {"path": "devenv.lock", "sha256": "…"}
  },
  "nix": {
    "allowUnfree": false,
    "sandbox": true,
    "impure": false,
    "substituters": ["optional-public-cache-id"]
  },
  "resources": {"cpu": 2, "memoryMiB": 4096, "diskMiB": 10240, "pids": 512, "timeoutSecs": 1800},
  "network": {"build": "NONE", "fetch": "BROKER", "fetchAllowlist": ["…"]},
  "licenseInventoryRef": "…",
  "sbomRef": "…",
  "attributionRef": "…",
  "blockers": [],
  "createdAt": "RFC3339"
}
```

Rules:

- Every executable command is an argv vector, not a shell string. The contract
  bounds argv count/length, working directory to the project root, timeout and
  allowed exit status. Shell interpolation of untrusted values is forbidden.
- `commands` may be empty only when the related check kind is not required; the
  plan must state why. Build/test/functional/negative checks still follow the
  Quality Profile.
- Environment variable entries are names and provenance only. Secret values are
  never placed in the plan, generated files, logs or evidence; they are injected
  by the authorized runtime at execution.
- Services are ephemeral and loopback-only unless an owner-approved plan says
  otherwise. No host port, host path or host socket is implicit.
- `nix.allowUnfree` is `false`; `nix.sandbox` is `true`; `nix.impure` is `false`.
  A request to change any of these is a blocker/owner decision, not a plan edit.
- The plan may propose files, but Core writes them only into an ephemeral staging
  area; the untrusted content is not executed during planning.
- For a greenfield request the Lead planning model may carry a bounded,
  untrusted `EnvironmentProposal` (exact nixpkgs pin or omitted builder default,
  toolchains, nixpkgs packages with SPDX licences and argv commands). It fills
  only missing fields after explicit request constraints and committed
  conventions; it never enables `allowUnfree`, chooses a language by default,
  grants capabilities or bypasses Core validation and the Lead review.
- When semantic triage and deterministic detection leave the stack empty, one
  bounded Lead-route model call selects an exact deterministic stack option;
  Core validates the option and its required project capability and records a
  `MODEL` stack decision. Unknown, empty or unauthorized selections fail closed
  with a typed `STACK_SELECTION_*` planning failure.

## 5. Lead approval and Core freeze

```json
{
  "reviewId": "…",
  "requestId": "…",
  "planSha256": "…",
  "planRevision": 1,
  "reviewerRole": "LEAD",
  "principalId": "lead:<session>",
  "decision": "APPROVED | CHANGES_REQUESTED | REJECTED",
  "rationale": "bounded text",
  "decidedAt": "RFC3339"
}
```

- The Lead is a bounded model role. It approves the plan by recording an
  `APPROVED` planning decision bound to the exact plan hash and revision. The
  Owner is not the routine approver; the Owner decides only a reserved matter.
- Lead approval grants **no** package, tool, credential, network, license,
  execution or delivery authority.
- Core requires `APPROVED` before freezing, but Core validation is
  authoritative. A Lead approval cannot override a failed schema, pin, license,
  sandbox or secret check.
- Owner approval is required only for reserved matters: an unfree override, a
  new paid service/credential, a stack change above the frozen autonomy budget,
  a public-contract change, an irreversible action or a policy exception.
- Freezing creates one immutable `EnvironmentSpec`; it does not run the build.

## 6. EnvironmentSpec

```json
{
  "schemaVersion": 1,
  "specId": "…",
  "specSha256": "…",
  "projectId": "…",
  "requestId": "…",
  "baseCommit": "…",
  "planSha256": "…",
  "conventionsSha256": "…",
  "reviewId": "…",
  "platform": "x86_64-linux",
  "nixVersion": "pinned",
  "devenvVersion": "pinned",
  "flakeLockSha256": "…",
  "devenvLockSha256": "…",
  "licenseInventorySha256": "…",
  "sbomSha256": "…",
  "resourcePolicy": {"…": "…"},
  "networkPolicy": {"build": "NONE", "fetch": "BROKER"},
  "status": "FROZEN | SUPERSEDED",
  "frozenAt": "RFC3339"
}
```

An `EnvironmentSpec` is immutable. Its `specSha256` is included in the
ActionSpec binding, the candidate record, verifier evidence and the delivery
record. A later environment change creates a new spec; it never rewrites a
frozen spec, accepted evidence or a merged delivery.

## 7. License Gate

### 7.1 Component findings

The Factory environment components are admitted with these findings:

| Component | License | Required attribution/obligation |
| --- | --- | --- |
| Nix | LGPL-2.1 (`COPYING`; do not assume an "or later" grant without checking the exact release) | Include the license and source offer/relinkable form when distributing a modified Nix binary; Factory invokes Nix as a separate tool and does not link it into Factory code. |
| nixpkgs | MIT | Preserve the copyright and permission notice in distributions/substantial portions. |
| devenv | Apache-2.0 | Retain notices, include any `NOTICE`, mark modified files and comply with the patent-termination terms. The repository `LICENSE` and `Cargo.toml` are authoritative; the marketing page's "MIT" text is a documentation inconsistency. |

Every package selected by the plan has its own license. The component table does
not license the packages.

### 7.2 Gate rules

- `allowUnfree = false` is the default and the frozen value unless an exact
  owner override exists.
- Allowed identifiers are recorded as SPDX expressions. Unknown, missing,
  contradictory or denied licenses produce `ENVIRONMENT_LICENSE_DENIED` or
  `ENVIRONMENT_LICENSE_UNKNOWN`.
- An unfree package requires an authenticated owner override with
  `(package, version, license, planSha256, reason, expiresAt, decisionId)`. It
  applies only to that exact package/version/plan. It never sets a global
  `allowUnfree = true`.
- No Devenv Cloud, Cachix account/token or paid binary-cache service is
  required. `cachix` authentication config, a cloud-only input or a mandatory
  hosted cache is `ENVIRONMENT_PAID_SERVICE_REQUIRED` unless the owner explicitly
  authorizes a new service (which is a reserved matter).
- A public cache such as `cache.nixos.org` may be recorded as an optional
  substitute; the plan must also support a local/offline build. Optional cache
  failure is not a license failure, but it may be a build blocker.
- The SBOM and license inventory include package name, version, source URL,
  hash, license expression, attribution text and any override. The inventory is
  part of the frozen spec and delivery evidence.

## 8. Builder sandbox

The generated plan/files are untrusted. Build/evaluation runs in a dedicated
builder with this minimum policy:

| Control | Requirement |
| --- | --- |
| Identity | Non-root ephemeral user; no host `HOME`, SSH agent or git credential helper |
| Secrets | No Factory, provider, GitHub, cloud or host credentials; no secret env/args/files |
| Network | Build/eval `NONE`; package/source acquisition only through the credential-free broker with an explicit allowlist |
| Nix | `sandbox=true`, `--impure` forbidden, no `import <nixpkgs>`, no unpinned `fetchTarball`/`fetchGit`, no `builtins.exec`, no `allowUnfree=true`, no `sandbox=false` |
| Filesystem | Read-only inputs; ephemeral writable store/tmp; no host socket, Podman/Docker socket or Nix daemon socket |
| Resources | Bounded CPU, memory, disk, PIDs and wall-clock time; output/truncated logs bounded |
| Evidence | Exact spec hash, platform, Nix/devenv versions, lock hashes, build command, result, duration, resource limits and sandbox attestation; secrets and source bodies redacted |

A sandbox attestation is evidence of the controls observed for that build, not
an isolation certification. Missing attestation is `NOT_TESTED`; it cannot be
presented as a passing build.

## 9. Invariants

1. Every executable project/check has one frozen `EnvironmentSpec`; there is no
   implicit host or language-image fallback.
2. Conventions come from the exact base commit and precede model proposals;
   there is no hardcoded language default.
3. The Lead review is recorded but grants no authority; Core validation, pinning,
   licensing and freezing are authoritative.
4. `allowUnfree=false` by default; only an exact, authenticated owner override
   may admit an unfree package.
5. No Devenv Cloud/Cachix account, token or paid service is required.
6. Generated Nix/devenv files are untrusted; the builder has no secrets, no host
   network and no host sockets, and runs under resource limits.
7. The exact environment hash is bound to the candidate, checks and delivery.
8. Missing or conflicting conventions, licenses, pins or sandbox evidence fail
   closed.
9. Model output never grants execution, credential, license or delivery
   authority.
10. A frozen spec is immutable; a later environment change creates a new
    revision and never rewrites accepted evidence.

## 10. Failure codes

| Code | Meaning |
| --- | --- |
| `ENVIRONMENT_CONVENTIONS_MISSING` | No usable committed conventions and no explicit owner selection |
| `ENVIRONMENT_CONVENTIONS_CONFLICT` | Equally-specific pins/commands contradict |
| `ENVIRONMENT_CONVENTIONS_STALE` | Snapshot base commit differs from the request/candidate base |
| `ENVIRONMENT_PLAN_INVALID` | Schema, path, argv, resource or network policy invalid |
| `ENVIRONMENT_UNSUPPORTED` | The requested toolchain/package/service cannot be expressed in Nix/devenv |
| `ENVIRONMENT_PIN_MISSING` | A tool, input or package has no exact pin/hash |
| `ENVIRONMENT_LICENSE_DENIED` | Package/license denied by policy |
| `ENVIRONMENT_LICENSE_UNKNOWN` | License missing/unknown/contradictory |
| `ENVIRONMENT_UNFREE_REQUIRES_OWNER` | Unfree package without an exact owner override |
| `ENVIRONMENT_PAID_SERVICE_REQUIRED` | Cloud/Cachix/paid service is required without owner authorization |
| `ENVIRONMENT_LEAD_APPROVAL_REQUIRED` | No bound Lead `APPROVED` review |
| `ENVIRONMENT_OWNER_APPROVAL_REQUIRED` | Reserved matter not decided |
| `ENVIRONMENT_SECRET_DETECTED` | Secret-shaped value in plan/files/args/logs |
| `ENVIRONMENT_SANDBOX_DENIED` | Sandbox/impure/network policy violation |
| `ENVIRONMENT_NETWORK_DENIED` | Fetch not on the broker allowlist |
| `ENVIRONMENT_RESOURCE_LIMIT` | CPU/memory/disk/PID/time bound exceeded |
| `ENVIRONMENT_BUILD_FAILED` | Nix/devenv evaluation or build failed |
| `ENVIRONMENT_BUILD_TIMEOUT` | Build exceeded its bound |
| `ENVIRONMENT_HASH_MISMATCH` | Spec/lock/SBOM/license/candidate hash differs |
| `ENVIRONMENT_NOT_FROZEN` | Execution/check attempted without a frozen spec |
| `ENVIRONMENT_SUPERSEDED` | Spec was superseded before use |

Codes are stable API values; human-readable details are bounded and redacted.

## 11. Idempotency

- Compile is keyed by
  `SHA256(projectId, requestId, baseCommit, conventionsSha256, policyRevision,
  explicitConstraintsHash)`. Recompiling the same key returns the same plan;
  changed input creates a new revision.
- Freeze is unique on `(planSha256, reviewId, environmentPolicyRevision)`.
  Repeating freeze returns the existing spec; it never creates a second spec.
- Build is unique on `(specSha256, platform, builderPolicyRevision)`. A repeated
  build returns the existing receipt; a retry after a transient failure creates
  a new receipt revision but not a new spec.
- A duplicate request, Core restart or replay never duplicates an environment
  spec, license override, build artifact or evidence row.

## 12. Evidence

A frozen environment retains:

1. the exact conventions snapshot and hash;
2. the plan and Lead review record;
3. the frozen spec and all lock/derivation hashes;
4. the license inventory, SBOM, attribution and any owner override;
5. the builder sandbox attestation, exact commands and bounded logs;
6. build/check results bound to the spec hash;
7. the candidate/check/delivery bindings that reference the spec.

Evidence is immutable and candidate-bound. A screenshot, a model statement or a
successful package search is not build evidence.

## 13. Office impact and accessibility

The implementation must expose the environment through an existing
authenticated Office plan-review/request surface (no new authority is created by
this contract):

- conventions sources and conflicts, with exact paths and pins;
- selected toolchains/versions, packages, services and commands;
- `allowUnfree`, license gate, SBOM/attribution links and owner override state;
- Lead decision, Core validation, freeze hash and blockers;
- build/sandbox status, resource/network policy and bounded evidence;
- loading, empty (no conventions), blocked, conflicting, build-failed,
  license-denied, stale and frozen states;
- no edit of a frozen spec; a recompile requires a new revision;
- keyboard operation, visible focus, screen-reader names, non-color state cues,
  no horizontal overflow at 320 px and EN/ES copy.

Real Chromium acceptance must cover each option/state; a route smoke test or a
screenshot is not acceptance. Implementation must update the Office parity
matrix and capability registry in the same delivery. WP0 does not change parity
states or capabilities.

## 14. Positive and negative acceptance cases

Positive:

- a `devenv.nix` project with a lock resolves its exact pins and produces a
  frozen spec with a complete license inventory;
- a Rust/Node/Python/Go project keeps its committed versions and commands and
  runs checks in the frozen environment;
- a project with an unfree package and an exact owner override is frozen with
  the override recorded and visible;
- a repeated compile/freeze/build is idempotent.

Negative:

- a project with no conventions and no explicit selection is blocked, not
  defaulted to Rust/Node;
- conflicting `rust-toolchain.toml` and CI pins are blocked;
- a generated `builtins.exec` or unpinned fetch is rejected;
- a build attempting network, a secret, host socket or over-limit resources is
  blocked and evidenced;
- an unknown license or `allowUnfree=true` without override is blocked;
- a candidate/check with a different environment hash is rejected;
- a Lead approval that conflicts with a Core failure remains blocked.

## 15. Compatibility

This is an experimental pre-release contract. Until a stable external contract is
declared, incompatible changes update every in-repo caller, fixture, test and
document in one change. Persisted environment specs are immutable; a revision is
a new row, never an in-place rewrite. See
[ADR-0006](../architecture/decisions/ADR-0006-early-development-compatibility.md).
