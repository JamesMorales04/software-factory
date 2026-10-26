# Container-first development

**Status:** Documented direction from [ADR-0019](../architecture/decisions/ADR-0019-container-first-cross-platform.md). The `.devcontainer` image was built locally and `cargo xtask check`, `cargo xtask migrate` and `cargo xtask e2e recovery` passed inside it on 2026-10-23, using `podman build/run --network=host` because the author host's default rootless networking lacked `/dev/net/tun` (pasta failure). Default container networking, the Office build inside the container, Windows 11/Podman Machine, runtime images, Quadlet and the unified `factory` CLI are **not tested or not implemented**. This guide does not certify a development environment, application runtime or execution sandbox.

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
| `.devcontainer/devcontainer.json` and `.devcontainer/Containerfile` | `IMPLEMENTED` as configuration | Files are present in this change |
| Development image build | `LOCALLY_VERIFIED` with caveat | `podman build --network=host` succeeded; image `4e4e407f0c7a` (author host). Default build network `BLOCKED` by host pasta `/dev/net/tun`; see below. |
| Development container run on Linux | `LOCALLY_VERIFIED` with caveat | `cargo xtask check`, `migrate` and `e2e recovery` passed with `--network=host`; default container networking `NOT_TESTED`/environment-blocked |
| Development container run on Windows 11 / WSL2 | `NOT_TESTED`; Windows verification `BLOCKED` | No Windows host was available |
| Repository gates inside the container | `LOCALLY_VERIFIED` | Full `cargo xtask check` (fmt, Clippy, 107 tests, build, docs, audit) passed; Office build inside the container `NOT_TESTED` |
| Unified `factory` CLI and `factory doctor` | `IMPLEMENTED` | Linux lifecycle subset exists; installer/restart/update and Windows launcher are not |
| Untrusted execution in this container | Forbidden; sandbox path `BLOCKED` | SEC-01 remains paused and uncertified |
| Independent verification | None | No independent verifier ran this change |

## What the image contains

The `Containerfile` is a multi-stage build:

| Component | Pin | Source |
|---|---|---|
| Rust | 1.90.0 | `rust-toolchain.toml`; official `rust:1.90.0-bookworm` base |
| Rust components | `clippy`, `rustfmt`, `rust-analyzer`, `rust-src` | Declared by `rust-toolchain.toml` and added by the Containerfile |
| Node.js and npm | 22.23.2 | `.nvmrc`; copied from the pinned official Node image |
| SQLx CLI | 0.8.6 | Containerfile build step with SQLite and rustls features; see the command table |
| Git and build tools | Debian Bookworm packages | `git`, `build-essential`, `pkg-config`, `libssl-dev`, `libsqlite3-dev`, `ca-certificates`, `curl`, `sqlite3` |
| Go | Intentionally absent | ADR-0018 selects Rust; the legacy `mise.toml` Go pin is not installed here |

The base images are pinned by tag, not digest, and Debian packages are not snapshot-pinned; byte-reproducible builds are `NOT_IMPLEMENTED`. The build context is the repository root so the Containerfile can read `.nvmrc`. A build-time check fails if `.nvmrc` and the `NODE_VERSION` build argument disagree. The image creates a non-root `factory` user (UID/GID 1000 by default, configurable at build time) and uses `/workspaces/software-factory` as the workspace folder.

The image does not install provider CLIs, provider credentials, GitHub tokens, cloud credentials or a Rust toolchain. It also does not install `cargo-audit`; see the command table below.

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

Every command named in this guide is listed in the table below with an explicit status. Inline `factory` references use the implemented lifecycle subset unless marked otherwise. The repository's Rust commands exist and are documented in [native/README.md](../../native/README.md); they were not run for this documentation change.

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

The following are open and must not be reported as passing:

- The image built and the in-container Rust gates passed only with the `--network=host` workaround on this host; default rootless container networking remains unverified and is `BLOCKED` here. Base images are tag-pinned, not digest-pinned. The Office build, application runtime image, Quadlet and Windows path were not exercised.
- The non-root user, workspace ownership and UID/GID mapping with rootless Podman are unverified.
- The `.nvmrc` build-time check has not executed.
- Base images are tag-pinned rather than digest-pinned, and apt packages are not snapshot-pinned.
- No Linux container run, no Windows WSL2 run and no container E2E journey was performed.
- `cargo fmt`, strict Clippy, tests, build, migration and audit were not run inside the image.
- `cargo-audit` is absent from the image.
- No host path, network, credential or socket negative test was executed.
- No independent verifier reviewed this configuration.

These gaps are `NOT_TESTED` or `NOT_IMPLEMENTED`, not `PASS`. The development container remains a trusted convenience environment until candidate-bound evidence says otherwise.
