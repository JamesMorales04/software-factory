# Deployment images

**Primary product path:** the repository-root [`compose.yaml`](../compose.yaml) built by
[`Containerfile`](Containerfile). See [Compose deployment](../docs/operations/compose-deployment.md).

```sh
PODMAN_COMPOSE_PROVIDER=/usr/bin/podman-compose podman compose up -d --build
```

The unified Containerfile builds Core, compose-init, Supervisor, Worker,
verifier, reviewer, runtime broker, provider bridges and the fixture provider in
one shared Rust builder stage, then produces minimal runtime targets selected
per Compose service. The provider target pins OpenCode `1.18.32` and Codex
`0.156.1`; the runtime target packages a private nested Podman engine.

## Legacy host-development image

[`Containerfile.core`](Containerfile.core) builds the older Core-only image used
by the host-managed `factory start` development path. It is retained for that
developer convenience and is not the container-native product package. The old
service-specific `Containerfile.*` files are historical packaging experiments;
Compose does not use them.

No image contains operator or provider credentials. The runtime contract for
the legacy image remains documented in the file header and in
[Compose deployment](../docs/operations/compose-deployment.md) for the current
stack. Rootless Podman is a candidate runtime, not certified isolation.
