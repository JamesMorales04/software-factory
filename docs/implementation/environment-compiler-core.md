# Agnostic environment compiler — Core and Office slice

**Status:** Implemented WP1-A — updated 2026-11-11. This records the
Core/domain/Office half of the agnostic environment compiler, including
greenfield stack selection and the Lead planning `EnvironmentProposal`. It is
not acceptance evidence and it does not claim a real Nix/devenv build. The
Nix/devenv builder image and the Node/Playwright runtime are the separate WP1-B
deliverable.

**Contracts:** [environment plan](../contracts/environment-plan.md),
[ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md),
[language-agnostic audit](language-agnostic-audit.md),
[Nix/devenv license findings](../evidence/nix-devenv-license.md).

**Security:** `TRUSTED_LOCAL`. Generated Nix/devenv files and the builder are
untrusted project input; a container is not an isolation certification.

## 1. Delivered records

| Record | Location | Notes |
| --- | --- | --- |
| `ProjectConventions` | `native/src/planning/environment.rs` | Reads only the exact Git commit through the shared `project_import` Git object readers. |
| `EnvironmentRequirements` / `EnvironmentProposal` | same | Explicit owner constraints and an untrusted model proposal. No language default. |
| `EnvironmentPlan` | same | Canonical `planSha256`, request/action/campaign and risk binding, detection snapshot, toolchains, packages, services, commands, files, Nix policy, resources, network, license inventory, builder descriptor, blockers, Lead review. |
| `EnvironmentPlanReview` | same | `LEAD` role, `lead:<lead-request>` principal, authenticated recorder, exact plan hash/revision. |
| `EnvironmentSpec` | same | Immutable frozen hash over the plan, review, locks, license inventory, SBOM and builder identity. |
| `EnvironmentBuildReceipt` / `EnvironmentBuilder` | same | Port implemented by WP1-B. Includes sandbox attestation and content-addressed output. |
| `CheckExecutionTarget` / `select_check_execution` | same | A check must carry the exact plan/spec binding; no implicit language image. |
| Persistence | `migrations/0076_environment_specs.sql`, `native/src/db/environment.rs` | Append-only plans/reviews; immutable specs, inventories, receipts and Campaign bindings. The licence-override table is reserved for a protected Owner action and has no write path in this slice. |

The `environment_request_bindings` view reports
`ENVIRONMENT_PLAN_REQUIRED` for existing requests with no real plan. It never
fabricates a plan or spec row.

## 2. Compile, approve and freeze

1. Core discovers committed conventions from the project base commit.
2. Equally-specific conflicting pins fail with
   `ENVIRONMENT_CONVENTIONS_CONFLICT`; a missing pin fails with
   `ENVIRONMENT_PIN_MISSING`; no project is defaulted to Rust/Node/Python.
3. The compiler validates the pinned nixpkgs revision and inputs, generated Nix
   constructs, argv commands, sandbox/network/resource policy, services and the
   license inventory. The default is `allowUnfree=false`.
4. The plan is persisted as `CORE_VALIDATED` with an exact `planSha256`.
5. Core asks the frozen Lead profile route to review the plan with a typed
   `EnvironmentLeadReviewModel` schema (`APPROVE`, `CHANGES_REQUESTED`,
   `REJECT`). The recorded `LEAD` review is bound to the exact plan hash,
   revision, request, action and risk class and carries the provider/model,
   reservation, request digest, usage and cost. The operator may trigger or
   retry the review but cannot choose its decision; an Owner plan approval is
   never mapped to a Lead environment approval.
6. A generated environment is resolved by the `EnvironmentBuilder` port before
   freeze. The resolver returns exact `flake.lock`/`devenv.lock` content, Nix
   and devenv versions and a digest-pinned builder identity. A missing builder
   or failed resolution blocks with `ENVIRONMENT_BUILDER_UNAVAILABLE` or the
   typed builder error. The default production builder is explicitly
   unavailable; WP1-B supplies the real builder.
7. Core revalidates, freezes one `EnvironmentSpec`, persists the license
   inventory and binds the exact spec hash to the ActionSpec and every check.
