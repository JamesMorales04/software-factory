---
id: ADR-0019
status: Accepted
date: 2026-10-23
owner: product-owner
supersedes: null
---
# ADR-0019: Container-first development and cross-platform deployment

- **Status:** Accepted for direction only. The development-container configuration is `IMPLEMENTED`; the image was built locally and `cargo xtask check`, `migrate` and `e2e recovery` passed inside it on 2026-10-23 using a host-network workaround. Default rootless container networking, the Office build inside the container, application runtime images, Quadlet, Windows/Podman Machine and every unified `factory` CLI command remain `NOT_TESTED` or `NOT_IMPLEMENTED`. No isolation is certified.
- **Decision date:** 2026-10-23.
- **Relationship:** Adds packaging and platform direction to [ADR-0018](ADR-0018-native-rust-replacement.md). It does not supersede ADR-0018, [ADR-0009](ADR-0009-trusted-local-execution.md), the [service placement policy](../service-placement-policy.md), or the [isolation policy](../../security/isolation.md).
- **Approval scope:** The Product Owner approved the container-first direction recorded here. That approval does not authorize deployment, non-loopback exposure, host mounts, credential placement in untrusted execution, image publication, legacy deletion, or any security certification. No independent technical verification exists.

## Context and problem

The native Rust branch is an incomplete replacement. Core and Worker binaries exist, but there is no unified `factory` CLI, no container-first packaging, no Windows launcher, and no verified cross-platform path. Development currently depends on host-installed toolchains and host-specific paths. A Windows-native build and runtime would duplicate Core/Worker behavior, paths, permissions and packaging, and would make evidence harder to compare with Linux.

Factory must support two developer platforms without creating two products:

- Linux hosts where rootless Podman, systemd user services and Quadlet are available; and
- Windows 11 hosts where Podman Machine uses a WSL2 Linux environment.

At the same time, existing policy requires that a container is not treated as a trust upgrade, that untrusted execution is `LEVEL D` or `LEVEL E` only, that provider credentials stay outside untrusted execution, and that rootless Podman is a candidate runtime, not certified isolation. The container-first direction must therefore separate developer convenience from application runtime and from untrusted execution.

The relevant constraints are:

- Rust Core, Worker, provider adapters, persistence and governance tools use Rust; Office remains React/TypeScript.
- Persistence is SQLx + SQLite with one authoritative writer.
- The pinned toolchain is Rust 1.90.0 (`rust-toolchain.toml`), Node is pinned by `.nvmrc` (22.23.2), SQLx CLI is 0.8.6, and dependencies are locked.
- ADR-0018 rejects a Go Core/Worker and an intermediary coding-agent orchestrator.
- SEC-01 isolation work is paused and not approved. Rootless Podman fixture results do not certify credential confinement or a provider-in-sandbox topology.
- Git worktrees isolate revisions, not processes, credentials or the kernel.

## Decision drivers and invariants

1. **One Linux implementation, two launch environments.** The same Core, Worker, Supervisor, provider adapters, application runtime images and tests are used on Linux and Windows. Windows must not grow a second Core/Worker.
2. **Separate trust classes.** A trusted writable development container, a trusted minimal application runtime container and an untrusted ephemeral execution sandbox are different things and must not be conflated.
3. **No container-as-certification.** Packaging never changes a component's placement level or grants isolation, authority or credentials.
4. **Host paths are explicit.** A Windows path is not a Linux path. Factory materializes per-Attempt workspaces in a Linux filesystem and rejects unsafe configurations.
5. **No networking shortcuts.** Host networking, host loopback assumptions and broad port publication are forbidden as convenience mechanisms.
6. **Credentials stay scoped.** No host HOME copy, no committed tokens, no provider authentication in untrusted execution.
7. **One lifecycle entry point.** A single Rust `factory` CLI, launched host-natively, owns lifecycle operations. The Windows launcher is a thin adapter, not a second controller.
8. **Truthful status.** `SUPPORTED`, `IMPLEMENTED`, `LOCALLY_VERIFIED`, `INDEPENDENTLY_VERIFIED`, `NOT_TESTED` and `BLOCKED` mean exactly what the vocabulary below says. Documentation is not execution evidence.
9. **Deterministic placement policy.** Trust class and placement are chosen by deterministic policy, not by model output, skill output or a container label.

