# Live ed failure: Build fetch, not Resolve

Status: factual reconciliation for the build GO. No model/build call.

## Exact request/plan

- Intake: `fc7e7311-c49e-4c7a-abd9-49c501cb1c09`.
- Plan: `45f74096950806f7797aac1f191173d80ed7c8f569ff36042258fa85c5171ab1`, revision 3.
- `plan.source = PROJECT_COMMITTED`.
- `plan.packages = []`, `plan.toolchains = []`, `plan.services = []`.
- `plan.resolution = null`.
- Files: `flake.lock` and `flake.nix`, both `PROJECT`.

## Core route

`compile_environment_for_request` calls `resolve_generated_environment` only when:

```text
(plan.source == GENERATED && !has_lock) || !plan.packages.is_empty()
```

Neither condition holds for this plan. The live `POST
/agentic/requests/{id}/environment/review` path goes through
`freeze_reviewed_environment` → `build_environment_before_freeze` →
`EnvironmentBuilder::build_with_license_decisions`.

## Builder/broker evidence

The builder CLI `build` realizes the environment in its `fetch` phase and then
runs the licence gate. The failure was:

```text
{"blocker": {"code": "ENVIRONMENT_LICENSE_UNKNOWN",
 "message": "transitive closure path has no independently observed package metadata or reviewed identity: /nix/store/36qzll…-ed-1.21.1"},
 "status": "FAILED"}
```

The broker recorded `phase=fetch code=ENVIRONMENT_LICENSE_UNKNOWN`. That phase
is the Build command's fetch phase (the CLI accepts only `fetch` or `build`),
not the generated-environment Resolve command.

## Consequence

The missing generated-environment Resolve `realisation.json` / plan-keyed
persistent cache / RESOLUTION challenge path is **not** on the retained ed
route. It remains an open product gap for generated environments. The retained
ed pre-spec path can be assessed with the narrow build-phase realisation
pointer → broker fixed-query re-observation/sign → Core verify-only repair,
subject to Reviewer70's GO. This document does not claim
E2E or close the Resolve gap.