8. Campaign authorization inserts the immutable `campaign_environments` row and
   refuses a required ActionSpec without a frozen spec. The Worker assignment
   and verifier inspection carry the exact plan/spec/receipt; every bound check
   is selected with `select_check_execution` and executed with the selected
   digest-pinned builder image. There is no global language image fallback.

The automatic planning path may invoke the Lead model when the overall plan
review is `AUTO_APPROVED`; if the model, inference authorization or frozen route
is unavailable, the environment plan stays `CORE_VALIDATED` and execution is
blocked. The Director/Request surface exposes a "Request Lead environment
review" action that triggers the same model call. In every path Core validation
remains authoritative.

## 2.1 Greenfield stack selection and the Lead environment proposal

A greenfield request has no committed language manifest. Core does not choose a
language, image or package set from the Factory repository or from a
per-language table. Instead:

1. The semantic triage model may propose a concrete `StackProfile`. Prompting
   requires one for a greenfield `CHANGE`; an empty or unknown stack remains
   fail-closed.
2. If the triage/detected stack is still empty, `compile_stack_decision` makes
   one bounded Lead-route model call with purpose `lead_stack_selection`. The
   prompt supplies the objective, project config, allowed paths, authorized
   capabilities and the exact deterministic `selection_options` catalog. The
   typed output is `StackSelectionModel` (`optionId`, `language`, `framework`,
   `rationale`, optional `environmentProposal`).
3. Core validates the returned id/language/framework against the catalog and
   the option's `requiredCapability` against the project capabilities. Unknown,
   empty, mismatched or unauthorized selections fail closed with
   `STACK_SELECTION_INVALID`; route/inference unavailability is
   `STACK_SELECTION_MODEL_UNAVAILABLE`; transport, truncation or schema failure
   is `STACK_SELECTION_MODEL_FAILED`. A successful decision is recorded as
   `StackSource::MODEL` with every considered option and a model-call event; the
   model never grants capabilities or paths.
4. The Lead planning model (`LeadPlanProposal.environmentProposal`) may carry a
   bounded, untrusted `EnvironmentProposal`: an exact nixpkgs pin (or omitted to
   use the builder default), toolchains, nixpkgs packages with SPDX licences,
   and argv commands. A greenfield assessment prompt requires one; the
   stack-selection call may also return a fallback proposal when an older
   recorded Lead plan has none.
5. A model proposal may **not** supply Nix/devenv files. `EnvironmentProposal`
   with a non-empty `files` map is rejected with
   `ENVIRONMENT_SANDBOX_DENIED`; Core generates `flake.nix` from the validated
   toolchains/packages/commands. Committed project files remain supported, and
   the generated-file validator rejects every `unfree` policy identifier,
   dynamic config construction (`listToAttrs`, `mapAttrs`, `getAttr`,
   `fromJSON`, `attrNames`, `functionArgs`), dynamic attrset keys inside
   `config`, impure evaluation, unpinned fetches, secrets and unsafe
   sandbox/network constructs. Every Nix file is parsed with
   `nix-instantiate --parse`; the normalised AST is checked for dynamic keys in
   every `config =` expression, with no fixed lookahead window or stop at the
   first semicolon. A `config` value must be a literal attrset or a merge of
   literal attrsets; let-bound aliases, function calls and other non-literal
   config expressions fail closed with a typed `ENVIRONMENT_SANDBOX_DENIED`.
   Ordinary committed flake interpolation such as `devShells.${system}` remains
   valid, and a literal `config = { allowUnfree = false; }` is accepted (while
   `allowUnfree = true` remains rejected by the unfree policy). The production
   `RequestEnvironmentConstraints` does not expose a `files` field.
6. Precedence remains: explicit request `environmentConstraints`; committed
   conventions; CI/version evidence; the model proposal, which only fills
   missing toolchains, packages and commands. `EnvironmentProposal::validate`
   rejects unsafe `allowUnfree`, unknown licences, unpinned/unknown package
   sources, mismatched `nixpkgs:<attr>` suffixes, unsafe generated files and
   out-of-bound commands before any field is merged.
