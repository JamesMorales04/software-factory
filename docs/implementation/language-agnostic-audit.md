# Language-agnostic execution audit

**Status:** WP0 audit — 2026-11-10. Baseline
`ba2dda9` on `docs/delivery-environment-contracts`. This document inventories
language-specific assumptions in the current execution, planning, verifier,
dependency and Office paths. It does not change code, schema, parity states or
capabilities.

> **Later implementation note (2026-11-11):** the greenfield slice removes the
> hidden `generic-toolchain` executable default from the Lead prompt (ENV-05),
> adds a bounded validated `MODEL` stack decision when triage/detection are
> empty, and lets the Lead planning proposal supply a concrete
> `EnvironmentProposal` without overriding explicit constraints. Model proposals
> cannot supply Nix files, and selected package licence/version/source claims are
> checked against builder-observed pinned nixpkgs metadata before freeze. The
> selected stack profile now feeds architecture, skills and the Definition of
> Done. The remaining findings (global verifier image, dependency adapters, DoD
> language fallbacks, Office profile vocabulary) are unchanged and still require
> their named work packages.

**Decisions:** [ADR-0040](../architecture/decisions/ADR-0040-risk-tiered-delivery-and-auto-merge.md),
[ADR-0041](../architecture/decisions/ADR-0041-agnostic-environment-compiler.md),
[ADR-0042](../architecture/decisions/ADR-0042-issue-linked-delivery-lifecycle.md).

**Contracts:** [environment plan](../contracts/environment-plan.md),
[delivery policy](../contracts/delivery-policy.md),
[issue lifecycle](../contracts/issue-lifecycle.md).

## 1. Method

I read the current `native/src/bin/core.rs` planning/delivery handlers,
`native/src/db/quality.rs`, the Worker salvage/finalize path, the dependency and
project-discovery modules, `deploy/Containerfile`, `compose.yaml`,
`docs/product/office-parity.json` and the relevant engineering/ADR documents.
Dispositions below are about **the current tree**, not a claim that a future
work package is implemented.

Disposition vocabulary:

- **REPLACE** — the current mechanism must stop selecting project behavior.
- **GENERALIZE** — keep the mechanism but make its input/output environment- and
  project-driven.
- **BIND** — keep the mechanism and add an exact environment/candidate binding.
- **ADD** — a missing capability/record must be introduced.
- **RETAIN (Factory-only)** — valid for building Factory itself, but must not
  leak into user-project execution.
- **NO CODE CHANGE** — no language assumption found in the active path.

WP ownership is defined in
[delivery-environment program](delivery-environment-program.md). No WP below is
implemented by WP0.

## 2. Executive summary

The active execution path is not language-agnostic. It has a **Rust default at
the image, dependency, planning and check layers**, and a **Node assumption in
the quality-profile vocabulary**. The strongest biases are:

1. the verifier executes every project check in one `FACTORY_VERIFIER_IMAGE`
   pinned to `rust:1.90.0-bookworm`;
2. the runtime broker image allowlist is the same Rust image;
3. dependency plans recognise `DependencyKind::Rust` and require `Cargo.lock`;
4. the planning stack decision only auto-builds a dependency plan for Rust;
5. `SpecCheck` has no environment binding, so a check cannot say which
   toolchain/image/store it needs;
6. `generic-toolchain` is described to the model as `cargo`, `rustc`, `python3`
   and coreutils, and the verifier is described as one generic image;
7. project discovery does not read `flake.nix`, `devenv.nix`, `.devcontainer`,
   `mise.toml`, `.tool-versions` or CI pins;
8. the Office/quality vocabulary uses `node-basic`, `node-browser` and a
   `Generic` profile rather than an environment-derived capability.

These are not cosmetic. A Python, Go, Java, .NET, Ruby or mixed project either
cannot run or must be manually contorted into a Rust image. The environment
compiler must remove the hidden default and make the exact environment an
explicit, frozen planning artifact.

## 3. Findings

### 3.1 Check and verifier path