## Considered alternatives

1. **Windows-native Core/Worker plus a separate Linux build.** Rejected. It duplicates runtime behavior, path handling, credentials and evidence, and contradicts ADR-0018's single Rust implementation.
2. **Host-only development without a container.** Rejected as the primary path. It is not reproducible across Linux distributions and Windows, and it makes path and credential drift likely. Host-native execution remains useful only as an explicitly trusted local mode.
3. **Use the development container as the application runtime or execution sandbox.** Rejected. It is trusted, writable, contains compilers and may have package-network access; it is neither minimal nor credential-free.
4. **Use `--network=host` or assume container loopback reaches host loopback.** Rejected. Both hide exposure and make authentication and egress policy unreliable.
5. **Mount `C:\`, `/mnt/c`, a Windows user profile or a network share as the Linux workspace.** Rejected. POSIX semantics, permissions, case sensitivity, symlinks and inode behavior cannot be assumed, and the mount crosses the intended workspace boundary.
6. **Mount the host Podman/Docker socket into the development container or a runtime container.** Rejected by default. It gives the container control over the host user's runtime and is unacceptable for untrusted execution. A local, uncommitted override may be considered only for trusted diagnostics after a placement assessment.
7. **Run provider credentials and untrusted tool execution in one container.** Rejected. It is the credential-boundary failure already recorded by SEC-01. Credential-bearing provider processes stay outside untrusted execution.
8. **One container or Pod for all trusted services and untrusted code.** Rejected. A Pod is a scheduling and network unit, not a trust boundary; co-location weakens separation.

## Decision

### 1. Three workload classes remain distinct

| Class | Trust and purpose | Contents | Credentials | Lifecycle | Must not be used for |
|---|---|---|---|---|---|
| **Development container** | Trusted contributor workspace. Writable and convenient. | Compilers, Rust/Node toolchains, Git, build tools, editor server, test tools. | None by default; no host HOME and no provider auth baked or mounted by default. | Long-lived developer session. | Application runtime, production service, untrusted code execution, or a security boundary claim. |
| **Application runtime container** | Trusted service packaging. Minimal and controlled. | Only the service binary/runtime and required libraries; no compilers or general-purpose build tooling. | Only the service's scoped credentials, injected at runtime. | Persistent, restartable service managed by systemd/Quadlet on Linux or the Linux environment on Windows. | Arbitrary repository code, agent tool calls, downloaded scripts, or untrusted tests/builds. |
| **Ephemeral execution sandbox** | Untrusted execution. Disposable and credential-free. | Repository commands, builds, tests, candidate execution and agent/tool actions. | None: no provider, GitHub, Core, Worker or host tokens. | One Attempt or bounded unit; reconciled and removed. | Persistent state, provider authentication, host administration, or cross-Attempt reuse. |

Containerization does not promote a component into a higher trust class. Core authority, SQLite ownership, provider credentials and GitHub delivery rights remain where existing policy places them. The development container is not part of the Factory runtime topology and never receives Core authority.

### 2. Linux target

- Podman runs **rootless** in the developer's Linux user session.
- Persistent application runtime services use **systemd user services and Quadlet** for restart, lifecycle and boot integration. Rootful or system-wide services require a separate protected decision and are not approved here.
- The trusted **Sandbox Supervisor** is a host Linux process at `LEVEL B`, outside every untrusted execution container. It receives a Worker-scoped session credential and a scoped launcher capability. It launches, bounds and reconciles per-Attempt sandboxes. It does not hold provider or GitHub credentials by default and must not execute repository code in its own process.
- The development container is built from the pinned configuration in [Container-first development](../../engineering/container-first-development.md) and is not the Supervisor, Core, Worker or an execution sandbox.
- Images are digest-pinned before deployment. Tags alone are not a reproducibility or integrity claim. Digest pinning and image publication are `NOT_IMPLEMENTED`.
- Rootless Podman remains a candidate `LEVEL D` runtime. It is not certified isolation, and it does not prove credential confinement, cleanup, UID separation or kernel confinement.

### 3. Windows 11 target

- Windows 11 uses **Podman Machine backed by WSL2**. The same digest-pinned `linux/amd64` application runtime images are used as on Linux; there is no Windows-native Core/Worker image or process.
- The **Sandbox Supervisor** runs inside the WSL2 Linux environment, never as a Windows process. Provider servers and application runtime services also run in Linux.
- A **small Windows launcher** may start or verify the WSL2 distribution and Podman Machine, invoke the Linux `factory` CLI, translate display-only paths, stream output and return the exit code. It contains no scheduling, authorization, workspace, credential, provider or quality logic.
- The exact WSL topology (a dedicated Factory distribution versus the Podman Machine distribution) requires a placement assessment; this ADR does not select it. Every acceptable variant keeps the Supervisor and all Factory runtime processes in Linux.
- WSL2 provides a VM boundary, but it is not thereby a certified Factory sandbox. Container-to-container execution inside the Podman Machine still shares a Linux kernel. No `LEVEL E` or isolation claim follows from running on Windows.

### 4. Host path and workspace policy

The following rules are mandatory:

- Factory selects a canonical Linux workspace root with POSIX semantics. On Windows that root is inside the WSL2 ext4 filesystem, not a Windows drive view.
- A per-Attempt workspace is materialized under a Factory-owned root for exactly one Attempt, with an owner record, bounded lifetime, canonical path validation and cleanup. Mutable workspaces are never shared across Attempts, projects or trust classes.
- The following are rejected as workspace or bind-mount roots: a host HOME, `C:\`, `/mnt/c`, a Windows user profile, OneDrive or another synchronized folder, a network/UNC share, a symlink that resolves outside the approved root, and any path that is writable by an unrelated user.
- The Windows launcher may show a Windows path to the user, but it must not translate `C:\...` into a Linux workspace or expose a Windows drive to a container. The only supported Windows view is a WSL filesystem path that resolves inside the WSL distribution.
- A Git worktree is source-management isolation, not a security boundary. Existing worktree allocation does not satisfy this path policy by itself.
- Unsafe configuration fails closed. There is no fallback to a host directory, host shell or TRUSTED_LOCAL execution.

The full materialization, rejection and cleanup policy is `NOT_IMPLEMENTED`. Existing Rust workspace allocation is a starting point, not evidence that this policy exists.

### 5. Networking

- Do **not** use host networking (`--network=host`), host PID/IPC namespaces or container-runtime sockets as a shortcut.
- **Container loopback is container loopback.** A service bound to `127.0.0.1` inside a container is not reachable from host loopback merely because a port is published. Published traffic targets a container interface, not the container's loopback.
- Persistent services expose only an explicit, authenticated endpoint. The default publication, when one is required, is host loopback only (`127.0.0.1`). Binding `0.0.0.0`, publishing to a LAN interface or opening a Windows firewall rule is not a default and requires explicit authorization.
- Untrusted sandboxes are default-deny for egress where practical, with explicit destinations. They cannot reach Core, the database, provider control ports, the host LAN or metadata services.
- The current Core refuses non-loopback binding. Therefore containerized Core API exposure is `NOT_IMPLEMENTED`; running Core host-native with its loopback bind is the only current behavior. Using host networking to work around this is forbidden.
- Windows host loopback, WSL2 loopback, Podman Machine loopback and container loopback are four distinct scopes. No code may assume `localhost` crosses them. Port forwarding and firewall behavior require explicit verification.

### 6. Credential handling

- Never copy or mount the host HOME, Windows user profile, SSH keys, cloud configuration, GitHub configuration, Codex/OpenCode authentication directories or credential stores into a runtime container or sandbox.
- Never commit tokens, secrets or credential-bearing `.env` files, and never bake them into an image, build argument, layer, Quadlet unit, devcontainer file or command line.
- Provider authentication belongs to a trusted provider server or trusted adapter outside untrusted execution. A provider process may hold only its own provider credential; it must not receive GitHub, Core or delivery tokens.
- Application runtime credentials are scoped to one service and injected at runtime through a controlled mechanism. The exact mechanism is `NOT_IMPLEMENTED`; it must not rely on a host HOME bind.
- Untrusted execution receives no credentials. If a required tool cannot run without a credential, that work is `BLOCKED` rather than moved into a credential-bearing container.
- Logs, errors, evidence and telemetry redact secrets at emission.

### 7. CLI lifecycle

- There is one Rust `factory` CLI. It is a host-native lifecycle tool: a Linux binary on Linux, and on Windows a small launcher that invokes the Linux binary inside WSL2.
- The CLI is not a container and is not run inside the development container as the supported lifecycle entry point. It may be built and tested in the development container.
- `factory doctor` is the intended preflight command for host, Podman/WSL, rootless mode, path policy, port exposure, image digests and toolchain versions. It must fail closed with specific remediation.
- The Rust `factory` CLI exists and implements `doctor`, `start`, `status`, `logs` and `stop`; `install`, `restart`, `update` and `dev` remain unimplemented.
- `factory-core`, `factory-worker`, `factory-supervisor`, `factory-verifier`, `factory-provider-probe`, `factory-repo-hygiene` and `factory-architecture-check` are workspace binaries. The CLI manages the Core and, with explicit project roots, host Supervisor/Worker lifecycle.
- The CLI may not silently downgrade to host execution, host networking, a rootful runtime or TRUSTED_LOCAL when a prerequisite is missing.

**Command status:** `factory doctor`, `factory start`, `factory status`, `factory logs` and `factory stop` are implemented. `factory install`, `factory restart`, `factory update` and `factory dev` remain `NOT_IMPLEMENTED`. The runtime binaries are `IMPLEMENTED`; Windows lifecycle and production installation are `NOT_TESTED`.

### 8. Development-container configuration

The initial `.devcontainer` uses the official Development Containers shape with a `Containerfile` build, the workspace folder `/workspaces/software-factory`, a non-root user, no privileged mode and no Podman socket by default. It installs the pinned Rust 1.90.0 toolchain components, Node 22.23.2 from `.nvmrc`, Git, build tools and SQLx CLI 0.8.6. It intentionally does not install Go.

The optional host Podman socket is excluded. A local override is a trusted developer action outside CI and must never be used for untrusted code. The development container is not a runtime or sandbox.

Implementation guidance: [Container-first development](../../engineering/container-first-development.md), [Linux deployment](../../operations/linux-deployment.md), and [Windows 11 with Podman Machine](../../operations/windows-podman-machine.md).

## Relationship to existing authority and policy

- [ADR-0018](ADR-0018-native-rust-replacement.md) remains the target architecture. This ADR does not change Rust selection, Core ownership, provider integration or the incomplete status of the replacement.
- [ADR-0009](ADR-0009-trusted-local-execution.md) remains in force. A trusted developer container or host run is not isolated; TRUSTED_LOCAL is never described as sandboxed or certified.
- The [service placement policy](../service-placement-policy.md) remains authoritative. Development containers are not a placement level. Application runtime containers are `LEVEL C` packaging for trusted services. Per-Attempt execution is `LEVEL D`; stronger isolation is `LEVEL E`. A container or Pod is not a trust boundary.
- [Isolation policy](../../security/isolation.md) and the [SEC-01 results](../../security/SEC-01-results-2026-10-18.md) remain in force. This ADR does not resume SEC-01 and does not certify credential confinement. Credentialed untrusted execution remains `BLOCKED`.
- No model, skill, container label or image tag may choose a trust class or grant a capability.

This ADR does not update [MASTER](../MASTER.md), the [service placement policy](../service-placement-policy.md), the [ADR index](README.md) or the [documentation index](../../README.md). Those normative updates are required before this direction is treated as canonically indexed; their absence is an open governance task, not an implied approval.

## Verification status vocabulary

These labels are not interchangeable:

| Label | Meaning |
|---|---|
| `SUPPORTED` | An approved platform or toolchain target with a documented path. It does not mean a binary exists, the path was exercised or security is certified. |
| `IMPLEMENTED` | The described code, configuration or command exists in the repository or built image at the stated revision. It does not mean it ran successfully. |
| `LOCALLY_VERIFIED` | An author run in a named environment produced the stated observation for the exact candidate. It is not independent acceptance. |
| `INDEPENDENTLY_VERIFIED` | A verifier separate from the author exercised the claim on a frozen candidate with an independent oracle. No claim in this ADR has this status. |
| `NOT_TESTED` | No execution evidence exists. |
| `BLOCKED` | A required prerequisite, isolation boundary, authority or platform is absent. The action must fail closed, not fall back to a weaker mode. |

`NOT_IMPLEMENTED` is an implementation status for commands and automation: no Factory command or automation exists. A named host tool may exist in an operator procedure, but that procedure is not a Factory implementation and is `NOT_TESTED`.

## Verification status of this decision

| Claim | Status | Evidence |
|---|---|---|
| Container-first direction approved | Accepted for direction | Product Owner direction; no independent technical evidence |
| Linux rootless Podman + Quadlet topology | `SUPPORTED` target; `NOT_TESTED` | No Quadlet unit, installer or deployment was run |
| Windows 11 + WSL2 + Podman Machine topology | `SUPPORTED` target; `NOT_TESTED`; Windows verification `BLOCKED` | No Windows host was available |
| Development-container files | `IMPLEMENTED` as configuration | Files added; image build `NOT_TESTED` |
| Development-container build and run | `NOT_TESTED` | No image was built or started |
| Same Linux application runtime image on Linux and Windows | `NOT_TESTED` | No image was published or run |
| Unified `factory` CLI and `factory doctor` | `NOT_IMPLEMENTED` | No `factory` binary in the Cargo workspace |
| Containerized Core API exposure | `NOT_IMPLEMENTED` | Core currently refuses non-loopback binding |
| Per-Attempt workspace path policy and cleanup | `NOT_IMPLEMENTED` | Existing worktree allocation is not the full policy |
| Ephemeral sandbox isolation and credential confinement | `BLOCKED` | SEC-01 paused; rootless Podman not certified |
| Independent verification | None | No independent verifier ran this change |

## Consequences

### Positive

- Linux and Windows share one Rust runtime, one image family and one set of behavioral evidence.
- Development becomes reproducible without installing Rust, Node or SQLx on the host.
- Trust classes are explicit, reducing the chance that a development container is mistaken for a sandbox.
- Path, network and credential rules are decided before implementation rather than discovered during a Windows port.
- A single CLI avoids a second lifecycle authority.

### Negative and open

- No implementation or verification accompanies this ADR. The `.devcontainer` image is unbuilt, and the Windows path is entirely unexercised.
- Containerized Core networking conflicts with the current loopback-only bind; resolving it requires an explicit code and policy change, not a host-network workaround.
- Rootless Podman and WSL2 add operational complexity and do not certify isolation.
- Digest pinning, image publication, Quadlet units, the Windows launcher, path enforcement, secret injection and `factory doctor` all remain implementation work.
- The ADR index, MASTER and service-placement baseline are not updated here; governance reconciliation is outstanding.
- A developer container with compilers and package-network access is attractive for arbitrary code. It must not become the untrusted execution path.

## Verification obligations

Before any platform or container claim moves beyond `NOT_TESTED`:

1. Build the development-container image from the pinned base and verify Rust 1.90.0, Node 22.23.2, SQLx CLI 0.8.6 and the absence of Go.
2. Run the repository format, strict Clippy, test, build and migration checks inside the development container on a frozen candidate.
3. Exercise Linux rootless Podman with Quadlet for a persistent trusted service, including restart, boot/linger behavior and cleanup.
4. Prove that a containerized service is reachable only through its explicit authenticated loopback publication, and that container loopback is not treated as host loopback.
5. On Windows 11, exercise the launcher, WSL2 path policy, Podman Machine startup, the same image digest, port forwarding and failure behavior.
6. Prove that `C:\`, `/mnt/c`, host HOME, Windows profiles, network shares, runtime sockets and unsafe symlinks are rejected without fallback.
7. Prove that an untrusted sandbox has no provider, GitHub, Core or Worker credentials, no host paths, no runtime socket and no unintended egress; keep credentialed execution `BLOCKED` until independently verified.
8. Have an independent verifier repeat the user journeys on the exact candidate, with an independent oracle, before any `INDEPENDENTLY_VERIFIED` claim.

## Supersession

This ADR supersedes nothing. It must be superseded explicitly if the platform direction, trust-class separation or CLI lifecycle changes. Historical rationale in ADR-0018 and ADR-0009 remains intact.