7. `compile_environment_plan` re-pins, licenses, validates the sandbox/resources
   and records package claims. Before freeze, Core requires builder resolution
   for every generated plan and every plan with selected packages. The builder
   evaluates `meta.version`/`meta.license`/`meta.unfree` for each exact pinned
   nixpkgs attribute in a bounded pure `nix eval` against the plan's pinned
   nixpkgs (so Devenv-only projects work); `EnvironmentResolution.packages`
   carries the observed metadata. Core requires exact coverage and rejects a
   missing, mismatched or unknown version, licence or unfree state.
8. Resolution also realises the exact environment and inspects the **actual
   closure**, including transitive packages. Generated environments audit the
   selected package outputs; committed environments audit the realised package
   profile (`.devenv/profile` or the flake profile), excluding only that exact
   profile wrapper. A deriver-backed helper script is audited like any package;
   only the exact reviewed deriver-less `strip.sh` store path is treated as a
   non-package helper, and any other deriver-less unrecognized path fails
   closed. `EnvironmentResolution.closureLicenses`
   records builder-observed closure licence metadata; any unfree, denied or
   unknown closure package fails closed. Outputs of an observed package are
   mapped by their exact output path or shared deriver. The only non-nixpkgs
   exception is the internal `xgcc` bootstrap output, bound to its exact pinned
   nixpkgs revision, derivation path and output path; a name-only match is not
   accepted. Substrate paths (`nix`, `devenv`, `nixpkgs`, `git-hooks`) are
   accepted only by exact reviewed `(pin, deriver, output)` identities; a
   fabricated substrate-named path fails closed. A pin change also fails closed
   until the identities are reviewed. Any other unobserved closure path fails
   closed.
   This check is independent of the plan copy and of Nix text, so a constructed
   `allowUnfreePredicate` cannot hide an unfree dependency. The builder also
   checks every plan command's executable with `command -v` inside the realised
   devShell and fails closed if it is missing.
9. The model-selected stack profile is the input to the architecture, skill and
   Definition of Done artifacts. The raw triage profile remains audit context;
   the architecture artifact records the selected stack explicitly. The Lead
   environment review is still required before freeze.

## 3. Typed failures

`EnvironmentBlockerCode` is the stable API enum. The required WP1-A codes are
present and tested:

- `ENVIRONMENT_SPEC_INVALID`
- `ENVIRONMENT_BUILD_FAILED`
- `ENVIRONMENT_BUILDER_UNAVAILABLE`
- `ENVIRONMENT_LICENSE_BLOCKED`
- `ENVIRONMENT_PLAN_REQUIRED`

The full contract set (conventions conflict/stale, pin missing, secret,
sandbox, network, resource, hash mismatch, not frozen, superseded and approval
codes) is also represented.

## 4. Builder interface for WP1-B

WP1-B supplies the containerized Nix/devenv builder and Node/Playwright runtime.
It must implement `EnvironmentBuilder`:

```text
descriptor() -> EnvironmentBuilderDescriptor
resolve(plan) -> EnvironmentResolution
build(spec, plan) -> EnvironmentBuildReceipt
```

The descriptor carries the image, `sha256:` digest, platform
(`x86_64-linux`), policy revision and availability. The receipt must bind the
exact `specSha256`, the same builder image/digest and platform, a successful
status, a content-addressed output hash and a complete sandbox attestation
(non-root, no secrets, no host network/socket/mount, `sandbox=true`,
`impure=false`, bounded resources). A missing or failed receipt is a typed
blocker, never a fallback.

Factory configuration:

| Variable | Meaning |
| --- | --- |
| `FACTORY_ENVIRONMENT_BUILDER_IMAGE` | Builder image reference (no tag credentials). |
| `FACTORY_ENVIRONMENT_BUILDER_DIGEST` | `sha256:` image digest. |
| `FACTORY_ENVIRONMENT_BUILDER_DIGEST_FILE` | Compose: `/bundles/environment-builder.digest`; used when `_DIGEST` is absent. |
| `FACTORY_ENVIRONMENT_BUILDER_PLATFORM` | Builder platform; defaults to `x86_64-linux`. |
| `FACTORY_RUNTIME_BROKER_URL` | Runtime broker origin used by the Core builder client. |
| `FACTORY_RUNTIME_BROKER_TOKEN_FILE` | Distinct environment-owner broker token (`/config/runtime-broker-environment.token`). |
| `FACTORY_ENVIRONMENT_NIX_VERSION` | Pinned Nix version recorded in a frozen spec. |
| `FACTORY_ENVIRONMENT_DEVENV_VERSION` | Pinned devenv version recorded in a frozen spec. |
| `FACTORY_ENVIRONMENT_DEFAULT_NIXPKGS` | Deliberate override for the compiled default pin `github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d` (WP1-B builder `NIXPKGS_REV`). The constant is used when neither the request nor committed conventions pin one; it is infrastructure policy, never a language default. |

