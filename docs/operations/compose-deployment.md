# Compose deployment and first-run onboarding

**Status:** developer-preview Linux implementation, author-run. Not a
production, Windows or SEC-01 isolation claim. Only the rootless
`TRUSTED_LOCAL` boundary is offered.

## Host prerequisites

- A working **rootless** Podman 6.x installation.
- A supported Compose provider reachable through `podman compose`:
  - `podman-compose` (tested), or
  - Docker Compose with the rootless Podman socket enabled.
- A browser.
- Normal user namespaces, cgroup v2 and container kernel facilities.

On the tested CachyOS host, `podman compose` selected Docker Compose and the
rootless socket was disabled, so the tested command is:

```sh
PODMAN_COMPOSE_PROVIDER=/usr/bin/podman-compose podman compose up -d --build
```

If Docker Compose is the configured provider, enable the socket first:

```sh
systemctl --user enable --now podman.socket
podman compose up -d --build
```

Do not mount the host Podman socket into any service. The product uses its own
nested engine.

## Start

From a clean checkout:

```sh
podman compose up -d --build
```

This builds the images (the first build compiles all binaries once), creates
the private networks and durable volumes, generates per-service secrets,
imports the provider image into the dedicated engine, starts Core/Office,
Supervisor, Worker, verifier, reviewer and the runtime broker, and waits for
health through Compose health checks.

Office is published only on loopback:

```text
http://127.0.0.1:8893
```

Set `FACTORY_PORT` to change the host port. No other service publishes a port.

### First build time

The unified `deploy/Containerfile` compiles Core, compose-init, Supervisor,
Worker, verifier, reviewer, runtime broker, provider bridges and the fixture
provider in one shared builder stage. A cold build is several minutes; later
builds reuse the cache. The provider image contains pinned OpenCode/Codex
executables and Node, so its bundle is large (about 1.5 GiB on the tested host).

## First-run enrollment

The stack starts unconfigured and safe: no provider inference is started. Core
generates a one-time 160-bit enrollment code in the private data volume.

Read it locally without editing a container file:

```sh
podman compose exec core cat /data/enrollment-code
```

Open Office. The setup wizard asks for the code and returns the operator
credential to that browser tab only. The code is rate-limited and removed on
success; a completion marker prevents re-generation after restart. The
long-lived operator token may be read later with:

```sh
podman compose exec core cat /config/operator.token
```

Provider secrets are never stored in browser storage, logs or telemetry.

## Configure providers and models

Office → **Settings → Providers & Models**:

1. Select OpenCode Go or Codex.
2. Set model and base URL (HTTPS; loopback HTTP is test-only).
3. Submit the provider API key. It is encrypted at rest and never returned.
4. Choose budgets and, if intended, the default Core model route.
5. **Test connection** reads only the provider `/models` list. A model list
   does not certify access, pricing or inference capability.
6. **Revoke credential** clears the secret, disables the route and records an
   audit entry.

Semantic Reviewer model calls are separately disabled by default under
**Review & runtime policy**. Enabling them authorizes Core-budgeted model calls
for each candidate review and may incur provider charges. Deterministic review
always runs first.

Codex status is explicit:

- API-key transport is the only supported Codex authentication mode.
- Native ChatGPT OAuth remains **BLOCKED** by the credential boundary.
- Containerized Codex live stdio transport is not implemented; deterministic
  host/bwrap Codex regressions remain.

## Projects, checks and delegations

- **Projects**: register an HTTPS Git URL. Core imports it into managed storage
  and freezes the base commit. Configure allowed paths, capabilities and the
  quality profile; revisions and hashes are visible.
- **Actions**: enter the objective, provider/model route, allowed paths and
  typed checks (format, lint, build, test, functional, negative, browser).
  Commands are previewed; verification requires at least one executable check.
- **Delegations**: create revocable scoped grants with paths, capabilities,
  route, risk classes, budgets, concurrency/expiry and cumulative usage.
  Autonomous start is off by default.
- **Lead**: submit a request, assess with the configured model, inspect and
  freeze the plan, then explicitly authorize the Campaign.
- **Campaigns**: inspect Runs/Attempts, provider sessions, the candidate diff,
  independent receipts, Quality Gate, cost and cleanup.

## Persistence and lifecycle

| Command | Effect |
|---|---|
| `podman compose up -d --build` | Start or update; durable volumes retained |
| `podman compose down` | Stop all services; **retains** data/projects/evidence volumes |
| `podman compose down -v` | Destructive: removes volumes. Require explicit operator confirmation |
| `podman compose logs -f core` | Container-scoped logs |
| `podman compose ps` | Service state |

Durable state includes the Core SQLite database, imported projects,
Supervisor workspaces, verifier/reviewer state, provider sessions and the
dedicated engine storage. A restart preserves the Project, Campaign, candidate
and gate; services wait for init material and the runtime bundle marker before
starting, then converge through health checks.

## Diagnostics

- `podman compose ps` distinguishes created, starting, healthy and exited
  services.
- `podman compose logs <service>` is the first diagnostic.
- Core `/health`, `/setup/status` and authenticated projections identify
  configuration and authorization state.
- Office shows provider connection status, missing-model status, campaign
  errors, gate outcome and cleanup state.
- A missing or invalid prerequisite should be reported before secondary
  connection refusals; transient startup waits are silent.

## Images and provenance

| Component | Base / version |
|---|---|
| Core, Supervisor, Worker, verifier, reviewer | Debian bookworm-slim; Rust 1.90.0 builder |
| Runtime | `quay.io/podman/stable:v5.8.9` + VFS |
| Office build | Node `22.23.2`, React/Vite pinned by `office/package-lock.json` |
| Provider | Node `22.23.2`, OpenCode `1.18.32`, Codex `0.156.1` |

Base images are pinned by version tag. Digest pinning is not enforced; record
local image digests for any certification. The provider image is not published;
the bundle volume transfers its root filesystem to the dedicated engine.

## Windows target design (not claimed)

The intended Windows 11 path is Podman Machine on WSL2 running the same Linux
images and the same Compose stack. The browser would reach the published
loopback port through the Podman Machine forwarding behavior. Windows
installation, WSL networking, path translation, SELinux/AppArmor hosts and
multi-arch builds are **NOT_TESTED**. Do not claim Windows support.

## Unsupported / blocked

- Paid live provider UAT without a fresh Product Owner authorization.
- SEC-01 ISOLATED certification or credential confinement.
- Native ChatGPT-authenticated Codex.
- Containerized Codex stdio transport.
- SELinux/AppArmor, Windows, remote deployment and independent security review.
