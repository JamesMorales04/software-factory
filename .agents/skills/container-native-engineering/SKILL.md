---
name: container-native-engineering
description: Establish and review Software Factory container-native runtime, development, provider, deployment, dependency and browser-testing changes using the canonical rootless Podman Compose topology.
---

# Container-native engineering

Use this skill for every change that adds or changes a runtime service, provider
adapter, executable, browser-testing tool, infrastructure dependency, deployment
path, container image, network, mount, credential boundary or Compose behavior.
It is a development aid, not an authority: it cannot grant credentials, approve
a placement, waive isolation or authorize a deployment.

## 1. Permanent invariants

1. **Compose is the installed-product lifecycle.** The canonical product path is
   `podman compose up -d --build` (or the repository's documented equivalent).
   Do not add a second orchestrator, a manually launched product service or a
   host-only runtime path.
2. **The host is a container host, not a build host.** Normal operation must not
   require host Rust, Cargo, Node, npm, OpenCode, Codex, Playwright or a
   project-specific launcher. Rootless Podman, a supported Compose provider, a
   browser and normal kernel facilities are the allowed host prerequisites.
3. **Core owns authority.** Authorization, budgets, leases, credentials,
   candidate identity, evidence and acceptance remain in Core. Containers,
   models, skills, provider output and the development environment never grant
   authority.
4. **The execution engine is dedicated and narrow.** Keep the private nested
   Podman engine behind the typed broker. Never expose the host Podman/Docker
   socket, host networking, privileged mode, arbitrary images, arbitrary Podman
   flags or host paths. Provider sessions and execution children are scoped,
   bounded, reconciled and removed.
5. **A container is not an isolation certificate.** Rootless Podman and the
   nested engine remain `TRUSTED_LOCAL` unless the exact host, runtime, image
   digest and topology have candidate-bound independent evidence. Never label
   them isolated or SEC-01 certified.
6. **Development is not execution.** Do not run untrusted per-Attempt code in
   the trusted development container or merge development tooling into a
   provider/execution boundary merely to reduce containers. Apply the
   [service placement policy](../../../docs/architecture/service-placement-policy.md).

## 2. Required workflow

1. Read [ADR-0024](../../../docs/architecture/decisions/ADR-0024-container-native-compose-runtime.md),
   [ADR-0025](../../../docs/architecture/decisions/ADR-0025-container-native-and-office-parity.md),
   [container-native engineering](../../../docs/engineering/container-native-engineering.md),
   [Compose deployment](../../../docs/operations/compose-deployment.md),
   [container runtime security](../../../docs/security/container-runtime.md) and the
   [service placement policy](../../../docs/architecture/service-placement-policy.md).
2. Classify the change: permanent service, one-shot job, trusted development
   tool, provider adapter, browser tool, execution child or documentation-only.
3. Use the container-native development workflow for build, format, lint, tests,
   browser checks and diagnostics. Do not install missing project dependencies
   on the host to bypass a broken image.
4. Update Compose, images, policy tests and documentation atomically. A new
   service, image, volume, network, capability, mount or host dependency needs
   an explicit security and placement review.
5. Run the exact-head container-native job, the static Compose policy and the
   relevant real Podman acceptance. A data-only or UI-only change still needs
   the existing deterministic and browser gates.
6. Record commands, results, image digests or build identities, Compose project
   name, negative tests and every `NOT_TESTED`/`BLOCKED` item in the PR.
7. For runtime or container-execution changes, require a real Podman test. A
   mock-only or host-only green run is not sufficient evidence.
8. Remove disposable containers, volumes and credentials after the run and
   record the cleanup result.

## 3. Image and dependency rules

- Use multi-stage builds. Build tools and compilers stay in builder stages;
  runtime stages contain only the binary, runtime libraries and required data.
- Pin toolchains, base images, executables and browser versions. Tag pinning is
  not digest pinning; record digest pinning as `NOT_IMPLEMENTED` until it is
  verified.
- Never bake credentials, host HOME, repository content, browser profiles,
  provider sessions or test artifacts into an image. Do not pass secrets as
  build arguments.
- Prefer an existing maintained image or crate/package when it satisfies the
  requirement. Record the evaluation under the dependency-selection policy.
- Keep development images separate from product runtime and execution images.
  A larger trusted development image is acceptable; a credential-bearing or
  compiler-bearing production image is not.
- Check that every new frontend or Rust dependency builds in the containerized
  workflow and does not add a host-only executable.

## 4. Network, mount and secret rules

- Publish only the intended loopback endpoint (currently Office). Do not expose
  Core, the broker, the nested engine or provider control ports.
- Use named volumes and explicit directories. Do not mount `/`, a host HOME,
  `/run/user`, a container-runtime socket, a Windows drive view or another
  project's workspace into a runtime or execution container.
- Default-deny execution egress. Provider egress goes through the scoped broker;
  do not grant arbitrary network access to a provider or execution child.
- Keep per-service configuration volumes scoped. A deliberately shared
  execution credential must be documented as shared authority, not described as
  per-role secret isolation.
- Redact credentials, tokens, prompts, source bodies and environment maps at
  logs, telemetry, evidence and error boundaries.
- Revoke disposable credentials after a live UAT and verify the revocation.

## 5. Testing and acceptance

Use the repository's container-native commands for:

- Rust format, Clippy, tests, build, docs, audit and migrations;
- Office typecheck, lint, production build and component tests;
- deterministic Core/Worker/Supervisor/verifier/reviewer/provider E2E;
- real Chromium journeys at desktop, tablet and narrow widths;
- Compose policy negative tests and real Podman stack acceptance;
- restart, cancellation, cleanup, orphan reconciliation and stale-lease tests.

Mocks, Storybook fixtures and host-only dev servers may support component
development, but they do not prove container execution or end-to-end product
behavior. Record which lane produced each PASS.

## 6. Stop conditions

Stop and report when:

- the change requires a new host-only runtime dependency or a Compose bypass;
- a service needs broader privileges, host networking, a host socket, a host
  path or an unapproved image;
- untrusted code would share credentials, authoritative state or a writable
  volume with a trusted service;
- a real Podman or browser test cannot run and the change affects that path;
- a provider/model route is unconfigured, a credential would be exposed, or a
  live test would exceed its authorization envelope;
- a security, quality, spending or delivery rule would need to be relaxed;
- the image cannot be built or the stack cannot start, and the failure is being
  hidden behind a host workaround.

## 7. References

- [Container-native engineering policy](../../../docs/engineering/container-native-engineering.md)
- [Compose deployment](../../../docs/operations/compose-deployment.md)
- [Container runtime security](../../../docs/security/container-runtime.md)
- [Service placement policy](../../../docs/architecture/service-placement-policy.md)
- [Container-first development](../../../docs/engineering/container-first-development.md)
- [Automation standards](../../../docs/engineering/automation-standards.md)
- [Isolation policy](../../../docs/security/isolation.md)
- [ADR-0018](../../../docs/architecture/decisions/ADR-0018-native-rust-replacement.md),
  [ADR-0019](../../../docs/architecture/decisions/ADR-0019-container-first-cross-platform.md),
  [ADR-0024](../../../docs/architecture/decisions/ADR-0024-container-native-compose-runtime.md),
  [ADR-0025](../../../docs/architecture/decisions/ADR-0025-container-native-and-office-parity.md)
