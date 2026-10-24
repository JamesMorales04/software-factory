# Windows 11 with Podman Machine

**Status:** Target design from [ADR-0019](../architecture/decisions/ADR-0019-container-first-cross-platform.md). No Windows host, WSL2 distribution, Podman Machine or Windows launcher was exercised. Every Windows claim in this guide is `NOT_TESTED`; Windows verification is `BLOCKED` until a real host is available. This is not a support statement or a deployment certificate.

## Scope

This guide describes the intended Windows 11 developer and runtime shape:

- Podman Machine backed by WSL2;
- the same Linux application runtime images used on Linux;
- the Rust `factory` binary and trusted Sandbox Supervisor inside the WSL2 Linux environment;
- a small Windows launcher that starts/attaches to WSL2 and invokes the Linux CLI;
- no Windows-native Core, Worker, Supervisor, provider server or duplicate execution engine.

It does not cover Windows 10, Windows Server, Hyper-V-only Podman, Windows containers, Docker Desktop as the reference runtime, or a public production service. Linux details are in [Linux deployment direction](linux-deployment.md).

## Current status

| Item | Status | Notes |
|---|---|---|
| Windows 11 + WSL2 + Podman Machine topology | `SUPPORTED` target; `NOT_TESTED`; verification `BLOCKED` | No Windows host was available |
| Same Linux application runtime image on Windows and Linux | `NOT_TESTED` | No image was built, published or run |
| Windows launcher | `NOT_IMPLEMENTED` | No launcher exists |
| WSL2 path policy enforcement | `NOT_IMPLEMENTED` | No code enforces the policy |
| Supervisor process in WSL2 | `NOT_TESTED` | `factory start` can manage it on Linux; Windows/WSL acceptance is not run |
| Unified `factory` CLI and `factory doctor` | `PARTIAL` | Linux/WSL2 lifecycle commands exist; Windows launcher is not implemented |
| Windows container E2E | `NOT_TESTED` | Nothing was started |
| Independent verification | None | No independent verifier ran this topology |

## Target topology

```text
Windows 11 host
├── small Windows launcher
│   ├── starts/verifies WSL2 and Podman Machine
│   ├── invokes the Linux factory binary
│   ├── translates display paths only
│   └── returns output and exit status
└── WSL2 Linux environment (ext4 filesystem)
    ├── factory CLI (host-native inside Linux; NOT_IMPLEMENTED)
    ├── systemd --user / Quadlet for persistent trusted services
    ├── trusted Sandbox Supervisor (LEVEL B Linux process)
    ├── trusted provider server (own credentials only)
    └── rootless Podman via Podman Machine
        ├── application runtime containers (LEVEL C packaging)
        └── ephemeral execution sandboxes (LEVEL D candidate)
```

The Windows process is a launcher, not a controller. It owns no Factory authority, scheduling, budgets, workspace allocation, provider session, credential store or quality decision. A Windows-native Core or Worker is explicitly out of scope.

The Supervisor runs as a Linux process inside WSL2. The initial implementation target is the WSL2 Factory distribution that hosts the Linux CLI; running the Supervisor inside the Podman Machine Linux environment is an alternative topology that requires its own placement assessment. Neither choice is implemented. What is forbidden is running the Supervisor as a Windows service or process.

WSL2 provides a VM boundary around the Linux environment, but it is not thereby a certified Factory isolation boundary. Containers inside Podman Machine still share the Linux kernel, and no `LEVEL E` claim follows.

## Podman Machine requirements

- Use Podman Machine with the WSL2 backend and rootless Podman.
- Use the same digest-pinned `linux/amd64` application runtime images as Linux. Do not build a Windows-native image or a second image family for Windows.
- Keep the Linux runtime, provider servers and Supervisor inside WSL2. Do not create a Windows-native provider bridge that exposes credentials or tool execution outside Linux.
- Use systemd user services and Quadlet for persistent trusted services inside the Linux environment. If the chosen distribution does not provide a working systemd user session, that lifecycle topology is `NOT_TESTED` and requires a separate assessment.
- Do not use Windows containers, a Windows Docker daemon, host networking, a host Podman/Docker socket mount or privileged containers as a shortcut.
- Do not assume that Podman Machine startup, port forwarding, machine networking, cgroup limits or cleanup behave identically to a Linux host. Each is `NOT_TESTED`.

No specific Podman Machine version, WSL version or Windows build is approved by this guide. The exact combinations must be recorded and tested before a support claim.

