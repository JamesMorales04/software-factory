# Container-first development

> **Current workflow:** [container-native engineering](container-native-engineering.md)
> and `compose.dev.yaml` define the permanent containerized development path.
> This document remains the development-container design, trust and security
> rationale. Some older "not tested" notes below are historical; the current
> Slice 1 evidence and the container-native policy are authoritative.

**Status:** Documented direction from [ADR-0019](../architecture/decisions/ADR-0019-container-first-cross-platform.md), now governed by [ADR-0025](../architecture/decisions/ADR-0025-container-native-and-office-parity.md). The development image contains the pinned Rust, Node, SQLx and Playwright toolchains; the trusted Compose `dev` service is the supported contributor workflow. This guide does not certify an application runtime, production deployment or execution sandbox.

## Purpose and non-goals

The development container provides a reproducible, trusted contributor workspace with the pinned Rust, Node and SQLx toolchains. It is for editing, formatting, linting, building and testing known repository code.

It is **not**:

- the Factory application runtime;
- an untrusted execution sandbox;
- a credential boundary;
- a substitute for the trusted Sandbox Supervisor;
- a production deployment unit;
- evidence that the native Rust branch is complete.

Running code from an untrusted pull request or model-directed code inside this container is `TRUSTED_LOCAL`-class activity, not sandboxed execution. Factory untrusted work must use an ephemeral execution sandbox under the [service placement policy](../architecture/service-placement-policy.md) and [isolation policy](../security/isolation.md).

## Current status

| Item | Status | Notes |
|---|---|---|
| `.devcontainer/devcontainer.json` and `.devcontainer/Containerfile` | `IMPLEMENTED` | Pinned development image and Compose `dev` service |
| Development image build | `LOCALLY_VERIFIED` | `podman compose -f compose.yaml -f compose.dev.yaml build dev`; image `43cebaa1770f...` at Slice 1 candidate `a75d4c3` |
| Development container run on Linux | `LOCALLY_VERIFIED` | `cargo xtask check-container`, Office typecheck/lint/build and real Chromium journey passed inside the image; default container networking used, no host-network workaround |
| Development container run on Windows 11 / WSL2 | `NOT_TESTED` | No Windows host was available |
| Repository gates inside the container | `LOCALLY_VERIFIED` | fmt, strict Clippy, workspace tests excluding host-userns/bwrap fixtures, build, docs, audit; Office build and Chromium journey pass |
| Bubblewrap boundary tests inside the container | `BLOCKED` in the development image | The container cannot mount the nested proc/user namespace; the host Rust CI lane retains these tests |
| Unified `factory` CLI and `factory doctor` | `IMPLEMENTED` | Linux lifecycle subset exists; installer/restart/update and Windows launcher are not |
| Untrusted execution in this container | Forbidden; sandbox path `BLOCKED` | SEC-01 remains paused and uncertified |
| Independent verification | None | No independent verifier ran this slice |

## What the image contains

The `Containerfile` is a multi-stage build:

| Component | Pin | Source |
|---|---|---|
| Rust | 1.90.0 | `rust-toolchain.toml`; official `rust:1.90.0-bookworm` base |
| Rust components | `clippy`, `rustfmt`, `rust-analyzer`, `rust-src` | Declared by `rust-toolchain.toml` and added by the Containerfile |
| Node.js and npm | 22.23.2 | `.nvmrc`; copied from the pinned official Node image |
| SQLx CLI | 0.8.6 | Containerfile build step with SQLite and rustls features; see the command table |
| Git and build tools | Debian Bookworm packages | `git`, `build-essential`, `pkg-config`, `libssl-dev`, `libsqlite3-dev`, `ca-certificates`, `curl`, `sqlite3`, `bubblewrap`, `iproute2` |
| Playwright and Chromium | 1.64.1 | Development-only browser acceptance; downloads Chromium into the factory user's cache |
| OpenCode / Codex CLIs | 1.18.32 / 0.156.1 | Development-only provider compatibility probes; same pins as the provider image |
| Go | Intentionally absent | ADR-0018 selects Rust; the legacy `mise.toml` Go pin is not installed here |

The base images are pinned by tag, not digest, and Debian packages are not snapshot-pinned; byte-reproducible builds are `NOT_IMPLEMENTED`. The build context is the repository root so the Containerfile can read `.nvmrc`. A build-time check fails if `.nvmrc` and the `NODE_VERSION` build argument disagree. The image creates a non-root `factory` user (UID/GID 1000 by default, configurable at build time) and uses `/workspaces/software-factory` as the workspace folder.

