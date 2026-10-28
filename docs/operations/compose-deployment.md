# Compose deployment and first-run onboarding

**Status:** developer-preview Linux implementation, author-run. Not a
production, Windows or SEC-01 isolation claim. Only the rootless
`TRUSTED_LOCAL` boundary is offered.

## Host prerequisites

- A working **rootless** Podman 6.x installation. The runtime entrypoint and
  broker reject an initial (rootful) outer user namespace before starting the
  engine.
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

Only `podman-compose` was exercised on the tested host. The Docker Compose
through rootless-Podman-socket path is advertised but **NOT_TESTED**.

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
the Core, Supervisor, Worker and runtime health checks. The verifier and reviewer
are started after the runtime is healthy but have no Compose healthcheck.

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

Start and open Office with the host launcher:

```sh
./factory open
```

The launcher selects the installed `podman-compose` provider when the Podman
Compose wrapper would otherwise choose an unavailable Docker-socket provider,
waits for Core health, and only then issues a random, single-use confirmation
code that expires after five minutes. Enter it once in Office to enroll this
browser. It is never put in a URL, request log or browser history. Core stores
only the code digest and expiry, consumes the code on success, and issues a
persistent HttpOnly device session. The session and CSRF cookies are
SameSite=Strict; session tokens are stored only as SHA-256 digests. Browser
mutations require exact Origin validation and a CSRF header. HTTPS origins
receive Secure cookies. Local HTTP is accepted only on loopback.

To recover after losing every enrolled device, run:

```sh
./factory recover
```

This protected local operation revokes all device sessions and issues a new
five-minute confirmation code. The permanent operator credential remains an
internal recovery/API mechanism and is not part of browser onboarding. Existing
Projects, Campaigns, credentials and evidence are retained by the additive
session migration.

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

The base stack defaults to the live OpenCode adapter. If no matching provider
settings are saved, the Worker fails closed rather than running the deterministic
fixture. The fixture adapter is enabled only by the deterministic E2E override
(`FACTORY_PROVIDER_KIND=fixture`); it is not a silent fallback for a live route.

Semantic Reviewer model calls are separately disabled by default under
**Review & runtime policy**. Core rejects semantic model calls and MODEL
reviewer receipts while the policy is disabled. Enabling them authorizes
Core-budgeted model calls for each candidate review and may incur provider
charges. The reviewer binary performs deterministic pre-review before any
model request; Core enforces the policy and budget but cannot prove that
internal ordering against a compromised reviewer token.

Codex status is explicit:

- API-key transport is the only supported Codex authentication mode.
- Native ChatGPT OAuth remains **BLOCKED** by the credential boundary.
- Containerized Codex live stdio transport is not implemented; deterministic
  host/bwrap Codex regressions remain.

## Projects, checks and delegations

- **Projects**: register an HTTPS Git URL. Core imports it into managed storage
  and freezes the base commit. Configure allowed paths, capabilities and the
  quality profile; revisions and hashes are visible.
Provider-route token and cost limits cap Core model calls. The `max_turns`
field is stored as an operator default for future delegation tooling but is not
enforced by Core or the Worker in this iteration. The explicit Campaign
authorization remains authoritative for Implementer/provider sessions and
cannot be widened by provider settings. Per-session disk quota beyond the
64 MiB per-file limit is not implemented.

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

## Deterministic acceptance and browser smoke

The deterministic container journey uses the fixture provider only and never
starts paid inference:

```sh
FACTORY_E2E_KEEP=1 PODMAN_COMPOSE_PROVIDER=/usr/bin/podman-compose \
  bash scripts/compose-e2e.sh
```

`FACTORY_E2E_KEEP=1` leaves the dedicated `software-factory-e2e` stack running.
For this developer-only browser smoke, issue a short-lived local recovery code
and pass it through the environment. The code is captured by the shell variable
and is not printed by these commands:

```sh
CODE="$(podman exec software-factory-e2e_core_1 /usr/local/bin/factory-core issue-recovery-code)"
FACTORY_OFFICE_URL=http://127.0.0.1:8894 \
FACTORY_BOOTSTRAP_CODE="$CODE" \
CHROMIUM_EXE=/absolute/path/to/chrome \
FACTORY_BROWSER_NO_SANDBOX=1 \
  node scripts/compose-agent-office-smoke.mjs
unset CODE
```

