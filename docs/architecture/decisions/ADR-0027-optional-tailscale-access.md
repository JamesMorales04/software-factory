---
id: ADR-0027
status: Proposed
date: 2026-10-27
owner: product-owner
supersedes: null
amends: ADR-0024, ADR-0025
---
# ADR-0027: Optional private Tailscale access to Office

- **Status:** Proposed for developer-preview implementation. This ADR records
  the topology choice. Author-run evidence at candidate `e17eb80` covers an
  authenticated tailnet HTTPS path, the restricted Office boundary, Secure
  session cookies and node identity persistence across restart; independent
  acceptance and mobile-data UAT remain pending.
- **Security status:** `TRUSTED_LOCAL`. Tailscale supplies network
  connectivity only. Core remains the sole application authentication and
  authorization authority.

## Context

A workstation behind CGNAT needs private HTTPS access to Office from a phone on
mobile data without router changes, public port forwarding or a host-installed
application runtime. The default loopback-only Compose deployment must continue
to work without Tailscale.

## Decision

An opt-in `compose.tailscale.yaml` adds the official Tailscale container at
the reviewed multi-architecture digest
`sha256:ed6480b33c6eb67c9cbd203ed42b9fb57e361986725b6856def71b7a3d9e276c`
for official version 1.104.4. This patched release is required because the
Unix-socket Serve backend was added to the Tailscale boundary; v1.104.4 fixes
TS-2026-016 and TS-2026-017. The version and digest were verified from the
official OCI registry on 2026-10-29. See the
[official security bulletins](https://tailscale.com/security-bulletins) and
[v1.104.4 changelog](https://tailscale.com/changelog). It uses userspace
networking and a persistent named state volume. The container configuration
was also checked against [Tailscale container documentation](https://tailscale.com/docs/features/containers/docker/docker-params)
on 2026-10-29.
Initial login is interactive through the Tailscale approval URL; no reusable auth key is committed or required.

The placement analysis is recorded in the
[service placement assessment](../assessments/tailscale-service.md). The
Tailscale service joins one network only:

- `factory-tailscale-uplink` provides outbound tailnet connectivity and has no
  other Factory service members.

It shares no network with Core and cannot reach any Core TCP listener. Tailscale
Serve proxies HTTPS to Core's restricted Office listener through a dedicated
Unix domain socket volume (`factory-office-socket`) mounted read-only in the
Tailscale container. That listener serves Office assets and an explicit
operator/auth route allowlist; Worker, Supervisor, verifier, reviewer, and
Quality Gate authority endpoints are absent even when their role tokens are
supplied.

Core's full control listener binds only the `core-control` and `core-egress`
network aliases on `factory-control` and `factory-egress`. It has no listener
on the Tailscale uplink, no host networking and no shared network with the
Tailscale service. Core retains its existing loopback-only host publication for
local access and recovery; the Tailscale service publishes no host port. Serve
is configured for HTTPS port 443 only; Funnel is explicitly disabled. Core
receives the exact `FACTORY_TRUSTED_ORIGINS` value; wildcard `ts.net` origins
and incoming Tailscale identity headers are not trusted.

Because Core has no network path to Tailscale, it cannot probe the Tailscale
health endpoint. Core reports the configured private endpoint as `CONFIGURED`
and never claims that the Serve path is live; the operator confirms it by
opening the exact HTTPS origin.

The Tailscale container has no Core, provider, Worker, Supervisor, runtime,
reviewer or broker credentials. Its persistent state volume is its only
writable persistent mount. The static Serve configuration is mounted read-only from the repository and
contains no credentials.

## Consequences and open evidence

- The base installation remains local and does not start Tailscale.
- Enabling remote access requires an exact HTTPS origin and one-time Tailscale
  account approval. HTTPS certificate consent may also be required in the
  tailnet administration UI.
- A tailnet member still needs an independent Factory device session. Tailnet
  membership does not enroll or authorize an Office user.
- This container/network topology is packaging, not a new isolation claim.
- Rootless Podman networking, node identity persistence across restart, Serve
  HTTPS, Secure session cookies, forged Host/Origin rejection and negative
  control-route reachability passed in an author-run at candidate `e17eb80`.
  That run did not test direct reachability of the full Core port 8765 from the
  Tailscale network. The 2026-10-29 repair removes the shared network and
  records a real Podman probe from the Tailscale container: Core's control and
  egress addresses are unreachable, no port 8768 TCP listener exists, and the
  restricted Office socket is reachable only through the dedicated volume.
  Serve over the socket could not be exercised without a tailnet login in this
  slice, so the live HTTPS path remains `NOT_TESTED`. Mobile-network
  reachability from a phone, independent acceptance and deployment remain
  `USER_PENDING` or `NOT_TESTED`. Funnel remains explicitly disabled and is not
  an offered capability.

## Alternatives considered

1. **Publish Office on a LAN or public interface:** rejected because it creates
   direct network exposure and does not solve CGNAT safely.
2. **Share a network with Core:** rejected because network reachability cannot
   be limited to one TCP port when Core also exposes its full control API.
3. **Expose the Office listener on a dedicated TCP network:** rejected because a
   shared bridge still let every Core address be probed and made host port
   forwarding depend on interface selection. The Unix socket gives Tailscale no
   IP route to Core at all.
4. **Trust Tailscale identity headers:** rejected because the application does
   not verify their cryptographic provenance; Tailscale connectivity does not
   replace Core sessions.
5. **Kernel TUN networking:** deferred because userspace networking avoids
   `/dev/net/tun` and additional capabilities for this reverse-proxy use case.
