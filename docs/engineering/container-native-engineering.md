# Container-native engineering

**Status:** Permanent repository engineering policy. The Compose baseline
(PR #28) is merged and its deterministic and owner-authorized live container
evidence is recorded in [container-native status](../implementation/container-native-status.md).
This policy does not certify isolation, deployment, Windows or the Office
redesign.

**Applies to:** runtime services, provider adapters, executables, dependencies,
images, browser-testing tools, development environments, CI lane changes,
networks, mounts, credentials and every operator-facing deployment workflow.

Use the [container-native engineering skill](../../.agents/skills/container-native-engineering/SKILL.md)
with this policy. The policy is normative; a skill cannot override it.

## 1. The permanent invariant

Software Factory is container-native by default. The installed product runs
through the canonical rootless Podman Compose stack. Normal product operation
must not require host-installed Rust, Cargo, Node, npm, OpenCode, Codex,
Playwright or manually launched Factory processes.

The allowed host prerequisites are:

- rootless Podman with a supported Compose provider;
- a browser;
- normal user namespaces, cgroup v2 and container kernel facilities.

`factory start/stop` remains a development convenience, not the installed
product path. A new host-only runtime dependency or unsupported Compose bypass
is a review failure, not a deployment choice.

## 2. Trust classes stay separate

| Class | Use | Credentials | Must not be used for |
|---|---|---|---|
| Development container | Trusted contributor workspace, Rust/Node/browser checks, diagnostics | None by default; no host HOME, provider key or runtime socket | Application runtime, product acceptance, untrusted code execution, isolation claims |
| Application runtime container | Persistent Core/Office, Supervisor, Worker, verifier, reviewer, runtime broker | Only the service's scoped credentials; shared engine credentials documented | Arbitrary repository code, untrusted tool calls |
| Ephemeral execution container | One Attempt or bounded repository/tool execution | None; no provider, Core, GitHub or host credentials | Persistent state, cross-Attempt reuse, host administration |
| Provider session container | One scoped provider session | Per-run egress token and non-secret config only | Long-lived provider credential, Core/GitHub tokens, host paths |

A container is not a trust upgrade. The development container is trusted and
writable; it must not be merged with the execution engine to reduce containers.
Rootless Podman and the dedicated engine remain `TRUSTED_LOCAL` until an
independent, candidate-bound evaluation proves more. Never call them isolated
or SEC-01 certified.

Every new process, container, sandbox or privilege still goes through the
[service placement policy](../architecture/service-placement-policy.md).

## 3. Canonical development workflow

The development environment is a Compose service built from
`.devcontainer/Containerfile`. It contains the pinned Rust 1.90.0, Node 22.23.2,
SQLx CLI 0.8.6, Clippy, rustfmt, Git, SQLite tooling and pinned Playwright
Chromium. It is a trusted development image, not a runtime or sandbox.

Start or run one-shot jobs from the repository root:

```sh
# Build the development image once
podman compose -f compose.yaml -f compose.dev.yaml build dev

# Rust gates
podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev \
  cargo xtask check-container

# Office typecheck, lint and production build
podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev \
  sh -c 'npm --prefix office ci && npm --prefix office run typecheck && npm --prefix office run lint && npm --prefix office run build'

# Real Chromium Office journey
podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev \
  sh -c 'npm --prefix office ci && npm --prefix office/e2e-browser ci && cargo xtask e2e office'

# Inspect a running stack
podman compose ps
podman compose logs -f core
```

`check-container` runs format, strict Clippy, the workspace tests that do not
require host user namespaces, build, docs and audit inside the pinned image. It
skips the bubblewrap-dependent provider/campaign/Codex fixtures plus the pure
bubblewrap boundary tests because the development container cannot mount the
nested proc/user namespace. It does **not** replace the migration check,
deterministic recovery/dispatch/negatives/provider E2E, architecture
consistency or repository hygiene; those remain in the host/CI and Compose
lanes. No required CI acceptance is lost: the host Rust lane keeps the full
suite and the container-dev lane exercises the real Compose/Chromium path.

Commands and evidence must be run from the exact candidate. Do not install a
missing host tool to bypass a broken development image; repair the image or
record the workflow as `BLOCKED`.

`compose.dev.yaml` is a trusted development override, not part of the installed
topology. It may bind the repository and use a development image. The product
`compose.yaml` must not gain host binds or development services.

### Development workflow status

| Item | Status | Evidence |
|---|---|---|
| Development image with Rust/Node/SQLx | `IMPLEMENTED` | `.devcontainer/Containerfile` |
| Playwright Chromium in the development image | `IMPLEMENTED` in this policy change; exact-image run recorded with the Slice 1 candidate | Containerfile and Slice 1 evidence |
| Canonical Compose development service | `IMPLEMENTED` | `compose.dev.yaml` |
| Rust/Office/browser commands through the service | `LOCALLY_VERIFIED` at `a75d4c3f9e65cec4318678f4b0e4d6394e03a008`; container-dev CI lane `VERIFIED` on PR #29 head | [Slice 1 evidence](../evidence/office-command-center-slice1-2026-10-27.json) |
| Windows/Podman Machine | `NOT_TESTED` | No Windows host |
| SELinux/AppArmor hosts and digest pinning | `NOT_TESTED` / `NOT_IMPLEMENTED` | See security policy |

## 4. Images and dependencies

- Multi-stage builds only. Build toolchains stay in builder stages; runtime
  stages contain the binary/libraries and required runtime data.
- Pin Rust, Node, SQLx, OpenCode, Codex, Playwright and base images. Tag pinning
  is not digest pinning; record digest pinning as `NOT_IMPLEMENTED` until
  verified.
- Never bake credentials, host HOME, repository content, provider sessions,
  browser profiles or build outputs into a runtime image.
- Do not pass secrets as build arguments.
- Evaluate a maintained crate/package before custom infrastructure, per the
  [dependency-selection policy](dependency-selection.md).
- New frontend dependencies must build inside the container and must not add a
  host-only executable or a second component system.
- The provider image may contain provider runtimes and Node; it must not contain
  Core authority, provider credentials, a host toolchain or a runtime socket.

## 5. Networks, mounts and credentials

- Only the intended operator endpoint is published, on host loopback. Core,
  broker, engine, provider control and Worker ports remain internal.
- No host Podman/Docker socket, host networking, privileged mode, host HOME,
  `/run/user`, device passthrough or arbitrary host bind in the product stack.
- Use named volumes and explicit directories. A runtime service receives only
  the volumes needed for its responsibility.
- Default-deny execution egress. Provider traffic goes through the scoped egress
  broker; provider children have no host/control network.
- Per-service configuration volumes are scoped. A deliberately shared execution
  credential (for example the runtime-broker token used by the execution roles)
  is shared authority and must be documented as such; never describe it as
  per-role secret isolation.
- Redact credentials, tokens, prompts, source bodies and environment maps at
  logs, telemetry, evidence and error boundaries. Do not print an enrollment
  code, operator token or provider key in agent output.

## 6. Testing and CI

Use the same container-native commands in development and CI:

- Rust format, Clippy, tests, build, docs, audit and migrations;
- Office typecheck, lint, production build and component tests;
- deterministic Core/Worker/Supervisor/verifier/reviewer/provider E2E;
- real Chromium journeys at desktop, tablet and narrow widths;
- Compose static policy and negative tests;
- real Podman stack acceptance, restart/persistence, cancellation, cleanup and
  stale-lease/fencing negatives.

The deterministic Compose E2E is mandatory for changes that affect the
container topology, broker, engine, Supervisor/Worker startup, provider session
lifecycle or quality services. Mock-only or host-only evidence does not satisfy
a container-execution change. The real lane may be a required CI job or a
recorded manual acceptance when the runner cannot support the nested engine;
the PR must state which and why.

Every PR must fill the container-native impact declaration when it changes a
matching path. The declaration is bound to the PR base/head and is reviewed
semantically; a green form check is not proof of the claim.

## 7. Operations and diagnostics

- `podman compose ps` distinguishes created, starting, healthy and exited
  services.
- `podman compose logs <service>` is the first diagnostic. Never print secrets
  while collecting logs.
- Core `/health`, `/setup/status`, authenticated projections and the runtime
  broker `/health` expose configuration and readiness state.
- Office shows provider/session/campaign/gate state truthfully. Container-level
  health that Core cannot observe is not invented; use the documented Compose
  command instead.
- A broker-only restart removes managed containers, including containers that
  may belong to live attempts. Record the resulting failure and recovery; do
  not describe it as graceful.
- Remove disposable projects, volumes, provider sessions and credentials after
  a test or UAT and record the cleanup result.

## 8. Review checklist

- Is the change container-native, or does it add a host-only dependency?
- Does it use the approved development/CI commands?
- Does it preserve rootless Podman, the nested engine and the narrow broker?
- Are network, mount, image, capability and credential boundaries unchanged or
  explicitly reviewed?
- Are secrets outside images, logs, browser storage, prompts and evidence?
- Is there a real Podman test for the affected execution path?
- Are negative, restart, cleanup, resource-bound and stale-generation cases
  covered?
- Are `NOT_TESTED`, `BLOCKED` and `NOT_CERTIFIED` stated truthfully?
- Are docs, Compose policy, skills and evidence updated atomically?

## 9. Stop conditions

Stop and report when a change requires a host-only runtime dependency, a
Compose bypass, broader privileges, a host socket/path, an unapproved image, an
exposed credential, a relaxed spending/quality/delivery policy, or an
unsupported isolation claim.

## References

- [Compose deployment](../operations/compose-deployment.md)
- [Container runtime security](../security/container-runtime.md)
- [Service placement policy](../architecture/service-placement-policy.md)
- [Container-first development](container-first-development.md)
- [Automation standards](automation-standards.md)
- [Isolation policy](../security/isolation.md)
- [ADR-0019](../architecture/decisions/ADR-0019-container-first-cross-platform.md),
  [ADR-0024](../architecture/decisions/ADR-0024-container-native-compose-runtime.md),
  [ADR-0025](../architecture/decisions/ADR-0025-container-native-and-office-parity.md)
- [Container-native engineering skill](../../.agents/skills/container-native-engineering/SKILL.md)
