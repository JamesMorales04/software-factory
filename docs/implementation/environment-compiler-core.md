# Agnostic environment compiler — Core and Office slice

**Status:** Implemented WP1-A — 2026-11-10. This records the Core/domain/Office
half of the agnostic environment compiler. It is not acceptance evidence and it
does not claim a real Nix/devenv build. The Nix/devenv builder image and the
Node/Playwright runtime are the separate WP1-B deliverable.

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
| Persistence | `migrations/0076_environment_specs.sql`, `native/src/db/environment.rs` | Append-only plans/reviews; immutable specs, inventories, receipts, overrides and Campaign bindings. |

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
5. A `LEAD` review bound to that hash/revision is recorded. The Lead is a
   bounded planning role; the review grants no execution, credential or
   license authority.
6. Core revalidates, freezes one `EnvironmentSpec`, persists the license
   inventory and binds the exact spec hash to the ActionSpec and every check.
7. Campaign authorization inserts the immutable `campaign_environments` row and
   refuses a required ActionSpec without a frozen spec. The Worker assignment
   carries the same binding.

The existing overall plan review path records the Lead environment decision
automatically only when the deterministic plan-review policy is
`AUTO_APPROVED`; otherwise the Director/Request surface exposes the explicit
Lead action. In both cases Core validation is authoritative.

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
| `FACTORY_ENVIRONMENT_BUILDER_PLATFORM` | Builder platform; defaults to `x86_64-linux`. |
| `FACTORY_ENVIRONMENT_NIX_VERSION` | Pinned Nix version recorded in a frozen spec. |
| `FACTORY_ENVIRONMENT_DEVENV_VERSION` | Pinned devenv version recorded in a frozen spec. |

An absent image/digest is `ENVIRONMENT_BUILDER_UNAVAILABLE` at check selection;
it does not select a Rust or generic image.

## 5. Office

`EnvironmentPanel` is wired into the Request workspace, Director plan review and
Campaign overview. It shows:

- plan identity, revision, source and conventions hash;
- committed detection files, pins, precedence and conflicts;
- selected toolchains, packages, services, browsers, resources and network;
- license inventory, unfree state, owner override and blockers;
- builder image/digest/status and the latest receipt;
- the Lead review record and an exact-rationale Lead action while pending;
- the frozen spec hashes and the immutability rule;
- truthful missing/empty/blocked/error states.

Every displayed action calls an existing authenticated Core route:
`POST /agentic/requests/{id}/environment/review`. No button invents state or
acceptance.

## 6. Verification performed

- `cargo fmt --all`
- `cargo test -p factory-native --lib` — 872 passed, 4 ignored.
- `cargo test -p factory-native --bin factory-core` — 58 passed; Worker 28 passed; verifier 8 passed.
- `cargo clippy -p factory-native --lib --bins -- -D warnings` — clean.
- Office: `npm ci`, `npm run typecheck`, `npm run lint`,
  `npm test -- --run` — 47 files / 331 tests passed.
- `podman compose config --quiet` — exit 0.

**Not tested / not claimed:** a real Nix/devenv build, a real builder image, the
Node/Playwright runtime, live Podman check execution, real Chromium acceptance
of the new panel, and independent acceptance. `TRUSTED_LOCAL` remains not
isolated.