The image installs the pinned provider CLIs for compatibility tests, Playwright Chromium and `cargo-audit`. It does not install provider credentials, GitHub tokens, cloud credentials or a host runtime socket. The provider CLIs are development tooling, not product runtime authority.

## Development container versus runtime versus sandbox

| Property | Development container | Application runtime container | Ephemeral execution sandbox |
|---|---|---|---|
| Trust | Trusted contributor environment | Trusted service | Untrusted |
| Filesystem | Writable workspace and tool caches | Minimal read-only root where practical, explicit state volume | Disposable writable workspace only |
| Toolchain | Compilers, package managers, editors | Runtime only; no compilers | Task-specific image; no secrets |
| Credentials | None by default | Only the service's scoped credentials | None |
| Network | Package registries and normal developer access | Explicit authenticated endpoint and egress | Default-deny or explicit bounded egress |
| Lifetime | Developer session | Persistent service | One Attempt, then removed |
| Socket | No host Podman/Docker socket by default | No runtime socket | No runtime socket |
| Claim | Not a sandbox | `LEVEL C` packaging, not a trust upgrade | Candidate `LEVEL D`; not certified |

A development container is allowed to compile repository code because the contributor is trusted to choose what to run. That fact is exactly why it cannot be used to execute untrusted code for Factory.

## Workspace and host-path rules

- The workspace folder is `/workspaces/software-factory`.
- On Linux, the checkout may live on a normal Linux filesystem with POSIX semantics.
- On Windows 11, the checkout must live in the WSL2 ext4 filesystem, for example `/home/<user>/src/software-factory` inside WSL. From Windows it may be viewed through `\\wsl.localhost\<Distro>\home\<user>\src\software-factory`. The Windows view is not the execution path.
- Do not open, mount or bind `C:\...`, `\\?\C:\...`, `/mnt/c/...`, a Windows user profile, OneDrive, a network share or a mapped drive as the Factory workspace. Factory rejects these configurations even though a container runtime might be able to mount them.
- Do not mount the host HOME or individual host configuration directories. The container has its own `/home/factory`.
- Factory-managed per-Attempt workspaces are materialized and cleaned by the Supervisor on the host. The development container does not provide that boundary.

The reason is not cosmetic. Windows-backed Linux mounts have different permission, case-sensitivity, symlink, inode and performance behavior, and they collapse the workspace boundary Factory needs for per-Attempt ownership and cleanup. A successful bind mount is not evidence that the path policy is safe.

## Trust, credentials and networking

- The container is trusted and writable. It is not isolated from the developer's own actions and has no credential confinement claim.
- No host HOME, SSH key, cloud configuration, GitHub configuration, Codex/OpenCode authentication directory or Windows credential store is mounted or copied.
- No token or secret belongs in the image, `devcontainer.json`, Containerfile, build arguments, repository files or logs.
- Provider authentication belongs to a trusted provider process outside untrusted execution. A developer authenticating a provider manually inside the development container is performing a trusted local action outside Factory governance; Factory must not depend on it.
- The container uses normal outbound networking to fetch packages. That access is one reason it is not a sandbox. Do not expose Factory APIs from the development container, do not use host networking, and do not treat container loopback as host loopback.
- For published services, the default is an explicit authenticated endpoint on host loopback only. Broad LAN exposure requires separate authorization.

## Optional Podman socket

The host Podman/Docker socket is **not** mounted by default. `devcontainer.json` declares no mounts, and the Containerfile does not create or expose a runtime socket.

A socket mount would let any process in this trusted development container control the developer's rootless container runtime, including container creation, mounts and network publication. It would also blur the boundary between the development container and the untrusted sandbox launcher. It is therefore excluded from the committed configuration and CI.

If a trusted developer needs a local nested-container experiment, any socket mount must be an uncommitted local override made after a placement assessment. It must never be enabled for untrusted code, agent-generated code or CI. The trusted Sandbox Supervisor is the only intended launcher for Factory execution sandboxes, and it is a host Linux process, not this development container. No socket-mount path is provided or verified; it remains out of policy for this container.

## Commands

The canonical commands now run through the trusted `dev` service:

```sh
podman compose -f compose.yaml -f compose.dev.yaml build dev
podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev cargo xtask check-container
podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev \
  sh -c 'npm --prefix office ci && npm --prefix office run typecheck && npm --prefix office run lint && npm --prefix office run build'
podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev \
  sh -c 'npm --prefix office ci && npm --prefix office/e2e-browser ci && cargo xtask e2e office'
```

