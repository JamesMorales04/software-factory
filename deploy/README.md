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

When `FACTORY_PROJECT_ROOTS` lists explicit absolute Git repository roots,
`factory start` mounts each root read-only at the same path in Core, starts the
host Supervisor and Worker with a private runtime directory, and enables the
fixture execution path. `FACTORY_SUPERVISOR_URL` defaults to
`http://127.0.0.1:8766`. The Supervisor holds the Podman boundary; no Podman
socket enters a workload. `factory stop` stops the Worker and Supervisor before
the Core container.

## Runtime configuration

| Variable | Source | Purpose |
|---|---|---|
| `FACTORY_DATABASE` | image default `/data/factory.sqlite` | SQLite path on the data volume |
| `FACTORY_BIND` | CLI `0.0.0.0:8765` | Container interface; host publication stays loopback |
| `FACTORY_CONTAINER_MODE` | CLI `1` | Container-mode opt-in |
| `FACTORY_OPERATOR_TOKEN` | token env file | Operator API credential |
| `FACTORY_ATTEMPT_GRANT_KEY` | token env file | Core-signed Supervisor allocation grant key |
| `FACTORY_WORKER_BOOTSTRAP_TOKEN` | token env file | Worker registration credential |
| `FACTORY_VERIFIER_TOKEN` | token env file | Verifier credential |
| `FACTORY_REVIEWER_TOKEN` | token env file | Reviewer credential |
| `FACTORY_PROJECT_ROOTS` | host CLI | Explicit read-only project roots; enables managed host execution |
| `FACTORY_SUPERVISOR_URL` | host CLI | Loopback Supervisor origin |
| `FACTORY_SUPERVISOR_IMAGES` | host CLI | Exact fixture image allowlist |
| `FACTORY_WORKER_FIXTURE_IMAGE` | host CLI | Fixture image passed to the Supervisor |

The CLI generates six distinct secrets (four role tokens and two Supervisor/ Core keys) into a mode `0600` env file
(`$XDG_CONFIG_HOME/factory/core.env` by default, or `FACTORY_ENV_FILE`) and
passes it with `--env-file`. It prints only the file path, never the tokens.

## Verified runtime

On the tested CachyOS/rootless-Podman host, `factory start` served the Rust
Office, exposed only loopback, preserved SQLite state across restart, and
executed an authorized fixture Campaign through the managed Supervisor/Worker
to an accepted candidate. `factory status` reports Core health and host service
state. Windows/Podman Machine and production deployment are not verified.

## Security boundaries

Rootless Podman is a candidate runtime, not certified isolation. This image is
a trusted application-runtime package, not an untrusted execution sandbox. It
does not certify credential confinement, and it provides no Windows or Podman
Machine support claim.