| ID | Current assumption | Evidence | Impact | Disposition | Owner |
| --- | --- | --- | --- | --- | --- |
| ENV-01 | Every check runs in one global verifier image selected by `FACTORY_VERIFIER_IMAGE`. | `native/src/bin/verifier.rs:332`; `compose.yaml:413` | A project cannot select its own toolchain/store; the image is Rust. | **REPLACE**: verifier resolves and launches the candidate's frozen `EnvironmentSpec`. | WP-E2 |
| ENV-02 | `SpecCheck` has `id/kind/description/command/expectExit/timeout` but no environment, image, workdir or required capability. | `native/src/lead.rs:155` | A check cannot be bound to a project environment; all checks inherit one image. | **BIND**: add an environment/spec reference (and bounded workdir/capability where required). | WP-E1, WP-E2 |
| ENV-03 | Compose sets the verifier image to `docker.io/library/rust:1.90.0-bookworm`. | `compose.yaml:413` | Hidden Rust default for all projects. | **REPLACE** project execution; retain a pinned Factory verifier runtime only for the verifier process itself. | WP-E2 |
| ENV-04 | Runtime broker image allowlist is `docker.io/library/rust:1.90.0-bookworm`. | `compose.yaml:222`; `native/src/bin/runtime_broker.rs:363-365` | Only a Rust image is pre-approved for execution/checks. | **REPLACE/GENERALIZE**: allowlist resolved environment images by digest, not one language image. | WP-E2 |
| ENV-05 | The Lead prompt says `generic-toolchain` provides `cargo`, `rustc`, `python3`, `sh`, coreutils and that the verifier runs in "the frozen generic toolchain image". | `native/src/lead_model.rs:132-135` | The model is taught a Rust/Python default and plans checks around it. | **REPLACE**: capabilities and executable availability come from the frozen environment spec. | WP-E4 |
| ENV-06 | `Capability::GenericToolchain` and `NodeToolchain` are closed enum values; there is no environment capability. | `native/src/lead.rs:98-103` | Capability matching cannot express an arbitrary toolchain. | **GENERALIZE**: capabilities are project/environment-derived; keep closed policy values where they express authorization, not language. | WP-E4 |
| ENV-07 | `QualityProfile` is `NodeBasic | NodeBrowser | Generic`; required check kinds are fixed by that vocabulary. | `native/src/lead.rs:107-136` | Non-Node projects are forced into a Node or generic profile; missing profiles become a language bias. | **GENERALIZE**: profile requirements derive from the environment/DoD; preserve check-kind floors. | WP-E4 |

### 3.2 Dependency and supply-chain path

| ID | Current assumption | Evidence | Impact | Disposition | Owner |
| --- | --- | --- | --- | --- | --- |
| ENV-08 | `DependencyKind` is `Rust | Node | Python | Other`, but the active builder is Rust-specific. | `native/src/dependencies.rs:71`; `:678-679`; `:714-715` | Other ecosystems have no equal execution path. | **GENERALIZE**: environment-specific dependency adapters; `Other` must not mean "unsupported by default". | WP-E3 |
| ENV-09 | The dependency plan requires `Cargo.toml`/`Cargo.lock` paths by default. | `native/src/dependencies.rs:678-679`, `:714-715` | Non-Cargo projects cannot use the same supply-chain evidence. | **GENERALIZE**: manifest/lock set comes from the environment spec. | WP-E3 |
| ENV-10 | Compose dependency registries are only crates.io indexes; egress allowlist is crates.io/GitHub. | `compose.yaml:232-237` | Non-Rust package acquisition is blocked or ad hoc. | **GENERALIZE**: broker policy is per environment/ecosystem; no credential; bounded allowlist. | WP-E3 |
| ENV-11 | The dependency image is a Rust toolchain image with `factory-dependency-acquire`. | `deploy/Containerfile:193-211` | Dependency acquisition assumes Cargo. | **GENERALIZE**: acquisition image carries the environment's pinned fetchers/tools, not a Rust default. | WP-E3 |
| ENV-12 | `DependencyPlan`/`DependencyStrategy` are useful generic shapes but currently wired to Rust vendor/cache by the planner. | `native/src/planning/mod.rs:262`; `native/src/planning/stack.rs:157-167`, `:198-208` | The plan shape is reusable; the selection is biased. | **GENERALIZE**: select strategy from environment/conventions, not `language == rust`. | WP-E4, WP-E3 |

### 3.3 Planning, stack and project discovery

