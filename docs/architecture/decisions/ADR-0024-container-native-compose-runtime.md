---
id: ADR-0024
status: Accepted for developer-preview implementation
date: 2026-10-26
owner: product-owner
supersedes: null
amends: ADR-0019
---
# ADR-0024: Container-native Compose runtime and Office-first configuration

- **Status:** Accepted for developer-preview implementation by the Product Owner
  campaign directive of 2026-10-26. Compose, phase-zero nested-engine evidence,
  Office onboarding, provider settings, project/check/delegation configuration
  and automatic quality dispatch are implemented and author-run. Independent
  review, a fresh paid-provider UAT, Windows testing and SEC-01 isolation remain
  open.
- **Amends:** [ADR-0019](ADR-0019-container-first-cross-platform.md) adds the
  permanent Compose application topology and the dedicated execution engine.
  ADR-0018, ADR-0009, the service-placement policy and the isolation policy
  remain in force.
- **Security status:** `TRUSTED_LOCAL`. Rootless containerization is not an
  isolation certification. A dedicated nested engine is still a same-kernel
  execution boundary.

## Context

The previous developer-preview lifecycle started Core in a container but ran
the Supervisor and Worker as host processes and required host Rust, Node,
OpenCode/Codex and exact executable paths. That is not an installable product
experience and keeps the host toolchain in the runtime trust path.

The Product Owner approved a target where the only host requirements are a
rootless Podman installation, a supported Compose provider, a browser and
normal kernel container facilities. The complete stack must start with:

```sh
podman compose up -d --build
```

Provider/model selection, credentials, budgets, projects, checks, delegations
and Campaign authorization must be configurable through authenticated Office.

## Decision

### 1. Compose owns the permanent application lifecycle

`compose.yaml` is the canonical deployment. It starts:

- `init` — idempotent first-run material generation and per-service secret
  volume ownership;
- `provider-bundle` — exports the provider image root filesystem into a
  shared volume for the dedicated engine (no host Podman socket, no registry);
- `core` — Core API and the bundled Rust/React Office;
- `supervisor` — containerized trusted workspace/Attempt Supervisor;
- `worker` — containerized trusted Worker;
- `verifier` and `reviewer` — independent quality roles with distinct
  credentials, workspaces and execution identities;
- `runtime` — the dedicated nested Podman engine and its narrow broker.

Only Office is published, on `127.0.0.1`. `podman compose down` keeps durable
volumes; destructive removal requires explicit `-v`.

The old `factory start/stop/status` host-managed development path remains a
developer convenience, not the product installation path. It is not the
authority for Compose behavior and does not satisfy the install target.

### 2. Dedicated engine, narrow broker

The runtime container runs a private Podman instance with VFS storage and no
host Podman socket, `/run/user` mount, host network or `/dev/fuse`. The broker
in that container is the only interface:

- typed volume create/remove/import/export;
- bounded, network-disabled child execution with exact image allowlists,
  named-volume mounts, cgroup CPU/memory/PID limits and dropped capabilities;
- provider session containers with a shared private session subdirectory;
- managed-container reconciliation at startup.

The broker accepts no arbitrary Podman arguments, host paths, images or
environment keys. The inner engine API/socket is never exposed.

### 3. Why a rootful-in-userns engine, not nested rootless-in-rootless

Phase-zero measured that a second rootless engine inside a rootless outer
container cannot use `newuidmap` for its child IDs (`Operation not permitted`).
The selected design is a private **rootful engine inside the rootless outer
user namespace**. From the host it remains rootless; inside the engine it uses
the outer namespace's delegated IDs, VFS storage and cgroupfs.

Working child execution requires the runtime container to have
`CAP_SYS_ADMIN` and `CAP_NET_ADMIN` in addition to the normal root process
capability set, and to remount cgroup2 read-write inside its own cgroup
namespace. `/dev/fuse` is not required because storage is VFS. The container is
never privileged and never receives the host runtime socket. These exceptions
and their residual risks are recorded in
[container runtime security](../../security/container-runtime.md).