## Host path and workspace policy

Windows and Linux path semantics are not interchangeable. A mounted path succeeding is not evidence that it is a safe Factory workspace.

| Use | Path shape | Status |
|---|---|---|
| Linux workspace inside WSL2 ext4 | `/home/<user>/src/software-factory` inside the distribution | Intended; `NOT_IMPLEMENTED` policy enforcement |
| Windows view of the same WSL2 path | `\\wsl.localhost\<Distro>\home\<user>\src\software-factory` | Display/editor view only; must resolve inside WSL2 ext4 |
| Windows drive | `C:\...`, `\\?\C:\...` | Rejected |
| Windows drive mounted into Linux | `/mnt/c/...` | Rejected |
| Windows user profile | `C:\Users\<user>\...` | Rejected; never a host HOME mount |
| Synchronized or network folder | OneDrive, mapped drive, UNC share | Rejected |
| Unsafe symlink | A path that resolves outside the approved Factory root | Rejected |

Factory's intended behavior is:

- choose a canonical Linux workspace root inside WSL2 ext4;
- materialize one workspace per Attempt under a Factory-owned root;
- record ownership and lifetime before execution;
- reject `C:\`, `/mnt/c`, host HOME, Windows profiles, network shares, unsafe symlinks and overlapping roots;
- clean the workspace and reconcile orphans after success, failure, cancellation or crash;
- never reuse a mutable workspace across Attempts or projects.

The Windows launcher must not translate `C:\...` into `/mnt/c/...` or pass a Windows drive into a container. It may display a Windows path for user convenience only. The path policy, materialization and cleanup are `NOT_IMPLEMENTED`; existing Rust worktree allocation is not this policy.

Do not use Podman Desktop or Docker Desktop file sharing to inject `C:\` into a Linux container as the Factory workspace. That configuration is rejected even if the runtime permits it.

## Windows launcher

The launcher is deliberately small. It may:

- check that the required WSL2 distribution exists and is running;
- start or verify the Podman Machine;
- invoke the Linux `factory` binary with arguments and the caller's working directory mapped to a WSL path only when that path is inside WSL2 ext4;
- stream stdout/stderr and return the exit code;
- open logs or diagnostics locations without reading secrets.

It must not:

- implement Core, Worker, scheduling, authorization, budgets, workspace allocation, quality or delivery logic;
- hold provider, GitHub, Core or Worker credentials;
- persist provider authentication or copy Windows credential stores into Linux;
- pass secrets on the command line where process inspection can read them;
- expose Windows drives, WSL interop or a Windows shell to untrusted execution;
- silently fall back to a Windows-native process or host networking when WSL2 or Podman fails.

The launcher's implementation language is not selected by this ADR. A small Rust Windows-target launcher is allowed; a second .NET, PowerShell or Node controller is not implied. No launcher exists.

## Networking

Four loopback scopes are distinct and must not be conflated:

1. Windows host loopback;
2. WSL2 Linux loopback;
3. Podman Machine VM loopback;
4. container loopback.

Rules:

- Do not use host networking, host PID/IPC namespaces, privileged containers or a runtime socket mount. Do not assume WSL2 mirrored networking or automatic Windows `localhost` forwarding.
- Do not assume that `localhost` in Windows reaches a service in WSL2 or a container. Do not assume that a service bound to `127.0.0.1` inside a container is reachable through a published port.
- Expose a service only through an explicit, authenticated endpoint. The default intended publication is Windows loopback only, after verifying Podman Machine port forwarding.
- Do not publish a Factory API to the LAN, a corporate network or a public interface by default. Do not add a Windows Defender Firewall rule for broad access as a convenience.
- Untrusted sandboxes use default-deny egress where practical, with explicit destinations. They must not reach Core, SQLite, provider control ports, the Windows host, the host LAN or metadata services.
- Disable or avoid WSL interop and Windows drive access for untrusted execution. A container must not invoke Windows binaries or read Windows user files.

Containerized Core exposure requires explicit `FACTORY_CONTAINER_MODE=1` and loopback publication. Windows port forwarding, firewall behavior and loopback reachability are `NOT_TESTED`.

## Credentials

- Do not mount or copy `C:\Users\<user>`, the Windows HOME equivalent, SSH keys, cloud configuration, GitHub configuration, provider authentication directories or Windows Credential Manager material into WSL2 or a container.
- Do not automatically bridge Windows Credential Manager, DPAPI, Windows Hello or browser sessions into the Linux runtime.
- Do not put tokens in launcher arguments, scripts, environment dumps, logs, `devcontainer.json`, image layers or repository files.
- Provider authentication belongs to the trusted Linux provider server inside WSL2. It stays outside untrusted execution.
- Application runtime credentials are scoped to one service and injected at runtime. The mechanism is `NOT_IMPLEMENTED`.
- Untrusted sandboxes receive no credentials, no Windows drives and no WSL interop. If a task requires a credential, it is `BLOCKED` rather than run in a credential-bearing sandbox.

## CLI lifecycle

There is one Rust `factory` CLI. On Windows:

- the Linux binary is the single implementation;
- the Windows launcher invokes it through WSL2;
- `factory doctor` is the intended preflight for WSL2, Podman Machine, rootless mode, path policy, image digests, port exposure and toolchain versions;
- `factory` fails closed if WSL2, Podman Machine, a safe workspace or a required credential boundary is unavailable.

The Linux `factory` lifecycle binary exists; the Windows launcher and WSL integration are `NOT_IMPLEMENTED`/`NOT_TESTED`.

## Setup checklist

Commands below are illustrative operator steps. None is a Factory command, none is automated, and none was run on Windows.

| Procedure | Status | Notes |
|---|---|---|
| `wsl --install -d <Distro>` | `NOT_IMPLEMENTED` as Factory automation; `NOT_TESTED` | Windows/WSL setup is operator work |
| `podman machine init` and `podman machine start` | `NOT_IMPLEMENTED` as Factory automation; `NOT_TESTED` | Must use the WSL2 backend; exact options unverified |
| `wsl.exe -d <Distro> --exec /usr/local/bin/factory doctor` | `NOT_IMPLEMENTED` | The launcher and binary do not exist |
| `factory doctor` | `NOT_IMPLEMENTED` | Intended unified preflight |
| `factory <subcommand>` | `NOT_IMPLEMENTED` | Every unified CLI command |

Every inline `factory` or `factory doctor` reference in this guide is the unified CLI and is `NOT_IMPLEMENTED`. The launcher does not exist, and no Windows command is wrapped by Factory.

Intended order, without a claim that it works:

1. Install WSL2 and a Linux distribution.
2. Install Podman Machine with the WSL2 backend and confirm rootless Podman inside the Linux environment.
3. Clone the repository inside WSL2 ext4. Do not clone to `C:\` or `/mnt/c`.
4. Open the WSL2 folder with a Development Containers-capable editor or the Dev Containers CLI. The workspace folder is `/workspaces/software-factory`.
5. Build the development image and run the repository gates from [Container-first development](../engineering/container-first-development.md).
6. When implemented, run `factory doctor` from Windows through the launcher and fail closed on any error.

## Verification obligations

Windows must not be described as implemented, verified or production-ready until all of the following are exercised on a recorded Windows 11 host and exact candidate:

1. Record the Windows build, WSL version, Linux distribution, Podman Machine version and rootless status.
2. Start the launcher and Linux `factory` binary; verify argument mapping, exit codes and failure behavior.
3. Confirm that `C:\`, `\\?\C:\`, `/mnt/c`, Windows profiles, OneDrive, network shares and unsafe symlinks are rejected without fallback.
4. Build and run the same image digest used on Linux; record any architecture or runtime difference.
5. Prove an authenticated service is reachable only through the intended loopback publication, and that Windows, WSL2, Podman Machine and container loopbacks are not conflated.
6. Prove untrusted execution has no provider, GitHub, Core or Worker credentials, no Windows drives, no WSL interop and no unintended egress.
7. Crash, restart and reconcile the launcher, Podman Machine and sandbox; verify no orphaned process, container or workspace is left behind.
8. Repeat the journeys with an independent verifier on the frozen candidate before `INDEPENDENTLY_VERIFIED`.

## Non-claims

This guide does not claim:

- that Windows 11 is supported or that any Windows journey works;
- that WSL2 or Podman Machine is installed, configured or rootless on any host;
- that the same image runs on Windows;
- that the Windows launcher, `factory` CLI, `factory doctor`, path policy or Supervisor exists;
- that a WSL2 VM or container is an isolation boundary or that credentials are confined;
- that a Windows container E2E journey passed;
- that Windows port forwarding, firewall behavior or loopback reachability is known;
- that any independent verification occurred.

Those remain `NOT_IMPLEMENTED`, `NOT_TESTED` or `BLOCKED` as stated above.