| ID | Current assumption | Evidence | Impact | Disposition | Owner |
| --- | --- | --- | --- | --- | --- |
| ENV-13 | `compile_stack_decision` computes `Cargo.lock` and forces VendorCache/BrokeredFetch around Rust. | `native/src/bin/core.rs:11086-11126`, `:11159-11167` | Planning cannot produce a non-Rust dependency plan. | **REPLACE** with environment-plan-derived dependency binding. | WP-E4 |
| ENV-14 | `persist_planning_plan` compiles stack/architecture/docs/skills/DoD but has no environment artifact. | `native/src/bin/core.rs:11467-11656` | A plan can be frozen without a testable environment; checks later inherit the global image. | **ADD** an `ENVIRONMENT` planning artifact and Lead review before freeze. | WP-E1, WP-E4 |
| ENV-15 | `StackProfile` detection inspects `Cargo.toml`/`package.json` first and infers languages/frameworks from them. | `native/src/agentic.rs:445-500` | Non-Rust/Node stacks are under-detected; conventions like flake/devenv/mise are invisible. | **GENERALIZE**: conventions-first discovery; manifest inference is one input. | WP-E1, WP-E4 |
| ENV-16 | `selection_options` hardcodes `typescript-react`, `rust-cli`, `rust-service`, `python-service`. | `native/src/planning/stack.rs:38-77` | Greenfield choices are a fixed Rust/TS/Python catalog, not evidence-based options. | **REPLACE/GENERALIZE**: options derive from objective + environment + capability evidence; no hidden default. | WP-E4 |
| ENV-17 | `project_discovery` scans a fixed manifest set, not `flake.nix`/`devenv.nix`/`.devcontainer`/`mise.toml`/`.tool-versions`/CI. | `native/src/project_import.rs:619-780`, `:984-1030` | Discovery misses the exact conventions the environment compiler needs. | **GENERALIZE**: add convention files and command/CI evidence; keep bounded. | WP-E1 |
| ENV-18 | `explorer_inventory` reads only `Cargo.toml` and `package.json`. | `native/src/bin/core.rs:14200-14248` | Explorer context omits other ecosystems and environment files. | **GENERALIZE**: inventory from conventions/environment, bounded. | WP-E1, WP-E4 |
| ENV-19 | Knowledge parsers are Cargo/package.json-specific. | `native/src/knowledge.rs` `parse_cargo_manifest`/`parse_package_json` | Library/dependency context is biased. | **GENERALIZE** behind an ecosystem adapter; no unbounded parsing. | WP-E3, WP-E4 |
| ENV-20 | The Definition of Done hardcodes Rust/TypeScript/Python/Go checks and falls back to `test -s README.md` when no match. | `native/src/planning/dod.rs:55-108` | Other ecosystems get a non-test fallback; the check set is selected by hardcoded language names. | **REPLACE**: DoD check commands come from the frozen `EnvironmentSpec`; fallback is an explicit blocker, not a fake test. | WP-E4 |
| ENV-21 | The curated skill catalog contains Cargo/Go/npm/pytest command examples. | `native/src/planning/skills/catalog.rs` (many entries) | This is context, not execution authority, but it can bias the planner. | **RETAIN as context** with source/version metadata; the environment spec, not the catalog, selects commands. | WP-E4 |
| ENV-22 | Root `rust-toolchain.toml`, `mise.toml` and `.devcontainer` describe Factory's own development, not a user-project default. | repository root; `deploy/Containerfile`; `compose.dev.yaml` | A naive compiler could copy the Factory pins into every project. | **RETAIN (Factory-only)**: never use them as user defaults; only as evidence when the user project itself commits them. | WP-E1 |

### 3.4 Worker, candidate and delivery path

