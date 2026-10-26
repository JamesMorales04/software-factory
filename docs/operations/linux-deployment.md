# Linux deployment direction

**Status:** Target design from [ADR-0019](../architecture/decisions/ADR-0019-container-first-cross-platform.md). The development-container image was built and its Rust gates passed locally; no application runtime image, Quadlet unit, service or end-to-end container journey has been run. This guide is not a tested runbook and does not certify isolation or credential confinement.

## Scope

This guide describes the intended Linux deployment shape for Factory:

- rootless Podman in the developer/operator user session;
- systemd user services and Quadlet for persistent trusted application runtime containers;
- a trusted Sandbox Supervisor running as a host Linux process;
- ephemeral, credential-free execution sandboxes launched per Attempt;
- a single host-native Rust `factory` CLI, implemented for `doctor/start/status/logs/stop`. Installer, Quadlet, restart and update commands remain `NOT_IMPLEMENTED`.

It does not describe a rootful deployment, a multi-host cluster, a production hardening baseline, a public service, or a Windows host. Windows is covered by [Windows 11 with Podman Machine](windows-podman-machine.md).

## Current status

| Item | Status | Notes |
|---|---|---|
| Linux rootless Podman + systemd/Quadlet topology | `SUPPORTED` target; `NOT_TESTED` | No unit was installed or started |
| Quadlet units and installer | `NOT_IMPLEMENTED` | `factory start` manages detached host processes; no systemd units are shipped |
| Trusted Sandbox Supervisor service | `IMPLEMENTED` | `factory-supervisor` is started by `factory start` when project roots are configured |
| Persistent application runtime containers | `IMPLEMENTED` | `deploy/Containerfile.core` builds the Core/Office image |
| Containerized Core API exposure | `IMPLEMENTED` | Loopback-published container with explicit `FACTORY_CONTAINER_MODE=1` |
| Unified `factory` CLI and `factory doctor` | `IMPLEMENTED` | Lifecycle subset only; installer/restart/update remain unimplemented |
| Managed live route configuration | `IMPLEMENTED` | Optional strict `0600` Core model and Worker provider env files are preflighted, passed only to the intended process, and fingerprinted for Core recreation; Codex coding remains `BLOCKED` |
| Ephemeral sandbox isolation and credential confinement | `BLOCKED` | SEC-01 is paused; rootless Podman is not certified |
| Linux container E2E | `AUTHOR_VERIFIED` | Core/Office image, factory start/stop/status and fixture Campaign run on the tested CachyOS host |
| Independent verification | None | No independent verifier ran this topology |

### Command status

Every command-shaped procedure in this guide is listed here. None is a Factory implementation.

| Procedure or command | Status |
|---|---|
| `systemctl --user daemon-reload` | `NOT_IMPLEMENTED` as Factory automation; `NOT_TESTED` operator procedure |
| `systemctl --user enable --now factory-service.service` | `NOT_IMPLEMENTED` as Factory automation; `NOT_TESTED` operator procedure |
| `loginctl enable-linger "$USER"` | `NOT_IMPLEMENTED` as Factory automation; `NOT_TESTED` host policy |
| `podman` container lifecycle and `podman info` | `NOT_IMPLEMENTED` as Factory automation; host CLI exists, `NOT_TESTED` for this topology |
| `factory doctor` | `IMPLEMENTED`; lifecycle installation and Windows are not covered |
| `factory <subcommand>` | Partially implemented: doctor/start/status/logs/stop |

Inline `factory` references use the implemented lifecycle subset unless a row says `NOT_IMPLEMENTED`. Quadlet/installer and Windows automation remain unimplemented.

## Target topology

```text
Linux user session
├── systemd --user
│   ├── Quadlet: trusted application runtime container(s)
│   │   └── explicit state volume, scoped credentials, loopback-only publication
│   ├── Quadlet: trusted provider server (optional, own credentials only)
│   └── user service: factory-sandbox-supervisor (trusted host process, LEVEL B)
└── rootless Podman
    ├── persistent application runtime containers (LEVEL C packaging)
    └── ephemeral execution sandboxes (LEVEL D candidate, per Attempt)
```

Core may remain a host-native trusted process or be packaged as a trusted application runtime container. Container packaging does not change its authority or trust level. Core remains the sole writer of its SQLite database. The Sandbox Supervisor remains outside the containers it launches.

The development container described in [Container-first development](../engineering/container-first-development.md) is **not** part of this topology. It is a trusted developer workspace, not a deployment unit or execution sandbox.

## Placement mapping