Production intake requests may carry `environmentConstraints` with `nixpkgs`,
`toolchains`, `packages`, `services`, `browsers` and `resources`. Committed
conventions take precedence; a generated environment uses the builder's
documented default pin only when the request is silent, and otherwise exposes
`ENVIRONMENT_PIN_MISSING` in Core and Office.

An absent image/digest is `ENVIRONMENT_BUILDER_UNAVAILABLE` at resolution and
check selection; it does not select a Rust or generic image. A generated
environment without builder resolution cannot freeze. WP1-C installs the
`RuntimeBrokerEnvironmentBuilder` when the broker health projection reports the
exact configured image/digest as `READY`; otherwise the descriptor stays
truthfully `UNAVAILABLE`. See the
[environment runtime broker integration](environment-runtime-broker.md).

## 4.1 Owner unfree-licence override

A real authenticated Owner-only unfree-licence override API is deferred. The
compiler rejects `allowUnfree`, unknown/denied licences and any unfree package
without an exact owner override record. The previous unauthenticated database
write path was removed; there is no HTTP or database route that can create an
override with an arbitrary principal. The reserved Owner action must be added
under the protected approval path before any unfree package can be admitted.

## 5. Office

`EnvironmentPanel` is wired into the Request workspace, Director plan review and
Campaign overview. It shows:

- plan identity, revision, source and conventions hash;
- committed detection files, pins, precedence and conflicts;
- selected toolchains, packages, services, browsers, resources and network;
- license inventory, unfree state, owner override and blockers;
- builder image/digest/status and the latest receipt;
- the Lead review record, model-call provenance and a trigger/retry action while
  pending. The operator does not choose the decision;
- the frozen spec hashes and the immutability rule;
- truthful missing/empty/blocked/error states.

Every displayed action calls an existing authenticated Core route:
`POST /agentic/requests/{id}/environment/review`. No button invents state or
acceptance.

Greenfield stack selection adds no new Office control. The existing Request,
Director and Campaign environment panel already renders the resulting `MODEL`
stack decision's toolchains, packages, commands, license inventory and the
truthful blocked/validated/frozen states. An operator never chooses or forges
the stack or environment decision.

## 5.1 Test-only fixture boundary

Production authorization has no fake-binding path. Library unit tests use a
plain `#[cfg(test)]` fallback only: when a test ActionSpec has an exact
environment identity but no `environment_specs` row, authorization inserts a
test Campaign binding and the assignment loader may omit the execution receipt.
That fallback is not compiled for `feature = "test-support"`, binaries or
production builds; binary test harnesses seed real `environment_plans`,
reviews, `environment_specs` and build receipts into SQLite through a
`#[cfg(test)]` helper in the Core test module. The Core HTTP tests prove the
strict production path: missing plan/spec returns `ENVIRONMENT_PLAN_REQUIRED`
before any assignment.

## 6. Verification performed

- `cargo fmt --all -- --check` — clean.
- `cargo test -p factory-native --lib` — 964 passed, 4 ignored. Includes the
  greenfield Node/Rust environment proposal, explicit-constraint precedence,
  unknown licence/pin/source rejection, model-file rejection, every unfree
  policy form, independently observed package metadata mismatch, model stack
  selection, schema round-trip, selected-stack artifact flow and multi-line
  architecture-context tests.
- `cargo test -p factory-native --bin factory-core` — 65 passed.
- `cargo test -p factory-native --bin factory-core --features test-support` —
  84 passed, including the model-driven stack selection, invalid-selection,
  typed model-failure, generated environment fallback, Lead approval,
  operator-forgery, Owner-approval-only, replay and stale-hash tests.