| ID | Current assumption | Evidence | Impact | Disposition | Owner |
| --- | --- | --- | --- | --- | --- |
| DEL-01 | Worker stagnation/salvage can finalize a candidate after frozen checks; it is a candidate-submission path, not delivery. | `native/src/bin/worker.rs:2011-2280`, `:6910` regression test | Salvage must not be mistaken for merge/delivery authority. | **BIND**: candidate final source + delivery attempt; salvage never writes a branch/PR/merge. | WP5 / WP3 (table/WP-D2, environment/WP-E2) |
| DEL-02 | The merge handler is operator-confirmed and checks Gate/head/PR/checks, but has no risk-tier policy, durable attempt, idempotency, incident or post-merge check. | `native/src/bin/core.rs:5250-5405`; `:4716-4760` | R0/R1 cannot be safely automated; retries lack durable intent. | **EXTEND**: policy/attempt/incident/issue bindings (ADR-0040, 0042). | WP-D1, WP-D2 |
| DEL-03 | `db/quality.rs` stores candidates/receipts/gates but no delivery/issue-link records. | `native/src/db/quality.rs` (no delivery terms) | Delivery state is not durable or queryable. | **ADD** migrations/tables reserved as 0073/0074/0075. | WP-D1, WP-D2 |
| DEL-04 | Gate `ACCEPTED` is already checked before the existing merge, but there is no separate delivery decision. | `native/src/bin/core.rs:5304-5312` | The handler can drift into treating Gate as delivery authority. | **BIND**: separate `DeliveryDecision`; Gate remains necessary, not sufficient. | WP-D1 |
| ISS-01 | GitHub issue integration is read-only; intake creates a proposal with `autonomous_start=false`; no PR comment/close. | `docs/implementation/github-issues-integration-2026-10-28.md`; `docs/implementation/issue-intake-policy-2026-10-28.md`; `native/src/issue_intake.rs:820-860` | No issue-linked delivery lifecycle or closure evidence. | **ADD** immutable links, states, comment/close receipts (ADR-0042). | WP-I1 |

### 3.5 Build, development and Factory test-tooling assumptions

| ID | Current assumption | Evidence | Impact | Disposition | Owner |
| --- | --- | --- | --- | --- | --- |
| DEV-01 | Factory's build image is `rust:1.90.0`; Office builder is Node; the runtime is Debian. | `deploy/Containerfile:8-11`, `:45` | Correct for building Factory; wrong if reused as a project default. | **RETAIN (Factory-only)**; do not expose as a project environment. | WP-E2 |
| DEV-02 | The supervisor runtime installs `python3`; this is a runtime-language dependency that a project check could accidentally rely on. | `deploy/Containerfile:95-103` | A project check may silently use Python from the Factory runtime instead of its frozen environment. | **REPLACE** project checks with the environment builder/verifier; retain only Factory-internal tooling. | WP-E2 |
| DEV-03 | Compose's OpenSpec image is Node-based. | `compose.yaml:542` | It is a Factory tooling image, not a user project runtime. | **RETAIN (Factory-only)**; do not infer Node for user projects. | WP-E2 |
| DEV-04 | Provider image contains OpenCode/Codex and Node. | `deploy/Containerfile:213-241` | Provider runtime requirement; not a project environment. | **RETAIN (Factory-only)**; keep separate from execution. | WP-E2 |
| DEV-05 | The Factory browser runner uses a pinned Node runtime (`FACTORY_NODE_BINARY`) and the Node/`playwright-core` `browser-check-runner.mjs`; the docs pin Node 22.23.2 and describe the harness as the deterministic/independent browser-check surface. | `native/src/browser.rs:780-868`; `docs/engineering/browser-automation.md:12-22`; `docs/engineering/factory-browser-cli.md`; `office/e2e-browser/browser-check-runner.mjs` | **RETAIN (Factory test infrastructure)**; it is not a project environment default. A project that uses Node for its own browser tests still gets Node from its `EnvironmentSpec`. | WP-E2 (WP-T1 evidence) |

### 3.6 Additional active-path findings

These paths were not in the first audit pass. Each is classified explicitly as
production project-toolchain bias, Factory-only, or fixture/test-only. A
fixture-only image is not evidence that production execution is Rust-only, and
it is not a project environment.

