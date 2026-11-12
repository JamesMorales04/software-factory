# ADR-0041 — Agnostic environment compiler

**Status:** Proposed — 2026-11-10. This ADR records the Product Owner's
program constraints for implementation. It is not accepted and grants no
implementation, execution, merge or delivery authority until the protected
approval path records acceptance and independent review.

**Supersedes:** none.

**Extends:** [ADR-0018](ADR-0018-native-rust-replacement.md),
[ADR-0025](ADR-0025-container-native-and-office-parity.md),
[ADR-0034](ADR-0034-model-capability-telemetry-and-agent-scope.md),
[ADR-0038](ADR-0038-owner-charter-and-delegated-autonomy.md) and
[ADR-0039](ADR-0039-quality-first-planning-and-stack-selection.md).

**Contract:** [environment-plan](../../contracts/environment-plan.md).

**Evidence:** [Nix/devenv license findings](../../evidence/nix-devenv-license.md).

**Security status:** `TRUSTED_LOCAL`. A generated environment and its builder are
untrusted project inputs; a container is not an isolation certification.

## Context

The current Factory execution path is language-biased:

- the only check image configured in Compose is a Rust image
  (`FACTORY_VERIFIER_IMAGE: docker.io/library/rust:1.90.0-bookworm`);
- the runtime broker image allowlist contains the same Rust image;
- `DependencyKind::Rust` drives a plan that requires `Cargo.lock`/`Cargo.toml`
  and a crates.io-style vendor/cache;
- `SpecCheck` has no environment, image, toolchain or working-directory binding,
  so every check inherits one global verifier image;
- stack detection is manifest-driven and prioritises Rust/Node conventions;
- the Lead prompt tells the model that `generic-toolchain` provides `cargo`,
  `rustc` and `python3`, which is a hidden language default;
- project discovery understands many manifests but not `flake.nix`,
  `devenv.nix`, `.devcontainer`, `mise.toml`, `.tool-versions` or CI pins.

This makes a Rust or Node project the path of least resistance and forces every
other language into a hand-written exception. It also conflicts with
[ADR-0039](ADR-0039-quality-first-planning-and-stack-selection.md): technology
selection must be an explicit, evidence-based project decision, not an
accident of the check image.

The owner wants an agnostic compiler that reads a project's real conventions,
produces a reproducible Nix/devenv environment, validates licenses, builds
without secrets or uncontrolled network and freezes the exact environment used
for verification. The compiler must not silently fall back to a host toolchain,
a language-specific Dockerfile or a different package manager when it cannot
produce a valid plan.

## Decision

### 1. Nix + devenv is the only project environment substrate

Every Project execution environment is described by a Core-frozen
`EnvironmentSpec` compiled to Nix/devenv inputs (a flake and/or devenv files)
and their lock data. There is **no fallback** to:

- a host-installed compiler, package manager, Node, Python, Rust, Go or tool;
- a language-specific runtime image chosen by convention;
- a hand-written Dockerfile/Containerfile used instead of the environment spec;
- `mise`, `asdf`, `nvm`, `pyenv`, `rustup` or a CI-only toolchain as the
  execution environment;
- an unverified package manager download or an unpinned remote flake.

`mise`, `asdf`, `rust-toolchain.toml`, `.nvmrc`, `.python-version`,
`devcontainer.json`, lockfiles and CI files are **convention evidence** used to
choose versions and commands. They are not execution backends. If the compiler
cannot produce a valid, pinned Nix/devenv plan, the request is `BLOCKED` with an
explicit reason and a bounded remediation; it does not run with "whatever is
installed".

Nix itself is invoked as a separate executable/tool. Factory does not link Nix
libraries into its own binary. The license obligations of Nix, nixpkgs, devenv
and every selected package are handled by the License Gate and attribution
requirements in section 4.

### 2. ProjectConventions first; no hardcoded language defaults

The compiler first produces a typed, hash-addressed `ProjectConventions`
snapshot from the exact base commit. It reads, where present:

- `flake.nix`, `flake.lock`, `devenv.nix`, `devenv.yaml`, `devenv.lock`;
- `.devcontainer/devcontainer.json` and its referenced container files;
- `mise.toml`, `.mise.toml`, `.tool-versions`, `.nvmrc`, `.python-version`,
  `rust-toolchain.toml`, `rust-toolchain`, `global.json` and similar version
  pins;
- package-manager declarations and lockfiles (`Cargo.lock`, `package-lock.json`,
  `pnpm-lock.yaml`, `yarn.lock`, `bun.lock`, `poetry.lock`, `uv.lock`,
  `Pipfile.lock`, `go.sum`, `composer.lock`, `Gemfile.lock`, `mix.lock`,
  `pubspec.lock`, `Package.resolved`, and the project's actual ecosystem);
- CI workflows and documented build/test/lint/format commands;
- `AGENTS.md`, `README`, architecture docs, ADRs and repository conventions.

Precedence is:

1. an explicit, authenticated owner constraint for this request/project;
2. the project's committed environment pins and lock data (the most specific
   committed pin wins; conflicting equally-specific pins fail closed);