- `cargo test -p factory-native --bin factory-worker` — 33 passed.
- `cargo test -p factory-native --bin factory-verifier` — 11 passed.
- `cargo test -p factory-native --bin factory-runtime-broker` — 71 passed.
- `cargo test -p factory-native --bin factory-supervisor` — 11 passed.
- `python3 -W error::ResourceWarning -m unittest discover -s
  deploy/environment-builder/tests` — 80 tests OK.
- `scripts/environment-builder-real-resolve-smoke.sh` — real Nix resolution
  against the pinned nixpkgs passed: `real-nix-resolve: PASS packages=1
  closure=24`.
- `scripts/environment-builder-real-committed-devenv-smoke.sh` — real committed
  Devenv profile resolution passed: `real-committed-devenv: PASS closure=14`.
- `scripts/environment-builder-real-config-bypass-smoke.sh` — both the
  long-whitespace merged config and let-bound alias cases failed with
  `ENVIRONMENT_SANDBOX_DENIED` before realisation.
- `cargo clippy -p factory-native --all-targets --all-features -- -D warnings`
  — clean.
- Office: `npm ci`, `npm run typecheck`, `npm run lint`,
  `npm test -- --run` — 47 files / 333 tests passed.
- `podman compose config --quiet` — exit 0.
- `docs/product/office-parity.json` parses as valid JSON and includes the
  environment states and endpoints. `docs/product/capabilities.yaml` and the
  generated roadmap include the environment module, migration 0076, panel and
  truthful limitations.
- `factory-architecture-check --root .` — PASS (registry and roadmap current).
- `factory-repo-hygiene --mode audit --root .` — two pre-existing Go-toolchain
  findings in `planning/dod.rs` and `planning/skills/catalog.rs`; this candidate
  does not add a repository-hygiene finding.

### 6.1 Real Nix resolution observation (2026-11-11)

`scripts/environment-builder-real-resolve-smoke.sh` builds a minimal generated
environment (`flake.nix` with `pkgs.cargo`, exact nixpkgs pin
`ac62194c3917d5f474c1a844b6fd6da2db95077d`) and runs the current working-tree
builder code inside the existing
`localhost/software-factory-environment-builder:dev` image. The image provides
Nix 2.28.3 and devenv 1.11.2; no host toolchain is installed.

Observed result: exit 0, `real-nix-resolve: PASS packages=1 closure=24`.
`flake.lock` was generated, the builder observed `cargo 1.86.0` with
`MIT AND Apache-2.0`, and the closure audit independently observed 24
transitive packages (including bash, glibc, gcc, llvm/LLVM-exception, curl,
openssl, zlib, libffi and the reviewed internal `xgcc` bootstrap output) with no
unfree, denied or unknown licence. The generated devShell wrapper was not
required to be a nixpkgs package attribute.

A committed Devenv profile was also resolved with the current code against the
same image: the profile closure audit observed 14 package entries (including
`hello`, `glibc`, `bash`, `pkg-config`, `tzdata`, `process-compose` and
`libunistring`) with no unfree/denied/unknown licence. Devenv's `strip.sh`
helper script and the exact profile wrapper are excluded as non-package
wrappers; a fabricated substrate-named path still fails closed.

A real committed flake containing the reviewer's long-whitespace/nested-`let`
merged config bypass was also submitted to the builder. `nix-instantiate
--parse` normalised it and the AST check rejected it with
`ENVIRONMENT_SANDBOX_DENIED` before any Nix realisation.

The smoke uses `--privileged` because this developer host does not grant the
nested kernel namespaces Nix sandboxing needs; that is a harness detail, not the
product broker policy. This is implementer evidence, not independent acceptance
or a claim that the full Compose builder/broker topology or real Campaign
execution passed.

**Not tested / not claimed:** a real Nix/devenv **build** and builder receipt
(the resolution smoke above is real Nix lock/develop/eval evidence), the real
devShell `command -v` check in a built environment, the Node/Playwright runtime,
live Podman check execution, real Chromium acceptance of the new panel, a live
provider stack-selection call for the calculator request, a live re-plan of that
request, and independent acceptance. `TRUSTED_LOCAL` remains not isolated.