| ID | Current assumption | Evidence | Class | Disposition | Owner |
| --- | --- | --- | --- | --- | --- |
| ENV-23 | Dependency validation requires the lockfile path to end with `Cargo.lock`; acquisition defaults to `Cargo.lock`/`cargo fetch`/`cargo vendor` and prepares `CARGO_HOME`. | `native/src/dependency_service.rs:384-385`, `:787-797`; `native/src/bin/dependency_acquire.rs:77-89`, `:164` | **Project-toolchain bias (production dependency path)** | **GENERALIZE**: environment-selected lockfile/ecosystem adapter; broker policy stays. | WP-E3 |
| ENV-24 | Every Podman/broker execution container receives a `/cargo-target` tmpfs and `CARGO_TARGET_DIR`, regardless of project language. | `native/src/execution.rs:466-477`; `native/src/bin/runtime_broker.rs:2140-2154` | **Project-toolchain bias (production execution setup)**; harmless for a non-Cargo command but still a hidden Rust assumption | **BIND**: target/cache directories and env names come from the frozen `EnvironmentSpec`. | WP-E2 |
| ENV-25 | The Supervisor's production execution image allowlist is the Rust image. | `compose.yaml:334`; `native/src/bin/supervisor.rs:346-348`, `:623-626` | **Project-toolchain bias (production execution)** | **GENERALIZE**: allowlist resolved from the frozen environment image digest; retain a pinned Factory verifier/runtime image separately. | WP-E2 |
| ENV-26 | The Worker fixture image is the Rust image. | `compose.yaml:374`; `native/src/bin/worker.rs:151` | **Fixture/test-only**, not live project execution | **RETAIN as fixture infrastructure**, label it clearly and never use it to infer a project toolchain. | WP-T1 (WP-E2 review) |
| ENV-27 | The Office Project form offers only `generic`, `node-basic` and `node-browser` quality profiles. | `office/src/views/Projects.tsx:823-829` | **Project-toolchain bias (production Office surface)** | **GENERALIZE**: show the Core-owned environment/profile choices; do not invent a profile or claim environment support. | WP-O1 (depends WP-E4) |
| ENV-28 | Semantic review context ranks `lib.rs`/`main.rs` and `Cargo.toml`/`package.json` ahead of other files. | `native/src/semantic_review.rs:321-327` | **Project-toolchain bias (production review context)** | **REPLACE**: rank by changed/impacted paths and environment conventions, not Rust/Node filenames. | WP-E4 |

### 3.7 Salvage and final source

The Worker salvage path can finalize a stagnated Implementer only when it has
already written a changed workspace and every remaining frozen check passes
(`native/src/bin/worker.rs:2226-2286`). That is a **candidate submission**, not a
merge, delivery or issue close.

Required policy:

1. The exact final source used by salvage is recorded in
   `0075_candidate_final_source.sql`: candidate commit/tree/content digest,
   frozen environment hash, salvage checkpoint and check receipts, and the
   normal verifier/reviewer/Gate lineage. A missing final-source record is
   `FINAL_SOURCE_REQUIRED`/`NOT_TESTED`, not an accepted candidate.
2. **R0 salvage** is visible through the final-source record and is eligible for
   the ordinary verifier/reviewer/Gate and the ordinary R0 delivery policy. It
   receives no special acceptance claim.
3. **R1/R2/R3 salvage** requires an authenticated human review of the
   final-source and salvage evidence, even if the Gate later accepts it. It is
   **never auto-merged**; delivery is manual only. A model cannot self-approve
   the salvage.
4. Salvage never writes a branch, PR, merge, tag or issue close. Those remain
   Core delivery/issue operations under ADR-0040/ADR-0042.

**Owner:** WP5 / WP3. WP5 owns the Worker salvage/finalization and
provider-failure boundary; WP3 owns the planning/risk/Gate integration and the
final-source acceptance rule. WP-D2 implements the reserved `0075` table and
delivery binding with WP-E2's environment hash.

## 4. Cross-cutting conclusions

1. **No hidden Rust/Node bias is acceptable in the target path.** After WP-E2
   and WP-E4, project execution must select the frozen `EnvironmentSpec`, not an
   image by language, a `generic-toolchain` constant or the Factory repository's
   own pins.
2. **The Factory's own Rust/Node build and browser runner remain Factory
   tooling.** `deploy/Containerfile`, `.devcontainer`, `rust-toolchain.toml`,
   `mise.toml` and the Node/Playwright browser runner are Factory development
   and test facts. They must be labelled as such and never copied into a user
   project's environment.
3. **A check is not portable until it has an environment.** `SpecCheck` alone is
   insufficient; the frozen environment hash must be part of the action spec,
   check evidence, candidate and delivery.
4. **Dependency acquisition is part of the environment, not a Rust-only
   afterthought.** Lockfiles, registries and fetchers are environment-specific.
5. **Delivery and issue closure are separate from quality.** A candidate can be
   accepted and still not be delivered; an issue can be referenced and still
   remain open.