3. version files and CI commands;
4. manifest/package-manager inference;
5. a model proposal, which Core must validate and pin.

There is **no hardcoded language default**. A Rust repository does not imply a
Rust image for another project; a Node repository does not imply Node for
another project; the Factory repository's own Rust/Node development pins are
evidence for building Factory, not defaults for user projects. If a greenfield
project has no conventions, Core presents explicit options or requires the owner
to choose; it does not silently choose Rust, Node, Python or a "generic"
toolchain.

Conflicting conventions (for example, a `rust-toolchain.toml` pin that
contradicts CI, or a `devenv.nix` package version that contradicts a lockfile)
produce `ENVIRONMENT_CONVENTIONS_CONFLICT` and a bounded decision request. A
stale snapshot (base commit changed) is re-scanned before freezing.

### 3. The EnvironmentPlan is a reviewed planning artifact

The Planner/Environment Compiler emits a typed `EnvironmentPlan` containing the
conventions snapshot, selected toolchains and versions, packages, services,
commands, Nix/devenv files, lock data, license inventory, resource limits,
network policy and evidence references.

The **Lead** role approves the plan by recording an `EnvironmentPlanReview`
decision (`APPROVED`, `CHANGES_REQUESTED` or `REJECTED`) bound to the exact plan
hash and revision. This is a recorded planning decision, not an Owner reserved
decision and not an execution grant. The Lead is a bounded model role; its
approval does not make the plan safe or authoritative. The **Owner** is not the
approver for a routine environment plan; the Owner decides only a reserved
matter.

Core must still, deterministically:

- validate the schema, paths, commands, resource bounds and network policy;
- resolve and pin every toolchain, package, flake input and lock digest; a
  package version may be an exact version or a numeric dotted prefix constraint
  (`22`, `22.20`), resolved deterministically against the pinned nixpkgs to the
  exact observed version;
- run the License Gate and produce the SBOM/license inventory; the resolution
  records the exact observed package version and the original constraint, and
  there is no fallback to a different package, attribute or toolchain;
- reject secrets, impure evaluation, unpinned fetches and forbidden Nix
  constructs;
- bind the plan to the exact project/base commit/conventions hash;
- freeze the `EnvironmentSpec` only after the Lead approval and all Core checks
  pass.

The Owner is asked only for a reserved matter: an unfree-package override, a
new paid service or credential, a stack change above the frozen autonomy budget,
a public-contract change, an irreversible action or a policy exception. A model
or Lead approval never grants any of those. `APPROVED` from the Lead with a
failed Core validation is still `BLOCKED`.

### 4. License Gate

The compiler ships a License Gate. The default policy is:

- `allowUnfree = false`;
- no license is accepted by name alone; the exact package/version/source/hash
  and license expression must be recorded;
- unknown, missing, contradictory or denied licenses block the plan;
- an unfree package requires an explicit, authenticated owner override bound to
  the exact package/version/license/plan revision, with a reason and expiry;
- the override never silently changes `allowUnfree` globally and never applies
  to a different package or version.

The Factory environment components require these findings, recorded in
[Nix/devenv license findings](../../evidence/nix-devenv-license.md):