### 4. Provider runtime is packaged, not host-installed

The provider image pins OpenCode `1.18.32` and Codex `0.156.1` in a Node base
image and includes the Factory provider/egress bridges and deterministic
fixture provider. Provider sessions run in network-disabled child containers
with only the private session volume; the long-lived provider credential stays
in the trusted Worker's loopback egress broker. A per-run token and non-secret
provider configuration reach the sandbox.

The current container provider entrypoint implements OpenCode and the fixture
adapter. Containerized Codex stdio transport is **not implemented**; Codex
native ChatGPT authentication remains blocked. The existing host/bwrap Codex
deterministic regressions are unchanged.

### 5. Durable configuration and onboarding

Core owns typed provider settings and a semantic-review enable switch in
SQLite. Provider credentials are encrypted with AES-256-GCM under a master key
held only in the Core config volume. Credentials are never returned after
submission, never placed in browser storage, telemetry or logs, and can be
revoked. Model calls remain Core-budgeted; saving a setting or testing a
connection does not start inference.

First-run enrollment is a one-time 160-bit code written to the private data
volume, rate-limited, invalidated by removal plus a completion marker. The
operator completes enrollment in Office. Role secrets are split across
per-service config volumes so Worker, Supervisor, verifier, reviewer and the
runtime broker cannot read one another's credentials or the master key.

### 6. Project and campaign configuration is Office-native

Core imports HTTP(S) Git repositories into managed storage rather than mounting
arbitrary host folders. Office configures allowed paths, capabilities and the
quality profile; edits typed checks; creates and revokes scoped delegations;
and reviews Lead plans, Campaigns, diffs, receipts and gates. Core remains the
only authority for authorization, budgets and acceptance.

### 7. Automatic independent quality

The verifier and reviewer services poll Core with their own tokens. The
verifier executes frozen checks in a network-disabled child container against
the exact candidate. The reviewer runs deterministic review first; semantic
review is an explicit Office-controlled option that requires a configured
default provider and remains Core-budgeted. The deterministic gate accepts only
candidate-bound PASS evidence from both roles.

## Consequences

- A clean checkout requires no host Rust, Node, npm, OpenCode or Codex.
- The first build can take several minutes because all binaries are compiled in
  one shared builder stage; subsequent builds reuse the cache.
- The provider image is large because it contains pinned provider executables
  and Node. It is distributed through the bundle volume instead of a registry.
- The runtime container is the only root process in the stack and deliberately
  carries namespaced `SYS_ADMIN`/`NET_ADMIN`. An engine/broker compromise is
  equivalent to compromise of the execution supervisor, not host root, but it
  is not SEC-01 isolation.
- VFS is slower and larger than overlay; it avoids `/dev/fuse` and
  overlay-on-overlay ambiguity in the nested engine.
- SELinux/AppArmor hosts, Windows/Podman Machine and multi-arch images remain
  untested.
- A paid provider UAT is not authorized by this ADR.

## Alternatives considered

1. **Host Supervisor/Worker plus Compose Core.** Rejected as the install
   experience because it requires host toolchains and exact paths.
2. **Nested rootless-in-rootless engine.** Failed phase-zero `newuidmap`
   mapping; not used.
3. **Host Podman socket mounted into a broker.** Rejected: grants host runtime
   authority and broad storage access.
4. **Privileged runtime container.** Rejected. The selected engine uses only
   namespaced capabilities and mandatory putenv.
5. **Separate engine and broker containers with the native remote API.**
   Deferred: remote volume import/stdin behavior was not reliable in the
   tested Podman API. The broker is co-located with the engine so it can use the
   local CLI; the security boundary is the broker API, not co-location.
6. **Publish custom images to a registry.** Not available for a clean local
   checkout. The bundle-rootfs import is local and auditable.
7. **Keep provider configuration in `.env` files.** Rejected for the normal
   path. Environment remains available for deterministic tests and legacy
   developer mode only.