The table below records historical status and is not the current command
authority. Inline `factory` references use the implemented lifecycle subset
unless marked otherwise.

| Command | Status | Notes |
|---|---|---|
| `cargo install sqlx-cli --version 0.8.6 --locked --no-default-features --features rustls,sqlite` | `IMPLEMENTED` as a Containerfile build step | Image build `NOT_TESTED` |
| `rustup component add clippy rustfmt rust-analyzer rust-src` | `IMPLEMENTED` | Declared by `rust-toolchain.toml`; the Containerfile installs the same set |
| `cargo fmt --all -- --check` | `IMPLEMENTED` | Repository gate; not run in this change |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | `IMPLEMENTED` | Repository gate; not run in this change |
| `cargo test --workspace --locked` | `IMPLEMENTED` | Repository gate; not run in this change |
| `cargo build --bins` | `IMPLEMENTED` | Repository build; not run in this change |
| `cargo sqlx migrate run --source migrations` | `IMPLEMENTED` | Documented repository workflow; requires `DATABASE_URL`; SQLx CLI 0.8.6 is installed by the image; not run in this change |
| `cargo audit --file Cargo.lock` | `IMPLEMENTED` and run in the container | `cargo-audit` 0.22.2 is installed by the Containerfile; `cargo xtask check` passed |
| `factory doctor` | `IMPLEMENTED` | Host preflight exists; Windows/WSL not tested |
| `factory <subcommand>` | Partial | doctor/start/status/logs/stop; install/restart/update/dev remain unimplemented |
| `podman build` / `podman run` for this configuration | `LOCALLY_VERIFIED` with host-network caveat; not Factory automation | Author run on Podman 6.1.4 rootless: default build network failed (`pasta failed ... /dev/net/tun`), `--network=host` succeeded; container also run with `--network=host` |

No `go` command is part of this container or the supported workflow.

## Editor and language server

The development container includes the `rust-lang.rust-analyzer` extension and
the pinned `rust-analyzer` toolchain component. Open the repository root; the
extension reads `rust-toolchain.toml` and uses the 1.90.0 sysroot and component
set. `rust-src` enables standard-library source navigation without a custom
sysroot path.

To select the pinned component server explicitly in VS Code/Code OSS, set
`rust-analyzer.server.path` in **your user or workspace settings** to the
output of `rustup which rust-analyzer`. Do not commit an absolute path or a
global editor setting. If the extension uses its bundled server instead, keep
the extension and the pinned toolchain updated together; both read the same
workspace and `Cargo.lock`. See [editor setup](editor-setup.md).

## Intended workflow

1. Clone the repository on a Linux filesystem, or inside WSL2 ext4 on Windows. Do not use `C:\`, `/mnt/c` or a synchronized Windows folder.
2. Open the repository folder with a Development Containers-capable editor or the Dev Containers CLI, using the committed configuration.
3. Let the editor build `.devcontainer/Containerfile` from the repository-root context. The workspace folder must be `/workspaces/software-factory`.
4. Run the repository gates from the command table on the frozen candidate.
5. Keep the development container free of provider and delivery credentials unless a trusted developer explicitly chooses otherwise for a local, non-Factory action.
6. Use the host-native `factory` CLI, when it exists, for runtime lifecycle. Do not treat the development container as the deployment or execution interface.

The Dev Containers CLI and editor integration are external tools. No Factory command wraps them, and their compatibility with this Podman configuration is `NOT_TESTED`.

## Verification gaps

The following remain open and must not be reported as passing:

- Windows 11 / Podman Machine and SELinux/AppArmor hosts were not exercised.
- Base images are tag-pinned rather than digest-pinned; Debian packages are not snapshot-pinned, so byte-reproducible builds are `NOT_IMPLEMENTED`.
- Bubblewrap host-user-namespace boundary tests cannot run inside the development image; the host Rust CI lane retains them.
- The Dev Containers CLI/editor integration is not exercised by this repository.
- The GitHub-hosted container-dev lane uses unpinned runner Podman/Compose packages.
- No independent verifier reviewed this slice; the author-run evidence is not independent acceptance.

These gaps are `NOT_TESTED`, `BLOCKED` or `NOT_IMPLEMENTED`, not `PASS`. The development container remains a trusted contributor environment, not an application runtime or sandbox.