| Component | License finding | Commercial-use conclusion |
| --- | --- | --- |
| Nix | LGPL-2.1 (`COPYING`; do not assume an "or later" grant without checking the exact release) | Commercial use is permitted; distribute the license/source offer and relinkable form when distributing a modified Nix binary. Factory invokes Nix as a separate tool and does not link it into Factory code. |
| nixpkgs | MIT | Commercial use is permitted; preserve the copyright and permission notice. Each selected package still has its own license. |
| devenv | Apache-2.0 (repository `LICENSE` and `Cargo.toml`; the marketing page's "MIT" text is stale/inconsistent) | Commercial use is permitted; retain notices, include any `NOTICE`, mark modified files and comply with the patent-termination terms. |

No Devenv Cloud, Cachix account, Cachix token or paid binary-cache service is a
requirement. The open-source devenv CLI and local Nix store must work without a
hosted account. A public cache such as `cache.nixos.org` may be used as an
optional performance input, but the product must have an explicit local/offline
path and must not require a commercial subscription. Any optional cache is
recorded as optional, not as a license dependency.

The plan must include:

- an SBOM (CycloneDX or SPDX) covering selected packages, versions, sources,
  hashes and license expressions;
- a license inventory with attribution/notice text and source URLs;
- the `allowUnfree` decision and any owner override;
- a deterministic check that the generated files do not enable unfree packages
  or bypass the Gate.

### 5. Generated files and the builder are untrusted

The EnvironmentPlan and the generated Nix/devenv files are model-generated,
untrusted project input. Core validates them, but validation is not a sandbox.
Nix evaluation and builds execute code, so they run in a dedicated builder
sandbox:

- no Factory, provider, GitHub, cloud or host credentials;
- no host `HOME`, SSH agent, git credential helper, Podman/Docker socket or
  Nix daemon socket;
- no host network; package/source acquisition goes through the credential-free
  egress broker with an explicit allowlist, and the build/evaluation step runs
  with network disabled against the prefetched content-addressed store;
- no `--impure`, no host environment inheritance and no mutable global Nix
  configuration;
- `sandbox = true`; a request to disable it is `ENVIRONMENT_SANDBOX_DENIED`;
- non-root user, read-only inputs, ephemeral writable store/tmp, bounded CPU,
  memory, disk, PIDs and wall-clock time;
- bounded logs with secret redaction; raw prompts, values and source bodies do
  not enter telemetry;
- output produces a content-addressed store path and exact lock/derivation
  hashes.

Core rejects generated text containing `builtins.exec`, `--impure`, an
unpinned `fetchTarball`/`fetchGit`, `import <nixpkgs>`, `allowUnfree = true`,
`sandbox = false`, a secret-shaped value or an unapproved post-build hook. This
is defense in depth; the sandbox is the enforcement boundary.

### 6. Freezing and execution binding

The frozen `EnvironmentSpec` is immutable and identified by a SHA-256 hash over
canonical plan content, resolved pins, lock data, license inventory and policy
revision. The hash is included in the ActionSpec binding, the candidate record,
the verifier evidence and the delivery record. A candidate/check whose
environment hash differs from the frozen spec is rejected.

The verifier and check runner execute the frozen spec's environment, not a
global language image. A check may add a bounded working directory and command,
but it cannot change the environment, image, package set, network policy or
license decision. Missing environment tooling is `BLOCKED`/`NOT_TESTED`, never a
silent fallback.

An environment change is a stack change and follows the autonomy/risk policy in
ADR-0039. A later environment revision never rewrites an already frozen spec or
accepted evidence. An authenticated operator may explicitly recompile a plan or
environment with a bounded rationale only while execution is unbound; this
creates a new immutable revision, audits the prior revision/spec hash and
requires a fresh Lead review. A frozen environment is never mutated in place.

## Invariants

1. Every executable project has one frozen `EnvironmentSpec`; there is no
   implicit host or language-image fallback.
2. Project conventions and committed pins precede model proposals; no hardcoded
   language default exists.
3. The Lead approval is recorded but grants no authority; Core validation,
   pinning, licensing and freezing are authoritative.
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

## Consequences

- Projects can use any language/toolchain that Nix/devenv can package, without
  a Factory code change or a Rust/Node image assumption.
- The first plan for an existing project may require more discovery and a
  bounded owner decision when conventions conflict.
- The Factory check path must carry an environment spec and launch an
  environment-specific sandbox; the global verifier image becomes a Factory
  infrastructure detail, not the project environment.
- Nix/devenv evaluation is slower and larger than a single language image;
  caching and lock reuse are required for practicality but remain optional and
  replaceable.
- License/SBOM work becomes part of planning, not an afterthought.
- A generated environment can still be malicious or broken; the sandbox and
  fail-closed policy limit impact but do not certify isolation.

## Alternatives considered

1. **Per-project Dockerfile/Containerfile.** Rejected: it reintroduces
   language-specific base images, host build assumptions and drift, and gives no
   standard lock/license model.
2. **mise/asdf as the environment backend.** Rejected: host-dependent plugin
   managers and no hermetic Nix store; they remain convention evidence only.
3. **Nix without devenv.** Viable in principle, but devenv provides the
   language/service/task authoring layer and lock model the program needs. A
   future ADR may simplify; this ADR chooses devenv for the first implementation.
4. **Hardcoded language images with a "generic" escape hatch.** Rejected: that
   is the current bias and produces hidden defaults.
5. **Let the model choose and run an environment freely.** Rejected: model
   output is untrusted and cannot grant package, credential, license or
   execution authority.
6. **Require Devenv Cloud/Cachix for speed.** Rejected: it creates a mandatory
   paid/hosted dependency and a credential boundary unrelated to the product's
   local-first requirement.

## Verification

- Golden fixtures for Rust, Node/TypeScript, Python, Go, Java, .NET, mixed
  repositories, a `devenv.nix` project, a `flake.nix` project, a
  `.devcontainer`-only project, a `mise`/`.tool-versions` project and a
  greenfield project with no conventions.
- Deterministic tests for convention precedence/conflicts, no hardcoded language
  default, exact pins, Lead approval binding, Core validation failures, license
  allow/deny/unfree override, SBOM/attribution, forbidden Nix constructs,
  sandbox no-secret/no-network/resource-limit attestations, and hash binding to
  candidate/check/delivery.
- A real Nix/devenv build in the container-native topology with a bounded
  network fetch through the credential-free broker, followed by an offline
  check run and a restart/replay test. Mock-only evidence is insufficient.
- Office per-option browser acceptance for the environment review, license gate,
  unfree override, blocked/building/failed/passing states, keyboard and
  screen-reader labels at desktop, tablet and 320 px in EN and ES.
- A real multi-language campaign remains `NOT_TESTED` until an exact candidate,
  fresh authorization and independent acceptance exist.

## Supersession

None. A later ADR that changes the environment substrate, License Gate or
builder boundary must supersede this ADR explicitly.