| Component | Intended placement | Trust and credentials | Status |
|---|---|---|---|
| Factory Core | `LEVEL B` host process, or `LEVEL C` trusted container packaging | Owns authority, state, budgets, gate and delivery; sole SQLite writer | `IMPLEMENTED` container image and loopback publication |
| Sandbox Supervisor | `LEVEL B` host Linux process | Trusted infrastructure; Worker session credential and scoped Podman launcher capability; no provider or GitHub credentials by default | `IMPLEMENTED`; not isolation-certified |
| Provider server | `LEVEL B` host process or `LEVEL C` trusted container | Own provider credential only; no GitHub, Core or delivery token | `NOT_IMPLEMENTED`; provider coding fail-closed |
| Application runtime service | `LEVEL C` persistent container | Trusted service; minimal image; scoped credentials | `IMPLEMENTED` for Core/Office; not production-certified |
| Per-Attempt execution | `LEVEL D` ephemeral rootless container | Untrusted; no secrets, host paths or runtime socket | `BLOCKED` until independently verified |
| Development container | Trusted developer environment, outside runtime placement | No production credentials by default | Configuration `IMPLEMENTED`; image + in-container Rust gates `LOCALLY_VERIFIED` with host-network caveat; Office build and default networking `NOT_TESTED` |

The [service placement policy](../architecture/service-placement-policy.md) remains authoritative. A container or Pod does not create a trust boundary, and `LEVEL C` is not a security upgrade.

## Prerequisites

The target host is expected to provide:

- a Linux distribution with a working systemd user session;
- rootless Podman with Quadlet support;
- user-level cgroup delegation sufficient for bounded containers;
- a Linux filesystem with POSIX semantics for repositories, state and per-Attempt workspaces;
- a local loopback interface for restricted API publication;
- a host-native Rust toolchain only if the operator builds Factory locally; the intended development path uses the pinned development container.

Distribution-specific details such as SELinux labeling, AppArmor, cgroup delegation and rootless storage drivers are not assumed. They are `NOT_TESTED` and must be verified on the exact host. Podman 6.1.4 rootless was observed in the author environment during earlier unrelated work, but that observation is not evidence for this topology.

## Persistent services with Quadlet

Persistent services are trusted, long-lived application runtime containers. They are not sandboxes. A Quadlet unit must:

- run rootless in the operator's user session;
- use a digest-pinned image;
- run as a non-root container user where practical;
- use a read-only container root filesystem where the application permits it;
- drop capabilities and set `no-new-privileges`;
- bound memory and PIDs;
- mount only an explicit, owned state volume;
- publish only an explicit loopback endpoint;
- use a dedicated Podman network rather than host networking;
- never mount the host Podman/Docker socket, host HOME or host device nodes.

Illustrative shape only:

```ini
# NOT_IMPLEMENTED: illustrative Quadlet unit. No unit is shipped, installed,
# syntax-validated or tested. Replace the placeholder image and check every
# key on the target Podman/systemd version.
[Unit]
Description=Factory trusted application runtime (placeholder)

[Container]
Image=localhost/factory-service@sha256:REPLACE_WITH_DIGEST
ContainerName=factory-service
User=1000:1000
ReadOnly=true
NoNewPrivileges=true
DropCapability=ALL
PidsLimit=256
Memory=1g
Volume=factory-state:/var/lib/factory:U
PublishPort=127.0.0.1:8080:8080
Network=factory.network

[Install]
WantedBy=default.target
```

The referenced `factory.network` unit is not shown and does not exist. The example's application must listen on a container interface address that the published port can reach; a process bound only to container `127.0.0.1` will not receive published traffic. That is one reason the current loopback-only Core cannot simply be containerized.

### Lifecycle procedure

```sh
# NOT_IMPLEMENTED: no Factory installer or unit is shipped. This is an
# illustrative operator procedure and is NOT_TESTED on this topology.
systemctl --user daemon-reload
systemctl --user enable --now factory-service.service
loginctl enable-linger "$USER"
```

The exact service name, unit location, linger policy, boot ordering and restart behavior are `NOT_IMPLEMENTED`. A user service that must survive logout requires lingering, which is a host policy decision and must be reviewed. Rootful system services are not approved by this guide.

## Networking

Factory does not use host networking as a shortcut.

- Do not use `--network=host`, host PID/IPC namespaces, `--privileged` or a container-runtime socket mount.
- Container loopback is not host loopback. A service must bind an address reachable through the chosen publication, and the host must publish only the intended endpoint.
- Persistent services publish on host loopback (`127.0.0.1`) by default. Do not publish to `0.0.0.0`, a LAN interface or a public interface without explicit authorization.
- Authenticate the API endpoint. Loopback is a locality restriction, not authentication.
- Untrusted sandboxes use default-deny egress where practical, with explicit destinations. They must not reach Core, the SQLite volume, provider control ports, the host LAN or metadata services.
- Provider servers and application runtime services use a dedicated network. Untrusted execution does not join that network or a shared Pod with those services.

