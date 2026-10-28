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
  the topology choice; the optional path remains `NOT_TESTED` against an
  authenticated tailnet and is not independently accepted.
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
`sha256:2667499ed87ae29218f292556ba062918402dd5e92e93637af14867e4df12dd3`
for official version 1.102.4. It uses userspace networking and a
persistent named state volume. The official image and runtime parameters were
checked against [Tailscale container configuration documentation](https://tailscale.com/docs/features/containers/docker/docker-params) and the [official image tag listing](https://hub.docker.com/r/tailscale/tailscale/tags) on 2026-10-27.
Initial login is interactive through the Tailscale approval URL; no reusable auth key is committed or required.

The placement analysis is recorded in the
[service placement assessment](../assessments/tailscale-service.md). The
Tailscale service joins two dedicated networks only:

- `factory-office-private` is internal and contains only Core and Tailscale.
  Tailscale Serve proxies HTTPS to Core's Office/API listener on this network.
- `factory-tailscale-uplink` provides outbound tailnet connectivity and has no
  other Factory service members.

It does not join `factory-control` or the existing `factory-egress` network.
Core retains its existing loopback-only host publication for local access and
recovery; the Tailscale service publishes no host port. Serve is configured
for HTTPS port 443 only; Funnel is explicitly disabled. Core receives the exact
`FACTORY_TRUSTED_ORIGINS` value; wildcard
`ts.net` origins and incoming Tailscale identity headers are not trusted.

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
- Rootless Podman networking, persistence across restart, Serve behavior,
  mobile-network reachability, absence of public Funnel exposure, and negative
  unauthorized-service reachability require candidate-bound real evidence.
  Until then those items are `NOT_TESTED`.

## Alternatives considered

1. **Publish Office on a LAN or public interface:** rejected because it creates
   direct network exposure and does not solve CGNAT safely.
2. **Join Tailscale to `factory-control` or `factory-egress`:** rejected because
   it would expose control-plane peers beyond Office.
3. **Trust Tailscale identity headers:** rejected because the application does
   not verify their cryptographic provenance; Tailscale connectivity does not
   replace Core sessions.
4. **Kernel TUN networking:** deferred because userspace networking avoids
   `/dev/net/tun` and additional capabilities for this reverse-proxy use case.
