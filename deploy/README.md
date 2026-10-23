# Core runtime container (Linux rootless Podman)

`Containerfile.core` builds a minimal application-runtime image for
`factory-core`. The image contains the Rust binary, its runtime libraries and
CA certificates: no Rust toolchain, Go, Node or build tools, and no
credentials.

```sh
podman build --network=host -t localhost/software-factory-core:dev \
  -f deploy/Containerfile.core .
factory start
factory status
factory logs
factory stop          # keeps the data volume
factory stop --purge  # prompts before deleting the data volume
```

`factory start` requires rootless Podman and an existing image. It creates or
reuses the named data volume, publishes `127.0.0.1:<FACTORY_CORE_URL port>` to
container port `8765`, and waits for `/health`. The container runs with a
read-only root filesystem, a `/tmp` tmpfs, all capabilities dropped,
`no-new-privileges`, a non-root user, Podman init for signal forwarding, and
no host network or Podman socket.

## Runtime configuration

| Variable | Source | Purpose |
|---|---|---|
| `FACTORY_DATABASE` | image default `/data/factory.sqlite` | SQLite path on the data volume |
| `FACTORY_BIND` | CLI `0.0.0.0:8765` | Container interface; host publication stays loopback |
| `FACTORY_CONTAINER_MODE` | CLI `1` | Container-mode opt-in; see blocker below |
| `FACTORY_OPERATOR_TOKEN` | token env file | Operator API credential |
| `FACTORY_WORKER_BOOTSTRAP_TOKEN` | token env file | Worker registration credential |
| `FACTORY_VERIFIER_TOKEN` | token env file | Verifier credential |
| `FACTORY_REVIEWER_TOKEN` | token env file | Reviewer credential |

The CLI generates four distinct role tokens into a mode `0600` env file
(`$XDG_CONFIG_HOME/factory/core.env` by default, or `FACTORY_ENV_FILE`) and
passes it with `--env-file`. It prints only the file path, never the tokens.

## Known blocker

The current `factory-core` rejects any non-loopback `FACTORY_BIND`, so the
container exits before serving. `FACTORY_CONTAINER_MODE` support is being added
by the main implementer; until it lands, `factory start` cannot complete the
health check. End-to-end container start is therefore not working yet.

## Security boundaries

Rootless Podman is a candidate runtime, not certified isolation. This image is
a trusted application-runtime package, not an untrusted execution sandbox. It
does not certify credential confinement, and it provides no Windows or Podman
Machine support claim.