The containerized Core accepts `0.0.0.0` only with explicit `FACTORY_CONTAINER_MODE=1`; the CLI publishes it on loopback only. Do not use host networking or a broadly exposed proxy.

## Persistent state

- Core owns the authoritative SQLite database and is its only writer.
- The database volume is never mounted into a provider server, development container or execution sandbox.
- Use an explicit named or owned volume with restrictive permissions. Network filesystems are not approved without separate evidence.
- Back up while the service is stopped or through a transactionally consistent mechanism. A file copy of a live SQLite database is not automatically a valid backup.
- Migrations are versioned and run by the owning Core/operator process. Migration automation and rollback are `NOT_IMPLEMENTED`.
- Deleting a container is not proof that its state, external sessions or workspaces were cleaned up. Orphan reconciliation is required.

## Credentials

- Do not mount the host HOME or individual host configuration directories.
- Do not put tokens in Quadlet unit files, repository files, image layers, environment dumps or logs.
- Use a scoped runtime secret mechanism for application runtime credentials. The exact Podman/systemd integration is `NOT_IMPLEMENTED`.
- Keep provider credentials in the trusted provider server. A provider server may hold its own provider credential but not GitHub, Core, Worker or delivery tokens.
- Untrusted sandboxes receive no credentials. If a required action cannot run credential-free, it is `BLOCKED`; it is not moved into a credential-bearing container.
- The Supervisor may hold a Worker-scoped session credential and a scoped launcher capability. It must not hold provider or GitHub credentials by default.

## Sandbox Supervisor

The trusted Sandbox Supervisor is a host Linux process at `LEVEL B`. It is not a container, not the development container and not an execution sandbox.

It is responsible for:

- launching one ephemeral container per Attempt or bounded unit;
- providing only the scoped workspace, resource bounds, explicit egress and image digest;
- withholding provider, GitHub, Core and Worker credentials from the sandbox;
- recording container identity before launch;
- reconciling and removing orphaned containers after a crash;
- refusing to fall back to host execution when Podman or policy is unavailable.

`factory start` manages the Supervisor/Worker process lifecycle with private pid/token/log files. `factory start` is not a systemd/Quadlet installer. Provider credential provisioning is limited to the explicit private `FACTORY_PROVIDER_ENV_FILE` and `FACTORY_CORE_MODEL_ENV_FILE` files validated by the CLI; general secret-store integration is not implemented. Rootless Podman remains a candidate runtime and is not certified isolation. Credentialed untrusted execution is `BLOCKED`.

## Upgrade and rollback

The intended procedure is:

1. Pin the new image by digest and record the previous digest.
2. Stop the affected trusted service.
3. Back up authoritative state using a consistent method.
4. Apply reviewed migrations through the owning component.
5. Start the service and verify its authenticated loopback endpoint.
6. Roll back the image digest and the database backup if verification fails.

Rolling-upgrade automation remains `NOT_IMPLEMENTED`. There is no automatic health-based promotion and no claim that rollback is safe without a tested migration and backup path.

## Verification obligations

Before claiming this topology works:

1. Install and start a rootless Quadlet unit on a named Linux distribution and record the Podman/systemd versions.
2. Prove the service is reachable only through its explicit authenticated loopback publication, and that container loopback is not treated as host loopback.
3. Prove the service cannot reach or mount the host HOME, host database, provider credentials, runtime socket or another project's workspace.
4. Crash and restart the service; verify SQLite ownership, migration state and orphan cleanup.
5. Verify the Supervisor launches, bounds, reconciles and removes an ephemeral sandbox, including after a crash.
6. Run the negative tests in [Isolation, credentials and trust boundaries](../security/isolation.md).
7. Repeat the journeys on the exact candidate with an independent verifier before any `INDEPENDENTLY_VERIFIED` claim.

## Non-claims

This guide does not claim:

- that a Linux deployment exists;
- that rootless Podman, Quadlet, systemd, SELinux/AppArmor or cgroups are configured correctly;
- that a container is an isolation boundary or that credentials are confined;
- that the Core API is safely containerized;
- that the Supervisor, `factory` CLI, `factory doctor` or migrations are implemented;
- that any container E2E journey passed;
- that Windows behavior is known.

Those remain `NOT_IMPLEMENTED`, `NOT_TESTED` or `BLOCKED` as stated above.