6. **Salvage is candidate submission, not delivery.** R0 salvage is visible via
   `candidate_final_source` and eligible for the ordinary Gate; R1+ salvage
   requires human review and is never auto-merged (WP5/WP3).
7. **Auto-merge is default-branch-only.** A non-default protected base remains
   manual-only; the policy must not imply otherwise.

## 5. Work-package ownership

| WP | Owns | Files (indicative, not exclusive) | Depends | Acceptance |
| --- | --- | --- | --- | --- |
| WP0 | ADRs, contracts, audit, index/reservations | `docs/architecture/decisions/ADR-0040..0042`, `docs/contracts/{environment-plan,delivery-policy,issue-lifecycle}.md`, this audit, `docs/evidence/nix-devenv-license.md` | — | docs-only; contracts reviewed; no code/parity change |
| WP-E1 | Conventions + environment compiler + 0076 | `native/src/planning/environment*`, `native/src/project_import.rs` (conventions), `migrations/0076_environment_specs.sql` | WP0 | conventions precedence/conflict, plan/spec freeze, license gate |
| WP-E2 | Runtime/verifier/check environment binding | `native/src/bin/verifier.rs`, `native/src/bin/runtime_broker.rs`, `native/src/execution.rs`, `native/src/lead.rs` (`SpecCheck`), `deploy/Containerfile`, `compose.yaml` | WP0, WP-E1 | verifier runs the frozen spec; no global Rust image |
| WP-E3 | Dependency/supply-chain generalization | `native/src/dependencies.rs`, `native/src/dependency_broker.rs`, `native/src/bin/dependency_acquire.rs`, `compose.yaml` deps | WP0, WP-E1 | non-Rust lockfile/adapter, SBOM/license/audit, broker |
| WP-E4 | Planning/stack/quality generalization | `native/src/planning/stack.rs`, `native/src/planning/*`, `native/src/agentic.rs`, `native/src/bin/core.rs` planning handlers | WP0, WP-E1, WP-E2 | no hardcoded language default; environment artifact in plan |
| WP-D1 | Delivery policy + 0077 | `native/src/db/delivery*`, `migrations/0077_delivery_policies.sql`, Core routes | WP0 | policy revisions, R0/R1/R2/R3 eligibility |
| WP-D2 | Delivery execution/incidents + 0078/0075 | `native/src/bin/core.rs` delivery handlers, `native/src/db/delivery*`, `migrations/0078_delivery_attempts.sql`, `0075_candidate_final_source.sql` | WP0, WP-D1 | exact-head merge, idempotency, incident, manual revert, final-source binding for salvage |
| WP-I1 | Issue links/lifecycle + 0072/0073 | `native/src/issue_intake.rs`, `native/src/db/issue_intake.rs`, `native/src/github.rs`, `migrations/0072_issue_links.sql`, `0073_issue_delivery_claims.sql` | WP0 | links frozen, six states, comment/close receipts, retry fencing |
| WP-O1 | Office surfaces | `office/src/views/*`, `office/src/coreApi.ts`, `docs/product/office-parity.json` | WP-E1, WP-D1, WP-D2, WP-I1 | per-option browser/a11y; parity updated |
| WP-T1 | Tests/chaos/evidence | `native/tests/*`, `office/e2e-browser/*`, fixtures | all | deterministic + real Podman/browser evidence |

**Cross-program salvage ownership:** WP5 / WP3 own the salvage/final-source
policy (WP5 Worker salvage/finalization; WP3 planning/risk/Gate integration).
WP-D2 implements the applied `0075_candidate_final_source.sql` table and
delivery binding; WP-E2 supplies the frozen environment hash. This is an
explicit ownership assignment, not a claim that WP5/WP3 have implemented the
policy.

Shared-file rules and migration reservations are in
[delivery-environment program](delivery-environment-program.md). No WP0 file
claims implementation status.

## 6. Explicit non-claims

- This audit does not prove a non-Rust project can execute today; it proves the
  current tree is biased and names the work.
- This audit does not change `docs/product/office-parity.json`, capability
  states or migration files.
- A real multi-language Nix/devenv build, a real GitHub auto-merge and a real
  issue close/comment are `NOT_TESTED` until exact-candidate authorized evidence
  exists.
- `TRUSTED_LOCAL` remains not isolated. A generated environment, builder or
  container is not an isolation certification.