The smoke enrolls its isolated browser context through the one-time confirmation
flow, then inspects real Implementer/Verifier/Reviewer/Worker participants,
reads the recorded tool-event projection, preserves participant context across
reload, opens the Campaign WorkGraph (empty or populated), and checks document
overflow at 1440/834/390 px. It writes evidence under
`/tmp/factory-agent-office-smoke` unless `S4A_BROWSER_EVIDENCE_DIR` is set.
The managed-session harness and the existing `cargo xtask e2e office` journey passed against implementation `f2d1be5` together with session
persistence across Core restart, logout revocation, local recovery and a safe
migration copy from schema 23 to 24; see
[personal-access-auth-2026-10-28.json](../evidence/personal-access-auth-2026-10-28.json).
Tailscale remote HTTPS, mobile-data access, paid inference, independent review
and isolation remain **NOT_TESTED**. The short-lived confirmation is for this
local test harness and is not part of normal onboarding.
Tear down the kept stack with:

```sh
podman compose -p software-factory-e2e -f compose.yaml -f compose.e2e.yaml down -v
```

## Diagnostics

- `podman compose ps` distinguishes created, starting, healthy and exited
  services.
- `podman compose logs <service>` is the first diagnostic.
- Provider containers have a broker-side lifetime reaper and a 64 MiB per-file
  limit. Total engine-volume quota is an open hardening item.
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

The container provider entrypoint implements OpenCode and the deterministic
fixture adapter. The Worker fetches the assigned provider route only while the
attempt is `LEASED`, the owning Campaign is `AUTHORIZED`, the Worker holds the
exact lease generation and the authorization deadline has not passed. **Live
OpenCode-in-container inference remains NOT_TESTED**; the fixture path is what
the deterministic E2E exercises. A successful live OpenCode container journey
is part of the pending paid UAT, not a claim of this document.
Containerized Codex stdio is not implemented.

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

## Optional private Tailscale access

The base Compose command stays loopback-only and does not start Tailscale. To
serve Office over private HTTPS from another tailnet device, first identify the
exact MagicDNS HTTPS origin for the chosen tailnet. The default node name is
`factory-office`; `FACTORY_TAILSCALE_HOSTNAME` can change it. The `FACTORY_TRUSTED_ORIGINS` value must be
an exact origin such as `https://factory-office.<tailnet>.ts.net`.

Create or update a local `.env` file (do not commit it) with the exact origin:

```dotenv
FACTORY_TRUSTED_ORIGINS=https://factory-office.<tailnet>.ts.net
FACTORY_TAILSCALE_HOSTNAME=factory-office
```

Then start the optional overlay:

```sh
podman compose -f compose.yaml -f compose.tailscale.yaml up -d --build
```

On first start, inspect only the Tailscale service logs for its account approval
URL and complete the one-time browser approval:

```sh
podman compose -f compose.yaml -f compose.tailscale.yaml logs -f tailscale
```

Before HTTPS Serve can issue a certificate, enable MagicDNS and HTTPS
certificates on the tailnet's **DNS** page in the Tailscale administration
console and accept its disclosure. Certificate Transparency makes the selected
machine name and tailnet DNS name publicly visible; choose a non-sensitive
machine name before enabling it. Once the node is authenticated and Serve is
active, open the exact HTTPS origin from a phone with the Tailscale app
connected to the same tailnet. Core still requires a separately enrolled
Factory device session; tailnet membership alone grants no Factory access.

The overlay uses the pinned official Tailscale image, userspace networking,
a persistent `factory-tailscale-state` volume, and private Tailscale Serve on
HTTPS 443. It adds an internal network shared only by Core and Tailscale plus a
separate uplink network containing only Tailscale. It does not mount a host
runtime socket, join the Factory control/egress networks, publish its own host port,
configure Funnel, or pass Factory/provider credentials to Tailscale. Core
keeps its existing `127.0.0.1` publication for local access and recovery.
Tailscale Serve forwards only to Core's restricted Office listener on container
port 8768. The listener omits Worker, Supervisor, verifier, reviewer, and
Quality Gate role/control routes; the port is not published on the host.
The container exposes only Tailscale's sanitized `/healthz` process status on
its private network interfaces for Core's Access & Devices projection. Metrics
are disabled and the health port has no host publication; the network contains
only Core and Tailscale.

To disable the overlay, stop it with the same two Compose files. This keeps the
node identity volume:

```sh
podman compose -f compose.yaml -f compose.tailscale.yaml down
```

The node can be removed from the tailnet in the Tailscale admin console; remove
`factory-tailscale-state` only after revoking that node. Real mobile-network
access, authenticated HTTPS Serve and restart persistence remain **NOT_TESTED**
until candidate-bound Podman and cross-network evidence is recorded. Do not
infer them from Compose rendering or a browser mock.
